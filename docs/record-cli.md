# Recorder command line and MCP (`ir-record`)

Stage 8 of `record` (macOS, Apple Silicon). `ir-record` sits next to the app binary in the bundle (`Inspector Rust.app/Contents/MacOS/ir-record`) and drives the screen recorder of the running app. `ir-record mcp` is the same client as an MCP server for AI clients.

## Setup

Settings → Recording → *Command line & MCP* shows the installed path and two lines to copy:

```
claude mcp add inspector-rust-record -- "<path>/ir-record" mcp
ln -s "<path>/ir-record" ~/.local/bin/ir-record
```

A development build has no `ir-record`; the block says so instead.

## How the app is found and started

The app writes a discovery file (`control.json`: socket, token) into its data directory (`$IR_DATA_DIR`, else `~/Library/Application Support/InspectorRust`). `ir-record` connects to the unix socket and sends `control.ping`; the pid in the file is ignored, so a stale file after a crash is harmless. If nobody answers, it starts the app in the background (`open -g -b io.celox.inspector-rust`, or `open -g "$IR_RECORD_APP"`) and retries for up to 10 s. `stop`, `pause`, `resume` and `status` never start the app. Failure to start: `Inspector Rust is not running and could not be started (...)`.

## Commands

`<project>` is a folder name from `list` or an absolute path.

| Command | Does |
|---|---|
| `list` (`ls`, `projects`) | all recordings, newest first |
| `info <p>` | project metadata and timeline view |
| `doc <p>` | the edit document and view (region ids, clip indices) |
| `sources` | displays, windows, microphones, cameras |
| `record [--display Q \| --window Q] [--[no-]mic] [--[no-]system-audio] [--[no-]camera] [--mic-device Q] [--camera-device Q] [--no-countdown] [--title T] [--duration S]` | start a take; waits for `--duration` or Ctrl+C, then stops and prints the project path. `Q` = id (number) or name part. Microphone, system sound and camera: `--x` on, `--no-x` off, neither = the app's setting. A window take records only that app's sound |
| `stop`, `pause`, `resume`, `status` | control the running take |
| `export <p> [--format mp4\|gif] [--quality p720\|p1080\|p1440\|p2160\|source] [--preset youtube\|shorts\|reels\|tiktok] [--bitrate auto\|small\|standard\|high\|max\|MBIT] [--loudness -16\|-14] [--captions srt\|vtt] [--fps N] [--codec h264\|hevc] [--gif-width N] [-o PATH]` | export; prints the file path |
| `transcribe <p> [--lang de\|en\|auto]` | transcribe on the Mac, merge into the doc |
| `auto-zoom <p>` | add zooms from the pointer clicks |
| `edit <p> --ops JSON\|@file\|@-` | apply edit ops (array, or one op object) |
| `rename <p> <title>` | set the title (folder stays) |
| `delete <p> --yes` | delete the project folder; without `--yes` it is a usage error before anything is sent |
| `mcp` | MCP server on stdio |
| `help`, `version` | text / version |

Options take `--opt value` or `--opt=value`. Global `--json` switches to NDJSON.

### Output

Human mode: the result on stdout (table for `list`, path for `export`/`stop`), progress overwritten on stderr (`export 50/200 (25 %)`), errors as `ir-record: <message> (<code>)` on stderr.

`--json`: everything on stdout, one object per line, nothing on stderr:

| `type` | Fields |
|---|---|
| `progress` | the app's event fields (`phase`, `frame`, `total`, countdown seconds, ...) |
| `warning` | `message` (transcription skipped/error, camera lost, low disk, take stopped in the app) |
| `done` | `result` |
| `error` | `code`, `message` |

### Exit codes

`0` ok. `2` usage errors and usage-type sentinels (`bad_request`, `*.bad_params`, `*.bad_op`, `*.bad_project`, `*.confirm_required`, `*.ambiguous`, `*.no_match`, ...). `1` everything else: `record.permission` (Screen Recording not granted), `record.busy`, `record.unsupported`, client errors (not running, token, interrupted), and a stop that did not end `ready`.

Ctrl+C stops a recording (`record` without `--duration`) or cancels a running export/transcription (the connection closes, the app aborts the job; exit 1). Ctrl+C during the countdown cancels the take before it starts — nothing is recorded, no project is left behind (exit 1).

`record --mic-device` / `--camera-device` take an id or a name from `sources` and switch that track on; a microphone that is not connected at start is replaced by the system default input (the control bar says so).

`export --preset` sets frame rate, bitrate level and voice level for a platform and names the file; it does not change the project's format — set 16:9 or 9:16 with `edit` (`look.set`). `--bitrate` takes a level or a number of Mbit/s (2–80). `--captions` writes the spoken text next to the video and needs a transcript (`transcribe`).

`export --out` takes an absolute path; without the format's extension it is appended (`demo.v2` → `demo.v2.mp4`). Another video format's extension (`.mov`, `.gif` …) is refused, an existing file is never overwritten.

## `record.edit` ops

`zoom.add`, `zoom.set`, `zoom.remove`, `zoom.auto`, `clip.split`, `clip.delete`, `clip.move`, `clip.duplicate`, `clip.speed`, `clip.crop`, `annotation.add`, `annotation.set`, `annotation.remove`, `effect.add`, `effect.set`, `effect.remove`, `track.add`, `track.set`, `track.remove`, `look.set`, `preset.apply`, `rename`.

`track.add {file, kind?, atProgrammeMs | clip (+startMs)?, gainDb?, fadeInMs?, fadeOutMs?, looped?, label?, id?}` lays an audio file under the programme: `file` is an absolute path (MP3, M4A, AAC, WAV, FLAC), copied into the project before the ops run and removed again if they fail; `kind` is `voiceover` (default, 0 dB, no fades) or `music` (−18 dB, 1 s fades); without a placement it starts at clip 0. `track.set {id, …}` changes level, mute, loop, fades or label; `track.remove {id}`. Narration for a scripted demo: `say -o q.aiff "…"` → `afconvert -f WAVE -d LEI16 q.aiff q.wav` → `track.add`.

Placement for add ops (and `*.set` to move): `atProgrammeMs` (+ `durationMs`, programme ms = the edited timeline) or `clip` (+ `startMs`, `durationMs`, source ms of that clip); default duration 2000. Read `doc` first: region ids and clip indices come from it. Ops apply in order, all or nothing, as one undo step in an open editor window. Example:

```
ir-record edit demo --ops '[{"op":"zoom.add","atProgrammeMs":1500,"depth":3,"focusX":0.4,"focusY":0.5},{"op":"rename","title":"Demo"}]'
```

## MCP tools

stdio, newline-delimited JSON-RPC 2.0, protocol 2025-06-18 (also 2025-03-26, 2024-11-05). Annotations (`readOnlyHint`, `destructiveHint`, `idempotentHint`) are sent from 2025-03-26; `structuredContent` from 2025-06-18. All schemas are objects with no extra properties.

| Tool | Description |
|---|---|
| `record_projects` | List all recordings. |
| `record_project` | One project: metadata, recording info, timeline view. |
| `record_doc` | Edit document + view; region ids and clip indices come from here. |
| `record_transcript` | Transcript words with start/end in source ms per take. |
| `record_sources` | Displays, windows, microphones, cameras. |
| `record_frames` | Render 1-8 frames (`atProgrammeMs`, `maxWidth` 64-1920); returns JPEG images. |
| `record_edit` | Apply `ops` (1-500) to a project, all or nothing. |
| `record_auto_zoom` | Add zooms planned from the pointer clicks. |
| `record_transcribe` | Transcribe and merge into the doc (`lang`). |
| `record_export` | Export MP4/GIF (`format`, `quality`, `preset`, `bitrate`, `bitrateMbps`, `loudness`, `captions`, `fps`, `codec`, `gifWidth`, `out`). |
| `record_rename` | Set the title. |
| `record_delete` | Delete a project folder; needs `confirm: true`. |
| `record_start` | Start a take (`display`/`window`/`area`, `mic`, `systemAudio`, `camera`, `countdown`, `title`). |
| `record_stop` | Stop and wait for the files (`open`). |
| `record_pause`, `record_resume` | Pause / resume. |
| `record_status` | Phase, elapsed ms, project, warnings. |

Projects are named by folder name or absolute path. `record_stop/pause/resume/status` never start the app.

**Progress and cancellation.** With `params._meta.progressToken`, control events become `notifications/progress` (monotonic `progress`, `total` for exports). `notifications/cancelled` closes that call's connection, which aborts the job in the app; the cancelled request gets no answer. Closing stdin cancels everything and exits 0. Each call has its own connection; read methods have a 60 s deadline (frames 180 s), long jobs none.

**Errors.** Tool errors are `isError: true` with `"<message> (<code>)"` and `structuredContent.error`; bad arguments never reach the app (`record.control.bad_params`).

## Safety

- One writer: the app serialises edits per project; a second export answers `record.editor.export_busy`.
- The socket is local only; every request carries a token from the discovery file (`control.bad_token` otherwise).
- Delete needs `--yes` (CLI) or `confirm: true` (MCP), checked before anything is sent.
- `record_edit` is marked destructive in MCP annotations (ops can remove clips); an open editor window can undo the whole call in one step.
- Screen Recording permission is needed for `record`; a missing grant is named up front (`record.permission`).
