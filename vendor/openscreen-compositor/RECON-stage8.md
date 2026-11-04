# Stage 8 recon — MCP server + CLI for `record`

Read-only; nothing in the clone was executed. Prefixes: `C/` = `<scratchpad>/openscreen-src/new/` (untrusted
upstream clone), `IR/` = repo root `/Users/martin/claude/inspector-rust`, `S/` = `IR/core/rust-lib/src/`,
`R/` = `S/record/`. Where docs and code disagree, code wins.

## 0. Summary

1. **Upstream MCP is Electron code** in `C/electron/mcp/` (server `openscreen-mcp-server.ts`, stdio entry
   `stdio-server.ts`, media tools `media-tools.ts`, project tools `project-tools.ts`, app channel `app-channel.ts`).
   Two transports over ONE tool set: in-app **HTTP** `127.0.0.1:<port>/mcp` (Bearer token, Host/Origin checks,
   off by default, "MCP edits" switch) and **stdio** (`openscreen mcp`, the app's own executable with a subcommand,
   `C/website/docs/cli.md` "openscreen mcp"). `C/glama.json` is only a directory listing (maintainer field).
2. **stdio forwards to the running app** over a Unix socket / named pipe announced in `<userData>/mcp-app.json`
   `{pid, endpoint, token}` (`app-channel.ts:1-30`); the channel is itself MCP (`tools/call`). With no app, the same
   tools run locally on the project files (`stdio-server.ts:3-9`). Reason: the app must stay the ONE writer of a project
   open in its editor (`app-channel.ts:1-3`, "#1174").
3. **Tool surface ≈ 55 tools**: 36 document tools (`OPENSCREEN_TOOL_NAMES`, `C/electron/ai-edition/agent-tools.ts:1126-1166`
   + 6 caption tools `C/electron/ai-edition/caption-tools.ts:97-104`), 3 project tools, `transcribe`, 2 checkpoint tools,
   `listProjects`, and 8 media tools (`listSources startRecording stopRecording setPointerTrack exportVideo exportAudio
   getFrames createProject`, `media-tools.ts:642-1097`). Plus one MCP resource (effects-contract). Out of scope for us:
   the 4 shader-effect tools (`addEffect setEffect setEffectDefinition getEffect`) and `effects-contract` resource.
4. **Upstream CLI** (`C/electron/cli/*`, docs `C/website/docs/cli.md`): `record sources export captions pack info mcp help`.
   Runs inside Electron (no window), `--json` = NDJSON, exit codes 0/1/2, CLI does NOT take the single-instance lock
   (`cli.md` "CLI runs do not take the app's single-instance lock"; `C/electron/main.ts:214`).
5. **IR already has a surprisingly complete headless core**: `R/editor.rs`, `scene_json.rs`, `project.rs`, `presets.rs`,
   `zoom_plan.rs`, `captions.rs`, `audio_tracks.rs`, `peaks.rs`, `render_client.rs` import NO Tauri (grep: 0 `tauri::`/`AppHandle`),
   and `RenderClient::spawn_with(path, env, sink)` (`R/render_client.rs:284`) needs no AppHandle. Only the *session glue*
   is Tauri-bound: `editor_ipc.rs` (static `SESSION`, `ensure_client(app)`, `AssetRoot::resolve(app)` `R/assets.rs:76`,
   `export(app,…)` `editor_ipc.rs:2140`, `transcribe_project(app,…)` `:1836`) and `session.rs` recording (55 `tauri` hits).
6. **A headless recording path exists but is `#[cfg(test)]`**: `record_headless` (`R/session.rs:1897-1930`) calls
   `prepare_project → begin_capture(None, …) → finish` with `app = None`. A non-test variant is a small refactor; the real
   gaps are *source enumeration by name* (only `Picked::display(id)` / picker exist, `R/picker_macos.rs:51`) and TCC (§4).
7. **No existing MCP server in IR or workspace.** `/Users/martin/claude/_mcp/` holds only `bambu-a1-mini` (docs/models, it is a
   *config* for the third-party `@rowbotik/bambu-printer-mcp` via npx; `~/.claude.json` mcpServers = `pigeon` (node
   `/Users/martin/claude/pigeon/server/dist/index.js`) + `bambu`). So the user's MCP pattern is **Node/TS SDK servers**;
   no Rust `rmcp`/MCP crate in `IR/Cargo.lock` (only `tokio`, `schemars` present). IR's helpers speak hand-rolled JSON-lines
   (`transcribe-protocol`, `record-render-protocol`), so a **hand-rolled minimal MCP (JSON-RPC 2.0 over stdio, ~300 LOC)** fits
   house style better than adding `rmcp`.
8. **Single-instance forwarding is fire-and-forget** (`S/lib.rs:178-183`: callback gets `argv`, calls
   `cli_dispatch::dispatch`; the second process exits immediately). It cannot return stdout/exit codes. So a CLI that
   needs `--json` progress and exit status cannot ride only on it → needs a local socket (same as upstream's app channel)
   or a standalone binary.
9. **Recommendation (§5): option (a+c hybrid)** — a small separate binary `ir-record` (CLI) that is also the MCP stdio server
   (`ir-record mcp`), linking the pure record modules for no-app work and exports, and talking to a **local control socket of the
   running app** for everything that needs the app (project open in editor, recording, live edits). Recording always goes through
   the app (TCC + HUD + session state). 6 tasks in §6.

## 1. Upstream MCP

### 1.1 Location / transport / reach
- Server factory `createOpenScreenMcpServer(deps, runTool, media)` `C/electron/mcp/openscreen-mcp-server.ts:~860-960`; name
  `"openscreen"` (`:100`), endpoint `/mcp` (`:101`).
- **HTTP** `startMcpHttpServer` (`:963-1053`): `127.0.0.1` only, Host allow-list (`isAllowedHost`), Origin check (browser
  always sends one), `Authorization: Bearer <token>` compared with `timingSafeEqual` (`:~936`), stateless (one server+transport
  per request, `enableJsonResponse`). Lifecycle `C/electron/mcp/mcp-controller.ts`: **off by default** ("a user who never turns it on
  never meets a Keychain prompt", `mcp-controller.ts:3-5`), settings enabled/port/allowEdits (`mcp-settings-store.ts`), token
  regenerate restarts the server.
- **stdio** `serveMcpOverStdio` (`C/electron/mcp/stdio-server.ts:23-77`): "No port and no token: the client launched this process
  itself, with the user's rights" → `editsAllowed: () => true`. Builds the same server, wraps `runTool` with `routeToApp(...)`.
- **App channel** (`C/electron/mcp/app-channel.ts`): app serves the same MCP server on a Unix socket / named pipe (`:1-30`);
  discovery file `<userData>/mcp-app.json` `{pid, endpoint, token}` (0600), stale if pid dead or endpoint silent; client sends the
  token line first; 3 s connect timeout, 90 s call timeout, 65 min for transcribe (`:44-52`). Calls over it "edit as
  `openscreen mcp` does, whatever the in-app server's switches say" (`:24-26`). If an older app lacks a tool, it runs locally.
- **Editor host** (`McpDocumentHost`, `openscreen-mcp-server.ts:114-145`): `snapshot()` returns `{document, revision}`;
  `apply(doc, expectedRevision)` → `applied|conflict|save-failed|no-live-document|no-editor|timeout`. Optimistic
  concurrency by **revision**. File-mode (`McpProjectStore.saveProjectIfUnchanged(doc, version)`, `:147-163`) returns null if the
  file changed since read and **rejects with `ProjectOpenInEditorError` when an editor holds it**; an edit that races the
  editor opening is refused with PROJECT_CHANGED_MESSAGE (`:~215`).

### 1.2 Tool list (name — purpose; I/O brief)
Inputs are zod objects, every project-scoped tool takes optional `projectId` (default = project open in editor; stdio without app
requires it, `NO_PROJECT_MESSAGES`, `:~210`). Outputs are one `text` content part (JSON string or message), errors `isError:true`
(`textResult`, `:344`). Annotations: `readOnlyHint = !mutating`, `destructiveHint` for `replaceTimeline removeTrim removeModifier
removeClip` + restore/removeMedia (`DESTRUCTIVE_TOOLS`, `:261-266`).

**Projects / checkpoints** (`mutating?`):
- `listProjects` — id, title, updatedAt, assetCount, `open` (held by editor). RO.
- `createCheckpoint` (RO, in-memory snapshot, last 20, `MAX_CHECKPOINTS` `:~190`) / `restoreCheckpoint{checkpointId}` (mutating, one
  undo step in editor; also restores pointer track).
- `createProject` (media tools, `media-tools.ts:1096`), `setProjectInfo` (rename), `importMedia{path,kind?,label?}`,
  `removeMedia{assetId}` (refused while used) — `project-tools.ts:60-80`. `importMedia` validates absolute path, extension list,
  `fs.stat().isFile()` (`mediaPathProblem`, `project-tools.ts:~89`).
**Document read tools**: `getCurrentDocument`, `getTranscript`, `getTranscriptWords`, `getCursorTrack`, `getCaptions`,
`getCaptionTranslationUnits`, (shader) `getEffect`.
**Document mutating tools** (`MUTATING_TOOL_NAMES`, `agent-tools.ts:1211-1245`): `setWordText removeWords` (transcript edits),
`addTrim addTrims setTrim setClipRange moveClip insertClip duplicateClip replaceTimeline removeTrim removeModifier removeClip`
(clips/cuts), `addZoom addZooms addAutoZooms setZoom`, `addSpeed setSpeed`, `addAnnotation setAnnotation`,
`addLayoutChange setLayoutPreset moveLayoutBoundary addCameraFullscreen setCameraFullscreen` (camera layouts), `addAudio setAudio`,
`setAspectRatio`, `setEditorSettings` (look/cursor/camera settings), captions: `setCaptionSettings putCaptionTranslation
removeCaptionTranslation setVocabulary`.
**Out of scope for IR (agent-written WGSL, mark WGSL)**: `addEffect`, `setEffect`, `setEffectDefinition`, `getEffect`,
resource `effects-contract` (`openscreen-mcp-server.ts:~945-960`, `C/electron/mcp/effects-contract.ts`). Also AI-chat-only
paths in `C/electron/ai-edition/{chat-service,deep-agent}` are not MCP and stay out.
**Speech**: `transcribe{assetId?, language?(iso|"auto")}` (`:~120-140`) — runs on-device Whisper, stores transcript; waits for an
editor job of its own; no-audio ⇒ failure kind `no-audio` remembered on the asset.
**Media** (`media-tools.ts`):
- `listSources{}` → `{displays:[{index,name}], windows:[name], microphones:[label], cameras:[label], *LabelsUnavailable}` (`:642`).
- `startRecording{window?,display?,systemAudio?,microphone?:bool|string,camera?:bool|string,cursor?:"editable-overlay"|"system"}`
  (`:669`); returns after capture started. `stopRecording` (`:741`) → project/take info + a **checkpoint** + auto zooms planned
  from clicks (`autoZoomsAdded`). `setPointerTrack` (`:769`): script-driven takes provide a synthetic pointer path.
- `exportVideo{projectId, path(.mp4|.gif, absolute for http), quality?, fps?, gifFrameRate?, gifSize?, gifLoop?}` (`:794`) —
  validates absolute path, extension, MP4-vs-GIF option misuse (`:847`), refuses while a take is recording or another export runs,
  gated by edits switch; returns `{path,format,width,height}`; remembers `lastExports` for getFrames.
- `exportAudio{projectId,path(.wav|.m4a)}` (`:916`); `getFrames{projectId? | path?, atSec[]|count}` (`:980`, max `MAX_FRAMES`) —
  returns image content parts, ruler-seconds mapped through cuts/speed.
**Gating**: all mutating + record/export/createProject refuse with GATED_MESSAGE when `editsAllowed()` is false (HTTP only; stdio
always true). `consentRequired(name,args)` (`agent-tools.ts:2031-2033`).

### 1.3 Validation & safety model
- Every document tool re-parses args with zod `safeParse` and returns an `{error}` JSON instead of throwing (`setProjectInfo`
  pattern, `project-tools.ts:~95-110`; executor switch `agent-tools.ts:2035+`). Ranges are the editor's (zoom depth 1-6, etc.).
- **No path sandbox for exports/imports on stdio** (relative paths resolved against server cwd, `clientPath`, `media-tools.ts:284-287`);
  HTTP requires absolute paths. Only the *renderer* has a media-approval rule: media referenced by a loaded project is auto-approved
  only inside the recordings dir or the project file's folder (`cli.md` "Where media may live").
- **Edits to a non-open project need a checkpoint first** (CHECKPOINT_REQUIRED_MESSAGES, `:~225`) because Ctrl+Z can't reach file
  edits — a safety convention worth copying (cheap with our `.bak`/`doc` snapshot).
- Server `instructions` string embeds editing guidance + capability notes (`mcpInstructions`, `:~275-320`); `updateNotice` surfaces a
  newer version to agents.

## 2. Upstream CLI
- Source `C/electron/cli/{args,cliMain,projectCommands,runnerHost,sessionData,openGui,stdin}.ts`; renderer runners
  `C/src/cli/Cli*Runner.tsx` (hidden window does record/export/captions in the renderer; the compositor is native).
- Commands (all documented `C/website/docs/cli.md`): `record` (`--display --window --mic --mic-device --system-audio --camera
  --camera-device --cursor --duration --project --json`), `sources` (`-o file`), `export <proj> [-o --format mp4|gif|wav|m4a --track
  --quality --fps --gif-fps --gif-size --gif-loop --auto-zoom --audio --audio-mode --audio-offset --json]`, `captions <proj>
  --min-words --max-words`, `mcp`, `pack <proj> --out dir`, `info <proj> [--json]`, `help`.
- **Headless**: runs Electron with no visible window; Chromium switches before the subcommand are skipped; needs a display server.
  Export captures nothing (Vulkan/Metal compositor); record needs a real desktop session and TCC (`cli.md` "Per platform").
- **Output**: `--json` ⇒ NDJSON events `started|log|progress|stopping|warning|error|done` (`cli.md` "Machine-readable output");
  progress `{percentage,currentFrame,totalFrames,estimatedTimeRemaining}`; `-o file` for `sources` because wrappers (xvfb-run)
  merge stderr into stdout.
- **Exit codes**: 0 ok, 1 failure (also `info` on missing media), 2 bad args (plain text on stderr even with `--json`);
  `args.ts` arg errors → `app.exit(2)` (`cliMain.ts:223-228`), failures `exit(1)` (`:263-302`, `:595`).
- **Stop semantics (record)**: SIGINT/SIGTERM/stdin `stop|q|quit`/`--duration`; closing stdin does NOT stop. No cancel for export.
- Not single-instance-locked: works while the GUI app is open (but then two writers — upstream's `projectCommands` act on files).

## 3. IR today

### 3.1 CLI plumbing
- `S/cli_dispatch.rs:12-25` enum `CliAction {TogglePopup,Ocr,Screenshot,PickColor,RetroToggle,RetroPreset(String),SetupShortcuts}`;
  `parse_args` (`:28-52`) returns the **first** recognised flag; unknown flags only warn. `dispatch(app, action)` (`:~94+`) fires
  actions on worker/main threads, no result channel. Help text `print_help` (`:~64`).
- Wiring: `S/lib.rs:178-183` single-instance callback → `parse_args(argv)` → `dispatch`; also at first start `lib.rs:585-587`.
  `exit_if_help_requested` at `lib.rs:157`. Positional args and per-command flags (paths, `--json`) are **not supported** by this
  parser — it is a flag→enum map.
- Therefore record commands need either (i) extending `CliAction` with argument-carrying variants for *fire-and-forget* commands
  (`record start/stop/pause`, `open <project>`), and (ii) a result-bearing channel for `export`/`info`/`list` (§4).

### 3.2 Record APIs (pure vs. app-bound)
**Pure / headless-ready** (no Tauri):
- `R/project.rs`: `read_project(dir)` `:229`, `list_projects(root)` `:345` (`ProjectSummary` incl. thumb data-URI), `rename_project`
  `:321`, `is_project_dir` `:338`, `write_atomic` `:215` (tmp+rename via `safe_fs`), `clean_title` `:305`.
- `R/session.rs` free fns: `list_items` `:425`, `mark_interrupted` `:403`, `resolve_root` `:338` (setting → default
  `~/Movies/Inspector Rust`; setting lives in DB `record.*`, `RecordConfig::load(db)` `:256`), `project_dir_inside_root` `:92`,
  `project_dir_for_delete` `:150`, `import_take_into` `:521`.
- `R/editor.rs` (3.5k lines, pure): `load/load_with/store` `:1078-1098`, `default_doc`, `normalize` `:1343`, `move_clip`, `duplicate_clip`,
  `append_take`, programme↔source mapping `:1828-1887`, `output_size` `:2066`, `view` `:1897`. These are the *document ops* to expose
  as MCP tools. There are NO named high-level ops like `add_zoom(doc, …)`; edits are "mutate EditDoc fields then `normalize`"
  (UI sends the whole doc, `editor_ipc::update(app, session_id, doc)` `:1407`). MCP therefore needs thin Rust wrappers per tool
  (build struct, push into `doc.zooms`, `normalize`) — ~15-25 LOC each; `normalize` is the single validation gate (clamps, re-indexes).
- `R/editor_ipc.rs` pure helpers: `with_auto_zooms` `:941` (auto-zoom on any doc+cursor), `doc_for_open` `:959`, `export_plan` `:441`
  (validates fps/gif width presets → `ERR_BAD_EXPORT`), `export_path` `:337` (`<Downloads>/<title> <date>.<ext>`, never overwrites),
  `session_view` `:916`, `transcribe_plan` `:1660`, `to_transcript` `:1605`, `audio_source` `:1577`, `open_target` `:1301`.
- `R/scene_json.rs`: `scene_json_with(dir, project, doc, out, fps, assets, cursor, takes)` (used at `editor_ipc.rs:809`) → scene Value for the
  helper. `R/presets.rs` (list/save/apply on files under `<root>/Presets`, `:` pure). `R/captions.rs` `caption_entries` pure.
- `R/render_client.rs`: `RenderClient::spawn_with(path, env, sink)` `:284`, `.open(scene,w,h)` `:404`, `.export(scene, ExportKind, out)` `:495`
  (blocks; progress through `sink` events), `.cancel_export()` `:513`, `.thumbs(...)` `:453` (frames for `getFrames`!), `helper_path()` `:231`
  (finds `ir-record-render` next to the exe / dev path), `helper_env`/`dev_env` `:255-269`. Sink type = callback of helper events
  (progress). `ExportKind::{Mp4{w,h,fps,codec},Gif{…}}`. No AppHandle needed except `AssetRoot::resolve(app)` for the fonts dir
  (`assets.rs:76`) — headless alternative: `AssetRoot` from the executable's `Resources/` (small new constructor).
**App-bound (Tauri windows/state):** `editor_ipc::{open_editor, update, export, auto_zoom, transcribe_project, play, seek, voiceover_*,
preset_apply}` all take `&AppHandle` and operate on the static `SESSION` (`editor_ipc.rs:~966`, one editor at a time); `session::macos::{start,
stop,pause,resume,pick_source,delete_project,rename_project}` take `&AppHandle` (HUD, countdown, bar, toasts, sounds, picker).
`transcribe_client` (`S/transcribe_client.rs`) is **process-global statics** (no AppHandle): `send(&Command::TranscribeWords…)`,
`subscribe`, `request` → usable from any process that links core (spawns `ir-transcribe` found next to the exe); the editor wrapper
`transcribe_project` adds model-setting lookup (`app.state` DB) and emits UI events.

### 3.3 Concurrency on `project.json` (two-writer problem)
- One writer today: the editor session holds `doc` in memory and `save_session` writes the whole `project.json` (debounced 800 ms
  `SAVE_DEBOUNCE`, `editor_ipc.rs:40,1072-1096`, `write_atomic` temp+rename). Reads of `project.json` outside the session happen
  only at open. **No file lock, no revision/version field** in `Project` (grep: none) — a second process writing `project.json` while an
  editor session is open is silently overwritten by the next debounced save (and vice-versa); `rename` while open is routed through
  the session under its lock (`retitle_open` `:1314`) precisely for that reason.
- Guards IR has: `open_allowed/open_refusal` (no open while exporting `:1289-1296`), `editor_ipc::open_dir()` `:1360` tells which project
  is open **within the app process only**.
- Therefore: any MCP/CLI mutation of a project that the app has open **must go through the app process** (call into `editor_ipc::update`
  via the control channel, giving an undo-able live edit like upstream), or be refused with an actionable error. A standalone process can
  detect "app running + project open" only through the channel (or a lockfile written by the app, see Task 1).

### 3.4 TCC / bundle facts
- `ir-transcribe`, `ir-record-render`, `ir-ocr` are plain executables in `Contents/MacOS` / `Contents/Resources` spawned by the app
  (`transcribe_client::helper_path`, `render_client::helper_path`); per `IR/CLAUDE.md` ("TCC attributes mic/audio-capture to the app
  bundle") the grant belongs to the app because the app is the (launchd/LaunchServices-started) responsible process. The camera/mic
  usage strings are injected into the bundle's Info.plist (`scripts/install-macos.sh:453`).
- A process started by an *agent* (Claude Code → `ir-record mcp`) has the agent/terminal as responsible process; ScreenCaptureKit/mic would be
  prompted for / denied against **that** parent (upstream says the same: "for a development build started from a terminal, grant it to the
  terminal", `cli.md` record/macOS). I could not verify responsibility-inheritance rules for children of a bundle binary launched from a
  terminal from the repo alone → **treat as: recording in a standalone process = TCC lands on the parent; do not rely on it.**
- `main.rs` of platform shells is 2 lines (`macos/src-tauri`); extra binaries in the workspace are separate crates bundled by
  `scripts/bundle-*.sh` (`bundle-transcribe.sh`, `bundle-record-render.sh`) before signing — adding one more helper follows an existing recipe.
- `Cargo.toml` workspace members: `core/rust-lib` (lib, depends on `tauri`), helpers `ocr-helper`, `record-render`, `transcribe-helper`
  (+ protocol/core crates). A new bin that links `inspector_rust_core` pulls the whole Tauri dep tree (acceptable for size? binary
  would be tens of MB; no window code runs). A cleaner split would extract the pure record modules into a `record-core` crate; that
  is a large mechanical move (editor.rs imports only `crate::record::*`, so feasible but touches 25k lines of paths) — not recommended in
  stage 8.

## 4. Design options

| | (a) separate binary `ir-record` (+ `mcp`) | (b) app mode `inspector-rust --mcp/--cli` | (c) forward to running app via socket |
|---|---|---|---|
| Works without app | yes: list/info/doc edit (file)/export/getFrames/pack/transcribe (links core + spawns `ir-record-render`, `ir-transcribe`) | yes but boots Tauri builder; must skip single-instance plugin (else it forwards argv and exits, `lib.rs:178`) and tray/hotkeys/DB init (`run()` does all in `setup`) → invasive early branch | **no** (needs app) |
| Live edit of a project open in editor | no (would create 2nd writer) → refuse/forward | would be the same process only if it IS the app | **yes**, one writer, undo-able |
| Recording | TCC on parent (§3.4) — unreliable | same problem when launched by agent; fine if launched by the user | **yes**, app's own grants/HUD |
| Bundle/sign | extra bin, follows `bundle-record-render.sh` recipe, signed with app | none | app side adds a socket server |
| Complexity | medium | medium-high, risk to app startup | medium (server ~200 LOC) |

- **Concurrency**: only (c) can edit an open project safely; (a) must detect and delegate/refuse. Detection: the app writes
  `<data dir>/control.json {pid, endpoint, token}` (0600) while running (copy of upstream `mcp-app.json`, `app-channel.ts:1-30`) and
  records the open project path inside `status`; client treats dead pid as stale.
- **Transcription**: both processes can drive `ir-transcribe`, but `transcribe_client::is_busy`/`FILE_BUSY` are per-process statics; the app's
  helper and a standalone helper would run two Whisper models concurrently (1-2 GB each). Mitigation: when the app is up, delegate.
- **Export**: `RenderClient` per process; a standalone export is independent of the app's helper (fine, but heavy → delegate when app up to
  honour the "one export at a time" rule, `open_allowed`).

**Recommendation: hybrid (a)+(c).** New crate `record-cli` (binary `ir-record`, one executable for both CLI and MCP: `ir-record mcp`
= stdio server). Core idea = upstream's architecture, re-expressed in IR terms:
1. Command/tool *definitions and arg validation* live once (`record-cli::ops`), executed by one of two backends:
   - **Local backend** (no app): file-based on `project.json` using the pure modules + `RenderClient` + `transcribe_client`.
   - **App backend**: JSON-RPC over a Unix socket served by the app (`control.rs`, new), calling into `editor_ipc`/`session::macos`.
   Selection: app reachable ⇒ app backend for everything that touches the open project, recording, export, transcribe; else local.
2. Recording tools exist only on the app backend; when the app is not running, `ir-record record` / MCP `startRecording` launch it with
   `open -a "Inspector Rust"` (LaunchServices ⇒ app is its own responsible process) and wait for `control.json`, else fail with the message
   "start Inspector Rust". This makes TCC a non-issue.
3. Keep stdio only (no HTTP/token surface to defend; IR is a menu-bar tool, the user's MCP clients use stdio). The socket is the only
   new listener: 0600 unix socket in the data dir + token line (as upstream).
4. Safety copies: mutating file-mode edits on a project not open in the app write `project.json.bak` (single rolling) before the write;
   refuse (never merge) when the project dir contains a live `control.json`/lock claim by another pid for that project.
Why not (b): changes app boot, no benefit for TCC when an agent is the parent; why not pure (c): loses export/inspect/list in CI or when the
app is quit (upstream and IR's own `ir-record-render` are designed to run without the UI).

## 5. Proposed surface (IR names; no upstream-name compatibility needed)

### 5.1 MCP tools (stdio server `ir-record mcp`, server name `inspector-rust-record`)
Read-only: `record_projects` (list: id=dir name, title, updated, duration, status, open), `record_project` (info: recording meta, editor doc
summary, clips/zooms/annotations/tracks counts, media presence), `record_doc` (full `EditDoc` JSON + `EditorView` programme spans),
`record_transcript` (words/segments per take), `record_sources` (displays/windows/mics/cameras; app backend), `record_frames`
(`RenderClient::thumbs` → image content, `atProgrammeSec[]`).
Mutating (all return the new `EditorView` summary; all pass through `editor::normalize`; project-scoped, optional `project`):
`record_clip` (op: split|delete|trim|move|duplicate|speed|crop), `record_zoom` (add|set|remove|auto), `record_annotation` (add|set|remove),
`record_audio` (add music/voiceover file, set gain/fades/mute, remove), `record_look` (appearance/cursor/aspect/preset apply),
`record_camera` (layout/preset/regions), `record_captions` (settings, transcribe+generate), `record_transcript_cut` (remove words
range → clip cuts, uses stage-6 `cutSourceRange` logic — port needed in Rust, today in TS `lib/transcript.ts`; check before promising),
`record_rename`, `record_import` (insert another take / media file), `record_checkpoint` / `record_restore`.
Heavy: `record_transcribe` (`TranscribeWords` via helper), `record_export` (mp4|gif; wav/m4a audio-only is **not** in IR export_plan
today → defer), `record_start`, `record_stop`, `record_pause`, `record_status`, `record_delete` (destructive, requires `confirm:true`).
Not offered: shader effects, device-frame WGSL, AI chat. Tool set stays ~25 (fewer, op-multiplexed tools are better for agents than
upstream's 55 one-per-verb; this is a deliberate departure).

### 5.2 CLI (`ir-record <cmd>`; also reachable as `inspector-rust --record …` via forwarding for fire-and-forget actions)
`list`, `info <project>`, `record start|stop|pause|resume|status [--display N|--window T] [--mic] [--system-audio] [--camera] [--cursor …]
[--duration S] [--title T] [--project-out DIR]`, `export <project> [-o file] [--format mp4|gif] [--quality …] [--fps] [--gif-width] [--auto-zoom] [--json]`,
`transcribe <project> [--lang de|en|auto]`, `captions <project> [--min-words --max-words]`, `auto-zoom <project>`, `preset list|apply|save`,
`rename`, `delete --yes`, `sources [--json]`, `mcp`. `--json` = NDJSON events (reuse upstream event vocabulary: started/progress/warning/error/done);
exit codes 0/1/2 as upstream (`cli.md`); `--help` plain. Fire-and-forget extras via `cli_dispatch`: `--record-start`, `--record-stop`
(hotkey-equivalent for Wayland-style custom shortcuts) — these need `CliAction` variants and argument parsing (`parse_args` only returns the first flag).

## 6. Task breakdown (proposed)

1. **Control channel + discovery (app side).** New `S/control.rs`: Unix socket (0600) + token line + `control.json {pid,endpoint,token,openProject}`;
   JSON-RPC methods wrapping `editor_ipc::{update,export,auto_zoom,transcribe_project,open_dir}` and `session::macos::{start,stop,pause,resume}`;
   register in `lib.rs` setup, remove on exit. Needs a decision on `EditorInit`/session semantics when no editor window is open (open session headless
   without window: `open_editor` builds a window — likely add a window-less `open_session(dir)`). Tests: protocol framing, token, stale file.
2. **Headless core extraction.** `AssetRoot::from_exe()` (no AppHandle), `RenderClient` spawn helper for a non-Tauri process, non-test
   `session::record_*` variant (`record_headless` minus `cfg(test)`, display by id) — the latter only for the app backend; define
   `ops` layer: pure functions `(project dir, args) -> Result<EditDoc/summary>` for each document op (wrapping `editor::*` + `normalize`),
   with golden tests against `fixtures/scene-golden*.json`. Mutation-test the validation (bad indices, overlapping zooms).
3. **`record-cli` crate / `ir-record` binary (CLI).** Arg parser (hand-rolled like the rest, or `clap` if already in lockfile — check),
   local+app backends, NDJSON/exit-code contract, project path/id resolution (reuse `project_dir_inside_root`, `open_target`), export with
   progress (RenderClient sink → NDJSON), `.bak` + app-open refusal logic. Bundle recipe (`scripts/bundle-record-cli.sh`, sign with app, copy next to
   `inspector-rust` so `helper_path()` lookups resolve).
4. **MCP stdio server.** Hand-rolled JSON-RPC (initialize, tools/list, tools/call, ping; image content for `record_frames`), schema JSON per tool
   from the same arg structs (serde + manual schema or `schemars` already in lockfile), tool annotations (readOnly/destructive), `instructions`
   string (checkpoint-first rule from upstream §1.3), notifications none. Tests: handshake transcript fixture, each tool's schema validity,
   refusal paths.
5. **Recording + transcription via app.** MCP/CLI `record_*` through the control channel; `open -a` launcher; sources listing (needs new
   `SCShareableContent` enumeration function — only the picker exists today, `picker_macos.rs`; `capture_macos.rs:593 shareable_content` is the
   building block); transcription delegating to the app when up, local `transcribe_client` otherwise.
6. **Docs/Settings/hardening.** `docs/record-cli.md` + `docs/record-mcp.md`, `features.txt` lines, CommandDoc? (not a search-bar command — Features-tab extras row),
   Settings → Recording "Command line & MCP" (copy-paste `claude mcp add` line, path of `ir-record`), i18n en/de, CHANGELOG, README matrix gate,
   Playwright-free e2e script (spawn `ir-record mcp`, run list/info/export against a fixture project). Live checks (display awake) for record+stop.

Open decisions for the user: (1) accept the hybrid with `open -a` launching the app for recording; (2) op-multiplexed tool names vs
upstream-style one-verb tools; (3) audio-only export (wav/m4a) — not in IR's export_plan today, add or skip; (4) whether MCP edits to an open
project should be undoable via the editor's history (needs `update` through the app = yes in the hybrid).
