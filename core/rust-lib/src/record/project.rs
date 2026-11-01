//! Versioned project file (`project.json`) and cursor sidecar (`cursor.json`).
//! Cursor field names follow OpenScreen's `src/native/contracts.ts`.

use super::core::CursorType;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const PROJECT_FILE: &str = "project.json";
pub const CURSOR_FILE: &str = "cursor.json";
pub const SCREEN_FILE: &str = "screen.mp4";
pub const MIC_FILE: &str = "mic.wav";
pub const PROJECT_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    Recording,
    Ready,
    Incomplete,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    #[default]
    Display,
    Window,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CursorMode {
    #[default]
    EditableOverlay,
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicMeta {
    pub offset_ms: i64,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RecordingMeta {
    pub source: SourceKind,
    pub source_label: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_ms: u64,
    pub system_audio: bool,
    pub mic: Option<MicMeta>,
    pub cursor_mode: CursorMode,
    pub pauses: Vec<(u64, u64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub schema: u32,
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    pub status: ProjectStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    #[serde(default)]
    pub recording: RecordingMeta,
    /// Reserved for later stages; passed through untouched.
    #[serde(default = "empty_object")]
    pub editor: serde_json::Value,
}

fn empty_object() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CursorProvider {
    Native,
    /// Our own extension (not in OpenScreen's contracts.ts): positions only,
    /// no clicks/cursor types, e.g. when Accessibility is not granted.
    PositionOnly,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Interaction {
    Move,
    Click,
    DoubleClick,
    RightClick,
    MiddleClick,
    Mouseup,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorSample {
    pub time_ms: u64,
    pub cx: f64,
    pub cy: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_type: Option<CursorType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_type: Option<Interaction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorAsset {
    pub id: String,
    /// "darwin" | "win32" | "linux"
    pub platform: String,
    /// `data:image/png;base64,...`
    pub image_data_url: String,
    pub width: u32,
    pub height: u32,
    pub hotspot_x: f64,
    pub hotspot_y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_factor: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_type: Option<CursorType>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorRecording {
    pub version: u32,
    pub provider: CursorProvider,
    pub samples: Vec<CursorSample>,
    pub assets: Vec<CursorAsset>,
}

/// Write `<dir>/<name>` atomically: tmp in the same dir, fsync, rename.
pub fn write_atomic(dir: &Path, name: &str, value: &impl Serialize) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = dir.join(format!("{name}.tmp"));
    let res = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, dir.join(name))
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

pub fn read_project(dir: &Path) -> Result<Project, String> {
    let bytes = std::fs::read(dir.join(PROJECT_FILE)).map_err(|e| format!("cannot read project.json: {e}"))?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| format!("invalid project.json: {e}"))?;
    let schema = v.get("schema").and_then(|s| s.as_u64()).unwrap_or(0);
    if schema > PROJECT_SCHEMA as u64 {
        return Err(format!("project was written by a newer version (schema {schema})"));
    }
    serde_json::from_value(v).map_err(|e| format!("invalid project.json: {e}"))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub dir: PathBuf,
    pub title: String,
    pub created_at: i64,
    pub duration_ms: u64,
    pub status: ProjectStatus,
    pub failure: Option<String>,
}

/// A folder is a project exactly when it holds a `project.json` — even an
/// unparseable one (that is a broken project, listed as `Failed`). Any other
/// folder in the recordings root belongs to the user, not to us.
pub fn is_project_dir(dir: &Path) -> bool {
    dir.join(PROJECT_FILE).is_file()
}

/// List all projects below `root`, newest first. Broken projects appear as
/// `Failed`; folders without a `project.json` are not listed at all.
pub fn list_projects(root: &Path) -> Vec<ProjectSummary> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let dir = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !dir.is_dir() || !is_project_dir(&dir) {
            continue;
        }
        let created_fallback = entry
            .metadata()
            .ok()
            .and_then(|m| m.created().or_else(|_| m.modified()).ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        out.push(match read_project(&dir) {
            Ok(p) => {
                let (status, failure) = match p.status {
                    ProjectStatus::Recording => (
                        ProjectStatus::Incomplete,
                        Some(p.failure.clone().unwrap_or_else(|| "recording was interrupted".into())),
                    ),
                    ProjectStatus::Ready if !dir.join(SCREEN_FILE).is_file() => {
                        (ProjectStatus::Incomplete, Some("screen.mp4 is missing".into()))
                    }
                    s => (s, p.failure.clone()),
                };
                ProjectSummary {
                    dir,
                    title: p.title,
                    created_at: p.created_at,
                    duration_ms: p.recording.duration_ms,
                    status,
                    failure,
                }
            }
            Err(reason) => ProjectSummary {
                dir,
                title: name,
                created_at: created_fallback,
                duration_ms: 0,
                status: ProjectStatus::Failed,
                failure: Some(reason),
            },
        });
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| a.dir.cmp(&b.dir)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ir-record-project-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn project(id: &str, status: ProjectStatus, created: i64) -> Project {
        Project {
            schema: 1,
            id: id.into(),
            title: id.into(),
            created_at: created,
            updated_at: created,
            status,
            failure: None,
            recording: RecordingMeta::default(),
            editor: serde_json::json!({}),
        }
    }

    #[test]
    fn project_roundtrip_keeps_editor_block() {
        let d = tmp("rt");
        let mut p = project("a", ProjectStatus::Ready, 1);
        p.editor = serde_json::json!({"zoom":[{"a":1.5,"n":null}],"x":{"y":[1,2,3]}});
        write_atomic(&d, PROJECT_FILE, &p).unwrap();
        let back = read_project(&d).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.editor, p.editor);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn newer_schema_is_refused() {
        let d = tmp("newer");
        std::fs::write(d.join(PROJECT_FILE), r#"{"schema":2,"id":"x","status":"ready"}"#).unwrap();
        let e = read_project(&d).unwrap_err();
        assert!(e.contains("newer"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let d = tmp("defaults");
        std::fs::write(d.join(PROJECT_FILE), r#"{"schema":1,"id":"x","status":"ready"}"#).unwrap();
        let p = read_project(&d).unwrap();
        assert_eq!(p.editor, serde_json::json!({}));
        assert_eq!(p.recording, RecordingMeta::default());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn incomplete_project_is_listed_with_reason() {
        let root = tmp("list-inc");
        let a = root.join("a");
        std::fs::create_dir_all(&a).unwrap();
        write_atomic(&a, PROJECT_FILE, &project("a", ProjectStatus::Recording, 1)).unwrap();
        let b = root.join("b");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join(PROJECT_FILE), "{ not json").unwrap();
        let c = root.join("c");
        std::fs::create_dir_all(&c).unwrap();
        write_atomic(&c, PROJECT_FILE, &project("c", ProjectStatus::Ready, 3)).unwrap();
        let d = root.join("d");
        std::fs::create_dir_all(&d).unwrap();
        let hidden = root.join(".trash");
        std::fs::create_dir_all(&hidden).unwrap();
        std::fs::write(root.join("file.txt"), "x").unwrap();

        let list = list_projects(&root);
        assert_eq!(list.len(), 3, "a folder without project.json is not a project");
        let by = |n: &str| list.iter().find(|s| s.dir.ends_with(n)).unwrap();
        assert_eq!(by("a").status, ProjectStatus::Incomplete);
        assert!(by("a").failure.is_some());
        assert_eq!(by("b").status, ProjectStatus::Failed);
        assert!(by("b").failure.is_some());
        assert_eq!(by("c").status, ProjectStatus::Incomplete); // ready but no screen.mp4
        assert!(list.iter().all(|s| !s.dir.ends_with("d")), "no project.json → not listed");
        std::fs::write(c.join(SCREEN_FILE), b"x").unwrap();
        assert_eq!(list_projects(&root).iter().find(|s| s.dir.ends_with("c")).unwrap().status, ProjectStatus::Ready);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn list_is_newest_first() {
        let root = tmp("order");
        for (n, t) in [("old", 10), ("new", 30), ("mid", 20)] {
            let p = root.join(n);
            std::fs::create_dir_all(&p).unwrap();
            write_atomic(&p, PROJECT_FILE, &project(n, ProjectStatus::Failed, t)).unwrap();
        }
        let titles: Vec<_> = list_projects(&root).into_iter().map(|s| s.title).collect();
        assert_eq!(titles, ["new", "mid", "old"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cursor_json_uses_openscreen_field_names() {
        let s = CursorSample {
            time_ms: 5,
            cx: 0.5,
            cy: 0.25,
            visible: Some(true),
            asset_id: Some("a1".into()),
            cursor_type: Some(CursorType::ResizeEw),
            interaction_type: Some(Interaction::DoubleClick),
        };
        let j = serde_json::to_string(&s).unwrap();
        for k in ["\"timeMs\"", "\"cursorType\":\"resize-ew\"", "\"interactionType\":\"double-click\"", "\"assetId\""] {
            assert!(j.contains(k), "{k} in {j}");
        }
        let bare = CursorSample { time_ms: 1, cx: 0.0, cy: 0.0, visible: None, asset_id: None, cursor_type: None, interaction_type: None };
        assert!(!serde_json::to_string(&bare).unwrap().contains("null"));
        assert_eq!(serde_json::to_string(&CursorProvider::PositionOnly).unwrap(), "\"position-only\"");
        let a = CursorAsset {
            id: "a".into(), platform: "darwin".into(), image_data_url: "data:image/png;base64,AA".into(),
            width: 1, height: 1, hotspot_x: 0.0, hotspot_y: 0.0, scale_factor: Some(2.0), cursor_type: None,
        };
        let j = serde_json::to_string(&a).unwrap();
        assert!(j.contains("\"imageDataUrl\"") && j.contains("\"hotspotX\"") && j.contains("\"scaleFactor\""));
    }

    #[test]
    fn write_atomic_leaves_no_tmp() {
        let d = tmp("atomic");
        write_atomic(&d, CURSOR_FILE, &CursorRecording { version: 1, provider: CursorProvider::None, samples: vec![], assets: vec![] }).unwrap();
        let names: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, [CURSOR_FILE]);
        // failure path also cleans up
        assert!(write_atomic(&d.join("nope"), "x.json", &1).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
