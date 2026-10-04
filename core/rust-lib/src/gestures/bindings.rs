//! User-defined gesture → action bindings (the BetterTouchTool principle:
//! pick a gesture, pick what it does, change or delete it any time).
//!
//! The recogniser and the four guard levels decide WHETHER a gesture
//! happened; this module decides WHAT it does. Everything here is pure and
//! unit-tested; the side effects (sending a shortcut, opening a URL, starting
//! a task) live in `mod.rs::perform`.
//!
//! Migration: until the user saves a list, the bindings are derived from the
//! historic switches (`gestures.volume` / `gestures.mute` / `gestures.tiptap`),
//! so an existing install behaves exactly as before ([`legacy`]). The first
//! save writes `gestures.bindings`; from then on the switches are ignored.

use super::{GestureAction, GestureConfig, GestureEvent, GestureKind};
use crate::db::DbHandle;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const KEY_BINDINGS: &str = "gestures.bindings";
/// Swipes and taps: 3 to 5 fingers. Two fingers are scrolling and the
/// secondary click — binding them would fight the system on every scroll.
pub const MIN_FINGERS: u8 = 3;
pub const MAX_FINGERS: u8 = 5;
pub const MAX_BINDINGS: usize = 64;
/// Tip-taps have a fixed posture (one finger rests, a second taps); their
/// trigger stores this count and matching ignores the event's count.
pub const TIPTAP_FINGERS: u8 = 2;

// ── Model ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub kind: GestureKind,
    #[serde(default)]
    pub fingers: u8,
}

impl Trigger {
    pub fn is_tiptap(&self) -> bool {
        matches!(self.kind, GestureKind::TipTapLeft | GestureKind::TipTapRight)
    }

    /// `Some(score)` when `ev` fires this trigger; higher wins.
    /// * tip-tap: the kind alone (the event's count is not meaningful);
    /// * swipe: EXACTLY the count — a 3-finger swipe must not fire a
    ///   4-finger binding, and a 2-finger scroll fires nothing;
    /// * tap: AT MOST the event's count, the largest bound count wins. A
    ///   coalesced tap cluster can over-count by one when a finger re-touches
    ///   (a new contact id), so a sloppy 3-finger tap read as 4 still fires
    ///   the 3-finger binding when no 4-finger one exists (the historic rule).
    pub fn score(&self, ev: &GestureEvent) -> Option<u8> {
        if self.kind != ev.kind {
            return None;
        }
        match self.kind {
            GestureKind::TipTapLeft | GestureKind::TipTapRight => Some(0),
            GestureKind::Tap => (self.fingers <= ev.fingers).then_some(self.fingers),
            _ => (self.fingers == ev.fingers).then_some(self.fingers),
        }
    }

    /// Two triggers fire on the same physical gesture.
    pub fn same_as(&self, other: &Trigger) -> bool {
        self.kind == other.kind && (self.is_tiptap() || self.fingers == other.fingers)
    }
}

/// What a gesture does. Unit variants are the built-in actions; the rest
/// carry their target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BindingAction {
    VolumeUp,
    VolumeDown,
    MuteToggle,
    NextTab,
    PrevTab,
    /// One of the app's global actions (`hotkey::ActionId::key`).
    Hotkey { action: String },
    /// Send a key chord, in the spec format the hotkey fields record
    /// (`Meta+Shift+KeyT`).
    Shortcut { keys: String },
    /// Open a URL (http/https/mailto) or an absolute path (app, file, folder).
    Open { target: String },
    /// Run an approved AI task.
    Task { id: i64 },
}

impl BindingAction {
    pub fn kind(&self) -> GestureAction {
        match self {
            BindingAction::VolumeUp => GestureAction::VolumeUp,
            BindingAction::VolumeDown => GestureAction::VolumeDown,
            BindingAction::MuteToggle => GestureAction::MuteToggle,
            BindingAction::NextTab => GestureAction::NextTab,
            BindingAction::PrevTab => GestureAction::PrevTab,
            BindingAction::Hotkey { .. } => GestureAction::Hotkey,
            BindingAction::Shortcut { .. } => GestureAction::Shortcut,
            BindingAction::Open { .. } => GestureAction::Open,
            BindingAction::Task { .. } => GestureAction::Task,
        }
    }

    /// Tab switching sends keystrokes itself and is used in bursts — the
    /// historic exemption from the typing guard is its default.
    pub fn default_typing_guard(&self) -> bool {
        !matches!(self, BindingAction::NextTab | BindingAction::PrevTab)
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GestureBinding {
    #[serde(default)]
    pub id: String,
    pub trigger: Trigger,
    pub action: BindingAction,
    /// Only while this app is in front (macOS bundle id, e.g.
    /// `com.googlecode.iterm2`; elsewhere the process name). `None` = everywhere.
    #[serde(default)]
    pub app: Option<String>,
    /// The app's display name for the editor (the id is what matches).
    #[serde(default)]
    pub app_name: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Suppress this binding right after typing (the global typing guard
    /// must be on too).
    #[serde(default = "yes")]
    pub typing_guard: bool,
}

// ── Defaults + migration ─────────────────────────────────────────────────────

fn builtin(id: &str, kind: GestureKind, fingers: u8, action: BindingAction, enabled: bool) -> GestureBinding {
    GestureBinding {
        id: id.into(),
        trigger: Trigger { kind, fingers },
        typing_guard: action.default_typing_guard(),
        action,
        app: None,
        app_name: None,
        enabled,
    }
}

/// The out-of-the-box set — the historic bindings. Tip-taps stay off (they
/// misfired during thumb-anchored cursor use, v0.84.209).
pub fn defaults() -> Vec<GestureBinding> {
    legacy(&GestureConfig::default())
}

/// The historic switches as bindings: what an install that never saved a
/// list does. Every existing test and golden fixture runs through this, so
/// it must reproduce `map_action`'s old behaviour exactly.
pub fn legacy(cfg: &GestureConfig) -> Vec<GestureBinding> {
    let f = cfg.fingers;
    vec![
        builtin("volume-up", GestureKind::SwipeUp, f, BindingAction::VolumeUp, cfg.volume),
        builtin("volume-down", GestureKind::SwipeDown, f, BindingAction::VolumeDown, cfg.volume),
        builtin("mute", GestureKind::Tap, f, BindingAction::MuteToggle, cfg.mute),
        builtin("next-tab", GestureKind::TipTapRight, TIPTAP_FINGERS, BindingAction::NextTab, cfg.tiptap),
        builtin("prev-tab", GestureKind::TipTapLeft, TIPTAP_FINGERS, BindingAction::PrevTab, cfg.tiptap),
    ]
}

// ── Matching ─────────────────────────────────────────────────────────────────

/// Bundle ids / process names compare case-insensitively.
pub fn app_matches(app: &str, frontmost: Option<&str>) -> bool {
    frontmost.is_some_and(|f| f.trim().eq_ignore_ascii_case(app.trim()))
}

fn candidates<'a>(
    list: &'a [GestureBinding],
    ev: &GestureEvent,
    frontmost: Option<&str>,
    enabled: bool,
) -> Vec<(&'a GestureBinding, (bool, u8))> {
    list.iter()
        .filter(|b| b.enabled == enabled)
        .filter_map(|b| {
            let score = b.trigger.score(ev)?;
            let scoped = match &b.app {
                None => false,
                Some(a) if app_matches(a, frontmost) => true,
                Some(_) => return None,
            };
            Some((b, (scoped, score)))
        })
        .collect()
}

/// The binding `ev` fires: an app-specific binding for the frontmost app
/// beats a global one, then the larger finger count, then list order.
pub fn resolve<'a>(list: &'a [GestureBinding], ev: &GestureEvent, frontmost: Option<&str>) -> Option<&'a GestureBinding> {
    let mut best: Option<(&GestureBinding, (bool, u8))> = None;
    for (b, key) in candidates(list, ev, frontmost, true) {
        if best.is_none_or(|(_, k)| key > k) {
            best = Some((b, key));
        }
    }
    best.map(|(b, _)| b)
}

/// A switched-off binding WOULD have fired — the reason the guard logs, so a
/// gesture never vanishes without a trace (the 2026-09-04 lesson).
pub fn disabled_match<'a>(list: &'a [GestureBinding], ev: &GestureEvent, frontmost: Option<&str>) -> Option<&'a GestureBinding> {
    candidates(list, ev, frontmost, false).into_iter().map(|(b, _)| b).next()
}

/// Only then is the frontmost app worth asking for.
pub fn has_app_scoped(list: &[GestureBinding]) -> bool {
    list.iter().any(|b| b.enabled && b.app.is_some())
}

// ── Key chords ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

/// Parse `Meta+Shift+KeyT` (the hotkey-field format). The key must be one
/// [`mac_keycode`] knows — a chord we can't send is refused when saving, not
/// discovered when the gesture fires.
pub fn parse_chord(spec: &str) -> Result<(Mods, String), String> {
    let tokens: Vec<&str> = spec.trim().split('+').map(str::trim).collect();
    let Some((code, mods)) = tokens.split_last() else {
        return Err("leerer Tastenkurzbefehl".into());
    };
    if code.is_empty() || mods.iter().any(|t| t.is_empty()) {
        return Err(format!("unvollständiger Tastenkurzbefehl „{spec}“"));
    }
    let mut m = Mods::default();
    for t in mods {
        match t.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => m.ctrl = true,
            "shift" => m.shift = true,
            "alt" | "option" | "opt" => m.alt = true,
            "meta" | "cmd" | "command" | "super" | "win" => m.meta = true,
            other => return Err(format!("unbekannte Sondertaste „{other}“")),
        }
    }
    if mac_keycode(code).is_none() {
        return Err(format!("Taste „{code}“ kann nicht gesendet werden"));
    }
    Ok((m, (*code).to_string()))
}

/// macOS virtual keycode for a W3C `KeyboardEvent.code`. Physical positions
/// (ANSI), so a chord recorded on any layout presses the same physical key.
pub fn mac_keycode(code: &str) -> Option<u16> {
    const LETTERS: [(char, u16); 26] = [
        ('A', 0), ('B', 11), ('C', 8), ('D', 2), ('E', 14), ('F', 3), ('G', 5), ('H', 4), ('I', 34),
        ('J', 38), ('K', 40), ('L', 37), ('M', 46), ('N', 45), ('O', 31), ('P', 35), ('Q', 12),
        ('R', 15), ('S', 1), ('T', 17), ('U', 32), ('V', 9), ('W', 13), ('X', 7), ('Y', 16), ('Z', 6),
    ];
    const DIGITS: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
    const FKEYS: [u16; 20] = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64, 79, 80, 90];
    if let Some(l) = code.strip_prefix("Key") {
        let mut c = l.chars();
        let (Some(ch), None) = (c.next(), c.next()) else { return None };
        return LETTERS.iter().find(|(x, _)| *x == ch).map(|(_, k)| *k);
    }
    if let Some(d) = code.strip_prefix("Digit") {
        return d.parse::<usize>().ok().filter(|n| *n < 10).map(|n| DIGITS[n]);
    }
    if let Some(n) = code.strip_prefix('F').and_then(|n| n.parse::<usize>().ok()) {
        return (1..=20).contains(&n).then(|| FKEYS[n - 1]);
    }
    Some(match code {
        "Enter" => 36,
        "NumpadEnter" => 76,
        "Tab" => 48,
        "Space" => 49,
        "Backspace" => 51,
        "Escape" => 53,
        "Delete" => 117,
        "Home" => 115,
        "End" => 119,
        "PageUp" => 116,
        "PageDown" => 121,
        "ArrowLeft" => 123,
        "ArrowRight" => 124,
        "ArrowDown" => 125,
        "ArrowUp" => 126,
        "Minus" => 27,
        "Equal" => 24,
        "BracketLeft" => 33,
        "BracketRight" => 30,
        "Backslash" => 42,
        "Semicolon" => 41,
        "Quote" => 39,
        "Comma" => 43,
        "Period" => 47,
        "Slash" => 44,
        "Backquote" => 50,
        _ => return None,
    })
}

/// Arrow and navigation keys carry the Fn flag on real hardware (arrows also
/// NumericPad) — some apps match exactly (the iTerm2 lesson, v0.84.211).
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn needs_fn_flag(code: &str) -> (bool, bool) {
    let arrow = code.starts_with("Arrow");
    let nav = matches!(code, "Home" | "End" | "PageUp" | "PageDown" | "Delete");
    (arrow || nav, arrow)
}

// ── Open targets ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenTarget {
    Url(String),
    Path(String),
}

/// A URL with a safe scheme, or an absolute path. Anything else is refused:
/// arbitrary schemes can launch arbitrary handlers, and a leading `-` would
/// reach the opener as a flag.
pub fn classify_open(target: &str) -> Result<OpenTarget, String> {
    let t = target.trim();
    if t.is_empty() {
        return Err("Ziel fehlt".into());
    }
    if t.starts_with('-') {
        return Err("Ziel darf nicht mit „-“ beginnen".into());
    }
    let lower = t.to_ascii_lowercase();
    if ["http://", "https://", "mailto:"].iter().any(|s| lower.starts_with(s)) {
        return Ok(OpenTarget::Url(t.to_string()));
    }
    if lower.contains("://") {
        return Err("nur http-, https- und mailto-Adressen".into());
    }
    let absolute = t.starts_with('/')
        || t.starts_with("~/")
        || t.starts_with("\\\\")
        || (t.len() >= 3 && t.as_bytes()[1] == b':' && matches!(t.as_bytes()[2], b'\\' | b'/'));
    if absolute {
        Ok(OpenTarget::Path(t.to_string()))
    } else {
        Err("Pfad muss absolut sein (z. B. /Applications/Safari.app)".into())
    }
}

// ── Validation ───────────────────────────────────────────────────────────────

/// Clean up and check a list before it is saved: trims, fills missing ids,
/// normalises tip-tap finger counts, checks every target and refuses two
/// enabled bindings on the same gesture in the same scope.
pub fn validate(mut list: Vec<GestureBinding>) -> Result<Vec<GestureBinding>, String> {
    if list.len() > MAX_BINDINGS {
        return Err(format!("höchstens {MAX_BINDINGS} Zuordnungen"));
    }
    for b in &mut list {
        b.id = b.id.trim().to_string();
        b.app = b.app.take().map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
        b.app_name = if b.app.is_some() {
            b.app_name.take().map(|a| a.trim().to_string()).filter(|a| !a.is_empty())
        } else {
            None
        };
        if b.trigger.is_tiptap() {
            b.trigger.fingers = TIPTAP_FINGERS;
        } else if !(MIN_FINGERS..=MAX_FINGERS).contains(&b.trigger.fingers) {
            return Err(format!("Fingerzahl muss zwischen {MIN_FINGERS} und {MAX_FINGERS} liegen"));
        }
        match &mut b.action {
            BindingAction::Hotkey { action } => {
                if crate::hotkey::ActionId::from_key(action.trim()).is_none() {
                    return Err(format!("unbekannte Aktion „{action}“"));
                }
                *action = action.trim().to_string();
            }
            BindingAction::Shortcut { keys } => {
                parse_chord(keys)?;
                *keys = keys.trim().to_string();
            }
            BindingAction::Open { target } => {
                classify_open(target)?;
                *target = target.trim().to_string();
            }
            BindingAction::Task { id } if *id <= 0 => return Err("kein KI-Task gewählt".into()),
            _ => {}
        }
    }
    // Ids: unique and present (a fresh row from the UI carries none). A
    // generated id must not collide with one kept further down the list.
    let mut taken: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut keep = vec![false; list.len()];
    for (i, b) in list.iter().enumerate() {
        if !b.id.is_empty() && taken.insert(b.id.clone()) {
            keep[i] = true;
        }
    }
    let mut next_id = 1;
    for (i, b) in list.iter_mut().enumerate() {
        if !keep[i] {
            while taken.contains(&format!("b{next_id}")) {
                next_id += 1;
            }
            b.id = format!("b{next_id}");
            taken.insert(b.id.clone());
        }
    }
    for (i, a) in list.iter().enumerate() {
        for b in &list[i + 1..] {
            let same_scope = match (&a.app, &b.app) {
                (None, None) => true,
                (Some(x), Some(y)) => x.eq_ignore_ascii_case(y),
                _ => false,
            };
            if a.enabled && b.enabled && same_scope && a.trigger.same_as(&b.trigger) {
                return Err("zwei aktive Zuordnungen für dieselbe Geste".into());
            }
        }
    }
    Ok(list)
}

// ── Persistence + the live copy ──────────────────────────────────────────────

/// Saved list, or the legacy derivation when none was ever saved. A
/// hand-edited, unreadable value falls back to the legacy derivation (logged)
/// rather than to nothing — gestures that stop working without a word is the
/// failure this programme exists to prevent.
pub fn load(db: &DbHandle) -> Vec<GestureBinding> {
    let stored = crate::settings::get(db, KEY_BINDINGS).ok().flatten();
    match stored {
        None => legacy(&GestureConfig::load(db)),
        Some(json) => match serde_json::from_str::<Vec<GestureBinding>>(&json).map_err(|e| e.to_string()).and_then(validate) {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!("gestures: saved bindings unreadable ({e}) — using the built-in set");
                legacy(&GestureConfig::load(db))
            }
        },
    }
}

/// Whether the user ever saved a list (the UI says "Standard" otherwise).
pub fn is_customised(db: &DbHandle) -> bool {
    crate::settings::get(db, KEY_BINDINGS).ok().flatten().is_some()
}

pub fn save(db: &DbHandle, list: Vec<GestureBinding>) -> Result<Vec<GestureBinding>, String> {
    let list = validate(list)?;
    let json = serde_json::to_string(&list).map_err(|e| e.to_string())?;
    crate::settings::set(db, KEY_BINDINGS, &json).map_err(|e| e.to_string())?;
    set_live(list.clone());
    Ok(list)
}

/// Forget the saved list: back to the built-in set (the historic switches).
pub fn reset(db: &DbHandle) -> Result<Vec<GestureBinding>, String> {
    crate::settings::delete(db, KEY_BINDINGS).map_err(|e| e.to_string())?;
    let list = load(db);
    set_live(list.clone());
    Ok(list)
}

static LIVE: RwLock<Option<Arc<Vec<GestureBinding>>>> = RwLock::new(None);

/// The bindings the running capture uses; swapped in place on save, so an
/// edit takes effect with the next gesture — no capture restart.
pub fn set_live(list: Vec<GestureBinding>) {
    *LIVE.write() = Some(Arc::new(list));
}

pub fn live() -> Arc<Vec<GestureBinding>> {
    LIVE.read().clone().unwrap_or_else(|| Arc::new(defaults()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: GestureKind, fingers: u8) -> GestureEvent {
        GestureEvent { kind, fingers }
    }

    fn bind(kind: GestureKind, fingers: u8, action: BindingAction) -> GestureBinding {
        GestureBinding {
            id: String::new(),
            trigger: Trigger { kind, fingers },
            typing_guard: action.default_typing_guard(),
            action,
            app: None,
            app_name: None,
            enabled: true,
        }
    }

    fn open(url: &str) -> BindingAction {
        BindingAction::Open { target: url.into() }
    }

    #[test]
    fn swipes_need_the_exact_finger_count() {
        let list = vec![bind(GestureKind::SwipeUp, 3, BindingAction::VolumeUp)];
        assert!(resolve(&list, &ev(GestureKind::SwipeUp, 3), None).is_some());
        assert!(resolve(&list, &ev(GestureKind::SwipeUp, 4), None).is_none());
        assert!(resolve(&list, &ev(GestureKind::SwipeUp, 2), None).is_none());
    }

    #[test]
    fn a_tap_fires_the_largest_bound_count_not_above_it() {
        let three = bind(GestureKind::Tap, 3, BindingAction::MuteToggle);
        let four = bind(GestureKind::Tap, 4, open("https://a.example"));
        // Only 3 bound: an over-counted 4-finger tap still mutes (historic rule).
        let only3 = vec![three.clone()];
        assert_eq!(resolve(&only3, &ev(GestureKind::Tap, 4), None).unwrap().action, BindingAction::MuteToggle);
        // Both bound: each count reaches its own binding, regardless of order.
        let both = vec![three.clone(), four.clone()];
        assert_eq!(resolve(&both, &ev(GestureKind::Tap, 4), None).unwrap().action, four.action);
        assert_eq!(resolve(&both, &ev(GestureKind::Tap, 3), None).unwrap().action, three.action);
        let reversed = vec![four.clone(), three.clone()];
        assert_eq!(resolve(&reversed, &ev(GestureKind::Tap, 3), None).unwrap().action, three.action);
        assert!(resolve(&both, &ev(GestureKind::Tap, 2), None).is_none());
    }

    #[test]
    fn tip_taps_ignore_the_event_finger_count() {
        let list = vec![bind(GestureKind::TipTapRight, TIPTAP_FINGERS, BindingAction::NextTab)];
        // The recogniser reports tip-taps with fingers = 3.
        assert!(resolve(&list, &ev(GestureKind::TipTapRight, 3), None).is_some());
        assert!(resolve(&list, &ev(GestureKind::TipTapLeft, 3), None).is_none());
    }

    #[test]
    fn an_app_binding_beats_the_global_one_only_in_that_app() {
        let global = bind(GestureKind::SwipeLeft, 3, open("https://global.example"));
        let mut scoped = bind(GestureKind::SwipeLeft, 3, open("https://iterm.example"));
        scoped.app = Some("com.googlecode.iterm2".into());
        // Scoped listed AFTER global: scope must win, not list order.
        let list = vec![global.clone(), scoped.clone()];
        let e = ev(GestureKind::SwipeLeft, 3);
        assert_eq!(resolve(&list, &e, Some("COM.googlecode.iTerm2")).unwrap().action, scoped.action);
        assert_eq!(resolve(&list, &e, Some("com.apple.Safari")).unwrap().action, global.action);
        assert_eq!(resolve(&list, &e, None).unwrap().action, global.action);
        // Without a global fallback, another app gets nothing.
        assert!(resolve(&[scoped], &e, Some("com.apple.Safari")).is_none());
    }

    #[test]
    fn disabled_bindings_never_fire_but_are_reported() {
        let mut b = bind(GestureKind::Tap, 3, BindingAction::MuteToggle);
        b.enabled = false;
        let list = vec![b];
        let e = ev(GestureKind::Tap, 3);
        assert!(resolve(&list, &e, None).is_none());
        assert!(disabled_match(&list, &e, None).is_some());
        assert!(disabled_match(&list, &ev(GestureKind::SwipeUp, 3), None).is_none());
    }

    #[test]
    fn legacy_reproduces_the_historic_switches() {
        let cfg = GestureConfig { enabled: true, ..GestureConfig::default() };
        let l = legacy(&cfg);
        let kind = |e: GestureEvent| resolve(&l, &e, None).map(|b| b.action.kind());
        assert_eq!(kind(ev(GestureKind::SwipeUp, 3)), Some(GestureAction::VolumeUp));
        assert_eq!(kind(ev(GestureKind::SwipeDown, 3)), Some(GestureAction::VolumeDown));
        assert_eq!(kind(ev(GestureKind::Tap, 3)), Some(GestureAction::MuteToggle));
        assert_eq!(kind(ev(GestureKind::Tap, 4)), Some(GestureAction::MuteToggle));
        assert_eq!(kind(ev(GestureKind::SwipeLeft, 3)), None);
        // Tip-taps are opt-in.
        assert_eq!(kind(ev(GestureKind::TipTapRight, 3)), None);
        let tiptap = legacy(&GestureConfig { tiptap: true, volume: false, ..cfg });
        assert_eq!(resolve(&tiptap, &ev(GestureKind::TipTapLeft, 3), None).map(|b| b.action.kind()), Some(GestureAction::PrevTab));
        assert!(resolve(&tiptap, &ev(GestureKind::SwipeUp, 3), None).is_none());
        // Tabs keep their historic typing-guard exemption.
        assert!(l.iter().filter(|b| b.trigger.is_tiptap()).all(|b| !b.typing_guard));
        assert!(l.iter().filter(|b| !b.trigger.is_tiptap()).all(|b| b.typing_guard));
    }

    #[test]
    fn validate_refuses_two_active_bindings_for_one_gesture() {
        let a = bind(GestureKind::SwipeLeft, 4, open("https://a.example"));
        let b = bind(GestureKind::SwipeLeft, 4, BindingAction::NextTab);
        assert!(validate(vec![a.clone(), b.clone()]).is_err());
        // A disabled duplicate is fine (a parked alternative)…
        let mut off = b.clone();
        off.enabled = false;
        assert!(validate(vec![a.clone(), off]).is_ok());
        // …so is a different scope or a different count.
        let mut scoped = b.clone();
        scoped.app = Some("com.apple.Safari".into());
        assert!(validate(vec![a.clone(), scoped]).is_ok());
        let five = bind(GestureKind::SwipeLeft, 5, BindingAction::NextTab);
        assert!(validate(vec![a.clone(), five]).is_ok());
        // Tip-taps collide regardless of the stored count.
        let t1 = bind(GestureKind::TipTapLeft, 2, BindingAction::PrevTab);
        let t2 = bind(GestureKind::TipTapLeft, 4, BindingAction::NextTab);
        assert!(validate(vec![t1, t2]).is_err());
    }

    #[test]
    fn validate_checks_counts_targets_and_fills_ids() {
        assert!(validate(vec![bind(GestureKind::SwipeUp, 2, BindingAction::VolumeUp)]).is_err());
        assert!(validate(vec![bind(GestureKind::SwipeUp, 6, BindingAction::VolumeUp)]).is_err());
        assert!(validate(vec![bind(GestureKind::SwipeUp, 3, BindingAction::Hotkey { action: "nope".into() })]).is_err());
        assert!(validate(vec![bind(GestureKind::SwipeUp, 3, BindingAction::Hotkey { action: "ocr".into() })]).is_ok());
        assert!(validate(vec![bind(GestureKind::SwipeUp, 3, BindingAction::Task { id: 0 })]).is_err());
        assert!(validate(vec![bind(GestureKind::SwipeUp, 3, BindingAction::Shortcut { keys: "Meta+Nope".into() })]).is_err());
        let ok = validate(vec![
            bind(GestureKind::SwipeUp, 3, BindingAction::VolumeUp),
            bind(GestureKind::SwipeDown, 3, BindingAction::VolumeDown),
            bind(GestureKind::TipTapRight, 5, BindingAction::NextTab),
        ])
        .unwrap();
        let ids: std::collections::HashSet<_> = ok.iter().map(|b| b.id.clone()).collect();
        assert_eq!(ids.len(), 3);
        assert!(ok.iter().all(|b| !b.id.is_empty()));
        assert_eq!(ok[2].trigger.fingers, TIPTAP_FINGERS);
        // Duplicate ids are renamed, not merged.
        let mut d1 = bind(GestureKind::SwipeUp, 3, BindingAction::VolumeUp);
        let mut d2 = bind(GestureKind::SwipeDown, 3, BindingAction::VolumeDown);
        d1.id = "x".into();
        d2.id = "x".into();
        let fixed = validate(vec![d1, d2]).unwrap();
        assert_ne!(fixed[0].id, fixed[1].id);
        assert_eq!(fixed[0].id, "x");
        // A generated id never steals one a later row keeps.
        let mut fresh = bind(GestureKind::SwipeUp, 3, BindingAction::VolumeUp);
        fresh.id = String::new();
        let mut later = bind(GestureKind::SwipeDown, 3, BindingAction::VolumeDown);
        later.id = "b1".into();
        let fixed = validate(vec![fresh, later]).unwrap();
        assert_eq!(fixed[1].id, "b1");
        assert_ne!(fixed[0].id, "b1");
        // Blank app scope = everywhere.
        let mut blank = bind(GestureKind::SwipeUp, 3, BindingAction::VolumeUp);
        blank.app = Some("  ".into());
        assert_eq!(validate(vec![blank]).unwrap()[0].app, None);
    }

    #[test]
    fn open_targets_are_urls_or_absolute_paths() {
        assert_eq!(classify_open(" https://celox.io ").unwrap(), OpenTarget::Url("https://celox.io".into()));
        assert!(matches!(classify_open("mailto:a@b.c"), Ok(OpenTarget::Url(_))));
        assert!(matches!(classify_open("/Applications/Safari.app"), Ok(OpenTarget::Path(_))));
        assert!(matches!(classify_open("~/Downloads"), Ok(OpenTarget::Path(_))));
        assert!(matches!(classify_open("C:\\Windows\\notepad.exe"), Ok(OpenTarget::Path(_))));
        for bad in ["", "Safari.app", "-a Terminal", "file:///etc/passwd", "javascript:alert(1)x://", "ssh://host"] {
            assert!(classify_open(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn chords_parse_in_the_hotkey_field_format() {
        let (m, code) = parse_chord("Meta+Shift+KeyT").unwrap();
        assert_eq!(m, Mods { meta: true, shift: true, ..Mods::default() });
        assert_eq!(code, "KeyT");
        assert!(parse_chord("Ctrl+Alt+Digit1").is_ok());
        assert!(parse_chord("F5").is_ok());
        assert!(parse_chord("Ctrl+").is_err());
        assert!(parse_chord("Hyper+KeyA").is_err());
        assert!(parse_chord("Ctrl+IntlRo").is_err());
    }

    #[test]
    fn keycodes_follow_the_ansi_positions() {
        // Spot checks against Carbon's kVK_ANSI_* constants.
        assert_eq!(mac_keycode("KeyA"), Some(0));
        assert_eq!(mac_keycode("KeyT"), Some(17));
        assert_eq!(mac_keycode("Digit0"), Some(29));
        assert_eq!(mac_keycode("Digit5"), Some(23));
        assert_eq!(mac_keycode("F1"), Some(122));
        assert_eq!(mac_keycode("F12"), Some(111));
        assert_eq!(mac_keycode("BracketRight"), Some(30));
        assert_eq!(mac_keycode("ArrowUp"), Some(126));
        assert_eq!(mac_keycode("F21"), None);
        assert_eq!(mac_keycode("KeyAB"), None);
        // Every letter and digit maps, and no two to the same key.
        let mut codes: Vec<u16> = ('A'..='Z')
            .map(|c| mac_keycode(&format!("Key{c}")).unwrap())
            .chain((0..10).map(|d| mac_keycode(&format!("Digit{d}")).unwrap()))
            .collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), 36);
        assert_eq!(needs_fn_flag("ArrowLeft"), (true, true));
        assert_eq!(needs_fn_flag("Home"), (true, false));
        assert_eq!(needs_fn_flag("KeyA"), (false, false));
    }

    #[test]
    fn the_serialised_form_is_stable() {
        // The frontend and every saved list depend on this shape.
        let b = bind(GestureKind::SwipeLeft, 4, BindingAction::Shortcut { keys: "Meta+KeyW".into() });
        let json = serde_json::to_value(&b).unwrap();
        assert_eq!(json["trigger"]["kind"], "swipe_left");
        assert_eq!(json["trigger"]["fingers"], 4);
        assert_eq!(json["action"]["type"], "shortcut");
        assert_eq!(json["action"]["keys"], "Meta+KeyW");
        // An old/minimal entry fills its defaults.
        let parsed: GestureBinding =
            serde_json::from_str(r#"{"trigger":{"kind":"tap","fingers":3},"action":{"type":"mute_toggle"}}"#).unwrap();
        assert!(parsed.enabled && parsed.typing_guard && parsed.app.is_none());
    }

    #[test]
    fn persistence_round_trips_and_falls_back() {
        let db: DbHandle = std::sync::Arc::new(parking_lot::Mutex::new(rusqlite::Connection::open_in_memory().unwrap()));
        crate::settings::init_table(&db).unwrap();
        // Nothing saved → the legacy set, derived from the switches.
        crate::settings::set(&db, "gestures.mute", "false").unwrap();
        let l = load(&db);
        assert!(!is_customised(&db));
        assert!(!l.iter().find(|b| b.action == BindingAction::MuteToggle).unwrap().enabled);
        // Save → load gives the saved list.
        let list = vec![bind(GestureKind::SwipeLeft, 4, open("https://a.example"))];
        save(&db, list).unwrap();
        assert!(is_customised(&db));
        let got = load(&db);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].trigger.kind, GestureKind::SwipeLeft);
        // Garbage → legacy, never empty.
        crate::settings::set(&db, KEY_BINDINGS, "not json").unwrap();
        assert_eq!(load(&db).len(), 5);
        // Reset → legacy again, and no longer customised.
        reset(&db).unwrap();
        assert!(!is_customised(&db));
        assert_eq!(load(&db).len(), 5);
    }
    #[test]
    fn the_editor_mirrors_the_rules_and_offers_every_option() {
        let ts = include_str!("../../../frontend/src/lib/gesture-bindings.ts");
        assert!(ts.contains(&format!("export const MIN_FINGERS = {MIN_FINGERS};")));
        assert!(ts.contains(&format!("export const MAX_FINGERS = {MAX_FINGERS};")));
        assert!(ts.contains(&format!("export const TIPTAP_FINGERS = {TIPTAP_FINGERS};")));
        let kinds = [
            GestureKind::SwipeUp,
            GestureKind::SwipeDown,
            GestureKind::SwipeLeft,
            GestureKind::SwipeRight,
            GestureKind::Tap,
            GestureKind::TipTapLeft,
            GestureKind::TipTapRight,
        ];
        for k in kinds {
            let name = serde_json::to_value(k).unwrap();
            assert!(ts.contains(&format!("{{ kind: {name}, label:")), "no editor entry for {name}");
        }
        let actions = [
            BindingAction::VolumeUp,
            BindingAction::VolumeDown,
            BindingAction::MuteToggle,
            BindingAction::NextTab,
            BindingAction::PrevTab,
            BindingAction::Hotkey { action: String::new() },
            BindingAction::Shortcut { keys: String::new() },
            BindingAction::Open { target: String::new() },
            BindingAction::Task { id: 0 },
        ];
        for a in actions {
            let ty = serde_json::to_value(&a).unwrap()["type"].clone();
            assert!(ts.contains(&format!("{{ type: {ty}, label:")), "no editor entry for {ty}");
        }
    }

}
