# Touchpad gestures — guard against accidental gestures

Inspector Rust recognises its own trackpad gestures. Out of the box a 3-finger swipe changes the volume and a 3-finger tap toggles mute; since v0.193.0 every gesture can be bound to an action of your choice (see *Bindings* below). This page covers how the app keeps these gestures from firing when you didn't mean them to: a palm while typing, a resting thumb, a brush along the edge.

**Scope.** Only Inspector Rust's own gestures are filtered. macOS system gestures, pointer movement and tap-to-click stay untouched — BetterTouchTool has the same limit.

> Status: **complete (v0.190.0).** The four filter levels run as one pipeline (`core/rust-lib/src/gestures/guard.rs`). macOS and Windows feed it per-contact data; the typing level gets every key press from the app's keyboard monitor. Linux passes libinput's gestures through levels 3 and 4 with libinput's own palm and typing detection switched on (see *Platforms*). The `gestures` panel shows the trackpad live, holds the sliders, lists the last 20 decisions, runs the guided calibration and manages the recordings; the hotkey ⌃⇧⌥G saves the last 3 seconds when a gesture fired by itself.

## Bindings (v0.193.0)

Settings → Touchpad gestures holds a table: gesture → action, each row can be switched off, edited or deleted, and new rows are added with **Neue Zuordnung**. The principle is BetterTouchTool's — pick a gesture, pick what it does — nothing more of it is copied.

| | Options |
|---|---|
| Gestures | swipe up / down / left / right and tap, each with 3, 4 or 5 fingers; tip-tap left / right (one finger rests, a second taps next to it) |
| Actions | volume up / down, mute toggle, next / previous tab, send a key shortcut, run an Inspector Rust action (the ones under Global shortcuts), open a URL (http, https, mailto) or an absolute path, run an approved AI task |
| Scope | everywhere, or only while one app is in front — an app binding beats the global one for the same gesture |
| Typing guard | per binding (level 3 below; tab switching is exempt by default) |

**Matching** (`gestures/bindings.rs`, pure): a swipe needs exactly its finger count (a 2-finger scroll fires nothing); a tap fires the binding with the largest count not above the recognised one — a sloppy 3-finger tap read as 4 still fires the 3-finger binding when no 4-finger binding exists, the historic rule; tip-taps ignore the count. App-specific beats global, then the larger count, then list order. Two **active** bindings on one gesture in one scope are refused when saving; a switched-off twin is allowed.

**Showing the gesture.** „Geste vormachen" in the editor takes kind and finger count from the next gesture in the live log; while it waits (at most 15 s, `CAPTURE_MAX_MS`) recognised gestures are logged but not performed, so demonstrating the mute tap doesn't mute.

**Migration.** Until a list is saved, the bindings are derived from the old switches `gestures.volume`, `gestures.mute`, `gestures.tiptap` — an existing install behaves exactly as before (`bindings::legacy`; all golden fixtures run through it). The first save writes `gestures.bindings` (JSON); from then on the switches are ignored. **Standard wiederherstellen** deletes the saved list. An unreadable saved list falls back to the derived set and is logged — gestures never go silent because of a bad value.

**Running.** The capture reads the live list in place (`bindings::live`), so a save takes effect with the next gesture, no restart. The frontmost app is only asked when some binding is app-specific (macOS: bundle id via NSWorkspace; elsewhere the app name). A rejected gesture names a switched-off binding as `config:binding is off`; an unbound one is `unmapped`. Key shortcuts are sent by physical key position (macOS virtual keycodes from the recorded W3C code; Windows/Linux through enigo, untested on hardware). Open targets with any other scheme, relative paths and a leading `-` are refused. A task binding only starts approved tasks — the scheduler refuses the rest, and a failure shows a toast.

**Not done:** per-app scope on Windows/Linux matches the app's name and is untested; macOS system gestures (System Settings → Trackpad → More Gestures) can claim the same gesture, then both fire — the editor says so for swipes and multi-finger taps.

## The pipeline

Each frame of touches runs through four levels. Every decision is reported with its level and reason; the app log shows each rejected gesture as `gesture rejected [<level>] <reason>`.

### Level 1: classification per contact

| Class | Rule (default) |
|---|---|
| Palm | size ≥ 2.0 (the historic rule), or ellipse major axis ≥ `palm_major` (off by default) |
| Thumb | major/minor ≥ 1.7 **and** size ≥ 1.3 **and** landed in the lower 40 % of the pad |
| Finger | none of the above, after 30 ms on the pad |
| Unclear | the first 30 ms; counts as a finger, so a quick tap isn't delayed |

Palm and thumb are **sticky**: they are judged on the largest size and ratio seen so far, so a contact whose area shrinks later doesn't turn back into a finger. Palms and thumbs never count as gesture fingers.

⚠️ The default thumb thresholds are estimates; `gestures calibrate` measures them on your trackpad (see [Calibration](#calibration-phase-5)). `thumb_ratio = 0` switches the thumb rule off.

### Level 2: space

- **Edge zones**: left 3 %, right 3 %, top 5 %, bottom 5 % by default. A contact that **lands** in a zone is ignored; a finger that slides into a zone from the middle stays valid.
- **Profiles**: separate zones for the built-in trackpad and an external one (Magic Trackpad). macOS tells the two apart through an optional private function; if it can't, the built-in profile applies.
- **Palm blocks everything** (off by default): while a palm lies on the pad, no gesture fires.

### Level 3: typing

A key press shortly before or during a touch blocks volume and mute.

| Situation | Window |
|---|---|
| A single key press | 250 ms |
| A key press within 500 ms of the previous one (a burst) | 600 ms |
| History unknown (keyboard tap not running, e.g. no Accessibility grant) | 600 ms |

Never blocked: **unmute**, tab switching, and a key pressed **after** the fingers lifted. Modifier keys never count, and neither do shortcuts (a key pressed with ⌘ or ⌃ is a command, not text). Option counts, because on many layouts it types characters.

**Where the key times come from (macOS).** The app's one keyboard tap — the text expander's monitor — reports every key press to the typing level: only **when** and **whether ⌘/⌃ was held**, never which key. When the typing guard is on, the tap runs even if the text expander is off; it needs the Accessibility grant. Without the tap, the typing level falls back to macOS's "seconds since the last key press", which can't tell a single press from a burst, so every press gets the 600 ms window. Synthetic keystrokes the app sends itself (expansions, tab switching) are ignored.

Optional **release by a centre touch**: after typing, gestures stay blocked until a touch starts in the centre area (by default the middle half of the pad).

### Level 4: plausibility

| Rule | Default |
|---|---|
| Swipe: the finger count stays constant; a contact that joins and stays still rejects it | on |
| Swipe: fingers move coherently in one direction | coherence ≥ 0.6 |
| Swipe: minimum travel per finger | 0.06 of the pad (early emission: 0.12) |
| Swipe: minimum speed and evenness of the fingers | off |
| Tap: all fingers within one window, held briefly, barely moving | 700 ms / 350 ms / 0.12 |
| Cooldown after every accepted gesture | 150 ms |

All thresholds are stored as JSON under `gestures.guard` in the settings table. They are clamped on load, so a hand-edited or old value can't break recognition.

### Intentional changes from Phase 1

The golden fixtures pin exactly these four changes; every other scenario behaves as before.

| Scenario | Before | Now |
|---|---|---|
| `edge-stripe-top` | volume down | nothing (edge zone) |
| `swipe-plus-fourth-contact` | volume up | rejected: finger count changed |
| `tap-landing-while-typing` (single key 0.49 s before) | blocked | fires (a single key blocks for 250 ms) |
| `thumb-plus-two-finger-tap` (new) | would have muted | 2-finger tap, ignored |

Performance: 0.5 µs per frame in a release build (budget 0.2 ms), measured by `guard_frame_budget`.

## The `gestures` panel (Phase 4)

Type `gestures` (or `gesten`). The panel appears while the command is typed.

**Live trackpad (macOS).** Every contact is drawn as an ellipse, using the size and angle the trackpad reports:

| Colour | Class |
|---|---|
| blue | finger |
| grey | not yet decided (the first 30 ms) |
| amber | thumb |
| red | palm |

A contact that **landed** in an edge zone is drawn dashed and hollow; the zones themselves are shaded. With "release by a centre touch" on, the centre area is outlined. A badge appears while a recent key press would block volume and mute. The pad keeps the real proportions of the device (157.8 × 97.8 mm on the built-in MacBook trackpad, reported by the driver).

**Sliders.** Edge zones per device profile (built-in / external — "aktiv" marks the one in use), palm size, thumb ratio, settle time, both typing windows, cooldown, minimum swipe travel, maximum tap hold, and three switches (palm blocks everything, release by a centre touch, constant finger count). They are saved 250 ms after you stop dragging. On macOS they reach the running pipeline **in place** — no restart of the capture; the recognisers are rebuilt with the new limits and contacts still down are classified again. A reset button appears once anything differs from the defaults (taken from the Rust `Default`, not a copy). Each slider's range lies inside the backend's clamp, so the panel can't produce a value that would be silently corrected.

**Decision log.** The last 20 decisions, newest first: what was recognised, whether it fired, and if not, which level stopped it (hover the verdict for the level). Contacts excluded as palm, thumb or edge contact appear too. The trash button empties it.

**`gestures on` / `gestures off`** switches the gestures themselves (Enter). Anything else after `gestures` is rejected rather than read as a toggle.

**Cost.** The panel polls about 30 times a second while it is visible and stops when the popup hides. The per-frame snapshot is only taken while the panel polls; with it closed a frame pays one atomic read. Measured with the snapshot on every frame: 0.6 µs per frame in a release build (`guard_frame_budget`). The log keeps decisions in memory only; nothing is written or sent.

**Linux** shows the sliders and the log, but no live trackpad, recording or calibration: libinput hands over finished gestures, not contacts. Windows has all of it.

## Calibration (Phase 5)

`gestures calibrate` + Enter, or **Kalibrieren** in the panel. After a 3-second countdown three steps of 6 seconds each follow; the first second of every step is not measured, the hands are still moving there.

| Step | What to do | What it measures |
|---|---|---|
| 1 Handballen | Both hands on the keyboard as when typing, palm heels on the trackpad | contacts resting ≥ 400 ms → palm size |
| 2 Daumen | Hands off, only the thumb resting low on the pad | contacts resting ≥ 400 ms → thumb size, shape (major/minor axis) and landing height |
| 3 Finger | Thumb off, tap and swipe with three fingers several times | contacts shorter than 1.5 s → finger size and shape (a palm left on the pad is longer and drops out) |

From each class the 10th and 90th percentile are taken (nearest rank). A threshold is placed halfway between what the fingers reached (90 %) and where the other class starts (10 %), on the slider's grid and never at or below the fingers. Two classes count as separable only with a 10 % margin; if palms and fingers are closer than that, `palm_size` keeps its value and the proposal says why. For the thumb, the rule needs shape **and** size **and** a low landing: whichever of shape and size separates is placed between the classes, the other is lowered just under the thumb so it can't block it; the thumb zone becomes the thumb's landing height minus 5 %. Step 3 needs at least 6 finger contacts — without fingers there is nothing to separate from, so that is an error, not a guess.

The proposal lists only the values that change (current → proposed) and the warnings. **Nothing is saved until you press *Übernehmen***; it is then applied onto the current thresholds, so a slider moved during the calibration keeps its value. The device that saw most of the calibration is the one measured; the classification thresholds apply to every device (only the edge zones are per profile). The recording behind a calibration stays in memory and is never written to disk. The calculation is pure (`gestures/calibrate.rs`) and tested on synthetic sessions, including that the proposal classifies the session it came from.

## Volume overlay (v0.198.0)

A volume or mute gesture shows the volume overlay. It stays **2.2 s** after the last gesture (was 1.1 s) and **never closes while the mouse pointer rests on it**; after the pointer leaves it stays 3 more seconds. Clicking or dragging the bar sets the volume (v0.197.0). The hover state comes from the Rust mouse gate (`status-toast-hover` event), not from DOM `pointerleave`, which the webview can miss once the window turns click-through again.

## Self-healing capture (macOS)

The private MultitouchSupport registration can go deaf — after sleep, a display change, or with no visible cause. A watchdog thread (`ir-gestures-wake`, every 15 s) rebuilds the capture when:

- **(v0.198.1)** macOS reports the screens waking, the system waking, or a display change (`display_wake.rs`, 2 s after the last notification of a burst) — a display-only sleep never moves the clocks, so the check below can't see it, or
- the system slept (wall clock jumped past the monotonic clock), or
- a **trackpad** scroll (non-zero scroll phase) produced no touch frames, or
- **(v0.197.1)** the HID system saw a scroll that the app's own scroll tap missed while no touch frames arrive. Needed because the trackpad-scroll proof comes from that tap: when tap and touch registration die together, the proof never arrives. A wheel mouse can't trigger this — a working tap sees its scrolls too.

Rebuilds back off from 60 s to 15 min while frames stay away; every rebuild logs one line (`rebuilding the touch capture`).

## What existed before the pipeline (macOS)

| Filter | What it does |
|---|---|
| Palm size | A contact with `size ≥ 2.0` is a palm and stays one until it lifts. |
| Resting | A contact that has barely moved for 600 ms doesn't count as a gesture finger and blocks no gesture of the other fingers. |
| Swipe | At least two fingers have to move in one direction (coherence ≥ 0.6). Decisive 3-finger swipes fire mid-motion. |
| Tap | Contacts that come in one after another within 700 ms count as **one** tap; a light 3-finger tap often arrives in exactly that form. |
| Tip-tap | The resting finger has to be still beforehand, and neither finger may travel during the gesture. |
| Typing guard | Volume and mute are blocked if a real key was pressed within 0.5 s before the fingers landed or while they were down. A key pressed after the lift doesn't count. **Unmuting is never blocked.** Modifier keys never count. |

## Recording and replay

**Record:** `gestures record` + Enter, or **30 s aufnehmen** in the panel. For the next 30 seconds every trackpad frame is recorded and then saved as a JSON file:

```
~/Library/Application Support/InspectorRust/gesture-traces/trace-YYYYMMDD-HHMMSS.json
```

**"That was unintended" (⌃⇧⌥G).** A gesture fired by itself: press the hotkey right away. The last 3 seconds of touch data are saved as `trace-unintended-YYYYMMDD-HHMMSS.json`, with `expect: []` (nothing should have fired) and a note naming what did fire in that window. A passive toast says whether something was saved. The 3-second buffer lives in memory only and only while the hotkey is bound; rebind or switch it off under Settings → Global shortcuts.

**List, replay, delete:** the panel lists every recording (kind *Aufnahme* or *Ungewollt*, time, size), newest first. ▶ plays one against the current settings and lists, per gesture, whether it fired, was blocked (and why) or was ignored — move a slider and play again to see whether it would have caught the misfire. 🗑 deletes one, *Alle löschen* all of them; both ask once more inline. Only file names of the form `trace-….json` are accepted, so a name from the UI can't reach outside the folder.

### Privacy

- A recording holds **touch data** (position, velocity, ellipse, size, state) and the **times** of key presses — **never which key**.
- The key times come from the app's one keyboard tap (the text expander's), or, without it, from macOS's "how long since the last key press?". No second keyboard event tap is installed.
- Files stay in the app data folder until you delete them in the panel. There is no network access.
- A calibration is never written to disk; the 3-second buffer of the hotkey is memory only.

### Trace format (version 1)

```json
{
  "version": 1,
  "source": "macos-multitouch",
  "note": "",
  "devices": [{ "builtin": true, "width_mm": 160.0, "height_mm": 100.0 }],
  "muted_at_start": false,
  "frames": [
    { "t_ms": 0, "device": 0, "touches": [
      { "id": 3, "x": 0.41, "y": 0.52, "vx": 0.0, "vy": -1.2,
        "major": 8.1, "minor": 6.3, "angle": 0.4, "size": 1.0, "phase": "touching" } ] }
  ],
  "keys": [{ "t_ms": 120, "modifier": false }],
  "expect": null
}
```

- `x`/`y` are normalised to 0..1, and **y grows downwards** ("up" = decreasing y).
- `phase` is one of `hover` / `make` / `touching` / `break` / `leaving`. Only `make`, `touching` and `break` count as contact.
- On macOS, `devices` comes from optional private functions. It stays `null` if the OS build doesn't offer them.
- A reader rejects a version newer than its own instead of misreading it.

### Replay as a test

`gestures/trace.rs::replay` runs a trace through the **same** recognisers and dispatch decisions as the live path (config, typing guard, unmute exception). It also emulates the 24 ms tick that finalises a deferred tap.

The split of each frame into the two recogniser inputs (`contact_feeds`) is code shared with the live path, not a copy.

Fixtures live in `core/rust-lib/src/gestures/fixtures/*.json`. Each one carries its expected outcome under `expect`, and `fixtures_replay_as_expected` checks it. To regenerate after changing a scenario:

```
cargo test -p inspector-rust-core --lib gen_gesture_fixtures -- --ignored
```

The synthetic scenarios record **today's** behaviour. Their `note` says where that behaviour is a known gap the next phases should close:

| Scenario | Today |
|---|---|
| `palm-typing-left` | Palm ignored, no gesture |
| `palm-typing-right` (heel as three small contacts) | Read as a 3-finger tap; only the typing guard blocks it |
| `thumb-rest-two-finger-scroll` | 2-finger swipe, ignored |
| `edge-stripe-top` | **Volume-down fires** — gap, planned: edge zones |
| `swipe-up-3` / `swipe-down-3` | Volume up / down |
| `tap-3` / `tap-3-sequential` | Mute toggle, exactly once |
| `swipe-plus-fourth-contact` | **Volume-up fires** — gap, planned: constant finger count |
| `tap-after-keypress` / `tap-landing-while-typing` | Blocked by the typing guard |
| `key-right-after-tap` | Fires (the key came after the lift) |
| `tap-after-modifier` | Fires (modifiers don't count) |
| `unmute-tap-while-typing` | Fires (unmuting is never blocked) |
| `tiptap-with-resting-palm` | Tab switch fires |

## Platforms (Phase 6)

| | macOS | Windows | Linux |
|---|---|---|---|
| Source | MultitouchSupport, per contact | Raw Input, Precision Touchpad HID, per contact | libinput, finished gestures |
| Level 1 | size, ellipse, landing height | the touchpad's **confidence bit** | libinput's palm detection (always on) |
| Level 2 | edge zones | edge zones | — |
| Level 3 | keyboard tap (expander) | keyboard hook (expander) | libinput key events + **disable-while-typing** |
| Level 4 | all rules | all rules | finger-count change (libinput cancels), cooldown |
| Live view, recording, calibration | yes | yes | no |

**Windows.** `windows.rs` reads each HID report per contact: contact id, tip switch, position, the confidence bit and, when the touchpad reports them, width and height. A Precision Touchpad clears the confidence bit for a contact it judges too large to be a fingertip; that contact becomes a palm until it lifts, even if a later report sets the bit again. Width and height become the contact ellipse in millimetres when the descriptor states physical units. A Precision Touchpad reports no contact size comparable to macOS, so the size and thumb rules stay inactive there — calibration still proposes values, they just have nothing to act on. Hybrid touchpads that spread one frame over several reports are reassembled first (`ptp.rs`, tested on every platform); a contact that disappears without a lift report gets one, so no ghost finger stays on the pad. Key times come from the low-level keyboard hook the text expander already installs: only the time, whether Ctrl or a Windows key was held (Ctrl+Alt counts as typing — that is AltGr on German layouts) and never which key; modifier presses don't count. Windows can't tell a built-in touchpad from an external one, so the built-in edge profile applies. **Not yet tested on a Windows machine** — it compiles against the `windows` crate and the parsing is unit-tested, like the rest of the repo's Windows code.

**Linux.** libinput recognises the gestures; the app can't see single contacts. In its own libinput context the app switches on **disable-while-typing** and **disable-while-trackpointing** for every touchpad that offers them — this affects the app's gesture events only, not the desktop's cursor or its own gestures. libinput's palm detection is always on. Key presses of the same context (time and whether Ctrl or Super was held, never the key) feed the typing level, and a swipe that libinput cancels because the finger count changed is logged as a level-4 rejection. Compiled against libinput on Debian (bookworm); not run on a real Linux desktop.
