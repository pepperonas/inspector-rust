//! Golden replay fixtures (gesture-guard Phase 1).
//!
//! Every file in `src/gestures/fixtures/*.json` is a [`Trace`] with an
//! `expect` block; [`fixtures_replay_as_expected`] replays each through the
//! live recognisers + dispatch decisions and demands exactly that outcome.
//! Recorded real traces go into the same folder.
//!
//! The synthetic scenarios are built here and written by the ignored
//! generator: `cargo test -p inspector-rust-core --lib gen_gesture_fixtures
//! -- --ignored`. Their `expect` is TODAY'S behaviour (characterisation, not
//! wish) — each `note` says where that behaviour is a known gap the guard
//! pipeline is meant to close, so a later, deliberate change of an expected
//! outcome is visible in review instead of silent.

use super::trace::{replay, DeviceInfo, Frame, KeyDown, Touch, TouchPhase, Trace, TRACE_VERSION};
use super::GestureConfig;

/// The config every fixture is replayed with: all gestures on, defaults
/// otherwise. A fixture asserts behaviour, not a user's settings.
pub fn fixture_config() -> GestureConfig {
    GestureConfig { enabled: true, tiptap: true, ..GestureConfig::default() }
}

fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gestures/fixtures")
}

/// One contact's path: lands at `t0`, moves linearly from `from` to `to`,
/// lifts at `t1`.
struct Path {
    id: i32,
    from: (f64, f64),
    to: (f64, f64),
    t0: u64,
    t1: u64,
    size: f32,
}

struct Scenario {
    name: &'static str,
    note: &'static str,
    paths: Vec<Path>,
    keys: Vec<KeyDown>,
    muted_at_start: bool,
}

const FINGER: f32 = 1.0;

/// Four decimals. serde_json (without `float_roundtrip`) may read an arbitrary
/// f64 back one ULP off, which would make the stale-fixture check flaky; a
/// short decimal parses exactly.
fn q(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}
const FRAME_MS: u64 = 10;

impl Scenario {
    fn new(name: &'static str, note: &'static str) -> Scenario {
        Scenario { name, note, paths: Vec::new(), keys: Vec::new(), muted_at_start: false }
    }
    fn path(mut self, id: i32, from: (f64, f64), to: (f64, f64), t0: u64, t1: u64, size: f32) -> Self {
        self.paths.push(Path { id, from, to, t0, t1, size });
        self
    }
    fn still(self, id: i32, at: (f64, f64), t0: u64, t1: u64, size: f32) -> Self {
        self.path(id, at, at, t0, t1, size)
    }
    fn key(mut self, t_ms: u64, modifier: bool) -> Self {
        self.keys.push(KeyDown { t_ms, modifier });
        self
    }
    fn typing(mut self, from: u64, to: u64, every: u64) -> Self {
        let mut t = from;
        while t <= to {
            self.keys.push(KeyDown { t_ms: t, modifier: false });
            t += every;
        }
        self
    }

    /// Frames every [`FRAME_MS`] while anything touches, plus ONE empty frame
    /// right after the pad empties — the live driver sends a single 0-contact
    /// frame on lift and then nothing until the next touch.
    fn trace(&self) -> Trace {
        let end = self.paths.iter().map(|p| p.t1).max().unwrap_or(0);
        let mut frames = Vec::new();
        let mut was_empty = true;
        let mut t = 0;
        while t <= end + FRAME_MS {
            let touches: Vec<Touch> = self
                .paths
                .iter()
                .filter(|p| p.t0 <= t && t < p.t1)
                .map(|p| {
                    let span = (p.t1 - p.t0).max(1) as f64;
                    let k = (t - p.t0) as f64 / span;
                    let x = p.from.0 + (p.to.0 - p.from.0) * k;
                    let y = p.from.1 + (p.to.1 - p.from.1) * k;
                    Touch {
                        id: p.id,
                        x: q(x),
                        y: q(y),
                        vx: q((p.to.0 - p.from.0) / span * 1000.0),
                        vy: q((p.to.1 - p.from.1) / span * 1000.0),
                        major: p.size * 8.0,
                        minor: p.size * 6.0,
                        angle: 0.0,
                        size: p.size,
                        phase: TouchPhase::Touching,
                    }
                })
                .collect();
            let empty = touches.is_empty();
            if !(empty && was_empty) {
                frames.push(Frame { t_ms: t, device: 0, touches });
            }
            was_empty = empty;
            t += FRAME_MS;
        }
        Trace {
            version: TRACE_VERSION,
            source: "synthetic".into(),
            note: self.note.into(),
            devices: vec![DeviceInfo { builtin: Some(true), width_mm: Some(160.0), height_mm: Some(100.0) }],
            muted_at_start: self.muted_at_start,
            frames,
            keys: self.keys.clone(),
            expect: None,
        }
    }
}

/// The scenarios the guard programme requires, each with a note on what
/// today's code does with it.
// Scenarios are built one by one between `let` bindings; a single `vec![]`
// literal would read worse.
#[allow(clippy::vec_init_then_push)]
fn scenarios() -> Vec<Scenario> {
    let three = [0.40, 0.50, 0.60];
    let mut s = Vec::new();

    s.push(
        Scenario::new(
            "palm-typing-left",
            "Heel of the left hand brushes the pad while typing: one large contact \
             (size ≥ PALM_SIZE) plus a smaller one. Today: the large one is a palm, \
             the small one alone is a 1-finger tap — unmapped. Nothing fires.",
        )
        .typing(0, 900, 120)
        .still(1, (0.12, 0.88), 200, 650, 2.6)
        .still(2, (0.22, 0.80), 260, 420, 1.4),
    );

    s.push(
        Scenario::new(
            "palm-typing-right",
            "Right heel lands as THREE small contacts (each below PALM_SIZE) while \
             typing. Today: read as a 3-finger tap → mute, vetoed only by the \
             typing guard. Without typing it would mute — the guard pipeline's \
             contact classification is meant to catch it earlier.",
        )
        .typing(0, 900, 120)
        .still(1, (0.84, 0.86), 300, 420, 1.5)
        .still(2, (0.90, 0.80), 305, 425, 1.5)
        .still(3, (0.94, 0.90), 310, 430, 1.5),
    );

    s.push(
        Scenario::new(
            "thumb-rest-two-finger-scroll",
            "A thumb rests at the bottom, then two fingers scroll down. Today: the \
             parked thumb is not a mover, the scroll reads as a 2-finger swipe — \
             unmapped, no volume change.",
        )
        .still(1, (0.50, 0.92), 0, 1500, 1.8)
        .path(2, (0.40, 0.30), (0.40, 0.60), 800, 1100, FINGER)
        .path(3, (0.55, 0.30), (0.55, 0.60), 800, 1100, FINGER),
    );

    s.push(
        Scenario::new(
            "edge-stripe-top",
            "Three contacts graze the TOP edge and slide down a little (wrist / \
             sleeve). Today: a 3-finger swipe down → volume down FIRES. Known gap: \
             the guard pipeline's edge zones should reject contacts that land there.",
        )
        .path(1, (0.40, 0.01), (0.40, 0.20), 0, 180, FINGER)
        .path(2, (0.50, 0.01), (0.50, 0.20), 0, 180, FINGER)
        .path(3, (0.60, 0.01), (0.60, 0.20), 0, 180, FINGER),
    );

    let mut up = Scenario::new("swipe-up-3", "A real 3-finger swipe up → volume up.");
    let mut down = Scenario::new("swipe-down-3", "A real 3-finger swipe down → volume down.");
    for (i, x) in three.iter().enumerate() {
        up = up.path(i as i32 + 1, (*x, 0.70), (*x, 0.35), 0, 220, FINGER);
        down = down.path(i as i32 + 1, (*x, 0.35), (*x, 0.70), 0, 220, FINGER);
    }
    s.push(up);
    s.push(down);

    let mut tap = Scenario::new("tap-3", "A real 3-finger tap → mute toggle.");
    for (i, x) in three.iter().enumerate() {
        tap = tap.still(i as i32 + 1, (*x, 0.50), 0, 90, FINGER);
    }
    s.push(tap);

    let mut seq = Scenario::new(
        "tap-3-sequential",
        "A light 3-finger tap the driver reports as three SEPARATE touches, ~25 ms \
         apart. Must stay ONE mute toggle — the settle cluster coalesces them.",
    );
    for (i, x) in three.iter().enumerate() {
        let t0 = i as u64 * 30;
        seq = seq.still(i as i32 + 1, (*x, 0.50), t0, t0 + 20, FINGER);
    }
    s.push(seq);

    let mut fourth = Scenario::new(
        "swipe-plus-fourth-contact",
        "A 3-finger swipe up while a FOURTH contact lands and stays still. Today: \
         only movers count, the swipe fires as 3 fingers → volume up. The guard \
         pipeline is meant to reject a gesture whose contact count changes.",
    );
    for (i, x) in three.iter().enumerate() {
        fourth = fourth.path(i as i32 + 1, (*x, 0.70), (*x, 0.35), 0, 220, FINGER);
    }
    s.push(fourth.still(4, (0.80, 0.70), 30, 260, FINGER));

    let mut after_key = Scenario::new(
        "tap-after-keypress",
        "A 3-finger tap 150 ms after a real key-down → vetoed by the typing guard.",
    )
    .key(100, false);
    for (i, x) in three.iter().enumerate() {
        after_key = after_key.still(i as i32 + 1, (*x, 0.50), 250, 340, FINGER);
    }
    s.push(after_key);

    let mut landed = Scenario::new(
        "tap-landing-while-typing",
        "A 3-finger tap that LANDS 0.49 s after a key-down and lifts 0.58 s after \
         it. Only the touch-start sample is inside the guard window — the veto \
         must come from it (libinput's disable-while-typing question).",
    )
    .key(100, false);
    for (i, x) in three.iter().enumerate() {
        landed = landed.still(i as i32 + 1, (*x, 0.50), 590, 680, FINGER);
    }
    s.push(landed);

    let mut resumed = Scenario::new(
        "key-right-after-tap",
        "A deliberate 3-finger tap, then typing resumes 60 ms after the lift — \
         BEFORE the deferred tap is dispatched (settle + tick). The key came \
         after the touch, so it must not veto it (the v0.166.1 field bug).",
    )
    .key(400, false);
    for (i, x) in three.iter().enumerate() {
        resumed = resumed.still(i as i32 + 1, (*x, 0.50), 250, 340, FINGER);
    }
    s.push(resumed);

    let mut after_mod = Scenario::new(
        "tap-after-modifier",
        "A 3-finger tap 150 ms after a pure MODIFIER press → modifiers never arm \
         the guard, the mute fires.",
    )
    .key(100, true);
    for (i, x) in three.iter().enumerate() {
        after_mod = after_mod.still(i as i32 + 1, (*x, 0.50), 250, 340, FINGER);
    }
    s.push(after_mod);

    let mut unmute = Scenario::new(
        "unmute-tap-while-typing",
        "Output is muted; a 3-finger tap right after typing. Today: an UNMUTE is \
         never vetoed (a vetoed unmute strands the user muted).",
    )
    .key(100, false);
    for (i, x) in three.iter().enumerate() {
        unmute = unmute.still(i as i32 + 1, (*x, 0.50), 250, 340, FINGER);
    }
    unmute.muted_at_start = true;
    s.push(unmute);

    s.push(
        Scenario::new(
            "tiptap-with-resting-palm",
            "A palm heel rests; one finger rests, a second taps to its right. \
             Today: size-palms are dropped from the tip-tap feed, so the tip-tap \
             fires → next tab.",
        )
        .still(1, (0.15, 0.90), 0, 900, 2.6)
        .still(2, (0.45, 0.50), 100, 900, FINGER)
        .still(3, (0.60, 0.50), 400, 480, FINGER),
    );

    s
}

#[test]
#[ignore = "writes src/gestures/fixtures — run by hand after changing a scenario"]
fn gen_gesture_fixtures() {
    let dir = fixtures_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = fixture_config();
    for sc in scenarios() {
        let mut t = sc.trace();
        t.expect = Some(replay(&t, &cfg).iter().map(|o| o.expected()).collect());
        let json = serde_json::to_string_pretty(&t).unwrap();
        std::fs::write(dir.join(format!("{}.json", sc.name)), json + "\n").unwrap();
    }
}

/// Every fixture replays to exactly its expected outcomes.
#[test]
fn fixtures_replay_as_expected() {
    let cfg = fixture_config();
    let mut checked = 0;
    for entry in std::fs::read_dir(fixtures_dir()).expect("fixtures folder") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let t = Trace::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let expect = t.expect.clone().unwrap_or_else(|| panic!("{} has no expect", path.display()));
        let got: Vec<_> = replay(&t, &cfg).iter().map(|o| o.expected()).collect();
        assert_eq!(got, expect, "{}: replay diverged from its golden outcome", path.display());
        checked += 1;
    }
    // A replay over an empty folder would pass by emptiness.
    assert!(checked >= scenarios().len(), "only {checked} fixtures found");
}

/// Every scenario the guard programme requires has a fixture on disk, and the
/// fixture still matches the scenario that generated it (a hand-edited
/// fixture or a changed builder can't drift silently).
#[test]
fn every_scenario_has_an_up_to_date_fixture() {
    for sc in scenarios() {
        let path = fixtures_dir().join(format!("{}.json", sc.name));
        let json = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("missing fixture {} — run gen_gesture_fixtures", path.display()));
        let mut on_disk = Trace::from_json(&json).unwrap();
        on_disk.expect = None;
        assert_eq!(on_disk, sc.trace(), "{} is stale — rerun gen_gesture_fixtures", sc.name);
    }
}

/// The outcomes the scenarios exist for, spelled out — so a regenerated
/// fixture with a changed outcome cannot pass review by accident.
#[test]
fn key_scenarios_have_the_documented_outcome() {
    use super::{GestureAction as A, GestureKind as K};
    let cfg = fixture_config();
    let get = |name: &str| {
        let sc = scenarios().into_iter().find(|s| s.name == name).unwrap();
        replay(&sc.trace(), &cfg)
            .into_iter()
            .filter(|o| o.action.is_some())
            .map(|o| (o.event.kind, o.action.unwrap(), o.verdict.code()))
            .collect::<Vec<_>>()
    };
    let d = "dispatched".to_string();
    assert_eq!(get("swipe-up-3"), [(K::SwipeUp, A::VolumeUp, d.clone())]);
    assert_eq!(get("swipe-down-3"), [(K::SwipeDown, A::VolumeDown, d.clone())]);
    assert_eq!(get("tap-3"), [(K::Tap, A::MuteToggle, d.clone())]);
    assert_eq!(get("tap-3-sequential"), [(K::Tap, A::MuteToggle, d.clone())]);
    assert_eq!(get("tap-after-keypress"), [(K::Tap, A::MuteToggle, "typing_guard".to_string())]);
    assert_eq!(get("tap-after-modifier"), [(K::Tap, A::MuteToggle, d.clone())]);
    assert_eq!(get("key-right-after-tap"), [(K::Tap, A::MuteToggle, d.clone())]);
    assert_eq!(get("tap-landing-while-typing"), [(K::Tap, A::MuteToggle, "typing_guard".to_string())]);
    assert_eq!(get("unmute-tap-while-typing"), [(K::Tap, A::MuteToggle, d.clone())]);
    assert_eq!(get("palm-typing-left"), []);
    assert_eq!(get("thumb-rest-two-finger-scroll"), []);
    assert_eq!(get("tiptap-with-resting-palm"), [(K::TipTapRight, A::NextTab, d)]);
}
