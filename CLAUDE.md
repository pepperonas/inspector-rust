# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Install all workspace dependencies (run once after clone)
pnpm install

# Dev servers (hot-reload Rust + frontend)
pnpm dev:win          # Windows
pnpm dev:macos        # macOS
pnpm dev:linux        # Linux (Ubuntu/Debian)

# Production builds
pnpm build:win        # → target/release/bundle/msi/*.msi + target/release/inspector-rust.exe
pnpm build:macos      # → target/release/bundle/dmg/*.dmg
pnpm build:linux      # → .deb + AppImage (AppImage may fail locally; use build:linux:deb)
pnpm build:linux:deb  # → .deb only (recommended on Ubuntu)

### Hidden game easter eggs (search bar)

Exact match in the popup search field (case-insensitive, no autocomplete):

| Trigger | Game |
|---------|------|
| `getshaky` | Pong (`PongGame.tsx`, `lib/pong.ts`) |
| `rockthebox` | Snake — walls kill (`SnakeGame.tsx`, `lib/snake.ts`) |
| `rockthabox` | Snake — wrap-around edges |
| `space` | Space Invaders (`SpaceInvadersGame.tsx`, `lib/space-invaders.ts`) |

# Tests
pnpm test                                     # frontend vitest (all, single run)
pnpm --filter inspector-rust-frontend test:watch    # frontend vitest watch mode
cargo test --workspace                        # all Rust unit tests

# Static analysis (clippy + tsc + eslint in one shot)
pnpm check            # or: bash scripts/check.sh

# Individual checks
pnpm typecheck        # tsc --noEmit
pnpm lint             # eslint src
cargo clippy --workspace --all-targets -- -D warnings
```

Running Rust tests locally on Linux requires system libs:
```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libxdo-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

## Architecture

### Workspace layout

```
core/rust-lib/   — inspector-rust-core rlib: ALL business logic (DB, clipboard, hotkey, paste, snippets, notes, settings, backup, expander)
core/frontend/   — React 19 + TS + Tailwind v4 + Vite 7 (shared by all platforms)
win/src-tauri/   — Windows bundle shell: 2-line main.rs + Tauri config + capabilities
macos/src-tauri/ — macOS bundle shell: 2-line main.rs + Tauri config + capabilities
linux/src-tauri/ — Linux bundle shell: 2-line main.rs + Tauri config + capabilities (Ubuntu/Debian)
```

All three platform shells contain only `inspector_rust_core::run(tauri::generate_context!())`. All logic is in `core/rust-lib`. The Tauri CLI is invoked per platform via `pnpm --filter inspector-rust-{win,macos,linux} tauri {dev,build}`.

### Adding a new IPC command (end-to-end)

1. Implement logic in the relevant `core/rust-lib/src/*.rs` module.
2. Add a `#[tauri::command]` wrapper in `core/rust-lib/src/commands.rs`.
3. Register it in the `invoke_handler![]` macro in `core/rust-lib/src/lib.rs`.
4. Add a typed `invoke("command_name", { ...args })` wrapper in `core/frontend/src/lib/ipc.ts`.

### Database — five tables in one SQLite file

`DbHandle = Arc<Mutex<Connection>>` (rusqlite + parking_lot). Managed as Tauri state. File location:
- Windows: `%APPDATA%\InspectorRust\history.db`
- macOS: `~/Library/Application Support/InspectorRust/history.db`
- Linux: `~/.local/share/InspectorRust/history.db`

| Table | Purpose | Notes |
|---|---|---|
| `entries` | Clipboard history | SHA-256 deduped; capped at 1 000 rows via `prune_locked`; sorted by `last_used_at DESC` |
| `snippets` | Text expander templates | `abbreviation` + `title` + `body`; index on `abbreviation` |
| `notes` | Persistent bookmarks | Not pruned; `title` + `category`; any clipboard entry can be saved here |
| `settings` | Key/value app settings | Simple `key TEXT PK, value TEXT`; used for expander hotkey, theme, bruno defaults, input-lock chord, etc. |
| `totp_entries` | 2FA / TOTP accounts (v0.47.0) | `issuer` + `account` + `secret_enc` + `digits`/`period`/`algorithm`; index on `LOWER(issuer)` |

Rust unit tests use `Connection::open_in_memory()` — no temp files needed.

**Field-level encryption at rest (`crypto.rs`, v0.47.0).** Sensitive columns are AES-256-GCM-encrypted: `entries.content_text`, `entries.content_data`, `snippets.body`, `notes.content_text`, `notes.content_data`, and all TOTP secrets. Timestamps, IDs, content-type tags, hashes, abbreviations, titles, and categories stay plaintext (needed for sort/dedup/index). The key lives in the OS keychain (`keyring` crate; service `io.celox.inspector-rust`, user `history-db-key-v1`) with a `0600` `.dbkey` file fallback in the data dir. Storage format is `"v1:" + base64(12-byte nonce ‖ ciphertext+tag)`. `decrypt` is permissive — any value **not** prefixed `v1:` is treated as legacy plaintext and returned as-is, which is how `migrate_table` lazily upgrades pre-encryption rows. TOTP secrets are decrypted only on-demand for code generation and **never cross the IPC boundary**.

### Frontend data flow and `ListEntry` union

The history tab renders a unified `ListEntry` discriminated union (`lib/types.ts`):

```ts
type ListEntry =
  | { kind: "clip";               data: ClipEntry }
  | { kind: "snippet";            data: Snippet }
  | { kind: "calc";               data: CalcEntry }
  | { kind: "color";              data: ColorEntryView }
  | { kind: "command";            data: CommandEntryView }        // runnable power command
  | { kind: "command-suggestion"; data: CommandSuggestionView }   // autocomplete hint
  | { kind: "kill-target";        data: KillTargetView }          // process in the kill picker
  | { kind: "opener";             data: OpenerEntryView }         // German pickup line (hidden)
  | { kind: "finder-file";        data: FinderFileView }          // current Finder selection
  | { kind: "bruno";              data: BrunoEntryView }          // net-pay calculator result
  | { kind: "app";                data: AppEntryView }            // app-launcher hit
  | { kind: "pwgen";              data: PwgenEntryView }          // generated password
  | { kind: "bpm";                data: BpmTriggerView }          // BPM-detector launcher row
  | { kind: "totp-manage";        data: { label: string } }      // "2fa" → TOTP overlay launcher
  | { kind: "totp";               data: TotpListView }            // "otp <issuer>" autocomplete
```

Assembly order in `App.tsx` (`combined`): runnable command → command suggestions → calc result → color result → snippet matches → fuzzy clips, with the special rows (opener, bruno, pwgen, app, finder-file, totp) spliced in near the top when their trigger matches. Several **whole-list / whole-popup overrides**: in **kill-mode** (`kill` parsed) the list becomes `kill-target` rows; **game-mode** replaces the whole popup with a game (`<PongGame>` `getshaky`, `<SnakeGame>` `rockthebox`/`rockthabox`, `<SpaceInvadersGame>` `space`); **`2fa`** replaces it with `<TotpOverlay>`; **`bpm`** (Enter) replaces it with `<BpmDetector>`; **`freeze`** starts the input lock.

Snippet matches come from `findSnippets(query)` (backend prefix/contains SQL). The inline calculator (`lib/calc.ts`) runs `tryEvaluate(query)` — returns non-null only when the input contains an operator, function, or constant. Color rows come from `tryParseColor`. Command rows + suggestions come from `lib/commands.ts` (`parseCommand` / `commandSuggestions`).

### Tabs

`App.tsx` manages `activeTab: "history" | "snippets" | "notes" | "features" | "settings"`. Each tab is a separate panel component:

| Tab | Component | Backing data |
|---|---|---|
| History | `HistoryList` + `PreviewPanel` | `useClipboardHistory` + `useFuzzySearch` |
| Snippets | `SnippetsPanel` | `useSnippets` |
| Notes | `NotesPanel` | `useNotes` |
| Features | `FeaturesPanel` | read-only catalogue; fetches live shortcuts (`get_popup_hotkey` / `get_expander_config` / `get_direct_slots` / `get_input_lock_chord`) |
| Settings | `SettingsPanel` | IPC to `settings.rs` + `expander.rs` |

**Features tab** (`FeaturesPanel.tsx`) — a read-only, tabular reference of every function grouped into *Global hotkeys* · *Search-bar commands* · *In-popup & preview actions* · *Hidden games*, each row showing the **currently configured** shortcut/trigger + a short how-to. Configurable hotkeys are fetched live on mount (the panel remounts on each tab switch, so values are current); fixed global hotkeys are literal constants mirroring `hotkey.rs`. Tauri shortcut specs are pretty-printed by `lib/platform.ts::formatHotkey` (`Alt+Digit1` → `⌥1`, `Ctrl+Shift+V` → `⌃⇧V` on macOS).

### Tauri events

The table maps each emitted event to where it's emitted and what the frontend does with it (grep `\.emit(` in `core/rust-lib/src` for the authoritative list).

| Rust `app.emit(...)` | Emitted from | Frontend reaction |
|---|---|---|
| `"clipboard-changed"` | `clipboard_watcher` + every `db::upsert_clip`-adjacent IPC | `useClipboardHistory` re-fetches the list |
| `"capture-state-changed"` | Tray "Pause Capture" toggle | Header label flips between paused / active |
| `"window-shown"` | `hotkey::show_popup` | Resets to History tab + focuses search bar |
| `"popup-hidden"` | `hotkey::hide_popup` | Clears any transient toast / inline editor that shouldn't survive between sessions |
| `"open-snippets-tab"` | Tray "Manage Snippets" | Frontend switches to Snippets tab |
| `"open-notes-tab"` | Tray "Manage Notes" | Frontend switches to Notes tab |
| `"ocr-permission-needed"` | OCR / Screenshot hotkey fails Screen Recording pre-check | Popup opens, Settings tab + amber banner with `Open System Settings` + `Force reset` |
| `"expander-permission-needed"` | Expander hotkey fails Accessibility pre-check | Popup opens, Settings tab + amber banner (same shape as OCR banner) |
| `"expander-blocked"` / `"expander-hotkey-forwarded"` | Expander hotkey handler | Diagnostics / toast feedback on a blocked or forwarded expansion |
| `"finder-automation-needed"` | `Ctrl+Shift+F` Finder read fails the Automation TCC pre-check | Settings tab + amber banner (same shape as OCR), errno -1743 path |
| `"finder-selection-loaded"` | `get_finder_selection` succeeds | Popup shows the selected files as `finder-file` rows |
| `"autostart-changed"` (v0.14.0) | Tray "Start at Login" toggle | Settings → Startup checkbox reconciles to the now-effective OS state |
| `"color-picked"` | `pick_screen_color` worker completes (NSColorSampler / GDI overlay) | `ColorPickerModal` stores the hex; payload is `string \| null` (`null` = cancelled) |
| `"screenshot-saved"` (v0.19.2) | Screenshot pipeline finishes in save-to-file mode | Frontend toast confirming the file path the PNG was written to |
| `"screenshot-pending"` | `run_screenshot_pipeline` stashes a capture | Spawns the floating `ScreenshotPreview` window (see Screenshot preview) |
| `"editor-screenshot-changed"` | Editor opened / pending screenshot swapped | `ScreenshotEditor` reloads the source PNG |
| `"timer-fired"` / `"timers-changed"` | `timer.rs` worker on expiry / list mutation | Popup banner on fire; footer LED count reconciles |
| `"wakelock-changed"` | `wakelock_set` | Footer keep-awake indicator toggles |
| `"bruno-defaults-changed"` | `bruno_set_defaults` | Settings → Bruno section re-reads defaults |

### Text expander (`expander.rs`)

**Dynamic placeholders (`snippet_template.rs`, v0.50.0+).** Snippet bodies are stored verbatim and expanded at *paste* time by the pure `snippet_template::render(body, now, clipboard) -> Rendered { text, cursor_back }`. Tokens: `{date}`/`{date:FMT}`, `{time}`/`{time:FMT}`, `{datetime}` (chrono strftime), `{clipboard}` (clipboard text at paste time), `{cursor}` (removed; the caret is repositioned there afterwards via `paste::move_cursor_left`), and `{{`/`}}` for literal braces; an unknown `{token}` or a malformed strftime is emitted verbatim (never panics). Rendering happens at the **leaf paste primitives** so every path gets it: `commands::paste_snippet` (popup, via `expander::render_snippet_body` — current clipboard), `expander::paste_over_selection` and `expand_via_clipboard` (the saved pre-cycle clipboard is the `{clipboard}` source, since the live clipboard transiently holds the abbreviation there), and the AX in-place `try_replace_word_before_cursor` arm. The Snippets editor shows a one-line placeholder cheat-sheet under the body field.

Three expansion modes exist:

1. **Search-based** (always on): type an abbreviation in the search field → matching snippets appear at top of list → Enter pastes. Handled entirely in the frontend via `findSnippets()`.

2. **Abbreviation hotkey** (`expander.rs`, default hotkey `Alt+Digit1` — shown as `Alt+1`): fires from any app without opening the popup. Three paths via `text_field::FieldAccess::try_replace_word_before_cursor` → `ReplaceOutcome`:
   - **`Replaced`** — AX/UIA read the word + replaced it in place; on macOS this is verified by re-reading `AXValue`. No clipboard touch.
   - **`SelectionActive`** — AX *selected* the abbreviation but the in-place text set was a no-op (Electron / Chromium / Mac-Catalyst: WhatsApp, Slack, Discord, VS Code, …). `expander::paste_over_selection` pastes the body over the live selection (one clipboard write + paste + restore, **no** re-select).
   - **`Unsupported`** — the focused element exposes no settable text attributes → legacy cycle: save clipboard → `Opt/Ctrl+Shift+←` selects previous word → copy → look up → paste body → restore clipboard.
   Enabled/disabled + hotkey configurable in Settings tab (with `Alt+1`/`Alt+2`/`Alt+3` quick-pick presets). Pre-0.12 the default was `Alt+Backquote`, unreachable on German ISO Macs — `expander::migrate_legacy_default` bumps an un-customised install to `Alt+Digit1` once (idempotent). **Terminals are unsupported by this mode** (no AX-exposed input line, no GUI word-select on a shell prompt) — pressing the hotkey there does nothing.

3. **Direct hotkey → snippet slots** (`expander.rs` + `hotkey::register_direct_slots`, v0.13.0): bind a hotkey straight to a snippet — `expander::DirectSlot { hotkey, snippet_id }`, persisted as a JSON array under settings key `expander.direct_slots`. On press: `expander::paste_snippet_body` (AX-gated on macOS, runs on main thread) → **blind-delete the snippet's abbreviation length in Backspaces** (so typing `aiplan` + hotkey replaces, not appends — v0.25.2+; character count, multibyte-safe) → write body to clipboard → synthesize `Cmd/Ctrl+V` → restore clipboard. Reads nothing, so it still works **everywhere including terminals**; the Backspace approach is the trade-off — it deletes N chars before the cursor whether the user typed the abbreviation or not. `register_direct_slots` validates against collisions with the popup/OCR/abbreviation hotkeys + duplicates. `ExpanderShortcutState.direct: Vec<(Shortcut, i64)>`. IPC: `get_direct_slots` / `set_direct_slots`. Re-registered at startup from settings. Settings UI: "Direct hotkey → snippet" section (rows of `[HotkeyCapture] → [snippet <select>] [×]` + Add + Save).

On macOS, if Accessibility isn't granted the hotkey handler short-circuits *before* the doomed cycle: `expand_at_cursor` returns the `expander::ERR_NO_ACCESSIBILITY` (`"ax.permission_denied"`) sentinel, and `hotkey::register_expander`'s callback pre-checks `accessibility_granted()` → on a miss it shows the popup + emits `"expander-permission-needed"` (frontend turns it into an amber banner). Mirrors the OCR `screen.permission_denied` path.

The Settings panel includes a **"Diagnose"** button that calls `diagnose_at_cursor` — runs the capture half (no paste) and returns what would have been matched (or, on macOS without Accessibility, an explanatory error), for debugging.

### Screen-region OCR (`region_picker.rs`, `ocr.rs`)

Triggered by `Ctrl+Shift+O` — literal Control on every OS (v0.14.1+), not Cmd on macOS; avoids the `⌘⇧O` collision with VS Code / IntelliJ "Go to Symbol". Registered alongside the popup hotkey in `hotkey::register` or via the tray's **OCR Region** menu. Pipeline lives in `commands::run_ocr_pipeline(app)`, shared between the IPC `ocr_region` command, the global-shortcut callback, and the tray handler. Always dispatched to a worker thread (`std::thread::spawn`) because `screencapture -i` blocks until the user finishes the marquee.

- **Region capture** (macOS) shells out to `/usr/sbin/screencapture -i -x -t png <tmpfile>`. Read the file back, delete it. Empty / missing file = user pressed Esc → return `region_picker::Cancelled`. **Windows** (v0.19.2+) uses a GDI fullscreen layered overlay in `region_picker.rs` — the user drags a marquee, the picker blits the selected rect into a PNG. No external tool. **Linux** (v0.25.0+) shells out to `grim` + `slurp` on Wayland, or `scrot -s` on X11 — a missing tool yields a descriptive error pointing at the `apt` package.
- **OCR** (macOS) uses Vision via raw `objc2` msg_send: `NSData::dataWithBytes:length:` → `VNImageRequestHandler.alloc().initWithData:options:` → `VNRecognizeTextRequest` (recognitionLevel=0/Accurate, usesLanguageCorrection=true) → `performRequests:error:` synchronously → enumerate `request.results` taking `topCandidates(1).string`. Vision is linked explicitly via `core/rust-lib/build.rs` (`cargo:rustc-link-lib=framework=Vision`). **Windows** (v0.19.2+) uses WinRT `Windows.Media.Ocr` + `Windows.Graphics.Imaging` — picks up whatever language packs are installed in *Settings → Time & Language*; COM is initialised per-thread on the worker and the WinRT futures are `.get()`-blocked to keep the pipeline synchronous. **Linux** (v0.25.0+) shells out to the `tesseract` CLI — write the PNG to a temp file, `tesseract <tmp> stdout -l <langs>`, read stdout; offline, no extra Rust deps (`apt install tesseract-ocr tesseract-ocr-eng`, `-deu` optional).
- **Output**: text written to system clipboard (with `WatcherState::mark_self_write` so the watcher doesn't recapture it), plus two history entries — **source PNG first, recognised text second** (v0.14.2+), so the text wins the later `last_used_at` and is the most-recent entry at the top of the list (Enter then pastes text, not the screenshot). Returns `OcrResult { text, cancelled, chars }` so the frontend can show "recognised N chars" toasts.

### Screen-region screenshot (`commands::run_screenshot_pipeline`, v0.15.0)

Triggered by `Ctrl+Shift+S` (literal Control on every OS) or the tray's **Screenshot Region** menu. Same `region_picker::capture` + Screen-Recording TCC gate as OCR but **no OCR step** — the captured PNG is written straight to the system clipboard via `ClipboardContext::set_image` and persisted to history as a `[screenshot · N B]` image entry. Works on regions that contain no recognisable text (charts, buttons, photos, UI mockups). `mark_self_write(Image, b64)` arms the watcher to skip the round-trip. IPC: `screenshot_region` returns `ScreenshotResult { cancelled, bytes }`. `register_direct_slots` rejects `Ctrl+Shift+S` alongside the popup/OCR/abbreviation hotkeys.

### Eyedropper — global hotkey (`commands::run_eyedropper_pipeline`, v0.17.0)

Triggered by `Ctrl+Shift+C` or the tray's **Pick Color** menu. Reuses `screen_picker::pick_color_async` (macOS — `NSColorSampler` loupe) / `pick_color_blocking` (Windows — GDI overlay), but **does not open the popup** the way `pick_screen_color` (the in-modal entry point) does. On result: the hex string is written to the system clipboard via `ClipboardContext::set_text`, marked self-write so the watcher skips it, and persisted as a Text history entry. Cleanup (`clear_eyedropper_no_popup`) defers `demote_to_accessory` + `suppress_hide` clear via a 500 ms thread so the macOS focus-loss event from the policy demote doesn't fire before we want it to. No Screen Recording TCC grant needed — NSColorSampler / GDI overlay don't go through `screencapture`. IPC: `eyedropper_to_clipboard`. `register_direct_slots` rejects `Ctrl+Shift+C` alongside the popup/OCR/screenshot/abbreviation hotkeys.

**Multi-screen note (v0.19.1+)**: before hiding the popup, both eyedropper entry points call `hotkey::park_on_cursor_monitor` — `NSColorSampler` renders its loupe on the calling app's *primary* screen, which macOS derives from the last-active window. Parking the hidden popup on the cursor's monitor anchors the activation there so the loupe appears under the cursor, not on the main display.

### Power commands — search-bar palette (`commands.rs`, `lib/commands.ts`, v0.18.0+)

The search bar parses shell-style commands via `lib/commands.ts::parseCommand`. Complete commands surface as a `command` `ListEntry`; partial keywords surface as `command-suggestion` autocomplete rows. The `COMMANDS` catalogue (autocompletable):

| Keyword | Action | Backed by |
|---|---|---|
| `tren` / `trde` / `tr <text>` | Google Translate EN→DE / DE→EN / →DE; opens URL via `tauri-plugin-opener` | frontend only |
| `rz <W>x<H>` | Resize clipboard image (Lanczos3, 16 MP cap) | `image_ops`, IPC `resize_clipboard_image` |
| `optim` | Optimise clipboard PNG → Downloads (`oxipng`, lossless) | `image_ops`, IPC `optimize_clipboard_image` |
| `rmvvls <text>` | Strip vowels (aeiou + AEIOU + ä/ö/ü) → clipboard | IPC `remove_vowels_to_clipboard` |
| `kill [-9] [pattern]` | Process kill picker (see System commands) | `system_commands` |
| `reboot` / `shutdown` / `lock` | System power / lock (macOS) | `system_commands` |
| `mute` | Toggle system mute (macOS) | IPC `toggle_mute` |
| `freeze` | Input lock (block keyboard+mouse until unlock chord) | `input_lock` |
| `wakelock=1`/`wakelock1` / `wakelock=0`/`wakelock0` | Keep-awake on / off | `wakelock` |
| `bruno <€>` | German net-pay calculator | `bruno` |
| `timer <n>[s/min]` | Countdown timer | `timer` |
| `pwgen [N]` | Password generator (bare = default length, runnable so it outranks snippet matches) | `lib/pwgen.ts` |

`image_ops.rs` holds the resize/optim pipelines; `oxipng` is a workspace dep (pure-Rust, statically linked).

**Hidden triggers — exact word, NOT in `COMMANDS`** (never autocompleted; detection lives in `lib/commands.ts`): `getshaky` (Pong), `rockthebox`/`rockthabox` (Snake), `space` (Space Invaders), `opener` (German pickup line), `2fa` (`is2faTrigger` → TOTP overlay), `otp <issuer>` (`parseOtpQuery` → TOTP autocomplete rows), `bpm`/`bpms`/`bpmusic` (`isBpmTrigger`, Enter-activated → BPM detector). The app-launcher and Finder-selection rows are also implicit (no keyword).

### System commands (`system_commands.rs`, v0.19.0+)

Four system-level commands, also in the search-bar palette:

- **`kill [-9] [pattern]`** — `system_commands::list_running_processes` (via the `sysinfo` crate, sorted by memory desc, excludes our own PID) drives a live picker rendered as `kill-target` `ListEntry` rows; App.tsx overrides the whole list in kill-mode. `kill_process_by_pid(pid, force)` sends SIGTERM (or SIGKILL with `-9`). Native `window.confirm` before the kill.
- **`reboot` / `shutdown`** — `osascript` → `loginwindow` Apple Events (`aevtrrst` / `aevtrsdn`). No sudo. Native `window.confirm` first.
- **`lock`** — `pmset displaysleepnow`. No confirm (cheap to undo). IPC: `list_processes`, `kill_process`, `system_reboot`, `system_shutdown`, `system_lock`. macOS-only — Windows stubs return "not implemented".
- **`mute`** — toggles system output mute via `osascript`. IPC `toggle_mute` (`adjust_volume` is the related volume IPC).

### 2FA / TOTP manager (`totp_store.rs`, `totp_import.rs`, `crypto.rs`, v0.47.0)

RFC 6238 authenticator built into the popup. Two entry points: typing **`2fa`** (`is2faTrigger`) replaces the popup body with `<TotpOverlay>` (List / Add / Import-Export tabs); typing **`otp <issuer>`** (`parseOtpQuery`) surfaces matching accounts as `totp` rows with the live 6-digit code — Enter copies it. The List tab refreshes every 1 s via `totp_current_codes_all` and draws a countdown ring.

- Storage is the `totp_entries` table; secrets are `crypto::encrypt`-ed and **never** returned over IPC — only generated codes (`TotpCode { code, seconds_remaining }`) cross the boundary.
- **Import** (`totp_import.rs`) autodetects format from the first bytes: `otpauth://totp/…` single URI, `otpauth-migration://offline?data=…` (Google Authenticator bulk protobuf), Aegis JSON, 2FAS JSON, or a plaintext file of one `otpauth://` per line. Per-line failures are recorded in `ImportSummary { added, failed }`, never aborting the batch.
- IPC: `totp_list`, `totp_add`, `totp_delete`, `totp_current_code`, `totp_current_codes_all`, `totp_import`, `totp_export`. Frontend types in `lib/totp.ts` (`matchTotpEntries` is the fuzzy issuer/account ranker).

### Markdown → PDF (`md_to_pdf.rs`, v0.46.0)

Standalone, **no external `mrxdown` CLI**. Triggered by **`Ctrl+Shift+M`** on the current Finder selection. Pipeline: `pulldown-cmark` (CommonMark + GFM tables/footnotes/strikethrough/task-lists) → HTML with embedded GitHub CSS → WKWebView `createPDF` (macOS 11+). Output PDF lands sibling to source (`foo.md` → `foo.pdf`). `convert_files` is synchronous and **must run on the main thread** (WebKit is main-thread-only). `ConvertSummary { converted, skipped, failed, backend_unavailable }`; `backend_unavailable` is `true` on Windows/Linux (no native HTML→PDF backend yet).

### Bruno — German net-pay calculator (`bruno.rs`, `lib/bruno.ts`)

`bruno <€>` in the search bar. The actual tax/social-contributions compute (Steuerjahr 2025) runs in the **frontend** (`lib/bruno.ts`, constants in `TC`) for instant per-keystroke feedback as a `bruno` `ListEntry`; the Rust module only persists per-user defaults (`BrunoDefaults { tax_class, state, children, is_church_member, health_add }`) as individual `bruno.<field>` settings rows. Defaults: single, childless, NRW, TK Zusatzbeitrag 2.45%. IPC `bruno_get_defaults` / `bruno_set_defaults`; Settings → Bruno edits them. Not tax advice — a simplified §32a tariff.

### App launcher (`app_launcher.rs`)

Spotlight-like launcher (macOS). At startup walks `/Applications`, `~/Applications`, `/System/Applications`, `/System/Applications/Utilities` (top-level `*.app`); fuzzy matches surface as `app` rows, Enter does `/usr/bin/open`. Icons are lazy + bounded-LRU-cached (cap 100), rendered per-row via `sips … *.icns → png` → base64. IPC: `list_apps`, `refresh_apps`, `launch_app`, `get_app_icon`.

### Finder selection (`finder_selection.rs`, `frontmost_app.rs`, `osascript_util.rs`)

**`Ctrl+Shift+F`** reads the current Finder selection via `osascript` and shows the files as `finder-file` rows; selected images can be resized (`resize_file`), optimised (`optimize_file`), or cut out. This path needs the **Automation** TCC grant (Privacy → Automation → Finder) — distinct from Accessibility/Screen-Recording. Denial → errno -1743 → sentinel `ERR_AUTOMATION_DENIED` (`"finder.automation_denied"`) → `"finder-automation-needed"` event + amber banner. IPC: `get_finder_selection`, `resize_file`, `optimize_file`, `get_finder_automation_status`, `open_finder_automation_settings`, `force_reset_finder_automation_grant`.

`frontmost_app.rs` best-effort-names the frontmost app (used to name saved screenshots `<App>-YYYYMMDD-HHMMSS.png`); `osascript_util.rs::run_osascript` is the shared spawn-with-watchdog helper (SIGKILL on timeout) so a hung Finder / System Events can't wedge the main-thread hotkey handler.

### Input lock (`input_lock.rs`)

Typing **`freeze`** blocks all keyboard/mouse/trackpad input until an unlock chord (default: hold `i`, press `r`; configurable in Settings → Input Lock). macOS impl is **raw FFI to `CGEventTapCreate` + `CFRunLoop`** installed on the main thread's run loop (the `core-graphics` wrapper didn't actually drop events on Sonoma). Requires Accessibility (shares the expander grant). Safety hatch: `⌥⌘Esc` (Force Quit) always works above the tap. IPC: `get_input_lock_chord`, `set_input_lock_chord`, `start_input_lock`.

### Timer + wake-lock (`timer.rs`, `wakelock.rs`)

- **`timer <n>[s/min]`** — each `start_timer` spawns a worker; on expiry fires a native notification + `afplay Glass.aiff` + a `timer-fired` popup banner. Cancellable per-timer (`AtomicBool` polled ~200 ms). Footer shows the live count (`list_timers`, `timers-changed`). IPC: `start_timer`, `cancel_timer`, `list_timers`.
- **`wakelock=1` / `wakelock=0`** — keep-awake. macOS spawns `/usr/bin/caffeinate -disu` (real IOPM assertions); Windows uses `SetThreadExecutionState` **plus** a periodic invisible `F15` keypress (v0.50.2 — `SetThreadExecutionState` blocks power-sleep but does NOT reset the screensaver/lock idle timer, so on its own Windows kept *locking* the screen; the F15 nudge every 30 s keeps the screensaver + lock from engaging, the Caffeine/PowerToys-Awake trick); Linux jiggles the cursor (X11 only, no-op on Wayland). IPC: `wakelock_set`, `wakelock_get` (`wakelock-changed` event drives the footer indicator).

### Status toast (`status_toast.rs`, `StatusToast.tsx`, v0.51.0+)

A brief, on-screen confirmation flourish in its own transient window — same multi-window pattern as the screenshot preview, routed by `window.label` in `main.tsx`. Currently fired by the **wakelock** toggle: `wakelock_set` hides the popup *window* (not `app.hide()` — that would swallow the toast), then `status_toast::show` builds/reuses a frameless, transparent, click-through (`set_ignore_cursor_events(true)`), always-on-top `status-toast` window centred on the cursor's monitor, stores the payload in `LatestToast`, and emits `status-toast-changed`. `StatusToast.tsx` pulls the payload via `get_status_toast` on mount + on each event, replays a CSS flourish (`statusToastPop`/`statusToastIcon`/`statusToastRing`, ~1.6 s — Coffee icon for on, Moon for off), then calls `hide_status_toast`. The toast's hide path runs the deferred macOS `app.hide()` to return focus to the prior app. The window is **hidden, not closed**, between toasts. Payload is generic (`kind`/`on`/`title`/`subtitle`) for future one-shot status confirmations. The `status-toast` window is listed in each platform's `capabilities/default.json`.

### Screenshot preview + editor (`screenshot_preview.rs`, `screenshot_editor.rs`, `ScreenshotPreview.tsx`, `ScreenshotEditor.tsx`)

CleanShot-X-style flow layered on `run_screenshot_pipeline`. After capture the temp PNG (in `~/Library/Caches/InspectorRust/`) is stashed in `PendingScreenshot` state and a small frameless transparent window (`PREVIEW_LABEL`, 340×220) spawns bottom-left of the cursor's monitor — **no side effects until the user acts**: Save (→ Downloads + clipboard + history), Copy, Discard (delete temp), Edit, or Pin (keep current preview when a new shot arrives). The app name (`frontmost_app`) is baked into the saved filename. The **Edit** button opens a separate singleton annotation window (`EDITOR_LABEL`, 900×640) where `ScreenshotEditor.tsx` draws arrows / text / rectangles / highlights / pixelate-blur on a canvas; Save bakes a PNG and writes `<App>-<ts>-edited.png`. IPC: `get_pending_screenshot_path`/`_info`, `set_screenshot_pinned`, `screenshot_preview_save`/`_copy`/`_discard`/`_edit`, `reposition_preview_to_cursor`, `editor_save`, `editor_cancel`.

### Password generator + text transforms (`lib/pwgen.ts`, `lib/text-transform.ts`)

- **`pwgen [N]`** (frontend-only, `pwgen` `ListEntry`) — `requiresArg: false`, so a bare `pwgen` is a runnable command that surfaces the generator row (length `DEFAULT_PWGEN_LENGTH`) at the **top**, above any matching snippets; `pwgen 16` overrides the length. Four modes: `all` (alnum+symbols), `alnum`, `dict` (CapitalisedConcatenated words from `pwgen-dict.ts` padded with digits), `leet` (dict + leet-subst). CSPRNG via `crypto.getRandomValues` with rejection-sampling to avoid modulo bias; always returns exactly `length` chars.
- **`lib/text-transform.ts`** — pure transforms applied to a selected text entry and committed via the `commit_transformed_text` IPC: remove-vowels, upper/lower/title/camel/snake/kebab case, base64 encode/decode, url encode/decode, plain-text. The first nine map to `Cmd/Ctrl+1…9` in `PreviewPanel`.

### BPM detector (`components/BpmDetector.tsx`, `lib/bpm.ts`, v0.45.x)

Typing **`bpm`** (`isBpmTrigger`, Enter-activated like the games) replaces the popup body with a live mic BPM meter. Audio graph: `mic → highpass 30 Hz → lowpass 100 Hz (Q 1.5) → AnalyserNode` (a 30-100 Hz kick band; no speaker monitoring), 1024-sample frames fed to `BpmAnalyzer` (`lib/bpm.ts`). Detection is energy-onset + inter-onset-interval median clustering, octave-folded into [60,200] BPM with an octave-snap to stop 120↔240 flips; the displayed value is the **mean over a 4 s sliding window**. Pure frontend — no IPC, no Rust module.

### Appearance / theming (`styles.css`, `lib/theme.ts`, v0.20.0+)

All surface colours are CSS custom properties (`--color-bg`, `--color-surface`, `--color-border`, `--color-muted`, `--color-fg`, `--color-accent`, `--color-accent-fg`). The `@theme` block in `styles.css` is the **dark** palette (also the Tailwind-token default). Theme resolution keys off a `data-theme` attribute on `<html>`, written by `lib/theme.ts::applyTheme`:

- `data-theme="dark"` / `"light"` → explicit `:root[data-theme="…"]` override blocks.
- `data-theme="system"` (or absent) → the `@media (prefers-color-scheme)` query follows the OS.

Persisted in the `settings` table under `appearance.theme`; IPC `get_theme_preference` / `set_theme_preference` (the `normalise_theme` whitelist collapses anything unknown to `"system"`). Applied on App.tsx mount; Settings → Appearance has the three-way picker.

**Popup overlay size** (v0.49.0+) — a second Appearance control: a three-way `Small` / `Medium` / `Large` picker that resizes the `popup` window. Presets in `commands::window_size_dimensions` — small `600×430`, medium `700×500` (the historical `tauri.conf.json` default), large `840×600`. Persisted under `appearance.window_size` (`normalise_window_size` whitelist → `"medium"` on anything unknown); IPC `get_window_size_preference` / `set_window_size_preference`. `set_*` resizes the live window (`set_size` on the main thread); `commands::apply_window_size` re-applies the saved preset at startup from `lib.rs` setup. The next `show_and_position` recentres the window with the new dimensions, so no explicit re-centre is needed.

### `getshaky` — hidden Pong easter egg (`components/PongGame.tsx`, `lib/pong.ts`, v0.21.0+)

Typing the exact word **`getshaky`** into the search bar (detected by `commands::isGetShakyTrigger` — case-insensitive, whitespace-tolerant) sets `App.tsx`'s `gameMode` state (`"pong" | "snake" | null`) to `"pong"`, which full-screen-takes-over the app-shell with `<PongGame>`. **Deliberately NOT in the `COMMANDS` catalogue** — it must never surface in autocomplete; you have to know the word.

- `lib/pong.ts` — pure, unit-tested game maths: `clamp`, `botMaxSpeed` (ramp-up: 4.5 cap → +0.75 per bot point), `nextBallSpeed` (per-rally speed-up, capped), `paddleBounce` (edge-hit deflection, magnitude-preserving), `serveBall`, `frameScale` (frame-rate independence), `paddleHit` (swept collision).
- `components/PongGame.tsx` — the stateful `<canvas>` + `requestAnimationFrame` loop. Three phases: `intro` (~1.3 s shake transformation — `getshakyShake` / `getshakyTitle` CSS keyframes in `styles.css`), `playing`, `over`. Mutable game state lives in a `useRef` so the 60 fps loop never re-renders React; only score + phase changes do. Player paddle: mouse **and** arrow/W-S, both live. Board colours read live from the theme CSS vars.
- `useKeyboardNav` gained an `enabled` flag — set to `!gameMode` in App.tsx so the popup's nav handler doesn't double-fire Esc / arrows while a game owns the keyboard. **Esc is the only abort** (Space rematches on the game-over screen — not an abort).
- Entirely client-side: no backend, no IPC, no new Rust module.
- **Persistence (`lib/game-storage.ts`)** — every game persists a high score and a *suspended run* in `localStorage` under a per-game key (`pong` · `snake-classic` · `snake-wrap` · `space`). Pressing **Esc mid-game writes the full game state**; the next launch loads it (via a `useState` initializer), skips the intro, and **resumes exactly where it left off**. Ending a game (or Esc on the over/intro screen) clears the suspended run and finalises the high score. Pong has no per-match score, so its persisted stat is **career wins**; the two Snake variants keep **separate** high scores + separate suspended runs. All best-effort — a throwing/full `localStorage` degrades to "no saved data".

### `rockthebox` — hidden Snake easter egg (`components/SnakeGame.tsx`, `lib/snake.ts`, v0.24.0+)

The second hidden game, same shape as `getshaky`. `commands::rockTheBoxMode` detects the trigger word and returns the variant: **`rockthebox`** → `"classic"` (walls kill), **`rockthabox`** → `"wrap"` (the snake reappears on the opposite edge). `App.tsx` maps these to `gameMode` `"snake-classic"` / `"snake-wrap"`, replacing the app-shell with `<SnakeGame wrap={…}>`. Also **not** in `COMMANDS`.

- `lib/snake.ts` — pure, unit-tested grid logic: `step` (move / eat-grow / self collision with the tail-follow nuance; an optional `wrap` arg toggles wall-death vs. modulo-wrap), `spawnFood` (uniform free-cell pick), `tickInterval` (score-driven speed ramp, capped), `initialSnake`, `dirDelta`, `isOpposite`. Grid is `GRID_COLS × GRID_ROWS`.
- `components/SnakeGame.tsx` — the stateful `<canvas>` loop. Three phases: `intro` (~1.9 s box-assembling flourish — `rockTheBoxRock` / `rockTheBoxTitle` CSS keyframes; `INTRO_MS` must match the keyframe durations), `playing`, `over`. The game advances on a **fixed-timestep wall-clock accumulator** (frame-rate independent). Steered by arrow keys **and** WASD; a buffered `pendingDir` is reversal-checked so the snake can't whip into its own neck. Board colours read live from the theme CSS vars. **Each variant keeps its own high score + suspended run** (`snake-classic` / `snake-wrap`) — see the persistence note under `getshaky` and `lib/game-storage.ts`.
- Entirely client-side: no backend, no IPC, no new Rust module (persistence is `localStorage` via `lib/game-storage.ts`).

### `opener` — hidden German pickup-line easter egg (`lib/openers.ts`, v0.26.0+)

The third hidden trigger, same shape as `getshaky` / `rockthebox`. Typing **`opener`** in the popup search bar surfaces a random German pickup-line at the top of the list; Enter pastes it via the existing `pasteText` IPC. Detected by `commands::isOpenerTrigger` (`/^opener\b/i` against the trimmed query) — anchored to a word boundary so `opener foo` triggers (and re-rolls on every keystroke) while `openers` / `bopener` do not. Also **not** in `COMMANDS`.

- `lib/openers-data.ts` — auto-generated from the maintainer's `nicetobenice_db` PostgreSQL DB on the VPS via `ssh root@69.62.121.168 "sudo -u postgres psql -d nicetobenice_db ..."`. The export query LEFT-JOIN-LATERALs per-user rating + favourite state onto every approved opener, then `ROW_NUMBER() OVER (ORDER BY user-priority DESC, my_rating DESC, avg_rating DESC, id ASC) <= 100` — guarantees 100 entries even if the user has fewer than 100 marked (the remaining slots are filled with the highest global `avg_rating` rows). Output piped through `json_agg(text ORDER BY ord)` and embedded as a `ReadonlyArray<string>`. Re-run the same SQL to refresh.
- `lib/openers.ts` — pure picker. `hashString` (FNV-1a-variant) returns an unsigned 32-bit integer; `pickOpener(seed)` returns `TOP_OPENERS[hash % length]`. Deterministic per seed, so the React render loop doesn't flicker between picks while the query is unchanged.
- App.tsx wires an `openerEntry: ListEntry | null` (kind `"opener"`, `data.text`) into the top of `combined` when the trigger matches; the seed is the full query, so each keystroke re-rolls. The activate-handler pastes via `pasteText`. `HistoryItem` renders it with a `Sparkles` icon + italic body + an "opener" chip; `PreviewPanel` shows the full text with a "type any key to re-roll" hint.
- Entirely client-side at runtime — no live DB call, no IPC, no Rust module.

### `space` — hidden Space Invaders easter egg (`components/SpaceInvadersGame.tsx`, `lib/space-invaders.ts`)

Typing the exact word **`space`** (`commands::isSpaceInvadersTrigger`) sets `gameMode` to `"space"` and replaces the app-shell with `<SpaceInvadersGame>`. Not in `COMMANDS`. Arrow/A-D to move, Space/W/↑ to fire, Esc to quit (Space rematches on game-over).

- `lib/space-invaders.ts` — formation movement, bullets, collision, scoring (row bonuses).
- `components/SpaceInvadersGame.tsx` — canvas loop; intro uses `space-invaders-descend` / `space-invaders-title` in `styles.css` (`INTRO_MS` = 1400). Persists best score + a suspended run (key `space`) via `lib/game-storage.ts` — Esc resumes; see the persistence note under `getshaky`.

### Image tools (`recolor.rs`, `cutout_ml.rs`)

Two image actions surface in the preview pane:

- **Recolor** (`recolor.rs`) — `image::load_from_memory` → for each RGBA pixel, replace RGB with `lerp(target, white, luminance)` (alpha untouched) → re-encode → `db::upsert_clip` as a new history row. Eligibility gate: `image_chromaticity` samples up to 4096 opaque pixels (`max((max-min)/max)`). Toolbar only shown when chromaticity < 0.12 (logos / silhouettes).
- **Cut-out background** (`cutout_ml.rs`) — runs U2Netp via the `ort` crate (ONNX Runtime). Decode → resize to 320×320 → ImageNet-normalise (mean `[0.485, 0.456, 0.406]`, std `[0.229, 0.224, 0.225]`) → inference → resize mask back → apply as alpha on the original RGB → encode PNG. Output to `~/Downloads/<name>-cutout-<ts>.png`. Triggered by button in PreviewPanel or `Cmd/Ctrl+B`. Works on real photos (subject/background colour overlap is no longer fatal).
  - Model file: `core/rust-lib/models/u2netp.onnx` (~4.5 MB, embedded via `include_bytes!`).
  - ONNX Runtime is statically linked via `ort`'s `download-binaries` feature → release binary ~40 MB.
  - Session is held in `OnceLock<Mutex<Session>>` so the first cutout pays the model-load cost (~150 ms) once and subsequent calls reuse it.
- **Cutout source variants** — IPC has both `cut_out_image_entry(id)` (clipboard image rows) and `cut_out_image_file(path)` (single-file Files entries pointing at PNG/JPG/WebP/GIF/BMP). Same `cutout_ml::cut_out_subject` underneath via `commands::write_cutout`.
- The legacy chroma-key (`cutout.rs`) stays in the tree under `#![allow(dead_code)]` as a future fast-path for true-uniform-background images.

Both modules share the 16 MP hard cap and the multi-format `image` 0.25 dependency (PNG / JPEG / WebP / GIF / BMP).

### Clipboard capture priority

`clipboard_watcher::capture` checks formats in this order: **image → files → html → rtf → text**. Image-before-files matters on macOS, where copying a PNG/JPG/HEIC from Finder puts both the bitmap and the file path on the pasteboard — capturing as Files first meant the user only saw paths in history.

### `UiState` and modal focus

`UiState.suppress_hide` (AtomicBool, Tauri state) prevents the popup's "hide on focus-loss" handler from firing while a native file dialog is open. The frontend toggles it via `set_suppress_hide` before/after calling `tauri-plugin-dialog` commands (`dialog:allow-open`, `dialog:allow-save`).

### Platform-specific behaviour in shared code

- **Paste shortcut** (`paste.rs`): `Key::Meta` (Cmd+V) on macOS; `Key::Control` (Ctrl+V) elsewhere.
- **Focus-settle delay** (`paste.rs`): 120 ms on macOS, 50 ms on Windows/Linux.
- **Word-select modifier** (`expander.rs`): `Key::Alt` (Option) on macOS; `Key::Control` elsewhere.
- **Accessibility check** (`expander.rs`): `AXIsProcessTrusted()` via direct CoreFoundation FFI on macOS; always `true` on other platforms.
- **Dock visibility** (`lib.rs`): `set_activation_policy(Accessory)` on macOS.
- **Autostart tray label** (`lib.rs`): `cfg!(target_os = "windows")` → "Start with Windows", else "Start at Login". As of v0.14.0 it's a `CheckMenuItem` reflecting the current state (probed from `app.autolaunch().is_enabled()` on tray build); toggling updates the check + emits `autostart-changed`. IPC: `get_autostart_enabled` / `set_autostart_enabled`. Settings → Startup mirrors the toggle.
- **OCR engine** (`ocr.rs`): macOS Vision (`objc2` FFI); Windows WinRT `Windows.Media.Ocr`; Linux the `tesseract` CLI (`apt install tesseract-ocr` + language packs).
- **Region capture** (`region_picker.rs`): macOS `screencapture -i`; Windows a GDI overlay; Linux `grim`+`slurp` on Wayland, else `scrot -s` on X11.
- **Global-shortcut registration** (`lib.rs`): non-fatal since v0.25.0 — a failure (common on GNOME/Wayland) logs a warning instead of aborting startup; the tray menu and CLI flags still work.

### Linux notes (v0.25.0+)

The Linux port mirrors `win/` and `macos/` — `linux/src-tauri/` is a thin 2-line shell, all logic stays in `core/`. Two Linux-specific concerns drove new code:

- **`cli_dispatch.rs`** — Tauri global shortcuts often don't receive key events under GNOME + Wayland. The module parses CLI flags (`--toggle-popup` / `--open`, `--ocr`, `--screenshot`, `--pick-color`, `--help`) and dispatches them — the same actions as the tray menu (the tray handlers were refactored to call `cli_dispatch::dispatch`). `tauri-plugin-single-instance` routes a second `inspector-rust --ocr` invocation to the already-running instance, so the flags can be bound as desktop "custom shortcuts".
- **`desktop_shortcuts.rs`** (Linux-only `#[cfg]` module) — on first start under GNOME/Cinnamon Wayland it auto-registers `gsettings` custom keybindings (`Ctrl+Shift+V/O/S/C` → `inspector-rust --…`). Desktop env is detected via `XDG_SESSION_TYPE` / `XDG_CURRENT_DESKTOP`. The install is recorded under settings key `linux.desktop_shortcuts_profile`; clear it to re-apply. KDE is detected but not yet automated.
- `scripts/install-linux.sh` provisions apt deps + Node + Rust; `scripts/install-desktop-shortcuts.sh` and `scripts/ubuntu-terminal-copy-paste-ctrl-cv.sh` are standalone helpers. Build prerequisites + the per-feature support matrix live in `linux/README.md`.
- Not on Linux yet: the in-app eyedropper and the in-place AX/UIA text expander (the clipboard-paste expander fallback is used instead).

### macOS notes

`macos/src-tauri/Cargo.toml` requires `tauri = { features = ["macos-private-api"] }` for transparent windows. `enigo`'s `CGEventPost` (paste/expander) is gated by the TCC **Accessibility** permission (System Settings → Privacy → Accessibility), *not* an entitlement — the first paste or expander use triggers the prompt; after granting, a relaunch is required (macOS caches `AXIsProcessTrusted` per process) and the Settings panel offers a one-click relaunch. The Finder-selection + Markdown→PDF features (v0.46–0.47) additionally need the **Automation** TCC grant for Finder; `com.apple.security.automation.apple-events` + `NSAppleEventsUsageDescription` are injected into the bundle post-build by `scripts/install-macos.sh`. Three independent TCC surfaces are therefore in play and each has its own status/force-reset IPC: Accessibility (paste, expander, input-lock), Screen Recording (`screen_recording.rs` — OCR, screenshot, NOT the eyedropper), and Automation→Finder (`finder_selection.rs`).

### Backup

`backup.rs` serialises history + snippets + notes into a single versioned JSON document. Import merges: snippets upsert by abbreviation, history upserts by hash (dedup), notes append verbatim (no dedup key → re-import creates duplicates). `CURRENT_VERSION = 1`; importing a higher version is rejected.
