# Touchpad gestures — guard against accidental gestures

Inspector Rust recognises its own trackpad gestures: a 3-finger swipe changes the volume, a 3-finger tap toggles mute, and a tip-tap switches tabs. This page covers how the app keeps these gestures from firing when you didn't mean them to: a palm while typing, a resting thumb, a brush along the edge.

**Scope.** Only Inspector Rust's own gestures are filtered. macOS system gestures, pointer movement and tap-to-click stay untouched — BetterTouchTool has the same limit.

> Status: **Phase 4 of 7.** The four filter levels run as one pipeline (`core/rust-lib/src/gestures/guard.rs`); on macOS the typing level gets every key press from the app's keyboard tap. Windows and Linux pass the gestures they recognise themselves through levels 3 and 4. The `gestures` panel shows the trackpad live, holds the sliders and lists the last 20 decisions. Calibration, recording from the panel and the "unintended" hotkey follow in Phase 5.

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

⚠️ The thumb thresholds are estimates until recordings from real trackpads exist. `thumb_ratio = 0` switches the thumb rule off.

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

**Windows and Linux** show the sliders and the log, but no live trackpad: those platforms hand over finished gestures, not contacts.

## What existed before the pipeline (macOS)

| Filter | What it does |
|---|---|
| Palm size | A contact with `size ≥ 2.0` is a palm and stays one until it lifts. |
| Resting | A contact that has barely moved for 600 ms doesn't count as a gesture finger and blocks no gesture of the other fingers. |
| Swipe | At least two fingers have to move in one direction (coherence ≥ 0.6). Decisive 3-finger swipes fire mid-motion. |
| Tap | Contacts that come in one after another within 700 ms count as **one** tap; a light 3-finger tap often arrives in exactly that form. |
| Tip-tap | The resting finger has to be still beforehand, and neither finger may travel during the gesture. |
| Typing guard | Volume and mute are blocked if a real key was pressed within 0.5 s before the fingers landed or while they were down. A key pressed after the lift doesn't count. **Unmuting is never blocked.** Modifier keys never count. |

## Recording and replay (Phase 1)

**Record:** Settings → Touchpad gestures → *Record a trace* → **Record 30 s**. For the next 30 seconds every trackpad frame is recorded and then saved as a JSON file:

```
~/Library/Application Support/InspectorRust/gesture-traces/trace-YYYYMMDD-HHMMSS.json
```

**Replay:** In the same section, *Replay* plays a saved recording against the current settings and lists, per gesture, whether it fired, was blocked (and why) or was ignored. That is the evidence for "that fired by itself".

### Privacy

- A recording holds **touch data** (position, velocity, ellipse, size, state) and the **times** of key presses — **never which key**.
- The key times come from the app's one keyboard tap (the text expander's), or, without it, from macOS's "how long since the last key press?". No second keyboard event tap is installed.
- Files stay in the app data folder. There is no network access.

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

## Next phases

5. Calibration, recording from the panel, a hotkey for "that was unintended".
6. Windows confidence bit, Linux palm data.
7. Docs and release.

**Limits by platform:**

- **Windows** (Precision Touchpad) gets the confidence bit and per-contact data only once the HID parser is rewritten.
- **Linux** hands libinput's finished gestures through; libinput already does its own palm detection internally. The app's own levels 1 and 2 would only be possible by reading the evdev devices directly, which is deliberately not done.
