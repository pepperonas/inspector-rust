# macOS 27 compatibility

Checked on 2026-11-08 against the running system, not only the release notes.

| | |
|---|---|
| macOS | 27.2 (build 26B123), Darwin 27.2.0, Apple Silicon (M1 Pro) |
| SDK | MacOSX 27.2 (Command Line Tools 27.2, Apple clang 21.0.0) |
| Rust | 1.98.0 |
| Install history | 26.6.2 → 27.0 (2026-09-16) → 27.0.1 (2026-10-01) → 27.2 (2026-11-07) |

Minimum system version is unchanged (`minimumSystemVersion` 10.15, record
features macOS 14+). Nothing new in this pass needs macOS 27: the one new API
read (`NSPasteboard.accessBehavior`) is checked with `respondsToSelector` first.

## Fixed

### Spotify history stopped recording (macOS 27)

- **Change:** on macOS 27 (observed on 27.2; the last saved play is from
  2026-10-30, on 27.0.1) `AEDeterminePermissionToAutomateTarget` for a running Spotify
  (`com.spotify.client`) blocks forever in a semaphore, even with
  `askUserIfNeeded = 0`. Reproduced with a stand-alone Swift probe; Finder
  answers in 30 ms, and AppleScript to Spotify itself still works.
- **Effect:** the `ir-spotify` recorder thread called the check in its loop,
  hung in it (seen in a `sample` of the running app) and recorded nothing —
  no plays since 2026-10-30 although Spotify was playing.
- **Code:** `core/rust-lib/src/spotify/macos.rs` `permission()`.
- **Fix:** the check runs through `spotify::bounded` on a helper thread with a
  1.5 s limit; at most one call is in flight, later calls answer from the
  state the last AppleScript call learned. Tests:
  `spotify::bounded_tests` (timeout, no pile-up, recovery).

### Pasteboard privacy is now visible in the log

- **Change:** macOS 15.4 added a pasteboard-privacy alert for programmatic
  reads; macOS 27.2 removed its developer-preview switch
  (`EnablePasteboardPrivacyDeveloperPreview`, 27.2 release notes). On this
  machine reads still work without an alert (44 clips captured in the last
  day).
- **Effect if Apple turns it on, or the user picks "Deny":** the watcher reads
  nothing and the history stops without an error.
- **Code:** `clipboard_watcher::log_pasteboard_access` (called at start).
- **Change in Inspector:** one log line with `NSPasteboard.accessBehavior`
  (`default` / `ask` / `always allow` / `always deny`); "always deny" is a
  warning. Reading the property does not trigger the alert.

## Verified unchanged on 27.2

| Area | How it was checked |
|---|---|
| `pmset -g assertions`, `pmset -g`, `pmset -g custom` | Live output parsed; 27.2 captures added as fixtures (`sleep_status`, `nosleep`) |
| `nettop -P -x -n -l 2` | Longer header, spaced process names; 27.2 rows added as a fixture (`snitch`) |
| `lsof -nP -iTCP -sTCP:ESTABLISHED -iUDP` | Same columns as the existing fixture |
| `launchctl load -w` / `unload -w` (keep-alive) | Loaded and unloaded a probe agent; still works (legacy, `bootstrap` recommended) |
| Login item (`~/Library/LaunchAgents/InspectorRust.plist`) | `sfltool dumpbtm`: enabled, allowed; no quarantine attribute (27.0 refuses quarantined plists) |
| Agent app (`LSUIElement`) | `lsappinfo`: type `UIElement`, no Dock icon |
| `NSZoomButtonShowMenu` (window palette) | Key still present in the 27.2 AppKit; `defaults read -g` reads `0` |
| IOBluetooth, ambient light sensor, IOKit disk counters, memory pressure, process scan, free space, Finder context, CoreAudio mute, Codex/Antigravity limits | `#[ignore]` live tests pass |
| Vision OCR (in-process) | Live test passes |
| SDK deprecations | All C functions, classes and selectors used through FFI checked against the 27.2 headers: none deprecated. Only the unused NSColorSampler fallback calls `activateIgnoringOtherApps:` ("to be deprecated") |
| Compiler | `cargo clippy --workspace --all-targets`: no warnings in our code. The vendored BlackHole driver builds with only upstream format warnings |

## Not relevant to Inspector

- Menu item images hidden for apps linked against the 27 SDK — the tray menu
  has no item images.
- Direct TCC database access removed — Inspector only calls `tccutil reset`.
- VideoToolbox super-resolution and frame-interpolation additions — not used.
- `spctl --purge`, Intel-only app warnings — not applicable (universal or
  arm64 builds).

## Remaining risks

- **App-data protection (27.0):** access to other developers' app containers
  is denied without a prompt, and according to third-party reports also some
  `~/Library/Application Support` folders (Firefox confirmed, Chrome not).
  Could affect `yt-dlp --cookies-from-browser` (social-download retry) and the
  editor-cache category of `clean`. Not reproducible from a terminal with Full
  Disk Access; if a cookie retry or an editor cache fails, grant Inspector Rust
  Full Disk Access.
- **OCR cold starts:** since 2026-10-29 the first Vision call after a model
  recompile often fails with `CRImageReaderError 1` and the warm-up needs
  minutes or several attempts (see the OCR notes in `CLAUDE.md`). It recovers
  and warm calls take ~0.2 s, but the cause is not understood and not clearly
  tied to a macOS 27 change.
- **Pasteboard alert:** status after 27.2 unconfirmed by Apple; watch the log
  line above.
- **Private MultitouchSupport framework** (gestures): works on 27.2, but
  private APIs are not covered by release notes.
