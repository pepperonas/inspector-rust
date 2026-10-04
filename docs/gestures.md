# Touchpad gestures — guard against accidental gestures

Inspector Rust recognises its own trackpad gestures: a 3-finger swipe changes the volume, a 3-finger tap toggles mute, and a tip-tap switches tabs. This page covers how the app keeps these gestures from firing when you didn't mean them to: a palm while typing, a resting thumb, a brush along the edge.

**Scope.** Only Inspector Rust's own gestures are filtered. macOS system gestures, pointer movement and tap-to-click stay untouched — BetterTouchTool has the same limit.

> Status: **Phase 1 of 7.** In place so far are the touch model, the recording format, recording and replay, and fixed test scenarios. The four filter levels below are the plan for the next phases; today's filters are described under *What already exists*.

## What already exists (macOS)

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
- The key times come from macOS's question "how long since the last key press?". No second keyboard event tap is installed.
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

## Plan: four filter levels (Phases 2–7)

1. **Classification per contact** — finger / thumb / palm / unclear, sticky until the contact lifts, decided after a short settling time.
2. **Spatial** — edge zones (only new contacts are ignored there), separate profiles for the built-in trackpad and a Magic Trackpad, an optional block while a palm rests.
3. **Typing** — a short block after a single key press, a longer one after a run of key presses; optionally released only by a touch in the middle area.
4. **Plausibility** — constant finger count for swipes, coherence, minimum distance and speed, a tap window, a cooldown after every gesture.

Each decision is reported as an event (accepted / rejected, reason, level) and shown in the `gestures` panel.

**Limits by platform:**

- **Windows** (Precision Touchpad) gets the confidence bit and per-contact data only once the HID parser is rewritten.
- **Linux** hands libinput's finished gestures through; libinput already does its own palm detection internally. The app's own levels 1 and 2 would only be possible by reading the evdev devices directly, which is deliberately not done.
