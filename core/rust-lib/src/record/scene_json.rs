//! Builds the OpenScreen compositor's `Scene` JSON from a project + editor document.
//!
//! Core must not depend on the compositor crate, so this emits JSON whose field names
//! mirror `vendor/openscreen-compositor/src/scene.rs` (serde camelCase) exactly — the port of
//! OpenScreen v2.1.2 `buildSceneDescription` (`src/native/sceneDescription.ts`) for one recording
//! with its camera: effects, background, `layout` (+ `layoutByClip`, `layoutRegions`) from
//! [`layout::composite_layout`] per clip, the camera file per clip and its `webcamEffect`, zoom
//! regions bounded by [`layout::max_zoom_scale`], the cursor with
//! its sprite map, and the click sound. Stage-7 fields stay off (`rotation: null`,
//! `model3d: false`, `depthOfField: false`) unless the document asks for them (stage 7).
//!
//! Files named by the document (own background image, own cursor, wallpaper and theme ids) are
//! never trusted: only a plain file name under `<project>/assets/`, only known ids — anything
//! else, or a file that is gone, falls back to the default. A bad reference costs the look,
//! never the helper.

use super::assets::{AssetRoot, CURSOR_KINDS, AS_ARROW_DEFAULT};
use super::click_cues::click_hits_by_source;
use super::editor::{
    self, take_dir, valid_take_id, zoom_scale, AnnotationBody, Aspect, BlurShape, Background, EditDoc, FocusMode, Frame, FrameTheme, TakeMeta,
    TakeMetas, WallpaperMotion, ZoomRotation, CameraBgMode, CameraMask, DEFAULT_CURSOR_THEME, DEFAULT_WALLPAPER, LAYOUT_TRANSITION_SEC, TAKES_DIR,
};
use super::layout::{self, LayoutPreset};
use super::project::{read_project, CursorProvider, CursorRecording, Project, RecordingMeta, CAMERA_FILE, CURSOR_FILE, MIC_FILE, SCREEN_FILE};
use std::collections::BTreeMap;
use serde_json::{json, Map, Value};
use std::path::{Component, Path, PathBuf};

/// Folder (inside the project) own background images and cursors are copied into.
pub const PROJECT_ASSETS_DIR: &str = "assets";
/// `ROUNDNESS_REFERENCE_PX`: the radius slider is in px of a 1080 frame.
pub const ROUNDNESS_REFERENCE_PX: f64 = 1080.0;

fn ms_to_sec(ms: u64) -> f64 {
    ms as f64 / 1000.0
}

/// The project's cursor telemetry, if it reads.
pub fn read_cursor(project_dir: &Path) -> Option<CursorRecording> {
    let raw = std::fs::read(project_dir.join(CURSOR_FILE)).ok()?;
    serde_json::from_slice(&raw).ok()
}

/// An inserted take as the scene needs it: its recording metadata and pointer telemetry.
#[derive(Debug, Clone, PartialEq)]
pub struct TakeSource {
    pub meta: RecordingMeta,
    pub cursor: Option<CursorRecording>,
}

/// The inserted takes of a project by id (`<project>/takes/<id>/`).
pub type TakeSources = BTreeMap<String, TakeSource>;

/// What the editor document needs of each take (lengths and sizes).
pub fn take_metas(takes: &TakeSources) -> TakeMetas {
    takes
        .iter()
        .map(|(id, t)| (id.clone(), TakeMeta::from_recording(&t.meta, has_cursor_data(t.cursor.as_ref()))))
        .collect()
}

/// Reads every inserted take under `<project>/takes/`: a folder with a plain id, a readable
/// `project.json` and a `screen.mp4`. Anything else is skipped (a clip naming it is dropped by
/// `editor::normalize`).
pub fn load_takes(project_dir: &Path) -> TakeSources {
    let Ok(rd) = std::fs::read_dir(project_dir.join(TAKES_DIR)) else { return TakeSources::new() };
    let mut out = TakeSources::new();
    for e in rd.flatten() {
        let id = e.file_name().to_string_lossy().into_owned();
        let dir = e.path();
        if !valid_take_id(&id) || !dir.is_dir() || !dir.join(SCREEN_FILE).is_file() {
            continue;
        }
        if let Ok(mut p) = read_project(&dir) {
            crate::record::project::drop_missing_camera(&mut p.recording, &dir);
            out.insert(id, TakeSource { meta: p.recording, cursor: read_cursor(&dir) });
        }
    }
    out
}

/// Clip `clip`'s layout under the camera layout `preset` (resolved for that clip: without a
/// camera it is `no-webcam`), with the clip's screen source size (`layoutForClip`).
pub fn clip_layout(
    meta: &RecordingMeta,
    takes: &TakeMetas,
    doc: &EditDoc,
    out: (u32, u32),
    clip: usize,
    preset: LayoutPreset,
) -> (layout::CompositeLayout, (u32, u32)) {
    let a = &doc.appearance;
    let even_border = doc.output.aspect == Aspect::Auto;
    let src = editor::clip_source_size(meta, takes, doc, clip);
    // `no-webcam` drops the camera; a clip without one lays out the screen alone under any
    // preset (`composite_layout`'s fallback), which is what resolving it would give.
    let camera = editor::clip_camera_box(meta, takes, doc, clip).filter(|_| preset != LayoutPreset::NoWebcam);
    let look = doc.camera.look(preset);
    (layout::composite_layout(out, src.0 as f64 / src.1 as f64, camera, &look, a.padding, a.frame, even_border), src)
}

/// One screen box per clip (fractions of `out`) with the clip's source size: the screen's
/// shape is the clip's recording × its crop, its place the clip's layout under the global camera
/// preset (a block layout makes it smaller). The scene (zoom limit) and the editor's focus
/// crosshair both take it from here.
pub fn screen_boxes(meta: &RecordingMeta, takes: &TakeMetas, doc: &EditDoc, out: (u32, u32)) -> Vec<(layout::Rect, (u32, u32))> {
    (0..doc.clips.len())
        .map(|i| {
            let (l, src) = clip_layout(meta, takes, doc, out, i, doc.camera.preset);
            (l.screen, src)
        })
        .collect()
}

/// Whether the recording has pointer data to draw (`provider` not none, any samples).
pub fn has_cursor_data(cursor: Option<&CursorRecording>) -> bool {
    cursor.is_some_and(|c| c.provider != CursorProvider::None && !c.samples.is_empty())
}

/// `<project>/assets/<name>` for a plain file name that exists — `None` for anything with a
/// separator, `..`, an absolute path, or a missing file. A project opened from anywhere may
/// carry a planted symlink, so `assets/` must be a real directory and the file a real file
/// (neither is followed): otherwise the asset counts as missing. Every reader (scene, peaks,
/// previews, presets) goes through here and inherits that.
pub fn project_asset(project_dir: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains(['/', '\\', '\0']) || name.starts_with('.') {
        return None;
    }
    let mut parts = Path::new(name).components();
    if !matches!((parts.next(), parts.next()), (Some(Component::Normal(_)), None)) {
        return None;
    }
    let assets = project_dir.join(PROJECT_ASSETS_DIR);
    if !std::fs::symlink_metadata(&assets).is_ok_and(|m| m.file_type().is_dir()) {
        return None;
    }
    let p = assets.join(name);
    std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_file()).then_some(p)
}

/// `0.1205f32` as `0.1205`, not `0.12049999833106995` (themes.json's decimals, as written).
fn f32_to_f64(v: f32) -> f64 {
    v.to_string().parse().unwrap_or(v as f64)
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn motion_name(m: WallpaperMotion) -> Option<&'static str> {
    match m {
        WallpaperMotion::None => None,
        WallpaperMotion::Drift => Some("drift"),
        WallpaperMotion::Aurora => Some("aurora"),
        WallpaperMotion::Waves => Some("waves"),
    }
}

/// `sceneBackground`: wallpaper / own image → `image`, gradient, colour. `motion` only on the
/// kinds that can move and only when set. A missing wallpaper or image → the default wallpaper.
fn background(project_dir: &Path, doc: &EditDoc, assets: &AssetRoot) -> Value {
    background_json(project_dir, &doc.appearance.background, doc.appearance.background_motion, assets)
}

fn background_json(project_dir: &Path, background: &Background, motion: WallpaperMotion, assets: &AssetRoot) -> Value {
    let default_image = || assets.wallpaper(DEFAULT_WALLPAPER);
    let mut bg = match background {
        Background::Color { color } => return json!({"kind": "color", "color": color}),
        Background::Gradient { angle_deg, stops } if !stops.is_empty() => {
            let angle = if angle_deg.is_finite() { *angle_deg } else { 135.0 };
            json!({"kind": "gradient", "angleDeg": angle, "stops": stops})
        }
        Background::Gradient { .. } => match default_image() {
            Some(p) => json!({"kind": "image", "path": path_str(&p)}),
            None => return json!({"kind": "color", "color": "#000000"}),
        },
        Background::Wallpaper { id } => match assets.wallpaper(id).or_else(default_image) {
            Some(p) => json!({"kind": "image", "path": path_str(&p)}),
            None => return json!({"kind": "color", "color": "#000000"}),
        },
        Background::Image { file } => match project_asset(project_dir, file).or_else(default_image) {
            Some(p) => json!({"kind": "image", "path": path_str(&p)}),
            None => return json!({"kind": "color", "color": "#000000"}),
        },
    };
    if let Some(m) = motion_name(motion) {
        bg["motion"] = json!(m);
    }
    bg
}

fn frame_name(f: Frame) -> Option<&'static str> {
    match f {
        Frame::None => None,
        Frame::Window => Some("window"),
        Frame::Laptop => Some("laptop"),
        Frame::Phone => Some("phone"),
        Frame::Monitor => Some("monitor"),
    }
}

fn effects(doc: &EditDoc) -> Value {
    let a = &doc.appearance;
    let framed = a.frame != Frame::None;
    let mut e = json!({
        "padding": a.padding / 100.0,
        "blur": a.background_blur,
        "shadow": a.shadow,
        "roundnessFrac": if a.padding > 0.0 || framed { a.radius_px / ROUNDNESS_REFERENCE_PX } else { 0.0 },
        "motionBlur": a.motion_blur,
        "depthOfField": a.depth_of_field && doc.zooms.iter().any(|z| z.rotation.is_some()),
    });
    if let Some(f) = frame_name(a.frame) {
        e["frame"] = json!(f);
        if a.frame_theme == FrameTheme::Dark {
            e["frameTheme"] = json!("dark");
        }
    }
    e
}

/// Themes with modelled (sculpted) arrow and pointer shapes in the compositor.
pub const SCULPTED_THEMES: [&str; 6] = ["studio-ink", "prism-glow", "pop-coral", "pixel-candy", "star-sprout", "glass-lens"];
/// The cursor states that have a sculpted model.
pub const SCULPTED_STATES: [&str; 2] = ["arrow", "pointer"];

/// `"<theme>/<state>"` for a sculpted theme/state, and only with the 3D cursor on.
pub fn sculpt_name(theme: &str, state: &str, model3d: bool) -> Option<String> {
    (model3d && SCULPTED_THEMES.contains(&theme) && SCULPTED_STATES.contains(&state)).then(|| format!("{theme}/{state}"))
}

/// `"glass-lens/<state>"` for the glass-lens arrow and pointer, with or without the 3D cursor.
pub fn glass_name(theme: &str, state: &str) -> Option<String> {
    (theme == "glass-lens" && SCULPTED_STATES.contains(&state)).then(|| format!("{theme}/{state}"))
}

/// The sprite map: the theme (unknown → default) with the default as-arrow kinds; an own cursor
/// (a plain file under `<project>/assets` that exists) takes the arrow and its as-arrow group.
fn cursor_sprites(project_dir: &Path, doc: &EditDoc, assets: &AssetRoot) -> Map<String, Value> {
    let theme = assets
        .cursor_theme_as_arrow(&doc.cursor.theme, &AS_ARROW_DEFAULT)
        .or_else(|| assets.cursor_theme_as_arrow(DEFAULT_CURSOR_THEME, &AS_ARROW_DEFAULT));
    let custom = doc.cursor.custom.as_ref().and_then(|c| project_asset(project_dir, &c.file).map(|p| (p, c)));
    let arrow_group: Vec<&str> = std::iter::once("arrow")
        .chain(CURSOR_KINDS.iter().filter(|(k, _)| AS_ARROW_DEFAULT.contains(k)).flat_map(|(_, s)| s.iter().copied()))
        .collect();
    let mut map = Map::new();
    for (state, path, hx, hy) in theme.map(|t| t.sprites).unwrap_or_default() {
        let sprite = match &custom {
            Some((p, c)) if arrow_group.contains(&state.as_str()) => {
                json!({"path": path_str(p), "hotspotX": c.hotspot_x, "hotspotY": c.hotspot_y})
            }
            _ => json!({"path": path_str(&path), "hotspotX": f32_to_f64(hx), "hotspotY": f32_to_f64(hy)}),
        };
        let mut sprite = sprite;
        // An own pointer image stands in for the arrow group: no modelled stand-in for those.
        let own = custom.is_some() && arrow_group.contains(&state.as_str());
        if !own {
            if let Some(n) = sculpt_name(&doc.cursor.theme, &state, doc.cursor.model3d) {
                sprite["sculpt"] = json!(n);
            }
            if let Some(n) = glass_name(&doc.cursor.theme, &state) {
                sprite["glass"] = json!(n);
            }
        }
        map.insert(state, sprite);
    }
    map
}

/// The compositor scene for `doc`, reading the project's `cursor.json`. Paths are absolute.
pub fn scene_json(project_dir: &Path, project: &Project, doc: &EditDoc, out: (u32, u32), fps: u32, assets: &AssetRoot) -> Value {
    let takes = load_takes(project_dir);
    scene_json_with(project_dir, project, doc, out, fps, assets, read_cursor(project_dir).as_ref(), &takes)
}

/// [`scene_json`] with the cursor telemetry already read (the editor session reads it once).
/// The mic rides along each clip as `extraAudio` (ruling R1: mic time = source time − offset,
/// so `offsetSec = mic.offset_ms / 1000`) and only when the recording has one. Without pointer
/// data the cursor is off and there is no click sound. Each clip names the files of its own
/// recording: the project's, or an inserted take's under `takes/<id>/` (from `takes`).
#[allow(clippy::too_many_arguments)]
pub fn scene_json_with(
    project_dir: &Path,
    project: &Project,
    doc: &EditDoc,
    out: (u32, u32),
    fps: u32,
    assets: &AssetRoot,
    cursor: Option<&CursorRecording>,
    takes: &TakeSources,
) -> Value {
    let meta = &project.recording;
    let metas = take_metas(takes);
    // A clip whose take is missing is not in the scene; regions are re-indexed onto the rest
    // (`editor::playable` collapses it in the programme, so the times agree).
    let index = editor::scene_clip_indices(doc, &metas);
    let kept = |clip: usize| index.get(clip).copied().flatten();
    // Does any clip in the scene have a camera? Without one the layout block stays exactly as
    // before the camera existed (no-webcam, size 1, rectangle).
    let any_camera = (0..doc.clips.len()).any(|i| kept(i).is_some() && editor::clip_camera(meta, &metas, doc, i).is_some());
    // A clip's recording: its folder, metadata and telemetry.
    let source_of = |source: Option<&str>| -> (PathBuf, &RecordingMeta, Option<&CursorRecording>) {
        match source.and_then(|id| takes.get(id).map(|t| (id, t))) {
            Some((id, t)) => (take_dir(project_dir, Some(id)), &t.meta, t.cursor.as_ref()),
            None => (project_dir.to_path_buf(), meta, cursor),
        }
    };
    let clips: Vec<Value> = doc
        .clips
        .iter()
        .enumerate()
        .filter(|(i, _)| kept(*i).is_some())
        .map(|(i, c)| {
            let (dir, m, _) = source_of(c.source.as_deref());
            // The same question the layout asks (`clip_camera`), so decoder and layout agree.
            let camera = editor::clip_camera(meta, &metas, doc, i);
            let mut clip = json!({
                "screenPath": path_str(&dir.join(SCREEN_FILE)),
                "webcamPath": camera.map_or(String::new(), |_| path_str(&dir.join(CAMERA_FILE))),
                "sourceStartSec": ms_to_sec(c.start_ms),
                "sourceEndSec": ms_to_sec(c.end_ms),
                // Camera time = screen time − offset (compositor convention).
                "webcamOffsetSec": camera.map_or(0.0, |(_, off)| off as f64 / 1000.0),
                "hasAudio": m.system_audio,
                "cursorPath": path_str(&dir.join(CURSOR_FILE)),
                // Patch 14 (ruling R6): with a mic, the mic is the voice (loudness-normalised)
                // and the system sound keeps its recorded level; without one, the screen
                // audio is normalised as upstream. The system level applies either way.
                "normalise": if m.mic.is_some() { "voice" } else { "screen" },
                "systemGainDb": doc.system_gain_db,
            });
            if let Some(mic) = &m.mic {
                clip["extraAudio"] = json!({
                    "path": dir.join(MIC_FILE).to_string_lossy(),
                    "offsetSec": mic.offset_ms as f64 / 1000.0,
                    "gainDb": doc.mic_gain_db,
                });
            }
            clip
        })
        .collect();
    let speed_regions: Vec<Value> = doc
        .speed
        .iter()
        .filter_map(|r| kept(r.clip).map(|ci| (ci, r)))
        .map(|(ci, r)| {
            json!({
                "clipIndex": ci,
                "startSec": ms_to_sec(r.start_ms),
                "endSec": ms_to_sec(r.end_ms),
                "speed": r.speed,
            })
        })
        .collect();
    let crop_by_clip: Vec<Value> = (0..doc.clips.len())
        .filter(|i| kept(*i).is_some())
        .map(|i| match doc.crop.get(i).copied().flatten() {
            Some(c) => json!({"x": c.x, "y": c.y, "width": c.w, "height": c.h}),
            None => Value::Null,
        })
        .collect();

    let layouts: Vec<(layout::CompositeLayout, (u32, u32))> = (0..doc.clips.len())
        .filter(|i| kept(*i).is_some())
        .map(|i| clip_layout(meta, &metas, doc, out, i, doc.camera.preset))
        .collect();
    let rects: Vec<(layout::Rect, (u32, u32))> = layouts.iter().map(|(l, src)| (l.screen, *src)).collect();
    let layout_by_clip: Vec<Value> = if any_camera {
        layouts.iter().map(|(l, _)| resolved_layout_json(l)).collect()
    } else {
        rects.iter().map(|(r, _)| json!({"screenRect": rect_json(r)})).collect()
    };

    let zoom_regions: Vec<Value> = doc
        .zooms
        .iter()
        .filter_map(|z| kept(z.clip).map(|ci| (ci, z)))
        .map(|(ci, z)| {
            let (rect, src) = rects[ci];
            let mut r = json!({
                "id": z.id,
                "clipIndex": ci,
                "startSec": ms_to_sec(z.start_ms),
                "endSec": ms_to_sec(z.end_ms),
                "scale": zoom_scale(z).min(layout::max_zoom_scale(rect, out, src)),
                "focusX": z.focus_x,
                "focusY": z.focus_y,
                "focusMode": match z.focus_mode { FocusMode::Auto => "auto", FocusMode::Manual => "manual" },
                "rotation": z.rotation.map(|r| match r {
                    ZoomRotation::Left => "left",
                    ZoomRotation::Right => "right",
                    ZoomRotation::Orbit => "orbit",
                }),
            });
            if z.hide_cursor {
                r["hideCursor"] = json!(true);
            }
            r
        })
        .collect();

    let mut annotations = annotations_json(project_dir, doc, &kept);
    // Captions after every annotation (painted last); none without a transcript or when off.
    annotations.extend(super::captions::caption_entries(doc, out, &kept));

    let c = &doc.cursor;
    // Drawn when any clip's recording has pointer data (a clip without plays none).
    let show = c.show
        && doc
            .clips
            .iter()
            .filter(|clip| !editor::take_missing(clip, &metas))
            .any(|clip| has_cursor_data(source_of(clip.source.as_deref()).2));
    let mut cursor_block = json!({
        "show": show,
        "autoHide": c.auto_hide,
        "size": c.size,
        "smoothing": c.smoothing,
        "motionBlur": c.motion_blur,
        "clickBounce": c.click_bounce,
        "model3d": c.model3d,
        "clipToBounds": false,
        "theme": c.theme,
        "cursorSprites": Value::Object(cursor_sprites(project_dir, doc, assets)),
    });
    if c.click_impact {
        cursor_block["clickImpact"] = json!(true);
    }

    let layout_block = if any_camera {
        camera_layout_json(doc, layouts.first().map(|(l, _)| l), layout_by_clip)
    } else {
        json!({
            "preset": "no-webcam",
            "webcamSize": 1.0,
            "webcamShape": "rectangle",
            "webcamMirror": false,
            "webcamPosition": null,
            "webcamReactiveZoom": false,
            "screenRect": rects.first().map_or(json!({"x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0}), |(r, _)| rect_json(r)),
            "layoutByClip": layout_by_clip,
        })
    };
    let mut scene = json!({
        "clips": clips,
        "layout": layout_block,
        "effects": effects(doc),
        "background": background(project_dir, doc, assets),
        "zoomRegions": zoom_regions,
        "speedRegions": speed_regions,
        "cursor": cursor_block,
        "cropByClip": crop_by_clip,
        "output": {"width": out.0, "height": out.1, "fps": fps},
    });

    if any_camera {
        let regions = layout_regions_json(meta, &metas, doc, out, &kept);
        if !regions.is_empty() {
            scene["layoutRegions"] = Value::Array(regions);
        }
        let bg = &doc.camera.background;
        // Omitted at `none`, like upstream. No `modelPath`: the render helper names its own
        // model (and ignores one sent here); without a usable model the compositor draws the
        // camera unchanged.
        if bg.mode != CameraBgMode::None {
            let mut e = json!({"mode": enum_name(&bg.mode), "blurIntensity": bg.blur});
            if bg.mode == CameraBgMode::Custom {
                e["background"] = background_json(project_dir, &bg.custom, WallpaperMotion::None, assets);
            }
            scene["webcamEffect"] = e;
        }
    }
    // Only when there are any: a document without annotations keeps its scene byte for byte.
    if !annotations.is_empty() {
        scene["annotations"] = Value::Array(annotations);
    }
    // On the programme as it plays: a track anchored to a missing take's clip is left out.
    let tracks = super::audio_tracks::project_tracks(&editor::playable(doc, &metas), project_dir);
    if !tracks.is_empty() {
        scene["audioTracks"] = serde_json::to_value(tracks).unwrap_or_else(|_| json!([]));
    }

    if show && c.click_sound {
        let level = 10f64.powf(c.click_sound_gain_db / 20.0);
        // On the programme as it plays (a missing take's clip lasts no time, see `playable`).
        let play = editor::playable(doc, &metas);
        let hits: Vec<Value> = click_hits_by_source(&play, |source| source_of(source).2.filter(|r| has_cursor_data(Some(r))))
            .iter()
            .map(|h| json!({"timeSec": ms_to_sec(h.prog_ms), "gain": h.gain * level, "release": h.release}))
            .collect();
        if !hits.is_empty() {
            let (down, up) = assets.click_sounds();
            scene["clickSound"] = json!({"downPath": path_str(&down), "upPath": path_str(&up), "hits": hits});
        }
    }
    scene
}

fn rect_json(r: &layout::Rect) -> Value {
    json!({"x": r.x, "y": r.y, "width": r.width, "height": r.height})
}

fn mask_name(m: CameraMask) -> &'static str {
    match m {
        CameraMask::Square => "square",
        CameraMask::Rectangle => "rectangle",
    }
}

/// `ResolvedClipLayout` (`resolvedLayoutOf`): absent keys for what the layout leaves open.
fn resolved_layout_json(l: &layout::CompositeLayout) -> Value {
    let mut v = json!({"screenRect": rect_json(&l.screen), "webcamShape": mask_name(l.mask), "screenCover": l.screen_cover});
    if let Some(c) = &l.camera {
        v["webcamRect"] = rect_json(c);
    }
    if let Some(r) = l.screen_radius_frac {
        v["screenRadiusFrac"] = json!(r);
    }
    if let Some(r) = l.camera_radius_frac {
        v["webcamRadiusFrac"] = json!(r);
    }
    v
}

/// `webcamReactiveZoom` gated by the preset: only picture-in-picture shrinks during a zoom.
fn reactive_zoom(preset: LayoutPreset, on: bool) -> bool {
    on && preset.supports_reactive_zoom()
}

/// The scene's `layout` with a camera somewhere in it: the camera style, the first clip's
/// resolved layout as the scalars (fallback and starting value), and every clip's layout.
fn camera_layout_json(doc: &EditDoc, first: Option<&layout::CompositeLayout>, layout_by_clip: Vec<Value>) -> Value {
    let c = &doc.camera;
    let mut v = json!({
        // Resolved globally: there is a camera in the scene, so the global preset stands.
        "preset": enum_name(&c.preset.resolve(true)),
        // `webcamSizeToFraction`.
        "webcamSize": c.size_pct.clamp(layout::CAMERA_SIZE_MIN, layout::CAMERA_SIZE_MAX) / 100.0,
        "webcamShape": mask_name(first.map_or(c.mask, |l| l.mask)),
        "webcamMirror": c.mirror,
        "webcamPosition": null,
        "webcamReactiveZoom": reactive_zoom(c.preset, c.reactive_zoom),
        "webcamAnchor": enum_name(&c.anchor),
        "screenRect": first.map_or(json!({"x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0}), |l| rect_json(&l.screen)),
        "screenCover": first.is_some_and(|l| l.screen_cover),
        "layoutByClip": layout_by_clip,
    });
    if let Some(k) = &c.crop {
        v["webcamCrop"] = json!({"x": k.x, "y": k.y, "width": k.w, "height": k.h});
    }
    if let Some(l) = first {
        if let Some(r) = &l.camera {
            v["webcamRect"] = rect_json(r);
        }
        if let Some(r) = l.screen_radius_frac {
            v["screenRadiusFrac"] = json!(r);
        }
        if let Some(r) = l.camera_radius_frac {
            v["webcamRadiusFrac"] = json!(r);
        }
    }
    v
}

/// The layout partition in source seconds (`sceneDescription.ts:1350-1420`). Only with layout
/// regions in the document; then every clip in the scene is partitioned: from its start the
/// global preset, from each of its regions on that region's preset, until the next or the clip's
/// end. A part after another morphs from it for `LAYOUT_TRANSITION_SEC` (at most half the part);
/// `keepScreenClipFromSec` = the end of the last earlier block-layout part (a zoom in flight keeps
/// its viewport), as upstream.
fn layout_regions_json(meta: &RecordingMeta, metas: &TakeMetas, doc: &EditDoc, out: (u32, u32), kept: &dyn Fn(usize) -> Option<usize>) -> Vec<Value> {
    if doc.layout_regions.is_empty() {
        return Vec::new();
    }
    let reactive = doc.camera.reactive_zoom;
    let mut res = Vec::new();
    for (i, clip) in doc.clips.iter().enumerate() {
        let Some(ci) = kept(i) else { continue };
        let has_camera = editor::clip_camera(meta, metas, doc, i).is_some();
        // (start, requested preset); a region on the clip's start replaces the global part.
        let mut starts: Vec<(u64, LayoutPreset)> = vec![(clip.start_ms, doc.camera.preset)];
        for r in doc.layout_regions.iter().filter(|r| r.clip == i && r.start_ms >= clip.start_ms && r.start_ms < clip.end_ms) {
            starts.retain(|(s, _)| *s != r.start_ms);
            starts.push((r.start_ms, r.preset));
        }
        starts.sort_by_key(|(s, _)| *s);
        let parts: Vec<(u64, u64, LayoutPreset)> = starts
            .iter()
            .enumerate()
            .map(|(k, &(s, p))| (s, starts.get(k + 1).map_or(clip.end_ms, |n| n.0), p))
            .filter(|(s, e, _)| e > s)
            .collect();
        let resolve = |p: LayoutPreset| resolved_layout_json(&clip_layout(meta, metas, doc, out, i, p).0);
        let mut keep: Option<f64> = None;
        for (k, &(s, e, requested)) in parts.iter().enumerate() {
            let preset = requested.resolve(has_camera);
            let (start, end) = (ms_to_sec(s), ms_to_sec(e));
            let mut v = json!({
                "clipIndex": ci,
                "startSec": start,
                "endSec": end,
                "preset": enum_name(&preset),
                "layout": resolve(requested),
                "webcamReactiveZoom": reactive_zoom(preset, reactive),
                "transitionStartSec": start,
                "transitionSec": 0.0,
            });
            if let Some(kf) = keep {
                v["keepScreenClipFromSec"] = json!(kf);
            }
            if k > 0 {
                let prev = parts[k - 1].2;
                v["fromLayout"] = resolve(prev);
                v["fromPreset"] = enum_name(&prev.resolve(has_camera));
                v["fromWebcamReactiveZoom"] = json!(reactive_zoom(prev, reactive));
                v["transitionSec"] = json!(LAYOUT_TRANSITION_SEC.min((end - start) / 2.0));
            }
            if has_camera && requested.is_block() {
                keep = Some(end);
            }
            res.push(v);
        }
    }
    res
}

/// Reference height a text annotation's font size is given in (OpenScreen `annotationScale.ts`):
/// the compositor draws `fontSizeRel × anchor height`, and text is anchored on the frame.
pub const TEXT_REFERENCE_PX: f64 = 1080.0;

fn pct(v: f64) -> f64 {
    v / 100.0
}

fn enum_name<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// The document's annotations as the compositor's `SceneAnnotation`s (`V/src/scene.rs`), the port
/// of `sceneDescription.ts`: box percent → fractions, times in the clip's source seconds, text /
/// image / arrow measured on the frame (`space: "frame"`), blur on the screen (no key). Sorted
/// ascending by `zIndex` (the compositor paints in list order). An annotation of a clip that is
/// not in the scene goes, so does an image that is not a plain file under `<project>/assets/`.
fn annotations_json(project_dir: &Path, doc: &EditDoc, kept: &dyn Fn(usize) -> Option<usize>) -> Vec<Value> {
    let mut out: Vec<(i32, Value)> = Vec::new();
    for a in &doc.annotations {
        let Some(ci) = kept(a.clip) else { continue };
        let mut v = json!({
            "id": a.id,
            "clipIndex": ci,
            "startSec": ms_to_sec(a.start_ms),
            "endSec": ms_to_sec(a.end_ms),
            "x": pct(a.x),
            "y": pct(a.y),
            "w": pct(a.w),
            "h": pct(a.h),
            "zIndex": a.z_index,
        });
        match &a.body {
            AnnotationBody::Text(t) => {
                v["kind"] = json!("text");
                v["space"] = json!("frame");
                v["text"] = json!({
                    "content": t.content,
                    "color": t.color,
                    "backgroundColor": t.background,
                    "fontSizeRel": t.font_size / TEXT_REFERENCE_PX,
                    "fontFamily": t.font_family,
                    "fontWeight": if t.bold { "bold" } else { "normal" },
                    "fontStyle": if t.italic { "italic" } else { "normal" },
                    "textDecoration": if t.underline { "underline" } else { "none" },
                    "textAlign": enum_name(&t.align),
                    "animation": enum_name(&t.animation),
                });
            }
            AnnotationBody::Image { file } => {
                let Some(path) = project_asset(project_dir, file) else { continue };
                v["kind"] = json!("image");
                v["space"] = json!("frame");
                v["imagePath"] = json!(path_str(&path));
            }
            AnnotationBody::Arrow { direction, color, stroke_width } => {
                v["kind"] = json!("figure");
                v["space"] = json!("frame");
                v["figure"] = json!({"direction": enum_name(direction), "color": color, "strokeWidth": stroke_width});
            }
            AnnotationBody::Blur(b) => {
                v["kind"] = json!("blur");
                let mut blur = json!({
                    "style": enum_name(&b.style),
                    "shape": enum_name(&b.shape),
                    "color": enum_name(&b.color),
                    "intensity": b.intensity,
                    "blockSize": b.block_size,
                });
                if b.shape == BlurShape::Freehand {
                    // Fractions of the annotation box (vendor patch 13 draws the polygon there).
                    blur["freehandPoints"] =
                        Value::Array(b.points.iter().map(|&(x, y)| json!({"x": pct(x), "y": pct(y)})).collect());
                }
                v["blur"] = blur;
            }
        }
        out.push((a.z_index, v));
    }
    out.sort_by_key(|(z, _)| *z);
    out.into_iter().map(|(_, v)| v).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::editor::{
        Appearance, CropRect, CursorStyle, CustomCursor, EditClip, OutputShape, OutputTarget, ZoomOrigin,
        ZoomRegion, EDITOR_VERSION,
    };
    use std::collections::HashSet;
    use crate::record::project::{CursorSample, Interaction, MicMeta, ProjectStatus, PROJECT_SCHEMA};

    const GOLDEN: &str = include_str!("fixtures/scene-golden.json");
    const GOLDEN_STYLED: &str = include_str!("fixtures/scene-golden-styled.json");
    const GOLDEN_3D: &str = include_str!("fixtures/scene-golden-3d.json");
    const GOLDEN_CAPTIONS: &str = include_str!("fixtures/scene-golden-captions.json");
    const DIR: &str = "/Users/someone/Movies/Inspector Rust/take-1";
    const PLACEHOLDER: &str = "<PROJECT>";
    const ASSETS_PLACEHOLDER: &str = "<ASSETS>";

    fn assets() -> AssetRoot {
        AssetRoot::dev()
    }

    fn project() -> Project {
        Project {
            schema: PROJECT_SCHEMA,
            id: "take-1".into(),
            title: String::new(),
            created_at: 0,
            updated_at: 0,
            status: ProjectStatus::Ready,
            failure: None,
            recording: RecordingMeta {
                width: 3024,
                height: 1964,
                fps: 60,
                duration_ms: 10_000,
                system_audio: true,
                mic: Some(MicMeta { offset_ms: 120, sample_rate: 48_000 }),
                ..RecordingMeta::default()
            },
            editor: json!({}),
        }
    }

    /// The stage-2 document as it sits in a project file (v1), upgraded by `migrate`.
    fn doc() -> EditDoc {
        let v1 = json!({
            "version": 1,
            "clips": [{"startMs": 0, "endMs": 4_000}, {"startMs": 6_500, "endMs": 10_000}],
            "speed": [
                {"clip": 0, "startMs": 1_000, "endMs": 3_000, "speed": 2.0},
                {"clip": 1, "startMs": 7_000, "endMs": 9_250, "speed": 0.5},
            ],
            "crop": [null, {"x": 0.25, "y": 0.0, "w": 0.5, "h": 1.0}],
            "output": {"aspect": "16:9"},
            "micGainDb": -6.0,
        });
        let d = editor::migrate(v1).expect("v1 migrates");
        assert_eq!(d.appearance, Appearance::plain());
        assert_eq!(d.cursor, CursorStyle::plain());
        d
    }

    fn sample(t: u64, cx: f64, cy: f64, kind: Option<Interaction>) -> CursorSample {
        CursorSample { time_ms: t, cx, cy, visible: None, asset_id: None, cursor_type: None, interaction_type: kind }
    }

    fn cursor_rec() -> CursorRecording {
        CursorRecording {
            version: 1,
            provider: CursorProvider::Native,
            samples: vec![
                sample(0, 0.5, 0.5, Some(Interaction::Move)),
                sample(1_500, 0.3, 0.4, Some(Interaction::Click)),
                sample(1_580, 0.3, 0.4, Some(Interaction::Mouseup)),
                sample(7_200, 0.7, 0.6, Some(Interaction::DoubleClick)),
                sample(8_000, 0.7, 0.6, Some(Interaction::RightClick)),
            ],
            assets: vec![],
        }
    }

    /// Every effect on: own wallpaper with motion, padding, radius, shadow, blur, motion blur,
    /// a dark window frame, Auto output, a themed cursor with click sound and impact, two zooms
    /// (one asking for more than the source can give).
    fn styled_doc() -> EditDoc {
        let mut d = doc();
        d.version = EDITOR_VERSION;
        d.output = OutputShape { aspect: Aspect::Auto };
        d.appearance = Appearance {
            background: Background::Wallpaper { id: "03".into() },
            background_motion: WallpaperMotion::Drift,
            padding: 40.0,
            radius_px: 24.0,
            shadow: 0.6,
            background_blur: 0.3,
            motion_blur: 0.2,
            frame: Frame::Window,
            frame_theme: FrameTheme::Dark,
            depth_of_field: true,
        };
        d.cursor = CursorStyle {
            show: true,
            auto_hide: true,
            theme: "studio-ink".into(),
            click_impact: true,
            click_sound: true,
            click_sound_gain_db: -6.0,
            ..CursorStyle::default()
        };
        d.zooms = vec![
            ZoomRegion {
                id: "z1".into(),
                clip: 0,
                start_ms: 1_000,
                end_ms: 3_000,
                depth: 3,
                custom_scale: None,
                focus_x: 0.3,
                focus_y: 0.4,
                focus_mode: FocusMode::Auto,
                origin: ZoomOrigin::Auto,
                hide_cursor: false,
                rotation: None,
            },
            ZoomRegion {
                id: "z2".into(),
                clip: 1,
                start_ms: 7_000,
                end_ms: 9_000,
                depth: 6,
                custom_scale: None,
                focus_x: 0.7,
                focus_y: 0.6,
                focus_mode: FocusMode::Manual,
                origin: ZoomOrigin::Manual,
                hide_cursor: true,
                rotation: None,
            },
        ];
        d.auto_zoom_done = true;
        editor::normalize(d, 10_000, &TakeMetas::new())
    }

    /// Recursively sorted keys + pretty print, so the text does not depend on whether
    /// serde_json's `preserve_order` is unified in by some other crate in the build.
    fn canonical(v: &Value) -> String {
        fn sort(v: &Value) -> Value {
            match v {
                Value::Object(m) => {
                    let mut keys: Vec<_> = m.keys().cloned().collect();
                    keys.sort();
                    let mut out = serde_json::Map::new();
                    for k in keys {
                        out.insert(k.clone(), sort(&m[&k]));
                    }
                    Value::Object(out)
                }
                Value::Array(a) => Value::Array(a.iter().map(sort).collect()),
                other => other.clone(),
            }
        }
        let mut s = serde_json::to_string_pretty(&sort(v)).unwrap();
        s.push('\n');
        s
    }

    fn placeholders(v: &Value) -> String {
        canonical(v).replace(DIR, PLACEHOLDER).replace(&path_str(&assets().dir), ASSETS_PLACEHOLDER)
    }

    fn check_golden(text: &str, golden: &str, file: &str) {
        if std::env::var_os("IR_WRITE_GOLDEN").is_some() {
            let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/record/fixtures").join(file);
            std::fs::write(p, text).unwrap();
            return;
        }
        assert_eq!(text, golden);
    }

    /// Regenerate with `IR_WRITE_GOLDEN=1 cargo test … scene_json`, then review the diff by
    /// eye against RECON-stage3 §1 and scene.rs before committing. A v1 document keeps its
    /// look: black background, no padding/effects, no cursor drawn, no zooms.
    #[test]
    fn scene_json_matches_golden() {
        let v = scene_json(Path::new(DIR), &project(), &doc(), (1920, 1080), 30, &assets());
        check_golden(&placeholders(&v), GOLDEN, "scene-golden.json");
    }

    #[test]
    fn styled_scene_matches_golden() {
        let d = styled_doc();
        let out = editor::output_size(&project().recording, &TakeMetas::new(), &d, OutputTarget::P1080);
        let v = scene_json_with(Path::new(DIR), &project(), &d, out, 30, &assets(), Some(&cursor_rec()), &TakeSources::new());
        check_golden(&placeholders(&v), GOLDEN_STYLED, "scene-golden-styled.json");
        // The parts the golden pins, spelled out.
        assert_eq!(v["effects"]["frame"], "window");
        assert_eq!(v["effects"]["frameTheme"], "dark");
        assert_eq!(v["effects"]["padding"], 0.4);
        assert!((v["effects"]["roundnessFrac"].as_f64().unwrap() - 24.0 / 1080.0).abs() < 1e-12);
        assert_eq!(v["background"]["kind"], "image");
        assert_eq!(v["background"]["motion"], "drift");
        assert!(v["background"]["path"].as_str().unwrap().ends_with("wallpapers/03.jpg"));
        assert_eq!(v["cursor"]["show"], true);
        assert_eq!(v["cursor"]["clickImpact"], true);
        assert_eq!(v["cursor"]["model3d"], false);
        assert_eq!(v["zoomRegions"][0]["rotation"], Value::Null);
        assert_eq!(v["zoomRegions"][1]["hideCursor"], true);
        assert!(v["zoomRegions"][0].get("hideCursor").is_none());
        // Depth 6 (5×) on a half-width crop is bounded by the source's magnification.
        let z2 = v["zoomRegions"][1]["scale"].as_f64().unwrap();
        assert!((1.0..5.0).contains(&z2), "{z2}");
        // Click sound: levelled by −6 dB, the programme clock (1.5 s src in clip 0 with 1–3 s ×2 → 1.25 s).
        let hits = v["clickSound"]["hits"].as_array().unwrap();
        assert!((hits[0]["timeSec"].as_f64().unwrap() - 1.25).abs() < 1e-9, "{}", hits[0]);
        assert!((hits[0]["gain"].as_f64().unwrap() - 10f64.powf(-6.0 / 20.0)).abs() < 1e-12);
        for c in v["clips"].as_array().unwrap() {
            assert_eq!(c["cursorPath"], format!("{DIR}/cursor.json"));
        }
        assert_eq!(v["layout"]["layoutByClip"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn scene_json_has_required_fields() {
        let v = scene_json(Path::new(DIR), &project(), &doc(), (1920, 1080), 30, &assets());
        let has = |obj: &Value, keys: &[&str], what: &str| {
            for k in keys {
                assert!(obj.get(*k).is_some(), "{what} lacks `{k}`: {obj}");
            }
        };
        has(&v, &["clips", "layout", "effects", "background", "zoomRegions", "cursor", "output"], "scene");
        has(&v["layout"], &["preset", "webcamSize", "webcamShape", "webcamMirror", "webcamPosition", "webcamReactiveZoom", "screenRect"], "layout");
        has(&v["effects"], &["padding", "blur", "shadow", "roundnessFrac", "motionBlur"], "effects");
        has(&v["background"], &["kind", "color"], "background");
        has(&v["cursor"], &["show", "size", "smoothing", "motionBlur", "clickBounce", "clipToBounds", "theme"], "cursor");
        has(&v["output"], &["width", "height", "fps"], "output");
        for c in v["clips"].as_array().unwrap() {
            has(c, &["screenPath", "webcamPath", "sourceStartSec", "sourceEndSec", "webcamOffsetSec", "hasAudio", "cursorPath"], "clip");
        }
        assert_eq!(v["layout"]["preset"], "no-webcam");
        assert_eq!(v["background"], json!({"kind": "color", "color": "#000000"}));
        assert_eq!(v["cursor"]["show"], false);
        assert_eq!(v["zoomRegions"], json!([]));
        assert!(v.get("clickSound").is_none());
        for k in ["padding", "blur", "shadow", "roundnessFrac", "motionBlur"] {
            assert_eq!(v["effects"][k].as_f64(), Some(0.0), "effects.{k}");
        }
        assert!(v["effects"].get("frame").is_none());
        assert_eq!(v["output"], json!({"width": 1920, "height": 1080, "fps": 30}));
    }

    #[test]
    fn scene_json_maps_clips_speed_crop_and_mic() {
        let v = scene_json(Path::new(DIR), &project(), &doc(), (1920, 1080), 30, &assets());
        let clips = v["clips"].as_array().unwrap();
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[1]["screenPath"], format!("{DIR}/screen.mp4"));
        assert_eq!(clips[1]["sourceStartSec"], 6.5);
        assert_eq!(clips[1]["sourceEndSec"], 10.0);
        assert_eq!(clips[0]["hasAudio"], true);
        // Ruling R1: mic time = source time − offset → offsetSec = offset_ms / 1000.
        assert_eq!(clips[0]["extraAudio"], json!({"path": format!("{DIR}/mic.wav"), "offsetSec": 0.12, "gainDb": -6.0}));
        assert_eq!(
            v["speedRegions"],
            json!([
                {"clipIndex": 0, "startSec": 1.0, "endSec": 3.0, "speed": 2.0},
                {"clipIndex": 1, "startSec": 7.0, "endSec": 9.25, "speed": 0.5},
            ])
        );
        assert_eq!(v["cropByClip"], json!([null, {"x": 0.25, "y": 0.0, "width": 0.5, "height": 1.0}]));
        // The cropped clip gets its own, narrower screen box.
        let r0 = v["layout"]["layoutByClip"][0]["screenRect"]["width"].as_f64().unwrap();
        let r1 = v["layout"]["layoutByClip"][1]["screenRect"]["width"].as_f64().unwrap();
        assert!(r1 < r0, "{r0} {r1}");
        assert_eq!(v["layout"]["screenRect"], v["layout"]["layoutByClip"][0]["screenRect"]);

        // No mic recorded → no extra audio at all (not an empty path).
        let mut p = project();
        p.recording.mic = None;
        p.recording.system_audio = false;
        let v = scene_json(Path::new(DIR), &p, &doc(), (1920, 1080), 30, &assets());
        for c in v["clips"].as_array().unwrap() {
            assert!(c.get("extraAudio").is_none(), "{c}");
            assert_eq!(c["hasAudio"], false);
        }
        // A negative offset (mic started before the video) stays negative.
        let mut p = project();
        p.recording.mic = Some(MicMeta { offset_ms: -250, sample_rate: 48_000 });
        let v = scene_json(Path::new(DIR), &p, &doc(), (1920, 1080), 30, &assets());
        assert_eq!(v["clips"][0]["extraAudio"]["offsetSec"], -0.25);
    }

    #[test]
    fn no_cursor_data_disables_cursor() {
        let d = styled_doc();
        let none = CursorRecording { version: 1, provider: CursorProvider::None, samples: vec![], assets: vec![] };
        let empty = CursorRecording { provider: CursorProvider::Native, ..none.clone() };
        let with_none_provider = CursorRecording { provider: CursorProvider::None, ..cursor_rec() };
        for rec in [None, Some(&none), Some(&empty), Some(&with_none_provider)] {
            let v = scene_json_with(Path::new(DIR), &project(), &d, (1920, 1080), 30, &assets(), rec, &TakeSources::new());
            assert_eq!(v["cursor"]["show"], false, "{rec:?}");
            assert!(v.get("clickSound").is_none(), "{rec:?}");
            // Zooms keep their focus mode; the compositor stays on the focus point.
            assert_eq!(v["zoomRegions"][0]["focusMode"], "auto");
        }
        // Position-only data still draws a pointer (no clicks → no click sound).
        let pos = CursorRecording { provider: CursorProvider::PositionOnly, samples: vec![sample(0, 0.5, 0.5, None)], ..none };
        let v = scene_json_with(Path::new(DIR), &project(), &d, (1920, 1080), 30, &assets(), Some(&pos), &TakeSources::new());
        assert_eq!(v["cursor"]["show"], true);
        assert!(v.get("clickSound").is_none());
    }

    fn tmp_project(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ir-scene-json-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(PROJECT_ASSETS_DIR)).unwrap();
        dir
    }

    #[test]
    fn missing_custom_image_falls_back() {
        let dir = tmp_project("image");
        std::fs::write(dir.join(PROJECT_ASSETS_DIR).join("bg.png"), b"png").unwrap();
        // A file outside assets/ that exists must still not be reachable.
        std::fs::write(dir.join("outside.png"), b"png").unwrap();
        let default = path_str(&assets().wallpaper(DEFAULT_WALLPAPER).unwrap());
        let bg_for = |file: &str, motion: WallpaperMotion| {
            let mut d = styled_doc();
            d.appearance.background = Background::Image { file: file.into() };
            d.appearance.background_motion = motion;
            scene_json_with(&dir, &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new())["background"].clone()
        };
        assert_eq!(bg_for("bg.png", WallpaperMotion::Waves), json!({"kind": "image", "path": path_str(&dir.join("assets/bg.png")), "motion": "waves"}));
        for bad in ["gone.png", "../outside.png", "/etc/passwd", "sub/bg.png", "..\\outside.png", "", ".", ".."] {
            assert_eq!(bg_for(bad, WallpaperMotion::None), json!({"kind": "image", "path": default}), "{bad:?}");
        }
        // Unknown wallpaper id, including traversal → the default wallpaper.
        for id in ["99", "../01", ""] {
            let mut d = styled_doc();
            d.appearance.background = Background::Wallpaper { id: id.into() };
            d.appearance.background_motion = WallpaperMotion::None;
            let v = scene_json_with(&dir, &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
            assert_eq!(v["background"], json!({"kind": "image", "path": default}), "{id:?}");
        }
        // A gradient and a colour pass through; a colour never carries motion.
        let mut d = styled_doc();
        d.appearance.background = Background::Gradient { angle_deg: f64::NAN, stops: vec!["#3b82f6".into(), "#8b5cf6".into()] };
        let v = scene_json_with(&dir, &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        assert_eq!(v["background"], json!({"kind": "gradient", "angleDeg": 135.0, "stops": ["#3b82f6", "#8b5cf6"], "motion": "drift"}));
        d.appearance.background = Background::Color { color: "#16171d".into() };
        let v = scene_json_with(&dir, &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        assert_eq!(v["background"], json!({"kind": "color", "color": "#16171d"}));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_custom_cursor_falls_back() {
        let dir = tmp_project("cursor");
        std::fs::write(dir.join(PROJECT_ASSETS_DIR).join("me.png"), b"png").unwrap();
        std::fs::write(dir.join("outside.png"), b"png").unwrap();
        let sprites = |custom: Option<&str>, theme: &str| {
            let mut d = styled_doc();
            d.cursor.theme = theme.into();
            d.cursor.custom = custom.map(|f| CustomCursor { file: f.into(), hotspot_x: 0.25, hotspot_y: 0.1 });
            scene_json_with(&dir, &project(), &d, (1920, 1080), 30, &assets(), Some(&cursor_rec()), &TakeSources::new())["cursor"]["cursorSprites"].clone()
        };
        let themed = sprites(None, "studio-ink");
        assert_eq!(themed.as_object().unwrap().len(), 16);
        assert!(themed["arrow"]["path"].as_str().unwrap().contains("studio-ink"));
        // As-arrow kinds take the arrow; the hand stays a hand.
        assert_eq!(themed["text"], themed["arrow"]);
        assert_ne!(themed["pointer"], themed["arrow"]);
        // An own cursor takes the arrow and its group.
        let own = sprites(Some("me.png"), "studio-ink");
        let me = json!({"path": path_str(&dir.join("assets/me.png")), "hotspotX": 0.25, "hotspotY": 0.1});
        assert_eq!(own["arrow"], me);
        assert_eq!(own["move"], me);
        assert_eq!(own["pointer"], themed["pointer"]);
        // Gone, or reaching outside assets/ → the theme.
        for bad in ["gone.png", "../outside.png", "/etc/hosts", "a/me.png"] {
            assert_eq!(sprites(Some(bad), "studio-ink"), themed, "{bad:?}");
        }
        // An unknown theme → the default theme.
        let unknown = sprites(None, "../../etc");
        assert_eq!(unknown, sprites(None, "default"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_assets_folder_or_asset_counts_as_missing() {
        use std::os::unix::fs::symlink;
        let outside = tmp_project("ln-outside");
        std::fs::write(outside.join("secret.png"), b"png").unwrap();
        // assets/ itself a link to somewhere else.
        let dir = tmp_project("ln-dir");
        std::fs::remove_dir(dir.join(PROJECT_ASSETS_DIR)).unwrap();
        symlink(&outside, dir.join(PROJECT_ASSETS_DIR)).unwrap();
        assert_eq!(project_asset(&dir, "secret.png"), None);
        let default = path_str(&assets().wallpaper(DEFAULT_WALLPAPER).unwrap());
        let mut d = styled_doc();
        d.appearance.background = Background::Image { file: "secret.png".into() };
        d.appearance.background_motion = WallpaperMotion::None;
        let v = scene_json_with(&dir, &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        assert_eq!(v["background"], json!({"kind": "image", "path": default}), "the scene falls back");
        // A real assets/ with one linked file inside.
        let dir2 = tmp_project("ln-file");
        symlink(outside.join("secret.png"), dir2.join(PROJECT_ASSETS_DIR).join("bg.png")).unwrap();
        assert_eq!(project_asset(&dir2, "bg.png"), None);
        for d in [&dir, &dir2, &outside] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn project_asset_accepts_only_plain_names() {
        let dir = tmp_project("names");
        std::fs::write(dir.join(PROJECT_ASSETS_DIR).join("ok.jpg"), b"x").unwrap();
        assert_eq!(project_asset(&dir, "ok.jpg"), Some(dir.join("assets/ok.jpg")));
        for bad in ["../x.png", "..", ".hidden", "a/b", "a\\b", "/abs", "x\0y", "missing.png"] {
            assert_eq!(project_asset(&dir, bad), None, "{bad:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crop_lowers_zoom_limit() {
        // A crop blows the source up at rest → a lower bound for the same depth-6 zoom.
        let scale_with = |crop: Option<CropRect>| {
            let mut d = styled_doc();
            d.crop = vec![None, crop];
            let v = scene_json_with(Path::new(DIR), &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
            v["zoomRegions"][1]["scale"].as_f64().unwrap()
        };
        let full = scale_with(None);
        let quarter = scale_with(Some(CropRect { x: 0.0, y: 0.0, w: 0.25, h: 0.25 }));
        assert!(full > 4.0, "a 3024 px source barely bounds 5× at 1080p: {full}");
        assert!((1.0..1.5).contains(&quarter), "{quarter}");
    }

    #[test]
    fn scene_clip_paths_follow_source() {
        let mut d = doc();
        d.cursor = crate::record::editor::CursorStyle { show: true, click_sound: true, ..crate::record::editor::CursorStyle::default() };
        d.clips[1].source = Some("t1".into());
        d.crop[1] = None;
        let take = TakeSource {
            meta: RecordingMeta { width: 1280, height: 720, duration_ms: 20_000, system_audio: false, mic: None, ..RecordingMeta::default() },
            cursor: Some(cursor_rec()),
        };
        let takes = TakeSources::from([("t1".to_string(), take)]);
        // The project's own take has no telemetry: only the take's clip shows clicks.
        let v = scene_json_with(Path::new(DIR), &project(), &d, (1920, 1080), 30, &assets(), None, &takes);
        let clips = v["clips"].as_array().unwrap();
        assert_eq!(clips[0]["screenPath"], format!("{DIR}/screen.mp4"));
        assert_eq!(clips[0]["cursorPath"], format!("{DIR}/cursor.json"));
        assert_eq!(clips[0]["hasAudio"], true);
        assert_eq!(clips[0]["extraAudio"]["path"], format!("{DIR}/mic.wav"));
        assert_eq!(clips[1]["screenPath"], format!("{DIR}/takes/t1/screen.mp4"));
        assert_eq!(clips[1]["cursorPath"], format!("{DIR}/takes/t1/cursor.json"));
        assert_eq!(clips[1]["hasAudio"], false);
        assert!(clips[1].get("extraAudio").is_none(), "the take has no mic");
        assert_eq!(v["cursor"]["show"], true);
        // The take's clip covers 6.5–10 s of the take; its click at 7.5 s is a hit, the
        // project's clip (0–4 s) contributes none (it has no telemetry).
        let hits = v["clickSound"]["hits"].as_array().expect("hits");
        assert!(!hits.is_empty());
        let first = hits[0]["timeSec"].as_f64().unwrap();
        assert!(first >= 3.0, "first hit {first} must lie in the second clip");
        // The screen box of the take's clip has the take's 16:9 shape.
        let metas = take_metas(&takes);
        let boxes = screen_boxes(&project().recording, &metas, &d, (1920, 1080));
        assert_eq!(boxes[1].1, (1280, 720));
    }

    #[test]
    fn load_takes_reads_only_plain_complete_takes() {
        let dir = std::env::temp_dir().join(format!("ir-takes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mk = |id: &str, screen: bool| {
            let d = dir.join(TAKES_DIR).join(id);
            std::fs::create_dir_all(&d).unwrap();
            let mut p = project();
            p.recording.duration_ms = 7_000;
            crate::record::project::write_atomic(&d, crate::record::project::PROJECT_FILE, &p).unwrap();
            if screen {
                std::fs::write(d.join(SCREEN_FILE), b"x").unwrap();
            }
        };
        mk("good", true);
        mk("noscreen", false);
        mk("bad id", true);
        let takes = load_takes(&dir);
        assert_eq!(takes.keys().collect::<Vec<_>>(), vec!["good"]);
        assert_eq!(take_metas(&takes)["good"].duration_ms, 7_000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scene_skips_a_clip_whose_take_is_missing() {
        let mut d = doc();
        // Clip 1 (with its speed region and zoom) plays a take that is not there.
        d.clips[1].source = Some("gone".into());
        d.zooms = vec![ZoomRegion {
            id: "z".into(),
            clip: 1,
            start_ms: 7_000,
            end_ms: 8_000,
            depth: 3,
            custom_scale: None,
            focus_x: 0.5,
            focus_y: 0.5,
            focus_mode: FocusMode::Auto,
            origin: ZoomOrigin::Manual,
            hide_cursor: false,
            rotation: None,
        }];
        let v = scene_json_with(Path::new(DIR), &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        let clips = v["clips"].as_array().unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0]["screenPath"], format!("{DIR}/screen.mp4"));
        assert!(!v.to_string().contains("gone"), "the scene never names the missing take");
        assert!(v["speedRegions"].as_array().unwrap().iter().all(|r| r["clipIndex"] == 0));
        assert_eq!(v["speedRegions"].as_array().unwrap().len(), 1);
        assert!(v["zoomRegions"].as_array().unwrap().is_empty());
        assert_eq!(v["cropByClip"].as_array().unwrap().len(), 1);
        assert_eq!(v["layout"]["layoutByClip"].as_array().unwrap().len(), 1);
    }

    fn ann(id: &str, clip: usize, z: i32, body: editor::AnnotationBody) -> editor::Annotation {
        let mut a = editor::Annotation::new(id.into(), clip, 1_000, 2_500, body, &[]);
        a.z_index = z;
        a
    }

    fn text_body() -> editor::AnnotationBody {
        editor::AnnotationBody::Text(editor::TextBody { content: "Hallo".into(), font_family: "Caveat".into(), ..Default::default() })
    }

    fn freehand_body() -> editor::AnnotationBody {
        editor::AnnotationBody::Blur(editor::BlurBody {
            shape: BlurShape::Freehand,
            points: vec![(0.0, 0.0), (100.0, 0.0), (50.0, 100.0)],
            ..Default::default()
        })
    }

    #[test]
    fn annotations_emitted_in_scene() {
        let dir = tmp_project("ann");
        std::fs::write(dir.join(PROJECT_ASSETS_DIR).join("logo.png"), b"png").unwrap();
        let mut d = doc();
        let mut t = ann("t", 1, 0, text_body());
        t.x = 25.0;
        t.y = 40.0;
        t.w = 30.0;
        t.h = 20.0;
        t.start_ms = 7_000;
        t.end_ms = 8_500;
        d.annotations = vec![
            t,
            ann("i", 0, 1, editor::AnnotationBody::Image { file: "logo.png".into() }),
            ann(
                "a",
                0,
                2,
                editor::AnnotationBody::Arrow {
                    direction: editor::ArrowDirection::UpLeft,
                    color: "#34B27B".into(),
                    stroke_width: 4.0,
                },
            ),
            ann("b", 0, 3, freehand_body()),
            ann("r", 0, 4, editor::AnnotationBody::Blur(editor::BlurBody::default())),
        ];
        let v = scene_json_with(&dir, &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        let a = v["annotations"].as_array().unwrap();
        assert_eq!(a.len(), 5);
        assert_eq!(
            a[0],
            json!({
                "id": "t", "clipIndex": 1, "startSec": 7.0, "endSec": 8.5, "kind": "text", "space": "frame",
                "x": 0.25, "y": 0.4, "w": 0.3, "h": 0.2, "zIndex": 0,
                "text": {
                    "content": "Hallo", "color": "#ffffff", "backgroundColor": "rgba(0, 0, 0, 0.7)",
                    "fontSizeRel": 32.0 / 1080.0, "fontFamily": "Caveat", "fontWeight": "bold",
                    "fontStyle": "normal", "textDecoration": "none", "textAlign": "center", "animation": "none",
                },
            })
        );
        assert_eq!(a[1]["kind"], "image");
        assert_eq!(a[1]["space"], "frame");
        assert_eq!(a[1]["imagePath"], path_str(&dir.join("assets/logo.png")));
        assert_eq!(a[1]["startSec"], 1.0);
        assert_eq!(a[1]["endSec"], 2.5);
        assert_eq!(a[2]["kind"], "figure");
        assert_eq!(a[2]["figure"], json!({"direction": "up-left", "color": "#34B27B", "strokeWidth": 4.0}));
        assert_eq!(a[3]["kind"], "blur");
        assert!(a[3].get("space").is_none(), "blur is measured on the screen");
        assert_eq!(
            a[3]["blur"],
            json!({
                "style": "mosaic", "shape": "freehand", "color": "white", "intensity": 12.0, "blockSize": 12.0,
                "freehandPoints": [{"x": 0.0, "y": 0.0}, {"x": 1.0, "y": 0.0}, {"x": 0.5, "y": 1.0}],
            })
        );
        assert_eq!(a[4]["blur"]["shape"], "rectangle");
        assert!(a[4]["blur"].get("freehandPoints").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn annotations_sorted_by_z() {
        let mut d = doc();
        d.annotations = vec![ann("c", 0, 7, text_body()), ann("a", 0, 1, text_body()), ann("b", 0, 3, text_body())];
        let v = scene_json_with(Path::new(DIR), &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        let ids: Vec<_> = v["annotations"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    #[test]
    fn image_outside_assets_dropped() {
        let dir = tmp_project("ann-img");
        std::fs::write(dir.join("x.png"), b"png").unwrap();
        let mut d = doc();
        let abs = path_str(&dir.join("x.png"));
        for bad in ["../x.png", abs.as_str(), "missing.png", ""] {
            d.annotations = vec![ann("i", 0, 0, editor::AnnotationBody::Image { file: bad.into() })];
            let v = scene_json_with(&dir, &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
            assert!(v.get("annotations").is_none(), "{bad:?} must not reach the helper");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn annotation_of_a_missing_take_is_omitted_and_reindexed() {
        let mut d = doc();
        d.clips[0].source = Some("gone".into());
        d.annotations = vec![ann("x", 0, 0, text_body()), ann("y", 1, 0, text_body())];
        let v = scene_json_with(Path::new(DIR), &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        let a = v["annotations"].as_array().unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0]["id"], "y");
        assert_eq!(a[0]["clipIndex"], 0);
    }

    /// The caption entries of a small transcript over the stage-2 document (two clips, the
    /// second from 6.5 s): golden for the annotations block; the rest of the scene is the
    /// unchanged `scene-golden.json`.
    #[test]
    fn scene_with_captions() {
        let mut d = doc();
        let w = |id: &str, s: u64, e: u64, t: &str| editor::TranscriptWord { id: id.into(), start_ms: s, end_ms: e, text: t.into(), original_text: None };
        d.transcripts = vec![editor::Transcript {
            source: None,
            lang: "de".into(),
            words: vec![
                w("w1", 500, 800, "Hallo"),
                w("w2", 850, 1_200, "zusammen,"),
                w("w3", 1_250, 1_500, "das"),
                w("w4", 3_700, 4_300, "geht"),
                w("w5", 6_000, 6_900, "über"),
                w("w6", 7_000, 7_400, "Clips."),
            ],
        }];
        d.captions.enabled = true;
        // A real annotation above the base: captions still paint after it.
        let mut a = ann("note", 1, 500_000, text_body());
        a.start_ms = 7_000;
        a.end_ms = 8_000;
        d.annotations = vec![a];
        let v = scene_json_with(Path::new(DIR), &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        let list = &v["annotations"];
        check_golden(&canonical(list), GOLDEN_CAPTIONS, "scene-golden-captions.json");
        let ids: Vec<&str> = list.as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["note", "caption-0-0", "caption-0-1", "caption-1-0"]);
        // Everything but the annotations is the scene without captions, byte for byte.
        let mut plain = d.clone();
        plain.captions.enabled = false;
        let mut without = scene_json_with(Path::new(DIR), &project(), &plain, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        let mut with = v.clone();
        with.as_object_mut().unwrap().remove("annotations");
        without.as_object_mut().unwrap().remove("annotations");
        assert_eq!(canonical(&with), canonical(&without));
        // Captions off: the plain document's scene has only the real annotation.
        let off = scene_json_with(Path::new(DIR), &project(), &plain, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        assert_eq!(off["annotations"].as_array().unwrap().len(), 1);
        // Transcript without captions on and no annotations: the stage-2 golden, unchanged.
        let mut bare = doc();
        bare.transcripts = d.transcripts.clone();
        let g = scene_json(Path::new(DIR), &project(), &bare, (1920, 1080), 30, &assets());
        assert_eq!(placeholders(&g), GOLDEN);
    }

    #[test]
    fn no_annotations_no_key() {
        let v = scene_json_with(Path::new(DIR), &project(), &doc(), (1920, 1080), 30, &assets(), None, &TakeSources::new());
        assert!(v.get("annotations").is_none());
    }

    #[test]
    fn system_gain_in_scene() {
        let mut d = doc();
        d.system_gain_db = -6.5;
        let v = scene_json_with(Path::new(DIR), &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        for c in v["clips"].as_array().unwrap() {
            assert_eq!(c["systemGainDb"], -6.5);
            assert_eq!(c["normalise"], "voice", "the test project has a mic");
        }
        // The master level stays at 0: the system level is the screen audio's alone.
        assert!(v.get("audio").is_none());
    }

    /// Ruling R6: a recording without a mic keeps upstream's normalisation of its screen audio.
    #[test]
    fn mic_less_clip_normalises_the_screen_audio() {
        let mut p = project();
        p.recording.mic = None;
        let mut d = doc();
        d.system_gain_db = 3.0;
        let v = scene_json_with(Path::new(DIR), &p, &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        for c in v["clips"].as_array().unwrap() {
            assert_eq!(c["normalise"], "screen");
            assert_eq!(c["systemGainDb"], 3.0);
            assert!(c.get("extraAudio").is_none());
        }
    }

    #[test]
    fn audio_tracks_emitted_in_scene() {
        let dir = tmp_project("tracks");
        std::fs::write(dir.join(PROJECT_ASSETS_DIR).join("song.m4a"), b"m4a").unwrap();
        let mut d = doc();
        let mut t = editor::AudioTrack {
            id: "m".into(),
            kind: editor::TrackKind::Music,
            file: "song.m4a".into(),
            clip: 0,
            start_ms: d.clips[0].start_ms,
            duration_ms: 1_000,
            offset_ms: 0,
            span_ms: None,
            gain_db: -18.0,
            looped: true,
            fade_in_ms: 200,
            fade_out_ms: 300,
            muted: false,
            label: String::new(),
        };
        let v = scene_json_with(&dir, &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        assert!(v.get("audioTracks").is_none(), "no tracks, no key");
        d.audio_tracks = vec![t.clone()];
        let v = scene_json_with(&dir, &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        let a = v["audioTracks"].as_array().unwrap();
        assert!(a.len() > 1, "the loop fills the programme");
        assert_eq!(a[0]["path"], path_str(&dir.join("assets/song.m4a")));
        assert_eq!(a[0]["startSec"], 0.0);
        assert_eq!(a[0]["gainDb"], -18.0);
        assert_eq!(a[0]["fadeInSec"], 0.2);
        assert_eq!(a[0]["kind"], "music");
        assert_eq!(a.last().unwrap()["fadeOutSec"], 0.3);
        // A muted track is left out.
        t.muted = true;
        d.audio_tracks = vec![t];
        let v = scene_json_with(&dir, &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        assert!(v.get("audioTracks").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── camera (stage 5) ─────────────────────────────────────────────────────

    fn cam(w: u32, h: u32, offset_ms: i64) -> crate::record::project::CameraMeta {
        crate::record::project::CameraMeta { width: w, height: h, fps: 30, offset_ms, device_name: "Cam".into(), ended_early_ms: None }
    }

    fn cam_project() -> Project {
        let mut p = project();
        p.recording.camera = Some(cam(1920, 1080, 0));
        p
    }

    /// `doc()` with a look worth laying out (padding) and a camera style.
    fn cam_doc(preset: editor::LayoutPreset) -> EditDoc {
        let mut d = doc();
        d.appearance.padding = 20.0;
        d.camera = editor::CameraStyle { preset, ..editor::CameraStyle::default() };
        d
    }

    fn rect_of(v: &Value) -> layout::Rect {
        layout::Rect {
            x: v["x"].as_f64().unwrap(),
            y: v["y"].as_f64().unwrap(),
            width: v["width"].as_f64().unwrap(),
            height: v["height"].as_f64().unwrap(),
        }
    }

    fn close(a: layout::Rect, b: layout::Rect) -> bool {
        [a.x - b.x, a.y - b.y, a.width - b.width, a.height - b.height].iter().all(|d| d.abs() < 1e-9)
    }

    /// Two inserted takes: `cam` with a camera (offset 250 ms), `plain` without.
    fn cam_takes() -> TakeSources {
        let take = |camera| TakeSource {
            meta: RecordingMeta { width: 1280, height: 720, duration_ms: 20_000, camera, ..RecordingMeta::default() },
            cursor: None,
        };
        TakeSources::from([("cam".to_string(), take(Some(cam(1280, 720, 250)))), ("plain".to_string(), take(None))])
    }

    #[test]
    fn scene_camera_paths_follow_source() {
        let mut d = cam_doc(editor::LayoutPreset::PictureInPicture);
        d.clips.push(EditClip { start_ms: 0, end_ms: 2_000, source: Some("cam".into()) });
        d.clips.push(EditClip { start_ms: 0, end_ms: 2_000, source: Some("plain".into()) });
        d.crop.resize(4, None);
        let d = editor::normalize(d, 10_000, &take_metas(&cam_takes()));
        let v = scene_json_with(Path::new(DIR), &cam_project(), &d, (1920, 1080), 30, &assets(), None, &cam_takes());
        let clips = v["clips"].as_array().unwrap();
        assert_eq!(clips[0]["webcamPath"], format!("{DIR}/camera.mp4"));
        assert_eq!(clips[0]["webcamOffsetSec"], 0.0);
        assert_eq!(clips[2]["webcamPath"], format!("{DIR}/takes/cam/camera.mp4"));
        assert_eq!(clips[2]["webcamOffsetSec"], 0.25);
        assert_eq!(clips[3]["webcamPath"], "");
        assert_eq!(clips[3]["webcamOffsetSec"], 0.0);
        // The project's own recording without a camera: its clips name none.
        let v = scene_json_with(Path::new(DIR), &project(), &d, (1920, 1080), 30, &assets(), None, &cam_takes());
        assert_eq!(v["clips"][0]["webcamPath"], "");
        assert_eq!(v["clips"][2]["webcamPath"], format!("{DIR}/takes/cam/camera.mp4"));
    }

    /// An inserted take whose project.json names a camera but whose camera.mp4 is absent or a
    /// link: `load_takes` drops the camera, so its clip names no webcam file and lays out as
    /// no-webcam (the export would otherwise fail opening the missing file).
    #[test]
    fn load_takes_drops_a_camera_whose_file_is_missing() {
        use crate::record::project::{write_atomic, CAMERA_FILE, PROJECT_FILE};
        let dir = std::env::temp_dir().join(format!("ir-takes-cam-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mk = |id: &str| {
            let d = dir.join(TAKES_DIR).join(id);
            std::fs::create_dir_all(&d).unwrap();
            let mut p = project();
            p.recording = RecordingMeta { width: 1280, height: 720, duration_ms: 20_000, camera: Some(cam(1280, 720, 0)), ..RecordingMeta::default() };
            write_atomic(&d, PROJECT_FILE, &p).unwrap();
            std::fs::write(d.join(SCREEN_FILE), b"x").unwrap();
            d
        };
        let real = mk("real");
        std::fs::write(real.join(CAMERA_FILE), b"cam").unwrap();
        mk("absent");
        let linked = mk("linked");
        #[cfg(unix)]
        std::os::unix::fs::symlink(real.join(CAMERA_FILE), linked.join(CAMERA_FILE)).unwrap();
        let takes = load_takes(&dir);
        let metas = take_metas(&takes);
        assert_eq!(metas["real"].camera, Some((1280, 720)));
        assert_eq!(metas["absent"].camera, None);
        assert_eq!(metas["linked"].camera, None);
        let mut d = cam_doc(editor::LayoutPreset::DualFrame);
        for id in ["real", "absent", "linked"] {
            d.clips.push(EditClip { start_ms: 0, end_ms: 2_000, source: Some(id.into()) });
        }
        d.crop.resize(d.clips.len(), None);
        let d = editor::normalize(d, 10_000, &metas);
        let v = scene_json_with(&dir, &project(), &d, (1920, 1080), 30, &assets(), None, &takes);
        let clips = v["clips"].as_array().unwrap();
        let n = clips.len();
        assert_eq!(clips[n - 3]["webcamPath"], path_str(&real.join(CAMERA_FILE)));
        assert_eq!(clips[n - 2]["webcamPath"], "");
        assert_eq!(clips[n - 1]["webcamPath"], "");
        let by_clip = v["layout"]["layoutByClip"].as_array().unwrap();
        assert_eq!(by_clip.len(), n);
        assert!(by_clip[n - 3]["webcamRect"].is_object());
        assert!(by_clip[n - 2].get("webcamRect").is_none(), "{}", by_clip[n - 2]);
        assert!(by_clip[n - 1].get("webcamRect").is_none(), "{}", by_clip[n - 1]);
        for i in 0..n {
            assert!(editor::clip_camera(&project().recording, &metas, &d, i).is_none() || i == n - 3, "clip {i}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scene_no_camera_clip_is_no_webcam() {
        let mut d = cam_doc(editor::LayoutPreset::DualFrame);
        d.clips.push(EditClip { start_ms: 0, end_ms: 2_000, source: Some("plain".into()) });
        d.crop.resize(3, None);
        let d = editor::normalize(d, 10_000, &take_metas(&cam_takes()));
        let out = (1920, 1080);
        let v = scene_json_with(Path::new(DIR), &cam_project(), &d, out, 30, &assets(), None, &cam_takes());
        let by_clip = v["layout"]["layoutByClip"].as_array().unwrap();
        // The camera clip: a welded block, the screen covering its slot.
        assert!(by_clip[0]["webcamRect"].is_object());
        assert_eq!(by_clip[0]["screenCover"], true);
        assert_eq!(by_clip[0]["webcamShape"], "rectangle");
        // The camera-less clip: the screen alone, exactly as without any camera.
        let plain = &by_clip[2];
        assert!(plain.get("webcamRect").is_none(), "{plain}");
        assert_eq!(plain["screenCover"], false);
        let alone = layout::screen_rect(out, 1280.0 / 720.0, 20.0, Frame::None, false);
        assert!(close(rect_of(&plain["screenRect"]), alone), "{plain}");
        // screen_boxes (focus crosshair, zoom limit) read the same per-clip layout.
        let boxes = screen_boxes(&cam_project().recording, &take_metas(&cam_takes()), &d, out);
        assert!(close(boxes[0].0, rect_of(&by_clip[0]["screenRect"])));
        assert!(close(boxes[2].0, alone));
        let alone0 = layout::screen_rect(out, 3024.0 / 1964.0, 20.0, Frame::None, false);
        assert!(boxes[0].0.width < alone0.width, "the block screen is narrower than the screen alone");
        // Scalars = the first clip's layout, the global preset resolved.
        let l = &v["layout"];
        assert_eq!(l["preset"], "dual-frame");
        assert!(close(rect_of(&l["webcamRect"]), rect_of(&by_clip[0]["webcamRect"])));
        assert!(close(rect_of(&l["screenRect"]), rect_of(&by_clip[0]["screenRect"])));
        assert_eq!(l["screenCover"], true);
        assert_eq!(l["webcamReactiveZoom"], false, "only picture-in-picture shrinks during a zoom");
        assert_eq!(l["webcamPosition"], Value::Null);
        assert_eq!(l["webcamAnchor"], "bottom-right");
        assert_eq!(l["webcamSize"], 0.4);
        assert!(l.get("webcamCrop").is_none());
        // A camera-less project lays out exactly as before (the goldens pin the bytes).
        let v = scene_json_with(Path::new(DIR), &project(), &d, out, 30, &assets(), None, &TakeSources::new());
        assert_eq!(v["layout"]["preset"], "no-webcam");
        assert_eq!(v["layout"]["webcamSize"], 1.0);
        assert!(v["layout"].get("webcamRect").is_none());
        assert!(v.get("layoutRegions").is_none());
    }

    #[test]
    fn scene_pip_layout_carries_the_camera_style() {
        let mut d = cam_doc(editor::LayoutPreset::PictureInPicture);
        d.camera.mirror = true;
        d.camera.size_pct = 30.0;
        d.camera.anchor = editor::CameraAnchor::TopLeft;
        d.camera.crop = Some(CropRect { x: 0.25, y: 0.0, w: 0.5, h: 1.0 });
        let out = (1920, 1080);
        let v = scene_json_with(Path::new(DIR), &cam_project(), &d, out, 30, &assets(), None, &TakeSources::new());
        let l = &v["layout"];
        assert_eq!(l["preset"], "picture-in-picture");
        assert_eq!((l["webcamMirror"].clone(), l["webcamReactiveZoom"].clone()), (json!(true), json!(true)));
        assert_eq!(l["webcamShape"], "square");
        assert_eq!(l["webcamAnchor"], "top-left");
        assert_eq!(l["webcamSize"], 0.3);
        assert_eq!(l["webcamCrop"], json!({"x": 0.25, "y": 0.0, "width": 0.5, "height": 1.0}));
        assert!((l["webcamRadiusFrac"].as_f64().unwrap() - 0.35).abs() < 0.01, "{l}");
        let look = d.camera.look(editor::LayoutPreset::PictureInPicture);
        let want = layout::composite_layout(out, 3024.0 / 1964.0, Some((960, 1080)), &look, 20.0, Frame::None, false);
        assert!(close(rect_of(&l["webcamRect"]), want.camera.unwrap()), "{l}");
        assert!(close(rect_of(&l["screenRect"]), want.screen));
        assert!(l.get("screenRadiusFrac").is_none());
    }

    #[test]
    fn scene_layout_regions_have_transitions() {
        let mut d = cam_doc(editor::LayoutPreset::PictureInPicture);
        d.layout_regions = vec![
            editor::LayoutRegion { id: "a".into(), clip: 0, start_ms: 1_000, preset: editor::LayoutPreset::DualFrame },
            editor::LayoutRegion { id: "b".into(), clip: 0, start_ms: 3_900, preset: editor::LayoutPreset::VerticalStack },
        ];
        let v = scene_json_with(Path::new(DIR), &cam_project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        let regs = v["layoutRegions"].as_array().expect("regions");
        let parts: Vec<_> = regs
            .iter()
            .map(|r| (r["clipIndex"].as_u64().unwrap(), r["startSec"].as_f64().unwrap(), r["endSec"].as_f64().unwrap(), r["preset"].as_str().unwrap()))
            .collect();
        // The whole programme is partitioned: before the first region the global preset.
        assert_eq!(
            parts,
            vec![
                (0, 0.0, 1.0, "picture-in-picture"),
                (0, 1.0, 3.9, "dual-frame"),
                (0, 3.9, 4.0, "vertical-stack"),
                (1, 6.5, 10.0, "picture-in-picture"),
            ]
        );
        // The first part of a clip starts without a morph.
        assert!(regs[0].get("fromLayout").is_none());
        assert_eq!(regs[0]["transitionSec"], 0.0);
        assert_eq!(regs[0]["webcamReactiveZoom"], true);
        // A change morphs for 0.2 s from the previous region's layout.
        assert_eq!(regs[1]["fromPreset"], "picture-in-picture");
        assert_eq!(regs[1]["fromWebcamReactiveZoom"], true);
        assert_eq!(regs[1]["webcamReactiveZoom"], false);
        assert_eq!(regs[1]["transitionStartSec"], 1.0);
        assert!((regs[1]["transitionSec"].as_f64().unwrap() - 0.2).abs() < 1e-12);
        assert_eq!(regs[1]["fromLayout"], regs[0]["layout"]);
        assert!(regs[1].get("keepScreenClipFromSec").is_none());
        // A part shorter than 0.4 s morphs over half its length; a block before it keeps its viewport.
        assert_eq!(regs[2]["fromPreset"], "dual-frame");
        assert!((regs[2]["transitionSec"].as_f64().unwrap() - 0.05).abs() < 1e-9, "{}", regs[2]);
        assert!((regs[2]["keepScreenClipFromSec"].as_f64().unwrap() - 3.9).abs() < 1e-9);
        assert_eq!(regs[2]["layout"]["screenCover"], true);
        // The region's layout is the per-clip layout of its preset.
        assert_eq!(regs[0]["layout"], v["layout"]["layoutByClip"][0]);
        // A clip without a camera resolves every region to no-webcam.
        let mut d2 = d.clone();
        d2.clips[1].source = Some("plain".into());
        d2.crop[1] = None;
        d2.layout_regions.push(editor::LayoutRegion { id: "c".into(), clip: 1, start_ms: 8_000, preset: editor::LayoutPreset::DualFrame });
        d2.layout_regions.push(editor::LayoutRegion { id: "d".into(), clip: 1, start_ms: 9_000, preset: editor::LayoutPreset::PictureInPicture });
        let v = scene_json_with(Path::new(DIR), &cam_project(), &d2, (1920, 1080), 30, &assets(), None, &cam_takes());
        let last: Vec<_> = v["layoutRegions"].as_array().unwrap().iter().filter(|r| r["clipIndex"] == 1).cloned().collect();
        assert_eq!(last.iter().map(|r| r["preset"].clone()).collect::<Vec<_>>(), vec![json!("no-webcam"); 3]);
        // No camera, no block: a "dual-frame" part before it keeps no zoom viewport.
        assert!(last.iter().all(|r| r.get("keepScreenClipFromSec").is_none()), "{last:?}");
    }

    #[test]
    fn scene_effect_absent_when_none() {
        let mut d = cam_doc(editor::LayoutPreset::PictureInPicture);
        let scene = |d: &EditDoc, p: &Project| scene_json_with(Path::new(DIR), p, d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        assert!(scene(&d, &cam_project()).get("webcamEffect").is_none());
        d.camera.background.mode = editor::CameraBgMode::Blur;
        d.camera.background.blur = 0.8;
        let e = scene(&d, &cam_project())["webcamEffect"].clone();
        assert_eq!(e, json!({"mode": "blur", "blurIntensity": 0.8}), "the app never names a model (the helper puts its own in)");
        d.camera.background.mode = editor::CameraBgMode::Custom;
        d.camera.background.custom = Background::Color { color: "#102030".into() };
        let e = scene(&d, &cam_project())["webcamEffect"].clone();
        assert_eq!(e["mode"], "custom");
        assert_eq!(e["background"], json!({"kind": "color", "color": "#102030"}));
        d.camera.background.mode = editor::CameraBgMode::Transparent;
        assert_eq!(scene(&d, &cam_project())["webcamEffect"]["mode"], "transparent");
        // Without any camera there is nothing to cut out.
        assert!(scene(&d, &project()).get("webcamEffect").is_none());
    }

    /// Ruling R3: a camera-less v3 project migrates to the PiP default, keeps its old scene byte
    /// for byte, and shows the camera of a camera take appended later.
    #[test]
    fn camera_less_project_shows_an_appended_camera_take() {
        let v3 = json!({"version": 3, "clips": [{"startMs": 0, "endMs": 4_000}], "output": {"aspect": "16:9"}, "autoZoomDone": true});
        let d = editor::migrate(v3).expect("v3 migrates");
        assert_eq!(d.camera.preset, editor::LayoutPreset::PictureInPicture);
        let before = scene_json_with(Path::new(DIR), &project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        assert_eq!(before["layout"]["preset"], "no-webcam", "no camera anywhere: the old scene");
        assert_eq!(before["layout"]["webcamSize"], 1.0);
        assert!(before["layout"].get("webcamRect").is_none());
        let takes = cam_takes();
        let metas = take_metas(&takes);
        let d = editor::normalize(editor::append_take(&d, "cam", &metas["cam"]), 10_000, &metas);
        let v = scene_json_with(Path::new(DIR), &project(), &d, (1920, 1080), 30, &assets(), None, &takes);
        assert_eq!(v["layout"]["preset"], "picture-in-picture");
        let by_clip = v["layout"]["layoutByClip"].as_array().unwrap();
        assert!(by_clip[0].get("webcamRect").is_none(), "the project's own clip has no camera");
        assert!(by_clip[1]["webcamRect"].is_object(), "the take's camera is shown: {}", by_clip[1]);
        assert_eq!(v["clips"][1]["webcamPath"], format!("{DIR}/takes/cam/camera.mp4"));
    }

    /// Every key the camera emission writes is a field of the compositor's types
    /// (`vendor/openscreen-compositor/src/scene.rs`, read from disk — core must not link the
    /// compositor), and every field it requires is there.
    #[test]
    fn camera_scene_keys_match_the_compositor() {
        const SCENE_RS: &str = include_str!("../../../../vendor/openscreen-compositor/src/scene.rs");
        fn camel(s: &str) -> String {
            let mut out = String::new();
            let mut up = false;
            for c in s.chars() {
                if c == '_' {
                    up = true;
                } else if up {
                    out.extend(c.to_uppercase());
                    up = false;
                } else {
                    out.push(c);
                }
            }
            out
        }
        // (all fields, required fields) of `pub struct name`.
        let fields = |name: &str| -> (HashSet<String>, HashSet<String>) {
            let start = SCENE_RS.find(&format!("pub struct {name} {{")).unwrap_or_else(|| panic!("{name} in scene.rs"));
            let body = &SCENE_RS[start..];
            let body = &body[..body.find("\n}").unwrap()];
            let (mut all, mut req) = (HashSet::new(), HashSet::new());
            let mut defaulted = false;
            for line in body.lines().skip(1).map(str::trim) {
                if line.starts_with("#[serde(") && (line.contains("default") || line.contains("skip")) {
                    defaulted = true;
                } else if let Some(rest) = line.strip_prefix("pub ") {
                    let rest = rest.strip_prefix("(crate) ").unwrap_or(rest);
                    let Some((field, ty)) = rest.split_once(':') else { continue };
                    let key = camel(field.trim());
                    if !defaulted && !ty.trim().starts_with("Option<") {
                        req.insert(key.clone());
                    }
                    all.insert(key);
                    defaulted = false;
                } else if !line.starts_with("///") && !line.starts_with("#[") && !line.is_empty() {
                    defaulted = false;
                }
            }
            (all, req)
        };
        let check = |v: &Value, ty: &str| {
            let (all, req) = fields(ty);
            let obj = v.as_object().unwrap_or_else(|| panic!("{ty}: {v}"));
            for k in obj.keys() {
                assert!(all.contains(k), "{ty} has no field `{k}` ({v})");
            }
            for k in &req {
                assert!(obj.contains_key(k), "{ty} requires `{k}` ({v})");
            }
        };
        let mut d = cam_doc(editor::LayoutPreset::DualFrame);
        d.camera.crop = Some(CropRect { x: 0.0, y: 0.0, w: 0.5, h: 0.5 });
        d.camera.background.mode = editor::CameraBgMode::Custom;
        d.layout_regions = vec![editor::LayoutRegion { id: "a".into(), clip: 0, start_ms: 1_000, preset: editor::LayoutPreset::PictureInPicture }];
        let v = scene_json_with(Path::new(DIR), &cam_project(), &d, (1920, 1080), 30, &assets(), None, &TakeSources::new());
        for c in v["clips"].as_array().unwrap() {
            check(c, "SceneClip");
        }
        check(&v["layout"], "SceneLayout");
        check(&v["layout"]["webcamCrop"], "SceneCrop");
        for r in ["webcamRect", "screenRect"] {
            check(&v["layout"][r], "SceneRect");
        }
        for l in v["layout"]["layoutByClip"].as_array().unwrap() {
            check(l, "ResolvedClipLayout");
        }
        let regs = v["layoutRegions"].as_array().unwrap();
        assert!(regs.iter().any(|r| r.get("fromLayout").is_some() && r.get("keepScreenClipFromSec").is_some()));
        for r in regs {
            check(r, "SceneLayoutRegion");
            check(&r["layout"], "ResolvedClipLayout");
            if let Some(f) = r.get("fromLayout") {
                check(f, "ResolvedClipLayout");
            }
        }
        check(&v["webcamEffect"], "SceneWebcamEffect");
        // The resolved names the compositor matches on.
        let presets = ["picture-in-picture", "dual-frame", "vertical-stack", "camera-only", "no-webcam"];
        assert!(regs.iter().all(|r| presets.contains(&r["preset"].as_str().unwrap())));
    }

    // ---- stage 7: 3D camera, depth of field, 3D / glass cursor ----

    fn scene_of(d: &EditDoc) -> Value {
        let out = editor::output_size(&project().recording, &TakeMetas::new(), d, OutputTarget::P1080);
        scene_json_with(Path::new(DIR), &project(), d, out, 30, &assets(), Some(&cursor_rec()), &TakeSources::new())
    }

    fn with_zoom_rotations(mut d: EditDoc, rot: &[Option<ZoomRotation>]) -> EditDoc {
        d.zooms.clear();
        for (i, r) in rot.iter().enumerate() {
            d.zooms.push(ZoomRegion {
                id: format!("r{i}"),
                clip: 0,
                start_ms: 100 + i as u64 * 1_000,
                end_ms: 900 + i as u64 * 1_000,
                depth: 3,
                custom_scale: None,
                focus_x: 0.5,
                focus_y: 0.5,
                focus_mode: FocusMode::Manual,
                origin: ZoomOrigin::Manual,
                hide_cursor: false,
                rotation: *r,
            });
        }
        d
    }

    #[test]
    fn v5_document_opens_as_v6_unchanged() {
        let v5 = serde_json::json!({
            "version": 5,
            "clips": [{"startMs": 0, "endMs": 5000}],
            "zooms": [{"id": "z", "clip": 0, "startMs": 0, "endMs": 1000, "depth": 3, "focusX": 0.5, "focusY": 0.5}],
        });
        let d = editor::migrate(v5).unwrap();
        assert_eq!(d.version, 6);
        assert_eq!(d.version, EDITOR_VERSION);
        assert_eq!(d.zooms[0].rotation, None);
        assert!(d.appearance.depth_of_field);
        assert!(!d.cursor.model3d);
    }

    #[test]
    fn rotation_presets_reach_the_scene() {
        let d = with_zoom_rotations(
            styled_doc(),
            &[Some(ZoomRotation::Left), Some(ZoomRotation::Right), Some(ZoomRotation::Orbit), None],
        );
        let v = scene_of(&d);
        let r: Vec<Value> = v["zoomRegions"].as_array().unwrap().iter().map(|z| z["rotation"].clone()).collect();
        assert_eq!(r, vec![json!("left"), json!("right"), json!("orbit"), Value::Null]);
    }

    #[test]
    fn depth_of_field_is_emitted_only_with_a_tilted_zoom() {
        let flat = with_zoom_rotations(styled_doc(), &[None]);
        assert_eq!(scene_of(&flat)["effects"]["depthOfField"], false);
        let mut tilted = with_zoom_rotations(styled_doc(), &[None, Some(ZoomRotation::Left)]);
        assert_eq!(scene_of(&tilted)["effects"]["depthOfField"], true);
        tilted.appearance.depth_of_field = false;
        assert_eq!(scene_of(&tilted)["effects"]["depthOfField"], false);
    }

    #[test]
    fn model3d_reaches_the_scene() {
        let mut d = styled_doc();
        assert_eq!(scene_of(&d)["cursor"]["model3d"], false);
        d.cursor.model3d = true;
        assert_eq!(scene_of(&d)["cursor"]["model3d"], true);
    }

    #[test]
    fn sculpt_is_emitted_only_for_sculpted_states_with_model3d() {
        let mut d = styled_doc();
        d.cursor.theme = "studio-ink".into();
        d.cursor.model3d = true;
        let v = scene_of(&d);
        let sprites = v["cursor"]["cursorSprites"].as_object().unwrap();
        assert_eq!(sprites["arrow"]["sculpt"], "studio-ink/arrow");
        assert_eq!(sprites["pointer"]["sculpt"], "studio-ink/pointer");
        for (state, s) in sprites {
            if !SCULPTED_STATES.contains(&state.as_str()) {
                assert!(s.get("sculpt").is_none(), "{state}");
            }
            assert!(s.get("glass").is_none(), "{state}");
        }
        d.cursor.model3d = false;
        let v = scene_of(&d);
        for (state, s) in v["cursor"]["cursorSprites"].as_object().unwrap() {
            assert!(s.get("sculpt").is_none(), "{state}");
        }
        d.cursor.model3d = true;
        d.cursor.theme = "default".into();
        let v = scene_of(&d);
        for (state, s) in v["cursor"]["cursorSprites"].as_object().unwrap() {
            assert!(s.get("sculpt").is_none() && s.get("glass").is_none(), "{state}");
        }
    }

    #[test]
    fn glass_lens_carries_glass_with_or_without_model3d() {
        for model3d in [false, true] {
            let mut d = styled_doc();
            d.cursor.theme = "glass-lens".into();
            d.cursor.model3d = model3d;
            let v = scene_of(&d);
            let sprites = v["cursor"]["cursorSprites"].as_object().unwrap();
            assert_eq!(sprites["arrow"]["glass"], "glass-lens/arrow");
            assert_eq!(sprites["pointer"]["glass"], "glass-lens/pointer");
            for (state, s) in sprites {
                if !SCULPTED_STATES.contains(&state.as_str()) {
                    assert!(s.get("glass").is_none(), "{state}");
                }
            }
        }
        let mut d = styled_doc();
        d.cursor.theme = "pop-coral".into();
        for (_, s) in scene_of(&d)["cursor"]["cursorSprites"].as_object().unwrap() {
            assert!(s.get("glass").is_none());
        }
    }

    #[test]
    fn sculpt_and_glass_names_are_pure_and_total() {
        for t in SCULPTED_THEMES {
            for s in SCULPTED_STATES {
                assert_eq!(sculpt_name(t, s, true), Some(format!("{t}/{s}")));
                assert_eq!(sculpt_name(t, s, false), None);
            }
            assert_eq!(sculpt_name(t, "text", true), None);
        }
        assert_eq!(sculpt_name("default", "arrow", true), None);
        assert_eq!(glass_name("glass-lens", "arrow").as_deref(), Some("glass-lens/arrow"));
        assert_eq!(glass_name("studio-ink", "arrow"), None);
    }

    #[test]
    fn stage7_golden() {
        let mut d = with_zoom_rotations(styled_doc(), &[Some(ZoomRotation::Left), Some(ZoomRotation::Orbit)]);
        d.cursor.theme = "studio-ink".into();
        d.cursor.model3d = true;
        let v = scene_of(&d);
        check_golden(&placeholders(&v), GOLDEN_3D, "scene-golden-3d.json");
        assert_eq!(v["effects"]["depthOfField"], true);
        assert_eq!(v["cursor"]["model3d"], true);
    }
}
