# record — Stage 8: command line + MCP server — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drive `record` from a terminal (`ir-record …`) and from MCP clients (`ir-record mcp`, stdio): list and inspect projects, edit them (zooms, clips, look, captions…), transcribe, export, and start/stop recordings.

**Architecture:** The running app is the ONE place that touches projects. It serves a local control socket (`control.rs`, JSON lines, token). `ir-record` is a small client binary with no core dependency: it finds the socket via `control.json`, launches the app in the background when it is not running (`open -g -b io.celox.inspector-rust`), and maps CLI commands and MCP tools onto control methods. Edits run through the editor session (`editor_ipc`), so a project open in the editor window gets the change as one undo step, and there is never a second writer of `project.json`.

**Tech Stack:** Rust (core + new crate `record-cli`), Unix domain sockets (std), hand-rolled JSON-RPC/MCP (house style like `transcribe-protocol`), React for one Settings section and the editor's external-change hook.

**Spec:** `docs/superpowers/specs/2026-11-01-record-design.md` (goal 5, "Weiteres", stage 8). Recon: `vendor/openscreen-compositor/RECON-stage8.md` — §3 and §5 are required reading for every task.

## Global Constraints

- One writer: only the app process reads-modifies-writes `project.json`. `ir-record` never opens project files itself.
- Control socket: `<app data dir>/control.sock`, mode 0600; discovery file `<app data dir>/control.json` `{ "pid": u32, "socket": path, "token": hex32 }`, mode 0600, written at app start, removed on exit; a stale file (dead pid / refused connect) is ignored by the client. Every request carries the token; a wrong token gets error `control.bad_token` and the connection is closed.
- Wire format (both directions one JSON object per line): request `{"id":n,"token":"…","method":"…","params":{…}}`; reply `{"id":n,"result":…}` or `{"id":n,"error":{"code":"<sentinel>","message":"…"}}`; progress `{"id":n,"event":{…}}` lines before the reply of a long method.
- Error codes are stable sentinels (`record.*`, `control.*`) — CLI maps them to exit codes, MCP to `isError` tool results.
- CLI: `--json` = NDJSON on stdout (`{"type":"progress"|"warning"|"done"|"error",…}`); exit codes 0 = ok, 1 = failed, 2 = usage error. Human output otherwise; errors to stderr.
- MCP: stdio, JSON-RPC 2.0, protocol version `2025-06-18` (answer the client's version if it is one of `2024-11-05`, `2025-03-26`, `2025-06-18`), server name `inspector-rust-record`, tools only (no resources/prompts). Tool names `record_*`, op-multiplexed (≈ 20 tools). No shader-effect authoring tools (spec non-goal); the built-in effect regions are editable like any other region.
- Destructive operations (`delete`) need an explicit `confirm: true` (MCP) / `--yes` (CLI).
- Recording always runs inside the app (TCC grants belong to the app bundle). Apple Silicon only, like the rest of `record`; on other platforms the control methods answer `record.unsupported`.
- Edits to a project open in a visible editor window reach the window as one undo step. A control call targeting project B while the editor window shows project A is refused with `record.control.other_open`.
- Strings for the Settings section in en.json AND de.json. No new runtime dependency in `ir-record` beyond `serde`/`serde_json` (already in the workspace).
- Audio-only export is out of scope (IR's export has no audio-only format).

## Review Focus

1. **Stale discovery file** (app crashed, pid reused): the client must connect-and-ping, not trust the pid; a dead socket triggers the launch path. Test in Task 4.
2. **App not running and launch fails / app not installed**: the CLI must fail within its timeout (10 s) with one readable line and exit 1, never hang. Test in Task 4.
3. **A long export while the client disconnects** (Ctrl+C, MCP client quits): the export keeps running in the app or is cancelled — decide: it is CANCELLED when the requesting connection closes (no orphaned export). Test in Task 1/2.
4. **Editor window open on the same project** while a control edit lands: the window shows the change and ⌘Z undoes exactly that edit; a debounced save in flight must not overwrite it. Test in Task 2.
5. **Malformed MCP input** (unknown tool, wrong param types, huge numbers, negative indices): a tool error result, never a crash of `ir-record mcp`. Test in Task 5.

---

### Task 1: Control socket in the app

**Files:**
- Create: `core/rust-lib/src/control.rs` (server, discovery file, framing, dispatch table)
- Modify: `core/rust-lib/src/lib.rs` (start in setup, remove on `RunEvent::Exit`), `core/rust-lib/src/lib.rs` mod list
- Test: unit tests in `control.rs`

**Interfaces:**
- Produces: `pub fn start(app: AppHandle) -> std::io::Result<()>`; `pub fn stop()`; `pub type Handler = fn(&AppHandle, &serde_json::Value, &mut dyn FnMut(serde_json::Value)) -> Result<serde_json::Value, ControlError>` (third arg = emit a progress event); `pub struct ControlError { pub code: String, pub message: String }`; `pub fn register(method: &'static str, h: Handler)` (or a static table built in Task 2/3); built-in methods `control.ping` → `{"version": <app version>, "platform": "<os>"}`.
- Pure, tested: `parse_request(line) -> Result<Request, ControlError>`, `reply_line(id, result)`, `error_line(id, err)`, `event_line(id, ev)`, `token_ok(given, expected)` (constant-time), `discovery_json(pid, socket, token)`.

- [ ] **Step 1: Failing tests:** framing round trip; bad JSON → `control.bad_request` with id null; missing/wrong token → `control.bad_token`; unknown method → `control.unknown_method`; `discovery_json` shape; files are created 0600 (tempdir); `start` on a path where a stale socket file exists replaces it; a handler that panics answers `control.internal` and the server keeps serving (catch_unwind).
- [ ] **Step 2: Run** `cargo test -p inspector-rust-core --lib control::` — FAIL.
- [ ] **Step 3: Implement** with std `UnixListener` on a thread `ir-control`, one thread per connection, requests on one connection handled in order. A connection closing sets a per-request cancel flag that long handlers poll (pass it via the emit closure's context — define `pub struct Call<'a> { pub params: &'a Value, pub cancelled: &'a AtomicBool, emit: … }` if a closure is awkward; state the final shape in the report). Token = 32 random hex chars per app start.
- [ ] **Step 4: Run, mutation-check** the token check. Commit `feat(control): local control socket for the app`.

### Task 2: Record project methods (read, edit, export, transcribe)

**Files:**
- Create: `core/rust-lib/src/record/control_ops.rs`
- Modify: `core/rust-lib/src/record/editor_ipc.rs` (window-less session open; external-change event), `core/frontend/src/components/editor/RecordEditor.tsx` (+ test) for the external-change event
- Test: `control_ops.rs` unit tests (pure parts), `RecordEditor` test

**Interfaces:**
- Consumes: Task 1 `register`/`Handler`; `project::list_projects`, `session::resolve_root`, `editor_ipc::{open_editor, update, auto_zoom, transcribe_project, export, open_dir}` and the pure `editor::*` ops (recon §3.2).
- Produces control methods (params/result JSON, camelCase):
  - `record.projects` → `[ProjectSummary without thumb]`; `record.project {project}` → `{project: Project, view: EditorView, open: bool}`; `record.doc {project}` → `{doc, view}`; `record.transcript {project}` → transcripts.
  - `record.edit {project, ops:[…]}` → `{view}` — applies a list of ops to the doc in order, then ONE `update`; op kinds: `zoom.add|set|remove|auto`, `clip.split|delete|move|duplicate|speed|crop`, `annotation.add|set|remove`, `effect.add|set|remove`, `look.set` (appearance/cursor/output aspect/camera style/captions settings — partial JSON merged then normalised), `preset.apply`, `rename`. Region ops take ids (from `record.doc`); index/time params in source ms of a clip or programme ms (`atProgrammeMs`, mapped with `editor_to_source` in Rust).
  - `record.export {project, format:"mp4"|"gif", quality?, fps?, codec?, gifWidth?, out?}` → `{path}`, progress events `{phase,frame,total}`; cancelled when the connection closes.
  - `record.transcribe {project, lang}` → `{words, skipped}`, merged into the doc (one undo step if the window is open).
  - `record.frames {project, atProgrammeMs:[…], maxWidth}` → `[{atProgrammeMs, jpegBase64}]` (≤ 8 frames per call).
  - `record.delete {project, confirm:true}`, `record.rename {project, title}`.
  - `project` = project folder name (as listed) or absolute path; resolved by a pure `resolve_project(root, arg) -> Result<PathBuf, ControlError>` that only accepts folders inside the recordings root or an existing project dir (`open_target` rules).
- Session rule: a control call opens the target project as the editor session WITHOUT a window when no session/window exists (`open_session(app, dir)`, sharing `open_editor`'s code minus the window); same project already open → reuse; a different project open in a visible window → `record.control.other_open`; a different project open without a window → flush-save it and switch.
- External change: after a control edit/transcribe on a session whose window is open, emit `record-editor-external {sessionId, doc}`; `RecordEditor` replaces its doc as ONE undo step and cancels its pending debounced save of the older doc.

- [ ] **Step 1: Failing tests:** `resolve_project` (folder name, absolute path inside root, path outside root that is a project → ok, `../` escape → error, non-project → `record.editor.not_project`); each op kind applied by a pure `apply_ops(doc, ops, view_ctx) -> Result<EditDoc, ControlError>` (zoom add at programme ms lands in the right clip; unknown region id → `record.control.no_region`; out-of-range clip index → error, doc unchanged; ops are all-or-nothing); `look.set` partial merge keeps untouched fields; RecordEditor: an external event pushes one undo step and ⌘Z restores the previous doc.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** (`apply_ops` pure; handlers thin). Export of a window-less session uses the same `editor_ipc::export` path.
- [ ] **Step 4: Run** core + frontend tests; mutation-check all-or-nothing and the escape check. Commit `feat(record): control methods for projects, edits, export and transcription`.

### Task 3: Recording and sources over control

**Files:**
- Modify: `core/rust-lib/src/record/session.rs` (start with a given source, no picker), `core/rust-lib/src/record/picker_macos.rs` or `capture_macos.rs` (enumerate shareable displays/windows), `control_ops.rs`
- Test: pure parts in `session.rs` / `control_ops.rs`

**Interfaces:**
- Produces control methods: `record.sources` → `{displays:[{id,name,width,height,main}], windows:[{id,app,title}], mics:[{id,name}], cameras:[{id,name}]}`; `record.start {display?|window?|area?{x,y,w,h,display}, mic?, systemAudio?, camera?, countdown?:bool, title?}` → `{project}` (starts the normal session: bar shown, countdown when asked — default true for CLI, false for MCP); `record.stop` → `{project, ready}` (waits for finish); `record.pause` / `record.resume` / `record.status` → `RecordStatus` JSON.
- Source selection by id or by a name substring (pure `pick_source(list, query) -> Result<…>`; ambiguous match → `record.control.ambiguous` listing candidates).

- [ ] **Step 1: Failing tests:** `pick_source` (exact id, unique substring, ambiguous, none); `record.start` while recording → `record.busy` (existing sentinel if any — reuse); start options map onto the session's existing config struct (pure builder test).
- [ ] **Step 2–4:** implement, run, mutation-check ambiguity. Live check (display awake, `caffeinate -u -t 1` first): an `#[ignore]` test that calls the handlers to record 3 s of the main display and stop → project ready. Commit `feat(record): start and stop recordings over control`.

### Task 4: `ir-record` client + CLI

**Files:**
- Create: `record-cli/Cargo.toml` (package `ir-record-cli`, bin `ir-record`; deps serde, serde_json only), `record-cli/src/main.rs`, `record-cli/src/client.rs` (discovery, connect, launch, request/stream), `record-cli/src/cli.rs` (arg parsing + commands + output), workspace `Cargo.toml` member
- Create: `scripts/bundle-record-cli.sh` (build release, copy to `Contents/MacOS/ir-record` before signing); modify `scripts/install-macos.sh` and `scripts/pack-macos-release-dmg.sh` to call it (Apple Silicon only, like record-render)

**Interfaces:**
- Consumes: Task 1 wire format + discovery file; data dir = `~/Library/Application Support/InspectorRust/` (`IR_DATA_DIR` env override for tests).
- Produces CLI commands: `ir-record list`, `info <project>`, `doc <project>`, `sources`, `record [--display Q|--window Q] [--mic] [--system-audio] [--camera] [--no-countdown] [--title T] [--duration S]` (with `--duration`, stops by itself; without it, Ctrl+C stops), `stop`, `pause`, `resume`, `status`, `export <project> [--format mp4|gif] [--quality …] [--fps N] [--codec h264|hevc] [-o PATH]`, `transcribe <project> [--lang de|en|auto]`, `auto-zoom <project>`, `edit <project> --ops JSON|@file`, `rename <project> <title>`, `delete <project> --yes`, `mcp`, `--help`, `--version`. Global `--json`.
- Client: `pub fn connect(launch: bool, timeout: Duration) -> Result<Conn, ClientError>` (read `control.json` → connect → `control.ping`; on failure and `launch`, run `open -g -b io.celox.inspector-rust` (override `IR_RECORD_APP=/path/to/App.app`) and retry until timeout); `Conn::call(method, params, on_event) -> Result<Value, RemoteError>`.

- [ ] **Step 1: Failing tests:** arg parser (every command, unknown flag → exit 2 with usage line, `--ops @file`); exit-code mapping from sentinels; NDJSON formatting; client against an in-test fake server on a temp socket (`IR_DATA_DIR`): ping ok, wrong token → error, stale `control.json` with dead socket + `launch=false` → `ClientError::NotRunning` within the timeout, events streamed before the result, Ctrl+C/closing the connection mid-call doesn't panic.
- [ ] **Step 2–4:** implement, run `cargo test -p ir-record-cli`, mutation-check the stale-file path. Commit `feat(record): ir-record command line`.

### Task 5: MCP stdio server (`ir-record mcp`)

**Files:**
- Create: `record-cli/src/mcp.rs`, `record-cli/src/tools.rs` (tool table: name, description, JSON Schema, annotations, → control method + param mapping)
- Test: `record-cli/tests/mcp_transcript.rs` (fixture conversation against the fake server from Task 4)

**Interfaces:**
- Tools (input schemas mirror the control params of Tasks 2–3): `record_projects` (readOnly), `record_project` (readOnly), `record_doc` (readOnly), `record_transcript` (readOnly), `record_sources` (readOnly), `record_frames` (readOnly; returns `image` content items, `image/jpeg`), `record_edit` (ops list), `record_auto_zoom`, `record_transcribe`, `record_export`, `record_rename`, `record_delete` (destructiveHint; requires `confirm: true`), `record_start`, `record_stop`, `record_pause`, `record_resume`, `record_status`.
- `initialize` result includes `instructions`: one paragraph (read `record_doc` before editing; ids come from it; times are programme ms unless a field says source ms; one export at a time).
- Results: `content:[{type:"text", text:<pretty JSON>}]`, plus `structuredContent` with the same object; errors as `isError: true` with the sentinel + message.

- [ ] **Step 1: Failing tests:** handshake (`initialize` → `notifications/initialized` → `tools/list` → `tools/call`), version negotiation, every tool's `inputSchema` is valid JSON Schema object with `type: "object"`, unknown tool → JSON-RPC error `-32602`, wrong param types → tool error result, `ping`, EOF on stdin ends the server with exit 0, the app not running → tool error "Inspector Rust is not running and could not be started".
- [ ] **Step 2–4:** implement, run, mutation-check unknown-tool handling. Commit `feat(record): MCP server ir-record mcp`.

### Task 6: Settings, docs, live end-to-end (controller)

- [ ] Settings → Recording: "Command line & MCP" block — path of `ir-record` inside the installed bundle, a copyable `claude mcp add inspector-rust-record -- "<path>" mcp` line, a copyable `ln -s "<path>" ~/.local/bin/ir-record` line; en+de strings; test.
- [ ] `docs/record-cli.md` (commands, NDJSON, exit codes, MCP tools), README link (docs index both languages), `features.txt`, Features-tab extras row (`lib/feature-extras.ts`), CommandDoc tip on `record`, CLAUDE.md stage-8 section, CHANGELOG + version bump.
- [ ] Live: installed app → `ir-record list`, `info`, `export --json` of an existing project, `transcribe`, `record --duration 3 --no-countdown` + `stop`, and an MCP session via `claude mcp add` in a scratch config (or a scripted JSON-RPC transcript). `bash scripts/check.sh`, `pnpm test`, install, commit, push.
