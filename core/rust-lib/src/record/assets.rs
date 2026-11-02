//! Bundled assets of the recorder's editor: cursor sprite themes, click sounds and wallpapers.
//!
//! Layout under the asset root (`Contents/Resources/record/` in the bundle,
//! `core/rust-lib/assets/record/` in dev and tests):
//!
//! ```text
//! cursors/themes.json            hotspots (fractions of the sprite image)
//! cursors/<theme>/<state>.png    sprites; a theme ships arrow + pointer, `default` all 16
//! sounds/{click-down,click-up}.wav
//! wallpapers/NN.jpg, thumbs/NN.jpg   our own generated backgrounds (see `gen_record_wallpapers`)
//! ```
//!
//! The sprite resolution mirrors OpenScreen's `resolveCursorSprites`: the built-in set is the
//! base, a theme lays its own art over `arrow`/`pointer`, and the kinds a project draws "as the
//! arrow" take the arrow's sprite.

use std::path::{Path, PathBuf};

/// The 16 cursor states the recorder's telemetry can name (OpenScreen `NativeCursorType`).
pub const CURSOR_STATES: [&str; 16] = [
    "arrow",
    "text",
    "pointer",
    "crosshair",
    "open-hand",
    "closed-hand",
    "resize-ew",
    "resize-ns",
    "resize-nesw",
    "resize-nwse",
    "move",
    "not-allowed",
    "wait",
    "app-starting",
    "help",
    "up-arrow",
];

/// Cursor kinds a project can draw as the arrow, with the states each covers
/// (OpenScreen `CURSOR_KINDS`). `arrow` itself is not a kind: it is what the others turn into.
pub const CURSOR_KINDS: [(&str, &[&str]); 9] = [
    ("pointer", &["pointer"]),
    ("text", &["text"]),
    ("grab", &["open-hand", "closed-hand"]),
    ("resize", &["resize-ew", "resize-ns", "resize-nwse", "resize-nesw", "move"]),
    ("busy", &["wait", "app-starting"]),
    ("crosshair", &["crosshair"]),
    ("not-allowed", &["not-allowed"]),
    ("help", &["help"]),
    ("up-arrow", &["up-arrow"]),
];

/// The default "draw as arrow" selection: every kind except the pointer hand.
pub const AS_ARROW_DEFAULT: [&str; 8] =
    ["text", "grab", "resize", "busy", "crosshair", "not-allowed", "help", "up-arrow"];

const WALLPAPER_IDS: [&str; 8] = ["01", "02", "03", "04", "05", "06", "07", "08"];

/// One resolved cursor theme: for each state its sprite file and the hotspot as a 0..1 fraction
/// of that image.
#[derive(Debug, Clone, PartialEq)]
pub struct CursorTheme {
    pub id: String,
    /// `(state, sprite path, hotspot x fraction, hotspot y fraction)`, in `CURSOR_STATES` order.
    pub sprites: Vec<(String, PathBuf, f32, f32)>,
}

#[derive(Debug, Clone)]
pub struct AssetRoot {
    pub dir: PathBuf,
}

impl AssetRoot {
    /// The bundle's `Resources/record`, else the source tree (dev builds, `cargo run`).
    pub fn resolve(app: &tauri::AppHandle) -> AssetRoot {
        use tauri::Manager;
        if let Ok(res) = app.path().resource_dir() {
            let dir = res.join("record");
            if dir.join("cursors").is_dir() {
                return AssetRoot { dir };
            }
        }
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            tracing::warn!("record assets: the bundle has no record/ directory, using the source tree");
        });
        AssetRoot::dev()
    }

    /// The source tree's asset directory (tests and dev).
    pub fn dev() -> AssetRoot {
        AssetRoot { dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("record") }
    }

    pub fn wallpaper_ids() -> &'static [&'static str] {
        &WALLPAPER_IDS
    }

    /// The full-size wallpaper for `id`, only when the id is known and the file exists.
    pub fn wallpaper(&self, id: &str) -> Option<PathBuf> {
        if !WALLPAPER_IDS.contains(&id) {
            return None;
        }
        let p = self.dir.join("wallpapers").join(format!("{id}.jpg"));
        p.is_file().then_some(p)
    }

    /// The picker thumbnail for `id`.
    pub fn wallpaper_thumb(&self, id: &str) -> Option<PathBuf> {
        if !WALLPAPER_IDS.contains(&id) {
            return None;
        }
        let p = self.dir.join("wallpapers").join("thumbs").join(format!("{id}.jpg"));
        p.is_file().then_some(p)
    }

    /// `(click-down, click-up)`.
    pub fn click_sounds(&self) -> (PathBuf, PathBuf) {
        let s = self.dir.join("sounds");
        (s.join("click-down.wav"), s.join("click-up.wav"))
    }

    /// All theme ids, the built-in one first.
    pub fn cursor_theme_ids(&self) -> Vec<String> {
        let mut ids = vec!["default".to_string()];
        if let Some(themes) = self.themes_json().and_then(|j| j.get("themes").cloned()) {
            if let Some(o) = themes.as_object() {
                ids.extend(o.keys().cloned());
            }
        }
        ids
    }

    /// A theme with no kind drawn as the arrow.
    pub fn cursor_theme(&self, theme: &str) -> Option<CursorTheme> {
        self.cursor_theme_as_arrow(theme, &[])
    }

    /// A theme where the listed kinds (`CURSOR_KINDS` names) take the arrow's sprite.
    /// `None` for an unknown theme or an unreadable `themes.json`.
    pub fn cursor_theme_as_arrow(&self, theme: &str, as_arrow: &[&str]) -> Option<CursorTheme> {
        let json = self.themes_json()?;
        let frac = |v: &serde_json::Value| -> Option<(f32, f32)> {
            let a = v.as_array()?;
            Some((a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))
        };
        let defaults = json.get("default")?;
        let overrides = if theme == "default" { None } else { Some(json.get("themes")?.get(theme)?) };
        let mut sprites: Vec<(String, PathBuf, f32, f32)> = Vec::with_capacity(CURSOR_STATES.len());
        for state in CURSOR_STATES {
            let (owner, hot) = match overrides.and_then(|o| o.get(state)).and_then(frac) {
                Some(h) => (theme, h),
                None => ("default", frac(defaults.get(state)?)?),
            };
            let path = self.dir.join("cursors").join(owner).join(format!("{state}.png"));
            sprites.push((state.to_string(), path, hot.0, hot.1));
        }
        let arrow = sprites.iter().find(|s| s.0 == "arrow").cloned()?;
        for (kind, states) in CURSOR_KINDS {
            if !as_arrow.contains(&kind) {
                continue;
            }
            for s in sprites.iter_mut().filter(|s| states.contains(&s.0.as_str())) {
                s.1 = arrow.1.clone();
                s.2 = arrow.2;
                s.3 = arrow.3;
            }
        }
        Some(CursorTheme { id: theme.to_string(), sprites })
    }

    fn themes_json(&self) -> Option<serde_json::Value> {
        let raw = std::fs::read_to_string(self.dir.join("cursors").join("themes.json")).ok()?;
        serde_json::from_str(&raw).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> AssetRoot {
        AssetRoot::dev()
    }

    #[test]
    fn every_theme_resolves_all_16_states() {
        let r = root();
        let ids = r.cursor_theme_ids();
        assert_eq!(ids.len(), 7, "{ids:?}");
        for id in ["default", "studio-ink", "prism-glow", "pop-coral", "pixel-candy", "star-sprout", "glass-lens"] {
            assert!(ids.iter().any(|i| i == id), "missing {id}");
            let t = r.cursor_theme(id).unwrap_or_else(|| panic!("theme {id}"));
            assert_eq!(t.sprites.len(), 16);
            for (state, path, hx, hy) in &t.sprites {
                assert!(path.is_file(), "{id}/{state}: {path:?}");
                assert!((0.0..=1.0).contains(hx) && (0.0..=1.0).contains(hy), "{id}/{state} hotspot {hx},{hy}");
            }
        }
        assert!(r.cursor_theme("no-such-theme").is_none());
    }

    #[test]
    fn themes_override_arrow_and_pointer_only() {
        let t = root().cursor_theme("studio-ink").unwrap();
        let get = |s: &str| t.sprites.iter().find(|x| x.0 == s).unwrap().clone();
        assert!(get("arrow").1.to_string_lossy().contains("studio-ink"));
        assert!(get("pointer").1.to_string_lossy().contains("studio-ink"));
        assert!(get("text").1.to_string_lossy().contains("default"));
        // Upstream value 6.2304 / 32.
        assert!((get("arrow").2 - 0.1947).abs() < 1e-4);
    }

    #[test]
    fn as_arrow_states_use_arrow_sprite() {
        let r = root();
        for id in ["default", "glass-lens"] {
            let t = r.cursor_theme_as_arrow(id, &AS_ARROW_DEFAULT).unwrap();
            let arrow = t.sprites.iter().find(|s| s.0 == "arrow").unwrap().clone();
            for (state, path, hx, hy) in &t.sprites {
                if *state == "pointer" {
                    assert_ne!(path, &arrow.1, "the hand stays a hand");
                    continue;
                }
                assert_eq!((path, hx, hy), (&arrow.1, &arrow.2, &arrow.3), "{id}/{state}");
            }
        }
        // Without the selection nothing changes.
        let plain = r.cursor_theme("default").unwrap();
        assert_ne!(plain.sprites.iter().find(|s| s.0 == "text").unwrap().1, plain.sprites[0].1);
        // A partial selection only touches its own states.
        let t = r.cursor_theme_as_arrow("default", &["resize"]).unwrap();
        let p = |s: &str| t.sprites.iter().find(|x| x.0 == s).unwrap().1.clone();
        assert_eq!(p("move"), p("arrow"));
        assert_ne!(p("text"), p("arrow"));
    }

    #[test]
    fn wallpaper_ids_all_exist() {
        let r = root();
        assert_eq!(AssetRoot::wallpaper_ids().len(), 8);
        for id in AssetRoot::wallpaper_ids() {
            let w = r.wallpaper(id).unwrap_or_else(|| panic!("wallpaper {id}"));
            let t = r.wallpaper_thumb(id).unwrap_or_else(|| panic!("thumb {id}"));
            assert!(std::fs::metadata(w).unwrap().len() > 10_000);
            assert!(std::fs::metadata(t).unwrap().len() > 500);
        }
        assert!(r.wallpaper("99").is_none());
        assert!(r.wallpaper("../01").is_none());
    }

    #[test]
    fn click_sounds_exist() {
        let (down, up) = root().click_sounds();
        for p in [down, up] {
            let b = std::fs::read(&p).unwrap_or_else(|e| panic!("{p:?}: {e}"));
            assert_eq!(&b[0..4], b"RIFF");
            assert_eq!(&b[8..12], b"WAVE");
        }
    }

    #[test]
    fn wallpapers_total_under_4_mb() {
        let dir = root().dir.join("wallpapers");
        let mut total = 0u64;
        for e in walk(&dir) {
            total += std::fs::metadata(e).unwrap().len();
        }
        assert!(total < 4_000_000, "{total}");
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = vec![];
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
        out
    }

    // ---- wallpaper generator (run on demand, output is committed) -------------------------

    struct Blob {
        color: [f32; 3],
        cx: f32,
        cy: f32,
        r: f32,
        weight: f32,
    }

    fn b(color: [u8; 3], cx: f32, cy: f32, r: f32, weight: f32) -> Blob {
        Blob { color: [color[0] as f32, color[1] as f32, color[2] as f32], cx, cy, r, weight }
    }

    /// Soft mesh gradient: a base colour plus gaussian colour blobs, blended by weight, with
    /// a hair of dither noise against banding. Fully deterministic.
    fn render(seed: u64, base: [u8; 3], blobs: &[Blob], w: u32, h: u32) -> image::RgbImage {
        let base = [base[0] as f32, base[1] as f32, base[2] as f32];
        let mut img = image::RgbImage::new(w, h);
        let aspect = w as f32 / h as f32;
        for y in 0..h {
            for x in 0..w {
                let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
                let (mut acc, mut wsum) = ([base[0] * 0.15, base[1] * 0.15, base[2] * 0.15], 0.15f32);
                for bl in blobs {
                    let (dx, dy) = ((u - bl.cx) * aspect, v - bl.cy);
                    let g = bl.weight * (-(dx * dx + dy * dy) / (2.0 * bl.r * bl.r)).exp();
                    for (a, col) in acc.iter_mut().zip(bl.color) {
                        *a += col * g;
                    }
                    wsum += g;
                }
                let mut z = seed ^ ((y as u64) << 32 | x as u64);
                z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                z ^= z >> 31;
                let noise = ((z & 0xFFFF) as f32 / 65535.0 - 0.5) * 1.6;
                let px: Vec<u8> = (0..3).map(|c| (acc[c] / wsum + noise).round().clamp(0.0, 255.0) as u8).collect();
                img.put_pixel(x, y, image::Rgb([px[0], px[1], px[2]]));
            }
        }
        img
    }

    /// `cargo test --release -p inspector-rust-core --lib gen_record_wallpapers -- --ignored`
    /// rewrites the eight wallpapers and their thumbnails in the source tree.
    #[test]
    #[ignore]
    fn gen_record_wallpapers() {
        use image::codecs::jpeg::JpegEncoder;
        let out = root().dir.join("wallpapers");
        std::fs::create_dir_all(out.join("thumbs")).unwrap();
        // (base, blobs). Four dark motifs (01-04), four light (05-08).
        let motifs: Vec<([u8; 3], Vec<Blob>)> = vec![
            ([14, 16, 48], vec![b([88, 70, 220], 0.2, 0.25, 0.32, 1.0), b([36, 120, 230], 0.8, 0.75, 0.36, 0.9), b([160, 60, 200], 0.7, 0.15, 0.22, 0.6)]),
            ([6, 36, 44], vec![b([14, 150, 150], 0.25, 0.8, 0.34, 1.0), b([20, 90, 160], 0.75, 0.3, 0.4, 0.9), b([60, 200, 150], 0.9, 0.9, 0.2, 0.5)]),
            ([30, 14, 34], vec![b([200, 50, 120], 0.15, 0.8, 0.3, 0.9), b([110, 40, 170], 0.6, 0.2, 0.38, 1.0), b([240, 110, 90], 0.9, 0.7, 0.22, 0.6), b([60, 40, 120], 0.45, 0.6, 0.3, 0.7)]),
            ([14, 26, 20], vec![b([30, 120, 80], 0.3, 0.3, 0.36, 1.0), b([20, 70, 90], 0.8, 0.8, 0.4, 0.9), b([140, 170, 60], 0.9, 0.2, 0.18, 0.45)]),
            ([252, 232, 214], vec![b([255, 170, 140], 0.2, 0.3, 0.34, 1.0), b([255, 214, 150], 0.8, 0.7, 0.36, 0.9), b([250, 140, 170], 0.75, 0.15, 0.2, 0.6)]),
            ([218, 240, 240], vec![b([150, 220, 230], 0.25, 0.7, 0.36, 1.0), b([170, 200, 255], 0.8, 0.25, 0.36, 0.9), b([190, 240, 205], 0.55, 0.95, 0.24, 0.6)]),
            ([240, 226, 250], vec![b([210, 170, 250], 0.2, 0.2, 0.34, 1.0), b([250, 180, 220], 0.8, 0.8, 0.38, 0.9), b([170, 190, 250], 0.85, 0.2, 0.22, 0.6), b([255, 215, 235], 0.4, 0.7, 0.26, 0.6)]),
            ([248, 238, 214], vec![b([245, 200, 120], 0.3, 0.75, 0.36, 1.0), b([240, 170, 130], 0.8, 0.3, 0.34, 0.9), b([210, 225, 150], 0.1, 0.1, 0.22, 0.55)]),
        ];
        for (i, (base, blobs)) in motifs.iter().enumerate() {
            let id = WALLPAPER_IDS[i];
            let img = render(0xA5A5_0000 + i as u64, *base, blobs, 2560, 1440);
            let mut buf = Vec::new();
            JpegEncoder::new_with_quality(&mut buf, 85).encode_image(&img).unwrap();
            std::fs::write(out.join(format!("{id}.jpg")), &buf).unwrap();
            let thumb = image::imageops::resize(&img, 320, 180, image::imageops::FilterType::Triangle);
            let mut tb = Vec::new();
            JpegEncoder::new_with_quality(&mut tb, 85).encode_image(&thumb).unwrap();
            std::fs::write(out.join("thumbs").join(format!("{id}.jpg")), &tb).unwrap();
        }
    }
}
