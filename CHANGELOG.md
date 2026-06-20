# Changelog

All notable changes to Inspector Rust are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.84.82] — 2026-06-20

### Added

- **Timesheet — step 8: Claude-Code usage detection.** While tracking, a
  `notify` file-watcher on `~/.claude/projects/**/*.jsonl` records active Claude
  Code usage as its own `claude` intervals **per project** (the `cwd` basename),
  extending an interval per appended assistant turn and starting a new one after
  a 3-minute gap — with per-turn **token usage** (model · in/out). Claude time is
  a **separate dimension**: it's excluded from the focus/browser "active" total
  and `by_app` (so terminal + Claude time isn't double-counted) and shown in its
  own **Claude Code** card (project · time · tokens) in the tab + HTML export.
  Defensive JSONL parsing (only `type`/`timestamp`/`cwd`/`message.model`/
  `usage.*`); only new appends after `track on` are counted. New `tracking/
  claude.rs` (+4 tests); `notify` dependency.

## [0.84.81] — 2026-06-20

### Added

- **Timesheet — step 7: CSV + HTML export.** The Timesheet tab gets **CSV** and
  **HTML** export buttons (the viewed day → `~/Downloads`, revealed). CSV is a
  flat `date,start,end,duration_min,app,category,project,host,title,source,idle`.
  HTML is a **single self-contained file** (CSS + server-rendered inline-SVG
  charts, **zero external requests**, offline-viewable, dark theme): totals
  header + top-3 apps, active-time-per-day bars, an app donut, category + top-
  host bars, and the full event table. Footer `© 2026 Martin Pfeffer | celox.io`.
  Pure builders (`tracking/export.rs`) unit-tested (CSV escaping, self-contained
  HTML, empty-day). IPC `track_export`.

## [0.84.80] — 2026-06-20

### Added

- **Timesheet — step 6: inline editing.** Each event in the Timesheet tab can
  now be edited in place (✎): **relabel** the app, set/clear a **category**
  (optionally applied to *all* events of that app), toggle **idle**, and adjust
  **start/end** times — plus **delete** a row and **multi-select to merge**
  adjacent intervals. Changes go straight to the encrypted DB and the day
  reloads. `EventPatch` switched to `Option<String>` with `""`-clears semantics
  (JSON-`null` can't express an explicit clear through serde's double-`Option`).

## [0.84.79] — 2026-06-20

### Added

- **Timesheet — step 5: the Timesheet tab.** A new **Timesheet** tab visualises
  tracked time: day navigation (← / → / Today / date picker, `t` = today), a
  totals header (Active / Idle / Sessions), and dependency-free **inline-SVG
  charts** — a 24 h day timeline (Gantt, app-coloured, idle dimmed), an app
  donut, a category breakdown, and top hosts — plus a per-event list (time
  range, app, host/title, source/idle badges, duration). While viewing *today*
  with tracking active it refreshes live. Bare **`track`** opens this tab. Pure
  chart/format helpers in `lib/timesheet.ts` (unit-tested).

### Fixed

- The popup's list keyboard-nav (↑/↓/Enter) is now gated to the **History** tab
  only — previously it stayed armed on other tabs, so Enter could activate a
  hidden History item from Snippets/Notes/Settings/etc.

## [0.84.78] — 2026-06-20

### Added

- **Timesheet — steps 2–4: the tracker core + `track` command (macOS).** A
  working opt-in time tracker. `track on` starts a session and a background
  focus/idle loop; `track off` ends it (with a status toast). The loop records
  **gap-free, non-overlapping app-focus intervals** (frontmost app + window
  title via `osascript`, ~1.5 s tick), and on input inactivity past a threshold
  (default 300 s) **retroactively** closes the active interval at the moment
  input stopped and marks an `idle` span — resuming on input (idle is kept,
  visible/editable, not counted as active). New `tracking/mod.rs` (state machine
  + tick logic, unit-tested), `tracking/os/macos.rs` (frontmost +
  `CGEventSourceSecondsSinceLastEventType` idle). IPC: `track_start/stop/status`,
  `track_get_day` (events + totals + app/category/host breakdowns), plus
  `track_update_event/delete_event/merge_events/set_category/clear_all`. 12
  tracking unit tests. The Timesheet **tab** (visualisation + editing), exports,
  Claude watcher and browser bridge land in the next steps (see
  `docs/timesheet.md`). macOS-first; Windows/Linux OS modules are step 10.

## [0.84.77] — 2026-06-20

### Added

- **Timesheet (time tracking) — step 1/10: data layer.** Foundation for an
  opt-in, offline time-tracking feature (search-bar `track on/off`, focus-based
  interval recording, idle auto-pause, browser-tab via a loopback extension,
  Claude-Code usage, a day-navigable editable timesheet tab with CSV + HTML
  export — delivered incrementally; see `docs/timesheet.md`). This commit adds
  the SQLite schema (`track_sessions` / `track_events` / `track_claude_turns` /
  `track_categories`, created idempotently in `db::open`) and the persistence
  layer (`tracking/db.rs`): session + event lifecycle, edit/merge/category,
  range query, Claude turns, clear-all. `window_title` + `url` are encrypted at
  rest via the existing AES-256-GCM path (no new crypto). 8 unit tests.

## [0.84.76] — 2026-06-20

### Added

- **Loud dismiss-to-stop alarm overlay for timers/countdowns (new default).**
  When a timer, countdown or `alarm` fires it now (by default) shows a focused,
  always-on-top **overlay you must click** (or press Esc/Enter/Space) to stop —
  and it's **much more audible** than the old OS notification: a new bell-
  arpeggio alarm sound **loops** until dismissed, and the **system volume is
  raised** while it rings (then restored). Settings → **Timer alarm** lets you
  switch back to the classic **OS notification** style. New `alarm.rs` module
  (volume raise/restore + looping player + overlay) + `AlarmOverlay.tsx`;
  setting `timer.alarm_style` (`overlay` default / `notification`). (Volume
  raise is macOS; the overlay + looping sound are cross-platform.)

## [0.84.75] — 2026-06-20

### Added

- **`mkdir` and `touch` create nested structures.** `mkdir neuesVerzeichnis/neuesUnterverzeichnis`
  and `touch neuesVerzeichnis/neuesUnterverzeichnis/neueHallo.txt` now create the
  intermediate directories as needed (`create_dir_all`). Path validation was
  reworked into `sanitize_relpath`, which allows `/` (and `\` on Windows) for
  nesting but still rejects absolute paths, `..` traversal, NUL and reserved
  characters, so creation can never escape the target folder.

## [0.84.74] — 2026-06-20

### Added

- **`touch` can write file content inline.** `touch hallo.txt > das ist ein test`
  creates `hallo.txt` containing `das ist ein test` in the active Finder/Explorer
  folder (split on the first `>`; no `>` = an empty file as before).
- **Command aliases:** `resize` now works as an alias for `rz`, and `optimize`
  for `optim`.

## [0.84.73] — 2026-06-20

### Changed

- **`rz` accepts a plain space between the dimensions.** In addition to
  `rz 200x200` and `rz 200 x 200`, you can now write **`rz 200 200`**
  (space-separated). `parseResizeArg`'s separator is an `x`/`X` (optionally
  padded) **or** whitespace.

## [0.84.72] — 2026-06-20

### Fixed

- **`rz` (resize) now works on the image(s) selected in Finder/Explorer.** Like
  `optim` in the previous release, `rz <W>x<H>` only acted on the selection if
  you were already in finder-mode — otherwise it silently targeted the clipboard
  image (a no-op when none was there), so it looked broken. It now reads the
  **live** Finder/Explorer selection and writes a resized `<name>-WxH.<ext>`
  next to each selected image (Lanczos3; PNG/JPEG/WebP/GIF/BMP), falling back to
  the clipboard image when nothing usable is selected.

## [0.84.71] — 2026-06-20

### Changed

- **`optim` now compresses the image(s) selected in Finder/Explorer.** Typing
  `optim` reads the **live** Finder/Explorer selection (you no longer have to
  open finder-mode first) and writes a compressed `<name>-optim.<ext>` next to
  each selected image — **PNG** lossless via oxipng, **JPEG** re-encoded at
  quality 85 (kept only if it's actually smaller). When nothing usable is
  selected (or Automation isn't granted), it falls back to the old behaviour:
  optimise the clipboard PNG to ~/Downloads. (Previously `optim` only touched
  the selection if you were already in finder-mode, and was PNG-only.)

## [0.84.70] — 2026-06-20

### Changed

- **Flappy Bird (`learningtofly`) controls.** Holding Space no longer makes the
  bird climb continuously — a flap now fires only on a **fresh** key press (OS
  auto-repeat is ignored). Flying into the **ceiling** now ends the run (it used
  to clamp). And a new **AI autopilot easter egg**: if you're *holding* the flap
  key the moment the bird hits the **ground**, the AI takes over — the bird
  becomes invincible and flies forever (🤖 Autopilot). Pure AI controller
  (`aiTargetY` / `aiShouldFlap`) + `step(..., invincible)` are unit-tested.

## [0.84.69] — 2026-06-20

### Added

- **Color-picker loupe with the live hex under it (macOS).** The eyedropper
  (`Ctrl+Shift+C`, the tray *Pick Color*, and the modal *Pick from screen*) now
  shows a **custom magnifier loupe** that follows the cursor with the **live hex
  rendered right under it** — Apple's `NSColorSampler` can't show that, so it's
  replaced by our own loupe. The hex animates (per-character accent "heat" flare)
  as the colour under the cursor changes. Implemented via a one-shot snapshot of
  the cursor's display magnified in a transparent overlay (smooth — no per-frame
  capture); pixel grid + centre reticle; click picks, Esc cancels. Reuses the
  Screen-Recording permission already granted for OCR/screenshots. Windows keeps
  its GDI picker; the old NSColorSampler path is retained as a fallback.

## [0.84.68] — 2026-06-20

### Changed

- **Space Invaders (`spacer`) got a proper visual pass.** Replaced the plain
  ellipse aliens / flat triangle ship with **classic pixel-art invader sprites**
  (squid / crab / octopus, two animation frames each) pre-rendered with a baked
  accent glow, a **player cannon with a flickering engine flame**, **glowing
  laser bolts**, a **parallax twinkling starfield**, **explosion particle
  bursts** + a player-hit flash, and a vignette — all reading the app's theme
  colours (accent-bright top row → cooler bottom). Crisp on Retina (DPR-aware);
  lives shown as ▲ ship glyphs. Game logic unchanged.

## [0.84.67] — 2026-06-20

### Changed

- **`uptime` hero now shows animated milliseconds** — `Dd HH:MM:SS.mmm`. The
  three ms digits are a smaller tail that changes every frame, so they shimmer
  constantly (heat-glow) while the readable clock stays dominant.

## [0.84.66] — 2026-06-20

### Changed

- **`uptime` readout redesigned around the readable time.** The hero is now the
  uptime in a normal clock format — **`Dd HH:MM:SS`** (big, clean) — with each
  digit pulsing (accent heat-glow) as it ticks. The converted total-seconds /
  microseconds representation is demoted to a **small, dimmed** line below, but
  kept animated (its sub-second digits shimmer constantly). Boot timestamp under
  both. (Earlier versions made the µs seconds the giant hero, which looked
  noisy.)

## [0.84.65] — 2026-06-20

### Fixed

- **`uptime` looked broken.** The v0.84.64 odometer translated each digit by its
  *continuous* value, so every cell showed two half-digits smeared together. It
  now renders **clean tabular monospace digits** (exactly one glyph each) and
  conveys the constant motion with a **"heat" glow**: a digit flares to the
  accent colour the instant it changes and cools back over ~0.6 s, so the fast
  sub-second digits stay lit/shimmering while the slower ones pulse as they tick.
  Still one rAF loop, paint-only (`color`/`text-shadow`), no per-frame re-render.

### Changed

- **Screenshot preview: controls reveal on hover (CleanShot X behaviour).** The
  bottom-left capture preview is now just a clean thumbnail at rest; its
  darkening overlay + all action buttons (Close / Pin / Copy / Save / Pin-to-
  screen / Edit / Cloud) fade in only while the cursor is over it.

## [0.84.64] — 2026-06-20

### Added

- **`uptime` command — a live, animated uptime readout.** Typing `uptime`
  renders the system uptime in the right preview column as a **continuous
  odometer**: the uptime in seconds with **6 decimals (down to microseconds)**,
  where each digit is a vertical 0–9 strip translated every animation frame to
  its continuous value — so the sub-second digits scroll/blur nonstop and the
  slower places tick, motion always visible. A human-readable `Dd HH:MM:SS` line
  + boot timestamp sit below; a per-second pop and breathing accent glow add
  polish (reduced-motion disables them). Driven by one `requestAnimationFrame`
  loop writing `transform` to DOM refs (no per-frame React render; compositor-
  only). Base uptime via a cheap new IPC `get_uptime_secs`, anchored to
  `performance.now()`. Pure odometer maths unit-tested (`lib/uptime.ts`). Esc
  closes. (Followed the new add-a-command checklist: catalogue, dispatch,
  priority/red, Features tab, docs + README.)

## [0.84.63] — 2026-06-20

### Fixed

- **`stats` panel: laggy wheel/trackpad scrolling.** Arrow-key scrolling was
  already smooth (discrete jumps), but continuous wheel/trackpad scrolling
  repainted on the main thread each frame. The scrollport is now promoted to its
  own GPU compositor layer (`transform: translateZ(0)` + `contain: paint`), so
  scrolling is a cheap GPU translate; and the 1.5 s stats refresh is **paused
  while a scroll is in flight** (resumes ~200 ms after it settles), so a
  mid-momentum re-render can't stutter it.

### Changed

- **Custom commands are robustly always-highest-priority + red now.** The
  `commandEntry` `default` arm in `App.tsx` builds a generic runnable command
  row instead of returning null, so any command kind (even one missing a
  tailored case) still outranks clipboard history and gets the red accent —
  preventing a recurrence of the v0.84.59 `stats`/`trim` "clip won Enter" class
  of bug. Documented a **required checklist** in CLAUDE.md for adding a new
  custom command (catalogue → dispatch → priority/red → **Features tab** →
  **docs + README**); added `stats` to both READMEs' command list + count badge.

## [0.84.62] — 2026-06-20

### Fixed

- **`stats` panel still janked while scrolling.** The v0.84.61 `will-change:
  transform` left ~15 permanent compositor layers (one per usage bar) that the
  compositor had to blend on every scroll frame — itself a scroll-jank cause.
  Removed `will-change` **and** the bar transitions: the bars now snap to each
  poll's value via `transform: scaleX/scaleY` (no layout, no persistent layers,
  no per-frame blending), which a stats readout doesn't need to tween.

### Added

- **Arrow-key scrolling in the `stats` panel.** While the panel is active,
  **↑ / ↓** scroll it (plus **PageUp / PageDown** and **Home / End**); Esc still
  closes. Instant `scrollBy` so held keys step responsively.

## [0.84.61] — 2026-06-20

### Fixed

- **`stats` panel janked while scrolling.** The CPU / per-core / memory / disk /
  battery usage bars animated `width`/`height`, which are layout-triggering
  transitions; with the panel polling every 1.5 s, ~15 bars ran a 500 ms layout
  animation on the main thread roughly a third of the time, fighting scroll. The
  bars now animate `transform: scaleX/scaleY` (GPU-composited — no layout/paint
  during scroll), and each card gets CSS `contain: content` so scroll repaints
  are isolated. Scrolling is smooth now.

## [0.84.60] — 2026-06-20

### Fixed

- **`stats` (and `trim`) pasted a matching clip instead of running the command.**
  Commands must always outrank clipboard history, but typing `stats` copied a
  clip whose text contained "stats". Root cause: a runnable single-row command
  only surfaces (spliced to the top of the list, above fuzzy clips) if it has a
  `case` in `App.tsx`'s `commandEntry` `switch`; `stats` and `trim` were wired
  into `dispatchCommand` but **missing** from that switch, so they hit the
  `default` arm (`return null`) → no command row → a fuzzy-matching clip won
  Enter. Added the missing `case`s and documented the invariant (CLAUDE.md +
  an in-code comment on the `default` arm): a new single-row command needs
  **both** a `dispatchCommand` branch **and** a `commandEntry` case.

## [0.84.59] — 2026-06-19

### Added

- **`stats` command — live system stats in the preview column.** Typing `stats`
  renders a read-only, auto-refreshing dashboard in the right preview pane (same
  inline family as `brightness` / `sound` / `hue`; Esc closes): **CPU** (overall
  + per-core usage bars, brand, frequency, core count, load average), **memory**
  (used / available + swap), **disks** (per-mount usage), **network** (live
  up/down throughput), **temperatures**, **fans**, and **battery & power draw**
  (charge %, state, time remaining, **instantaneous watts**, health, cycle count,
  temperature). New Rust module `system_stats.rs` + IPC `get_system_stats`,
  polled every 1.5 s.
  - **Cross-platform, best-effort per OS.** CPU / memory / disks / network /
    uptime / load come from `sysinfo` (reliable on macOS, Windows, Linux).
    Battery & power draw come from `starship-battery` (IOKit / WMI / sysfs) —
    `energy_rate` is the live system power in watts. Temperatures come from
    `sysinfo::Components` (hwmon on Linux, SMC on Intel macOS, WMI on Windows)
    and are **summarised** from the raw (often dozens of cryptic, duplicated)
    sensors into clean **CPU / GPU / Battery / SSD** rows. Fans are read directly
    where there's an API: **macOS via a self-contained SMC reader** (`F<n>Ac`
    keys, decoding the `flt`/`fpe2`/`sp78`/`ui*` value formats — verified live on
    Apple Silicon: 2 fans, ~1600 rpm), **Linux via `/sys/class/hwmon`**; Windows
    has no rootless fan API (none shown). On Apple Silicon, where `Components`
    can be sparse, the SMC reader also supplies a CPU temperature by averaging
    the per-core thermal sensors. Every source degrades gracefully — missing
    sensors are simply omitted, never faked.
  - Pure cores unit-tested: the SMC value decoders, the temperature summariser,
    and the frontend byte/rate/duration formatters (`lib/format-stats.ts`).

## [0.84.58] — 2026-06-19

### Fixed

- **Popup that "opens briefly then closes by itself" (Windows).** After some
  time of use the clipboard overlay would flash open and immediately dismiss on
  every hotkey press, only recovering after killing and restarting the app. The
  cause was the focus-loss auto-hide (`WindowEvent::Focused(false)` →
  `hide_popup`): on Windows, `show()` + `set_focus()` can emit a **spurious**
  `Focused(false)` the instant the popup appears — a focus flicker that becomes
  reliable once another transient always-on-top window (the status toast, the
  record overlay) has perturbed the foreground z-order during the session, which
  is why it only started after a while and cleared on restart. The auto-hide now
  honours a short **post-show grace window** (`hotkey::within_show_grace`,
  300 ms): a focus-loss arriving immediately after a show is treated as the
  flicker and ignored, while a genuine click-away (well after the grace period)
  still dismisses. The same auto-hide path exists on macOS; its Accessory-app
  focus model doesn't produce the spurious event, so the bug wasn't observed
  there, but the guard protects it too. Pure core (`is_within_grace`) unit-tested.

## [0.84.57] — 2026-06-18

### Added

- **Feedback sounds for more actions + a master toggle.** The expand/paste
  click is now joined by short cues for **OCR recognised**, **screenshot
  captured**, **recording started / stopped**, and **copy to clipboard**
  (eyedropper). A new **Settings → Sounds** toggle (at the very top) turns all
  feedback cues on or off; it takes effect immediately (no relaunch) and
  defaults to on. The `sound.rs` module grew from the single hard-coded click
  into a small `Sound` palette (each cue an embedded WAV played fire-and-forget
  on a worker thread), gated by an in-process `AtomicBool` seeded from the
  `sound.enabled` setting at startup so the hot path never touches the DB.
  IPC: `get_sound_enabled` / `set_sound_enabled`.

### Tests

- **+4 unit tests**: Rust — every embedded cue is a valid RIFF/WAVE, cue
  filenames are unique, and the enable/disable toggle round-trips
  (`sound.rs`); frontend — the `getSoundEnabled` / `setSoundEnabled` IPC
  wrappers round-trip (`ipc.test.ts`).

## [0.84.56] — 2026-06-17

### Fixed

- **Screenshot-preview buttons now act on the first click.** The floating preview
  (bottom-left after a capture) is a non-activating window (`focused=false`), so
  on macOS the first click only made it key — Close/Save/Edit/Pin needed a second
  click. Added `accept_first_mouse(true)` so that first click routes straight to
  the webview and the button fires immediately.

## [0.84.55] — 2026-06-17

### Fixed

- **About-footer alignment.** The GitHub link / "made with ♥ by Martin Pfeffer"
  icons sat slightly high — added `leading-none` so the icons and text share the
  same optical centre.

### Tests & docs

- **New unit tests** (+8): a pure, extracted **`monitor_index_for_point`**
  (`screenshot_preview.rs`) covering the multi-monitor cursor-hit logic
  (half-open bounds, shared edges, negative origins, out-of-bounds) — the
  per-OS `pick_cursor_monitor_globally` branches now share it; and frontend
  coverage for `parseOtpQuery`, `isBpmTrigger`, `isFlappyTrigger`. **1198 tests
  (477 Rust + 721 frontend).** Docs/badges refreshed.

## [0.84.54] — 2026-06-17

### Added

- **Draggable Hue brightness sliders.** The per-lamp (and "All lamps") brightness
  bars in the `hue` preview are now click-and-drag sliders — set the brightness
  directly with the mouse (pointer-capture keeps the drag alive outside the bar;
  the track grows slightly on hover). The keyboard ←→ ±10 % still works.

## [0.84.53] — 2026-06-17

### Tests & docs

- **Extracted the audio/disco maths into pure, unit-tested helpers** (and reused
  them across the BPM detector + disco engine): `lib/audio-level.ts` (`rms`,
  `rmsToDbfs`, `dbfsToLevel`, `smoothStep`), `lib/disco-math.ts` (`beatColor`,
  `floorBrightness`, `nextIndex`), and `bpm.ts`'s `onsetThresholdForSensitivity`.
  **+25 frontend tests (692 → 717).**
- Docs/badges refreshed: **1190 tests (473 Rust + 717 frontend)**, dev-section
  counts, and the disco-engine notes (rAF + AnalyserNode, pure helpers).

## [0.84.52] — 2026-06-17

### Fixed

- **Disco didn't drive the lamps at all.** Detection ran in a `ScriptProcessorNode`
  whose `onaudioprocess` never fired reliably in WKWebView → no beats, frozen
  gauge, dead lamps. Switched to the proven **rAF + `AnalyserNode`** path (the
  same one the BPM detector uses), so beats fire and the lamps pulse again.
  **Caveat:** rAF is throttled while the window is hidden, so disco now **pauses
  while the popup is closed** — true run-while-hidden needs an AudioWorklet
  (audio-thread), planned as a follow-up.

### Changed

- **BPM detector uses a shared "warm" AudioContext (option C).** One process-wide
  context with a silent output unit kept open between uses, so the output device
  is already running when the mic opens (smaller mic-open glitch). The mic is
  wired while the context is **suspended**, then resumed — input+output come up
  together, avoiding the v0.84.50 ducking. (`lib/warm-audio.ts`.)

## [0.84.51] — 2026-06-17

### Fixed

- **BPM detector made the music go super-quiet (regression from v0.84.50).**
  Establishing the silent output unit *before* opening the mic made macOS treat
  the context as a full-duplex "communication" session and **duck** other apps'
  audio. Reverted to the v0.84.49 order — mic first, silent output added after —
  which keeps the reduced-stutter benefit without ducking. (The remaining brief
  mic-open glitch is the inherent CoreAudio device-reconfiguration; fully
  removing it needs native capture — see notes.) The v0.84.50 dB-readout
  reposition/enlarge is kept.

## [0.84.50] — 2026-06-17

### Changed

- **BPM detector: further reduced the brief mic-open stutter.** The silent output
  unit is now established and resumed **before** `getUserMedia` (with an 80 ms
  settle), so macOS *adds input to an already-running play-and-record session*
  rather than reconfiguring the device from a record-only state — shrinking the
  residual glitch left after v0.84.49.
- **dB readout repositioned + more present.** It no longer overlaps the canvas
  status line: moved into the lower third (clear of the BPM hero and the bottom
  status), enlarged (26 px number + "dB" unit + a wide glowing meter bar that
  eases up in scale/opacity/glow with the level). Still quieter than the BPM
  hero; red while pinned.

## [0.84.49] — 2026-06-17

### Fixed

- **BPM detector audio stutter — real root cause.** The warm-up defer (v0.84.47)
  wasn't it. A **capture-only** `AudioContext` (only analysers, nothing wired to
  `destination`) makes WebKit/macOS use a record-oriented audio session that
  re-routes/reconfigures the shared device on mic-open, glitching other apps'
  playback for a few seconds. The detector now routes a **muted gain to
  `destination`** (+ `ctx.resume()`) — a stable play-and-record session with an
  already-running output unit, the exact setup the disco engine uses (which
  doesn't stutter). The viz warm-up is trimmed to 300 ms.

### Added

- **dB readout in the BPM detector.** A subtle, animated full-band **dBFS meter**
  (smoothed number + thin level bar with a soft glow that tracks the level,
  attack-fast/release-slow) under the BPM hero — deliberately quieter than the
  big BPM number. Goes red while pinned to match the visualizer.

## [0.84.48] — 2026-06-17

### Changed

- **Disco detection is much stronger / the gauge actually swings.** Three fixes,
  bringing it in line with the raspi3 disco-controller:
  - **Loudness-gated punches, not confidence-gated.** The lights now flash on
    *every* onset while music is playing (gate on the level), instead of only
    when a confident tempo had locked — the old behaviour dropped most punches on
    irregular music or before the lock.
  - **Full-band dBFS gauge.** The level meter was reading only the 30–100 Hz bass
    band (tiny on a laptop mic) ×8; it now meters the **whole mix** as a VU-style
    dBFS meter with attack/release smoothing, so it swings with the music.
  - **Input gain + higher default sensitivity** (`INPUT_GAIN` 4×, sensitivity
    0.5 → 0.65) lift quiet laptop-mic signals above the noise floor.

## [0.84.47] — 2026-06-17

### Fixed

- **BPM detector no longer stutters other apps' audio on start.** `latencyHint:
  "playback"` (v0.84.45) wasn't enough on its own: the AAA visualizer's per-frame
  canvas/GPU work piled onto the ~1 s window where macOS reconfigures the shared
  input/output device on mic-open. The detector now runs only the cheap detection
  during a 900 ms warm-up and **defers the heavy spectrum read + particles +
  `drawScene`** until the device has settled (the viz fades in a beat late;
  detection is unaffected — `BpmAnalyzer` needs ~3 s of baseline anyway).

## [0.84.46] — 2026-06-17

### Added

- **Disco persists + `disco` command.** The Hue beat-sync was lifted out of the
  `HueBeatSync` component into a **module-level singleton** (`lib/disco-engine.ts`)
  that keeps running after the popup is dismissed, until explicitly stopped.
  Detection now runs in a **`ScriptProcessorNode`** (audio-render thread) instead
  of `requestAnimationFrame`, so it isn't frozen by the hidden window's rAF
  throttling. New **`disco`** command toggles it (`disco 1` = on · `disco 0` =
  off · bare = toggle). `AudioContext` keeps `latencyHint:"playback"` (the
  mic-open output-stutter fix).
- **Pin the BPM detector.** In `bpm`, **Enter** pins the detector — click-outside
  no longer closes it (`set_suppress_hide`), and the whole visualizer recolours
  **red** with a "pinned" label. Enter again / Esc unpins.

### Changed

- **Hue: selected lamp auto-scrolls to centre.** Tab/↑↓ now keeps the selected
  row vertically centred in the preview (`scrollIntoView({block:"center"})`), so
  navigating a long lamp list keeps the selection in view.

## [0.84.45] — 2026-06-16

### Fixed

- **BPM detector / Hue beat-sync no longer stutter other apps' audio** for the
  first few seconds. Opening the mic reconfigures the shared macOS CoreAudio
  device; the default `latencyHint: "interactive"` AudioContext uses a tiny
  output buffer that glitches running playback while the device settles. Both
  mic graphs now create the `AudioContext` with `latencyHint: "playback"` (a
  comfortable buffer) — detection is unaffected (it reads the input).

### Added

- **Expand sound.** Expanding a snippet via the abbreviation hotkey (`Alt+1` &
  co.) now plays a short mechanical-keyboard click for tactile feedback. New
  `sound.rs` embeds a WAV (low-latency, no decode) and plays it off-thread via
  the per-OS CLI player (macOS `afplay` · Windows PowerShell `SoundPlayer` ·
  Linux `paplay`/`aplay`). It fires only on a *real* expansion (in-place AX
  replace, paste-over-selection, or the clipboard cycle) — never on a no-match,
  and passive auto-expansion stays silent.

## [0.84.44] — 2026-06-16

### Changed

- **Hue panel keyboard model.** In the lamp controls: **Tab** jumps to the next
  lamp (Shift+Tab previous; both wrap, the "All lamps" master is the top row),
  **← / →** dim / brighten the selected lamp by **10%**, and **Enter** (or Space)
  toggles the selected lamp on/off. Esc still closes; 1–8 still pick a colour;
  ↑/↓ still move the selection. (Enter previously handed the arrows back to the
  list — that's dropped in favour of on/off toggle.)

## [0.84.43] — 2026-06-16

### Added

- **Beat-sync for Hue lamps** (mic-driven "disco", ported in spirit from the
  raspi3 `disco-controller`). The `hue` panel gains a **Beat sync** section: it
  listens to the laptop mic, detects beats with IR's own `BpmAnalyzer` (reusing
  the `bpm` detector's mic graph), and pulses the lamps on the beat — a
  **round-robin chase** via `hue_set_light` (punch the next lamp bright + colour,
  settle the previous). Round-robin individual lamps is deliberate: the Hue
  *group* endpoint is rate-limited to ~1 cmd/s. Three modes (rainbow / pulse /
  strobe), a sensitivity slider, and a live BPM + level readout. Lamp state is
  snapshotted on start and restored on stop; the mic + audio graph tear down on
  stop and on dismiss (a hidden popup never keeps the mic open). Frontend-only —
  new `HueBeatSync.tsx` + a runtime `BpmAnalyzer.setSensitivity()` (the `bpm`
  command's analyzer is unaffected).

## [0.84.42] — 2026-06-16

### Fixed

- **`hue` command now surfaces.** The command-row label builder's `switch` had no
  `case "hue"`, so it fell through to the `default` that returns `null` — the row
  was silently dropped and typing `hue` did nothing. Added the label/hint case
  (the backend + panel from v0.84.40 were already wired).

## [0.84.41] — 2026-06-16

### Changed

- **Dismiss now reverses the summon.** Closing the popup (Esc, or after running a
  command) plays a short accelerate-away exit — fade + drop + scale-down on the
  MD3 emphasized-accelerate curve — *before* the OS window hides, mirroring the
  spring entrance instead of snapping out. New `playExit` (reverse of
  `playEntrance`) in `lib/md3-motion.ts`; all frontend `hidePopup()` callers route
  through a wrapper that plays it first. Honors `prefers-reduced-motion` (resolves
  instantly → no added dismiss latency), and the Rust focus-loss/click-away path
  stays immediate on purpose.

## [0.84.40] — 2026-06-16

### Added

- **Philips Hue control (`hue` command).** Type `hue` to control your lamps
  inline in the preview column (same arrow-key model as `brightness`/`sound`):
  an **All lamps** master (on/off + brightness + colour) plus a row per lamp with
  on/off, brightness (←→), and **8 colour-preset swatches** (1–8) on colour-capable
  bulbs. First run pairs the bridge — **local SSDP discovery** (or manual IP) →
  press the bridge's link button → Connect. All traffic is **LAN-only plain HTTP**
  (no Philips cloud, no TLS); bridge IP + username persist in settings.
  - New `hue.rs` module (pure, unit-tested: `hex_to_rgb`, `rgb_to_xy`,
    `percent_to_bri`/`bri_to_percent`, `build_state_body`, link-button error
    mapping, light parsing) + `HuePanel.tsx`. IPC: `hue_status` · `hue_discover`
    · `hue_set_bridge_ip` · `hue_pair` · `hue_forget` · `hue_list_lights` ·
    `hue_set_light` · `hue_set_all`. Adds a minimal `ureq` HTTP dependency.
  - Documented in both READMEs + the in-app Features tab.

## [0.84.39] — 2026-06-16

### Docs

- **Audited every function and reconciled the docs.** Updated `README.md`,
  `README.de.md` and the in-app **Features** tab so they reflect the current
  feature set:
  - Added to the READMEs' "what it does" list: **screen recording**, **media
    tools** (social download + audio swap + trim), **audio-output picker**,
    **QR code**, **dev quick-tools**, **web-search bangs**, **BPM detector**, and
    the **unit/base/time converter**; expanded the power-commands line accordingly.
  - Corrected stale facts: the annotation editor now lists **9 tools** (was 5);
    **`reboot`/`shutdown`/`lock`/`mute` are cross-platform** (macOS · Windows ·
    Linux), not macOS-only — fixed in both READMEs and the in-app note; the
    multi-tab list now includes the **Features** tab.
  - Refreshed counts: **1158 tests (466 Rust + 692 frontend)**, **181 IPC
    commands**, and the dev-section test numbers.

### Added

- **Meme starter pack.** A curated set of **351 reaction GIFs (14 categories,
  ~126 MB)** for the `meme` picker now ships in the repo under [`memes/`](./memes)
  and as a downloadable **`inspector-rust-memes.zip`** release asset. Both READMEs
  gained a step-by-step install guide (extract to the default `~/My Drive/media/memes`
  path, or point Settings → Meme library at any folder).

## [0.84.38] — 2026-06-16

### Fixed (CI green)

- **Rust CI build failed on Linux** — `ddc-hi` (Linux DDC/CI brightness) pulls
  `udev` → `libudev-sys`, whose build needs `libudev-dev`. Added it to the CI
  workflow's apt install step.
- **Frontend lint failed CI** — two issues: `ScreenshotEditor`'s `TextInputOverlay`
  read `canvasRef.current` during render (`react-hooks/refs` error) — it now takes
  the ref object and reads the canvas geometry in a `useLayoutEffect` (positioned
  before paint, no flash); and a stale, unused `eslint-disable` directive in
  `useTauriEvent` was removed.

## [0.84.37] — 2026-06-16

### Added

- **Tab switches video / audio on a YouTube download suggestion.** When a YouTube
  `social` row is selected, **Tab** now flips the download target between
  **Download video** and **Download audio** (the selected one is highlighted), and
  **Enter** downloads the chosen one. The Enter path is routed through the preview
  bar so it shows the same progress animation as a click. Non-YouTube platforms
  (video-only) are unaffected. (`socialMode` + a run-signal in `App.tsx`;
  `SocialDownloadBar` becomes controllable.)

### Verified

- **`wakelock` / `caffeine` are enterable on every OS.** Neither command carries a
  platform gate, and the keep-awake backend has macOS (`caffeinate`), Windows
  (`SetThreadExecutionState` + F15 nudge) and Linux (`systemd-inhibit`) impls — so
  the same command triggers the equivalent logic everywhere. Locked in with a
  cross-OS `isCommandAvailable` regression test.

## [0.84.36] — 2026-06-16

### Added

- **Bruno net-pay result animates like the calculator.** The two **Netto / Monat**
  and **Netto / Jahr** headline rows now slot-machine-roll their digits and settle
  left→right (via the existing `AnimatedNumber`), re-rolling while you type the
  gross — exactly the calculator-result reveal. The other breakdown rows stay
  static so the panel doesn't get noisy. (`BrunoRow` was hoisted to module scope so
  the animated rows keep a stable identity and don't restart on unrelated renders.)
- **Opener switch is animated.** Cycling openers with **← / →** now slides the new
  opener in from the direction of the switch (from the right for *next*, from the
  left for *prev*) with a fade — a small MD3 carousel. The switch direction is
  tracked in `App.tsx` and carried on the opener entry; the preview keys the text
  on its value so each press replays the slide. New CSS keyframes
  `md3-opener-in-right` / `md3-opener-in-left`, `prefers-reduced-motion`-aware.

## [0.84.35] — 2026-06-16

### Fixed

- **`qr <text>` Enter → clean clipboard copy.** Pressing Enter on the QR command
  already put the PNG on the system clipboard, but the watcher then re-captured
  it as a second `[image W×H]` row next to the intended `[qr · …]` entry (its
  read-back PNG bytes differed from the frontend-canvas bytes, so the
  self-write fuse never matched). `qr_copy_png` now writes via the new
  `image_ops::write_clipboard_png_canonical`, which re-encodes through
  clipboard-rs's own PNG encoder and returns that **canonical** base64 — used
  for both the `mark_self_write` fuse and the stored history payload. Result:
  one clean QR entry, and the stored clip is byte-identical to what's on the
  clipboard (paste-ready).

## [0.84.34] — 2026-06-16

### Added

- **Animated download progress** in the preview panel. While a YouTube / Instagram /
  TikTok / Facebook clip is being fetched, the `SocialDownloadBar` now plays a
  Material 3 Expressive flourish — a bobbing download glyph inside two expanding
  accent rings, over a scrolling wavy indeterminate-progress line (`DownloadAnimation`,
  CSS keyframes `md3-dl-ring` / `md3-dl-icon` / `md3-dl-wave`) — instead of a plain
  spinner. Respects `prefers-reduced-motion`.

## [0.84.33] — 2026-06-16

### Docs & tests

- **README badges** refreshed + extended: corrected stale counts (IPC commands 166,
  events 26, Rust modules 51, global hotkeys 11, SQLite tables 5, popup tabs 5) and
  added **tests (1157 passing)**, **search-bar commands (57)**, a **media**
  (record · download · trim · swap) and a **Material 3 Expressive motion** badge.
- New **"Media tools"** prose section in `README.md` / `README.de.md` covering
  screen recording, audio swap, social download, and trim.
- **More unit tests** for the new media features: real-world social-URL variants +
  audio-mode flags + the cookie-fallback list (`social_dl`), faststart / lossless-
  audio / time formatting (`media_trim`), and extra `detectSocial` shapes (frontend).

## [0.84.32] — 2026-06-16

### Docs

Careful documentation pass: `docs/ai-prompts.md` updated to **27 prompts** with the
two new templates (`aifrontend`, `aibanana`) added to the table; every stale
"25 prompts" reference fixed across `README.md` / `README.de.md` (intro, badge,
heading, source-tree comments). The social-download feature note (README matrices
+ in-app Features tab) now mentions H.264 (QuickTime-playable) and the YouTube
browser-cookie fallback.

## [0.84.31] — 2026-06-16

### Fixed — YouTube "confirm you're not a bot" download failures

YouTube increasingly blocks anonymous downloads with an anti-bot check. The
downloader now retries automatically with **`--cookies-from-browser`** (Chrome →
Firefox → Brave → Edge; first that works wins) when it hits that gate, so a
download succeeds using your logged-in browser session. The first time, macOS may
prompt to allow reading Chrome's keychain cookie key — click Allow. (Safari is
skipped: macOS sandboxing blocks reading its cookie store without Full Disk
Access.)

## [0.84.30] — 2026-06-16

### Fixed — Instagram (and VP9) video download was unplayable on macOS

Instagram/TikTok/etc. serve **VP9** video, which can't be muxed into a playable
mp4 and which macOS QuickTime can't decode — so a "video" download came out
audio-only / errored. The downloader now **prefers H.264** (`-S
vcodec:h264,res,acodec:m4a`); yt-dlp picks the H.264 rendition when one exists
(e.g. Instagram's combined format) → a Mac-playable mp4, no re-encode. Verified
on a real Instagram reel (h264 + aac).

### Docs

Thoroughly refreshed `README.md` + `README.de.md`: screen recording, audio swap,
social download, trim, brightness/sound/clean commands, dev-tools, web-search
bangs, QR, second clipboard hotkey, MD3 motion, the calc reveal, encrypted
backups, and the new hotkeys; AI-prompt count 25 → 27.

## [0.84.29] — 2026-06-16

### Fixed — social download: wrong file timestamp + YouTube SABR failures

- The downloaded file now gets the **download time**, not the video's upload date
  (`--no-mtime`). yt-dlp's default stamps the file with the metadata timestamp, so
  a download of an old video sorted to the wrong place when you sort Downloads by
  date — it now appears at the top as expected.
- Work around YouTube's new **"SABR streaming"** restriction (`--extractor-args
  youtube:player_client=default,ios,web_safari`) — without it, many videos failed
  with "Requested format is not available". Verified: both audio and video now
  download. Files save to `~/Downloads` (unchanged).

## [0.84.28] — 2026-06-16

### Added — social-media download + trim command

- **Download social media** (YouTube / Instagram / TikTok / Facebook via yt-dlp).
  IR auto-detects a social URL — in a copied clip **or** typed/pasted into the
  search bar (query params like `&list=…&index=…` are kept) — and the preview
  offers **Download video** (all platforms) + **Download audio** (YouTube only);
  files land in `~/Downloads` and are revealed. Needs yt-dlp (install hint shown
  if missing). URLs are scheme-checked + `--`-guarded against argv injection.
- **Trim** (`trim` search-bar command). Pick a local audio/video file, set
  start/end, and cut it **lossless & fast** (`-c copy`, keyframe-snapped) or
  **frame-accurate** (re-encode) in an overlay; saves a sibling `<name>-trim.<ext>`.

The platform detector, yt-dlp arg builder, and trim arg builders are pure and
unit-tested; the trim filter graphs are verified end-to-end.

## [0.84.27] — 2026-06-16

### Changed — calculator input is highlighted + animated like a command

Using the calculator/converter now feels as "active" as a keyword command: the
search input turns **rose** with a pulsing calculator icon while computing, and
the result row gets the same reddish accent + entrance/icon-pop animation as the
command rows (previously it stayed neutral). The calc row's React key was made
stable so the animation fires once when the result appears, not on every keystroke.

## [0.84.26] — 2026-06-16

### Added — two new curated AI-prompt snippets

Added two prompt templates to the seeded AI-prompt library (now 27):
`aifrontend` — an "AAA premium frontend" Material 3 Expressive design brief; and
`aibanana` — an agent pipeline to auto-generate a creative Open Graph thumbnail
with Nano Banana (Gemini image API). Type the abbreviation in the popup to expand
the full prompt. Both ship in `seed/ai_prompts.json` (fresh installs) and were
added to the existing database; the Features tab lists them.

## [0.84.25] — 2026-06-16

### Fixed — screen-recording audio: de-click the residual crackle

Diagnosed the remaining crackle precisely: the capture path is silent-clean, but
system audio captured through a BlackHole loopback clicks because the playing app
and ffmpeg read/write the virtual device on **different clocks** (periodic
over/underruns). The stop-time audio post-process now always runs `adeclick` —
which removes the impulse noise and, verified, leaves clean audio untouched (a
pristine sine stays crest 1.414), so it's safe for mic recordings too — plus the
existing `atempo` time-stretch when needed, in one re-encode (256 kbps / 48 kHz).
Measured: the worst click jumps roughly halved. (A complete fix for loopback
crackle would need native ScreenCaptureKit capture — a larger, macOS-only change.)

## [0.84.24] — 2026-06-16

### Fixed — screen-recording audio quality (low bitrate / crackle)

Recorded audio was poor and crackly because ffmpeg's native AAC encoder, with no
explicit bitrate, defaulted to a very low rate (~62 kbps measured) — which sounds
like artefacts/crackle on music and system audio. Every audio path (and the
`atempo` sync re-encode, which would otherwise silently downgrade it again) now
encodes at **256 kbps AAC** at a standardised **48 kHz**, and the avfoundation /
dshow / pulse capture inputs get a generous **`-thread_queue_size 1024`** so the
capture doesn't drop packets (clicks) under load. Measured: the audio bitrate
went from ~62 kbps to a proper 256/170 kbps; no encoder warnings.

## [0.84.23] — 2026-06-16

### Security — audio-swap YouTube URL hardening

Hardened the `yt-dlp` invocation against argv flag-smuggling: the audio-swap
overlay's YouTube field could otherwise pass a value starting with `-` straight
to yt-dlp as an **option** (yt-dlp has dangerous flags like `--exec`).
`download_youtube_audio` now rejects any URL that doesn't start with `http://` or
`https://` before spawning, and `build_ytdlp_args` inserts a `--` end-of-options
guard immediately before the URL. Unit-tested.

## [0.84.22] — 2026-06-16

### Added — replace / overlay a video's audio (`Ctrl+Shift+Alt+M`)

Select a video in Finder and press **`Ctrl+Shift+Alt+M`** to open an overlay that
swaps or layers in a new audio track:

- **Audio source:** a local file (native open dialog) or a **YouTube track**
  downloaded via `yt-dlp` (`-x --audio-format m4a`; found on PATH like ffmpeg, with
  an install hint if missing).
- **Placement:** set the **start position** in the video and optionally **trim** the
  audio (in/out) with sliders.
- **Mode:** **Replace** (drop the original audio; the new audio plays from the start
  position, silence elsewhere) or **Mix** (keep the original and overlay the new
  audio, with volume sliders for each).
- Output is a non-destructive sibling **`<name>-audioswap.mp4`** (video stream-copied,
  so it's fast and lossless — only the audio is re-encoded), revealed in Finder.

Needs ffmpeg (and yt-dlp for the YouTube option). The ffmpeg/yt-dlp argument
builders are pure and unit-tested; the filter graphs are verified end-to-end.
macOS-first (Finder selection); the overlay also lets you pick the video manually.

## [0.84.21] — 2026-06-16

### Fixed — screen recording: system audio now actually records (macOS)

macOS avfoundation can only *capture* a loopback device (BlackHole), and it's
silent unless the system **output** is routed through that loopback — so
recordings with "System" audio came out silent even with BlackHole installed,
because the default output was the plain speakers.

The recorder now arranges this automatically: when you record with system audio,
it checks whether the default output already routes to a loopback and, if not,
**temporarily switches the default output to a Multi-Output device that contains
both the loopback and a real output** (so the audio is captured *and* still
audible), then **restores the original output when the recording stops**. The
suitable device is found by inspecting each output's CoreAudio aggregate
sub-device list for a BlackHole/loopback member — no guessing. Verified
end-to-end: the captured BlackHole track went from −91 dB (silence) to −18 dB
(real audio) with the auto-switch.

If no Multi-Output device containing a loopback exists, recording proceeds as
before (the system track is silent) and a warning is logged — set one up in
Audio MIDI Setup (BlackHole + your speakers) and it'll be picked up automatically.

## [0.84.20] — 2026-06-16

### Added — calculator result "slot-machine" reveal

The calculator's big result (in the preview pane) now spins like a slot machine
before locking in. For ~0.5 s the digit characters roll through random values,
then settle **left→right** in a cascade, finishing with a spring "pop" + accent
flash the instant the final value locks. Only digits spin — signs, decimal
points, separators, hex/unit letters (`0xff`, `5 km`) and dates stay legible, so
it works for every result type.

The settle deadline is pushed forward on each keystroke, so the digits keep
rolling *while you type the expression* and lock in 0.5 s after you stop —
smooth at any typing speed, no restart flicker. Honours **prefers-reduced-motion**
(renders the value instantly). The per-frame roll/cascade math (`lib/scramble.ts`)
is pure and unit-tested.

## [0.84.19] — 2026-06-15

### Added — more Material 3 Expressive motion (focus: custom commands)

Extended the motion layer, especially around the keyword-triggered custom
commands (the reddish/rose rows):

- **Command rows** fade in when they surface as you type (opacity-only — the row
  carries the virtual list's transform, so animating transform there would fight
  the virtualizer). The `command` row's React key was made stable (no longer keyed
  on the raw input) so typing an argument updates it in place instead of
  remounting — the entrance fires once when the command appears, not per keystroke.
- **Selected command icon** does a one-shot spring "pop" — a small *ready to run*
  affordance that re-fires each time the row is re-selected.
- **Command-mode takeovers** now spring in: the **2FA** overlay (`2fa`), the **BPM**
  detector (`bpm`), and the inline **brightness** / **sound** panels scale in with
  the MD3 pop-in instead of a plain fade.

All of it still honours **prefers-reduced-motion**.

## [0.84.18] — 2026-06-15

### Added — Material 3 Expressive motion

Introduced a Material Design 3 *Expressive* motion layer at the app's key
interaction points, tuned to stay snappy for a keyboard-first launcher:

- **Popup open** — a real spring "pop-in" (the spring's overshoot/bounce is
  simulated and baked into the keyframes, played via the Web Animations API so
  it re-triggers on every open) using the MD3 *expressive spatial* token.
- **Tab switch** — content fades + slides in on the emphasized-decelerate curve.
- **Tab buttons** — press scales them down; the active tab rests slightly
  enlarged and pops in with a small overshoot.
- **Inline banners** (paste/permission/timer alerts) — drop in from the top.
- **Colour-picker modal** — springy scale-in with a fading backdrop.
- **Action buttons** (preview pane: cut-out, save, recolour, copy, transforms,
  smart actions, regenerate) — tactile press feedback.

Motion tokens (the MD3 easing curves + duration scale) are shared as CSS custom
properties; the spring token table + the second-order spring simulator that
generates the entrance keyframes live in `lib/md3-motion.ts` and are unit-tested
(overshoot for under-damped springs, no overshoot for critically-damped effects
springs, faster settle for stiffer springs). All of it honours the OS
**prefers-reduced-motion** setting.

## [0.84.17] — 2026-06-15

### Changed — build tooling: `target/` no longer balloons

The Cargo build dir had grown to ~32 GB. `scripts/install-macos.sh` now self-cleans
after a successful install (the app is already in `/Applications`, so `target/` is
disposable): it deletes `target/debug` (dev-server artifacts, never needed for a
release install — the biggest hog if `pnpm dev:macos` was ever run), keeps only the
newest `.dmg`, and runs a one-off `cargo clean` if `target/` is past a size cap
(`IR_TARGET_CAP_GB`, default 12 GB; `0` disables) — needed because Cargo never
garbage-collects the rlibs of *old* dependency versions in `release/deps`, which
creep up over many builds. The release build cache is otherwise kept so normal
rebuilds stay fast/incremental. (App behaviour unchanged.)

The script is also **self-healing**: if `node_modules` is missing (a disk cleaner
can wipe it), it runs `pnpm install` before building instead of failing with
`tauri: command not found`.

## [0.84.16] — 2026-06-14

### Fixed — popup overlay loads slowly (huge perf win)

The popup felt slow to open because every show (and every clipboard change)
fetched the **full** history — including the multi-MB base64 PNG of every image
clip — decrypted each, JSON-serialised it, and marshalled it all across the
IPC boundary into the webview. On a real library that was **~143 MB** of image
blobs the list never even renders (image rows show an icon, not the bitmap).

The history-list query now omits the image blob (`db::list_slim`): the same
real library drops from **142.6 MB → 2.1 MB** per fetch (~68×). When an image
clip is selected, the preview fetches its pixels on demand by id (`get_clip`);
paste, cut-out, save and recolor already worked by id, so nothing else changed.
The full-payload query (`db::list`) is unchanged and still used by the backup
export, which needs every byte. Measured on the maintainer's Mac; the native
window-show path was already fast (6–16 ms in the logs) — the lag was entirely
this payload.

## [0.84.15] — 2026-06-14

### Changed — BPM detector: premium beat-reactive visualization

Replaced the BPM detector's plain number + two progress bars with a full
beat-reactive `<canvas>` animation. A second AnalyserNode taps the *raw* mic
(the detection chain stays bandpassed to the kick band) to drive a mirrored,
slowly-rotating **spectrum ring** with an accent→white gradient and additive
bloom; each detected beat fires an expanding **shockwave** and a **particle
burst** sized by the kick intensity, a **core orb** breathes with the bass and
springs on the beat, a **confidence arc** sweeps rose→amber→emerald, and the
hero BPM number glows brighter on every beat. All colors come from the active
theme and update live on a theme switch.

Also a performance win: the animation is drawn from a `requestAnimationFrame`
loop reading refs — **no React re-render per frame** (the old version called
`setState` 60×/s); only a throttled ~7 Hz update drives the top-bar level label.
The pure visual helpers (`lib/bpm-visual.ts`: log-spaced spectrum binning,
attack/release bar smoothing, color mixing, easing) are unit-tested.

## [0.84.14] — 2026-06-14

### Fixed — screen recording: mic audio plays too fast / silent tail (the real fix)

Definitively diagnosed and corrected the recording-audio desync that 0.84.12 and
0.84.13 only chased. The root cause: **avfoundation systematically under-delivers
audio samples.** A capture whose *video* spans N seconds (steady CFR frames) ends
up with only ~85–90 % of `N × sample_rate` audio samples — verified empirically (a
real 9.27 s recording held just 8.14 s of actual samples). The samples are
continuous (no silence gaps), so the audio is *time-compressed*: it plays ~1.15×
too fast and runs out before the video ends.

The trap that defeated the earlier attempts: the MP4 muxer writes **stretched PTS**
for the under-delivered audio, so the audio stream's reported `duration` metadata
reads ≈ the video length (a lie). The ground truth is the decoded **sample count**
(`astats`), which is immune to PTS.

The fix measures each finished recording (true audio sample count vs. video
duration) and, when they diverge by more than 2 %, re-syncs with a single
pitch-preserving `atempo` stretch (video stream copied untouched, no inline
resampler → no stutter/crackle). The correction factor is computed **per recording**
(the shortfall varies per run), and the pass is a no-op when audio is already in
sync — so it's safe on every platform (Windows/Linux capture paths that don't
under-deliver simply skip it). Verified: the 8.14 s-of-audio / 9.27 s-of-video clip
came out with audio filling the full 9.27 s, A/V in sync. The pure ratio math
(`atempo_ratio`) is unit-tested.

## [0.84.13] — 2026-06-14

### Fixed — recording audio stutter/crackle (regression from 0.84.12)

0.84.12 added `aresample=async=1` to "stretch" the mic audio to the video
length. But a WAV silence-scan showed avfoundation's captured audio is actually
**continuous** (no real dropouts — the apparent shortfall is just irregular
packet timestamps), so the resampler was needlessly resampling clean audio and
introduced stutter/crackle. The resampler is **removed** from all paths. The
single-input case (mic shares the screen-capture input's clock) already matches
the video length; the two-input system+mic mix keeps only `amix … ,apad` +
`-shortest` (silence-pad to the video length, no resampling), so a short/silent
loopback still can't truncate the track. Verified: matched audio/video durations
with no resampling.

## [0.84.12] — 2026-06-14

### Fixed — screen recording: mic audio played too fast / silent tail; A/V sync

avfoundation delivers audio slightly slower than wall-clock, so the recorded
audio came out ~10% short of the video — it played too fast and the end of the
clip had no sound. Every audio path now runs through `aresample=async=1:first_pts=0`,
which pads/aligns the audio to the capture timeline (verified: an 8 s capture
went from ~7.1 s of audio to a full 8.0 s). The system+mic mix additionally uses
`amix … ,apad` + `-shortest` so a short or silent input (e.g. an unrouted
loopback) can't truncate the track. Applied to macOS/Windows/Linux builders.

**System audio note:** capturing system audio on macOS requires the output to
be routed through a loopback (BlackHole) — e.g. a Multi-Output Device combining
your speakers + BlackHole, or using Background Music as the output. With output
set directly to speakers, BlackHole receives nothing and the system track is
silent (a macOS limitation, not a recorder bug).

## [0.84.11] — 2026-06-14

### Fixed — abbreviation hotkey (Alt+1) opened the popup instead of expanding

In a terminal, the buffer-backed `try_hotkey_expand` is what expands (the
AX/clipboard path can't, and falls back to opening the popup). It was failing
because the passive keystroke monitor's tap only treated **Cmd/Ctrl**-modified
keys as non-text — **Option/Alt was not checked**, so the `Alt+1` keypress
itself was decoded as a character and appended to the tracked buffer, clobbering
the abbreviation's suffix right before the hotkey read it. The tap now leaves
the buffer untouched for Option/Alt-modified keys, so the hotkey expands the
abbreviation you just typed (in terminals too). Added breadcrumb logs to
`try_hotkey_expand` for future diagnosis.

## [0.84.10] — 2026-06-14

### Fixed — deadlock that froze the whole app (no hotkeys after using the recorder)

A process sample (via the new logging + `sample`) pinned it exactly: the global
Esc-cancel added in 0.84.7 called `global_shortcut().unregister`/`.on_shortcut`
**inside `screen_record_open_overlay`**. When that runs from the record hotkey,
it executes within the global-shortcut event handler, which holds the plugin's
manager mutex — so registering/unregistering there re-entered the same mutex and
**deadlocked the main thread**. After triggering the screen recorder once, the
main run loop was hung forever: no global hotkey fired again (Ctrl+Space,
Ctrl+Shift+V did nothing), and the app beach-balled. Fix: arm the global Esc on
a worker thread, which waits for the handler to release the mutex instead of
re-entering it. The recorder + all hotkeys keep working.

## [0.84.9] — 2026-06-13

### Added — persistent file logging + crash capture

Bundled builds had no terminal, so all `tracing` output was lost and field
hangs/crashes were undiagnosable. Now the app writes a **daily-rolling log
file** to `<data dir>/InspectorRust/logs/inspector-rust.log` (macOS:
`~/Library/Application Support/InspectorRust/logs/`) in addition to stderr, at
`info` by default (`RUST_LOG` overrides). A **panic hook** records crashes — with
thread + source location — to both the rolling log and a dedicated `crash.log`
written synchronously so the trace survives an immediate abort. Key
interactions (popup show/hide, hotkeys, recording, etc.) are logged as
breadcrumbs. New `logging` module; `tracing-appender` dependency.

## [0.84.8] — 2026-06-13

### Fixed — popup (Ctrl+Space) opened on the wrong monitor

`show_and_position` resolved the cursor's monitor via `pick_cursor_monitor`,
which used `WebviewWindow::cursor_position()` — stale on the popup window (it
only refreshes when that window receives a mouse event). So when the cursor was
on a secondary monitor, the popup centered on the **primary** and looked like
"Ctrl+Space doesn't open" (it was appearing on another display). `pick_cursor_monitor`
now uses the same global cursor query (`CGEventGetLocation`, point-space /
mixed-DPI aware) the screenshot preview and record overlay use, so the popup
opens on the monitor the cursor is actually on. Verified the show/toggle path
itself is healthy (a clean launch + one toggle puts the popup on screen).

## [0.84.7] — 2026-06-13

### Fixed — recorder region selection on a secondary monitor (root cause)

The real bug: `screen_record_open_overlay` picked the cursor's monitor with
`WebviewWindow::cursor_position()`, which on a **freshly-built** window is stale
(it only updates once that window receives a mouse event) — so it always
resolved to the primary monitor, and the overlay never moved to the secondary.
Now it uses the same **global** cursor query the screenshot preview uses
(`pick_cursor_monitor_globally` → `CGEventGetLocation`, point-space bounds-check
that handles mixed-DPI). The overlay geometry is also **re-applied ~90 ms after
show** because `set_size(PhysicalSize)` converts through the window's current
scale factor, which lags a move to a different-scale display (Retina ↔
non-Retina) and would otherwise leave the overlay half/double sized.

### Added — Esc aborts region selection globally

Esc now cancels the recording region selection from anywhere via a temporary
**global Esc shortcut** registered while the overlay is open (disarmed on
cancel / record-start) — you no longer have to click into the transparent,
focus-less overlay first. (Screenshot region selection on macOS already cancels
with Esc natively via `screencapture -i`.)

## [0.84.6] — 2026-06-13

### Fixed — recorder selection on a secondary monitor (mixed-DPI)

v0.84.5 made the select overlay span the whole virtual desktop, but on macOS a
single window can't reliably span monitors with different scale factors (a
Retina primary + a non-Retina external): the overlay only partially covered the
secondary screen. The overlay now covers **the monitor under the cursor**,
sized from that one monitor's self-consistent physical position+size — so the
selection fills the whole screen on any monitor. Move the cursor to the screen
you want before triggering. (The region is still mapped to the correct display
for capture, as in 0.84.5.)

## [0.84.5] — 2026-06-13

### Fixed — screen recorder: select a region on any monitor

The region selection couldn't reach all monitors, and a region wasn't mapped to
the right display. Now:

- The select overlay covers the **entire virtual desktop** (bounding box of all
  monitors, re-applied after `show` for macOS reliability), so the marquee can
  be drawn on any screen.
- The selected region is converted to **absolute virtual-desktop coordinates**
  (overlay window position + marquee). Windows (`gdigrab`) and Linux (`x11grab`)
  capture the whole desktop with absolute offsets, so any monitor records
  directly.
- macOS `avfoundation` records one display at a time, so the region is mapped to
  the display it lands on (CoreGraphics `CGGetActiveDisplayList`/`CGDisplayBounds`
  + the unit-tested `pick_display_for_region`), that display's `Capture screen N`
  device is selected, and the crop is taken relative to that display. Single-
  monitor behaviour is unchanged. (Mixed-DPI multi-monitor layouts assume a
  uniform scale factor and may be slightly off; macOS multi-display capture is
  runtime-unverified on this build host.)

## [0.84.4] — 2026-06-13

### Fixed — edited screenshots vanished from Downloads after saving

`editor_save` wrote the annotated PNG to `~/Downloads`, then re-pointed the
preview's pending entry at that file and re-showed the preview. Dismissing the
preview (its Close button, Pin-to-screen, or auto-hide) runs
`screenshot_preview_discard`, which `remove_file`d the pending path — so the
just-saved Downloads file got deleted moments later. Edited screenshots now
persist: `Pending` carries a `saved` flag (true once the file lives in
Downloads), discard only deletes unsaved temp captures in the cache dir, and
re-saving an already-saved file no longer needlessly renames it.

## [0.84.3] — 2026-06-13

### Fixed — `freeze` (input lock) only worked once per session

The macOS input-lock event tap was enabled exactly once (inside the
install-guarded path) and the callback didn't handle the OS disabling it.
macOS auto-disables a `CGEventTap` on callback-timeout / heavy user input
(`kCGEventTapDisabledBy{Timeout,UserInput}`), and a disabled tap stays dead
until `CGEventTapEnable(true)` is called again — so `freeze` locked the first
time but, after unlocking, every later invocation flipped the lock flag on yet
intercepted nothing. Fix: the tap port is now remembered (`TAP_PORT`) and
**re-enabled on every `start_input_lock`**, plus the callback re-arms the tap
when it receives a disable event. Locking now works on every invocation.

## [0.84.2] — 2026-06-13

### Performance — faster launch + snappier popup open

- **App init no longer blocks on the app-launcher scan.** The startup `setup`
  closure ran `app_launcher::scan()` synchronously (a ~20-100 ms walk of the
  app directories), delaying when the app — and the popup hotkey — became
  ready. The scan now runs on a background thread and fills the (already-managed,
  initially-empty) index when done; `refresh_apps` still re-triggers it.
- **The popup opens without the corner-flash-then-snap.** `show_and_position`
  parked the hidden window at a quarter-monitor offset, showed it, then moved
  it to centre — so the window visibly appeared off-centre and jumped. It now
  parks at the final centred position whenever the window size is already known
  (every open after the first), so it appears directly in place; the post-show
  clamp still corrects the first-ever open.

## [0.84.1] — 2026-06-13

### Changed — per-OS command gating + frontend performance

- **Commands gate by platform.** A `CommandSpec` can declare which OSes it works
  on; `isCommandAvailable` filters both the runnable command and the suggestion
  list (App.tsx). Commands whose backend doesn't exist on the current OS no
  longer surface — `freeze` only on macOS; `touch`/`mkdir`/`terminal`/`md2pdf`
  only on macOS + Windows — so the user can't trigger a guaranteed failure. On
  macOS nothing changes (every command is available). The pure parsers stay
  platform-agnostic (unit-tested); gating is render-layer only.

### Performance

- **`combined` list is memoised** (App.tsx) — it was rebuilt on every render
  (incl. unrelated state like toasts / focus), handing a fresh array to the
  history list + virtualizer each time. Now recomputes only when the list
  actually changes.
- **`parseCommand` runs once per keystroke, not three times** — `appEntry` and
  `pwgenEntry` reuse the memoised `parsedCommand` instead of re-parsing.
- **PreviewPanel image `src` + themed HTML `srcDoc` are memoised** on the entry,
  so the multi-MB base64 concat and the `getComputedStyle` + template assembly
  no longer run on every parent render — only when the selection changes.
- **Color preview copies via the Tauri clipboard plugin** instead of
  `navigator.clipboard` (which can fail silently in the WKWebView).
- **BpmDetector UI strings translated to English** (the rest of the app is EN).

## [0.84.0] — 2026-06-13

### Added — cross-platform feature parity (Windows + Linux)

Features that were macOS-only / stubs now have real Windows and/or Linux
implementations. macOS is unchanged; the new Windows paths are **compile-clean
but runtime-unverified**, the Linux paths are testable on a Linux box (pure
arg/parser logic is unit-tested on every platform).

- **`kill` works on Windows.** `kill_process_by_pid` now uses `sysinfo`'s
  cross-platform kill (Windows `TerminateProcess`) instead of a stub — the kill
  picker's action actually terminates on Windows.
- **Linux system commands.** `reboot` → `systemctl reboot`, `shutdown` →
  `systemctl poweroff`, `lock` → `loginctl lock-session` (with
  `xdg-screensaver` / GNOME / Cinnamon fallbacks), `mute` + volume → `wpctl`
  (PipeWire) then `pactl` (PulseAudio). Each tries a list of tools and reports
  which it attempted on failure.
- **Linux audio output picker (`sound`/`audio`).** Lists sinks via `pactl list
  sinks`, reads the default from `pactl get-default-sink`, switches with
  `pactl set-default-sink` (works on PipeWire via pipewire-pulse). Sink parser
  is pure + unit-tested.
- **Linux wakelock under Wayland.** A logind idle+sleep inhibitor
  (`systemd-inhibit … sleep infinity`) is now the primary keep-awake on Linux —
  it actually prevents idle/lock/sleep under Wayland, where the old cursor
  jiggle was a silent no-op. The jiggle remains a complement on X11.
- **Cross-platform timer notifications.** A timer firing while the popup is
  hidden now shows a real OS notification + sound on **Linux** (`notify-send` +
  `canberra-gtk-play`/`paplay`) and **Windows** (WinRT toast + system sound via
  PowerShell), not just macOS.
- **App launcher on Windows + Linux.** Linux scans XDG `.desktop` entries
  (`/usr/share/applications`, `~/.local/share/applications`, Flatpak exports;
  honours `Hidden`/`NoDisplay`/`Type`) and launches via `gtk-launch` (Exec
  fallback). Windows scans the Start-Menu `.lnk` shortcuts and launches via the
  shell. Desktop-entry parsers are pure + unit-tested.
- **Linux screen recording.** ffmpeg `x11grab` (region via `-video_size` +
  offset) + PulseAudio (default-sink monitor for system, default source for
  mic, mic gain-boosted, `amix` for both). Arg-builder is pure + unit-tested.
  X11 / XWayland only; a pure-Wayland session without XWayland gets a clear
  error.

## [0.83.1] — 2026-06-13

### Fixed — audit-driven correctness & resource-safety pass

A code audit surfaced several real bugs (independent of platform); all fixed,
all green on macOS (`cargo test` + `pnpm test`):

- **Screen-record pause no longer blocks for up to 5 s.** `screen_record::pause`
  held the session mutex across `finalize_child` (which waits up to 5 s for the
  MP4 trailer to flush), stalling any concurrent `resume`/`stop`/`is_recording`.
  It now takes the child out under the lock, finalizes with the lock released,
  then re-locks briefly — the same pattern `stop` already used.
- **No more UI freeze on text-expansion.** `trigger_expand_at_cursor` /
  `diagnose_expand_at_cursor` ran a 250 ms focus-settle `sleep` *inside* the
  main-thread closure, freezing the AppKit run loop on every expansion / "Test
  now". The sleep now runs on a worker thread; only the enigo synthesis is
  dispatched to the main thread.
- **No startup panic on a missing tray icon.** The tray builder `.unwrap()`-ed
  the default window icon; a stripped/misconfigured bundle would panic at launch.
  It now falls back to an icon-less tray with a warning.
- **Clipboard auto-clear no longer accumulates threads.** The per-copy timer
  slept the full (up to 3600 s) window; under rapid copying that piled up
  sleeping threads. It now sleeps in 1 s chunks and exits within ~1 s once a
  newer copy supersedes it.
- **OCR / text-transform history failures are logged, not swallowed.**
  `db::upsert_clip` errors were discarded with `let _ =`, silently losing the
  history entry; they now `warn!`.
- **Windows auto-expand hook can recover.** If `SetWindowsHookExW` failed, the
  `THREAD_STARTED` flag stayed set and every later `install()` silently no-op'd;
  the flag now resets on failure so a retry can re-install. *(Windows
  runtime-unverified.)*
- **Orphan-cleanup `pkill` constrained to ffmpeg** (`ffmpeg.*InspectorRust/recordings`)
  so it can't match an unrelated process referencing that path.
- **Frontend listener & timer leaks.** Five Tauri event listeners
  (`usePauseOnPopupHidden`, `ScreenshotPreview`, `HistoryList`,
  `ColorPickerModal`, `StatusToast`) could orphan if the component unmounted
  before `listen()` resolved — now guarded with a `cancelled` flag. The
  `HistoryItem` "Saved!" timeout and `BrightnessPanel` debounce timers are now
  cleared on unmount. `ScreenshotPreview`'s 200 ms cursor poll only runs while a
  shot is staged. `RecordOverlay` re-arms its one-shot start guard so a retry
  after an ffmpeg error actually records.
- **Windows recording graceful-stop** via `CTRL+BREAK` to ffmpeg's own process
  group (`CREATE_NEW_PROCESS_GROUP`) so the MP4 trailer flushes cleanly on stop.
  *(Windows runtime-unverified.)*

## [0.83.0] — 2026-06-09

### Added — second, configurable "clipboard history" hotkey

The popup can now be opened by a **second global shortcut** (default
**`Ctrl+Shift+V`**) in addition to the main `Ctrl+Space`, so you can bind a
dedicated clipboard-history key. Both hotkeys are now configurable in
**Settings**: *Popup hotkey* (existing) + the new *Clipboard-history hotkey*
(with presets, Reset, and a Disable button — an empty value turns the second
hotkey off). Both are validated against each other and the reserved global
shortcuts. The **Features tab** now names them clearly ("Open app / clipboard
history" + "Clipboard history (2nd hotkey)", shown only when set) and loads the
second hotkey live. IPC: `get_/set_/get_default_history_hotkey`.

## [0.82.8] — 2026-06-09

### Fixed — recording played too fast + mic too quiet

- **Video timebase locked to CFR (`-r 30`).** The avfoundation screen input
  reports an undefined "1000k fps" nominal rate (it's event-driven), leaving the
  output with an irregular timebase that can play back too fast in some players
  and makes the pause/resume concat unreliable. Forcing constant 30 fps on the
  output gives real-time playback and clean concatenation. (In controlled tests
  the raw capture already measured real-time — the deficit was avfoundation
  startup latency — but the CFR lock removes the irregular-timebase failure mode
  and is the standard fix.)
- **Mic boosted +10 dB.** macOS built-in mics record well below line level
  (measured), so the mic input now gets a `+10 dB` gain (`volume=`), applied only
  to the microphone — system/loopback audio is left at its proper level. Works
  for mic-only and the system+mic mix.

## [0.82.7] — 2026-06-09

### Added — `audio` as an alias for `sound`

Typing **`audio`** now opens the same output-device picker as `sound` (hidden
alias, so it doesn't clutter the autocomplete list).

## [0.82.6] — 2026-06-09

### Verified + clarified — recording audio

Tested the macOS audio paths end-to-end (3 s captures + ffprobe/volumedetect):
**mic** records real audio (aac 48 kHz; measured signal, not silent) and **both**
mixes correctly via `amix` (mic audible in the output). **System audio** produces
a valid stream but is **silent unless the system output is actually routed
through the loopback device** (BlackHole is a virtual cable — it only carries
what's sent to it; use a Multi-Output Device). That's inherent to
avfoundation/ffmpeg system-audio capture, not a bug. The record overlay's hint
now spells this out so "System audio → silence" isn't mistaken for a failure.

## [0.82.5] — 2026-06-09

### Fixed — Windows orphan-ffmpeg cleanup was broken

The v0.82.4 Windows branch of `cleanup_orphans` used `taskkill` with a
`WINDOWTITLE` filter — ffmpeg console processes have no window title matching our
cache path, and the marker used `/` not `\`, so it matched nothing. Rewrote it to
PowerShell, killing any `ffmpeg.exe` whose `CommandLine` references our cache
(separator-agnostic `*InspectorRust*recordings*`), with `CREATE_NO_WINDOW` so no
console flashes. Compile-validated for `x86_64-pc-windows-gnu`; still
runtime-unverified on a real Windows box (like the rest of the Windows record
path).

## [0.82.4] — 2026-06-09

### Fixed — Stop/Pause/Resume froze the UI; orphaned ffmpeg ate CPU

- **UI froze, buttons didn't respond.** `stop`/`pause`/`resume`/`start` were
  synchronous `#[tauri::command]`s, which Tauri runs **on the main thread**. Each
  blocks for seconds (ffmpeg finalize waits up to 5 s, device re-listing ~0.5 s,
  lossless concat), freezing the whole UI so Stop/Pause/Resume appeared dead.
  They're now `async`, so Tauri runs them off the main thread — the bar stays
  responsive.
- **Orphaned recording ffmpeg.** When a stop failed (or the app crashed), the
  ffmpeg child kept capturing at high CPU forever (the "bad performance"). Added
  `screen_record::cleanup_orphans()` at startup — it kills any ffmpeg writing to
  our segment cache and clears stale segments (matched by our cache path, so it's
  unambiguously ours), mirroring the wakelock orphan-reaper.

## [0.82.3] — 2026-06-09

### Fixed — recording stop bar was hidden behind the Dock

The stop bar was positioned against the full monitor height, so its bottom-centre
spot sat behind the macOS Dock. It's now placed against the monitor's **work
area** (`Monitor::work_area()`, which excludes the Dock + menu bar), 12 pt above
the bottom edge of the visible region.

## [0.82.2] — 2026-06-09

### Fixed — recording never actually started (the *real* real cause)

The stop bar wasn't appearing because **the recording never started** — a
frontend bug in the countdown overlay. When the countdown hit 0, the same
`useEffect` both did `setPhase("starting")` and scheduled the
`setTimeout(beginRecording, 150)`; because `phase` is in that effect's deps,
`setPhase` tore the effect down and ran its cleanup (`clearTimeout`) **before
the timer fired**, so `beginRecording` (hence `startScreenRecord`) was never
called and the overlay sat invisible in the "starting" phase. Split into two
effects — a countdown ticker and a one-shot start guarded by a ref — so the
recording reliably starts and the stop bar appears. (The v0.82.1 worker-thread
build fix is still correct and stays.)

## [0.82.1] — 2026-06-09

### Fixed — recording stop bar STILL never appeared (the real cause)

The v0.82.0 fix was wrong: it dispatched the stop-bar window build to the main
thread, but a synchronous `#[tauri::command]` already runs **on** the main
thread, and calling `WebviewWindowBuilder::build()` there **deadlocks** (the
build needs the main-thread event loop to pump, but the command is blocking it).
The stop bar is now built from a **worker thread** (`std::thread::spawn`), the
same proven pattern as the screenshot editor/preview — Tauri then marshals the
window creation onto the event loop cleanly. The overlay is still closed on the
main thread (closing isn't a build, so it's safe).

## [0.82.0] — 2026-06-09

### Fixed — recording stop bar never appeared

The floating stop bar was built from a Tauri command-worker thread; on macOS,
creating an `NSWindow`/webview off the main thread is unreliable, so it silently
never showed. Window open/close for the recording flow now runs on the **main
thread** (`run_on_main_thread`), so the bar appears reliably.

### Added — pause / resume a recording

The floating bar now has a **Pause/Resume** button next to **Stop**, and the
elapsed timer freezes while paused. Because ffmpeg can't truly pause a live
capture, pause is implemented as **segment + concat**: each contiguous run is a
temp segment; Stop concatenates them losslessly (`-c copy`, no re-encode) into
the final MP4. A never-paused recording is just moved to the output (no concat).

### Changed — record hotkey is now `Ctrl+Shift+Alt+S` (⌃⇧⌥S)

Was `Ctrl+Shift+R`. The extra Alt keeps it clearly distinct from `Ctrl+Shift+S`
(screenshot region).

## [0.81.1] — 2026-06-09

### Fixed — `meme` command found nothing on Windows; meme folder now configurable

The meme library path was a hard-coded macOS path (`/Users/martin/My
Drive/media/memes`), so on Windows the `meme` command scanned a non-existent
folder and found nothing.

- The default is now **home-relative** (`~/My Drive/media/memes`) so it resolves
  per-user on every OS.
- **Settings → Meme library** lets you point the picker at any folder (with a
  Browse… picker). On Windows with Google Drive in *streaming* mode the library
  lives under a drive letter (e.g. `G:\My Drive\media\memes`); set it there. A
  blank value resets to the default. IPC `get_meme_dir` / `set_meme_dir`.
- The Windows/Linux asset-protocol scopes now include the default meme location
  so animated previews render there too. (A custom folder still lists and copies
  memes; only the in-app animated preview needs the scoped location.)

## [0.81.0] — 2026-06-09

### Added — screen recording (`Ctrl+Shift+R`)

Record a screen region to an **MP4 (H.264)**, the same workflow on **macOS and
Windows 11**. Press **`Ctrl+Shift+R`** → a fullscreen overlay lets you **drag a
region** → a small panel lets you choose which **audio tracks** to capture
(**System** and/or **Microphone** and/or **none**) → **Record** starts a **3-second
countdown** → recording begins. A floating **stop bar** (pulsing red dot + elapsed
timer + Stop button) sits at the bottom of the screen; Stop finalises the MP4 to
**Downloads** and reveals it in Finder/Explorer.

- **Engine: ffmpeg** (the one engine that gives an identical cross-platform
  workflow + MP4). macOS uses `avfoundation` (screen + device audio, region via
  `crop`); Windows uses `gdigrab` (region offsets) + `dshow` audio. Two audio
  tracks are mixed with `amix`. Output is `libx264 -preset ultrafast -pix_fmt
  yuv420p -movflags +faststart`.
- The pure ffmpeg-argument builders and avfoundation/dshow device parsers in
  `screen_record.rs` are unit-tested (8 tests).
- **Requirements / caveats:** ffmpeg must be installed (macOS: `brew install
  ffmpeg`; the overlay shows an install hint via the `record.no_ffmpeg`
  sentinel). System audio needs a loopback device (macOS: BlackHole). The
  Windows path is compile-validated; runtime verification on a real Windows box
  is pending. On macOS the OS may prompt ffmpeg for its own Screen-Recording
  permission the first time.

## [0.80.0] — 2026-06-09

### Added — `sound` command: pick the audio output device

Type **`sound`** and Enter to get an inline output-device picker in the preview
column (same arrow-key model as `brightness`): **↑/↓** select, **Enter** switches
the system default output, **Esc** closes. Mirrors the macOS Sound pane's Output
list.

- **macOS:** CoreAudio — enumerates output devices, marks the default, switches
  it. Tested live.
- **Windows:** MMDevice enumeration + the `IPolicyConfig` COM object to switch
  the default (the standard approach, since Windows has no public "set default
  device" API). Compile-validated against the `windows` 0.61 bindings; runtime
  verification on a real Windows box is pending.

## [0.79.2] — 2026-06-08

### Fixed — `kill` and `meme` picker rows now get the red highlight too

Completing the uniform command tint: the whole-list command pickers (`kill` /
`meme`, including with a parameter like `kill slack` or `meme cat`) now share
the reddish command styling. Every keyword-triggered command row is now
consistently red; only expression results (calc/color) and non-command rows
(app/finder/clip/snippet) stay neutral. (The `kill` chip keeps its more-alarming
`red-500` since it's destructive.)

## [0.79.1] — 2026-06-08

### Fixed — All keyword-commands now get the red highlight

Several command rows reached by typing a keyword — `otp`, `pwgen`, `bruno`,
`bpm` — were rendered in the neutral accent colour instead of the reddish
command tint, so they looked like plain results rather than commands.
`HistoryItem`'s `isCustomCommand` now covers every keyword-command row
(`command`, `command-suggestion`, `2fa`, `otp`, `pwgen`, `bruno`, `bpm`) — chip,
icon, and row background. Expression results (`calc`/`color`) and whole-list
pickers (`kill`/`meme`) intentionally keep their own styling.

## [0.79.0] — 2026-06-08

### Added / Fixed — Windows `touch` / `mkdir` / `terminal` in the Explorer folder

- **`terminal` now works on Windows** (it previously returned a macOS-only
  error): opens **Windows Terminal** (`wt.exe -d <dir>`), falling back to
  PowerShell then `cmd.exe`, each in a fresh console window with the working
  directory set to the active Explorer folder.
- **`touch` / `mkdir` more reliable**: when the precise frontmost-HWND / active
  tab match misses, `front_dir` now falls back to *any* open Explorer folder
  (`first_explorer_path`) before resorting to the Desktop — so the new item
  lands in an Explorer folder far more often.

(Builds on the pulled v0.79.0 Windows Explorer work — encrypted backup, TOTP /
settings export, Win11 tabbed-Explorer selection, EN locale.) Windows paths are
compile-validated against the `windows` 0.61 bindings; runtime verification on a
real Windows box is still pending.

## [0.78.0] — 2026-06-08

### Fixed — Keep-awake (`caffeine` / `wakelock`) could keep the Mac awake forever

`caffeinate` is now spawned as `-disu -w <ir-pid>`, tying its lifetime to
Inspector Rust: it exits the instant IR does (clean quit, crash, or a
reinstall). Previously the child was reparented to launchd on an unclean exit
and kept the Mac awake **forever**, unreachable by a later `caffeine off`. A
startup sweep (`cleanup_orphans`) also reaps any such pre-fix orphan.

### Fixed — `brightness` from a partial suggestion

Pressing Enter on the `brightness` autocomplete hint (while the field still read
e.g. `bright`) used to flip into brightness mode and immediately back out. The
command now canonicalises the query so it sticks.

### Changed — Features tab now lists everything

Audited the Features reference against the full feature set and added the
missing entries: inline calculator, unit/base/epoch converters, colour
converter, the six DE↔IT/ES/PL translate commands, web-search bangs, dev tools
(`uuid`/`slug`/`hash`/`json`/`jwt`), `qr`, `meme`, plus the in-popup actions
formatted paste, volume keys, pin-to-top, smart preview actions, and delete.
(The hidden `opener` easter egg is intentionally listed nowhere.)

## [0.77.0] — 2026-06-08

### Added — `kill` by PID

`kill <pid>` now targets a process by its exact PID (e.g. `kill 1234`,
`kill -9 1234`), in addition to the existing name/exe substring match. The
matched process floats to the top of the picker and is still shown with its
name + the confirm dialog before being killed.

## [0.76.1] — 2026-06-07

### Fixed — Features-tab search bar overlapped content while scrolling

The new search bar was a `sticky` element inside the scrolling list, so content
scrolled visibly behind it. It's now a separate fixed header above the scroll
area (with a divider), so it can never overlap the content.

## [0.76.0] — 2026-06-07

A big "Swiss-Army-knife" feature drop — all of **Tier 1** from the feature
roadmap, plus three UX fixes.

### Added — Web-search bangs

`g` · `ddg` · `gh` · `yt` · `npm` · `crates` · `so` · `mdn` · `wiki` `<query>`
open that site's search in the browser. Data-driven from a single table.

### Added — Dev quick-tools

`uuid [n]` · `slug <text>` · `hash <text>` (SHA-256) · `json` (pretty-print
clipboard JSON) · `jwt` (decode clipboard JWT) — all land on the clipboard.

### Added — Inline converters

The calculator box now also does unit conversions (`5 km in mi`, `72 f to c`,
`2 gb in mb` — length/mass/data/time/speed/temperature), number bases
(`0xff in dec`, `255 in hex`), and epoch→date (`1717000000 as date`).

### Added — QR codes (`qr <text>`)

Renders a QR live in the preview; Enter copies the PNG to the clipboard.

### Added — Smart preview actions

A text clip now offers one-tap buttons: **Open link** (URL/domain),
**Compose email**, **Call** (`tel:`), **Open in Maps** (`lat,lng`), and
**Make QR** for any short value.

### Added — Pinned clips

A pin ★ toggle on each history row floats the clip to the top and exempts it
from the 1 000-row prune.

### Added — Clipboard privacy

Settings → **Clipboard privacy**: never capture from listed apps (password
managers), and auto-clear the clipboard N seconds after a copy (opt-in).

### Added / Fixed — UX

- **Features tab** now has a **search bar** to filter the function reference.
- **Settings tab** fills the full width at the *large* window size (no more
  empty side margins).
- **Games**: leaving a running game by clicking outside the popup now re-arms
  the "press a key to continue" gate, exactly like pausing with Esc.

### Tests

974 → **1040 unit tests** (392 Rust + 648 frontend), incl. the new devtools,
converters, QR matrix, smart-action detector, pinned-clip prune/ordering,
app-exclusion matcher, and earlier RFC 6238 TOTP vectors.

## [0.75.0] — 2026-06-07

### Added — Six more translate commands (German ↔ Italian / Spanish / Polish)

`trde2it` · `trit2de` · `trde2sp` · `trsp2de` · `trde2pl` · `trpl2de` open
Google Translate for German↔Italian, German↔Spanish, and German↔Polish (same as
the existing `tren` / `trde` / `tr`). All translate commands are now data-driven
from a single `TRANSLATE_LANGS` table, so a new language pair is one map entry +
one catalogue row.

### Tests

25 new unit tests: the six new translate commands + `TRANSLATE_LANGS`/
`isTranslateKind` integrity, keyword-prefix collision guards, command-catalogue
guardrails, and the **RFC 6238 TOTP reference vectors** (SHA1/SHA256/SHA512) via
a new time-injectable `totp_store::generate_at`. 386 Rust + 588 frontend pass.

## [0.74.0] — 2026-06-07

### Added — Space Invaders is back, as `spacer`

The hidden Space Invaders game returns under a new trigger word: type **`spacer`**
(the old `space` was retired in v0.73.0). Same IR treatment as the other
easter-egg games — intro animation, Esc to pause/resume, persistent high score.

### Changed — `terminal` (and partial command names) outrank the app launcher

Command suggestions now rank **above** app-launcher hits, so typing `term`
surfaces the `terminal` command (open a terminal in the current Finder folder)
above Terminal.app — you no longer have to type the whole word to reach it.
Complete commands already won the top slot; this extends it to partial matches.

## [0.73.1] — 2026-06-07

### Changed — `pwgen` autocomplete inserts a trailing space; default length 12

Completing the `pwgen` suggestion (Enter / Tab / →) now fills `pwgen ` **with a
trailing space**, so you can type the length straight away. The same applies to
every argument-taking command. The default password length is now **12** (down
from 20) and is never shown in the input — completing a bare `pwgen` just
generates a 12-character password.

## [0.73.0] — 2026-06-07

### Changed — Brightness can now dim to 5%

The software-dimming floor dropped from 10% to **5%** (`MIN_PERCENT`), so a
monitor can be dimmed further while still staying recoverable. The inline
sliders' ←/→ steps reach 5%.

### Removed — Space Invaders hidden game

The `space` Space Invaders easter egg was removed (`SpaceInvadersGame.tsx`,
`lib/space-invaders.ts`, its CSS flourishes, and the `isSpaceInvadersTrigger`
trigger). Four hidden games remain: Pong (`getshaky`), Snake
(`rockthebox`/`rockthabox`), and Flappy Bird (`learningtofly`).

## [0.72.1] — 2026-06-07

### Changed — Brightness control is now inline in the popup (no separate window)

The brightness sliders no longer open a separate floating window (whose webview
didn't reliably load the monitor list — sliders never appeared). Pressing
**Enter** on the `brightness` row now renders the sliders in the **right preview
column** and keeps the popup open:

- **↑ / ↓** select a monitor
- **← / →** adjust the selected monitor's brightness (±5)
- **Enter** hands the arrow keys back to the left list
- **Esc** leaves brightness mode

This fixes "no slider is shown" and gives full keyboard control.

## [0.72.0] — 2026-06-07

### Fixed — Custom commands always win the top slot

Typing a command keyword that also happens to be an app name (e.g. `terminal`,
which fuzzy-matches Terminal.app) used to surface the **app-launcher** hit above
the custom command, so Enter launched the app instead of running the command —
that's why `terminal` opened Terminal.app in the home directory instead of the
custom "open a terminal at the Finder folder" command. A complete custom command
(`commandEntry`) is now spliced to the very top of the result list, above every
app/opener/special row.

### Added — Reddish highlight for custom-command rows

Command and command-suggestion rows (`terminal`, `freeze`, `wakelock`, `tren`,
`kill`, …) now render with a reddish (`rose`) accent — chip, icon, and row
background (selected = solid rose) — so it's immediately obvious you're about to
trigger a command rather than paste a clip or launch an app.

### Changed — Monitor brightness: software dimming on macOS + Windows

The brightness feature (`brightness` / `bri`) was rewritten after confirming on
real hardware that pure DDC/CI doesn't work on Apple Silicon — an external
monitor through a DP/HDMI adapter returned `invalid DDC/CI length` for every
read and writes silently no-op'd.

- **macOS** now dims in **software via the CoreGraphics gamma table**
  (`CGSetDisplayTransferByFormula`). This works on **every** display — the
  built-in Liquid Retina panel *and* external/adapter-connected monitors — with
  no DDC and no extra permission. (Previously: DDC-only, external-only, and
  broken on Apple Silicon.)
- **Windows 11** gets the same software-dimming approach via
  `SetDeviceGammaRamp`, covering built-in + external monitors uniformly.
  (Runtime-unverified; written compile-clean against the `windows` 0.61 GDI
  bindings and validated with a Windows-target `cargo check`.)
- **Linux** keeps hardware DDC/CI (`ddc-hi`).

A `MIN_PERCENT=10` safety floor means the screen can never dim to unrecoverable
black; the overlay slider min is 10. Note: software dimming reduces emitted
light, not the backlight — it can only go darker than native, never brighter.

### Tests

20 new unit tests: `percent_to_gamma_fraction` + `gamma_ramp_entry` (Rust),
`sh_squote`/`osa_escape` quoting helpers, and custom-command priority
preconditions (frontend). 380 Rust + 568 frontend tests pass.

## [0.71.1] — 2026-06-06

### Fixed — `terminal` opens in the Finder folder

The `terminal` command now opens the terminal **in** the frontmost Finder
window's folder. Previously iTerm2 was launched but landed in the home
directory (`open -b … <dir>` doesn't `cd`). It is now driven via AppleScript
(`tell application "iTerm" … write text "cd <dir>"`) when iTerm2 is installed,
falling back to `open -a Terminal <dir>` (Terminal.app honours the folder arg).
New pure helpers `sh_squote` (POSIX single-quote) and `osa_escape` (AppleScript
string escaping) are unit-tested.

## [0.71.0] — 2026-06-06

### Changed — Paused games wait for a keypress before resuming

When a suspended game (Pong, Snake, Space Invaders, Flappy) is reopened, the
loop now stays frozen on a **"▸ Resumed — press a key to continue"** overlay
until the first keydown/click, so the player isn't dropped straight back into
live action.

## [0.70.0] — 2026-06-06

### Added — Meme picker (`meme`)

Browse a folder of GIFs/images from the search bar: type **`meme`** (then a
fuzzy query like `meme cat`) to filter the library, the selected meme **plays
animated** in the preview, and **Enter copies it** to the clipboard (on macOS
as a file-URL, so pasting into a chat app / Finder keeps the animation). The
library scans recursively (category sub-folders), default
`~/My Drive/media/memes`, overridable via the `meme.dir` setting.

**Two build variants:** the meme command is gated by a build flag — the
default build includes it; `pnpm build:{macos,win,linux}:nomeme`
(`VITE_IR_MEME=0`) produces a build without the command, where the folder is
never surfaced.

## [0.69.0] — 2026-06-06

### Added — Flappy Bird (hidden game, `learningtofly`)

A fifth hidden easter-egg game, in the IR style. Type **`learningtofly`** in
the search bar. Space / ↑ / W / click to flap, Esc to quit (saving a live run
to resume next time), Space/click to rematch. Faithful physics (gravity +
flap impulse, scrolling pipe pairs with a fixed gap, +1 per pipe, death on
pipe/ground, ceiling clamp), an intro flourish, per-game high score, and the
bird tilts with its velocity. New `lib/flappy.ts` (pure, 21 unit tests) +
`components/FlappyGame.tsx`.

## [0.68.1] — 2026-06-06

### Fixed — `brightness` / `clean` / `shot*` / `random` showed no runnable row

The command-row builder (`App.tsx` `commandEntry`) was missing `case`s for the
screenshot, clean, brightness and random commands, so typing the full keyword
fell through to `default: return null` — no action row appeared and the command
could only be triggered while a fuzzy autocomplete suggestion was still showing.
Added the missing cases (+ the matching `CommandEntryView` kinds) so these
commands surface a runnable row like every other command.

## [0.68.0] — 2026-06-06

### Added — `rnd` / `random` command

Roll a random number, shown big in the on-screen toast (which lingers a bit
longer for this one). `rnd` = 1–6, `rnd 100` = 1–100, `rnd 5 500` = 5–500
(bounds swap if reversed). CSPRNG with rejection sampling, no modulo bias.

### Fixed — brightness overlay didn't open

`brightness` called `hide_popup`, which on macOS does `app.hide()` — that
deactivated the whole app, so the freshly-shown overlay never came to the
front (triggering the command appeared to do nothing). Now it hides only the
popup window and always rebuilds a fresh overlay (so monitors are re-probed
each open). External DDC monitors only; the built-in MacBook panel still
reports "no DDC-capable monitors".

## [0.67.0] — 2026-06-06

### Changed — Default popup hotkey is now `Ctrl+Space`

The popup-open shortcut default changed from `Ctrl+Shift+V` to **`Ctrl+Space`**
(everywhere: code default, tray, Settings presets, CLI/Linux shortcuts, docs).
A one-time migration bumps un-customised installs automatically; a custom
hotkey is left untouched. You can still change it in Settings → Popup hotkey.
Note (macOS): `Ctrl+Space` is also the system "previous input source"
shortcut — free it up in System Settings or pick another hotkey if it doesn't
open. (+4 tests: 877 → 881.)

## [0.66.1] — 2026-06-06

### Tests

Added 34 unit tests (843 → 877 total): `matchTotpEntries` fuzzy ranker (TS),
`fuzzyScore` command ranking (TS), `finder_selection::sanitize_name`
path-escape rejection (Rust), `brightness` percent↔VCP monotonicity +
round-trip bounds (Rust), and the editor window-size string parser (Rust).

## [0.66.0] — 2026-06-06

### Fixed — Screenshot editor: adding text

Placing a second text by clicking no longer closes the fresh input. In
WKWebView `mousedown` fires before the old input's `blur`; the click on
the canvas now suppresses that stray blur (`preventDefault`) so consecutive
text placement works.

### Added — Screenshot editor remembers its window size

The editor window's size is saved (debounced, `screenshot.editor_size`) and
restored on the next open.

## [0.65.0] — 2026-06-06

### Added — Edit the generated password (`pwgen`)

The pwgen preview shows the password in an editable field — type to tweak,
Enter copies, Esc done. Edits are keyed to the current generation so a
reroll / mode-switch / length-change discards a stale edit.

## [0.64.0] — 2026-06-06

### Changed — Abbreviation hotkey (`Alt+1`) now works everywhere, incl. terminals

When the abbreviation expander is enabled, a passive keystroke tracker
remembers the word you just typed, so the hotkey expands it from that
buffer (blind-Backspace + paste) **without reading the focused field** —
so it now works in terminals (iTerm2, Terminal.app, …) too. The AX/UIA
in-place paths remain as a fallback. The passive monitor is armed when
either the hotkey **or** automatic auto-expansion is on.

## [0.63.0] — 2026-06-06

### Added — Screenshot editor: copy + nicer arrows

- **Cmd/Ctrl+C** copies the *edited* screenshot to the clipboard (toolbar
  "Copy" button too), staying in the editor.
- Arrows redrawn with a concave-back head + round joins (CleanShot look).

## [0.62.1] — 2026-06-06

### Fixed

- Footer keep-awake LED now reconciles on every popup open, so `caffeine on`
  (and `wakelock on`) reliably show the status.

## [0.62.0] — 2026-06-06

### Added — Monitor brightness (`brightness` / `bri`)

Adjust the brightness of every DDC/CI-capable monitor — including
secondary/external ones — from a slider overlay (one per monitor + an
"all" master), Lunar / TwinkleTray style. Uses VCP feature `0x10` via
`ddc-hi` on macOS / Windows / Linux. Pure percent↔VCP mapping unit-tested;
DDC writes debounced. Internal laptop panels (no DDC) are a follow-up.

## [0.61.0] — 2026-06-06

### Added — Windows parity (first tranche)

- System `reboot` / `shutdown` / `lock` and `mute` / volume now work on
  Windows (`shutdown` CLI, `rundll32 LockWorkStation`, multimedia VK keys).
  IPC contracts unchanged. Compile-clean; runtime-unverified on real hardware.
- `docs/reports/WINDOWS_PARITY.md` audit (feature matrix + remaining gaps).

## [0.60.0] — 2026-06-06

### Added — Cleaning workflow (`clean`)

Delete cache/log/temp files with hard safety rails: strict per-OS allowlist,
canonicalise + containment check before every delete, symlinks never followed,
dry-run preview → confirm → re-validated execute. Conservative opt-in levels
(Safe/Standard/Aggressive) + age filter. Settings UI + 14 safety unit tests.

## [0.59.0] — 2026-06-06

### Added — Screenshot pin-to-screen

Float a capture as its own persistent, draggable, always-on-top window
(multiple pins coexist; close per pin).

## [0.58.0] — 2026-06-06

### Added — Screenshot editor parity

New annotation tools: line, ellipse, redact (opaque block), and
auto-numbered step badges. Geometry extracted to a unit-tested pure module.

## [0.57.0] — 2026-06-06

### Added — Screenshot capture modes

Full-screen and active-window capture, a self-timer, and repeat-last — all
feeding the same floating preview. Search commands `shot [n]` / `shotfull` /
`shotwin` / `shotlast`.

## [0.56.0] — 2026-06-06

### Added — Passive auto-expansion (aText-style)

A fourth text-expansion mode that needs **no hotkey**: a system-wide
keystroke monitor expands snippet abbreviations automatically as you
type, in any app — the way aText / TextExpander / Espanso work.

- **macOS** uses an active `CGEventTap` on the main run loop (the same
  raw-FFI pattern as the input-lock feature); **Windows** a
  `SetWindowsHookEx(WH_KEYBOARD_LL)` low-level keyboard hook (compiled
  cross-platform; Windows runtime still to be verified on real hardware).
  Linux stays on the clipboard-paste fallback (no rootless Wayland tap).
- **Trigger options:** expand after a delimiter (space / punctuation —
  the default, avoids accidental expansion) or *immediately* once the
  abbreviation is fully typed. Plus match-case, "expand inside words",
  and single-Backspace **undo** (restores the abbreviation).
- **Safe by design:** never expands in password / secure fields, ignores
  its own synthetic keystrokes, and clears its buffer on focus change,
  click and navigation keys. Dynamic placeholders (`{date}`,
  `{clipboard}`, `{cursor}`) work in this mode too.
- New module `auto_expand.rs` with a fully unit-tested pure core
  (ring-buffer matcher + trigger state machine); settings under
  `expander.auto_expand_*`; IPC `get/set_auto_expand_config`; a new
  "Auto-Expansion (aText-Stil)" section in Settings. The three existing
  expander modes are unchanged.

## [0.47.0] — 2026-05-30

### Added — 2FA / TOTP manager + `otp <issuer>` autocomplete

Full TOTP (RFC 6238) integration:

- **`otp ama` → live code copied on Enter.** Type `otp` plus a fuzzy
  query, and the matching entries surface as autocomplete rows at the
  top of the list with their currently-rolling 6-digit code and
  seconds-remaining. ↩ copies the code to clipboard + hides the popup.
- **`2fa` → full-screen management overlay.** List view with live codes
  + animated countdown rings + per-row copy/delete. Add tab with
  manual form (Issuer / Account / Secret + advanced digits/period/
  algorithm toggle). Import/Export tab with paste-and-go for every
  popular format.

### Supported import formats (autodetected)

| Format | Source |
|---|---|
| `otpauth://totp/...` | Single QR-code URI (universal) |
| `otpauth-migration://offline?data=...` | Google Authenticator bulk export |
| Aegis JSON (unencrypted) | Android Aegis Authenticator |
| 2FAS JSON | 2FAS Auth (Android/iOS) |
| Plain-text URI list (one per line) | manual / scripted exports |

The Google migration format is decoded via a hand-written protobuf
wire-format reader (no `prost` / `protoc` build-time dependency). All
parsers live in `core/rust-lib/src/totp_import.rs` and return a
common `ImportedEntry` shape that flows through the same
`totp_store::add` path as manual entries.

### Storage + encryption

Secrets are **never** stored in plaintext. Each base32 secret is
AES-GCM-encrypted via the existing `crate::crypto` (key in macOS
Keychain via `keyring`) before going into the DB, and decrypted
on-demand for code generation. The frontend never sees the raw
secret — only the live code.

DB schema:
```sql
CREATE TABLE totp_entries (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    issuer      TEXT NOT NULL,
    account     TEXT NOT NULL,
    secret_enc  TEXT NOT NULL,   -- crypto::encrypt(base32 secret)
    digits      INTEGER NOT NULL DEFAULT 6,
    period      INTEGER NOT NULL DEFAULT 30,
    algorithm   TEXT NOT NULL DEFAULT 'SHA1',
    created_at  INTEGER NOT NULL
);
CREATE INDEX idx_totp_issuer ON totp_entries (LOWER(issuer));
```

Export option dumps all entries as `otpauth://` URIs to the clipboard
— Plaintext, with a prominent warning in the UI.

### Files

- `core/rust-lib/src/totp_store.rs` (new) — DB schema, CRUD, code
  generation via `totp-rs`, base32 codec, normalization. 6 unit tests
  (base32 RFC 4648 vectors, padding tolerance, secret normalization,
  invalid char rejection, default-options code generation).
- `core/rust-lib/src/totp_import.rs` (new) — multi-format parsers +
  format autodetection + export. Hand-written protobuf reader (no
  `prost`). 11 unit tests cover all four parser families + the
  protobuf round-trip.
- `core/rust-lib/src/commands.rs` — 7 new IPCs: `totp_list`,
  `totp_add`, `totp_delete`, `totp_current_code`,
  `totp_current_codes_all`, `totp_import`, `totp_export`.
- `core/rust-lib/Cargo.toml` — added `totp-rs = "5.7"` (MIT, ~30 KB).
- `core/frontend/src/lib/totp.ts` (new) — `TotpEntry`/`TotpCode`
  types + `matchTotpEntries(query, entries)` (fuzzy issuer/account
  matcher with prefix-bonus ranking).
- `core/frontend/src/lib/ipc.ts` — 7 typed wrappers.
- `core/frontend/src/lib/types.ts` — new `ListEntry` kinds
  `"totp-manage"` + `"totp"` with full type info.
- `core/frontend/src/lib/commands.ts` — `is2faTrigger(query)` +
  `parseOtpQuery(query)`.
- `core/frontend/src/components/TotpOverlay.tsx` (new) — three-tab
  overlay (List / Add / Import-Export). Lives in App.tsx via the same
  full-screen-takeover pattern as `<BpmDetector />`. 1 s polling for
  live codes; SVG-based animated countdown ring per row that
  interpolates locally between server ticks for smooth animation.
- `core/frontend/src/App.tsx` — `totpMode` state, polling for
  `otp <query>` autocomplete, activate handler for `totp` (copy
  code) + `totp-manage` (open overlay) kinds.
- `core/frontend/src/components/HistoryItem.tsx` — icon + chip + row
  body for both new kinds; TOTP rows show issuer + account on the
  left and the big code on the right.
- `core/frontend/src/components/PreviewPanel.tsx` — preview panels
  for both new kinds (TOTP shows big code + countdown).

### Tests

**281 Rust + 450 frontend tests pass** (+17 new TOTP tests in Rust).

### Why 0.47.0

Major feature add: new search-bar shortcut family (`otp <query>`),
new command (`2fa`), new overlay, new DB table, new dependency
(`totp-rs`), new encryption usage. Minor bump.

## [0.46.1] — 2026-05-30

### Fixed — BPM displayed "too fast" for the first ~20 seconds

User report after 0.45.2 landed: BPM detection is accurate after about 20 seconds but shows clearly too-high values before that. Root cause traced to a single overly-lax guard in `bpm.ts::push`:

```ts
if (this.energyHistory.length < 4) return;   // ← way too short
```

The energy moving-average baseline is supposed to cover `AVG_WINDOW_MS = 3000 ms` so the per-chunk threshold (`avg × 1.4`) reflects real music-level baseline. The old guard only required 4 chunks (≈ 67 ms at 60 Hz rAF). The avg was therefore biased to whatever ambient / silence was at startup. As soon as music kicked in, EVERY chunk exceeded `avg × 1.4` → a burst of false onsets fired pinned to the refractory floor (300 ms ≈ 200 BPM) → IOI median locked at 200 → display showed 200 BPM. Those bad onsets then needed ~6 s to age out of the IOI window and another ~4 s to age out of the display mean → user-visible "wrong for ~15-20 s" before the system recovered.

**Fix:** replace the chunk-count guard with a duration guard — don't allow onsets until the energy history actually spans `AVG_WINDOW_MS`. Costs ~3 s of "Listening…" before the first BPM appears (vs ~15-20 s of wrong readings). Net UX win.

UI sub-label also updated: `"Listening… (Baseline-Kalibrierung ~3s, dann lock-in)"` so the user knows the 3-second wait is expected, not a bug.

### Files

- `core/frontend/src/lib/bpm.ts` — replaced `energyHistory.length < 4` with `nowMs - energyHistory[0].time < AVG_WINDOW_MS`.
- `core/frontend/src/components/BpmDetector.tsx` — listening sub-label mentions the 3-second baseline calibration.
- `core/frontend/src/lib/bpm.test.ts` — 2 new tests assert (a) no onsets fire while the gate is closed (silence-then-music worst case) and (b) estimates start firing once the gate opens. Existing tests adjusted to push 3 s of baseline before injecting onsets.

### Tests

**264 Rust + 450 frontend tests pass.**

## [0.46.0] — 2026-05-30

### Changed — Markdown → PDF is now fully standalone (no mrxdown CLI required)

Pre-0.46.0 `Ctrl+Shift+M` shelled out to the `mrxdown` Electron CLI to do the actual MD→PDF conversion. If mrxdown wasn't installed, the hotkey surfaced a "not installed" notification and did nothing. Now the whole pipeline runs in-process — every macOS install of Inspector Rust can convert Markdown to PDF, zero extra dependencies.

### Pipeline

```
.md / .markdown
   │
   │  pulldown-cmark::Parser
   │  (CommonMark + GFM tables + strikethrough + task-lists
   │   + footnotes + smart-punctuation)
   ▼
self-contained HTML doc with embedded GitHub-flavored CSS
   │
   │  WKWebView.createPDF (Apple's own Chromium-equivalent renderer)
   │  ↳ runs on main thread (WebKit requirement, dispatched via
   │    `app.run_on_main_thread`)
   ▼
.pdf next to source (foo.md → foo.pdf, same dir)
```

The CSS template is a GitHub-flavored Markdown-inspired stylesheet baked into the Rust binary: sober typography, syntax-highlighted code blocks (background-tinted), bordered tables with striped rows, `@media print` rules that tighten margins + prevent code/tables from breaking across pages. No external resources (no web fonts, no CDN CSS) so the renderer never needs network access.

Output PDF quality is comparable to mrxdown for plain markdown. KaTeX math + syntax-highlighting (syntect) are roadmap items for a future release if there's demand.

### Why WKWebView (not Chromium / wkhtmltopdf / weasyprint)

We're already in a Tauri app — WKWebView is the same engine that renders the popup UI, so we get Chromium-equivalent layout + CSS support **without bundling 100 MB of Chromium**. `createPDF` has been the Apple-blessed HTML→PDF API since macOS 11 (Big Sur, 2020), used by Safari's "Save as PDF…" itself.

### Files

- `core/rust-lib/src/md_to_pdf.rs` (renamed from `mrxdown.rs`):
  - `pub fn render_html(md) -> String` — pulldown-cmark + HTML template. Pure, deterministic, testable in isolation.
  - `pub fn convert_files(paths) -> ConvertSummary` — filters md, calls platform `write_pdf` per file. New `backend_unavailable` flag for Win/Linux fallback.
  - `mod macos` — raw `objc2`-based FFI to WKWebView + WKWebViewConfiguration + createPDFWithConfiguration:completionHandler:. CFRunLoop pumping between async stages (load, then PDF). Pure raw FFI to keep dependencies slim — no `objc2-web-kit` crate added.
- `core/rust-lib/src/hotkey.rs` — `Ctrl+Shift+M` handler now dispatches the conversion to the main thread via `app.run_on_main_thread` + oneshot channel (WebKit/AppKit assertion: main thread only). Per-file is brief (~50-300 ms) but a batch of 10 files briefly freezes the UI for the duration of the batch — acceptable trade-off for not bundling Chromium.
- `core/rust-lib/Cargo.toml` — new dep `pulldown-cmark = "0.10"` (~150 KB, zero unsafe, pure Rust, MIT).
- 11 unit tests in `md_to_pdf.rs`: filter behavior, sibling-path mapping, HTML rendering correctness (tables, strikethrough, task-lists, code blocks with language class), notification message variants including the new `backend_unavailable` case.

### What this does NOT solve

- **Windows + Linux not yet**: hotkey fires + notification reads "Markdown → PDF: macOS-only in v0.46.0 (Win + Linux folgen)". The architecture is set up to slot in `WebView2.PrintToPdfAsync` (Windows) + `webkit_print_operation_print_to_pdf` (Linux) — modules go behind the existing `cfg(target_os)` gate. Estimate: ~half a day per platform.
- **No KaTeX math** — mrxdown supports `$inline$` / `$$display$$`. To match, we'd embed KaTeX's ~300 KB JS lib in the rendered HTML. Doable, separate release.
- **No syntect / Prism syntax highlighting** — code blocks get a language-class attribute but no per-token coloring. Same story: embed a JS highlighter or pre-render via `syntect` crate.
- **No PDF template / theme picker** — single default theme (the embedded CSS). mrxdown's frontmatter `pdfTemplate` is not honored.

### Tests

**264 Rust + 448 frontend tests pass** (+3 new HTML-rendering tests, retaining the existing filter / notification tests).

### Why 0.46.0

Major feature swap on a tier-1 platform (standalone → no external dependency). UX is identical from the user's perspective (same hotkey, same output convention), but the dependency footprint goes from "needs mrxdown installed" to "works on any macOS 11+ install". Minor bump.

## [0.45.2] — 2026-05-30

### Changed — narrower bass filter + 4-second display mean

User feedback after 0.45.1 landed: BPM detection now locks correctly but (a) wants the high frequencies filtered out more, and (b) wants the displayed value to be properly averaged over a few seconds instead of updating per beat.

### Audio filter — 30-100 Hz bandpass

Replaced the single `lowpass(150 Hz, Q=1)` BiquadFilter with a cascade of `highpass(30 Hz, Q=0.7)` + `lowpass(100 Hz, Q=1.5)`. Effectively a 30-100 Hz bandpass — the prime kick-drum range (sub 30-50, fundamental 60-90, body 100). Vocals start at ~200 Hz, snare attack at 200 Hz+, hi-hats at 5 kHz+: all way outside the passband, so they no longer pollute the RMS envelope. Q=1.5 on the lowpass gives a small resonance bump at the kick fundamental — built-in boost where it matters. Highpass at 30 Hz cuts room rumble + BT-speaker low-end thump.

### Display — 4-second rolling mean

Replaced the EMA smoothing (α=0.12) on `smoothedBpm` with an explicit time-bounded mean of raw per-onset estimates over the last `DISPLAY_AVG_WINDOW_MS = 4000` ms. Each raw IOI-based estimate goes into a sliding history; the displayed value is the arithmetic mean of that history. Consecutive identical estimates are deduped so the rAF loop (60×/sec) doesn't oversample the same value between onsets.

At 120 BPM (~2 onsets/sec) the window holds ~8 raw estimates → reads "average tempo over the last few seconds", which is exactly what the user asked for. The number stops jumping per-beat; it stabilizes within ~6-8 seconds of locking on and only moves when the genuine average tempo moves.

Octave-snap + stale-reset from v0.45.1 still apply on top — display drops to "—" after 4 s of silence; rogue half/double IOIs get snapped to the locked octave before being averaged in.

### Files

- `core/frontend/src/lib/bpm.ts`: removed `SMOOTHING_ALPHA` config + `smoothedBpm` field. Added `DISPLAY_AVG_WINDOW_MS` config + `rawBpmHistory: Array<{time, bpm}>` + `displayBpm` field. `estimate()` now records each raw estimate (deduped) into the history, trims to window, and returns the mean.
- `core/frontend/src/components/BpmDetector.tsx`: cascaded `highpass(30 Hz) → lowpass(100 Hz, Q=1.5)` filter chain in the audio graph. Display sub-label now reads "4-Sekunden-Mittel · Confidence: N%".
- `core/frontend/src/components/PreviewPanel.tsx`: explainer updated to mention the bandpass + rolling-mean approach.

### Tests

`bpm.test.ts` unchanged for accuracy tests (windowed mean of a single estimate is just that estimate). Config-shape tests updated to assert `DISPLAY_AVG_WINDOW_MS` is in the 3-5 s range. **261 Rust + 448 frontend tests pass.**

## [0.45.1] — 2026-05-30

### Fixed — BPM detector stability + honest stale-reset

Two complaints after v0.45.0 shipped:

1. **"springt"** — over a Bluetooth speaker the BPM display flipped between 120 ↔ 240 (or similar octave pairs). Root cause: BT speakers introduce echoes / dropouts → some real beats get ghost-onsets in the IOI window → median IOI occasionally crosses an octave boundary → octave-correction multiplicatively doubles or halves the result → display flips.

2. **"soll live die aktuellen werte anzeigen"** — when audio stopped or BT dropped out, the last detected BPM stayed on screen indefinitely (smoothedBpm was sticky forever). Misleading — looked like a current reading but was historical.

### Algorithm changes (`core/frontend/src/lib/bpm.ts`)

**Octave snap** (`OCTAVE_SNAP_TOLERANCE_BPM = 8`): after octave-correcting the raw BPM into [60, 200], compare against the currently-locked smoothedBpm. If the raw value sits within ±8 BPM of half or double the locked value, snap to the locked octave instead of taking the multiplicative jump. Kills the 120↔240 oscillation directly — a few rogue IOIs no longer flip the displayed tempo across an octave.

**Stale-reset** (`STALE_RESET_MS = 4000`): track wall-clock time of the last estimate that had ≥ 4 onsets in window. If onsets drop below the threshold AND no valid estimate has happened in 4 s, force smoothedBpm to 0 → display reverts to "—". Brief onset droughts (BT mic stutter, song instrumental break) under 4 s still show the last value (no flicker); sustained silence honestly resets.

**Slightly stiffer refractory** (250 → 300 ms): drops max detectable from 240 to 200 BPM (still covers all popular music) but suppresses more BT-speaker echo onsets that arrived between the original refractory cutoff and the next real beat.

**Slower EMA** (α 0.20 → 0.12): displayed BPM lock-in still happens in ~6-8 s but the number stops flickering ±3 around the true value when echoes inject noise.

### Tests

`bpm.test.ts` gained **4 new tests** verifying the stale-reset (sticks through brief silence, resets after sustained) + octave-snap (a rogue half-IOI burst can't flip a locked value across an octave). **261 Rust + 448 frontend tests pass.**

## [0.45.0] — 2026-05-30

### Added — `bpm` live tempo detector from microphone

Type `bpm` in the popup → press Enter → full-overlay live BPM detector. The microphone is captured via `getUserMedia({ audio: true })`, lowpass-filtered to the bass band (~150 Hz, where popular music carries the kick drum), and analyzed by an energy-based onset detector with median IOI clustering. The big BPM number pulses on every detected beat; a confidence bar shows how steady the recent intervals are; an energy meter confirms audio is flowing.

Hold any speaker / phone playing music near the mic — within ~8 seconds the BPM locks onto the track. Esc exits + releases the microphone.

### Algorithm

Classic real-time DSP approach (Patin 2003 / used by Mixxx, RealtimeBPMAnalyzer.js, Spotify's web player):

1. Audio graph: `mic → BiquadFilter(lowpass 150Hz) → AnalyserNode → ∅`. The graph does NOT connect to the speakers — no monitoring, no feedback loop.
2. Per `requestAnimationFrame` (~60 Hz), read a 1024-sample Float32 time-domain frame from the analyser. Compute RMS energy.
3. Maintain a 3-second sliding moving average of energy. An "onset" fires when chunk-energy > avg × 1.4 AND ≥ 250 ms since the last onset (refractory).
4. Store onset timestamps in a 6-second sliding window. Compute inter-onset intervals (IOIs). Median IOI → `BPM = 60000 / median_ms`.
5. Octave-correct into the [60, 200] BPM range (halves a too-fast read, doubles a too-slow one).
6. EMA-smooth the visible value (α=0.2) so the displayed number doesn't flicker ±1 every beat.

Confidence = `1 - (stddev / median)` of recent IOIs. A track with steady quarter notes scores ~0.9, background noise ~0.1.

### Why not autocorrelation / spectral flux

Both are more accurate on syncopated material but 3-5× the CPU and substantially more code. For "what tempo is this music playing nearby" the energy-onset approach gives 85-95 % accuracy on 4/4 popular music, which matches the user expectation here.

### Files

- `core/frontend/src/lib/bpm.ts` (new) — pure-TS `BpmAnalyzer` class with `push(samples, nowMs)` + `estimate(nowMs)`. No DOM / no audio deps; testable in isolation.
- `core/frontend/src/lib/bpm.test.ts` (new) — **14 tests**: locks onto 90 / 120 / 175 BPM within 10 s of synthetic beat input; octave-corrects 50 → 100 and 240 → 120; rejects jittered onsets (low confidence); refractory period suppresses double-triggers; `beatJustFired` fires exactly once per beat.
- `core/frontend/src/components/BpmDetector.tsx` (new) — full-overlay React component. Three phases: `requesting` (mic prompt open) / `listening` (audio flowing) / `denied` (no mic / user said no). Builds the Web Audio graph, runs rAF loop, owns Esc-to-exit.
- `core/frontend/src/styles.css` — new keyframes `@keyframes bpmPulse` (slow breathing for the requesting state) + `@keyframes bpmBeatPulse` (0.3 s scale + accent flash on the BPM number per beat).
- `core/frontend/src/lib/commands.ts` — new `isBpmTrigger(query)` (exact `bpm` match, whitespace + case tolerant).
- `core/frontend/src/lib/types.ts` — new `ListEntry` kind `"bpm"` + `BpmTriggerView` data interface.
- `core/frontend/src/App.tsx` — `bpmEntry` useMemo, `bpmMode` state, activate handler routes Enter → `setBpmMode(true)`, render switch shows `<BpmDetector />` taking over the app-shell while active.
- `core/frontend/src/components/HistoryItem.tsx` — `Activity` icon + `bpm` chip + descriptive subtext for the trigger row.
- `core/frontend/src/components/PreviewPanel.tsx` — preview-pane explainer for the `bpm` row.
- `core/frontend/src/components/HistoryList.tsx` — react key fragment for the new entry kind.
- `macos/src-tauri/entitlements.plist` — added `com.apple.security.device.audio-input` (required by Hardened Runtime for any `getUserMedia({audio: true})` call).
- `scripts/install-macos.sh` — injects `NSMicrophoneUsageDescription` into the bundled `Info.plist` post-build (mirrors the existing `NSAppleEventsUsageDescription` pattern). This is the human-readable copy macOS shows in the first-time mic-permission prompt.

### Tests

**261 Rust + 444 frontend tests pass** (+14 new `bpm.test.ts`).

### Why 0.45.0

New user-facing surface (overlay + mic capture + DSP algorithm) + new entitlement + new Info.plist key → minor bump. No breaking changes; existing hotkeys / IPCs unchanged.

## [0.44.0] — 2026-05-30

### Added — `Ctrl+Shift+M` Markdown → PDF via mrxdown

New global hotkey: `Ctrl+Shift+M` reads the current Finder selection (via the same `osascript`-driven path as `Ctrl+Shift+F`), filters to `.md`/`.markdown` files, and shells out to the user-installed `mrxdown` CLI per file. Each PDF lands sibling to its source (`foo.md` → `foo.pdf` in the same directory) — mrxdown's own output convention, so we just pass paths through.

Native macOS notification on completion: `"3 konvertiert"`, `"1 konvertiert, 2 übersprungen"`, `"mrxdown ist nicht installiert"` (the PATH-scan pre-check surfaces this *before* spawning Electron N times). Glass sound on success, Funk sound on any failure.

The hotkey is non-fatal when mrxdown isn't installed — the user gets a clear actionable message instead of silent failure. Helpful for distributing the app to users who might not have mrxdown set up yet.

### Files

- **`core/rust-lib/src/mrxdown.rs`** (new) — `pub fn convert_files(&[PathBuf]) -> ConvertSummary` filters `.md`/`.markdown` extensions, runs `Command::new("mrxdown").arg(path)` per file. `mrxdown_available()` PATH-scans (cross-platform, also probes `.exe`/`.cmd`/`.bat` on Windows). `notify(&summary)` mirrors `timer.rs::notify_visual` pattern (`osascript display notification` + `afplay`). 8 unit tests pin the filter + summary-message invariants without requiring mrxdown to be installed.
- `core/rust-lib/src/lib.rs` — `mod mrxdown` declaration.
- `core/rust-lib/src/hotkey.rs`:
  - `Ctrl+Shift+M` registered in `register(app)` alongside the OCR/Screenshot/Eyedropper/Finder handlers. Worker thread dispatch (mrxdown spawns Electron per call, ~1-3 s).
  - Added to reserved-list in `register_popup` + `register_direct_slots` collision-check.

### Tests

**261 Rust + 430 frontend tests pass** (+8 new mrxdown unit tests).

### Why 0.44.0

New user-facing hotkey + new external integration → minor bump. No breaking changes; existing hotkeys + IPCs unchanged.

## [0.43.4] — 2026-05-30

### Changed — pwgen mode shortcut moves to Cmd/Ctrl+1…4

Pre-0.43.4 the pwgen mode-switch was bound to `Alt+1…4`, which collided with the default text-expander hotkey (`Alt+Digit1`). Even with the v0.43.2 event-forward workaround firing reliably, the user-facing model was confusing: "why does Alt+1 need a special path while Alt+2/3/4 just work?"

Bound the shortcut to `Cmd+1…4` on macOS and `Ctrl+1…4` on Win/Linux instead. No collision with anything global (the macOS Carbon dispatcher only swallows hotkeys we explicitly register, and none of our globals use `Cmd+Digit`). No collision with the `TransformBar` Cmd/Ctrl+1…9 listener either — that one only mounts for *text* entries, so pwgen rows have it unmounted.

Net effect: all four digits hit the same JS keydown path uniformly. The Tauri event-forward is kept as belt-and-suspenders in case a user binds `Cmd+Digit` to a direct snippet slot.

UI updated: mode buttons now display `⌘1`-`⌘4` (macOS) / `Ctrl+1`-`Ctrl+4` (else); bottom hint reads `⏎ copy · ⌘1–4 switch mode + regenerate`.

### Files

- `core/frontend/src/App.tsx`: pwgen keydown handler swaps `e.altKey` for the platform-correct `metaKey`/`ctrlKey` check; forwarded-event regex now matches `Cmd+Digit[1-4]` / `Ctrl+Digit[1-4]`.
- `core/frontend/src/components/PreviewPanel.tsx`: mode-button badges + bottom hint switch to `⌘`/`Ctrl+` formatting via `IS_MAC`.

### Docs

- `docs/macos-permissions.md` (new) — bug-story article on the three macOS quirks Inspector Rust hit on the way to a working expander: unstable code signature → TCC re-grants, System-Events Apple-Events grant missing → silent frontmost False, Carbon hotkey dispatcher → swallowed Alt+1. Written as a transferable lesson for other Tauri/Electron-on-macOS devs.

### Tests

**253 Rust + 430 frontend tests pass.**

## [0.43.3] — 2026-05-30

### Fixed — frontmost check no longer needs the System Events TCC grant

v0.43.2 still surfaced the "Text expansion failed — Accessibility not granted" banner on Alt+1, even though the bail-out was supposed to short-circuit before the AX check ever ran. Root cause: `inspector_rust_is_frontmost_public` calls AppleScript `tell application "System Events"` to read the frontmost process — which requires the *System Events* Apple Events grant, **separate** from the Finder one. On a fresh install where only Finder automation is granted (or neither), the probe silently returns `None` → the gate evaluates false → handler continues into the AX check → emits `expander-permission-needed` → Settings tab + amber banner.

**Fix:** replace the AppleScript probe with `popup_is_visible(app)` — a pure Tauri `Window::is_visible()` read. Needs zero TCC grants of any kind, works on a brand-new install with no permissions clicked. The "popup visible == we own the user's attention" heuristic is more accurate for our use-case anyway than "frontmost app name matches Inspector Rust"; the popup auto-hides on focus loss, so visibility is a reliable proxy for "user is interacting with us".

Both the expander handler and the direct-slot handler use the new check.

### Files

- `core/rust-lib/src/hotkey.rs`: new `fn popup_is_visible(app)` helper; both `register_expander` + `register_direct_slots` callbacks now gate on it instead of `expander::inspector_rust_is_frontmost_public`.

### Tests

**253 Rust + 430 frontend tests pass.**

## [0.43.2] — 2026-05-30

### Fixed — pwgen Alt+1 actually fires now (event-forward through the global hotkey)

After v0.43.1 stopped the Settings-tab from popping open on Alt+1, the in-popup pwgen mode-switch *still* didn't trigger on Alt+1 specifically — only Alt+2 / 3 / 4 worked. Root cause: macOS' Carbon hotkey dispatcher **swallows** the keydown of any registered global shortcut, so the webview never receives `Alt+Digit1` while the abbreviation expander is enabled on its default hotkey. The early-bail added in v0.43.1 prevented the wrong side-effect, but couldn't deliver the digit to the in-popup JS handler.

**Fix:** when the global expander / direct-slot hotkey fires while Inspector Rust is frontmost, the Rust handler now emits a new `expander-hotkey-forwarded` Tauri event carrying the hotkey string (e.g. `"Alt+Digit1"`). The pwgen handler in App.tsx listens for it and, if it matches `Alt+Digit1…4` and a pwgen row is selected, performs the same mode-switch + regenerate as the JS keydown handler would for the un-swallowed digits. Net effect: all four digits feel identical regardless of which one (if any) collides with the expander/slot binding.

### Files

- `core/rust-lib/src/hotkey.rs`: the expander + direct-slot handlers, on `inspector_rust_is_frontmost`, now `app.emit("expander-hotkey-forwarded", hotkey)` with the bound hotkey string instead of just bailing silently.
- `core/frontend/src/App.tsx`: new `useTauriEvent("expander-hotkey-forwarded", ...)` that translates `Alt+Digit1…4` into the same `setPwgenMode` + `setPwgenSeed` the keydown handler runs.

### Tests

**253 Rust + 430 frontend tests pass.**

## [0.43.1] — 2026-05-30

### Fixed — pwgen Alt+1…4 no longer hijacked by the global expander hotkey

Two issues with the v0.43.0 pwgen mode-switch shortcut:

1. **Alt+1 was opening the Settings tab.** If the user had the abbreviation expander enabled (default hotkey `Alt+Digit1`), pressing Alt+1 in our popup fired the *global* expander shortcut, which — on a fresh install with no Accessibility grant — emitted `expander-permission-needed` → the frontend switched to the Settings tab to show the permission banner. That was intrusive when the user is interacting with our own popup, not an external app.

   **Fix:** `hotkey::register_expander` now early-bails when Inspector Rust is the frontmost app (via `frontmost_app::name`, which uses NSWorkspace / AppleScript and doesn't need the Accessibility grant). When the popup owns focus the global hotkey is effectively a no-op, leaving the in-popup JS handler to do whatever the feature wants. Same early-bail added to `register_direct_slots`.

2. **Alt+1…4 was copying + hiding the popup.** Wrong UX — the user wants to tap Alt+1…4 several times to cycle through generated samples, then press Enter to commit. Now Alt+1…4 just switches the mode + regenerates; the password stays on screen + the popup stays open. Enter still copies + hides via the existing pwgen activate path.

### Files

- `core/rust-lib/src/expander.rs`: `inspector_rust_is_frontmost` now has a `_public` wrapper exposed for `hotkey.rs` callers.
- `core/rust-lib/src/hotkey.rs`: `register_expander` + `register_direct_slots` hotkey callbacks early-bail when Inspector Rust is frontmost.
- `core/frontend/src/App.tsx`: pwgen Alt+1…4 effect drops the `writeText` + `hidePopup` calls — pure mode-switch + regenerate.
- `core/frontend/src/components/PreviewPanel.tsx`: copy hints + tooltip text updated to "Enter copies".

### Tests

**253 Rust + 430 frontend tests pass.**

## [0.43.0] — 2026-05-29

### Added — `pwgen` mode-switch shortcuts (Alt+1…4)

While a `pwgen` row is selected in the popup, **Alt+1…4** switches the generator mode, regenerates the password with the new mode, copies to clipboard, and hides the popup — all in one keypress.

| Shortcut | Mode |
|---|---|
| `Alt+1` | All chars (A-Z a-z 0-9 + symbols) |
| `Alt+2` | Alphanumeric (no symbols) |
| `Alt+3` | Dictionary (English words + digit padding) |
| `Alt+4` | Leetspeak (dict + `a→@ e→3 i→1 o→0 s→$ t→7 …`) |

Mirrors the existing `Alt+Enter → alphanumeric + copy` shortcut. Uses `e.code = "Digit1"…"Digit4"` (W3C `KeyboardEvent.code`) not `e.key`, so it still works on German Mac keyboards where Alt+1 would otherwise type `¡`.

The mode-picker buttons in the preview pane now show each mode's `⌥1`-`⌥4` badge.

### Added — configurable popup hotkey (Settings → Popup hotkey)

The global shortcut that opens the search popup is now user-configurable from Settings. Pre-0.43.0 it was hard-coded to `Ctrl+Shift+V`; now any combination `parse_shortcut` accepts will do.

- New section **Settings → Popup hotkey** with `HotkeyCapture` widget + platform-aware preset chips:
  - macOS: `⌃⇧V (default)`, `⌘⇧V`, `⌘⇧Space`, `⌘J`
  - Windows: `Ctrl+Shift+V (default)`, `Win+Shift+V`, `Alt+Space`
  - Linux: `Ctrl+Shift+V (default)`, `Super+V`, `Super+Space`, `Alt+Space`
- Saved to the existing `settings` table under key `popup.hotkey`; restored at startup.
- Validation rejects collisions with the still-hard-coded `Ctrl+Shift+O/S/C/F` (OCR / Screenshot / Eyedropper / Finder) + the currently-armed expander hotkey + direct-slot hotkeys. On rejection the **old** hotkey stays armed (no orphaned popup access).

### Not yet — bare-modifier triggers (just ⌘ / Win / Super)

The OS-level global-shortcut plugin requires a non-modifier key, so registering a bare modifier ("just press ⌘ to open") isn't possible through the standard API. Adding it would need a separate `CGEventTap` (macOS) / `SetWindowsHookEx WH_KEYBOARD_LL` (Windows) / `libinput`-based monitor (Linux) and per-platform double-tap-vs-long-press logic. Tracked for a future release if there's demand — for now use any combo with at least one regular key.

### Files

- `core/rust-lib/src/hotkey.rs`:
  - New `pub struct PopupShortcutState` (Tauri state holding the current `Shortcut`).
  - New `pub const DEFAULT_POPUP_HOTKEY = "Ctrl+Shift+KeyV"` + `pub const KEY_POPUP_HOTKEY = "popup.hotkey"`.
  - New `pub fn register_popup(app, state, hotkey)` — collision-check + unregister-old + register-new. Validates against OCR / Screenshot / Eyedropper / Finder / expander / direct slots.
  - The popup registration is removed from `register(app)` (which now handles only the still-hard-coded globals).
- `core/rust-lib/src/commands.rs`: 3 new IPCs — `get_popup_hotkey`, `get_popup_hotkey_default`, `set_popup_hotkey`.
- `core/rust-lib/src/lib.rs`: manages `PopupShortcutState`; on startup, reads from settings (fallback to default) and calls `register_popup`. On failure (e.g. user-customised hotkey now colliding with something) falls back to the default so the popup is always reachable.
- `core/frontend/src/lib/ipc.ts`: 3 typed wrappers matching the new IPCs.
- `core/frontend/src/components/SettingsPanel.tsx`: new `PopupHotkeySection` component placed before the Text expander section.
- `core/frontend/src/App.tsx`: new `selectedPwgen` effect — Alt+1…4 keyboard listener firing only while a pwgen row is selected.
- `core/frontend/src/components/PreviewPanel.tsx`: pwgen mode buttons now display `⌥1`-`⌥4` badges; bottom hint updated to "`⏎ copy · ⌥1–4 switch mode + copy`".

### Tests

**253 Rust + 430 frontend tests pass.** No new tests added — the pwgen Alt+1-4 path reuses `generatePassword` (already tested), the popup-hotkey IPC path reuses the same code path as the existing `set_expander_config` (already tested via integration).

## [0.42.1] — 2026-05-29

### Changed — Transform chip overlay is now press-to-reveal (Cmd/Ctrl held)

The 12-chip "Transform" toolbar that surfaces under text / HTML / RTF / OCR previews was always-visible since it was added — taking ~2-3 rows of space at the bottom of the preview pane and pushing the actual clip content up. Hiding the content the user is actually trying to read defeats the purpose of the preview pane.

The toolbar is now **only rendered while Cmd (macOS) / Ctrl (Win+Linux) is held** — the same modifier the `Cmd/Ctrl+1…9` digit shortcuts already fire on. Release the modifier → it disappears + the text content expands back to fill the pane.

The digit shortcuts themselves still work without peeking at the overlay: the `keydown` handler is mounted regardless of toolbar visibility, so muscle-memory users who already know `Cmd+1` = "Remove vowels" never need the visual reminder.

### Files

- `core/frontend/src/hooks/useModifierHeld.ts` (new) — generic `boolean`-returning hook tracking platform-modifier hold state. Resets to `false` on window `blur` to dodge the "stuck modifier after Cmd+Tab" trap.
- `core/frontend/src/components/PreviewPanel.tsx` — `TransformBar` calls `useModifierHeld`; returns `null` when not held (keyboard `useEffect` keeps firing).
- `core/frontend/src/hooks/useModifierHeld.test.ts` (new) — 6 tests cover default-false, Meta + Control round-trip, blur-reset, accepting `metaKey` on non-modifier keys (Cmd+1 typed fast), and listener cleanup on unmount.

### Tests

**253 Rust + 430 frontend tests pass** (+6 for the new hook).

## [0.42.0] — 2026-05-29

### Changed — Windows wakelock now uses `SetThreadExecutionState` instead of cursor-jiggle

Sibling change to the v0.41.0 macOS fix. The Windows path keeps a worker thread alive while wakelock is on; on entry it calls `SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED)` from `kernel32`, on exit it clears with `SetThreadExecutionState(ES_CONTINUOUS)`. The flag is per-thread and sticky between those two calls, so the worker just sleeps in 200 ms chunks waiting for the stop signal — no periodic re-arming, no `SetCursorPos`-jiggle, no visual blip.

Why this matters even though Windows generally honoured the old `SetCursorPos` jiggle (unlike macOS):

- **Group-policy-managed corporate Windows** can disable synthetic-input idle resets. `SetThreadExecutionState` is the documented + GPO-resistant API.
- Zero visual cursor disturbance every 60 s.
- No 60 s polling — the worker just blocks on the stop flag.
- Symmetric architecture with the macOS `caffeinate` path (both are "engage the kernel-side inhibit, hold, release").

### Files

- `core/rust-lib/src/wakelock.rs`:
  - New `mod win_power` (raw `extern "system"` FFI to `SetThreadExecutionState` from `kernel32`, no `windows`-crate feature added — pattern matches the existing raw macOS FFI in the file).
  - `worker` split into two `#[cfg]`-gated implementations: Linux keeps the jiggle loop with panic-shield; Windows engages power state on entry, sleeps for stop, disengages on exit.
  - The legacy `mod win` (`SetCursorPos` jiggle) is kept under `#[allow(dead_code)]` for documentation symmetry with the `#[allow(dead_code)] mod macos` from v0.41.0.

### Tests

**253 Rust + 424 frontend tests pass** (no test changes — the platform-internal CAS tests already split per OS in v0.41.0; Win path is covered by the shared `set_enabled_round_trip_returns_new_state`).

### Why 0.42.0

Backend swap on a tier-1 platform — same reasoning as v0.41.0's minor bump. macOS users see no change; Windows users see better reliability + no cursor twitch.

## [0.41.0] — 2026-05-29

### Fixed — wakelock now actually keeps macOS awake

Pre-0.41.0 the wakelock LED would pulse + the search-bar `wakelock1` command would report enabled, but **the screen still locked** after the user's "Require password after N minutes" timeout fired. The cursor-jiggle path (60 s `CGEventPost` mouse-moves) survives application-level idle detectors (Teams / Slack), but **does not** reset the macOS idle counter for screensaver / screen-lock on modern macOS — Apple hardens against synthetic `kCGEventMouseMoved` events being counted as user activity.

The fix replaces the macOS jiggle worker with a spawned `/usr/bin/caffeinate -disu` child process held alive while wakelock is on, killed on toggle-off. `caffeinate` raises proper IOPM kernel assertions (`PreventUserIdleSystemSleep` + `PreventUserIdleDisplaySleep` + `kIOPMAssertionTypeNoDisplaySleep`) — the supported way to keep macOS awake (and how Apple's own `caffeinate` CLI works internally). The screen now stays unlocked as long as the LED is on.

**Windows + Linux** keep the cursor-jiggle worker — those OSes don't ship an equivalent CLI in the base install. Future Win could use `SetThreadExecutionState(ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED)`; future Linux/Wayland could use `org.freedesktop.ScreenSaver` D-Bus inhibit.

### Files

- `core/rust-lib/src/wakelock.rs`:
  - New `WakelockState::caffeinate: Mutex<Option<std::process::Child>>` (macOS-only field; `handle` + `stop` are now `#[cfg(not(target_os = "macos"))]`-gated to the jiggle path).
  - `set_enabled(true)` on macOS spawns `caffeinate` with `Stdio::null()` on all 3 streams (no terminal noise).
  - `set_enabled(false)` on macOS calls `child.kill() + child.wait()` (no zombie).
  - CAS-based idempotency still applies — 16 racing `set_enabled(true)` callers still spawn exactly one child.
  - The legacy `mod macos` (jiggle FFI) is kept under `#[allow(dead_code)]` as documentation of the FFI shape, in case a future fallback wants it.
- 4 wakelock unit tests now: shared round-trip + panic-shield test, plus the idempotency + concurrent-CAS test in two variants (`stop`-Arc-pointer on non-mac, `caffeinate.id()` PID on mac).

### Tests

**253 Rust + 424 frontend tests pass.** 2 new macOS-specific tests verify the new `caffeinate` spawn / kill / no-double-spawn invariants.

### Why 0.41.0

A latent feature was advertised but didn't actually work on the primary platform. Bumping minor (not patch) because the macOS implementation backend swapped (jiggle → IOKit assertion via caffeinate) — observable in any monitoring tool that lists IOPM assertions (`pmset -g assertions`).

## [0.40.0] — 2026-05-25

### Added — `pwgen N` password generator with 4 modes

Type `pwgen 16` in the popup → generated 16-char password surfaces at the top of the list with the new `pwgen` chip. Four modes selectable via preview-pane buttons or keyboard:

| Mode | Charset | Example (`pwgen 16`) |
|---|---|---|
| **All chars** *(default)* | A-Z, a-z, 0-9, `!@#$%^&*()_+-=[]{};:,.?` | `7K$pX2#mQ@vN!9zR` |
| **Alphanumeric** | A-Z, a-z, 0-9 | `K7pXmQvN9zRwB2sT` |
| **Dictionary** | English words + digit padding to exact length | `BraveCoffee94Run` |
| **Leetspeak** | dict words with `a→@ e→3 i→1 o→0 s→$ t→7 l→1 g→9 b→8` | `8r@v3C0ff33Run` |

**Keyboard shortcuts on the pwgen row:**
- `Enter` — copy current password (current mode).
- `⌥ Enter` (Alt+Enter) — switch to alphanumeric mode + regenerate + copy in one shot. Quick path for "I want a password I can paste without breaking field validators that reject symbols".

**Preview pane:**
- The full password in big mono font (easy to eyeball-check).
- 4 radio-style mode buttons (active highlighted).
- `↻ Regenerate` button for fresh randomness without changing mode.
- Hotkey hint.

### Entropy

All four modes use Web Crypto's `crypto.getRandomValues` (CSPRNG, available since Node 19 in test env). `randInt(max)` does proper **rejection sampling** to eliminate modulo bias when the charset size doesn't divide 2³². Fallback to `Math.random` exists only for environments without Web Crypto — never hit at runtime.

### Length bounds

Minimum 4 chars (anything shorter is trivially brute-forceable); maximum 128 (web password fields commonly cap there). Anything outside that range — or any non-integer / negative — surfaces as a no-op (no entry in the list).

### Files

- `core/frontend/src/lib/pwgen-dict.ts` — 400 curated 4-7 letter English words. ~3 KB minified, no proper nouns / no ambiguous-looking glyphs (no `rn` → `m` lookalikes).
- `core/frontend/src/lib/pwgen.ts` — pure-TS generator with all 4 modes + CSPRNG.
- `core/frontend/src/lib/pwgen.test.ts` — 8 tests: exact-length invariant across all 4 modes × 8 lengths, charset guarantees, determinism (CSPRNG produces distinct outputs), leet-substitution presence.
- New `parsePwgenArg` in `commands.ts` + 5 unit tests (range, integer-only, edge cases).
- New `pwgen` entry in `COMMANDS`; new `PwgenEntryView` type + ListEntry kind.
- App.tsx owns `pwgenMode` + `pwgenSeed` state; `pwgenEntry` useMemo regenerates when query / mode / seed change.
- Activate handler: special-cased for `pwgen` — copies to clipboard via `@tauri-apps/plugin-clipboard-manager.writeText` (no paste-to-app — explicit user choice since you rarely want a generated password pasted into the previously-focused window).
- `useKeyboardNav.onEnter` signature widened to `(shiftKey, altKey)` so the Alt+Enter→alphanumeric shortcut works.
- PreviewPanel takes `pwgenMode` / `onPwgenModeChange` / `onPwgenReroll` props.

### Tests

+8 pwgen generator + +5 parser + +1 altKey-flag test in useKeyboardNav. **253 Rust + 424 frontend tests now pass.**

### Why 0.40.0

Substantial new user-facing surface: new search-bar command, new ListEntry kind, new preview-pane UX (4-mode radio + regenerate), new altKey keyboard semantic. Backwards-compatible. Minor digit bump.

## [0.39.0] — 2026-05-25

### Added — `timer N[s|min|h]` command with visual + audio notification

Type `timer 12` in the popup → 12-minute timer fires → macOS native notification + Glass system sound. Defaults: minutes (so `timer 12` = 12 minutes, the dominant pomodoro/cooking case).

**Parser (`parseTimerArg`)** accepts every spelling combination:

| Input | Result |
|---|---|
| `12` | 12 minutes |
| `12m`, `12 min`, `12 mins`, `12 minute`, `12 minutes`, `12 minuten` | 12 minutes |
| `30s`, `30 sec`, `30 secs`, `30 sek`, `30 sekunden`, `30 second(s)` | 30 seconds |
| `2h`, `2 hr`, `2 hrs`, `2 hour`, `2 hours`, `2 std`, `2 stunden` | 2 hours |
| `2,5 min`, `0.5 h` | comma + dot decimals supported |

Rejects: zero, negative, unknown units (`12 fortnights`, `12 d`), garbage suffixes.

**Backend (`core/rust-lib/src/timer.rs`)** spawns a worker thread per active timer (poll-sleep every 200 ms for responsive cancellation), then fires three notifications in parallel when it elapses:

1. **macOS native notification** via `osascript -e 'display notification …'` — top-right of screen, visible regardless of whether the popup is open. Quote-sanitised so a user-typed `"` can't break the AppleScript string.
2. **System sound** via `/usr/bin/afplay /System/Library/Sounds/Glass.aiff` — spawned (not `status()`), so the worker exits immediately even if audio takes ~500 ms to play through.
3. **Tauri `timer-fired` event** → popup, if open, shows a 4-second accent-coloured banner with the timer's label.

State is a `HashMap<TimerId, TimerSlot>` behind a `parking_lot::Mutex`; the frontend uses `list_timers` to count active timers for the footer's new `⏰ N` badge. `timers-changed` event fires on every start / cancel / fire so the badge updates without polling.

### Footer indicator

When ≥ 1 timer is active, a small `⏰ N` chip appears in the footer's left cluster next to the wakelock LED and the keyboard hints. Hover tooltip explains it will fire a macOS notification + Glass sound.

### Frontend wiring

- New `parseTimerArg(arg)` in `lib/commands.ts` + 9 unit tests covering every alias / case-sensitivity / German term / decimal format / rejection path.
- New `timer` entry in the `COMMANDS` catalogue + `CommandKind`.
- `commandEntry` switch surfaces a runnable command row with the parsed label (`Start timer · 12 minutes`).
- Activate handler dispatches via new `startTimer` IPC.
- App.tsx now subscribes to `timers-changed` (recount → footer) + `timer-fired` (set banner state, 4 s dwell) using the v0.38.2 `useTauriEvent` hook (no listener leak).

### Tests

+9 frontend (`parseTimerArg` — bare-number default-minutes, all unit aliases, singular/plural labels, case + decimal handling, every rejection class). **253 Rust + 412 frontend tests now pass.**

### Why 0.39.0

Substantial new user-facing feature surface: new search-bar command, new IPC trio, new Rust module, new footer indicator, new toast banner. Backwards-compatible. Minor digit bump.

## [0.38.2] — 2026-05-25

### Fixed — Pong bot div-by-zero on stationary ball + 11× listener-leak race

Two audit findings.

**A — `botBehavior` div-by-zero on `ballVx == 0`.** Pre-0.38.2 the predict-intercept branch fired on `ballVx > 0`. With `ballVx` exactly 0 (theoretical: serve-delay state, or a sub-frame axis-aligned bounce), the formula `dt = (botX - ballX) / ballVx` produced `Infinity`, `predictedY = ballY + ballVy * Infinity = ±Infinity`, then `clamp(0, fieldH)` → `fieldH` or `0`. Combined with the v0.38.0 hardcore-bot 15.7 px/frame cap, the paddle would jerk to a field edge. **Fix:** threshold is now `ballVx > 0.01` — anything below counts as "not approaching" and routes to the existing idle-to-centre branch. +2 regression tests pinned.

**B — Listener-on-unmount race (11 sites).** All `useEffect`-based Tauri event subscriptions in `App.tsx` had the standard race:

```tsx
let unlisten: UnlistenFn | undefined;
void listen("foo", handler).then(u => unlisten = u);
return () => unlisten?.();
```

If the component unmounts before the `listen()` promise resolves, the cleanup runs with `unlisten === undefined` → no-op, and the listener leaks for the app's lifetime. Symptom: under React strict-mode double-mount in dev, every event delivered twice; in production, slow leak across popup show/hide cycles.

Fixed two ways:
1. **6 simple single-listener cases** converted to a new `useTauriEvent(name, handler, deps?)` hook in `core/frontend/src/hooks/useTauriEvent.ts`. The hook owns the cancelled-flag + cleans up orphan listeners that resolve post-unmount.
2. **3 stateful / multi-listener cases** kept inline with a `cancelled` flag pattern: bruno-defaults-changed (paired with an IPC fetch), finder-loaded + finder-denied (two sequential listens), wakelock-changed (paired with the v0.37.1 `eventAlreadyFired` flag).

### Tests

+2 frontend (`botBehavior` Vx-0 + Vx-below-threshold). **253 Rust + 403 frontend tests now pass.**

### Why 0.38.2

Pure bug fixes. The Pong fix is a real crash-class issue (Infinity propagation through later code); the listener race was a slow leak. No IPC break, no behaviour change for the happy path. Patch-level → `0.x.y`.

## [0.38.1] — 2026-05-25

### Fixed — Pong mouse keeps working when cursor leaves the canvas

v0.37.1 moved the mouse listener from `window` to `canvas` to stop the keys-vs-mouse fight, but that meant the paddle stopped tracking the cursor as soon as it left the play field — annoying because `cursor: none` (v0.38.0) hides the cursor *inside* the canvas, so the user often has the cursor parked outside or off-screen while playing.

Better fix: listener back on `window` (so off-canvas + off-window movement reaches us), but **skip mouse updates while any paddle key is currently held** (`s.keys.up || s.keys.down`). Keys still win during keystrokes, mouse wins between. Off-canvas cursor → still drives the paddle.

The clientY-to-fieldY mapping uses the canvas's `getBoundingClientRect`; even when the cursor sits outside the rect, the formula produces a usable logical Y that `clamp` pins to the field bounds — sliding the cursor off the top/bottom edge parks the paddle at the corresponding extreme.

Header hint updated: "↑↓ / W/S / mouse" (was: "mouse on field").

## [0.38.0] — 2026-05-25

### Changed — Pong (`getshaky`): rubber-band AI + ball prediction + cursor hides on canvas

Three changes to the Pong easter egg, requested by the user. The bot is now actually a challenge, with a self-balancing dynamic that keeps matches close.

**1. Cursor hidden over the play field while the match is live.** Mouse controls the paddle, so the visible cursor was just a distraction during fast rallies. `cursor: none` on the canvas during `phase === "playing"`; stays visible during `intro` (so the user sees the page they triggered) and during `over` (so the rematch button is click-targetable).

**2. Rubber-band AI** — new `botBehavior(state)` in `lib/pong.ts` replaces the v0.37.x `botMaxSpeed(botScore)`. Takes *both* scores plus the live ball state and returns `{ targetY, maxSpeed }`. Skill multiplier curve:

| Score state | Skill | Max speed | Behaviour |
|---|---|---|---|
| Bot leads by 2+ | 0.45× | ~4.3 px/frame | **Plays badly** — slow paddle + ~50 px tracking error → human catches up |
| Bot leads by 1 | 0.78× | ~7.4 px/frame | Slightly slow |
| Tied | 1.00× | 9.5 px/frame | Baseline hard |
| Behind by 1 | 1.18× | ~11.2 px/frame | Slightly faster |
| Behind by 2+ | 1.35× | ~12.8 px/frame | Hard |
| **Player one away from winning** | **1.65×** | **~15.7 px/frame** | **HARDCORE** — perfect tracking, near-max ball-speed paddle. Match-point overrides any lead-based throwing. |

**3. Ball-intercept prediction.** When the ball moves toward the bot (`ballVx > 0`), the bot now predicts where it'll be at the bot's paddle column by straight-line extrapolation (ignoring wall bounces — refinement for later) instead of just chasing the live `ballY`. Makes the bot proactive — sharp-angle shots that used to slip past now get intercepted. When the ball moves *away*, the bot drifts toward field-centre as a defensive idle posture.

**Deterministic tracking error.** Low-skill bots add up to ±60 px error to their target via a pure `pseudoNoise(seed)` hash (FNV-1a-style `fract(sin)` hack) — no `Math.random`, so the bot's behaviour is reproducible per game state and unit-testable.

### Tests

+13 frontend tests in `pong.test.ts`:
- 3 `pseudoNoise` (determinism, bounds, distinct seeds).
- 10 `botBehavior` covering each skill bucket + ball prediction (forward + with vertical velocity + ball moving away) + the field-clamp invariant.

**253 Rust + 401 frontend tests now pass.**

### Why 0.38.0

User-feelable behaviour change (the bot now plays seriously) + new exported API (`botBehavior`, `pseudoNoise`). `botMaxSpeed` kept as `@deprecated` for the existing tests' baseline. Minor digit bump.

## [0.37.1] — 2026-05-25

### Fixed — 5 audit findings + Pong input-fight

Five real bugs caught in an audit pass over the v0.36–v0.37 surface, plus a UX glitch in the Pong easter egg.

**A — `app_launcher::icon_png_base64` race-condition with temp file.** Pre-0.37.1 the `sips` output path was `inspector-rust-appicon-<pid>.png` — PID is constant for a single process, so two concurrent `get_app_icon` IPCs (user scrolls list quickly, multiple rows lazy-load in parallel) both wrote to the SAME path → last writer wins → first reader gets the wrong icon → wrong icon cached for the path → wrong icon shown forever. Fix: per-call atomic counter suffix (`-<pid>-<seq>.png`).

**B — Wakelock LED init-state race.** The `wakelock-changed` listener registration and the initial `wakelock_get()` fetch were both async, with a ~10 ms window where an event-driven update could arrive *before* the fetch completed → fetch's stale value then overwrote the fresher event-set state → LED flickered wrong. Fix: register the listener FIRST, set a flag when an event has already fired, and skip the initial-fetch's setState if the flag is set.

**C — Wakelock worker died silently on `jiggle_cursor` panic.** A future FFI break (macOS update changing `CGEventCreate`-style symbol signatures) would propagate the panic up the worker thread and kill it — leaving `state.active == true` but no actual jiggling happening. LED-on-but-machine-still-sleeps. Fix: `std::panic::catch_unwind(AssertUnwindSafe(jiggle_cursor))` inside the loop logs + continues.

**D — Screenshot Editor: clicking the canvas during a text-input required two clicks.** First click only blurred the input (which committed via onBlur); second click finally triggered the action. Native macOS apps (TextEdit, Pages) commit + act on a single click. Fix: `onCanvasMouseDown` now commits any pending text-input inline before dispatching the click's normal action.

**E — App icon cache grew unbounded.** Was a plain `HashMap<String, String>` — no eviction. Worst-case 500 apps × 5 KB icon = 2.5 MB. Not catastrophic but not defensive. Fix: replaced with `IconCache` LRU (FIFO eviction by insertion order), capped at 100 entries (~500 KB). +3 unit tests pin the cap behaviour, overwrite semantics, and clear().

### Fixed — Pong (`getshaky`): mouse + W/S keys fought each other

The mouse handler was on `window`, so *every* cursor movement anywhere in the popup wrote to `playerY` — meaning if the user pressed W to fly up but their mouse cursor happened to be at canvas Y=200, every tiny mouse twitch snapped the paddle back to 200, paddle looked stuck. Fix: register the mousemove listener on the canvas itself, so off-canvas movement no longer fights the keys. Mouse + keys now coexist contextually exclusive — mouse owns when hovering the play field, keys own otherwise. Header hint updated: "↑↓ / W/S / mouse on field".

### Tests

- +3 Rust tests for `IconCache` (cap eviction, overwrite-keeps-position, clear).
- +1 Rust test for the `catch_unwind` panic-shield semantics used by the wakelock worker.
- **253 Rust + 388 frontend tests now pass.**

### Why 0.37.1

Pure bug fixes — no IPC break, no new feature, no behaviour change for the happy path. Patch-level → `0.x.y`.

## [0.37.0] — 2026-05-25

### Added — Spotlight-like app launcher in the popup search bar (macOS)

Type the start of an app name in the popup → an "app" row surfaces at the top of the list with the app's real icon → Enter launches it (activating an already-running instance instead of spawning a duplicate, via macOS Launch Services). One row, top of the list — Spotlight-like, never crowds the popup with marginal matches.

**Backend (`core/rust-lib/src/app_launcher.rs`)** scans the four standard macOS app directories at startup:

- `/Applications`
- `~/Applications`
- `/System/Applications`
- `/System/Applications/Utilities`

Top-level only, no recursion past `*.app`. Display name = the bundle's filename without the suffix (matches `CFBundleDisplayName` for ~99 % of installed apps). Results sorted alphabetically + deduped by path. Typical machine: 150–400 apps, scanned in 20–100 ms once at startup.

**Launching** via `/usr/bin/open <path>` — macOS Launch Services standard. Already-running instances get activated, not duplicated.

**Icons** (lazy): only the currently-selected app row triggers `get_app_icon`. First call shells out to `sips -s format png -z 128 128 <Resources/*.icns>` (~50 ms cold), result base64-encoded + cached in an `AppIndex.icons` HashMap. Re-selecting the same app = HashMap lookup. Icons fall back to a generic `<AppWindow>` lucide icon while loading or if the bundle has no standard `.icns`.

**Match heuristic** (pure-TS, client-side):

1. Exact prefix match wins (`saf` → Safari, `cal` → Calculator if it comes before Calendar alphabetically).
2. Substring match falls back (`code` → Visual Studio Code).
3. Anything that doesn't prefix- or substring-match: no app row. Spotlight-like — typos don't launch random apps.

The match is **suppressed** when a complete power-command parses (`kill safari` should kill, not launch).

**Settings**: not in v1; the `refresh_apps` IPC exists for a future "Refresh app index" button in the Settings tab. For now, the index is rebuilt on app restart.

### Tests

+4 Rust unit tests: scan finds Terminal.app, results lowercased + alphabetically sorted + deduped. Real-filesystem tests against the running macOS.

**249 Rust + 388 frontend tests now pass.**

### Why 0.37.0

Substantial new user-facing feature surface: new ListEntry kind, new IPC trio, new top-level Rust module, new lazy-icon UX, new lucide icon (AppWindow). Backwards-compatible — no IPC break. Minor digit bump.

## [0.36.0] — 2026-05-24

### Added — Wakelock LED indicator in the popup footer

When `wakelock=1` is active, a small pulsing red LED + `wake` label now appears at the left edge of the popup footer (next to the keyboard-shortcut hints). Toggling `wakelock=0` makes it disappear. Hovering shows a tooltip explaining what it means and how to disable.

The LED itself is a 8×8 px `bg-red-500` dot with a soft red box-shadow bleed-glow, animated via a new `wakelockPulse` keyframe (1.6 s ease-in-out cycle, opacity 0.55→1, shadow 3px→5px glow). Slow enough to read as a gentle status pulse, not a frantic warning.

**Event-driven, no polling.** Backend `commands::wakelock_set` now emits a `wakelock-changed` event with the resulting boolean state after every successful toggle. Frontend reads the initial value once on mount via `wakelock_get`, then subscribes to the event for updates.

### Tests

+3 frontend tests for the LED visibility (hidden default / hidden when false / visible when true).

**245 Rust + 388 frontend tests now pass.**

### Why 0.36.0

User-visible new feature (LED indicator). Backwards-compatible: `wakelockActive` is an optional prop on `Footer`, IPC surface unchanged. Minor digit bump.

## [0.35.2] — 2026-05-24

### Fixed — Three audit findings: timer leak, TOCTOU race, hung-osascript wedge

Three correctness issues spotted during an audit pass, all real but low-frequency. None of them surfaced via user reports yet; better caught now than after a bug report.

**1. `ScreenshotPreview` "Copied" toast timer leaked on unmount.** Clicking Copy then immediately closing the preview within 1.4 s would fire `setCopied(false)` on a stale component, triggering React's "Can't perform a state update on an unmounted component" warning. Fixed by tracking the timer ID in a `useRef` + clearing it both before re-arming and in an unmount-effect cleanup.

**2. `wakelock::set_enabled` TOCTOU race.** The pre-0.35.2 code did `load → compare → store`. Two concurrent `set_enabled(true)` IPC calls could both observe `active=false`, both pass the equality check, and **both spawn a worker thread** — leaving one orphaned (its `stop` Arc overwritten by the second call in `state.stop`, the first worker now running on a now-unreachable stop flag, ticking forever until process exit). Fixed by replacing the load+compare+store with a single `compare_exchange` — the losing thread bails without doing any side-effects. Added 3 new unit tests including a 16-thread concurrent torture test that pins the invariant.

**3. `osascript` calls had no timeout.** Both `frontmost_app::name()` and `finder_selection::read()` shell out to `/usr/bin/osascript` and block on `.output()`. If the target app is hung (frozen Finder, stuck System Events daemon), the call blocks forever — wedging the hotkey handler indefinitely. New module `osascript_util` provides a watchdog wrapper: `Command::spawn()` + `try_wait()` poll loop with `Child::kill()` on timeout. `frontmost_app` uses a 1.5 s cap; `finder_selection` uses 2 s (more headroom for large selections on slow network volumes). Two unit tests pin the behaviour: a fast script returns `Done`, a `delay 5` script is killed within ~250 ms.

### Tests

- +3 Rust wakelock tests (round-trip, idempotent-no-double-spawn, concurrent 16-thread CAS torture).
- +2 Rust osascript-util tests (quick-script Done, slow-script TimedOut + killed in ~250 ms).
- +1 expander test (`block_reason_round_trips_through_anyhow`).
- **245 Rust + 385 frontend tests now pass.**

### Why 0.35.2

Pure bug fixes — no IPC change, no new feature, no behaviour change for the happy path. Patch-level → `0.x.y`.

## [0.35.1] — 2026-05-24

### Fixed — Expander silent-no-op (or wrong-paste!) in terminals

User report: `mfg + Alt+1` expands in CotEditor but does nothing in Terminal.app / iTerm2. Root cause was actually worse than "silent no-op":

1. AX read fails for terminals (no AX-exposed input line) → falls to the clipboard cycle.
2. The clipboard cycle synthesises `Option+Shift+Left` to select the previous word + `Cmd+C` to copy it.
3. **In terminals, `Option+Shift+Left` is not a selection** — it becomes an ESC-sequence (`ESC b` / readline word-back) that the shell interprets as text input. **Nothing gets selected; nothing new lands on the clipboard.**
4. We then read the clipboard, get the *old* contents back, look it up against the snippet table. **If the old clipboard text happens to match a configured abbreviation, we paste the WRONG body** into the terminal command line.

Two-layer fix:

**1. Terminal-frontmost short-circuit.** New `is_terminal_frontmost()` helper checks `frontmost_app::name()` against an allow-list (`Terminal`, `iTerm`, `iTerm2`, `Warp`, `kitty`, `Alacritty`, `Ghostty`, `WezTerm`, `Tabby`, `Hyper`) + substring catch-all. When matched, `expand_at_cursor` bails before the clipboard cycle even starts, with new sentinel `ax.terminal_unsupported`.

**2. Clipboard-unchanged guard.** Even for non-terminal apps that mistreat the keystroke selection (some browser text fields with custom key handlers, etc.), `expand_via_clipboard` now compares the post-cycle clipboard text against the saved pre-cycle text. If they're equal — meaning our select+copy was a no-op — bail with the same sentinel instead of looking up stale clipboard contents.

**Loud failure UX.** The hotkey handler reacts to `BlockReason::TerminalUnsupported` by **opening the popup** with the search bar focused + an 8-second amber hint banner: "Text expansion can't work in terminals. Workarounds: (a) type the abbreviation here in the popup, press Enter to paste; OR (b) configure a Direct hotkey → snippet in Settings (those bypass reading and work in any app, terminals included)." The user knows exactly what happened and what to do.

New tests:
- `error_sentinel_is_stable` extended to pin the new sentinel.
- `block_reason_round_trips_through_anyhow` — pins the `to_sentinel` / `from_error` round-trip for all five variants.

### Why 0.35.1

Real-world bug report fix. No new feature surface, no IPC break. Patch-level → `0.x.y`.

## [0.35.0] — 2026-05-24

### Performance — Expander: caching, batched-AX, smart sleeps (~80–150 ms faster per expansion)

Seven optimisations on top of the v0.34.0 security work — every one based on actual measurement of where the expander loop spends time.

**Caching: 3-4 redundant inits per expansion → 1 cached singleton each.**

| Resource | Pre-v0.35 | v0.35 |
|---|---|---|
| `Enigo::new()` | 3-4× per expansion (one per `select_previous_word` / `send_copy` / `send_paste` / `send_backspaces`) | Cached `OnceLock<Mutex<EnigoCell>>`, init once |
| `IUIAutomation` (Windows) | 2× per expansion (one for `read_word`, one for `is_focused_field_secure`) | Cached `OnceLock<Mutex<UiaCell>>`, init once |
| macOS `AX*` CFString constants | Allocated + released on every call (~5 strings × ~4 calls) | Cached, deliberately-leaked `CFStringRef` per attribute name |

**macOS AX batched read.** `read_focused()` now uses `AXUIElementCopyMultipleAttributeValues` to fetch `AXValue` + `AXSelectedTextRange` in a single XPC round-trip instead of two sequential `AXUIElementCopyAttributeValue` calls. Each AX call is ~2-5 ms; one batched vs. two sequential saves ~5 ms per expansion.

**Smart Alt-release wait.** Pre-v0.35 the handler slept a flat 40 ms at the top of `expand_at_cursor` to let the hotkey's own Alt come up before synthesising chords. Now polls the Alt key state directly:

- **macOS** — `CGEventSourceKeyState(kVK_Option_Left / Right)`.
- **Windows** — `GetAsyncKeyState(VK_MENU)`.

If Alt is already released (the dominant case for any fast typist), the wait is **0 ms**. If still held, we tick at 8 ms granularity up to 80 ms. Median user saves the full 40 ms.

**Background clipboard restore.** `paste_over_selection` + `expand_via_clipboard` used to block the caller for 180 ms after paste, waiting for the target app to consume the body before restoring the user's clipboard. Now the restore is **spawned in a background thread**: the expander returns immediately after the visible paste, and a worker waits 120 ms then checks if the clipboard still equals our body. If yes → restore the saved text. If no (user / another app wrote something in the meantime) → leave it alone, don't clobber. User-perceived expansion latency drops by ~180 ms.

`WatcherState` now derives `Clone` (cheap — two `Arc::clone`s) so the background thread can take an owned handle.

### Reliability — Stale direct-slot pruning

If you delete a snippet that's bound to a direct hotkey, the slot would previously linger forever pointing at a deleted ID — silent no-op on every press, log spam on each. v0.35 sweeps stale slots once at startup via the new `expander::prune_stale_direct_slots(db)`, called in `lib.rs::run::setup` before `register_direct_slots` arms the global shortcuts.

### Code quality — Typed `BlockReason` enum

The four expander error sentinels (`ax.permission_denied`, `ax.secure_input_active`, `ax.inspector_frontmost`, `ax.password_field`) now route through a typed `expander::BlockReason` enum. Hotkey handlers pattern-match on the enum instead of doing fragile string equality on `e.to_string()` — fewer copy-paste typos, easier to spot in code review. The string sentinels stay as the public IPC surface (`BlockReason::to_sentinel()` and `::from_error()` round-trip).

### Why 0.35.0

User-feelable latency improvement (~80-150 ms faster per expansion median) plus a real reliability fix (stale slots) and a typed-API refactor. No breaking changes to the IPC surface. Minor digit bump.

## [0.34.0] — 2026-05-24

### Security — Text-expander hardening across all OSes

Four new safety gates fire **before** the expander does any AX/UIA query or keystroke synthesis. Each one solves a real failure mode that v0.33.x could hit.

**1. Password-field refusal (macOS + Windows).** Before reading the focused field, we now query its security flag and refuse to expand into a password input:
- **macOS** — `AXSubrole == "AXSecureTextField"` on the focused element (catches Cocoa `NSSecureTextField` + WKWebView'd `<input type="password">`).
- **Windows** — `IUIAutomationElement::CurrentIsPassword` on the focused element (catches WinUI / WPF / WinForms password boxes + legacy Win32 EDIT with ES_PASSWORD style).

Without this, an unfortunate `mfg`-typed-in-password-field could expand the signature into a credential store or sudo prompt. New sentinel `ax.password_field`; backend emits `expander-blocked` event with reason `"password"`; popup (if open) shows an amber banner.

**2. macOS `IsSecureEventInputEnabled` check.** When the OS-level secure-event-input flag is on (typical for sudo prompts, password dialogs, some terminal apps), `CGEventPost` is silently dropped at the HID layer. Pre-0.34 the expander would fire, fail invisibly, and the user wondered why nothing happened. Now we probe via `Carbon::IsSecureEventInputEnabled` and bail with sentinel `ax.secure_input_active` + the same banner reason `"secure_input"`.

**3. Inspector-Rust-frontmost guard.** If the user has the popup open and accidentally fires the expander hotkey, the expansion used to dispatch into our own search bar (no-op at best). Now `frontmost_app::name()` is checked first and the expansion is silently skipped with sentinel `ax.inspector_frontmost`.

**4. Windows: clipboard-paste replaces `enigo.text()`.** The old replace path on Windows did `Backspace × N + enigo.text(body)` — which translates each char to a SendInput key event. That breaks:
- Dead-key layouts (US-International `"` + `e` → `"e` instead of `ë`).
- Active IMEs (CJK / Korean input).
- Supplementary-plane Unicode (emoji, math symbols).
- Speed on long bodies (each char is press + release).

New path mirrors macOS: save clipboard → write body → `Ctrl+V` → restore. IME-safe, dead-key-safe, fast. Adds a 4 ms pace gap between Backspaces too (same fix v0.33.0 made on macOS).

**5. macOS AX replace verification — poll instead of fixed sleep.** v0.33.x slept 15 ms after the AX `setAttributeValue` then re-read once to verify. Slow Electron apps occasionally take 20-40 ms to apply, and we'd mis-classify those as `SelectionActive` then double-paste. Now polls every 5 ms up to 60 ms total — returns fast (5-10 ms) when the app is snappy, gives slow ones a fair shake.

### Frontend

- New `expander-blocked` event listener in App.tsx with a 4-second amber banner explaining what happened ("focused field is a password input" / "secure event input is active"). Banner only fires if the popup is already visible — the safety guards explicitly don't steal focus from a password field by raising the popup.

### Why 0.34.0

User-visible behaviour change (expansions can now be blocked, with reasons). Plus a real cross-platform bug fix (Windows IME / dead-keys). Minor digit bump.

## [0.33.0] — 2026-05-24

### Added — `bruno`: Brutto/Netto-Rechner als Power-Command

Type **`bruno 60000`** (yearly gross) or **`bruno 5000m`** (monthly gross) in the search bar and get a full German income-tax + social-contributions breakdown for Steuerjahr 2025 (§32a EStG, simplified). Inline row shows net/month + net/year + Abgabenquote; preview-pane shows the full split (KV / PV / RV / AV + ESt / Soli / Kirche + Grenzsteuersatz). Enter copies the net amount to the clipboard (period-matched: `bruno 5000m` → monthly net, `bruno 60000` → yearly net).

- **Smart defaults**: Steuerklasse I, NRW, 0 children, no church, 2.45 % KV-Zusatz (TK 2025). Override per user in **Settings → Bruno** (Steuerklasse selector, all 16 Bundesländer, kids spinner, church toggle, KV-Zusatz numeric). Persisted via SQLite `settings` table; `bruno-defaults-changed` event refreshes the popup without restart.
- **Pure-TS compute** (`core/frontend/src/lib/bruno.ts`) — no IPC round-trip per keystroke. Ported from the maintainer's [steuerschleuder](https://steuerschleuder.celox.io/) web app. Number-format-tolerant parser (`bruno 60.000` ↔ `bruno 60,000` ↔ `bruno 60000`).
- **32 new frontend tests** (parser + compute + edge cases) + **4 new Rust tests** (settings round-trip).
- Backend: new `core/rust-lib/src/bruno.rs` owns only the persisted defaults (compute lives in TS for instant feedback). New IPCs `bruno_get_defaults` / `bruno_set_defaults`.

### Fixed — Text expander pollution & backspace timing

Two real bugs in the expander code, caught during a code audit:

1. **`paste_over_selection` and `expand_via_clipboard` polluted history** — they wrote the snippet body to the clipboard without arming `mark_self_write`, so the clipboard watcher captured every expansion as a new history entry (and sometimes the restored clipboard too). Pre-v0.33.0 every `Alt+1` expansion silently added the snippet body as a "new" clip.
   - **Fix:** thread `Option<&WatcherState>` through `expand_at_cursor` / `paste_snippet_body` / `expand_via_clipboard` / `paste_over_selection`; arm the watcher before BOTH the body-write and the clipboard-restore. Hotkey handlers in `hotkey.rs` pass `app.try_state::<WatcherState>()`. Backward-compatible signature: `None` = no protection (used in tests).
2. **`send_backspaces` synthesised key events with zero pacing** — older Electron + IME-active terminals coalesce or drop consecutive Backspace presses, leaving a residual character before the snippet body. **Fix:** 4 ms pace gap between presses (skipped after the final key so we don't add idle time before paste). Total overhead: <80 ms for a 20-char abbreviation — imperceptible.

### Docs — README refresh + new image + badges + LoC

- **New hero image** (`docs/ir-w1024.png`, 1.9 MB) — replaces the v3 inspector-rust.png + ir-ff-w1024-optimized.png pair in both READMEs.
- **+5 badges** in the status block: Last commit, Issues, Stars, Tests (235 Rust + 385 TS), Code Style (clippy + eslint).
- **Feature-matrix expansion** with v0.28.0–v0.33.0 entries: `freeze`, `wakelock`, Finder selection actions, resize-preset autocomplete, `bruno`, screenshot preview HUD, annotation editor, app-name filenames.
- **5 new feature sections** (Screenshot preview HUD + editor, Finder selection actions, Bruno, freeze, wakelock) in both `README.md` and `README.de.md`.
- Test counts updated: 213 + 162 → **235 Rust + 385 TS**.

### Why 0.33.0

New user-facing power command (bruno) + bug fixes in the expander (history pollution was real and user-visible). Additive on the IPC surface; backwards-compatible. Minor digit bump.

## [0.32.0] — 2026-05-24

### Added — CleanShot-X-style preview HUD, annotation editor, app-name filenames

Three additions, each useful on its own, packaged together because they all flow from the same screenshot pipeline.

**1. New preview HUD.** The screenshot-preview window is now a CleanShot-X-style dark card with the screenshot itself as the background and six controls floating on top:

- **X** (top-left) — close + discard the capture.
- **Pin** 📌 (top-right) — toggle pin state. While pinned, a *subsequent* screenshot doesn't replace the on-screen preview (new PNG still goes to clipboard + history as usual). Frontend-driven optimistic state, backed by an `AtomicBool` in `PendingScreenshot`.
- **Copy** (centre) — re-write the image to the clipboard. Keeps the preview open. 1.4 s "Copied" confirmation chip.
- **Save** (centre) — write to `~/Downloads` with the app-name prefix + clipboard + history + close.
- **Pencil** ✏️ (bottom-left) — open the annotation editor (see below).
- **Cloud** ☁️ (bottom-right) — placeholder, no-op, tooltip "Coming soon" — wired in a future commit when we pick a host.

**2. Annotation editor.** New Tauri window `screenshot-editor` (routed in `main.tsx` by window label). Five tools:

- **Arrow** (A) — line + filled arrowhead, stroke + colour configurable.
- **Text** (T) — click position, inline overlay input, Enter commits.
- **Rectangle** (R) — empty-outline box.
- **Highlight** (H) — translucent yellow marker, always #facc15 (ignores colour picker on purpose).
- **Blur** (B) — pixelate the underlying source pixels (mosaic, sampled from the original screenshot — non-destructive across undo/redo). Block size scales with the stroke-width slider.

Hotkeys: `⌘Z` / `⌘⇧Z` undo/redo, `⌘S` save, `Esc` cancel. Single-key tool shortcuts (A/T/R/H/B). 4 colour presets (red/yellow/white/black). 2–16 px stroke slider. Canvas is sized to the screenshot's natural pixel dimensions so the saved PNG is full-resolution; CSS scales to fit the viewport. Save bakes the canvas to PNG via `canvas.toDataURL`, ships it to the backend, which writes it as `<App>-<ts>-edited.png`, pushes to clipboard + history, closes the editor, re-shows the preview with the edited image.

**3. App-name in filenames.** The screenshot pipeline now captures the frontmost app's name (`osascript` → `tell application "System Events" to get name of first application process whose frontmost is true`) BEFORE the region picker opens (so we don't catch ourselves). Saved files become **`<App>-YYYYMMDD-HHMMSS.png`** (or `Screenshot-…` if the lookup fails — never blocks the save). Alphabetical sort in Finder groups all screenshots of the same app together. Edited variants get the `-edited` suffix. Uses the same Automation TCC grant the Finder-selection feature already needs.

### Backend changes

- New module `core/rust-lib/src/frontmost_app.rs` — best-effort `osascript` wrapper, 4 unit tests pinning the sanitiser (path separators, control chars, length cap, Unicode).
- New module `core/rust-lib/src/screenshot_editor.rs` — owns the editor webview, the `editor_save` (decode base64 → write Downloads → clipboard + history → re-show preview) and `editor_cancel` (close + re-show preview) IPCs.
- `PendingScreenshot` extended: now holds `current: Mutex<Option<Pending>>` (path + app_name) + `pinned: AtomicBool`. New IPCs `get_pending_screenshot_info`, `set_screenshot_pinned`, `screenshot_preview_copy`. `screenshot_preview_save` updated to bake the app name into the destination filename.
- `commands::run_screenshot_pipeline` captures `frontmost_app::name()` before `hide_popup`, respects the `pinned` flag (skips preview replacement but still writes to clipboard + history).

### Misc

- Tauri capabilities updated on all three platforms — `screenshot-editor` window added to the allow-list alongside `popup` + `screenshot-preview`.
- Frontend `main.tsx` routing extended to mount `<ScreenshotEditor>` for the new window label.

### Why 0.32.0

New user-facing surface (new preview HUD, new editor window, new filename schema), additive on the backend (existing IPCs untouched apart from the filename change in Save). Minor digit bump.



## [0.31.0] — 2026-05-24

### Added — `optim` on Finder files + resize-preset autocomplete

Three small but compounding improvements to the v0.30.0 Finder-selection flow.

**1. `optim` for Finder PNGs.** Same shape as `rz`: select one or more PNGs in Finder, `Ctrl+Shift+F`, type `optim`, Enter. Each PNG is run through oxipng (lossless, max compression) and written next to the source as `<stem>-optim.png`. Non-PNG selections are skipped (oxipng is PNG-only — JPEG support would need `mozjpeg` and is deferred). Mixed selections work; only the PNGs get touched. Originals are untouched. New backend `image_ops::optimize_file_to_neighbor(src)` + IPC `optimize_file(path)`. Outside finder-mode, `optim` still does the v0.18.0 thing (clipboard PNG → `~/Downloads/inspector-rust-optim-<ts>.png`).

**2. Multi-file resize.** Already shipped in v0.30.0 — `rz <W>x<H>` with multiple Finder images selected runs in parallel (`Promise.all`) and writes a `<stem>-<W>x<H>.<ext>` for each. Documented now in the CHANGELOG since the user asked.

**3. Resize-preset autocomplete.** Type `rz` (or `rz <partial-digits>`) and a list of preset dimensions appears in the suggestion list — `1920x1080`, `1280x720`, `1024x768`, `800x600`, `500x500`, `200x200`, `100x100`. Each is a labelled suggestion ("Full HD · 1920×1080", "HD · 1280×720", …). Filter narrows as you type (`rz 19` → only `1920x1080`; `rz 5` → only `500x500`).

Three keys do different things on a focused preset row:

- **Enter** — runs the resize directly (operates on Finder selection if in finder-mode, else clipboard image).
- **Tab** — fills the preset's completion into the search bar, parks the caret at the end. Lets you tweak before hitting Enter.
- **→ (Arrow Right)** — same as Tab, but *only when the caret is already at the end of the input* (so → still moves the caret within typed text otherwise).

### Behind the scenes

- New pure function `resizePresetSuggestions(query)` in `core/frontend/src/lib/commands.ts`. Filter-by-prefix on the dimension string; returns empty once the user typed a complete `WxH` (the runnable command row carries the load from there). 7 new unit tests pin the behaviour.
- New `command-suggestion` Enter branch in App.tsx: if the completion parses as a complete `resize` command, dispatch the resize directly (finder vs. clipboard logic mirrored from the regular `command` branch). Otherwise the existing autocomplete-only behaviour.
- Global keydown handler attached when a `command-suggestion` row is selected: intercepts Tab unconditionally, and → only when the caret is at the input's end. Same shape as the existing opener `← / →` cycling handler — capture-phase listener that's mounted/unmounted with the selection state.

345 frontend tests (+7).

### Why 0.31.0

New IPC + new user-visible interaction surface (preset rows + Tab/→ semantics). Additive. Bumping the minor digit.

## [0.30.1] — 2026-05-23

### Changed — Automation→Finder folded into the Set-up-permissions flow

The v0.30.0 Ctrl+Shift+F feature introduced a third macOS TCC grant (Automation → Finder) — but only surfaced it as an in-popup amber banner when the user hit the hotkey and it failed. That left a gap: the consolidated permissions card in Settings only tracked Accessibility + Screen Recording, so a user setting up Inspector Rust for the first time wouldn't know the third grant existed until they happened to try Finder selection.

Now the card tracks **all three** grants:

- **New `PermRow`** — "Automation → Finder" with the same live-status indicator, deep-link "Open Settings" button, and 1 s poll-while-not-granted pattern as the other two rows.
- **"Set up permissions" chains all three** — clicking the button walks Accessibility → Screen Recording → Automation→Finder in order, auto-firing each next still-missing grant once the previous flips to granted.
- **Initial probe on Settings mount** — `getFinderAutomationStatus()` calls a no-op `tell application "Finder" to get selection` through `osascript`. macOS has no separate "not determined" state for AppleEvents TCC, so the first probe ever doubles as the prompt — that's the only way to fire it. The `NSAppleEventsUsageDescription` injected into Info.plist (v0.30.0) gives the prompt its explanation copy.
- **"Reset stale grants" + "Re-check now"** also extended — both now cover the AppleEvents bucket via `tccutil reset AppleEvents io.celox.inspector-rust`.

New IPCs: `get_finder_automation_status`, `open_finder_automation_settings`, `force_reset_finder_automation_grant`. Same shape as the existing `get_screen_recording_status` / `open_screen_recording_settings` / `force_reset_screen_recording_grant` trio.

### Why 0.30.1

UX polish on the v0.30.0 feature — no new feature surface, just folding the third permission into the existing setup flow so it's discoverable + recoverable from one place. Patch-level → `0.x.y`.

## [0.30.0] — 2026-05-23

### Added — `Ctrl+Shift+F` reads the Finder selection (macOS)

Press **`Ctrl+Shift+F`** anywhere on macOS → the popup opens with whichever files you have selected in Finder listed at the top, ready to act on. Currently shipping action:

- **Resize images** — with one or more images selected, type `rz 1200x800` and hit Enter. Each selected image is Lanczos3-downscaled and written next to its source as `<stem>-1200x800.<ext>` (PNG → PNG, JPEG → JPEG, etc. — format is preserved). The originals are untouched.
- **Open in default app** — hit Enter on a file row to launch it.

Mixed selections (some images, some non-images) work fine: `rz` only touches the image rows. Non-image rows are still listed and openable.

### Behind the scenes

- New module `core/rust-lib/src/finder_selection.rs` shells out to `osascript -e 'tell application "Finder" to get selection'` and parses the POSIX paths back. ~30 ms cold round-trip. The `-1743` errAEEventNotPermitted (TCC Automation denied) error is translated to a `finder.automation_denied` sentinel, mirroring the existing `ax.permission_denied` / `screen.permission_denied` pattern, so the frontend can show a tailored "open System Settings → Privacy → Automation → Inspector Rust → Finder" banner instead of a generic error.
- New `image_ops::resize_file_to_neighbor(src, w, h)` — opens the source file, Lanczos3-resizes, writes the result with the same format alongside the original.
- New IPCs: `get_finder_selection() -> Vec<FinderItem>`, `resize_file(path, w, h) -> String` (returns the output path), plus the `run_finder_selection_pipeline` worker for the hotkey path.
- New global shortcut `Ctrl+Shift+F` registered alongside the existing OCR / screenshot / eyedropper hotkeys.
- New `ListEntry` kind `"finder-file"`; rendered with the existing file icon + a "finder" chip in the row; PreviewPanel shows the path + size + a "type `rz 1200x800` to resize all selected images" hint for images.

### Permissions

To talk to Finder via AppleEvents, a Hardened-Runtime app needs three things in alignment:

1. **Entitlement** `com.apple.security.automation.apple-events` (added back to `entitlements.plist`; the historical comment that warned against it applied only to apps that didn't actually use AppleEvents — we do now).
2. **Info.plist key** `NSAppleEventsUsageDescription` (injected post-build by `scripts/install-macos.sh` via `plutil -replace`, since the Tauri 2 bundler has no first-class field for arbitrary Info.plist keys).
3. **User grant** in *System Settings → Privacy & Security → Automation → Inspector Rust → Finder*. macOS prompts on the first Ctrl+Shift+F press; the in-app banner reminds you where to find the toggle if you missed the prompt.

### Why 0.30.0

New feature surface (new hotkey, new IPC, new entitlement, new Info.plist key), additive. Existing flows are unchanged. Bumping the minor digit.

## [0.29.0] — 2026-05-23

### Added — `wakelock=1` keep-awake mouse-jiggle

Type **`wakelock=1`** (or `wakelock1`) into the search bar and the cursor starts jumping 1 px right and immediately back every 60 s in the background. Defeats:

- macOS screen-saver / display-sleep idle timers.
- Teams / Slack / Discord "away" detection (anything that watches for HID activity).
- App-level "idle" UX (auto-pause on streaming sites, etc.).

Disable with **`wakelock=0`** (or `wakelock0`). State is in-memory only — restarting the app clears it (intentional: you shouldn't accidentally leave a stranger's machine awake).

Three platforms:

- **macOS** — `CGEventCreateMouseEvent(kCGEventMouseMoved, …)` + `CGEventPost(kCGHIDEventTap, …)` via raw `#[link(name = "ApplicationServices")]` FFI. Reads cursor with `CGEventGetLocation`. Same Accessibility TCC grant the paste / expander pipelines already need.
- **Windows** — `GetCursorPos` + `SetCursorPos` from the bundled `windows` crate (`Win32_UI_WindowsAndMessaging`). No extra permission.
- **Linux X11** — `XQueryPointer` + `XWarpPointer` on the root window via raw `#[link(name = "X11")]` FFI; `Display` connection cached for the app lifetime. Wayland is a no-op (the protocol denies global cursor synth at the security layer — a future D-Bus `org.freedesktop.ScreenSaver` inhibit would be the proper path there).

Architecture: `core/rust-lib/src/wakelock.rs` owns a Tauri-managed `WakelockState` (`active: AtomicBool`, worker `JoinHandle`, fresh per-worker stop `Arc<AtomicBool>` to avoid resurrecting a still-sleeping previous worker on rapid off→on→off). Worker thread polls a 200 ms cancel-tick wait so toggling off lands within 200 ms instead of waiting up to a minute. Two synthetic moves spaced 30 ms apart (one to `(x+1, y)`, one back to `(x, y)`) — the OS sees two distinct HID events, the user sees nothing.

Frontend: two visible `COMMANDS` entries (`wakelock=1` / `wakelock=0`) + two `hidden: true` aliases (`wakelock1` / `wakelock0`) so the autocomplete stays tidy. IPC: `wakelock_set(enable)` / `wakelock_get()`.

### Why 0.29.0

New user-facing command + new Rust module + new platform FFI surface (mouse synth on all three desktop OSes). Additive — no behaviour change for anyone who doesn't type the command. Bumping the minor digit fits.

## [0.28.9] — 2026-05-23

### Added — Native cursor queries on Windows + Linux X11

The screenshot preview's cursor-follow polling (every 200 ms it asks the backend to re-position itself if the cursor crossed to a different monitor) was macOS-only. The non-macOS path returned `None`, so the preview anchored to the primary monitor and never followed the cursor across screens. Filled in:

- **Windows** — `win_cursor::position_in_pixels` calls `GetCursorPos` (from the already-bundled `windows` crate, feature `Win32_UI_WindowsAndMessaging`). Result is physical pixels in the virtual-screen coord system, same units as Tauri's `Monitor::position` — direct bounds-check, no scale conversion.
- **Linux X11** — `x11_cursor::position_in_pixels` calls `XQueryPointer` on the root window via raw FFI (`#[link(name = "X11")]`). The `Display` connection is opened once via `OnceLock<Mutex<Option<DisplayPtr>>>` and reused for the app lifetime (opening one per 200 ms poll would burn server-side state).
- **Linux Wayland** — deliberately denied at the protocol level. `is_wayland()` (checks `WAYLAND_DISPLAY` + `XDG_SESSION_TYPE`) short-circuits to `None`, falling back to the primary monitor.

Same picker function (`pick_cursor_monitor_globally`) — now per-OS branch with each native API filling in the same `Option<Monitor>` contract.

### Why 0.28.9

Feature parity: cursor-follow now works on all three desktop OSes (modulo Wayland, where it's an OS-level restriction). Backwards-compatible. Patch-level.

## [0.28.8] — 2026-05-23

### Changed — Dock-aware preview position + no auto-hide + X-close button

Two refinements on the screenshot preview:

1. **Dock-aware bottom margin** — the v0.28.7 fixed 110 px bottom margin cleared the Dock but wasted space on monitors *without* a Dock (preview sat absurdly high). Now the bottom margin is computed dynamically from `NSScreen.visibleFrame`: the Dock height for whichever screen the cursor is on (0 if no Dock there) plus a 24 px gap. Preview sits just above the Dock on the Dock screen, and 24 px from the edge on every other screen. Works regardless of Dock size (default / Magnification: Large).

2. **No more auto-hide; X to close** — the 6 s auto-hide timer (which silently triggered Discard) is gone. The preview now stays put until you explicitly act on it. A new top-right **X** button closes the window (cleans up the temp file like Discard; the screenshot is already on the clipboard from the immediate-write step in v0.28.2, so closing is non-destructive).

Implementation: `cursor_screen_bottom_inset_pts()` in the new `ns_screen` sub-module uses `objc2` to call `[NSScreen screens]` + `[screen visibleFrame]` for whichever screen contains the global cursor (queried via `NSEvent.mouseLocation` in Cocoa coords).

### Why 0.28.8

Two UX refinements on the preview — backwards-compatible. Patch-level → `0.x.y`.

## [0.28.7] — 2026-05-23

### Fixed — Screenshot preview clears the Dock + follows cursor live

Two long-standing annoyances on the floating screenshot preview window:

1. **Dock occlusion.** The preview's 24 px bottom margin wasn't enough to clear the macOS Dock (default ~78 px, "Magnification: Large" up to ~128 px). Bumped the bottom margin to **110 px** so the preview sits cleanly above the Dock at any standard size. Side margin stays at 24 px.

2. **Cursor follow only on click.** The 200 ms reposition polling used `WebviewWindow::cursor_position()`, which is a Tauri/tao wrapper that returns coordinates from the *last mouse event delivered to the window* — so polling from an inactive preview window kept reporting a stale position until the user actually clicked on a different monitor. Replaced with **raw FFI `CGEventGetLocation`** on a freshly-synthesised event from the null source — returns the **global** cursor position in real time, exactly what we need. The preview now jumps to the new monitor the moment the cursor crosses the boundary.

The bounds check is done in POINTS (Carbon coords from `CGEventGetLocation`) against each monitor's physical-pixel bounds divided by its scale factor — handles mixed-DPI multi-monitor setups correctly.

Both changes are macOS-specific; the non-macOS code path falls back to `primary_monitor()`.

### Why 0.28.7

Two UX fixes on an existing feature — backwards-compatible. Patch-level → `0.x.y`.

## [0.28.6] — 2026-05-23

### Fixed — `freeze` callback now uses **raw FFI** (was: core-graphics wrapper)

The v0.28.5 callback used `core-graphics 0.24`'s `CGEventTap::new` closure API and returned `None` to drop events. Diagnostic logs proved the callback fired with `lock_active=true` on every key press — yet the events still reached focused apps. Best hypothesis: the core-graphics wrapper's `Option<CGEvent>` → C-ABI return path silently mis-translates `None` on macOS Sonoma (possibly due to having both 0.24 and 0.25 of the crate in the dep tree).

This release drops the wrapper entirely and uses **raw FFI**: `#[link(name = "ApplicationServices")]` for `CGEventTapCreate` / `CGEventTapEnable` / `CGEventGetIntegerValueField`, `#[link(name = "CoreFoundation")]` for `CFMachPortCreateRunLoopSource` / `CFRunLoopGetMain` / `CFRunLoopAddSource`. The callback is a plain `extern "C" fn` returning `CGEventRef` — `event` for pass-through, `std::ptr::null_mut()` for drop. Same C-ABI semantics as `macos-lock.py` (which works via PyObjC).

Tap installed on the main thread's run loop (where Tauri's NSApp is already spinning). Diagnostic log line gained `(raw FFI)` suffix so the install path is identifiable.

`core-graphics` + `core-foundation` dependencies dropped from `core/rust-lib/Cargo.toml` (transitive pulls from Tauri remain in the lock file).

### Why 0.28.6

Continuing to chase down the freeze regression — backwards-compatible. Patch-level → `0.x.y`.

## [0.28.5] — 2026-05-23

### Fixed — `freeze` event tap installed on the **main** run loop now

v0.28.3 / v0.28.4 ran the CGEventTap on its own worker thread with `CFRunLoopRun`. Compiled cleanly, returned success — but on macOS Sonoma+ never actually intercepted anything. Apple's docs don't promise that pattern works, and evidence (the user) said it didn't.

This release installs the Mach-port source on **the main thread's run loop** instead — the one Tauri's `NSApp.run` is already spinning. This is what the `macOS-lock` Python script does (it blocks main with `CFRunLoopRun`) and what every real Cocoa-app event-tap example does. The tap object is `std::mem::forget`-ed so it outlives the IPC handler that installed it (Drop would otherwise tear down the Mach port).

Plus first-eight-callback `tracing::info!` lines — launch the binary from a terminal (`/Applications/InspectorRust.app/Contents/MacOS/inspector-rust`) and you'll see whether the tap is receiving events.

### Why 0.28.5

Bug fix on top of v0.28.4 — backwards-compatible. Patch-level → `0.x.y`.

## [0.28.4] — 2026-05-23

### Fixed — `freeze` errors now surface to the user (silent-fail diagnosis)

v0.28.3 swallowed `CGEventTap::new` failures inside the background tap thread — if the tap couldn't be created (most commonly because Accessibility for the just-installed binary wasn't actually granted yet), the IPC returned Ok and the user saw "lock activated" but nothing actually blocked.

`start_input_lock` now uses a `mpsc::channel` handshake with the tap thread: it waits up to 2 s for the thread to report whether `CGEventTap::new` succeeded. On failure, the IPC returns the actual error string (mentions Accessibility) instead of pretending success. On 2 s timeout it surfaces a "stuck waiting on Accessibility prompt" hint. Extra `tracing::info!` lines around tap install / run loop entry-exit for log-side debugging.

If `freeze` still doesn't block input on your machine, the toast will now name the actual reason.

### Why 0.28.4

Diagnostic hardening of the v0.28.3 freeze implementation — backwards-compatible. Patch-level → `0.x.y`.

## [0.28.3] — 2026-05-23

### Fixed — `freeze` actually works now (native CGEventTap on macOS)

The v0.28.0 implementation used `rdev::grab` with the `unstable_grab` feature; that combination crashed Inspector Rust on macOS (v0.28.2 disabled it with a clear error). This release replaces it with a **native `CGEventTap`** via the `core-graphics` + `core-foundation` crates — the same Quartz Event Services API the original `pepperonas/macOS-lock` Python script uses through PyObjC.

- Tap installed at HID-session level + `HeadInsertEventTap` placement, so it sees every keyboard / mouse / trackpad event before any other process — exactly what's needed to swallow them.
- Runs on a dedicated `input-lock-tap` thread with its own `CFRunLoopRun`. Toggle behaviour via `LOCK_ACTIVE` atomic flag so subsequent lock cycles don't pay the tap-creation cost.
- Chord matching unchanged — press `i + r` (default) to unlock; configurable in Settings → Input Lock.
- Requires Accessibility (the existing grant covers it). If missing, the tap creation fails and `start_input_lock` returns an error without crashing.
- **Windows / Linux** still return "not implemented yet" — the Settings UI + trigger stay platform-agnostic; a native port (Windows `WH_KEYBOARD_LL`, Linux X11) is the next step.
- `rdev` dep dropped entirely.

Safety hatch: `⌥⌘Esc` (Force Quit) is processed by macOS WindowServer above any user-level event tap and cannot be intercepted — you can always recover even if you forget the chord.

### Fixed — Screenshot preview now actually follows the cursor between monitors

The v0.28.2 Rust background thread that called `set_position` from a `std::thread` was unreliable on macOS (Tauri's main-thread dispatch from a bare worker thread is flaky). Replaced with **frontend-driven polling**: the preview React component calls a new `reposition_preview_to_cursor` IPC every 200 ms, and Tauri's IPC layer marshals the `set_position` onto the main thread cleanly. Behaviour identical from the user's POV — the preview only "jumps" on monitor changes, not on every pixel of mouse motion — just actually working now.

### Why 0.28.3

Two real-feature-completes (freeze works, cursor-follow works) — backwards-compatible. Patch-level → `0.x.y`.

## [0.28.2] — 2026-05-23

### Fixed (critical) — `freeze` (input lock) was crashing the app

Typing `freeze` + Enter terminated Inspector Rust on macOS. Root cause: the v0.28.0 implementation spawned a worker thread that called `rdev::grab(...)` with the crate's `unstable_grab` feature — that combination triggers a process-level abort in the CGEventTap setup we couldn't isolate quickly.

`input_lock::start_input_lock` now returns an error immediately (with a clear message) instead of spawning the grab thread. The settings UI + the `freeze` trigger + the chord validation all stay in place so the planned replacement (native CGEventTap via `objc2`, parallel to how OCR uses Vision) just drops in.

If you typed `freeze` before and your app died — sorry. v0.28.2 is now safe; the worst that can happen is a clear error toast.

### Changed — Screenshot preview follows the cursor between monitors

The CleanShot-X-style preview spawned on the cursor monitor at capture time but stayed there if you dragged the mouse to another display. Now a 200 ms-ticking follower thread re-positions the window whenever the cursor crosses to a different monitor. Within a single monitor the target stays fixed (we anchor to the same bottom-left), so it only "jumps" on monitor changes, not on every pixel of mouse motion.

### Changed — Screenshots land on the clipboard immediately

Before, the captured PNG only hit the clipboard when you clicked **Save**. Now it lands on the clipboard the instant the capture completes — paste it anywhere right away. **Discard** still cancels the on-disk file + history entry, but leaves the clipboard alone (you may already have pasted it elsewhere; nulling it from under you would be surprising). **Save** still writes the file + history (and re-writes the clipboard idempotently in case you copied something else in between).

### Why 0.28.2

Critical crash fix + two UX refinements. Patch-level → `0.x.y`.

## [0.28.1] — 2026-05-23

### Added — "Plain text" string-transform (`Cmd/Ctrl+^`)

A new 12th transform on the TransformBar: **Plain text** — strips HTML / RTF markup, decodes named + numeric entities (`&amp;`, `&nbsp;`, `&#39;`, …), and commits the bare text as a new history entry + clipboard write. Use case: you copied a styled paragraph from a webpage / Notion / Slack and want the *text*, no formatting.

Bound to **`Cmd/Ctrl+^`** (Mac users on German ISO press a single bare `^` key with Cmd; US/intl users get the same chord as `Cmd+Shift+6` since `^` requires Shift on those layouts). The handler accepts either Shift state for `^` specifically — digit shortcuts (1–9) still reject Shift to leave `Shift+digit` (`!@#$…`) free.

Implementation uses the platform `DOMParser` for correctness — handles malformed HTML, nested tags, and the full entity set without us reimplementing an HTML spec. A regex-based fallback covers test environments without a DOM.

### Why 0.28.1

UX extension on an existing surface, backwards-compatible. Patch-level → `0.x.y`.

## [0.28.0] — 2026-05-23

### Added — Input lock (`freeze` command, macOS-lock-style)

Inspired by `pepperonas/macOS-lock`. Type **`freeze`** in the popup search bar → all keyboard, mouse and trackpad input is blocked until you press the configured **unlock chord**. The default chord is **`i + r`** (hold `i`, press `r`); configurable in **Settings → Input Lock** via a "Capture chord" widget that listens for keys held simultaneously.

Cross-platform via the `rdev` crate:

- **macOS** — `CGEventTap`. Uses the existing Accessibility grant.
- **Windows** — `WH_KEYBOARD_LL` + `WH_MOUSE_LL` low-level hooks. No extra permission.
- **Linux X11** — `XGrabKeyboard` + `XGrabPointer`. **Wayland is NOT supported** (rdev limitation); `start_input_lock` returns a clear error there.

**Safety hatches that always work** — OS-level system shortcuts cannot be intercepted by user-level event taps, so you can never truly lock yourself out of the machine:

- macOS: `⌥⌘Esc` → Force Quit Applications.
- Windows: `Ctrl+Alt+Del`.
- Linux: `Ctrl+Alt+F2` (switch VT).

### Implementation

- New module `core/rust-lib/src/input_lock.rs` with the persistent grab thread (spawned once at first lock activation, lives for the rest of the app — `rdev::grab` has no clean stop API; subsequent locks just flip an atomic flag), the `Key` parser (`key_from_str("i")` → `rdev::Key::KeyI`), and the chord-match callback. 4 unit tests.
- `start_input_lock` validates the chord and rejects empty / unparseable ones + Wayland sessions up front.
- Settings key `input_lock.unlock_keys` (JSON array). New IPCs `get_input_lock_chord` / `set_input_lock_chord` / `start_input_lock`.
- `lib/commands.ts::COMMANDS` gains the `freeze` entry; `App.tsx` dispatches it to `startInputLock()`.
- `SettingsPanel.tsx` gains a new **Input Lock** section with a chord-capture widget that listens for keydowns + commits on first keyup. Esc cancels.
- Workspace dep `rdev = "0.5"` with the `unstable_grab` feature.

### Why 0.28.0

A whole new system-level capability — backwards-compatible, no breaking changes. Feature-level → `0.x.0`.

## [0.27.0] — 2026-05-23

### Added — CleanShot-X-style floating screenshot preview

After `Ctrl+Shift+S` (or the tray "Screenshot Region" entry), a small frameless preview window now appears in the **bottom-left corner of the monitor your cursor is on** — exactly like CleanShot X. Three actions on the preview:

- **Save** — moves the PNG to `~/Downloads`, writes it to the system clipboard, adds a history entry.
- **Discard** — deletes the temp file. No clipboard, no Downloads, no history.
- **Edit** — moves to `~/Downloads` and hands the file to the system default image viewer (Preview.app on macOS, the default `.png` handler on Windows / Linux).

The preview **auto-hides after 6 s of no interaction** (counts as Discard, so a forgotten capture doesn't leave temp files around). Hovering the preview cancels the timer.

**Behaviour change**: until you click one of the three actions, the screenshot is **not** copied to the clipboard, **not** written to Downloads, and **not** added to history (the old v0.26.3 default did all three automatically). So Discard is now a true discard.

Multi-display aware via the existing `pick_cursor_monitor` machinery: the preview always pops on the screen the user just captured on, not a random fixed display.

### Implementation

- New module `core/rust-lib/src/screenshot_preview.rs` with `PendingScreenshot` Tauri state + `show_preview` window-builder + the three action IPCs (`screenshot_preview_save` / `_discard` / `_edit`).
- The capture pipeline writes the PNG to `~/Library/Caches/InspectorRust/screenshot-pending-<ts>.png` (or per-OS cache dir), stashes the path in `PendingScreenshot`, then builds (or reuses) a frameless transparent `screenshot-preview` Tauri window positioned at the cursor monitor's bottom-left with a 24 px margin.
- `main.tsx` routes by `getCurrentWebviewWindow().label` — the preview window mounts only the new `<ScreenshotPreview>` React component, not the full clipboard browser, so it's lightweight.
- `<ScreenshotPreview>` loads the PNG via `convertFileSrc(path)` (Tauri asset protocol — newly enabled in all three shells, scoped to the cache dir for safety) and renders the thumbnail + three action buttons + the auto-hide timer.
- Workspace `tauri` features gained `macos-private-api` (transparent windows) and `protocol-asset` (the `convertFileSrc` path); all three per-OS `tauri.conf.json` got matching `macOSPrivateApi: true` and `assetProtocol.scope` entries; capabilities extended to include the `screenshot-preview` window label.

### Why 0.27.0

Whole new interactive surface for an existing action — backwards-compatible (no IPC removed, no command renamed). Feature-level → `0.x.0`.

## [0.26.4] — 2026-05-23

### Added — String-transform bar on HTML + RTF entries

The transform bar (`Cmd/Ctrl+1…9` → remove vowels / UPPER / lower / Title / camel / snake / kebab / Base64 / URL-encode, plus click-only Base64/URL decode) now shows on **HTML** and **RTF** clipboard entries too, not just plain text. It operates on the entry's `content_text` (the plain-text representation), so the existing transforms apply directly.

This also covers a subtle dedup case: when the OCR-recognised text matches the SHA-256 hash of an existing HTML entry (e.g. the same text was previously copied from a webpage), the database upserts the existing HTML row rather than inserting a new Text row — without this fix the transform bar would have been hidden on that "OCR" result.

Plain-text and OCR-result entries (`content_type = Text`) already showed the bar; this extends the coverage so any text-bearing clip — text, OCR, HTML, RTF — has the same toolbox.

### Why 0.26.4

UX coverage extension on an existing feature — backwards-compatible. Patch-level → `0.x.y`.

## [0.26.3] — 2026-05-23

### Changed — OCR no longer saves the source PNG to history by default

The OCR pipeline used to upsert **two** history entries on every run — the source screenshot AND the recognised text — which doubled-up the history list with images you can't usefully paste back into a text field. The default is now **only the text**; the source PNG is captured for the recognition step and then discarded.

Settings → **Capture → "Keep OCR source image in history"** toggles the old behaviour back on for users who want to re-OCR or keep the source visible. Defaults to `false`. Persisted under the settings key `ocr.save_source_image`.

The system clipboard still receives only the recognised text (unchanged from before).

### Fixed — `Shift+↑` / `Shift+↓` system volume change is now instant

The volume shortcut spawned `osascript` **twice** per press (read current, then set new), ~150 ms each, so a single press took ~300 ms before the system moved — and a rapid Shift+↓ chord stacked latencies.

`adjust_system_volume` now:

- **Combines read + clamp + set into one `osascript` invocation** (multiple `-e` flags, atomic AppleScript). Saves ~50 % of the per-call latency.
- **Spawns the script on a worker thread** so the IPC resolves immediately — the next Shift+↑ / Shift+↓ press isn't queued behind the previous one. macOS plays its own native volume-change feedback, so the caller doesn't need to wait for the result.

Net result: pressing Shift+↑ feels native instead of laggy.

### Why 0.26.3

UX default flip + performance fix + new toggle — backwards-compatible (the old OCR behaviour is opt-in). Patch-level → `0.x.y`.

## [0.26.2] — 2026-05-23

### Fixed — HTML clipboard preview no longer clashes with the app theme

The HTML preview rendered the clipboard's HTML in a sandboxed iframe with a hardcoded white background, and the pasted HTML carried the source page's own inline `style="…"` attributes — so copying from any styled webpage produced a glaring white box on top of Inspector Rust's dark UI, often with the page's own colours leaking through (black-on-black blocks, neon highlights, etc.).

The iframe now:

- has its container `bg-` set to the app's `--color-surface` instead of hardcoded `bg-white`,
- ships a base `<style>` in its `srcDoc` that pulls live theme colours from the parent's CSS custom properties (`--color-fg` / `--color-surface` / `--color-accent` / …) and applies them with `!important` to `body, body *`, so pasted-in inline colours don't fight the theme,
- declares `color-scheme: dark` so browser-default scrollbars / form widgets match,
- gives `<a>`, `<code>`/`<pre>`, `<blockquote>`, `<table>` and `<img>` sensible theme-aware defaults.

Only colour and background are overridden — layout (margins, padding, sizing, borders' radius) survives, so the preview keeps the source's structure while reading like the rest of the app.

### Why 0.26.2

Visual polish for the HTML preview — no new feature, no breaking change. Patch-level → `0.x.y`.

## [0.26.1] — 2026-05-23

### Changed — `opener` easter egg: ← / → cycle through openers

Walking through the top-100 list via extra keystrokes (the seed-hash re-roll) was awkward. The opener row now reacts to **`←`** and **`→`** to step to the previous / next opener while the opener row is the selected entry:

- First activation seeds the index deterministically via `pickOpenerIndex(query)` — re-typing `opener` lands on the same starting line, so the easter egg feels predictable.
- The current pick lives in component state, so cycling state is preserved across additional keystrokes (the trigger is `^opener\b`, so `opener foo bar` still keeps your cycled pick).
- The arrow handler only attaches while `combined[selected].kind === "opener"`, so once you arrow Down to a clipboard row, ← / → fall through to the search-bar input's normal cursor-movement.
- HUD copy updated: "type any key to re-roll" → "← / → cycles to the previous / next opener" (HistoryItem chip tooltip + PreviewPanel hint).

`lib/openers.ts` gains the `pickOpenerIndex(seed)` helper (kept `pickOpener` as a thin wrapper). +3 unit tests pinning the new helper. Frontend total: **330**.

### Why 0.26.1

UX refinement of the v0.26.0 easter egg — no new feature surface, backwards-compatible. Patch-level → `0.x.y`.

## [0.26.0] — 2026-05-23

### Added — `opener` hidden German pickup-line easter egg

A third hidden trigger, alongside `getshaky` (Pong) and `rockthebox`/`rockthabox` (Snake). Typing **`opener`** in the popup search bar surfaces a random German pickup-line at the top of the list. Press Enter to paste it into the focused app.

- **Curated source** — 100 openers exported from the maintainer's `nicetobenice_db` PostgreSQL DB on the VPS (`69.62.121.168`), ranked by their personal ratings + favourites (DESC), tie-broken on the global `avg_rating`. Embedded as `core/frontend/src/lib/openers-data.ts` (no live DB call at runtime).
- **Re-roll on every keystroke** — the picker is a pure FNV-1a-style hash of the full query string. Identical query → identical pick (React render loop is stable, no flicker), and each extra keystroke (`opener `, `opener a`, `opener xy`, …) re-seeds → new pick.
- **Trigger** — `^opener\b` (case-insensitive, whitespace-tolerant): matches `opener`, `Opener`, `opener foo`, but NOT `openers` / `bopener`. Deliberately **not** in the `COMMANDS` catalogue → never appears in autocomplete; you have to know the word.
- **Integration** — new `kind: "opener"` in the `ListEntry` union; `HistoryItem` renders it with a `Sparkles` icon + an italic line; `PreviewPanel` shows the full text with a "type any key to re-roll" hint. Enter triggers `pasteText(opener)`.
- **Coverage** — 18 new tests (10 openers + 8 trigger), 327 frontend tests total.

### Why 0.26.0

A whole new interactive surface — backwards-compatible, no breaking changes. Feature-level → `0.x.0`.

## [0.25.2] — 2026-05-23

### Fixed — Direct-hotkey snippets now delete the typed abbreviation

If you typed an abbreviation (e.g. `aiplan`) and pressed the direct hotkey for that snippet, the body was *appended* — you got `aiplan<body>` instead of `<body>`. `expander::paste_snippet_body` now synthesizes `len(abbreviation)` Backspaces before pasting the body, so typed-then-trigger replaces the abbreviation cleanly (character count, not byte length, so multibyte abbreviations like umlauts work).

Trade-off, documented honestly: this is **blind** — the slot still doesn't read the field (otherwise it'd lose the "works in terminals" guarantee). Pressing the hotkey **without** first typing the abbreviation deletes N characters before the cursor. The normal flow is type-then-trigger, so this matches user expectation in the common case.

### Fixed — `Ctrl+Shift+S` now saves the screenshot on a single press

Before, the shortcut needed an awkward *double-tap within 1.5 s* to actually save a PNG to disk — a single press only wrote the image to the clipboard, and the only way to discover the "save" behaviour was to read the source. Now **one press** of `Ctrl+Shift+S`:

- writes the PNG to the system clipboard (as before),
- **auto-saves** to `~/Downloads/inspector-rust-screenshot-<timestamp>.png`,
- emits the existing `screenshot-saved` event so the frontend toast confirms the file path,
- and persists the history entry.

The double-tap mechanism is removed entirely (along with the `SCREENSHOT_SAVE_FILE` / `SCREENSHOT_LAST_MS` atomics and the Windows in-marquee `S`-key save-mode toggle); the only remaining state is `SCREENSHOT_IN_PROGRESS`, which still debounces a second press while the picker is open.

A file-write failure is non-fatal — clipboard and history still succeed, so the user never loses a capture.

### Added — Frontend tests for the IPC contract + fuzzy-search hook

- **`lib/ipc.test.ts`** (25 tests) — pins the IPC wrapper contract: every wrapper in `ipc.ts` calls `invoke("<rust_command_name>", {…})`, and the two halves are wired only by an exact string + the snake_case argument keys Tauri's auto-conversion expects. A typo on either side silently breaks the call. These tests mock `@tauri-apps/api/core` and assert command name, argument shape, default values, return-value pass-through, and error propagation across the seven IPC namespaces (history, snippets, notes, settings, expander, permissions, lifecycle).
- **`hooks/useFuzzySearch.test.ts`** (8 tests) — empty / whitespace queries return the entry list unchanged, substring + fuzzy matches surface the right rows, the no-match case returns `[]`, the `useMemo` cache holds across re-renders with identical inputs, recomputes when the query changes, and an empty entry list doesn't crash.

Total frontend test count: **309** (was 276); Rust workspace: **227** (was 216 in v0.25.1).

### Why 0.25.2

Pure test additions, no behaviour changes. Patch-level → `0.x.y`.

## [0.25.1] — 2026-05-23

### Fixed — "Set up permissions" now resolves the stale-TCC-entry case

The most common stuck state — *"the System-Settings switch is on, but Inspector Rust still asks for permission"* — wasn't handled by the v0.24.2 "Set up permissions" button, which only opened the System Settings pane. That case is a stale TCC entry: the stored code-requirement is from a previous binary (e.g. the pre-v0.23.2 ad-hoc signature) and doesn't match the current cert-signed binary, so `AXIsProcessTrusted` returns false even though the switch looks on.

The button now **always resets the TCC entry first** via `tccutil reset` (no admin password required) and re-fires the macOS permission prompt. Click *Allow → Open System Settings*, flip the switch once, and this time it sticks against the *current* signature. The same flow handles fresh installs (the reset is a no-op there). The card explainer is updated to say so.

### Added — Release artifacts for every supported OS/arch

`.github/workflows/release.yml` now ships a full set of bundles:

- **Windows x86_64** — `.exe` + `.msi` (unchanged).
- **Linux x86_64** — `.deb` **and `.AppImage`** (the bundle target list in `linux/src-tauri/tauri.conf.json` gains `appimage`; the workflow installs `libfuse2` and uploads the AppImage).
- **macOS Apple Silicon AND Intel** — matrix job (`macos-14` aarch64, `macos-13` x86_64). Each runner builds natively for its own arch (no cross-compile snags with the arch-specific `ort`/ONNX prebuilt binaries) and uploads the corresponding `InspectorRust_<ver>_<arch>.dmg`.

### Added — Unit tests for the new Linux CLI dispatcher

`core/rust-lib/src/cli_dispatch.rs::parse_args` (which routes `inspector-rust --toggle-popup` / `--ocr` / `--screenshot` / `--pick-color` to the running instance under GNOME/Wayland) gains 11 unit tests covering every alias, the help flag, unknown flags, multi-flag tie-breaking, and prefix-overlap guards.

### Why 0.25.1

A fix for a long-tail permission UX bug + release-workflow expansion + new test coverage. No breaking changes. Patch-level → `0.x.y`.

## [0.25.0] — 2026-05-23

### Added — Linux (Ubuntu / Debian) support

Inspector Rust now runs natively on Linux, merged from the community Linux port (PR #4). A new `linux/` bundle shell joins `win/` and `macos/` — the same thin 2-line `main.rs` calling `inspector_rust_core::run(...)`; all logic stays shared in `core/`.

- **Build** — `pnpm dev:linux` / `pnpm build:linux` → a `.deb` + AppImage. `scripts/install-linux.sh` provisions the apt deps, Node and Rust toolchain. Full prerequisites + a per-feature support matrix in [`linux/README.md`](./linux/README.md).
- **Region capture** (OCR + screenshot) — Wayland uses `grim` + `slurp`; X11 uses `scrot -s`. A missing tool produces a descriptive error naming the `apt` package.
- **OCR** — the `tesseract` CLI (`apt install tesseract-ocr` + language packs, e.g. `tesseract-ocr-eng` / `-deu`). Offline, no extra Rust dependencies.
- **GNOME / Wayland shortcuts** — Tauri's global shortcuts often don't receive key events under Wayland. The new `cli_dispatch` module exposes CLI flags (`--toggle-popup`, `--ocr`, `--screenshot`, `--pick-color`) routed to the running instance via `tauri-plugin-single-instance`; the Linux-only `desktop_shortcuts` module auto-registers GNOME/Cinnamon `gsettings` custom keybindings on first start.
- **Non-fatal shortcut registration** — a global-shortcut registration failure now logs a warning instead of aborting startup; the tray menu and CLI flags remain usable.
- System commands (kill / reboot / shutdown / lock) and the encryption keyring gained Linux backends. Data path on Linux: `~/.local/share/InspectorRust/history.db`.
- **Not yet on Linux** — the in-app eyedropper and the in-place AX text expander; the clipboard-paste expander fallback is used instead.
- A `.github/workflows/release.yml` job and the `inspector-rust.code-workspace` file round out the port.

### Why 0.25.0

A whole new supported operating system — backwards-compatible, no breaking changes. Feature-level → `0.x.0`.

## [0.24.2] — 2026-05-23

### Changed — consolidated macOS permissions card with one-click guided setup

The two separate amber permission banners (Accessibility, Screen Recording) are replaced by a single **macOS permissions** card with a **Set up permissions** button.

- **One-click chained setup** — clicking *Set up permissions* opens the first still-missing System Settings pane; the moment that grant flips on (the panel polls live), the card automatically opens the *next* missing pane. So one click walks you through both grants.
- Each permission has a live status row — an amber ring while missing, a green check + "Enabled" once granted — plus its own *Open* button.
- Troubleshooting (reset stale grants, re-check, quit) is tucked into one collapsible section instead of being duplicated across two banners.

**Note on automation:** there is no "grant everything with one password" — macOS deliberately does not let any app grant Accessibility or Screen Recording; the toggle must come from the user in System Settings, password or not. The button removes every other piece of friction (finding the panes, the right order, the stale-grant dance) but the final switch is, by Apple's design, yours to flip. Combined with the v0.23.2 stable-signing fix, you only ever do this once.

### Why 0.24.2

A UX rework of existing permission handling — no new IPC, no new capability, backwards-compatible. Patch-level → `0.x.y`.

## [0.24.1] — 2026-05-23

### Added — `rockthabox` wrap-around Snake variant

The `rockthebox` easter egg now has two modes, picked by the trigger spelling:

- **`rockthebox`** — *walls* mode (classic): hitting a wall ends the game.
- **`rockthabox`** — *wrap* mode: the snake reappears on the opposite edge instead of dying. Only a self-collision ends a wrap-mode game.

`lib/snake.ts::step` gained an optional `wrap` parameter (modulo the head back into the field instead of returning `dead`). `commands.ts` replaces `isRockTheBoxTrigger` with `rockTheBoxMode`, returning `"classic" | "wrap" | null`. The Snake HUD shows a `walls` / `wrap` mode chip. Pure-logic coverage extended (`snake.test.ts`).

### Why 0.24.1

A gameplay variant of the v0.24.0 easter egg — no new surface, backwards-compatible. Patch-level → `0.x.y`.

## [0.24.0] — 2026-05-23

### Added — `rockthebox` hidden Snake easter egg

A second hidden game, alongside `getshaky` (Pong). Typing **`rockthebox`** (or the variant **`rockthabox`**) into the popup search bar full-screen-takes-over the app shell with a game of Snake.

- **Gameplay** — steer with the arrow keys or **WASD**, eat the glowing food to grow, a wall or your own tail ends the run. The tick speed ramps up as your score climbs (capped so it stays playable). Score + a session-best are shown in the HUD; `Space` rematches, `Esc` quits.
- **Intro animation** — a ~1.9 s "box-assembling" flourish: the whole overlay rocks gently side-to-side while a glowing outline draws itself clockwise around the box, the grid dots sweep in on a diagonal wave, the snake's segments pop into place one by one (head first, with a back-ease bounce), and the food drops in with an expanding ring. The "ROCK THE BOX" title drops in with the letters spaced wide and snaps them tight.
- **Frame-rate independent** — the game advances on a fixed-timestep wall-clock accumulator, so it runs at the same real speed on 60/120/144 Hz displays (same lesson as the v0.23.1 Pong fix).
- Pure, unit-tested game maths in the new `core/frontend/src/lib/snake.ts` (`step`, `spawnFood`, `tickInterval`, collision rules — 24 tests); the stateful `<canvas>` loop is `components/SnakeGame.tsx`. Like `getshaky`, the trigger is **deliberately not** in the `COMMANDS` catalogue — it never surfaces in autocomplete; you have to know the word.
- Entirely client-side: no backend, no IPC, no new Rust module.

### Why 0.24.0

A whole new interactive surface (a second game mode), backwards-compatible. Feature-level → `0.x.0`.

## [0.23.2] — 2026-05-22

### Fixed — macOS permissions no longer need re-granting on every rebuild

`scripts/install-macos.sh` now signs every build with a **stable self-signed code-signing certificate** instead of leaving it ad-hoc-signed.

- **Root cause** — macOS TCC keys an Accessibility / Screen Recording grant to the app's code signature. An ad-hoc signature is keyed to the `cdhash` (binary hash), which changes on every rebuild → the grant was lost on every new version.
- **Fix** — the script creates (once, fully non-interactively) a self-signed certificate in a dedicated keychain `~/Library/Keychains/inspector-rust-signing.keychain-db` and signs with it. With a real certificate, TCC keys the grant to the app's *Designated Requirement* (`identifier "io.celox.inspector-rust" and certificate leaf = H"…"`) — which is **cdhash-free** and stable across rebuilds. Grant Accessibility + Screen Recording **once**; it now survives every future build.
- **One-time re-grant** — the first install after this change needs a single re-grant (the stale ad-hoc TCC entry won't match the new signature). The in-app Settings panel auto-detects the grant and offers the one-click relaunch as before.
- No admin password and no GUI prompt: the signing keychain has a hard-coded local password (it holds only a worthless self-signed key). If certificate creation fails for any reason, the script falls back to ad-hoc signing — it never hard-fails.
- The Settings panel's "Why does this keep happening on rebuild?" explainer is updated to reflect the new stable-signing behaviour.

### Why 0.23.2

Build-tooling fix for a long-standing macOS annoyance plus a docs-copy update — no runtime code change, no new IPC, backwards-compatible. Patch-level → `0.x.y`.

## [0.23.1] — 2026-05-22

### Fixed — `getshaky` Pong: frame-rate, serve delay, Shift boost, collision

Four fixes to the hidden Pong easter egg, all client-side (`lib/pong.ts` + `components/PongGame.tsx`):

- **Frame-rate independence** — the game ran "deutlich schneller" on a 144 Hz Windows display than on a 60 Hz MacBook because every frame advanced by a fixed step. The loop now scales all movement by `frameScale(dt)` — the wall-clock time since the previous frame, normalised to a 60 fps baseline — so the ball, both paddles and the Shift boost run at the same real-world speed on 60/120/144 Hz screens. A long stall (backgrounded tab) is clamped to 2.5× so the ball can't teleport.
- **1 s serve delay** — after a point the ball is parked at centre and the next serve fires `SERVE_DELAY_MS` (1000 ms) later, giving the player a beat to reposition.
- **Shift speeds up the paddle** — holding Shift while driving the paddle with the keys multiplies its travel speed by `SHIFT_SPEED_MULTIPLIER` (2×).
- **Swept paddle collision** — the per-frame point test is replaced by `paddleHit()`, a crossing test on the ball's leading edge: it registers a hit whenever the edge crossed the paddle face this frame, so a fast ball can no longer tunnel clean through a thin paddle.

New pure helpers `frameScale` / `paddleHit` + constants `REFERENCE_FRAME_MS` / `SHIFT_SPEED_MULTIPLIER` / `SERVE_DELAY_MS`, all vitest-covered (38 `pong.test.ts` tests).

### Why 0.23.1

Bug fixes to an existing feature, no new IPC, backwards-compatible. Patch-level → `0.x.y`.

## [0.23.0] — 2026-05-22

### Added — string-manipulation transforms on text entries

Select a **text** entry in the History list and the preview pane now shows a **Transform** toolbar — 11 string operations, each producing a new History entry + clipboard write (the original entry is untouched).

- **Transforms**: remove vowels, UPPERCASE, lowercase, Title Case, camelCase, snake_case, kebab-case, Base64 encode, URL encode (these nine are also keyboard-bound), plus Base64 decode and URL decode (click-only).
- **Keyboard**: `Cmd+1…9` on macOS / `Ctrl+1…9` on Windows trigger the first nine — the same `CmdOrCtrl` pattern as the existing `⌘B` / `⌘S` image actions. Plain digit keys can't be used (they'd type into the search bar); Shift+digit / Alt+digit type characters and Alt+1–3 collides with the text-expander hotkey, so `Cmd/Ctrl+digit` is the only conflict-free cross-platform choice.
- **Output**: each transform commits via the new `commit_transformed_text` IPC — clipboard self-write + a new Text History entry. Non-destructive; chain by selecting the new entry and transforming again.
- camel/snake/kebab share a tokeniser that breaks camelCase boundaries *and* splits on whitespace / `_` / `-`, so any of the three round-trips into any other. Base64 is Unicode-safe (`TextEncoder`/`TextDecoder`, not raw `btoa`). Decode transforms are total — invalid input is a no-op, never an error.
- Transform logic lives in the new pure, vitest-tested `core/frontend/src/lib/text-transform.ts` (24 tests); the `TransformBar` UI + `Cmd/Ctrl+1–9` handler are in `PreviewPanel.tsx`. Text entries only — image / files / html / rtf entries show no toolbar.

### Added — `mute` system command

The search-bar command palette gains **`mute`** — toggles the macOS system output mute (reads the current state via `osascript`, flips it). Like `lock` / `reboot` it surfaces in autocomplete. macOS-only; Windows returns "not implemented". IPC: `toggle_mute`.

### Why 0.23.0

A new interactive surface (the transform toolbar + `Cmd/Ctrl+digit` shortcuts), two new IPC commands, a new command-palette entry. Backwards-compatible. Feature-level → `0.x.0`.

## [0.22.0] — 2026-05-22

### Added — `Shift+↑` / `Shift+↓` adjust system volume

While the popup is open, **`Shift+ArrowUp`** raises and **`Shift+ArrowDown`** lowers the macOS output volume by 6 percentage points per press (≈ the 1/16 step macOS's own hardware volume keys use). Plain `↑`/`↓` still navigate the list — only the Shift modifier reroutes to volume.

- Backend: `system_commands::adjust_system_volume(delta)` reads the current level via `osascript`, applies the delta clamped to 0–100, sets it, and returns the new level. New IPC command `adjust_volume`. macOS-only — Windows returns "not implemented". The pure `clamp_volume` helper is unit-tested.
- Frontend: `useKeyboardNav` gained an `onShiftArrow` callback — `Shift+Arrow` invokes it (and skips list navigation) instead of moving the selection. App.tsx wires it to `adjustVolume(±6)`. Fire-and-forget; macOS plays its own volume feedback.
- No on-screen HUD — macOS's volume-change feedback sound is the confirmation, same as its hardware keys.

### Why 0.22.0

A new user-facing keybinding + a new IPC command. Compatible addition — plain arrow navigation is unchanged — but a new capability, so `0.x.0` per `docs/RELEASING.md`.

## [0.21.0] — 2026-05-22

### Added — `getshaky` 🏓 (hidden Pong easter egg)

Type **`getshaky`** into the search bar and the popup overlay shakes itself apart and reassembles as a game of Pong.

- **Hidden** — `getshaky` is *not* in the command catalogue, so it never appears in the autocomplete suggestions. It triggers only on an exact, fully-typed match (case-insensitive, whitespace-tolerant). You have to know the word.
- **The transformation** — a ~1.3 s flourish: the overlay jitters with an intensifying-then-settling shake (the "shaky" the command is named for), a big "GET SHAKY" title zooms in with an overshoot, then the play field + HUD fade in and the ball serves.
- **The game** — Pong against a bot, first to 5. Player paddle is driven by **mouse *and* arrow keys / W-S, both live at once**. The bot uses **ramp-up difficulty**: it starts fair and beatable (tracking-speed cap 4.5) and gains a little with every point it scores (cap → 7.5 at 4 points), so a deficit genuinely tightens. The ball speeds up slightly on every rally hit. Themed to the current Light/Dark palette — player paddle is the accent colour, board matches the app.
- **Esc is the only abort**, as specified. (After a match ends, Space offers a rematch — not an abort, so it doesn't break that rule.)
- Entirely client-side — a `<canvas>` + `requestAnimationFrame` loop. No backend, no IPC. Pure game maths (`clamp`, `botMaxSpeed` ramp-up, `paddleBounce` deflection, `serveBall`) lives in the new testable `lib/pong.ts`; the stateful loop + intro/over phases live in `components/PongGame.tsx`. `useKeyboardNav` gained an `enabled` flag so the popup's normal nav handler cleanly hands all keyboard control to the game.

### Why 0.21.0

A whole new (if playful) interactive surface — new module, new component, a search-bar trigger. No existing behaviour changed. Feature-level → 0.x.0.

## [0.20.2] — 2026-05-22

### Fixed — footer credit overflowing onto a second line

The footer is a fixed-height (`h-8`) single row: six keyboard hints on the left (`⏎ Paste`, `↑↓ Navigate`, `Esc Close`, `⌃⇧O OCR`, `⌃⇧S Shot`, `⌃⇧C Color`) and the credit + version + counter on the right. Six hints (OCR / Shot / Color were added incrementally over v0.9–v0.17) plus the verbose "made with ♥ by Martin Pfeffer" credit no longer fit the 600 px popup — the flex row wrapped, and the wrapped lines spilled out the bottom of the `h-8` strip.

Two-part fix:

- **Shortened the credit** — "made with ♥ by Martin Pfeffer" → "♥ Martin Pfeffer". The full wording is preserved in the hover `title` tooltip and the About dialog.
- **Widened the popup** 600 → 700 px. The list/preview split (40/60) and the cursor-monitor centring logic both scale automatically — no other change needed.
- Defensive: footer item groups are now `shrink-0` + `whitespace-nowrap`, so any future overflow clips cleanly at the edge instead of wrapping and breaking the row height.

### Why 0.20.2

Pure layout fix — a shorter string + a 100 px window-width bump + two CSS classes. Patch level.

## [0.20.1] — 2026-05-21

### Fixed — permission banners overlapping the Settings content (for real this time)

The two macOS TCC permission banners (Accessibility + Screen Recording) were `position: sticky`. The v0.16.2 attempt to fix their overlap gave them *staggered* `top` values so they'd stack instead of collide — but that just moved the bug: with both banners pinned at different heights, any section rendered between/below them (the new v0.20.0 **Appearance / Theme** section was the visible victim) got sandwiched and clipped between the two pinned bars.

Root cause: two **independently**-sticky elements in the same scroll container fundamentally don't coexist — there's no `top` arithmetic that makes scrolling content flow cleanly past *both*.

**Fix:** drop `sticky` from both banners entirely. They're now plain in-flow elements at the top of the Settings panel — the amber border + warning triangle keep them impossible to miss, and they scroll away like any other content when the user scrolls down. No pinning, no sandwich, no overlap.

### Fixed — stale `--color-text` in the permission banners

Two banner containers still used `text-[var(--color-text)]` — the CSS variable renamed to `--color-fg` in v0.20.0. The banner body text was resolving to an undefined variable. Corrected to `--color-fg`.

### Why 0.20.1

Two CSS/layout fixes in `SettingsPanel.tsx`, no API change. Patch level.

## [0.20.0] — 2026-05-21

### Added — Appearance theme control (Light / Dark / System)

Inspector Rust always *had* a dark theme — the `@theme` block in `styles.css` was the dark palette, and a `prefers-color-scheme: light` media query flipped to a light palette when the OS was in light mode. But that was invisible and un-overridable: the app simply followed macOS, with no way to force one or the other.

v0.20.0 makes the theme a first-class, user-controllable setting.

- **New "Appearance" section in Settings** — a three-way segmented control: **System** (follow the OS, the previous behaviour), **Light**, **Dark**. Light and Dark are hard overrides — they ignore the OS setting until you switch back to System. The choice persists in the `settings` table under `appearance.theme` and is re-applied on every launch.
- **Theme resolution** is now driven by a `data-theme` attribute on `<html>` (written by the new `lib/theme.ts`). `styles.css` carries explicit `:root[data-theme="light"]` / `:root[data-theme="dark"]` override blocks plus a system-scoped media query — so an explicit choice always wins, and "System" still tracks the OS live.
- **The dark palette was refined** — deeper near-black background (`#0c0d11`) with a faint cool undertone, the surface layer lifted enough to read as distinct, borders subtle but visible. Restrained, no neon. The light palette got a matching touch-up.

### Fixed — undefined `--color-fg` CSS variable

Components across the app referenced `var(--color-fg)` in hover states (`HistoryItem`, `AboutModal`, `SettingsPanel`, …), but `styles.css` only ever defined `--color-text`. `--color-fg` resolved to nothing, so those hover states silently fell back to inherited colour. Renamed the canonical variable to `--color-fg` (the name the component layer already standardised on) and defined it in every theme block — the hover states now work.

### Backend

- New IPC commands `get_theme_preference` / `set_theme_preference` (settings key `appearance.theme`), with a `normalise_theme` whitelist that collapses any unrecognised value to `"system"` so a hand-edited DB can't wedge the UI.

### Why 0.20.0

New Settings surface + two new IPC commands + a user-facing behaviour change (the app can now be themed independently of the OS). Compatible — a fresh install still defaults to `"system"`, i.e. the old behaviour. Feature-level → 0.x.0.

## [0.19.2] — 2026-05-21

### Added — Windows OCR + screenshot region parity, screenshot save-to-file mode

Merged via [#3](https://github.com/pepperonas/inspector-rust/pull/3). Brings the screen-region features — previously macOS-only — to Windows, and adds a save-to-file capture mode on both platforms.

- **Windows screen-region OCR** — `Ctrl+Shift+O` now works on Windows. Region selection uses a GDI fullscreen overlay; text recognition uses **WinRT `Windows.Media.Ocr`** + `Windows.Graphics.Imaging`. Picks up whatever OCR language packs are installed via *Settings → Time & Language → Language* — no bundled model, no extra install. COM is initialised per-thread on the capture worker; the WinRT futures are `.get()`-blocked to keep the pipeline synchronous like the macOS Vision path.
- **Windows screen-region screenshot** — `Ctrl+Shift+S` likewise works on Windows now (same GDI overlay, no OCR step).
- **Screenshot → save to file** — instead of writing the captured PNG to the clipboard, you can save it straight to disk via a native save dialog. On Windows the `S` key toggles the mode mid-overlay (the selection border turns green to confirm). On macOS — where `screencapture -i` is Apple's own process and can't have its keystrokes intercepted — a **double-tap of `Ctrl+Shift+S`** (second press within 400 ms of the first) flips the in-flight capture into save-to-file mode.
- **Docs** — README + README.de updated: Windows OCR/screenshot documented, the "macOS-only" limitation rows removed, a new note added about Windows OCR language packs. Region-picker module gained ~325 lines for the Windows path.

### Fixed — version manifests left at 0.19.1 by the merge

PR #3 bumped the README version badge to 0.19.2 but not the seven version manifests / `Cargo.lock` / the CHANGELOG. This release commit reconciles them — `Cargo.toml`, the four `package.json`s, both `tauri.conf.json`s, the three `Cargo.lock` workspace entries, and this CHANGELOG are now all 0.19.2.

## [0.19.1] — 2026-05-20

### Fixed — Color Picker on multi-screen setups (loupe appeared on main display instead of cursor display)

The `NSColorSampler` loupe always appeared on the **main display**, regardless of which monitor the user's cursor was actually on. Symptom: trigger `Ctrl+Shift+C` (or the in-modal Color Picker → "Pick from screen" button) with your cursor on a secondary monitor, and the magnifier appeared on the primary one — invisible to you until you moved the cursor over.

Root cause: macOS positions `NSColorSampler` on the calling app's **primary screen**. The "primary screen" is decided by where the app's most-recently-active window was. Inspector Rust's popup was hidden *before* the sampler was launched, and the popup's last known position (= whichever screen the user opened it on) was sometimes a different display than the cursor's. The `setActivationPolicy: Regular` + `activateIgnoringOtherApps:` pair that's needed to make `NSColorSampler` render its loupe then anchored the app to that stale screen.

**Fix:** before hiding the popup for either the eyedropper-pipeline (`Ctrl+Shift+C`) or the modal-flow Pick-from-screen button, park the popup at the centre of the cursor's monitor via the new `hotkey::park_on_cursor_monitor` helper (reuses the existing `pick_cursor_monitor` lookup that the popup-show path already uses). The hidden popup's "last seen" screen is then the right one, the activation snaps to the cursor's display, and the loupe renders where the user expects it.

- One-liner in two call-sites (`commands::run_eyedropper_pipeline` + `commands::pick_screen_color`); no behaviour change for single-screen users.
- No new dependencies. Cost: a single `set_position` call before each pick (~µs).

### Changed — fresh launcher icon set

App icons regenerated via `tauri icon` from `docs/inspector-rust.png` (the detective-themed hero artwork — same image used at the top of the README). Affects every bundled icon size: macOS `.icns`, Windows `.ico`, all `Square*Logo.png` Microsoft Store tile sizes, plus the platform PNG ladder (32×32 → 1024×1024).

- macOS Dock + Spotlight + Cmd-Tab → new icon.
- Windows Start menu + taskbar → new icon.
- New install ⇒ new icon. Existing macOS installs may need a Dock relaunch (`killall Dock`) to refresh the cached icon.

### Why 0.19.1

Two patch-level changes: a one-line multi-screen UX fix + an asset refresh (no code semantics changed by the icon swap). 0.x.y bump per `docs/RELEASING.md`.

## [0.19.0] — 2026-05-20

### Added — system-level power commands (kill / reboot / shutdown / lock)

Four new commands extend the v0.18.0 search-bar palette into a
proper power-user system control surface. Destructive commands
guard against accidents with native `window.confirm` dialogs;
locking the screen runs unconfirmed because it's cheap to undo.

**`kill [-9] [pattern]` — live process picker** *(macOS / Linux)*

Type `kill` alone → full process list (sorted by memory desc).
Type `kill slack` → filtered to processes whose name or exe path
contains "slack" (case-insensitive). Press Enter on a row → confirm
dialog showing PID + name + signal → SIGTERM is sent.

Add `-9` for SIGKILL: `kill -9 slack` filters the same way but
arms the row for force-quit instead of graceful shutdown. After a
successful kill the picker stays open and removes the killed PID
from the snapshot, so you can chain kills without re-typing.

- Backend: new `sysinfo`-crate-based `system_commands::list_running_processes` + `kill_process_by_pid(pid, force)`. List excludes the Inspector Rust process itself. ~10 ms for a full refresh on a typical desktop with 200+ processes.
- Frontend: new `ListEntry` kind `kill-target`; App.tsx detects kill-mode and overrides the whole list (history is hidden in kill mode — no point conflating clipboard rows with destructive process rows). New picker preview card in `PreviewPanel` with PID / memory / signal / executable path.

**`reboot` / `shutdown`** *(macOS only)*

Both shell out to `osascript` driving `loginwindow` via the legacy
Apple Events `aevtrrst` / `aevtrsdn`. No sudo required; macOS
handles its own "These apps have unsaved changes" dialog after
ours. Inspector Rust shows a native `window.confirm` first so a
typo-then-Enter doesn't reboot your machine.

**`lock`** *(macOS only)*

Shells out to `pmset displaysleepnow`. Instant, no confirmation —
the lock screen requires your password to dismiss, so the cost of
an accidental lock is just one password entry. No privilege needed.

### Why 0.19.0

Four new IPC commands + one new `ListEntry` kind + one new Rust
module + one new Cargo dep (`sysinfo`, ~150 KB). Backwards-compatible —
non-system queries route as before. New feature-level surface → 0.x.0.

### Windows

System commands are macOS-only in this release. Windows attempts return
`"not implemented on this platform"` and the frontend surfaces it as a
toast. Follow-up planned: `ExitWindowsEx` for reboot/shutdown,
`LockWorkStation` for lock, `OpenProcess` + `TerminateProcess` for kill.

## [0.18.0] — 2026-05-20

### Added — power-command palette in the search bar (six commands + autocomplete)

The search bar gains a shell-style command palette. Type a known
keyword + argument and Enter runs it; type a partial keyword and the
matching commands surface as autocomplete `hint` rows underneath.
Tab-completion not strictly needed — the suggestion row is itself
selectable, and activating it populates the search bar with the full
keyword prefix so you can just type the argument.

**Translation (open Google Translate in browser):**

- **`tren <text>`** — English → German.
- **`trde <text>`** — German → English.
- **`tr <text>`** — auto-detect → German.

Frontend constructs the canonical `https://translate.google.com/?sl=…&tl=…&text=…&op=translate` URL and opens it via `tauri-plugin-opener`'s external-URL handler. No translation runs locally; no network call from the app itself.

**Image ops (clipboard image in / out):**

- **`rz <W>x<H>`** — resize the clipboard image to the given dimensions via Lanczos3 sampling (best-quality downscaling), write the result back to the clipboard, push a fresh History entry. 16 MP target cap, `image` crate (already a workspace dep — no new system requirement).
- **`optim`** — read clipboard PNG, run through `oxipng` (lossless, zopfli + filter selection), save to `~/Downloads/inspector-rust-optim-<ts>.png`. Does *not* touch the clipboard. Returns before/after byte counts so the UI can confirm.

**Text:**

- **`rmvvls <text>`** — strip vowels (`aeiou` + uppercase + German umlauts `ä/ö/ü/Ä/Ö/Ü`) from text → clipboard + History entry. `rmvvls hello` → `hll`.

**Architecture:**

- New `image_ops.rs` Rust module (resize + optim pipelines, shared by IPC).
- Three new IPC commands: `resize_clipboard_image(W, H)`, `optimize_clipboard_image()`, `remove_vowels_to_clipboard(text)`.
- New workspace dep: `oxipng = "9"` (pure Rust, zero-config, statically linked, ~200 KB binary cost).
- New frontend `lib/commands.ts` with parser + autocomplete logic + `translateUrl` URL-builder.
- `ListEntry` discriminated union extended with `command` (runnable) and `command-suggestion` (autocomplete) kinds. Both render via existing `HistoryItem` + `PreviewPanel` paths.

**Tests** — 13 new Rust unit tests (`strip_vowels` + `image_ops` parse/serde) + 38 new frontend tests (`commands.test.ts` for parser/suggestions/URL builder/parseResizeArg).

### Why 0.18.0

Six new user-visible commands + new IPC surface + new frontend lib + new optional Cargo dep = clearly a feature release per `docs/RELEASING.md`'s 0.x.0 rule. Backwards-compatible — existing search behaviour unchanged when the input doesn't match a command keyword.

## [0.17.0] — 2026-05-20

### Added — `Ctrl+Shift+C` global eyedropper

- **New `Ctrl+Shift+C` global shortcut** fires the screen color picker directly from anywhere on the system. Cursor turns into the NSColorSampler loupe (macOS) / GDI overlay (Windows); one click on a pixel and the hex string (`#RRGGBB`) lands on the system clipboard **and** as a Text History entry. Parallel UX to the v0.15.0 `Ctrl+Shift+S` screenshot shortcut — fire-and-forget, no popup, no modal. The existing **Color Picker** button in the History tab still opens the HSV modal as before; this is the no-modal, just-give-me-the-hex path. — *#feat(color)*
- **Tray menu entry** "Pick Color (⌃⇧C)" / "Pick Color (Ctrl+Shift+C)" next to *Screenshot Region*. Same threading model as OCR + screenshot: dispatched to a worker thread.
- **Footer hint** gains `⌃⇧C Color` next to `⌃⇧O OCR` + `⌃⇧S Shot`.
- **Settings → Keyboard shortcuts** cheat sheet gains a row for the eyedropper alongside the OCR + screenshot rows.
- **Backend** (`commands.rs`): `run_eyedropper_pipeline(app)` reuses `screen_picker::pick_color_async` / `pick_color_blocking` but writes the hex to the clipboard via `ClipboardContext::set_text` + persists as a Text history entry instead of emitting `color-picked` for the modal. New `eyedropper_to_clipboard` IPC command (parallel to `screenshot_region`). New private helper `clear_eyedropper_no_popup` mirrors `clear_pick_suppress_hide` but doesn't re-show the popup window — appropriate for the global-hotkey flow.
- **Hotkey registration** (`hotkey.rs`): fourth global shortcut. `register_direct_slots` collision check now rejects `Ctrl+Shift+C` alongside popup / OCR / screenshot / expander.
- **No Screen Recording TCC grant needed** — NSColorSampler reads pixels via Quartz / GDI overlay reads via `GetPixel`, neither goes through `screencapture`.

### Why 0.17.0

New global shortcut + new IPC command + new tray entry + new event-emitting handler = feature-level addition per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule. Backwards-compatible — no existing functionality changed.

## [0.16.2] — 2026-05-20

### Fixed — overlapping permission banners in Settings tab

- **Both TCC permission banners (Accessibility + Screen Recording) had `position: sticky` with the same `top` value.** When one banner was expanded and the user scrolled, the other banner's *header* would stick on top of the first banner's *body* — the "Quit Inspector Rust / Force re-grant / Try system prompt" button block of the Accessibility banner would visually appear *below* the Screen Recording banner header, even though they belong to the Accessibility section. — *#fix(ui)*
- **Fix:** drop sticky positioning when a banner is expanded (the user is reading it, no need to pin it); stagger the `top` values when both banners are simultaneously collapsed-and-sticky so they stack instead of overlap.

### Why 0.16.2

Pure CSS / layout fix in `SettingsPanel.tsx`. No API change. Patch level.

## [0.16.1] — 2026-05-19

### Fixed — backup-export default filename regression from the v0.16.0 rebrand

- **Settings → Backup & restore → Export** proposed `inspector-rust-backup-.json` (no timestamp) instead of `inspector-rust-backup-<iso>.json`. The v0.16.0 brand rename ran a perl substitution that interpreted the JS template-literal `${stamp}` as a Perl variable lookup and silently dropped it. Caught during the v0.16.0 doc audit while sweeping for other rename damage; the file in question (`SettingsPanel.tsx`) is opaque to plain `grep` on this machine, which is why this and a dozen "ClipSnap" mentions slipped through the original rebrand. Now correctly proposes `inspector-rust-backup-2026-05-19T22-30-15.json` etc. — *#fix(backup)*

### Why 0.16.1

A one-line code fix to a user-visible default filename. Pure patch.

## [0.16.0] — 2026-05-19

### Changed — full rebrand: ClipSnap → Inspector Rust

This is a hard rebrand. Every user-visible "ClipSnap" string is now "Inspector Rust"; every technical identifier (Cargo package names, npm package names, bundle ID, app bundle, install paths) flipped to `inspector-rust` / `InspectorRust`. GitHub repo renamed from `pepperonas/clipsnap` to `pepperonas/inspector-rust`. **This is a breaking change at the install level** — see migration notes below.

- **Display name** (window title, tray tooltip, About modal, README, all docs): `ClipSnap` → `Inspector Rust` (two words, capitalised).
- **Bundle identifier**: `io.celox.clipsnap` → `io.celox.inspector-rust`. Triggers fresh macOS TCC grants on first launch (Accessibility, Screen Recording, PostEvent — all bound to bundle id + cdhash).
- **macOS app bundle**: `/Applications/ClipSnap.app` → `/Applications/InspectorRust.app`. **The old .app stays on disk** — uninstall it manually if you want a clean Spotlight / Launchpad. The new bundle name is CamelCase (no space) so terminal paths stay quote-free; the window title and tray label still render the spaced "Inspector Rust".
- **macOS LaunchAgent**: `~/Library/LaunchAgents/ClipSnap.plist` → `~/Library/LaunchAgents/InspectorRust.plist`. Old plist left in place — delete it manually or toggle autostart off in Inspector Rust before quitting the old build.
- **Data directory**: `~/Library/Application Support/ClipSnap/` → `.../InspectorRust/` (macOS); `%APPDATA%\ClipSnap\` → `%APPDATA%\InspectorRust\` (Windows). **Fresh start by design** — no auto-migration. To carry over snippets / notes / history, open the *old* ClipSnap one last time, Settings → Backup → Export, then import the JSON into Inspector Rust.
- **Keychain entry**: service `io.celox.clipsnap` → `io.celox.inspector-rust`. The old AES-256-GCM master key stays in Keychain (the migration plan above re-encrypts with the new key on import, so no plaintext leak).
- **Cargo packages**: `clipsnap-core` → `inspector-rust-core`, `clipsnap-win` → `inspector-rust-win`, `clipsnap-macos` → `inspector-rust-macos`. Lib code identifier `clipsnap_core` → `inspector_rust_core` (Rust auto-converts the hyphen).
- **Binary name**: `clipsnap` → `inspector-rust` (`win/src-tauri/Cargo.toml`'s `[[bin]] name`).
- **npm packages**: `clipsnap` → `inspector-rust`, `clipsnap-frontend` → `inspector-rust-frontend`, `clipsnap-{win,macos}` → `inspector-rust-{win,macos}`. The `pnpm dev:macos` / `pnpm build:win` aliases at the workspace root still work — they were already platform-named, not brand-named.
- **Release-artifact filenames**: `ClipSnap_<ver>_x64_en-US.msi` → `InspectorRust_<ver>_x64_en-US.msi`; `ClipSnap_<ver>_aarch64.dmg` → `InspectorRust_<ver>_aarch64.dmg`; the `clipsnap.exe` Windows standalone → `inspector-rust.exe`.
- **Output file prefixes**: `~/Downloads/clipsnap-image-<ts>.png` / `clipsnap-cutout-<ts>.png` → `inspector-rust-image-<ts>.png` / `inspector-rust-cutout-<ts>.png` (cutout-ML feature).
- **GitHub remote**: `https://github.com/pepperonas/clipsnap` → `https://github.com/pepperonas/inspector-rust`. GitHub auto-redirects the old URL for clones / git fetches, but please update your remotes (`git remote set-url origin https://github.com/pepperonas/inspector-rust.git`).
- **Win32 window class** (eyedropper overlay): `ClipSnapEyeDropper` → `InspectorRustEyeDropper`.

### Why 0.16.0

The rebrand changes the bundle identifier, the app bundle name, the data directory, and the binary name — anyone with the v0.15.x build installed will end up with both apps on disk after the upgrade. That's the upper bound of "breaking change" for a desktop app — `0.x.0` per `docs/RELEASING.md`'s SemVer policy.

### Migration notes

| You had                                           | After upgrade                                       | What to do                                              |
|---------------------------------------------------|-----------------------------------------------------|----------------------------------------------------------|
| `/Applications/ClipSnap.app`                      | `/Applications/InspectorRust.app` (new) + old one  | Manually drag the old `ClipSnap.app` to Trash            |
| TCC grants for `io.celox.clipsnap`                | Stale entries in System Settings → Privacy & Security | Manually remove them (or `tccutil reset ...`)             |
| Autostart entry (`~/Library/LaunchAgents/ClipSnap.plist`) | Old plist still firing on next reboot              | Delete it manually, or toggle autostart off in *old* ClipSnap, *then* delete the old app |
| Encrypted history at `~/Library/Application Support/ClipSnap/history.db` | Untouched on disk; unreachable from Inspector Rust | Open old ClipSnap → Backup → Export → import into Inspector Rust |

## [0.15.0] — 2026-05-19

### Added — dedicated screenshot region capture (no OCR required)

- **New `Ctrl+Shift+S` global shortcut** (literal Control on every OS, same convention as `Ctrl+Shift+O` / `Ctrl+Shift+V`): drag a marquee over any region → PNG lands on the system clipboard *and* in History. Same `screencapture -i` UX as `Cmd+Shift+4`, same Screen Recording (TCC ScreenCapture) gate as OCR, but **no OCR step** — works on regions that contain no recognisable text (a chart, a button, a UI mockup, a photo). The OCR shortcut still works as before; the screenshot shortcut is a strict superset of "what OCR couldn't preserve". — *#feat(screenshot)*
- **Tray menu entry** "Screenshot Region (⌃⇧S)" next to "OCR Region (⌃⇧S)". Same threading model — dispatched to a worker thread because `screencapture -i` blocks until the user finishes the marquee.
- **Footer hint** shows `⌃⇧S Shot` next to `⌃⇧O OCR` so the shortcut is discoverable every time the popup opens.
- **Settings → Keyboard shortcuts** cheat sheet gains a row for the screenshot shortcut alongside the OCR one.
- **Backend** (`core/rust-lib/src/commands.rs`): new `ScreenshotResult { cancelled, bytes }` type, `run_screenshot_pipeline(app)` function (parallel to `run_ocr_pipeline`), and `screenshot_region` IPC command. Shares `region_picker::capture` with OCR. Image is written to clipboard via `ClipboardContext::set_image` and persisted to history as a `[screenshot · N B]` entry. `mark_self_write(Image, b64)` arms the watcher so the round-trip doesn't double-record.
- **Hotkey registration** (`hotkey.rs`): added third global shortcut. `register_direct_slots` collision check now rejects `Ctrl+Shift+S` alongside the popup/OCR/expander hotkeys.

### Fixed — tray label for OCR shortcut

- macOS tray label said `OCR Region (⌘⇧O)` since the v0.14.1 hotkey change — the Cmd glyph should have been Control (`⌃⇧O`). Caught during the screenshot work; fixed in the same release.

### Why 0.15.0

New global shortcut + new IPC command + new event-emitting tray path = feature-level addition per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule. Backwards-compatible — no existing functionality changed.

## [0.14.2] — 2026-05-19

### Fixed — OCR history ordering: text on top, image below

- **OCR pipeline persists the source PNG *first*, then the recognised text.** Both rows get a `last_used_at` of `now()` at insert time, so the second insert wins the "most recent" slot. The popup sorts history `last_used_at DESC` — previously the *image* was on top (because text was inserted first), which is confusing because the *text* is the OCR result the user actually wanted: opening the popup post-OCR and pressing Enter pasted the screenshot instead of the recognised string. Now the text entry is on top and matches what's on the system clipboard. — *#fix(ocr)*
- No behaviour change for the clipboard write itself — `ctx.set_text` still runs once, before either history insert, with `mark_self_write(Text, ...)` so the watcher doesn't double-capture.

### Why 0.14.2

Pure ordering fix in `commands::run_ocr_pipeline`. No API surface change, no version-bump rationale beyond "patch level for a user-visible UX bug".

## [0.14.1] — 2026-05-19

### Changed — OCR hotkey is now literal `Ctrl+Shift+O` on every OS

- **macOS OCR shortcut moved from `⌘⇧O` to `⌃⇧O`** (literal Control, not Cmd). `Cmd+Shift+O` collides with **Go to Symbol** in VS Code, IntelliJ, WebStorm, and a host of other IDEs — pressing it inside an editor opened the IDE picker instead of triggering OCR. The Windows binding (`Ctrl+Shift+O`) was already correct; this just brings macOS in line. Same key combo, same physical position, no platform branching. — *#fix(macos)*
- **Hotkey registration** (`core/rust-lib/src/hotkey.rs`): both `register` and `register_direct_slots` now build the OCR `Shortcut` with `Modifiers::CONTROL | Modifiers::SHIFT` unconditionally — the `#[cfg(target_os = "macos")]` SUPER branch is gone. Direct-slot collision detection also tracks the new combo, so a slot can't shadow OCR.
- **Frontend display** (`core/frontend/src/components/Footer.tsx` + `SettingsPanel.tsx`): footer hint, Screen Recording explanation, direct-slot help text, and the Keyboard-shortcuts cheat sheet now render `⌃⇧O` on macOS (instead of `⌘⇧O`).
- **Docs** updated across `README.md`, `CLAUDE.md`, `macos/README.md`, and `docs/text-expander.md`. The Windows `Ctrl+Shift+O` references stayed correct.
- **Existing user impact** — pure muscle-memory change; the previous binding (`⌘⇧O` on mac) simply stops working after upgrade. Users who'd granted Screen Recording to Inspector Rust don't need to re-grant.

### Why 0.14.1

A targeted hotkey fix with no public-surface additions — pure patch.

## [0.14.0] — 2026-05-16

### Added — autostart UI: state-visible tray + Settings toggle

- **Tray menu's "Start at Login" / "Start with Windows" item is now a checkable menu item** that visibly reflects the current state (`☑` / ` `) and probes `~/Library/LaunchAgents/InspectorRust.plist` (macOS) / the run-key (Windows) on every tray build, so the checkmark stays right even if the autostart was enabled/disabled outside the app. Toggling updates the check in place and emits the new `autostart-changed` event so other UI surfaces stay in sync. — *#feat(tray)*
- **New "Startup" section in Settings** with a clearly-labelled "Start at login" (macOS) / "Start with Windows" toggle that explains where the entry lives — much more discoverable than the tray menu for users who don't routinely browse it. Listens for `autostart-changed` so toggling from the tray reflects immediately. — *#feat(ui)*
- **Two new IPC commands** `get_autostart_enabled` / `set_autostart_enabled` wrapping `tauri-plugin-autostart`'s `AutoLaunchManager`. Both read back the *now-effective* state from the OS rather than echoing the requested value, so the UI reconciles against actual filesystem / registry state if a toggle partially fails.
- The `tauri-plugin-autostart` default of `MacosLauncher::LaunchAgent` was already correct — no plugin-config change. Removed two dead-code lines (`let _ = autostart;` in setup; `let _ = MacosLauncher::LaunchAgent;` at the end of `build_tray`).

### Why 0.14.0

Adds a new event surface (`autostart-changed`), two new IPC commands, a new Settings section, and a tray menu item type change (`MenuItem` → `CheckMenuItem`). Compatible additions but a meaningful UX feature — new-feature bump per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule.

## [0.13.0] — 2026-05-13

### Added — direct hotkey → snippet slots (a paste-only expansion mode that works *everywhere*, including terminals)

- **New "Direct hotkey → snippet" section** in Settings → Text expander. Bind a hotkey straight to a snippet — e.g. `Alt+2` → the `aiplan` body — and pressing it pastes the body at the cursor, **no abbreviation typed**. Because it reads nothing from the focused field (it just writes the body to the clipboard and synthesizes `Cmd/Ctrl+V`, then restores the clipboard), it works in **any** app — including terminals (iTerm2, Terminal.app, kitty, Alacritty, …) where the abbreviation-based expander can't see the input line. — *#feat(expander)*
- **Backend**: `expander::DirectSlot { hotkey, snippet_id }` persisted as a JSON array under the `expander.direct_slots` settings key; `expander::paste_snippet_body` (AX-gated on macOS, same as the abbreviation expander); `hotkey::register_direct_slots` validates against collisions with the popup hotkey (`Ctrl+Shift+V`), the OCR hotkey, the abbreviation expander hotkey, and other slots, then registers each as a global shortcut whose handler dispatches to the main thread. Two new IPC commands `get_direct_slots` / `set_direct_slots`; `ExpanderShortcutState` grew a `direct` field; slots are re-registered from settings at startup. `snippets::get_by_id` added.
- **UI**: per-slot rows of `[hotkey recorder] → [snippet picker] [remove]`, an "Add slot" button, and a Save (which registers + persists; nothing is written if registration fails, so the previous slots stay live on error). A deleted bound snippet shows as `⚠ snippet deleted — pick another` so the slot can be rebound or removed. Missing-Accessibility warning mirrors the abbreviation expander's.
- **Why this mode exists:** the abbreviation expander ("type `aiplan`, press the hotkey") fundamentally can't work in a terminal — terminals don't expose the readline input buffer through accessibility, and a shell prompt has no GUI "select the word I just typed". Direct slots sidestep that by not needing to read anything.

### Why 0.13.0

New feature (a second expansion mode + its UI + storage + a new event-free IPC pair) with no breaking changes. New-feature bump per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule.

## [0.12.0] — 2026-05-12

### Fixed — text expander: hotkey now actually fires, failures are no longer silent

- **New default hotkey: `Alt+1`** (the `1`-row digit, not the numpad). The pre-0.12 default `Alt+Backquote` was *unreachable* on German ISO MacBooks — the physical `^`/`°` key under Esc reports as `IntlBackslash` (and on some layouts a different Carbon keycode), so the registered shortcut never matched the key the user pressed and the expander looked dead. Digit-row keys have a fixed `KeyboardEvent.code` on every layout, aren't dead keys anywhere, and aren't reserved by macOS or Windows. A one-time settings migration ([`expander::migrate_legacy_default`](./core/rust-lib/src/expander.rs)) bumps an un-customised `Alt+Backquote` install to `Alt+1`; a migration flag means it won't clobber a value the user deliberately re-picks afterwards. — *#fix(expander)*
- **Accessibility-missing no longer fails silently.** Previously, if macOS Accessibility wasn't granted, pressing the expander hotkey ran the whole capture/paste cycle — but `enigo`'s synthetic keystrokes silently no-op without the grant, so *nothing happened* and the user had no clue why. Now `expand_at_cursor` returns the `ax.permission_denied` sentinel instead of attempting a doomed clipboard roundtrip on macOS, and the hotkey handler pre-checks `AXIsProcessTrusted()` before dispatching — on a miss it pops the popup, switches to the Settings tab, and emits `expander-permission-needed` so the frontend shows an actionable amber banner ("Force re-grant → Restart now"). Mirrors the existing OCR `screen.permission_denied` pattern. — *#fix(macos)*
- **`diagnose_at_cursor` reports the real reason** when Accessibility is missing instead of an empty capture ("Accessibility permission isn't granted — … Grant it in the section above, then relaunch.").
- **Settings → Text expander: one-click presets** `Alt+1` / `Alt+2` / `Alt+3` next to the hotkey-capture button, so the common case doesn't require fighting the recorder widget. The capture widget still accepts any combination; help text now nudges toward digit keys for layout stability. Stored hotkey codes (`Alt+Digit1`) render in the friendly form (`Alt+1`) in tooltips, status text, and the keyboard cheat sheet.
- **Settle delay** (40 ms) at the start of the expand cycle so a physically-still-held `Alt` (from the hotkey itself) is released before `enigo` synthesizes its own modifier chords — avoids a stuck-modifier state in the source app. Invisible: the popup is hidden the whole time.
- **Expansion now works in Electron / Chromium / Mac-Catalyst text fields** (WhatsApp Desktop, Slack, Discord, VS Code, …). Those expose `AXValue` read-only: the old code set `AXSelectedTextRange` (which *selects* the abbreviation) then `AXSelectedText` (which returns success but does nothing), so the abbreviation just sat there highlighted, never replaced. The AX replace now **verifies** by re-reading `AXValue`; on a no-op it reports a new `ReplaceOutcome::SelectionActive` and `expander.rs` pastes the snippet body over the live selection (no re-select — `Cmd+Shift+←` would only swallow the previous word). Native Cocoa apps still get the clean in-place `AXSelectedText` replace with no clipboard touch. — *#fix(macos)*
- **Known limitation, now documented loudly:** the hotkey expander **cannot** work on a terminal command line (Terminal.app, iTerm2, kitty, Alacritty, WezTerm, …). Terminals don't expose the input line via AX, and there's no GUI-style "select previous word" shortcut on a shell prompt — pressing the hotkey there does nothing. Use the popup (`Ctrl+Shift+V` → search the abbreviation → Enter) for terminals.
- Windows is unaffected-positive: `Alt+1` registers cleanly there, `SendInput` needs no permission, and the UIA Backspace+type / clipboard-fallback paths are unchanged (the new `ReplaceOutcome` enum maps to `Replaced` / `Unsupported` there).

### Changed — bundled AI prompts: no more `[REQUIREMENT]` fill-in slots

- **All 25 `ai*` prompt snippets reworked** ([`core/rust-lib/src/seed/ai_prompts.json`](./core/rust-lib/src/seed/ai_prompts.json)) to drop the `[REQUIREMENT]` / `[CODE]` / `[CHANGE]` / `[SYSTEM]` / `[DOMAIN]` … input placeholders. The prompts are now the **structured-instruction half only** — designed to be appended to (or pasted alongside) your own prompt / code / context, so the subject comes from the surrounding text rather than a fill-in slot. Openers changed accordingly (`"…for: [REQUIREMENT]"` → `"…for the requirement at hand"`; `"the following code"` → `"the code at hand"`); choice-placeholders (`[PostgreSQL / SQLite / …]`, `[vitest / pytest / …]`, downtime budget, …) became `"as specified, or ask / default to X"` instead of literal brackets; the `## …` output structure is unchanged. — *#chore(snippets)*
- **Seed flag not bumped** (`seed.default_snippets_v1` stays). New installs get the new prompts automatically; existing installs keep their current `ai*` snippets until they click **Restore defaults** in the Snippets sidebar — deliberate, since a forced re-seed would clobber customised prompts and resurrect deleted ones.

### Why 0.12.0

Changes the default hotkey (a user-visible behaviour change with a settings migration), adds a new event surface (`expander-permission-needed`) and new public error sentinel, plus the presets UI. Beyond a 0.11.x patch — minor bump per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule.

## [0.11.0] — 2026-05-10

### Fixed — OCR no longer fails silently when Screen Recording is denied

- **Root cause.** macOS treats Accessibility and Screen Recording as **independent** TCC grants. Before this release, OCR pre-checks only knew about Accessibility — when the user had granted Accessibility (so paste worked) but never Screen Recording, pressing `⌘⇧O` would call `screencapture -i`, macOS would deny the spawn, the process would exit cleanly with an empty file, and the user saw … nothing. No marquee, no error, no clue. — *#fix(macos)*
- **New permission API** in [`core/rust-lib/src/screen_recording.rs`](./core/rust-lib/src/screen_recording.rs): `screen_recording_granted()` (`CGPreflightScreenCaptureAccess`), `request_screen_recording_grant()` (fires the macOS prompt), `open_screen_recording_settings()` (jumps straight to the right Privacy pane). Wired through four IPC commands plus a `tccutil reset ScreenCapture io.celox.inspector-rust` recovery path for stale grants.
- **`run_ocr_pipeline` pre-checks the grant** and returns the new `screen.permission_denied` sentinel when missing — same pattern as the existing `ax.permission_denied` for paste.
- **Hotkey handler surfaces the failure**: when `⌘⇧O` returns the sentinel, Inspector Rust now opens its popup and emits `ocr-permission-needed` so the frontend switches to the Settings tab and shows a clear amber banner pointing at the right System Settings pane. No more silent fail.
- **Settings panel** gets a second collapsible permission banner (parallel to the Accessibility one): one-line warning with `Open System Settings` button + chevron toggle for the full walkthrough (Quit · Force re-grant · Try system prompt · Re-check). Polls every second while not granted, like Accessibility, so the badge flips green within ~1 s of toggling in System Settings.
- **App-level toast banner** for the OCR-permission-needed event in `App.tsx`, mirroring the existing paste-failed banner. Auto-dismisses after 15 s (longer than the 8 s paste banner — the user needs more time to read + click into System Settings).

### Why 0.11.0

The change adds a whole new TCC permission grant the app depends on, plus four new IPC commands, a new Rust module, and a new event surface. That's beyond the bug-fix scope of a 0.10.x patch — minor bump per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule.

## [0.10.7] — 2026-05-10

### Added — Shortcut discovery

- **Footer now surfaces the OCR shortcut** (`⌘⇧O` on macOS, `Ctrl+⇧+O` elsewhere) next to the existing Paste / Navigate / Close hints. OCR was previously discoverable only via the tray menu, which most users rarely open. — *#feat(ui)*
- **New "Keyboard shortcuts" section in Settings** with a three-group cheat sheet: Global (Ctrl+Shift+V open popup, ⌘⇧O OCR, ⌥+` text expander), Popup list (Enter / Shift+Enter / arrows / Esc), and Image entry actions (⌘B cutout, ⌘S save). Modifier glyphs adapt to the running OS via the new `IS_MAC` helper in `core/frontend/src/lib/platform.ts`. — *#feat(ui)*
- The platform helper also exposes a `shortcut(...keys)` formatter so any future shortcut-rendering site can stay consistent without re-detecting macOS each time.

## [0.10.6] — 2026-05-09

### Changed — Accessibility banner is now collapsible

- **The Settings tab's Accessibility-required banner collapses to a single warning row by default.** When the macOS Accessibility permission is missing, the user sees a sticky amber-bordered bar with `⚠ Accessibility access required (macOS)` + the primary `Open System Settings` button + a chevron toggle. The full step-by-step walkthrough, the cdhash explanation, and the secondary buttons (Quit Inspector Rust / Force re-grant / Try system prompt / Re-check) only appear when the chevron is expanded. — *#chore(ui)*
- **Granted state is fully hidden** — when Accessibility is OK, no banner renders at all (previously the whole block was always present, which made the settings page feel cluttered for users who'd already granted). The `Restart now` prompt for the just-granted edge case still surfaces inside the Text-expander section as before.
- The collapsed bar stays prominent (amber border + warning icon + primary action button visible at all times), so the problem state is impossible to miss while occupying just one row of vertical real estate. — *#fix(ui)*

## [0.10.5] — 2026-05-09

### Fixed — Modals overflowing the popup window

- **About dialog** is now bounded to `max-h-[calc(100vh-2rem)]` and uses a three-row layout (sticky header / scrollable body / sticky footer). The natural height (~700 px) exceeded the 500-px-tall popup on the previous release, which clipped both the rounded top corners and the bottom credit line off-screen. The body now scrolls inside the modal, both sticky sections stay visible, the rounded `rounded-xl` corners are guaranteed visible. — *#fix(ui)*
- **Color picker dialog** gets the same `max-h-[calc(100vh-2rem)] overflow-y-auto` safety net so its rounded corners survive on small popup heights too. The picker is more compact (~450 px) so scrolling rarely triggers, but the constraint costs nothing and matches the About-dialog treatment.

## [0.10.4] — 2026-05-09

### Changed — UI consistency pass on modals

- **About dialog and Color picker dialog now share `rounded-xl` corners** (12 px instead of 8 px) for a softer, more macOS-native look. Inner cards inside the About dialog (identity block, workflow pitch) bumped to match. Establishes the visual hierarchy: modals = `rounded-xl`, inline cards/strips = `rounded-lg`, inputs/buttons = `rounded` / `rounded-md`. — *#chore(ui)*

### Added — Restore-defaults inline confirm

- **Snippets sidebar's "Restore defaults" icon now uses a two-step inline confirm**, matching the pattern History's "Clear all" introduced in v0.6.1. First click on the `RotateCcw` icon → toolbar row swaps to `Restore defaults? Yes / Cancel` in red; second click on `Yes` actually re-imports the bundled AI-prompt templates. Previously a single misclick would silently overwrite all default-abbreviation snippets — destructive without confirmation. — *#feat(snippets)*

## [0.10.3] — 2026-05-09

### Added — History time chip is now interactive

- **Hover the relative-time chip** (`just now`, `1h ago`, `3d ago`) on any history row → tooltip shows the absolute timestamps for both `Captured` and `Last used` (or `Captured: ... · (never re-used since)` when the entry hasn't been pasted again). — *#feat(history)*
- **Click the chip** → toggles the chip text in place between relative (`1h ago`) and absolute (`9 May 2026, 06:41:05`) display. `stopPropagation` so the click doesn't double-fire the row-select handler. Per-row state, so different rows can be in different display modes simultaneously.
- New `formatAbsolute(unixMs)` helper in [`core/frontend/src/lib/format.ts`](./core/frontend/src/lib/format.ts) using `Intl.DateTimeFormat` with the user's locale — matches Finder / Mail formatting muscle memory.

### Fixed — Snippets sidebar toolbar layout

- **Three sidebar actions are now icon-only.** `+ New Snippet`, `Import`, and `Restore defaults` previously wrapped two-line in the ~40 % sidebar column, with `Restore defaults` spilling outside the section. Replaced with three 28×28 icon buttons (`Plus`, `Upload`, `RotateCcw`) carrying the labels in `title` tooltips and `aria-label`s. — *#fix(snippets)*

## [0.10.2] — 2026-05-09

### Fixed — CI build on Linux runners

- **`ocr.rs` and `region_picker.rs` now have catch-all stubs for non-macOS / non-Windows targets.** Both modules were `#[cfg]`-gated for macOS + Windows but never declared a fallback impl, which made the `pub fn recognize` / `pub fn capture` wrappers fail to resolve their delegated `recognize_impl` / `capture_impl` symbol on Linux. The release CI runs on `ubuntu-latest` and broke as a result. The new stubs return `"OCR is not implemented on this platform"` / `"region capture is not implemented on this platform"`. — *#fix(ci)*
- Cleaned up the unused `anyhow::Context` import in `region_picker.rs` — only the macOS impl uses it, so it's now `#[cfg(target_os = "macos")] use anyhow::Context;`. Silences the `unused_imports` warning on Linux/Windows builds.

### Changed — README badge wall

- Doubled the badge set with grouped sections (Status / Platforms / Stack / Security / Quality / Community). Adds Linux planned, x86_64, ONNX Runtime, Apple Vision, U²-Net, AES-256-GCM, OS keychain, local-first, no-telemetry, offline, power-user, keyboard-first, Prettier, vitest count, contributors, forks, watchers, closed issues, PRs open, commit activity, lines-of-code. Test-count badge updated 98 → 107 (recolor + cutout + cutout_ml).

## [0.10.1] — 2026-05-09

### Added — Save image entry to Downloads

- **New "Save to Downloads" button + `Cmd/Ctrl+S` shortcut** below the cutout button on every image entry. Writes the selected entry's PNG bytes unchanged to `~/Downloads/inspector-rust-image-<ts>.png`. Companion to recolor — clicking a recolor swatch creates a new history entry with the tinted image; this lets the user grab that entry as a real file on disk without going through cutout (which would transform it). Same UX shape as the cutout button (busy state, saved-filename feedback, error toast). — *#feat(image)*
  - **IPC:** `save_image_entry_to_downloads(id) → path`. UI in `SaveImageButton` inside [`PreviewPanel.tsx`](./core/frontend/src/components/PreviewPanel.tsx).
  - Workflow: select image → recolor swatch → ↑ to the new tinted entry → `Cmd+S` → done.

## [0.10.0] — 2026-05-09

### Changed — Cutout switched from chroma-key to ML

- **U2Netp ONNX model now drives the cutout pipeline** (`cutout_ml.rs`). Cross-platform via the `ort` crate (ONNX Runtime, statically linked). Same architecture as Python's `rembg`, no Python dependency. — *#feat(cutout)*
  - **Why the switch.** The v0.8.0 chroma-key approach (corner-sampled background colour) only worked on truly uniform backgrounds. Real photos — airplane in gradient sky, person against cluttered background, anything where subject and background share colours — produced cutouts that left most of the background intact. Subject segmentation is the right tool; chroma-key is the wrong one.
  - **Pipeline:** decode any input format (PNG / JPEG / WebP / GIF / BMP) → resize to 320×320 → ImageNet-normalise → run U2Netp inference → resize the resulting saliency mask back to the original dimensions → apply as alpha on the original RGB → encode as PNG. ~1–4 s on CPU for a typical-size photo.
  - **Bundled artifacts:** [`core/rust-lib/models/u2netp.onnx`](./core/rust-lib/models/u2netp.onnx) (4.5 MB, Apache-2.0). The ONNX Runtime native library is statically linked via `ort`'s `download-binaries` feature, growing the release binary from ~12 MB to ~40 MB.
  - **Deps added:** `ort = "2.0.0-rc.12"` + `ndarray = "0.17"` (workspace); pulled into `core/rust-lib`. We tried `tract-onnx` first (pure Rust, no FFI) but it can't run U2Net's `Resize` ops with `pytorch_half_pixel` correctly; ort handles them natively.
  - **Old chroma-key code** in `cutout.rs` is kept around (marked `#![allow(dead_code)]`) as a future fast-path for known-uniform-background inputs.
  - **Tests:** 3 unit tests in `cutout_ml::tests` cover the smoke path (synthetic input → valid PNG out), oversize rejection, and corrupt-input rejection.

## [0.9.0] — 2026-05-09

### Added — Screen-region OCR (macOS)

- **`Cmd+Shift+O` triggers an interactive screen-region picker.** Drag a marquee over any text on screen, Inspector Rust runs Apple Vision OCR on the selection, writes the recognized text to the system clipboard, and pushes it into history. The source PNG is kept as a separate image entry so the user can re-OCR a different region without rescreenshotting. Tray menu also exposes an **OCR Region (⌘⇧O)** entry for discoverability. — *#feat(ocr)*
  - **Region picker** ([`region_picker.rs`](./core/rust-lib/src/region_picker.rs)) shells out to `/usr/sbin/screencapture -i -x -t png`, the same binary backing Cmd+Shift+4 — battle-tested marquee UX (Esc cancels, Space drags the rect, etc.) without reinventing an `objc2` overlay window. Captured PNG read from a temp file then deleted.
  - **OCR engine** ([`ocr.rs`](./core/rust-lib/src/ocr.rs)) uses Vision's `VNRecognizeTextRequest` (accuracy=Accurate, `usesLanguageCorrection=true`) via raw `objc2` `msg_send`. Joins one `\n` between observations (Vision returns one observation per visual line). Empty results are surfaced as `OcrResult { chars: 0 }` rather than an error so the UI can differentiate "engine ran but found nothing" from "engine failed".
  - **Build** — new `core/rust-lib/build.rs` emits `cargo:rustc-link-lib=framework=Vision` on macOS so the framework is linked. No new crate dependencies.
  - **IPC:** `ocr_region() -> { text, cancelled, chars }`. Both the global shortcut and the tray menu route through the shared `commands::run_ocr_pipeline(app)` helper, which dispatches the screencapture wait to a worker thread.
  - **Watcher integration:** the OCR pipeline calls `mark_self_write` before writing, so the clipboard watcher doesn't double-capture the result as a fresh user copy.
  - **Windows:** stubbed — both `region_picker::capture` and `ocr::recognize` return "not yet implemented on Windows" so the workspace still builds. Implementation will use `Windows.Media.Ocr` + a snipping overlay in a follow-up release.

## [0.8.0] — 2026-05-09

### Added — Image cutout / Freistellen

- **Background-removal action** in the image preview pane. Selecting an image entry shows a "Cut out background" button (plus `Cmd/Ctrl+B` shortcut); clicking it chroma-keys the image and saves the transparent PNG to `~/Downloads/inspector-rust-cutout-<timestamp>.png`. — *#feat(image)*
  - **Algorithm.** Sample the four corners of the image (8×8 patches per corner, median per channel — robust to subject pixels bleeding into the corner regions), treat that as the background colour, and replace each pixel with `alpha = 0` if its colour is within 30 RGB units of the background, `alpha = original` if beyond 50 units, with linear feathering in the band between (smooth cutout edge).
  - **Sweet spot.** Subjects on uniform backgrounds — sky, studio backdrops, solid logo fields. Cluttered / busy backgrounds hit the limit of chroma-keying; pro-grade results would need ML (rembg / U2Net), which is out of scope for a clipboard utility.
  - **Bounds & safety.** Hard cap at 16 megapixels. Output goes to `~/Downloads` (or `$HOME` if that doesn't resolve); the source history entry is left untouched.
  - **Module:** [`core/rust-lib/src/cutout.rs`](./core/rust-lib/src/cutout.rs) (~210 LOC). 5 unit tests cover background detection, subject preservation, oversize rejection, the all-background degenerate case, and transparent-corner handling.
  - **IPC:** `cut_out_image_entry(id) → saved_path`. Frontend wrapper in [`ipc.ts`](./core/frontend/src/lib/ipc.ts), UI in `CutoutButton` inside [`PreviewPanel.tsx`](./core/frontend/src/components/PreviewPanel.tsx).

### Added — About dialog + footer credit

- **About dialog** behind a button in **Settings → About**. Shows version, developer, license, year, target-audience pitch, and a tabular tech-stack overview (Tauri 2 / Wry / Rust / SQLite + AES-256-GCM / React 19 / TypeScript 5 / Vite 7 / Tailwind v4 / `image` 0.25). Esc / backdrop / X all close. — *#feat(ui)*
- **Author credit** ("made with ♥ by Martin Pfeffer") added to the popup footer next to the version chip and entry counter. — *#feat(ui)*

### Changed — Documentation

- **README rewrite.** Subtitle now reads "The keyboard-first clipboard toolkit for power users — Windows 11 & macOS"; new **Workflow** section frames the `Ctrl+Shift+V → type → Enter` loop; **Features** section reorganised by theme (Clipboard core / Text expander / AI prompts / Calculator / Color tools / Image tools / Notes / Backup / Plain-text paste / Tray + multi-monitor) with each block tightened to a scannable header + 3–6 bullets. Encryption (v0.6.0) promoted from "Limitations" into the Clipboard core feature list where it belongs.
- **Tauri bundle metadata** (`copyright`, `shortDescription`, `longDescription`) updated to drop the `celox.io` chatter and reflect the broader feature set / power-user audience. Bundle id stays `io.celox.inspector-rust` — that's a stable technical identifier the keychain & TCC depend on.
- **Snippet example signatures** anonymised to use `Your Name` / `https://example.com` placeholders so they're useful as templates for any user.

## [0.7.0] — 2026-05-08

### Added — Image recolor

- **Recolor toolbar in the image preview pane.** Selecting a mostly-grayscale image entry (logo, icon, silhouette) reveals a row of 9 preset swatches plus a hex input below the preview. Clicking a swatch or pressing Enter on a hex tints the image and stores the result as a new history entry — the original stays put. — *#feat(image)*
  - **Algorithm.** Decode PNG → for each RGBA pixel, replace RGB with `lerp(target, white, BT.601_luminance)`, preserve alpha → re-encode. Equivalent to ImageMagick's `+level-colors target,white`. Pure Rust via the `image` 0.25 crate (PNG-only feature set, no other format codecs pulled in).
  - **Photo guard.** Chromaticity sampling (`max((max-min)/max)` over up to 4096 opaque pixels) gates the UI: ≥ 0.12 hides the toolbar so saturated photos can't get accidentally tinted into Photoshop disasters.
  - **Bounds.** Hard cap at 16 megapixels to keep the synchronous recolor on the UI thread responsive on slower hardware.
  - **Module:** [`core/rust-lib/src/recolor.rs`](./core/rust-lib/src/recolor.rs) (~140 LOC). 6 unit tests cover dark→target mapping, white→white anchor, alpha preservation, oversize rejection, and chromaticity probe edges (pure-grayscale → ~0, pure-red → > 0.9).
  - **IPC:** `recolor_image_entry(id, hex) → new_id`, `image_chromaticity(id) → 0..1`. Frontend wrapper in [`core/frontend/src/lib/ipc.ts`](./core/frontend/src/lib/ipc.ts); UI in `RecolorToolbar` inside [`PreviewPanel.tsx`](./core/frontend/src/components/PreviewPanel.tsx).
  - **Deps added:** `image` 0.25 with `default-features = false, features = ["png"]` (avoids BMP/GIF/HDR/EXR/etc. baggage).

### Fixed — Clipboard capture priority

- **Image-before-files in the watcher.** macOS puts both the bitmap *and* the file path on the pasteboard when you copy an image file (PNG / JPG / HEIC) from Finder or use "Share → Copy Image" in many apps. The previous priority order (`files → image → …`) meant Inspector Rust stored only the path — users would see `/Users/.../foo.png` in history instead of the actual picture. Order is now `image → files → html → rtf → text`; pure file copies (PDFs, ZIPs, …) still capture as Files exactly as before. — *#fix(watcher)*

## [0.6.1] — 2026-05-07

### Fixed

- **Clear all confirmation** — replaced unreliable `window.confirm` (silent in Tauri's WebView2) with an inline "Delete N clips? Yes / Cancel" prompt in the history toolbar. — *#fix(ui)*
- **Bookmark visual feedback** — clicking the bookmark icon now shows a filled `BookmarkCheck` icon in accent color for 1.5 s so the user can see the note was saved. — *#fix(ui)*
- **Color picker modal height** — reduced SVPicker height (`h-44 → h-32`), swatch height (`h-16 → h-10`), and tightened margins so the modal fits inside the 500 px popup on Windows without scrolling. — *#fix(color-picker)*

## [0.6.0] — 2026-05-06

### Added — At-rest encryption for sensitive content

- **The SQLite database now encrypts every sensitive content field with AES-256-GCM.** Closes the long-standing "Unencrypted storage" limitation row in the README — passwords, tokens, snippet bodies, and note bodies are no longer readable to anyone who can `cat` the DB file. — *#feat(security)*
  - **Encrypted columns:** `entries.content_text`, `entries.content_data`, `snippets.body`, `notes.content_text`, `notes.content_data`. **Not encrypted:** timestamps, content-type tags, dedup `hash`, snippet abbreviations, note titles/categories — those are metadata that doesn't reveal clipboard content.
  - **Storage format.** Each encrypted value is stored as TEXT prefixed with `v1:` followed by base64 of `12-byte random nonce ‖ ciphertext+tag`. Legacy plaintext rows (no `v1:` prefix) are detected on read and returned as-is, then re-encrypted in place by the migration step at next startup. The migration is idempotent — already-encrypted rows are skipped.
  - **Key storage.** Per-install random 256-bit key kept in the **OS keychain** (macOS Keychain / Windows Credential Manager / Linux Secret Service) under service `io.celox.inspector-rust`, account `history-db-key-v1`. Falls back to a 0600 keyfile (`<data-dir>/.dbkey`) if the keychain is unavailable so the app stays usable instead of crashing. The fallback is strictly weaker — file-system access gets you the key — but matches the previous threat model floor.
  - **Roundtrip-safe across paths.** `save_from_clip` (Notes ← Clipboard) passes the already-encrypted ciphertext straight into the notes row instead of decrypt-then-reencrypt — same key, same scheme, ~free. `append_imported` from a JSON backup re-encrypts on the way in (backups stay plaintext for portability).
  - **Module:** [`core/rust-lib/src/crypto.rs`](./core/rust-lib/src/crypto.rs) (~280 LOC). 6 unit tests cover roundtrip, legacy plaintext passthrough, empty strings, fresh-nonce-per-encrypt, tampered-ciphertext rejection, wrong-key rejection.
  - **Deps added:** `aes-gcm` 0.10, `rand` 0.8, `keyring` 3 (cross-platform OS-keychain crate).

### Why 0.6.0

This is a feature with security implications and a one-time data migration on first launch — not a bug fix. Per `docs/RELEASING.md`'s 0.x.0-vs-0.x.y rule, that earns a minor bump.

## [0.5.2] — 2026-05-06

### Added — System-wide screen color picker (eyedropper)

- **The Color picker modal now has a "Pick from screen" button** that lets you sample a color from anywhere on the desktop, not just inside Inspector Rust's own UI. The picked hex is automatically inserted into the modal — ready to copy as HEX / RGB / HSL. — *#feat(colors)*
  - **macOS:** uses Apple's own `NSColorSampler` (AppKit, 10.15+) — the same magnifier-loupe used by Pages, Keynote, and Sketch. Clicking outside the loupe cancels.
  - **Windows:** spawns a fullscreen layered overlay; click anywhere on screen to sample (`GetPixel` on the desktop DC). Press Esc to cancel.
  - **Async architecture.** The `pick_screen_color` IPC returns immediately; the result arrives later via the `color-picked` Tauri event with `string | null` payload. Keeps the UI responsive while the user is targeting their click.
  - New module `core/rust-lib/src/screen_picker.rs` (≈180 lines, fully `#[cfg(target_os = …)]`-gated). Adds `objc2` 0.6 + `block2` 0.6 as macOS-only deps for the Objective-C runtime calls; Windows reuses the existing `windows` 0.61 crate with extra features (`Win32_UI_WindowsAndMessaging`, `Win32_Graphics_Gdi`, `Win32_UI_Input_KeyboardAndMouse`).
  - **Tahoe quirk worth knowing.** macOS Tahoe's `NSColorSampler` only renders its loupe when the calling app is a *Regular* (Dock-visible) NSApplication. Inspector Rust normally runs as `Accessory` (Dock-hidden tray app), so the picker briefly promotes the activation policy to Regular while the loupe is up, then demotes back 500 ms after the popup is restored. The popup itself stays visible during the pick — hiding it kills the loupe rendering ("no key window → no loupe").

### Docs

- README tagline updated to "Windows 11 & macOS"; previously said Windows 11 only.
- New / refreshed badges: separate Windows / macOS / Apple Silicon platform badges, plus Vite 7, ESLint flat-config, Vitest 3, cargo-test count, last-commit, repo-size, code-size, top-language.
- `docs/colors.md` rewritten end-to-end to describe the v0.5.x custom HSV modal, the click-to-select UX, and the screen eyedropper. The old "OS-native NSColorPanel / Win32 ChooseColor / GTK ColorChooser" copy was outdated since v0.5.0.

## [0.5.1] — 2026-05-06

### Fixed — Accessibility prompt fired on every paste

- **The actual root cause of "permission keeps re-prompting" is finally identified and fixed.** `enigo`'s `Settings::default()` ships with `open_prompt_to_get_permissions = true` on macOS — meaning every `Enigo::new()` call internally invokes `AXIsProcessTrustedWithOptions` *with the prompt option enabled*. So **every paste action on an untrusted process fired the standard "Inspector Rust would like to control this computer" dialog as a side effect** — even though we just wanted to silently fall back. — *#fix(macos)*
  - **Fix:** new `enigo_settings()` helper in `paste.rs`, `expander.rs`, and `text_field/windows.rs` constructs `Settings { open_prompt_to_get_permissions: false, ..Settings::default() }`. Every `Enigo::new()` now uses it. enigo silently returns `NoPermission` when the process is untrusted; the dialog never fires as a paste-time side effect.
  - **Plus AX guard at the top of every paste IPC.** `paste_entry`, `paste_entry_formatted`, `paste_text`, `paste_snippet`, `paste_note`, `paste_note_formatted` all start with `require_accessibility()?` — short-circuits before even touching enigo and returns the structured `ax.permission_denied` error string to the frontend.
  - **Frontend toast.** `App.tsx` catches paste errors and renders an amber sticky banner: *"Paste failed — macOS Accessibility access not granted. Open the Settings tab and click Force re-grant…"* with an **Open Settings** button. Auto-dismisses after 8 s. The user finally has clear feedback instead of a silent failure or a recurring system dialog.
- **Live-debug methodology** documented in the commit history (kept in `git log` rather than the codebase): a temporary background AX-poller revealed that `AXIsProcessTrusted()` does *not* cache per-process on Tahoe — it re-queries TCC on every call. So our SettingsPanel polling has always been correct; the `ax.permission_denied` toast is the right user-facing complement.

### Changed — Color picker UX

- **Modal opens in a "no selection yet" state.** v0.5.0 default-filled the picker with `#3366FF` so the toolbar-button click felt like it had already selected a color. Now the modal opens with: empty hex input, dashed-border placeholder swatch reading "Click in the picker above (or type a hex) to select a color", and Copy disabled. **The first click in the SV picker is the selection** — matching the user's mental model of "1st click opens, 2nd click selects". — *#fix(colors)*
  - SV-picker crosshair indicator hidden until first click.
  - Hue-slider drag and hex-input typing also count as "selection" once the user engages with them.
  - Closing & re-opening the modal resets to the no-selection state.

## [0.5.0] — 2026-05-05

### Added — 25 default AI prompt snippets, working color picker

- **Bundled default snippet library — 25 curated AI prompts.** First-launch seeds your snippet table with `ai*`-prefixed prompts covering programming (`aiplan`, `aireview`, `airefactor`, `airegex`, `aisql`, `aitest`, `aimigration`, `aibench`), web/frontend (`aithumb`, `aimobile`, `aia11y`, `aiseo`, `aicomponent`), IT security (`aithreat`, `aipentest`, `aiauth`, `aigdpr`), business workflows (`aibrief`, `airfp`, `aiokr`, `aichange`), data analysis (`aidataq`, `aiml`, `aidashboard`), and architecture (`aiapi`). Each prompt is a structured, opinionated brief — sections, bullets, output-format directives — written to be handed straight to an LLM without further massaging. Type the abbreviation in the search field, press Enter (or use the text expander), get the full prompt. — *#feat(snippets)*
  - **Idempotent seeding.** Tracked via `seed.default_snippets_v1` in the settings table. Runs once on first install; user-deleted prompts stay deleted on subsequent launches.
  - **Restore defaults button** in the Snippets-tab sidebar (rotate-counter-clockwise icon, next to Import). Re-imports all 25 prompts, upsert-by-abbreviation — your custom snippets with different abbreviations are untouched, but a deleted/edited `aiplan` *is* reset to the bundled version.
  - Embedded via `include_str!` so no external file is needed at runtime.
  - 3 new Rust unit tests (`embedded_json_parses_and_has_25_prompts`, `maybe_seed_inserts_on_first_run_and_skips_after`, `restore_defaults_re_imports_explicitly`).
- **Working cross-platform color picker.** v0.4.0's HTML5 `<input type="color">` was unreliable in WKWebView (Tauri's macOS renderer) — the OS picker often didn't open, and even when it did, `navigator.clipboard.writeText` got blocked because the `change` event fires outside the user-gesture context. Replaced with a **custom modal** that runs entirely in the WebView. — *#fix(colors)*
  - Hue slider + 2D saturation/value picker + live hex input + format tabs (HEX/RGB/HSL) + WCAG-readable preview swatch + Copy button.
  - Clipboard write goes through `@tauri-apps/plugin-clipboard-manager`'s `writeText` (no browser-API restrictions).
  - Esc / backdrop-click closes; copy feedback flashes "Copied!" for 2s.
  - Capabilities updated: `clipboard-manager:allow-write-text` added to both `macos/src-tauri/capabilities/default.json` and `win/src-tauri/capabilities/default.json`.

### Why 0.5.0 (not 0.4.3)

The 25-prompt seed is a real new feature surface, AND first-run behavior changes (new users automatically get a populated snippet library — that's an opinion, not a fix). Bumping minor signals it.

### Tests

`cargo test --workspace`: **84 → 87 green** (+3 seed). `pnpm test`: **77 → 85 green** (+8 HSV/HSL/hex helpers).

## [0.4.2] — 2026-05-05

### Fixed

- **No more duplicate history entries from plain-text paste.** v0.4.0's plain-text-paste downgrade for HTML / RTF clips was leaking back into the watcher: Inspector Rust wrote the plain-text version of an HTML clip to the OS clipboard → the clipboard watcher saw the change → recorded a *new* Text-type entry `just now`, sitting next to the original HTML clip from earlier. Hash-based dedup didn't catch it because `hash(Html, "<p>foo</p>") ≠ hash(Text, "foo")`. — *#fix(watcher)*
  - **Fix:** `WatcherState` gets a one-shot `self_written: Mutex<Option<String>>` fuse holding the SHA-256 of the most recent payload we wrote ourselves. The watcher checks this hash before storing and consumes-and-skips any matching event. Every paste IPC (`paste_entry`, `paste_entry_formatted`, `paste_text`, `paste_snippet`, `paste_note`, `paste_note_formatted`) calls `watcher.mark_self_write(content_type, payload)` immediately before triggering the OS clipboard write. Net effect: pasting from history never creates a duplicate entry, regardless of the plain-text setting.
- **Macros prompt no longer fires as an unwanted side effect.** When `expand_at_cursor` (hotkey trigger) or `diagnose_at_cursor` (Test button) call `AXUIElementCopyAttributeValue` on the system-wide element while Inspector Rust is **untrusted** (typical post-rebuild stale-cdhash state), macOS triggers the standard "would like to control this computer" prompt as a side effect — even when we just want to silently fall back to the clipboard path. — *#fix(macos)*
  - **Fix:** both functions now check `accessibility_granted()` *before* calling any AX function. When `false`, they go straight to the clipboard fallback (or return an empty diagnose result), and the macOS prompt isn't triggered as a no-op cost. The Settings panel's amber banner + **Force re-grant** button remain the right place to surface the underlying permission issue.

## [0.4.1] — 2026-05-05

### Changed

- **`paste_note` now respects `paste.plain_text_only`.** v0.4.0 added the plain-text-paste toggle for clipboard history, but notes (a separate paste path via `paste_note`) kept their old behaviour — HTML / RTF notes always pasted with formatting. The user's original ask was "always plain text in all OSes" which implicitly covers notes too. Now: HTML / RTF notes get downgraded to their plain-text preview when the toggle is on; image / files notes remain unaffected. — *#fix(paste)*
- New `paste_note_formatted` IPC command mirrors `paste_entry_formatted` — bypasses the setting and uses the note's original content type. Wires up symmetrically; the NotesPanel UI doesn't surface a Shift+click override yet but the IPC is ready when we add one.

### Docs

- `docs/notes.md` paste-behaviour table updated to call out which content types respect the plain-text-only toggle and which are unaffected.

## [0.4.0] — 2026-05-05

### Added — Plain-text paste, hex color preview, color picker

- **Plain-text paste mode (default on).** Settings → Paste section gets a new toggle. When on, HTML and RTF clipboard entries are stripped to their plain-text preview at paste time — so copy-from-Word / browser / mail and paste-into-anything no longer leaks the source app's font / colour / hyperlink styling. The original formatted content is preserved in the history (preview pane still renders it; the type icon still shows HTML / RTF), only the *paste action* downgrades. Image / Files entries are unaffected. — *#feat(paste)*
  - **Per-row override:** hold <kbd>Shift</kbd> while pressing <kbd>Enter</kbd> in the popup to paste *with* original formatting, regardless of the toggle. New IPC `paste_entry_formatted` bypasses the setting; `useKeyboardNav` forwards `event.shiftKey` to the activate handler.
  - Backend: `paste.plain_text_only` setting key (default `true`); `paste_entry` reads it and routes Html / Rtf entries to `paste::paste_text(content_text)`. `paste_entry_formatted` always uses `paste::paste_entry` for original-content-type behaviour.
- **Inline hex color preview** in the search input — Alfred-style. — *#feat(colors)*
  - Type `#3366FF` (or `3366FF`, `#abc`, `#abcdef12`, …) and a color row appears as the top list item with a swatch + hex + RGB summary. Press <kbd>Enter</kbd> to paste the canonical `#RRGGBB` (uppercase) into the previously focused app.
  - Heuristic: 3 / 4-digit forms require the `#` prefix (too ambiguous with search otherwise — `abc`, `f00d`, …); 6 / 8-digit forms accept either form.
  - Preview pane shows a full 128 px swatch with the hex overlaid (foreground auto-picked black/white via WCAG luminance for readability), plus copy-to-clipboard buttons for hex / `rgb(…)` / `hsl(…)` strings.
  - Pure frontend (`core/frontend/src/lib/colors.ts`); 24 vitest cases covering valid / invalid / canonicalisation / RGB-HSL conversion / readable-foreground.
- **OS-native color picker** — new "Color picker" button in the History tab's toolbar. Opens an `<input type="color">` which Tauri renders via the OS-native picker (NSColorPanel on macOS, Win32 ColorDialog on Windows, GTK ColorChooser on Linux). The chosen hex (uppercase) is written to the system clipboard via the Web Clipboard API; the watcher captures it as a fresh history entry within the next event tick. — *#feat(colors)*

### Changed

- `App.tsx` activate handler: signature changes to `activate(i, shiftKey)`. Color-row activation pastes the canonical hex via the existing `paste_text` command. Calc-row activation unchanged.
- `useKeyboardNav.onEnter` callback signature is now `(shiftKey: boolean) => void`.
- `HistoryItem` and `PreviewPanel` learn a fourth row kind (`color`) alongside clip / snippet / calc.
- `ListEntry` discriminated union gains `{ kind: "color"; data: ColorEntryView }`.

### Tests

`pnpm test`: **53 → 77 frontend** (+24 colors tests). `cargo test --workspace`: 84 unchanged (paste-plain-text logic exercises through existing paste tests; the wiring is straightforward enough that integration testing is overkill here).

### Why 0.4.0 (not 0.3.2)

Plain-text-paste-by-default is a **behaviour change**: clipboard entries that *used* to paste with formatting now arrive as plain text, by default, without the user opting in. That's a semver-meaningful flip. Two new user-facing features (hex preview, color picker) compound it. Bumping minor signals the change.

## [0.3.1] — 2026-04-29

### Fixed

- **macOS Accessibility prompt loop after rebuilds.** Common state after a real source-change install: the toggle in System Settings → Accessibility shows Inspector Rust as **enabled**, but Inspector Rust still asks for permission on every hotkey press. Cause: the toggle's underlying TCC entry is bound to the *previous* binary's cdhash; the new build has a different cdhash and is treated as a new app. The toggle UI just reports the bundle id, which masked the discrepancy.
  - **Fix:** new **Force re-grant (clear stale)** button in the amber Accessibility banner. Shells out to `tccutil reset Accessibility io.celox.inspector-rust` + `tccutil reset PostEvent io.celox.inspector-rust` (no sudo needed for the user's own bundle), then fires `AXIsProcessTrustedWithOptions(prompt: true)` so macOS re-adds Inspector Rust to the Accessibility list with the *current* cdhash. Toggling on again creates a TCC entry that matches what the running process actually is. — *#fix(macos)*
  - The legacy "Try system prompt" button stays as a secondary option (for the rare cases where the entry is sane and just needs a re-prompt).
- New IPC command `force_reset_and_request_grant` (macOS-only meaningful behaviour; no-op elsewhere). Backend in [`core/rust-lib/src/expander.rs`](./core/rust-lib/src/expander.rs); wrapper in [`core/frontend/src/lib/ipc.ts`](./core/frontend/src/lib/ipc.ts).

## [0.3.0] — 2026-04-28

### Added — Accessibility-first text expander

- **The text expander now reads the focused field directly via the OS accessibility layer** instead of synthesising `Cmd/Ctrl+Shift+←` + `Cmd/Ctrl+C` as the *primary* path. macOS uses **`AXUIElement`** (ApplicationServices), Windows uses **`IUIAutomation`** (UIAutomationCore). Same Accessibility permission already required for paste; no new permission added. Native FFI — no objc2/winRT macros needed. — *#feat(expander)*
  - **Why it matters:** the keystroke approach works in 90 % of apps but breaks in terminals (iTerm2, kitty, gnome-terminal — they reinterpret `Cmd/Ctrl+Shift+←` as pane-switch / mark-selection), web apps with custom keyboard handlers (Google Docs, online IDEs), and password fields. The accessibility approach succeeds wherever the focused element exposes its value to assistive tech — which is essentially every text field a screen reader can read.
  - **No more clipboard touch on the happy path.** When AX/UIA succeeds the user's clipboard is left completely untouched and there's no visible selection flicker.
  - **Clipboard fallback retained.** When the focused element doesn't expose the necessary attributes (rare native Carbon, Java/Swing without AccessBridge), Inspector Rust falls back to the previous keystroke + clipboard roundtrip seamlessly.
- **`text_field` module** — new abstraction in [`core/rust-lib/src/text_field/`](./core/rust-lib/src/text_field/):
  - `mod.rs` — `FieldAccess` trait + `CapturePath { Ax, Uia, Clipboard }` enum + UTF-16 ↔ char-index helpers + the platform-agnostic `word_start_before_cursor` algorithm. 7 unit tests covering ASCII, German umlauts, emoji (supplementary plane), cursor past end, whitespace-only.
  - `macos.rs` — raw FFI to `AXUIElementCreateSystemWide` / `AXUIElementCopyAttributeValue` / `AXUIElementSetAttributeValue` for the three attributes that matter: `AXFocusedUIElement`, `AXValue`, `AXSelectedTextRange`. UTF-16 helpers because AX reports cursor positions in UTF-16 code units. 3 unit tests.
  - `windows.rs` — `windows` crate bindings to `IUIAutomation`, `IUIAutomationTextPattern`, `IUIAutomationTextRange`. Uses UIA for the *read* (reliable) but deliberately uses Backspace×N + `enigo.text(body)` for the *write*, because UIA's `IUIAutomationTextEditPattern2::Replace` is patchily implemented across real-world Windows controls.
- **`Capture path` row in the Diagnose UI** — Settings → *Text expander* → Diagnose now shows whether the run used `macOS AX (clean — no clipboard touch)`, `Windows UIA (clean — no clipboard touch)`, or fell back to the `Clipboard fallback` path. Lets you tell at a glance whether the app you're testing in has working accessibility.

### Changed

- `expander::expand_at_cursor` and `expander::diagnose_at_cursor` now try AX/UIA first; the legacy clipboard roundtrip is the second-choice fallback. The fallback path can also be invoked with prefetched abbreviation+body so the lookup isn't repeated when AX read succeeded but AX replace didn't.
- `core/rust-lib/Cargo.toml` — added `windows = { version = "0.61", features = ["Win32_Foundation", "Win32_System_Com", "Win32_UI_Accessibility"] }` as a `target.'cfg(target_os = "windows")'` dependency. macOS / Linux builds don't pull it in.
- **`DiagnoseResult`** gains a `path: "ax" | "uia" | "clipboard"` field. Frontend `ipc.ts` interface updated to match.

### Why bump to 0.3.0

This is a real architecture change for the expander — the keystroke path is no longer the default. Bumping the minor signals that the failure modes (and therefore the user-visible behaviour) shift. The fallback path keeps full backward compatibility — every app that worked in 0.2.x still works in 0.3.0, just often via a cleaner mechanism.

### Tests

`cargo test --workspace`: **74 → 84 green** (+7 word-boundary, +3 UTF-16). `pnpm test`: 53 unchanged.

## [0.2.12] — 2026-04-28

### Changed

- **Backup Export / Import moved to the Settings tab.** Lived under the Notes tab's sidebar since v0.2.6, but conceptually belonged with the rest of the app-level configuration. The Notes tab keeps **+ New Note** and **Clear All**; everything backup-related is now under the new **Backup & restore** section in Settings. — *#feat(settings)*
- **Selective export.** Three checkboxes — *Clipboard history*, *Snippets*, *Notes* — let you choose which sections land in the file. All checked by default; unchecking any of them writes an empty array for that section in the JSON. Intended use: share snippets without leaking your clipboard history.
  - Backend: new `backup::ExportOptions { include_history, include_snippets, include_notes }` with `::all()` / `::default()` constructors. Both `export_backup` and `save_backup_to_file` IPC commands take three optional flags (default `true`). Existing callers stay backward-compatible.
  - Frontend: `BackupExportOptions` interface in `ipc.ts`. `exportBackup()` / `saveBackupToFile(path, opts)` accept the same fields.
  - 3 new Rust unit tests (`export_with_only_snippets…`, `export_with_all_off…`, `export_options_default…`). Backend total: 71 → **74 green**.

### Fixed

- After an Import, the Notes / Snippets / History tabs now refresh immediately. The Settings panel takes an `onBackupImported` prop from `App.tsx` that re-fires the three list hooks (`refreshHistory`, `refreshSnippets`, `refreshNotes`) once the merge returns.

## [0.2.11] — 2026-04-26

### Fixed

- **Crash on hotkey / Test now: `EXC_BREAKPOINT` from `_dispatch_assert_queue_fail`.** The text-expander dispatched `enigo` work onto a worker thread (`std::thread::spawn` in `register_expander`, plus the IPC handler thread for `trigger_expand_at_cursor` / `diagnose_expand_at_cursor`). On macOS, enigo's `Key::Unicode(...)` mapping calls `TSMGetInputSourceProperty` (Text Services Manager) which **asserts main-thread**. Calling it from any other thread fires a libdispatch assertion and aborts the process with SIGTRAP. Confirmed by three crash reports today: `inspector-rust-2026-04-26-070927.ips`, `…-070931.ips`, etc — all ended at `enigo::macos_impl::keycode_to_string` from a worker thread.
  - **Fix:** all three call sites now dispatch the expand cycle to the main thread via `AppHandle::run_on_main_thread`. The hotkey path is fire-and-forget; `diagnose_expand_at_cursor` ferries the result back through an `mpsc::channel`. The popup is hidden during the cycle, so the ~290 ms main-thread block is invisible to the user.

## [0.2.10] — 2026-04-26

### Fixed

- **macOS Accessibility re-grant loop is finally broken.** Real root cause this time, not symptoms: macOS Tahoe (26.x) binds the TCC Accessibility grant to the tuple `(bundle id, cdhash)`. `scripts/install-macos.sh` previously ran `codesign --force` on every install — even when the user re-installed an *unchanged* binary — which embedded a fresh CMS timestamp into the signature blob and produced a new cdhash. macOS then dropped the prior grant, prompting again. — *#fix(macos)*
  - **Idempotent install.** The script now SHA-256 compares the freshly built binary at `target/release/bundle/macos/InspectorRust.app/Contents/MacOS/inspector-rust` against the currently installed binary at `/Applications/InspectorRust.app/Contents/MacOS/inspector-rust`. If they're identical (and the bundle identifier already matches), the script **skips both `cp` and `codesign`** entirely — your install is preserved verbatim, the cdhash stays stable, and your TCC grant survives. Net effect: rebuilds without source changes never ask you to re-grant.
  - **Cleaner re-sign output.** When source *did* change, the script now prints both old and new SHA-256 prefixes plus the resulting cdhash, with an explicit "TCC grant must be re-given" warning so you know what to expect.
- **Wrong entitlement removed.** `com.apple.security.automation.apple-events` was misleadingly attached "for enigo to simulate paste" but actually covers AppleScript automation (NSAppleEvent / OSAScript), not `CGEventPost`-style synthetic input. Worse, on macOS Tahoe with Hardened Runtime its presence can trigger an unrelated TCC "Automation" prompt and confuse the permission flow. Removed from `macos/src-tauri/entitlements.plist`. The remaining three entitlements (`allow-jit`, `allow-unsigned-executable-memory`, `disable-library-validation`) correctly cover WebKit / Tauri plugin loading.

### Added

- **Auto-restart prompt after grant detected.** The Settings panel's polling loop now distinguishes the false→true transition of `accessibility_granted`. When it fires, an inline emerald-bordered prompt appears: **"Access detected — one more step"** with a **Restart now** button. Click → Inspector Rust spawns a fresh `/Applications/InspectorRust.app` process via `open -n` and exits cleanly. The new instance picks up the just-granted TCC state correctly (the running process couldn't, because macOS caches `AXIsProcessTrusted()` per-process). Total post-grant flow: ~30 seconds, one click. — *#feat(settings)*
  - New `relaunch_app` IPC command in `core/rust-lib/src/commands.rs`.
  - `relaunchApp()` wrapper in `core/frontend/src/lib/ipc.ts`.
- **"Why does this keep happening?" disclosure** in the amber banner of the Settings panel, explaining the cdhash binding in plain language so users understand the constraint instead of feeling gaslit by the OS.

### Changed

- **`[profile.release]`** at the workspace root: `codegen-units = 1`, `lto = true`, `strip = "debuginfo"`, `opt-level = 3`. Won't make Rust release builds fully byte-reproducible, but reduces non-determinism so the SHA-256 idempotency check has a fighting chance for trivial source changes.
- **`scripts/install-macos.sh`** — full restructure with helper functions (`bin_sha256`, `cdhash`, `current_identifier`, `kill_running`, `resign_app`, `reset_tcc`) and clearer printed status. The script's docstring at the top now accurately describes the cdhash binding and how the idempotent path works.
- **`macos/README.md`** "Why the dialog re-appears" section rewritten with the honest truth instead of the previous wishful "Sequoia and earlier accept this; later releases may still re-prompt." Now says: every meaningful rebuild requires re-grant on Tahoe; the script + auto-restart prompt make it bearable; the only permanent fix is an Apple Developer ID.

### Verification recipe

```bash
# 1) idempotent rebuild preserves grant
bash scripts/install-macos.sh        # initial install
# … grant Accessibility once via Settings panel banner …
bash scripts/install-macos.sh        # re-run with no source changes
#   ⇒ prints "Binary unchanged — keeping existing install"
#   ⇒ green banner stays green; Diagnose works without intervention

# 2) source change triggers single re-grant
echo "// touch" >> core/rust-lib/src/lib.rs
bash scripts/install-macos.sh
#   ⇒ prints "Binary changed — full reinstall"
#   ⇒ amber banner appears in Settings tab
#   ⇒ click Open System Settings → enable toggle → switch back
#   ⇒ green "Restart now" prompt appears within 1 s
#   ⇒ one click → app relaunches → Diagnose works
```

## [0.2.9] — 2026-04-26

### Added

- **Accessibility status badge in the Settings panel** — green when Inspector Rust has macOS Accessibility access, amber when it doesn't, with an inline explainer of what to do. Polled once per second while not granted, so the badge flips to green within ~1 s of the user toggling Inspector Rust on in System Settings — no panel reload needed. — *#feat(settings)*
- **`Test now` button** in the Settings panel — runs the full expand-at-cursor cycle without using the hotkey after a 2-second grace period (long enough to switch back to the source app and place the cursor after an abbreviation). Lets you tell whether the *hotkey* is the problem or the *expansion logic* is. Wired through the existing `trigger_expand_at_cursor` IPC.
- **`get_accessibility_status` Tauri command** + `ExpanderConfig.accessibility_granted` field — backed by macOS `AXIsProcessTrusted()` via FFI to `ApplicationServices.framework`. Returns `true` unconditionally on Windows / Linux, where synthetic input is either ungated or gated by a different mechanism.

### Fixed

- **`scripts/install-macos.sh`** — new helper that builds + re-signs Inspector Rust with a stable ad-hoc identifier (`io.celox.inspector-rust`) before copying into `/Applications`. Without an Apple Developer ID, every fresh `pnpm build:macos` produced a *random* identifier (e.g. `inspector-rust-c64f925d…`); macOS TCC then treated the rebuild as a brand-new app and discarded the previous Accessibility grant. The script's stable identifier lets the grant survive across rebuilds (where macOS allows bundle-id matching), and `--reset` runs `tccutil reset` to wipe stale carcass entries when needed.
- **macOS README** — new "Why the dialog re-appears after every rebuild" section explaining TCC binding to code-signature, plus how to use `install-macos.sh`.

## [0.2.8] — 2026-04-26

### Fixed

- **Expander hotkey capture failed for the `^` key on German ISO macOS keyboards.** WebKit reports the top-left key (`^`/`°`) as `event.code = "IntlBackslash"`, but the Tauri `tauri-plugin-global-shortcut` parser (`Shortcut::from_str`) maintains a hand-written allow-list that doesn't include any `Intl…` codes — the captured combo `Alt+IntlBackslash` was rejected with `UnsupportedKey("IntlBackslash")`. Two-part fix: — *#fix(expander)*
  - **Frontend** (`HotkeyCapture.tsx`) — new `normalizeCode()` maps WebKit's `IntlBackslash` back to `Backquote` (the layout-stable W3C name; same Carbon virtual keycode `kVK_ANSI_Grave` = 0x32 the OS will see at hotkey time).
  - **Backend** (`hotkey::parse_shortcut`) — replaces the plugin's narrow parser with our own. Routes the code token through `keyboard_types::Code::from_str`, which understands the **full** W3C `KeyboardEvent.code` spec. Future-proofs against other gaps in the plugin's allow-list (`IntlBackquote`, `IntlRo`, `IntlYen`, less-common media keys, …).
  - 9 new unit tests for the parser (modifier aliases, `IntlBackslash` accept, single-key, error cases). Backend tests: 62 → **71 green**.
- **HotkeyCapture button never recorded on macOS.** Safari/WebKit does **not** focus a `<button>` on click, so the button-level `onKeyDown` never fired. The capture indicator stayed at "Press a key combination…" forever. Fix: while capturing, attach a window-level keydown listener in *capture phase* — wins over the global keyboard-nav hook (which would otherwise consume Esc as "close popup"). — *#fix(settings)*
- **Search bar placeholder + Notes/Snippets/Settings titles ran behind the absolutely-positioned tab strip.** With four tabs (after Settings was added in 0.2.7) the strip overlapped the input. Fix: reserve `pr-[260px]` on the search bar and on the inactive-tab title row, tighten tab buttons to `px-2 whitespace-nowrap`, shorten the placeholder to `Search or calculate…`. — *#fix(ui)*

### Added

- **Per-row delete + Clear all** for clipboard history. Hover any clip row in the History tab → trash icon appears next to the bookmark icon → one click removes that single entry. A new toolbar at the top of the history list shows the clip count and a **Clear all** button (with `window.confirm` guard) for nuking everything at once. Wired through the existing `delete_entry` / `clear_history` IPC commands. — *#feat(history)*

### Changed

- `useClipboardHistory` now exposes its `refresh` callback to `App.tsx` so the list refetches immediately after delete/clear-all instead of waiting for the next `clipboard-changed` event.

## [0.2.7] — 2026-04-25

### Added

- **System-wide text expander.** Type a snippet abbreviation in any text field — code editor, browser, mail client, Slack — then press the configured hotkey, and Inspector Rust replaces the abbreviation in place with the snippet body. Default hotkey is `Alt+Backquote` (the `^` key on a German keyboard, ` on US). Disabled by default — opt in from the new **Settings** tab. — *#feat(expander)*
  - **How it works:** the popup stays out of the way. Inspector Rust synthesizes `Cmd/Ctrl+Shift+←` (select previous word) → `Cmd/Ctrl+C` (copy), looks the captured word up in the snippets table via the new `find_by_exact_abbreviation` (case-sensitive first, case-insensitive fallback), writes the body to the clipboard, and synthesizes `Cmd/Ctrl+V`. The user's clipboard is saved before the cycle and restored after.
  - **Trigger semantics, not silent watch.** No global keylogger — you decide when to expand.
  - **Configurable hotkey.** New **Settings** tab → click the hotkey field → press your combination (Backspace clears, Esc cancels). The string is stored in the new `settings` SQLite table and re-registered with the OS via `tauri-plugin-global-shortcut`. Bad combinations are rejected before the previous registration is touched, so you can't accidentally lose your hotkey to a typo.
  - **Cross-platform.** macOS / Windows / Linux X11 work the same. Linux Wayland depends on the compositor's global-shortcut portal (GNOME/KDE OK; sway-flavoured stacks may not).
  - Full reference: [`docs/text-expander.md`](./docs/text-expander.md).
- **Settings tab** in the popup, alongside History · Snippets · Notes. Designed to grow — first home for the expander toggle + hotkey capture; future settings (capture pause defaults, image-size cap, …) will land here.
- **`settings` SQLite table** — new key/value store via `core/rust-lib/src/settings.rs`. Idempotent migration; created on first launch of v0.2.7.
- **`HotkeyCapture` React component** that converts a `KeyboardEvent` into the W3C-code shortcut format the global-shortcut plugin's parser expects (`Modifier+...+Code`).
- **14 new Rust unit tests** — settings store roundtrip (6), `snippets::find_by_exact_abbreviation` semantics (5), expander helpers (3). `cargo test --workspace`: 48 → **62**.

### Changed

- IPC surface gains `get_expander_config`, `set_expander_config`, `trigger_expand_at_cursor`. The latter is a programmatic alternative to the hotkey — useful for testing and for any future tray-menu entry.
- `hotkey.rs` gains `ExpanderShortcutState` (Tauri-managed) and `register_expander(...)`, which idempotently swaps the previously-registered expander shortcut. Runs the actual expansion on a worker thread so the global-shortcut callback returns instantly (avoids platform-specific deadlocks).

### Caveats — what *won't* work cleanly

These are documented in [`docs/text-expander.md`](./docs/text-expander.md), surfaced in the Settings panel's "How it works" disclosure:

- **Terminals** (iTerm2, kitty, gnome-terminal) sometimes interpret `Cmd/Ctrl+Shift+←` as a pane-switch / mark-selection — the expander may grab the wrong "word" or nothing at all.
- **Password fields** in many apps refuse synthetic paste; the abbreviation gets selected but the body never lands.
- **Linux Wayland** in restrictive compositors blocks global shortcuts entirely.
- **Image / files snippets** are not supported by the expander (the orchestration only handles text). This is intentional for v1.

## [0.2.6] — 2026-04-25

### Added

- **Notes — a third tab for persistent, categorized clipboard items.** Notes live in their own SQLite table and are *not* affected by the 1 000-entry pruning of the clipboard history, so they're the right place for things you want to keep. — *#feat(notes)*
  - Three-pane layout: **Categories sidebar** (with note counts per category, plus virtual `All` and `Uncategorized` groups), **note list**, and **detail/edit pane**.
  - **Free-form categories** — typing a new category name in the edit form auto-creates it; the input has a `<datalist>` for autocomplete from existing categories.
  - **Editable bodies** for `text`, `html`, `rtf` notes; `image` and `files` notes are read-only (you can still rename them and change category). The detail pane renders images inline and shows file paths as a list.
  - **Paste from a note** preserves the original content type — image notes paste as images, HTML notes paste as HTML, etc.
- **Star button on history rows** — hover any clipboard entry in the History tab and the bookmark icon appears next to the timestamp; one click promotes the entry to a note in the `Uncategorized` bucket. The note is decoupled from the clip thereafter, so even if the clip gets pruned out of history, the note stays.
- **Full-app backup** — Notes tab toolbar gets `Export…` and `Import…` actions wired through `tauri-plugin-dialog`. Export writes a single pretty-printed JSON file (`{ version, exported_at, history, snippets, notes }`); import merges that file back into the live database with sensible per-table semantics:
  - **Snippets** — upsert by `abbreviation` (existing rows are overwritten).
  - **History** — upsert by SHA-256 hash; duplicates just bump `last_used_at`, new rows respect the existing 1 000-entry cap.
  - **Notes** — appended verbatim with original timestamps preserved (no natural dedup key, so re-importing the same backup creates duplicates — use Clear All first if you want a clean replace).
- **`Clear All` for notes**, with a `window.confirm` guard.
- **Tray menu entry “Manage Notes”** — opens the popup directly on the Notes tab via a new `open-notes-tab` event.
- **15 new Rust unit tests** for the notes module (CRUD, categories, save_from_clip, image-note read-only update) and the backup module (roundtrip into empty db, merge into populated db, version-rejection guard, replace-all). `cargo test --workspace` is now **48 → was 33**.

### Changed

- `paste.rs::write_to_clipboard` was refactored to take primitives `(content_type, data, text)` instead of a `&ClipEntry`, exposed via the new public `paste::paste_payload(...)`. This lets the `paste_note` IPC command paste any content type without needing to construct a fake `ClipEntry`.
- New IPC commands wired into `invoke_handler`: `list_notes`, `list_note_categories`, `save_clip_as_note`, `create_note`, `update_note`, `delete_note`, `clear_notes`, `paste_note`, `export_backup`, `save_backup_to_file`, `import_backup`.
- New permissions in both shells' `capabilities/default.json`: `dialog:allow-save` (for the export file picker).

### Database

- New table on first launch (idempotent `CREATE TABLE IF NOT EXISTS`):
  ```sql
  CREATE TABLE notes (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      content_type TEXT NOT NULL,
      content_text TEXT NOT NULL DEFAULT '',
      content_data TEXT NOT NULL DEFAULT '',
      title        TEXT NOT NULL DEFAULT '',
      category     TEXT NOT NULL DEFAULT '',
      byte_size    INTEGER NOT NULL DEFAULT 0,
      created_at   INTEGER NOT NULL,
      updated_at   INTEGER NOT NULL
  );
  ```
  Indexed on `category` and `updated_at DESC`.

## [0.2.5] — 2026-04-25

### Added

- **Inline calculator in the search field** — Alfred-style. As you type, Inspector Rust evaluates the input as a math expression and shows the result as the top list item; press Enter to paste the result into the previously active app. Bare numbers (`42`) and plain text (`hello`) are ignored; only inputs with at least one operator, function call, or named constant trigger calc mode. A leading `=` forces evaluation (so `=42` or `=pi` displays a result for a single literal). — *#feat(calc)*
  - Supported operators: `+ - * / % ^` (power is right-associative), unary `+`/`-`, parens.
  - Supported numbers: integers, decimals (`0.5`, `.5`), scientific (`1e3`, `1.5e-2`), digit grouping (`1_000`).
  - Constants: `pi` / `π`, `tau`, `e`.
  - Functions: `sqrt`, `cbrt`, `abs`, `sign`, `floor`, `ceil`, `round`, `ln`, `log` (base 10), `log2`, `exp`, `sin`/`cos`/`tan` (radians), `asin`/`acos`/`atan`/`atan2`, `sinh`/`cosh`/`tanh`, `min`, `max`, `pow`, `mod`.
- **`paste_text(text)` Tauri command** — generic "compute & paste" entry point used by the calculator (and available for future flows like unit-conversion / date-math). Hides the popup, writes `text` to the clipboard, and synthesizes Cmd+V / Ctrl+V via `enigo`, same as the existing snippet-paste path.
- **27 new vitest cases** for `tryEvaluate` and `formatResult` covering precedence, right-associative power, parens, decimals + scientific notation, every supported function/constant, `=`-forced evaluation, and rejection of plain numbers / malformed input. (`pnpm test`: 24 → 51 frontend tests.)

### Changed

- **Search field rebranded as a general input.** Placeholder is now `Search history or type an expression (2+2, sqrt(16), …)`. The leading icon is a chevron by default and switches to a calculator glyph the moment the input parses as a math expression — making the field read as an entry box, not just a search box.
- New `CalcEntry` variant in `ListEntry`; `HistoryItem` renders calc rows with a `calc` chip and `expr = result` formatting in monospace, `PreviewPanel` shows a centered large `= result` view.

## [0.2.4] — 2026-04-25

### Fixed

- **Paste did not land in the previously active app on macOS.** Hiding only the popup window left Inspector Rust (an `Accessory`-policy app) in a state where the OS could not reliably hand key focus back to the prior frontmost app, so `enigo`'s synthesized `Cmd+V` either dropped on the floor or arrived back at Inspector Rust. — *#fix(paste)*

### Changed

- `hotkey::hide_popup` now also calls `AppHandle::hide()` on macOS (no-op on other platforms), which invokes `NSApplication.hide(nil)` and forces the OS to restore the prior frontmost app as key window. The popup window is hidden first, then the app.
- The settle delay between clipboard write and the synthesized paste keystroke is now platform-specific: **120 ms on macOS** (was 50 ms — `NSApp.hide()` takes a frame or two), unchanged 50 ms on Windows / Linux.

## [0.2.3] — 2026-04-25

### Fixed

- **Import button appeared to crash the app on macOS.** When the native file dialog (`NSOpenPanel`) opened, the popup window lost focus, which fired our existing `Focused(false)` window event → `hide_popup()` ran → the popup vanished. The dialog often stayed half-up but with its parent gone, the user perceived the whole app as having crashed. — *#fix(snippets)*

### Added

- New `UiState { suppress_hide: AtomicBool }` shared state and IPC command `set_suppress_hide(suppress: bool)`. The Snippets-tab Import handler now wraps the `dialog.open()` call in `setSuppressHide(true) … finally setSuppressHide(false)` so the popup stays put while NSOpenPanel owns focus.
- `core/rust-lib/src/ui_state.rs` — new module owning the shared UI flag.

### Changed

- The popup's `Focused(false)` handler in `lib.rs` consults the suppress flag before calling `hide_popup`. Default behaviour (auto-hide on click-outside, Esc, alt-tab) is unchanged.

## [0.2.2] — 2026-04-25

### Fixed

- **JSON snippet import was broken on macOS.** The 0.2.1 implementation used a hidden `<input type="file">` triggered by `.click()` from React. WKWebView (Tauri's macOS renderer) does not reliably surface a native file picker for hidden inputs in this pattern, so the Import button appeared to do nothing on macOS. — *#fix(snippets)*

### Changed

- **Switched the snippet-import file picker to `tauri-plugin-dialog`.** The Import button now opens the native NSOpenPanel / Win32 OpenFileDialog via `@tauri-apps/plugin-dialog`'s `open()`, with a `.json` filter and a localized "Select snippets JSON file" title. Selected path is read in Rust (`std::fs::read_to_string`) and parsed by the existing `import_from_json` pipeline.

### Added

- New IPC command `import_snippets_from_file(path: String) -> ImportResult` (in addition to the existing `import_snippets(json: String)` which is still used by tests).
- `tauri-plugin-dialog` workspace dep + capability permission `dialog:allow-open` in both the Windows and macOS shells.
- Import button shows "Importing…" while the dialog/import is in flight.
- **5 themed example JSON files** under `docs/examples/snippets/` — `getting-started.json` (3 entries), `signatures.json` (4), `dev.json` (8), `markdown.json` (5), `wrapped-form.json` (2, demonstrates the `{ snippets: [...] }` shape). Each is a stand-alone, ready-to-import file; the folder has its own `README.md` indexing them and showing how to merge multiple files via `jq -s 'add'`.
- `docs/snippets-import.md` extended with a Tips & anti-patterns section.
- Root `README.md` Snippet-import section now lists all example files in a table instead of a placeholder code block.

## [0.2.1] — 2026-04-25

### Added

- **JSON snippet import** — bulk-load snippets from a `.json` file via **Snippets → Import** in the popup. Existing abbreviations are upserted in place, so re-importing the same file is idempotent. Both `[…]` (bare array) and `{ "snippets": [...] }` (wrapped) shapes are accepted; per-row failures are collected in the result without aborting the whole import. See [`docs/snippets-import.md`](./docs/snippets-import.md) for the schema and [`docs/snippets-example.json`](./docs/snippets-example.json) for a sample. — *#feat(snippets)*
- **`macos/README.md`** with installation, Gatekeeper bypass, Accessibility-permission setup, and troubleshooting (DMG bundle failures, missing tray icon).
- **`docs/snippets-import.md`** — full reference: file format, field semantics, sample-file walkthrough, manual export recipe via `sqlite3` + `jq`, IPC surface, test matrix.
- **`CHANGELOG.md`** (this file).
- **6 new Rust unit tests** for the snippet import path (`cargo test --workspace`: 27 → 33).

### Fixed

- **CI was failing** with `ERR_PNPM_OUTDATED_LOCKFILE` because `macos/package.json` (added in 0.2.0) declared `@tauri-apps/cli` without a lockfile refresh. The lockfile is now in sync. — *#fix(ci)*
- **macOS build was broken** in 0.2.0:
  - `tauri.conf.json` declared `macOSPrivateApi: true` but the corresponding `tauri/macos-private-api` cargo feature was not enabled — `tauri-build` aborted. — *#fix(build)*
  - `app.set_activation_policy(...)` was wrapped in `if let Err(e) = …`, but the function returns `()`, not `Result`. The whole crate failed to typecheck on macOS. — *#fix(build)*
- **Multi-monitor popup placement** — the popup occasionally opened in the bottom-right of the active monitor and could even extend past the screen edge, most reliably reproducible on mixed-DPI setups (MacBook Retina + external display). The show/position pipeline was restructured: pick cursor monitor first, park the hidden window onto it, **then** `show()` + `set_focus()` (so `outer_size()` returns a real value), then re-resolve the monitor and finally call new helper `clamp_into_monitor()` which hard-clamps `x`/`y` to the monitor's bounds so the window can never overflow. — *#fix(hotkey)*

### Changed

- **`README.md`** — added a Multi-monitor placement subsection, surfaced the JSON-import feature, refreshed the repo layout to include `macos/` and the new docs, bumped test counts (24 frontend, 33 Rust).
- **`.gitignore`** — ignore `.claude/` (per-machine agent session state).

### Known issues

- The macOS DMG bundling step (`bundle_dmg.sh`) occasionally fails on busy disks (FileVault background indexing, Time Machine snapshot in progress). The `.app` itself is built first and is unaffected — see [`macos/README.md` § Troubleshooting](./macos/README.md#troubleshooting).
- macOS builds are **arm64 only** (Apple Silicon). Intel-Mac users need to build from source with `--target x86_64-apple-darwin`.
- Bundles are **not Apple-signed** — Gatekeeper will refuse to open on first launch. Workarounds documented in `macos/README.md`.

## [0.2.0] — 2026-04-24

### Added

- **macOS bundle shell** under [`macos/`](./macos) — DMG + `.app` targets, `entitlements.plist`, capabilities, thin `main.rs` reusing `inspector-rust-core`.
- **Text expander** ("snippets") — abbreviations (e.g. `mfg`) with optional title and body. Matching snippets appear at the top of the History list when you type their abbreviation; Enter pastes the body. Dedicated **Snippets** tab for create/edit/delete, **Manage Snippets** entry in the tray menu.
- **GitHub Actions CI** — Rust + frontend tests on every push/PR ([`ci.yml`](./.github/workflows/ci.yml)).
- **GitHub Actions release** — builds Windows MSI/EXE and publishes a GitHub Release on `v*` tags ([`release.yml`](./.github/workflows/release.yml)).
- **Frontend unit tests** — vitest + happy-dom + @testing-library/react (`Footer`, `format` helpers — 24 tests).
- **Rust unit tests** — in-memory SQLite tests for `db` (insert/dedupe/list/touch/prune — 27 tests).
- README badges, icon header, polished layout.

### Known issues (resolved in 0.2.1)

- macOS build broken (`macos-private-api` cargo feature missing, `set_activation_policy` type mismatch). Fixed in 0.2.1.
- CI failing due to stale `pnpm-lock.yaml`. Fixed in 0.2.1.

## [0.1.0] — 2026-04-23

### Added

- Initial release. Windows-first clipboard history manager.
- Global hotkey `Ctrl+Shift+V` opens a frameless, always-on-top popup centered on the cursor's monitor.
- Captures **text**, **RTF**, **HTML**, **images** (≤ 5 MB, base64 PNG), and **file lists** via real OS clipboard change events (no polling).
- Fuzzy search (`fuse.js`, threshold 0.4) over preview text.
- Auto-paste with `enigo` (simulates `Ctrl+V` after the popup hides).
- SQLite history at `%APPDATA%\InspectorRust\history.db`, deduped on SHA-256, capped at 1 000 entries.
- System tray menu: Open · Pause Capture · Clear History · Start with Windows · Quit.
- pnpm + Cargo workspaces with shared [`core/`](./core) and [`win/`](./win) bundle shell.

[0.5.1]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.5.1
[0.5.0]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.5.0
[0.4.2]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.4.2
[0.4.1]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.4.1
[0.4.0]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.4.0
[0.3.1]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.3.1
[0.3.0]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.3.0
[0.2.12]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.12
[0.2.11]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.11
[0.2.10]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.10
[0.2.9]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.9
[0.2.8]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.8
[0.2.7]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.7
[0.2.6]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.6
[0.2.5]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.5
[0.2.4]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.4
[0.2.3]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.3
[0.2.2]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.2
[0.2.1]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.1
[0.2.0]: https://github.com/pepperonas/inspector-rust/releases/tag/v0.2.0
[0.1.0]: https://github.com/pepperonas/inspector-rust/commits/main
