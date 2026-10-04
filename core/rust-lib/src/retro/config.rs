//! Settings model of the retro overlay: per-mode settings, clamping, built-in
//! and user presets, persistence (JSON in the settings table).
//!
//! Each mode keeps its OWN settings slot. Switching the mode therefore loads
//! that mode's values — its defaults until the user changed them, their
//! changes afterwards ("Moduswechsel lädt die Standardwerte, sofern nicht
//! angepasst"). Reset puts only the active slot back to the defaults.

use serde::{Deserialize, Serialize};

use super::{find_palette, palettes, Reduce};
use crate::db::DbHandle;

pub const KEY_CONFIG: &str = "retro.config";
pub const KEY_PRESETS: &str = "retro.presets";
pub const MAX_PRESET_NAME: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Mode {
    #[default]
    #[serde(rename = "8bit")]
    Eight,
    #[serde(rename = "16bit")]
    Sixteen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Dither {
    Off,
    Bayer2,
    #[default]
    Bayer4,
    Bayer8,
}

impl Dither {
    /// Matrix size, `None` = off.
    pub fn size(self) -> Option<usize> {
        match self {
            Dither::Off => None,
            Dither::Bayer2 => Some(2),
            Dither::Bayer4 => Some(4),
            Dither::Bayer8 => Some(8),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LensView {
    #[default]
    Original,
    Focus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    #[default]
    Current,
    All,
}

/// Everything that differs per mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModeSettings {
    pub palette: String,
    pub pixel_pt: f32,
    pub dither: Dither,
    /// 0..=100
    pub dither_strength: u8,
    pub focus: bool,
    /// The active window stays the REAL window (a transparent hole in the
    /// overlay — instant, sharp, clickable as seen). `false` = the older
    /// "finer pixels" rendering with `focus_pixel_pt`.
    pub focus_native: bool,
    pub focus_pixel_pt: f32,
    pub focus_border: bool,
    pub lens: bool,
    pub lens_radius_pt: u32,
    pub lens_view: LensView,
    pub scanlines: bool,
    pub scanline_intensity: u8,
    pub crt: bool,
    pub crt_strength: u8,
    /// 50..=100
    pub opacity: u8,
    /// Stage 2
    pub retro_frames: bool,
    pub sprite_cursor: bool,
}

impl Default for ModeSettings {
    fn default() -> Self {
        Self::defaults(Mode::Eight)
    }
}

/// Allowed pixel-size range per mode, in pt (step 0.5).
pub fn pixel_range(mode: Mode) -> (f32, f32) {
    match mode {
        Mode::Eight => (3.0, 12.0),
        Mode::Sixteen => (1.0, 4.0),
    }
}

pub const FOCUS_PX_RANGE: (f32, f32) = (1.0, 4.0);
pub const LENS_RANGE: (u32, u32) = (40, 200);
pub const OPACITY_RANGE: (u8, u8) = (50, 100);

fn clamp_half(v: f32, (lo, hi): (f32, f32), fallback: f32) -> f32 {
    if !v.is_finite() {
        return fallback;
    }
    ((v * 2.0).round() / 2.0).clamp(lo, hi)
}

/// Does palette `id` belong to `mode`?
pub fn palette_fits(id: &str, mode: Mode) -> bool {
    find_palette(id).is_some_and(|p| p.eight_bit == (mode == Mode::Eight))
}

/// The mode a palette implies.
pub fn mode_of_palette(id: &str) -> Option<Mode> {
    find_palette(id).map(|p| if p.eight_bit { Mode::Eight } else { Mode::Sixteen })
}

impl ModeSettings {
    pub fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Eight => Self {
                palette: "nes".into(),
                pixel_pt: 4.0,
                dither: Dither::Bayer4,
                dither_strength: 60,
                focus: true,
                focus_native: true,
                focus_pixel_pt: 1.5,
                focus_border: true,
                lens: false,
                lens_radius_pt: 80,
                lens_view: LensView::Original,
                scanlines: true,
                scanline_intensity: 40,
                crt: false,
                crt_strength: 40,
                opacity: 100,
                retro_frames: false,
                sprite_cursor: false,
            },
            Mode::Sixteen => Self {
                palette: "snes".into(),
                pixel_pt: 2.0,
                dither: Dither::Bayer4,
                dither_strength: 25,
                focus: true,
                focus_native: true,
                focus_pixel_pt: 1.0,
                focus_border: false,
                lens: false,
                lens_radius_pt: 80,
                lens_view: LensView::Original,
                scanlines: false,
                scanline_intensity: 30,
                crt: false,
                crt_strength: 30,
                opacity: 100,
                retro_frames: false,
                sprite_cursor: false,
            },
        }
    }

    /// Bring every field into the range of `mode`; a palette of the other
    /// mode (or an unknown one) falls back to the mode's default palette.
    pub fn clamped(mut self, mode: Mode) -> Self {
        let d = Self::defaults(mode);
        let id = self.palette.trim().to_lowercase();
        self.palette = match find_palette(&id) {
            Some(p) if palette_fits(&p.id, mode) => p.id.clone(),
            _ => d.palette.clone(),
        };
        self.pixel_pt = clamp_half(self.pixel_pt, pixel_range(mode), d.pixel_pt);
        self.dither_strength = self.dither_strength.min(100);
        self.focus_pixel_pt = clamp_half(self.focus_pixel_pt, FOCUS_PX_RANGE, d.focus_pixel_pt);
        self.lens_radius_pt = self.lens_radius_pt.clamp(LENS_RANGE.0, LENS_RANGE.1);
        self.scanline_intensity = self.scanline_intensity.min(100);
        self.crt_strength = self.crt_strength.min(100);
        self.opacity = self.opacity.clamp(OPACITY_RANGE.0, OPACITY_RANGE.1);
        self
    }

    /// The colour reduction of this setting.
    pub fn reduce(&self) -> Reduce {
        find_palette(&self.palette)
            .map(|p| p.reduce.clone())
            .unwrap_or(Reduce::Depth { bits: 5 })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RetroConfig {
    pub mode: Mode,
    pub eight: ModeSettings,
    pub sixteen: ModeSettings,
    pub target: Target,
    /// 30 or 60
    pub fps: u32,
    /// Fade out while typing, scrolling or dragging; back after a pause.
    pub retreat: bool,
}

impl Default for RetroConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Eight,
            eight: ModeSettings::defaults(Mode::Eight),
            sixteen: ModeSettings::defaults(Mode::Sixteen),
            target: Target::Current,
            fps: 60,
            retreat: true,
        }
    }
}

impl RetroConfig {
    pub fn active(&self) -> &ModeSettings {
        match self.mode {
            Mode::Eight => &self.eight,
            Mode::Sixteen => &self.sixteen,
        }
    }

    pub fn active_mut(&mut self) -> &mut ModeSettings {
        match self.mode {
            Mode::Eight => &mut self.eight,
            Mode::Sixteen => &mut self.sixteen,
        }
    }

    pub fn clamped(mut self) -> Self {
        self.eight = self.eight.clamped(Mode::Eight);
        self.sixteen = self.sixteen.clamped(Mode::Sixteen);
        self.fps = if self.fps <= 30 { 30 } else { 60 };
        self
    }

    /// Reset only the active mode to its defaults.
    pub fn reset_active(&mut self) {
        *self.active_mut() = ModeSettings::defaults(self.mode);
    }

    /// Select a palette; the mode follows from the palette.
    pub fn select_palette(&mut self, id: &str) -> Result<(), String> {
        let p = find_palette(id).ok_or_else(|| format!("Unbekannte Palette: {id}"))?;
        self.mode = if p.eight_bit { Mode::Eight } else { Mode::Sixteen };
        self.active_mut().palette = p.id.clone();
        Ok(())
    }

    pub fn apply_preset(&mut self, p: &Preset) {
        self.mode = p.mode;
        *self.active_mut() = p.settings.clone().clamped(p.mode);
    }
}

// ── Presets ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub mode: Mode,
    pub settings: ModeSettings,
    #[serde(default)]
    pub builtin: bool,
}

pub fn builtin_presets() -> Vec<Preset> {
    let show = ModeSettings::defaults(Mode::Eight); // NES, 4 pt, Bayer 4×4 60 %, scanlines, real active window
    let alltag = ModeSettings::defaults(Mode::Sixteen); // SNES, 2 pt, Bayer 4×4 25 %, real active window
    let arbeit = ModeSettings {
        palette: "pico8".into(),
        focus: true,
        focus_pixel_pt: 1.5,
        lens: true,
        lens_radius_pt: 80,
        ..ModeSettings::defaults(Mode::Eight)
    };
    vec![
        Preset { name: "Show".into(), mode: Mode::Eight, settings: show, builtin: true },
        Preset { name: "Alltag".into(), mode: Mode::Sixteen, settings: alltag, builtin: true },
        Preset { name: "Retro-Arbeit".into(), mode: Mode::Eight, settings: arbeit, builtin: true },
    ]
}

/// Pure: validate a preset name.
pub fn clean_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("Der Preset braucht einen Namen.".into());
    }
    if n.chars().count() > MAX_PRESET_NAME {
        return Err(format!("Höchstens {MAX_PRESET_NAME} Zeichen."));
    }
    if palettes().iter().any(|p| p.id == n.to_lowercase() || p.aliases.iter().any(|a| a.eq_ignore_ascii_case(n))) {
        return Err(format!("„{n}“ ist schon der Name einer Palette."));
    }
    if matches!(n.to_lowercase().as_str(), "on" | "off" | "focus" | "lens") {
        return Err(format!("„{n}“ ist ein Befehlswort."));
    }
    Ok(n.to_string())
}

/// Pure: add or replace (same name, case-insensitive) a user preset.
pub fn upsert_preset(user: &mut Vec<Preset>, p: Preset) -> Result<(), String> {
    let name = clean_name(&p.name)?;
    if builtin_presets().iter().any(|b| b.name.eq_ignore_ascii_case(&name)) {
        return Err(format!("„{name}“ ist ein mitgelieferter Preset."));
    }
    let p = Preset { name: name.clone(), builtin: false, settings: p.settings.clamped(p.mode), ..p };
    match user.iter_mut().find(|u| u.name.eq_ignore_ascii_case(&name)) {
        Some(slot) => *slot = p,
        None => user.push(p),
    }
    Ok(())
}

/// Pure: remove a user preset. Built-ins can't be deleted.
pub fn remove_preset(user: &mut Vec<Preset>, name: &str) -> Result<(), String> {
    if builtin_presets().iter().any(|b| b.name.eq_ignore_ascii_case(name.trim())) {
        return Err("Mitgelieferte Presets lassen sich nicht löschen.".into());
    }
    let before = user.len();
    user.retain(|u| !u.name.eq_ignore_ascii_case(name.trim()));
    if user.len() == before {
        return Err(format!("Kein Preset „{}“.", name.trim()));
    }
    Ok(())
}

/// Pure: find a preset (built-in first) by name, case-insensitive.
pub fn find_preset<'a>(all: &'a [Preset], name: &str) -> Option<&'a Preset> {
    all.iter().find(|p| p.name.eq_ignore_ascii_case(name.trim()))
}

// ── Persistence ─────────────────────────────────────────────────────────────

pub fn load(db: &DbHandle) -> RetroConfig {
    crate::settings::get(db, KEY_CONFIG)
        .ok()
        .flatten()
        .and_then(|s| parse_stored(&s))
        .unwrap_or_default()
        .clamped()
}

/// Pure: read a stored config. A slot saved before `focus_native` existed
/// gets the active window as the real window switched ON — the overlay used
/// to cover it, which made the computer hard to use (the reason the option
/// exists). The user can switch it off again; that choice is then stored.
pub fn parse_stored(json: &str) -> Option<RetroConfig> {
    let raw: serde_json::Value = serde_json::from_str(json).ok()?;
    let mut cfg: RetroConfig = serde_json::from_value(raw.clone()).ok()?;
    for (key, slot) in [("eight", &mut cfg.eight), ("sixteen", &mut cfg.sixteen)] {
        if raw.get(key).and_then(|v| v.get("focus_native")).is_none() {
            slot.focus = true;
            slot.focus_native = true;
        }
    }
    Some(cfg)
}

pub fn save(db: &DbHandle, cfg: &RetroConfig) -> anyhow::Result<RetroConfig> {
    let c = cfg.clone().clamped();
    crate::settings::set(db, KEY_CONFIG, &serde_json::to_string(&c)?)?;
    Ok(c)
}

pub fn user_presets(db: &DbHandle) -> Vec<Preset> {
    crate::settings::get(db, KEY_PRESETS)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<Vec<Preset>>(&s).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| !p.builtin)
        .map(|p| Preset { settings: p.settings.clone().clamped(p.mode), ..p })
        .collect()
}

pub fn all_presets(db: &DbHandle) -> Vec<Preset> {
    let mut v = builtin_presets();
    v.extend(user_presets(db));
    v
}

pub fn save_user_presets(db: &DbHandle, user: &[Preset]) -> anyhow::Result<()> {
    crate::settings::set(db, KEY_PRESETS, &serde_json::to_string(user)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same numbers as `lib/retro.ts` (its test pins the TS side).
    #[test]
    fn ranges_mirror_the_frontend() {
        assert_eq!(pixel_range(Mode::Eight), (3.0, 12.0));
        assert_eq!(pixel_range(Mode::Sixteen), (1.0, 4.0));
        assert_eq!(FOCUS_PX_RANGE, (1.0, 4.0));
        assert_eq!(LENS_RANGE, (40, 200));
        assert_eq!(OPACITY_RANGE, (50, 100));
    }

    #[test]
    fn an_old_config_gets_the_real_active_window_once_and_a_new_choice_sticks() {
        // Stored before focus_native existed: focus was off in the 8-bit slot.
        let old = r#"{"mode":"8bit","eight":{"palette":"pico8","pixel_pt":4.0,"focus":false},"sixteen":{"palette":"amiga","focus":false},"target":"current","fps":60}"#;
        let c = parse_stored(old).unwrap();
        assert!(c.eight.focus && c.eight.focus_native);
        assert!(c.sixteen.focus && c.sixteen.focus_native);
        assert_eq!(c.eight.palette, "pico8", "everything else is kept");
        assert!(c.retreat, "retreat defaults on");
        // Saved with the new field: the user's choice wins.
        let mut chosen = c.clone();
        chosen.eight.focus = false;
        chosen.eight.focus_native = false;
        chosen.retreat = false;
        let back = parse_stored(&serde_json::to_string(&chosen).unwrap()).unwrap();
        assert!(!back.eight.focus && !back.eight.focus_native && !back.retreat);
        assert!(parse_stored("kaputt").is_none());
    }

    #[test]
    fn both_modes_default_to_the_real_active_window() {
        for m in [Mode::Eight, Mode::Sixteen] {
            let d = ModeSettings::defaults(m);
            assert!(d.focus && d.focus_native, "{m:?}");
        }
        for p in builtin_presets() {
            assert!(p.settings.focus && p.settings.focus_native, "{}", p.name);
        }
    }

    #[test]
    fn mode_switch_keeps_each_modes_own_values() {
        let mut c = RetroConfig::default();
        c.eight.pixel_pt = 6.0;
        c.mode = Mode::Sixteen;
        assert_eq!(c.active().palette, "snes"); // 16-bit defaults
        assert_eq!(c.active().pixel_pt, 2.0);
        c.mode = Mode::Eight;
        assert_eq!(c.active().pixel_pt, 6.0); // user's change survives
        c.reset_active();
        assert_eq!(c.active().pixel_pt, 4.0);
        assert_eq!(c.sixteen, ModeSettings::defaults(Mode::Sixteen));
    }

    #[test]
    fn clamping_per_mode() {
        let s = ModeSettings { pixel_pt: 1.2, palette: "snes".into(), ..ModeSettings::defaults(Mode::Eight) }.clamped(Mode::Eight);
        assert_eq!(s.pixel_pt, 3.0); // 8-bit minimum
        assert_eq!(s.palette, "nes"); // a 16-bit palette doesn't fit 8-bit
        let s = ModeSettings { pixel_pt: 2.74, ..ModeSettings::defaults(Mode::Sixteen) }.clamped(Mode::Sixteen);
        assert_eq!(s.pixel_pt, 2.5); // 0.5 steps
        let s = ModeSettings { pixel_pt: f32::NAN, opacity: 10, lens_radius_pt: 500, focus_pixel_pt: 9.0, dither_strength: 250, ..ModeSettings::defaults(Mode::Sixteen) }.clamped(Mode::Sixteen);
        assert_eq!((s.pixel_pt, s.opacity, s.lens_radius_pt, s.focus_pixel_pt, s.dither_strength), (2.0, 50, 200, 4.0, 100));
        let c = RetroConfig { fps: 45, ..RetroConfig::default() }.clamped();
        assert_eq!(c.fps, 60);
    }

    #[test]
    fn palette_selection_sets_the_mode() {
        let mut c = RetroConfig::default();
        c.select_palette("megadrive").unwrap();
        assert_eq!(c.mode, Mode::Sixteen);
        assert_eq!(c.sixteen.palette, "megadrive");
        c.select_palette("GB").unwrap();
        assert_eq!((c.mode, c.eight.palette.as_str()), (Mode::Eight, "gb"));
        assert!(c.select_palette("nope").is_err());
    }

    #[test]
    fn builtin_presets_match_the_brief() {
        let b = builtin_presets();
        let show = find_preset(&b, "show").unwrap();
        assert_eq!((show.mode, show.settings.palette.as_str(), show.settings.pixel_pt), (Mode::Eight, "nes", 4.0));
        assert_eq!((show.settings.dither, show.settings.dither_strength, show.settings.scanlines), (Dither::Bayer4, 60, true));
        // The active window stays real in every built-in (the computer must stay usable).
        assert!(show.settings.focus && show.settings.focus_native && !show.settings.lens);
        let a = find_preset(&b, "ALLTAG").unwrap();
        assert_eq!((a.mode, a.settings.palette.as_str(), a.settings.pixel_pt, a.settings.dither_strength), (Mode::Sixteen, "snes", 2.0, 25));
        assert!(a.settings.focus && a.settings.focus_native && !a.settings.scanlines && !a.settings.lens);
        assert_eq!(a.settings.focus_pixel_pt, 1.0);
        let r = find_preset(&b, "retro-arbeit").unwrap();
        assert_eq!((r.settings.palette.as_str(), r.settings.pixel_pt, r.settings.focus_pixel_pt, r.settings.lens_radius_pt), ("pico8", 4.0, 1.5, 80));
        assert!(r.settings.focus && r.settings.focus_native && r.settings.lens);
        // every built-in is already inside its mode's range
        for p in &b {
            assert_eq!(p.settings.clone().clamped(p.mode), p.settings);
        }
    }

    #[test]
    fn preset_save_replace_delete() {
        let mut user = Vec::new();
        let p = |name: &str, pt: f32| Preset { name: name.into(), mode: Mode::Eight, settings: ModeSettings { pixel_pt: pt, ..ModeSettings::defaults(Mode::Eight) }, builtin: false };
        upsert_preset(&mut user, p("Mein Look", 6.0)).unwrap();
        upsert_preset(&mut user, p("mein look", 8.0)).unwrap(); // replace, same name
        assert_eq!(user.len(), 1);
        assert_eq!(user[0].settings.pixel_pt, 8.0);
        assert!(upsert_preset(&mut user, p("Show", 5.0)).is_err()); // built-in
        assert!(upsert_preset(&mut user, p("  ", 5.0)).is_err());
        assert!(upsert_preset(&mut user, p("nes", 5.0)).is_err()); // palette name
        assert!(upsert_preset(&mut user, p("focus", 5.0)).is_err()); // command word
        assert!(upsert_preset(&mut user, p(&"x".repeat(41), 5.0)).is_err());
        assert!(remove_preset(&mut user, "Alltag").is_err());
        assert!(remove_preset(&mut user, "gibts nicht").is_err());
        remove_preset(&mut user, "MEIN LOOK").unwrap();
        assert!(user.is_empty());
    }

    #[test]
    fn applying_a_preset_switches_mode_and_slot() {
        let mut c = RetroConfig::default();
        let all = builtin_presets();
        c.apply_preset(find_preset(&all, "alltag").unwrap());
        assert_eq!(c.mode, Mode::Sixteen);
        assert!(c.active().focus);
        assert_eq!(c.eight, ModeSettings::defaults(Mode::Eight)); // other slot untouched
    }

    #[test]
    fn persistence_roundtrip_and_garbage_fallback() {
        let db: DbHandle = std::sync::Arc::new(parking_lot::Mutex::new(rusqlite::Connection::open_in_memory().unwrap()));
        crate::settings::init_table(&db).unwrap();
        assert_eq!(load(&db), RetroConfig::default());
        let mut c = RetroConfig { mode: Mode::Sixteen, ..RetroConfig::default() };
        c.sixteen.pixel_pt = 3.5;
        save(&db, &c).unwrap();
        assert_eq!(load(&db), c);
        crate::settings::set(&db, KEY_CONFIG, "{not json").unwrap();
        assert_eq!(load(&db), RetroConfig::default());
        // partial JSON fills the rest with defaults
        crate::settings::set(&db, KEY_CONFIG, r#"{"mode":"16bit"}"#).unwrap();
        assert_eq!(load(&db).active().palette, "snes");

        let mut user = Vec::new();
        upsert_preset(&mut user, Preset { name: "X".into(), mode: Mode::Sixteen, settings: ModeSettings::defaults(Mode::Sixteen), builtin: false }).unwrap();
        save_user_presets(&db, &user).unwrap();
        let all = all_presets(&db);
        assert_eq!(all.len(), 4);
        assert!(find_preset(&all, "x").is_some());
    }
}
