//! Builds the OpenScreen compositor's `Scene` JSON from a project + editor document.
//!
//! Core must not depend on the compositor crate, so this emits JSON whose field names
//! mirror `vendor/openscreen-compositor/src/scene.rs` (serde camelCase) exactly. Stage 2
//! keeps every effect off: no webcam, no padding/blur/shadow/roundness/motion blur, black
//! background, no cursor overlay, no zoom. Required fields per RECON §1e are always present.

use super::editor::EditDoc;
use super::project::{Project, MIC_FILE, SCREEN_FILE};
use serde_json::{json, Value};
use std::path::Path;

fn ms_to_sec(ms: u64) -> f64 {
    ms as f64 / 1000.0
}

/// The compositor scene for `doc`. Paths are absolute (`project_dir` joined), as the
/// compositor expects. The mic rides along each clip as `extraAudio` (ruling R1:
/// mic time = source time − offset, so `offsetSec = mic.offset_ms / 1000`) and only when the
/// recording has one — the compositor treats a missing file as silence, never an error.
pub fn scene_json(project_dir: &Path, project: &Project, doc: &EditDoc, out: (u32, u32), fps: u32) -> Value {
    let meta = &project.recording;
    let screen = project_dir.join(SCREEN_FILE).to_string_lossy().into_owned();
    let mic = meta.mic.as_ref().map(|m| {
        json!({
            "path": project_dir.join(MIC_FILE).to_string_lossy(),
            "offsetSec": m.offset_ms as f64 / 1000.0,
            "gainDb": doc.mic_gain_db,
        })
    });
    let clips: Vec<Value> = doc
        .clips
        .iter()
        .map(|c| {
            let mut clip = json!({
                "screenPath": screen,
                "webcamPath": "",
                "sourceStartSec": ms_to_sec(c.start_ms),
                "sourceEndSec": ms_to_sec(c.end_ms),
                "webcamOffsetSec": 0.0,
                "hasAudio": meta.system_audio,
            });
            if let Some(m) = &mic {
                clip["extraAudio"] = m.clone();
            }
            clip
        })
        .collect();
    let speed_regions: Vec<Value> = doc
        .speed
        .iter()
        .map(|r| {
            json!({
                "clipIndex": r.clip,
                "startSec": ms_to_sec(r.start_ms),
                "endSec": ms_to_sec(r.end_ms),
                "speed": r.speed,
            })
        })
        .collect();
    let crop_by_clip: Vec<Value> = (0..doc.clips.len())
        .map(|i| match doc.crop.get(i).copied().flatten() {
            Some(c) => json!({"x": c.x, "y": c.y, "width": c.w, "height": c.h}),
            None => Value::Null,
        })
        .collect();
    json!({
        "clips": clips,
        "layout": {
            "preset": "no-webcam",
            "webcamSize": 1.0,
            "webcamShape": "rectangle",
            "webcamMirror": false,
            "webcamPosition": null,
            "webcamReactiveZoom": false,
        },
        "effects": {
            "padding": 0.0,
            "blur": 0.0,
            "shadow": 0.0,
            "roundnessFrac": 0.0,
            "motionBlur": 0.0,
            "depthOfField": false,
        },
        "background": {"kind": "color", "color": "#000000"},
        "zoomRegions": [],
        "speedRegions": speed_regions,
        "cursor": {
            "show": false,
            "size": 1.0,
            "smoothing": 0.0,
            "motionBlur": 0.0,
            "clickBounce": 0.0,
            "clipToBounds": false,
            "theme": "default",
        },
        "cropByClip": crop_by_clip,
        "output": {"width": out.0, "height": out.1, "fps": fps},
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::editor::{Aspect, CropRect, EditClip, OutputShape, SpeedRegion, EDITOR_VERSION};
    use crate::record::project::{MicMeta, ProjectStatus, RecordingMeta, PROJECT_SCHEMA};

    const GOLDEN: &str = include_str!("fixtures/scene-golden.json");
    const DIR: &str = "/Users/someone/Movies/Inspector Rust/take-1";
    const PLACEHOLDER: &str = "<PROJECT>";

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

    fn doc() -> EditDoc {
        EditDoc {
            version: EDITOR_VERSION,
            clips: vec![EditClip { start_ms: 0, end_ms: 4_000 }, EditClip { start_ms: 6_500, end_ms: 10_000 }],
            speed: vec![
                SpeedRegion { clip: 0, start_ms: 1_000, end_ms: 3_000, speed: 2.0 },
                SpeedRegion { clip: 1, start_ms: 7_000, end_ms: 9_250, speed: 0.5 },
            ],
            crop: vec![None, Some(CropRect { x: 0.25, y: 0.0, w: 0.5, h: 1.0 })],
            output: OutputShape { aspect: Aspect::W16H9 },
            mic_gain_db: -6.0,
        }
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

    fn built() -> String {
        let v = scene_json(Path::new(DIR), &project(), &doc(), (1920, 1080), 30);
        canonical(&v).replace(DIR, PLACEHOLDER)
    }

    /// Regenerate with `IR_WRITE_GOLDEN=1 cargo test … scene_json_matches_golden`, then
    /// review the diff by eye against RECON §1e and scene.rs before committing.
    #[test]
    fn scene_json_matches_golden() {
        let text = built();
        if std::env::var_os("IR_WRITE_GOLDEN").is_some() {
            let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/record/fixtures/scene-golden.json");
            std::fs::write(p, &text).unwrap();
            return;
        }
        assert_eq!(text, GOLDEN);
    }

    #[test]
    fn scene_json_has_required_fields() {
        let v = scene_json(Path::new(DIR), &project(), &doc(), (1920, 1080), 30);
        let has = |obj: &Value, keys: &[&str], what: &str| {
            for k in keys {
                assert!(obj.get(*k).is_some(), "{what} lacks `{k}`: {obj}");
            }
        };
        has(&v, &["clips", "layout", "effects", "background", "zoomRegions", "cursor", "output"], "scene");
        has(&v["layout"], &["preset", "webcamSize", "webcamShape", "webcamMirror", "webcamPosition", "webcamReactiveZoom"], "layout");
        has(&v["effects"], &["padding", "blur", "shadow", "roundnessFrac", "motionBlur"], "effects");
        has(&v["background"], &["kind", "color"], "background");
        has(&v["cursor"], &["show", "size", "smoothing", "motionBlur", "clickBounce", "clipToBounds", "theme"], "cursor");
        has(&v["output"], &["width", "height", "fps"], "output");
        for c in v["clips"].as_array().unwrap() {
            has(c, &["screenPath", "webcamPath", "sourceStartSec", "sourceEndSec", "webcamOffsetSec", "hasAudio"], "clip");
        }
        assert_eq!(v["layout"]["preset"], "no-webcam");
        assert_eq!(v["background"], json!({"kind": "color", "color": "#000000"}));
        assert_eq!(v["cursor"]["show"], false);
        assert_eq!(v["zoomRegions"], json!([]));
        for k in ["padding", "blur", "shadow", "roundnessFrac", "motionBlur"] {
            assert_eq!(v["effects"][k].as_f64(), Some(0.0), "effects.{k}");
        }
        assert_eq!(v["output"], json!({"width": 1920, "height": 1080, "fps": 30}));
    }

    #[test]
    fn scene_json_maps_clips_speed_crop_and_mic() {
        let v = scene_json(Path::new(DIR), &project(), &doc(), (1920, 1080), 30);
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

        // No mic recorded → no extra audio at all (not an empty path).
        let mut p = project();
        p.recording.mic = None;
        p.recording.system_audio = false;
        let v = scene_json(Path::new(DIR), &p, &doc(), (1920, 1080), 30);
        for c in v["clips"].as_array().unwrap() {
            assert!(c.get("extraAudio").is_none(), "{c}");
            assert_eq!(c["hasAudio"], false);
        }
        // A negative offset (mic started before the video) stays negative.
        let mut p = project();
        p.recording.mic = Some(MicMeta { offset_ms: -250, sample_rate: 48_000 });
        let v = scene_json(Path::new(DIR), &p, &doc(), (1920, 1080), 30);
        assert_eq!(v["clips"][0]["extraAudio"]["offsetSec"], -0.25);
    }
}
