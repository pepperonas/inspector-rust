# AI tasks — `task` / `ki`

Describe in your own words what should happen automatically. An AI writes a
script for it, you read it and approve it, and from then on it runs on its own:
on an interval, at a time on chosen weekdays, or when something happens.

```
task                     → your tasks, their status and next run
task <beschreibung>      → Enter opens the editor with that description
ki                       → same, via the alias
```

## Setup — Settings → KI-Anbieter

| Provider | How it connects | Default model |
|---|---|---|
| Claude (API) | Anthropic API key, `POST /v1/messages` | `claude-sonnet-5-5` |
| Claude Code (lokal) | the installed `claude` program with its own login — no key here | Claude Code's own default |
| Gemini | Google AI key, `generateContent` (key in the `x-goog-api-key` header, never in the URL) | `gemini-3.8-flash` |
| ChatGPT | OpenAI key, `chat/completions` in JSON mode | `gpt-6.1-sol` |

* Keys live in the OS keychain (service `io.celox.inspector-rust`, accounts
  `ai-key-<provider>-v1`). The UI only ever shows `••••abcd`; a key is sent to
  its own provider and nowhere else.
* Models are editable per provider. Leaving the default stores nothing, so a
  later change of the built-in default reaches you.
* **Testen** asks for one word — a cheap check of key, model and network.
* Claude Code runs as `claude -p --output-format json --no-session-persistence
  --setting-sources user --tools ""` in an empty, private folder
  (`~/Library/Caches/InspectorRust/ai-cwd`, mode 0700, emptied each time): no
  tools, so it can only write text; no project settings, so nothing planted in
  a shared folder can steer it.

## Creating a task

1. **Describe it.** Plain words, e.g. "Verschiebe Screenshots vom Schreibtisch,
   die älter als eine Woche sind, nach ~/Bilder/Screenshots".
2. **Skript erzeugen** (⌘⏎). The AI answers with name, language, script and a
   short explanation. It picks the language per task from what this OS allows:
   macOS zsh · Bash · Python · AppleScript, Windows PowerShell · Python,
   Linux Bash · zsh · Python. The answer is read strictly (one JSON object,
   allowed language, non-empty script ≤ 64 KB) — anything else is an error, not
   a script.
3. **Review.** The script is editable. Lines worth a second look are flagged:
   deleting (`rm -r/-f`, `shutil.rmtree`, `Remove-Item`), `sudo`, piping a
   download into a shell, writing to disks, `chmod 777`, killing programs. A
   hint, not a verdict.
4. **Revise** with a change request ("auch .pkg-Dateien") — the AI gets the
   current script and your wish. When editing an approved task, the change is
   shown as a diff against the approved version.
5. **When** — see triggers below — and a time limit (default 120 s, max 1 h).
6. **Speichern & freigeben**, or **Nur speichern** to approve later.

Generating takes 10–60 s. The popup may close meanwhile: the request keeps
running and the result is there when you reopen `task`.

## Approval — what it means

An approval covers **exactly the version you saw**. Technically it stores the
SHA-256 of language + script; a task is approved only while that hash matches
the current script. So:

* any change to the script or its language withdraws the approval by
  construction — there is no code path that edits a script and keeps it;
* approving sends the hash the panel *showed*; if the script changed in the
  meantime, the approval is refused;
* changing only name, trigger, time limit or provider keeps it;
* unapproved tasks never run — neither automatically nor via "Jetzt ausführen".

## Triggers

| Trigger | Runs | Notes |
|---|---|---|
| Intervall | every N minutes (1 min … 7 days), counted from the last run | the first run is one interval after approval, not immediately |
| Uhrzeit / Wochentag | at HH:MM local time on chosen weekdays | missed while asleep/off → runs **once** when back, not once per missed slot; DST gap → first moment after |
| Ordner ändert sich | when files appear or change directly in a folder (not recursive) | changes are collected until the folder is quiet for 2 s; `.DS_Store`, `.crdownload`, `.part`, `.tmp` and any name containing a control character (a newline would forge a second line in `IR_TASK_PATHS`) are ignored; at most every 10 s, and a task's own writes right after its run don't restart it |
| App-Start | once when Inspector Rust starts | |
| Aufwachen | after the computer wakes | detected as a wall-clock jump > 60 s beyond monotonic time |
| Nur von Hand | only via "Jetzt ausführen" | |

**Alle pausieren** stops every automatic run at once; manual runs stay possible.

## Running

* The script is written to a private file (0700) in the app cache, run with the
  interpreter for its language, and deleted afterwards. Working directory is
  your home folder; `PATH` includes Homebrew (`/opt/homebrew/bin`,
  `/usr/local/bin`) because a GUI app inherits a bare one.
* Environment: `IR_TASK_NAME`, `IR_TASK_TRIGGER` (`interval`, `schedule`,
  `folder`, `app_start`, `wake`, `manual`), `IR_TASK_PATHS` (folder trigger:
  changed paths, one per line, at most 200). The AI is told this contract.
* Each task runs at most once at a time. On timeout the whole process group is
  killed — a script can't leave a background child behind.
* Every run is logged: start, duration, exit code, timeout, trigger and the
  last 16 KB of stdout+stderr (the end — that's where errors are). 50 runs per
  task are kept. Output and scripts are encrypted at rest like clipboard
  history.

## Code

| File | Role |
|---|---|
| `core/rust-lib/src/ai_tasks/provider.rs` | the four providers: requests, response parsing, error messages, keychain, Claude Code call |
| `ai_tasks/generate.rs` | system/user prompt, languages per OS, strict parsing of the answer |
| `ai_tasks/trigger.rs` | triggers, next-run computation (DST-aware), event matching |
| `ai_tasks/runner.rs` | interpreter, PATH, time limit, process-group kill, output capping |
| `ai_tasks/store.rs` | tables `ai_tasks` + `ai_task_runs`, approval by hash |
| `ai_tasks/scheduler.rs` | background thread: timed tasks, folder watchers, app start, wake |
| `ai_tasks/ipc.rs` | Tauri commands |
| `core/frontend/src/components/TasksPanel.tsx` | list, editor, detail with run log |
| `core/frontend/src/components/AiProvidersSection.tsx` | Settings → KI-Anbieter |
| `core/frontend/src/lib/tasks.ts` | types + pure helpers (summaries, diff, risk hints) |

Live check against the local Claude Code:
`cargo test -p inspector-rust-core --lib ai_tasks_live -- --ignored --nocapture`.

## TODO — more triggers and ideas

Not built yet; collected for later.

**Triggers**
- Clipboard changes (optionally only when it matches a pattern: URL, IBAN, tracking number …)
- Network: joined/left a specific Wi-Fi, VPN up/down, came online after offline
- Power: plugged in / unplugged, battery below N %, charging done
- Display: monitor connected/disconnected, screen locked/unlocked, screensaver
- Audio: headphones/Bluetooth device connected, default output changed
- An app starts, quits or comes to the front (e.g. "when Zoom starts, mute notifications")
- USB volume mounted/ejected (e.g. back up an SD card automatically)
- Calendar: N minutes before an event, at the start of the first event of the day
- Focus mode / Do Not Disturb switched
- A file *inside* a folder tree (recursive watch) or matching a glob
- Webhook / local HTTP endpoint ("run task X") for Shortcuts, Stream Deck, cron on other machines
- After another task finished (chains), or only if it failed
- Idle for N minutes / user back from idle
- Location changed (home/office), via the Wi-Fi name as a proxy
- A global hotkey per task
- Sunrise/sunset at the current location
- Time windows ("only between 9 and 18 Uhr", "not on public holidays")

**Tasks**
- Notifications from scripts: a convention like `IR_NOTIFY: text` in stdout → system toast
- Pass the run's output into the clipboard or the history on request
- Variables per task (paths, thresholds) the script reads, editable without a new approval
- Dry run: let the AI add a "--dry-run" mode and run that first before approving
- Explain a failed run: send the error output back to the AI for a fix proposal (as a new version needing approval)
- Templates gallery ("clean Downloads", "back up a folder", "rename screenshots", "convert HEIC to JPG")
- Export/import tasks (JSON), and include them in the backup / device sync
- Per-task environment allowlist and a "no network" sandbox (macOS `sandbox-exec`)
- Statistics: success rate, average duration, last error per task
- Retry with backoff after a failure; disable a task after N failures in a row
- Cost/token display per generation
- Run history export (CSV)
- Approval with a second look: show what the script *touches* (paths it writes, commands it calls) extracted from the code
