//! Versioned project file (`project.json`) and cursor sidecar (`cursor.json`).
//! Cursor field names follow OpenScreen's `src/native/contracts.ts`.

use super::core::CursorType;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PROJECT_FILE: &str = "project.json";
pub const CURSOR_FILE: &str = "cursor.json";
pub const SCREEN_FILE: &str = "screen.mp4";
pub const MIC_FILE: &str = "mic.wav";
/// The webcam recording (stage 5): H.264, its own size, on the screen's programme clock.
pub const CAMERA_FILE: &str = "camera.mp4";
/// Small preview picture of a project, written by the editor from its filmstrip render
/// (`editor_ipc::thumbs`). The project list shows it; without it a placeholder.
pub const THUMB_FILE: &str = "thumb.jpg";
/// A thumbnail larger than this is not a filmstrip frame (they are ~5 KB) — not inlined.
pub const MAX_THUMB_BYTES: u64 = 256 * 1024;
/// Longest project title a rename accepts (characters).
pub const MAX_TITLE_CHARS: usize = 200;
/// Rename refusals (catalogue keys).
pub const ERR_TITLE_EMPTY: &str = "record.title_empty";
pub const ERR_TITLE_TOO_LONG: &str = "record.title_too_long";
pub const ERR_TITLE_INVALID: &str = "record.title_invalid";
/// The project's file can't be read (a broken project), so there is nothing to rename.
pub const ERR_TITLE_UNREADABLE: &str = "record.title_unreadable";
/// Writing the new title failed (disk, permissions).
pub const ERR_TITLE_WRITE: &str = "record.title_write_failed";
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
    /// A free rectangle of one display (`RecordingMeta::region`).
    Region,
}

/// The recorded rectangle of a `region` take: display-local points, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct RegionRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
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

/// The camera file of a take (`camera.mp4`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CameraMeta {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Camera time = screen time − offset. Always 0 from our recorder (it starts the camera
    /// file at the screen's first frame); kept for OpenScreen's `webcamOffsetSec`.
    pub offset_ms: i64,
    pub device_name: String,
    /// Programme time (ms) at which the camera dropped out mid-take; the file still covers
    /// the whole take, holding its last frame from here on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_early_ms: Option<u64>,
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
    /// The webcam file (`camera.mp4`), when one was recorded.
    pub camera: Option<CameraMeta>,
    /// Only for `source: region`: the recorded rectangle of the display.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<RegionRect>,
}

/// Whether `dir/camera.mp4` is there as a plain file (a symlink or anything else is not).
pub fn camera_file_present(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(CAMERA_FILE)).is_ok_and(|m| m.file_type().is_file())
}

/// Drops `rec.camera` when the recording in `dir` has no usable `camera.mp4` (left out on
/// import because it was a link, deleted later, never written). The single gate between a
/// project.json that names a camera and the editor/scene: without it the scene would name a
/// missing file and the export would fail.
pub fn drop_missing_camera(rec: &mut RecordingMeta, dir: &Path) {
    if rec.camera.is_some() && !camera_file_present(dir) {
        rec.camera = None;
    }
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
    write_bytes_atomic(dir, name, &bytes)
}

/// Write `bytes` to `<dir>/<name>` atomically without ever following a planted symlink
/// (unique temp name, `create_new` + O_NOFOLLOW, fsync, rename — `safe_fs::write_atomic`).
/// Project folders can be opened from anywhere, so a fixed temp name would let a link put
/// there redirect the write to any file.
pub fn write_bytes_atomic(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    record_render_protocol::safe_fs::write_atomic(&dir.join(name), bytes)
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
    /// Last edit (editor save, end of the take); falls back to `created_at` when unset.
    pub updated_at: i64,
    pub duration_ms: u64,
    pub status: ProjectStatus,
    pub failure: Option<String>,
    /// `data:image/jpeg;base64,…` of the project's [`THUMB_FILE`], when the editor rendered one.
    pub thumb: Option<String>,
}

impl ProjectSummary {
    /// The time the list sorts by: the last edit, else the creation.
    pub fn last_change(&self) -> i64 {
        if self.updated_at > 0 { self.updated_at } else { self.created_at }
    }
}

/// Encoded thumbnails by file, keyed by (mtime, length): the panel polls the list every
/// 1.5 s and must not re-read and re-encode every picture each time.
type ThumbCache = std::collections::HashMap<PathBuf, (std::time::SystemTime, u64, String)>;
static THUMB_CACHE: std::sync::Mutex<Option<ThumbCache>> = std::sync::Mutex::new(None);

/// The project's thumbnail as a data URI (none when missing, a link, empty or implausibly big).
/// Read and encoded once per file version (see [`THUMB_CACHE`]).
pub fn thumb_data_uri(dir: &Path) -> Option<String> {
    use base64::Engine as _;
    let path = dir.join(THUMB_FILE);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_THUMB_BYTES {
        return None;
    }
    let mtime = meta.modified().ok()?;
    let key = (mtime, meta.len());
    if let Some((m, l, uri)) = THUMB_CACHE.lock().unwrap_or_else(|p| p.into_inner()).as_ref().and_then(|c| c.get(&path)) {
        if (*m, *l) == key {
            return Some(uri.clone());
        }
    }
    let bytes = std::fs::read(&path).ok()?;
    // JPEG SOI marker: anything else is not ours.
    if !bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return None;
    }
    let uri = format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
    THUMB_CACHE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get_or_insert_with(Default::default)
        .insert(path, (key.0, key.1, uri.clone()));
    Some(uri)
}

/// Drops cached thumbnails of projects that are no longer listed (deleted, moved).
fn prune_thumb_cache(listed: &[ProjectSummary]) {
    let keep: std::collections::HashSet<PathBuf> = listed.iter().map(|s| s.dir.join(THUMB_FILE)).collect();
    if let Some(c) = THUMB_CACHE.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        c.retain(|k, _| keep.contains(k));
    }
}

/// A user-typed title: trimmed, 1..=200 characters, no control characters.
pub fn clean_title(title: &str) -> Result<String, &'static str> {
    let t = title.trim();
    if t.is_empty() {
        return Err(ERR_TITLE_EMPTY);
    }
    if t.chars().count() > MAX_TITLE_CHARS {
        return Err(ERR_TITLE_TOO_LONG);
    }
    if t.chars().any(char::is_control) {
        return Err(ERR_TITLE_INVALID);
    }
    Ok(t.to_string())
}

/// Renames a project: only `title` in its `project.json` changes — the folder, the
/// recording, the editor document and both timestamps stay as they are.
pub fn rename_project(dir: &Path, title: &str) -> Result<Project, String> {
    let title = clean_title(title).map_err(str::to_string)?;
    let mut p = read_project(dir).map_err(|e| {
        tracing::warn!(target: "record::project", "rename {}: {e}", dir.display());
        ERR_TITLE_UNREADABLE.to_string()
    })?;
    p.title = title;
    write_atomic(dir, PROJECT_FILE, &p).map_err(|e| {
        tracing::warn!(target: "record::project", "rename {}: {e}", dir.display());
        ERR_TITLE_WRITE.to_string()
    })?;
    Ok(p)
}

/// A folder is a project exactly when it holds a `project.json` — even an
/// unparseable one (that is a broken project, listed as `Failed`). Any other
/// folder in the recordings root belongs to the user, not to us.
pub fn is_project_dir(dir: &Path) -> bool {
    dir.join(PROJECT_FILE).is_file()
}

/// List all projects below `root`, most recently edited first (`updated_at`, falling back
/// to `created_at`). Every project is listed — the panel scrolls. Broken projects appear as
/// `Failed`; folders without a `project.json` are not listed at all.
pub fn list_projects(root: &Path) -> Vec<ProjectSummary> {
    list_projects_with(root, true)
}

/// [`list_projects`]; `thumbs = false` skips reading + encoding every `thumb.jpg`
/// (up to 256 KB each) for callers that don't show them (`record.projects`).
pub fn list_projects_with(root: &Path, thumbs: bool) -> Vec<ProjectSummary> {
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
        let modified_fallback = std::fs::metadata(dir.join(PROJECT_FILE))
            .and_then(|m| m.modified())
            .ok()
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
                    thumb: if thumbs { thumb_data_uri(&dir) } else { None },
                    dir,
                    title: p.title,
                    created_at: p.created_at,
                    updated_at: p.updated_at,
                    duration_ms: p.recording.duration_ms,
                    status,
                    failure,
                }
            }
            Err(reason) => ProjectSummary {
                dir,
                title: name,
                created_at: created_fallback,
                updated_at: modified_fallback,
                duration_ms: 0,
                thumb: None,
                status: ProjectStatus::Failed,
                failure: Some(reason),
            },
        });
    }
    out.sort_by(|a, b| b.last_change().cmp(&a.last_change()).then_with(|| a.dir.cmp(&b.dir)));
    prune_thumb_cache(&out);
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
    fn recording_meta_without_camera_field_loads() {
        // A project.json from before stage 5 has no `camera` key.
        let old = r#"{"schema":1,"id":"a","status":"ready","recording":{"source":"display","width":1920,"height":1080,"fps":60,"durationMs":5000,"systemAudio":true,"mic":null,"cursorMode":"editable-overlay","pauses":[]}}"#;
        let p: Project = serde_json::from_str(old).unwrap();
        assert_eq!(p.recording.camera, None);
        assert_eq!(p.recording.duration_ms, 5000);
        // And a camera survives the round trip, `endedEarlyMs` only when set.
        let mut q = p.clone();
        q.recording.camera = Some(CameraMeta { width: 1280, height: 720, fps: 30, offset_ms: 0, device_name: "Cam".into(), ended_early_ms: None });
        let json = serde_json::to_string(&q).unwrap();
        assert!(json.contains(r#""camera":{"width":1280,"height":720,"fps":30,"offsetMs":0,"deviceName":"Cam"}"#), "{json}");
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back, q);
        q.recording.camera.as_mut().unwrap().ended_early_ms = Some(1500);
        let back: Project = serde_json::from_str(&serde_json::to_string(&q).unwrap()).unwrap();
        assert_eq!(back.recording.camera.unwrap().ended_early_ms, Some(1500));
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
    fn list_sorted_by_updated_at() {
        let root = tmp("updated");
        // (name, created, updated): an old project edited yesterday beats a newer one.
        for (n, c, u) in [("made-old-edited-now", 10, 500), ("made-new", 300, 300), ("never-saved", 200, 0)] {
            let p = root.join(n);
            std::fs::create_dir_all(&p).unwrap();
            let mut pr = project(n, ProjectStatus::Failed, c);
            pr.updated_at = u;
            write_atomic(&p, PROJECT_FILE, &pr).unwrap();
        }
        let list = list_projects(&root);
        let titles: Vec<_> = list.iter().map(|s| s.title.as_str()).collect();
        // updated 0 falls back to created (200), so it sorts between 300 and nothing older.
        assert_eq!(titles, ["made-old-edited-now", "made-new", "never-saved"]);
        assert_eq!(list[0].updated_at, 500);
        assert_eq!(list[2].last_change(), 200);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn list_has_no_row_cap() {
        let root = tmp("many");
        for i in 0..27 {
            let p = root.join(format!("p{i:02}"));
            std::fs::create_dir_all(&p).unwrap();
            write_atomic(&p, PROJECT_FILE, &project(&format!("p{i}"), ProjectStatus::Failed, i)).unwrap();
        }
        assert_eq!(list_projects(&root).len(), 27);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_changes_title_only() {
        let d = tmp("rename");
        let mut before = project("id-1", ProjectStatus::Ready, 100);
        before.updated_at = 250;
        before.recording.duration_ms = 4321;
        before.editor = serde_json::json!({"version": 3, "clips": [1, 2]});
        write_atomic(&d, PROJECT_FILE, &before).unwrap();
        let after = rename_project(&d, "  Neuer Name — Ü  ").unwrap();
        assert_eq!(after.title, "Neuer Name — Ü");
        let on_disk = read_project(&d).unwrap();
        assert_eq!(on_disk, Project { title: "Neuer Name — Ü".into(), ..before.clone() });
        // The folder keeps its name; nothing else appears in it.
        let names: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, [PROJECT_FILE]);
        // Refusals leave the file alone.
        assert_eq!(rename_project(&d, "   ").unwrap_err(), ERR_TITLE_EMPTY);
        assert_eq!(rename_project(&d, &"x".repeat(201)).unwrap_err(), ERR_TITLE_TOO_LONG);
        assert_eq!(rename_project(&d, "a\nb").unwrap_err(), ERR_TITLE_INVALID);
        // A broken project names the reason as a catalogue key, never raw English.
        let broken = d.join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join(PROJECT_FILE), "{ not json").unwrap();
        assert_eq!(rename_project(&broken, "x").unwrap_err(), ERR_TITLE_UNREADABLE);
        std::fs::remove_dir_all(&broken).unwrap();
        assert!(rename_project(&d, &"ä".repeat(200)).is_ok(), "the limit counts characters, not bytes");
        assert!(rename_project(&d.join("missing"), "x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_list_without_thumbs_reads_none_but_lists_the_same() {
        let root = tmp("list-nothumb");
        let a = root.join("a");
        std::fs::create_dir_all(&a).unwrap();
        write_atomic(&a, PROJECT_FILE, &project("a", ProjectStatus::Ready, 1)).unwrap();
        std::fs::write(a.join(THUMB_FILE), [0xFF, 0xD8, 0xFF, 0xE0, 1, 2]).unwrap();
        let with = list_projects_with(&root, true);
        let without = list_projects_with(&root, false);
        assert!(with[0].thumb.is_some());
        assert_eq!(without[0].thumb, None);
        assert_eq!((with[0].title.clone(), with[0].status), (without[0].title.clone(), without[0].status));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn thumb_is_inlined_only_when_it_is_a_small_jpeg() {
        let d = tmp("thumb");
        assert_eq!(thumb_data_uri(&d), None, "missing → placeholder");
        std::fs::write(d.join(THUMB_FILE), [0xFF, 0xD8, 0xFF, 0xE0, 1, 2]).unwrap();
        assert!(thumb_data_uri(&d).unwrap().starts_with("data:image/jpeg;base64,/9j/"));
        std::fs::write(d.join(THUMB_FILE), b"<svg/>").unwrap();
        assert_eq!(thumb_data_uri(&d), None, "not a JPEG");
        let mut big = vec![0xFF, 0xD8, 0xFF];
        big.resize(MAX_THUMB_BYTES as usize + 1, 0);
        std::fs::write(d.join(THUMB_FILE), big).unwrap();
        assert_eq!(thumb_data_uri(&d), None, "too big");
        // Cached per file version: a changed file (new length) is read again, the same one not.
        std::fs::write(d.join(THUMB_FILE), [0xFF, 0xD8, 0xFF, 1]).unwrap();
        let first = thumb_data_uri(&d).unwrap();
        assert_eq!(thumb_data_uri(&d).unwrap(), first);
        std::fs::write(d.join(THUMB_FILE), [0xFF, 0xD8, 0xFF, 1, 2, 3]).unwrap();
        assert_ne!(thumb_data_uri(&d).unwrap(), first, "a new thumbnail replaces the cached one");
        let _ = std::fs::remove_dir_all(&d);
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

    #[cfg(unix)]
    #[test]
    fn planted_symlink_at_a_temp_path_is_never_followed() {
        let d = tmp("symlink");
        let victim = d.join("victim.txt");
        std::fs::write(&victim, b"keep me").unwrap();
        // Links at every name a temp file could have had before (the old fixed names) and a
        // link AS the target: none of them is written through.
        for planted in [format!("{PROJECT_FILE}.tmp"), format!(".{PROJECT_FILE}.tmp"), format!(".{THUMB_FILE}.tmp")] {
            std::os::unix::fs::symlink(&victim, d.join(planted)).unwrap();
        }
        write_atomic(&d, PROJECT_FILE, &serde_json::json!({"x": 1})).unwrap();
        write_bytes_atomic(&d, THUMB_FILE, &[0xFF, 0xD8, 0xFF]).unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
        // A symlink as the target is replaced, not followed.
        let p2 = d.join("p2");
        std::fs::create_dir_all(&p2).unwrap();
        std::os::unix::fs::symlink(&victim, p2.join(PROJECT_FILE)).unwrap();
        write_atomic(&p2, PROJECT_FILE, &serde_json::json!({"y": 2})).unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
        assert!(!std::fs::symlink_metadata(p2.join(PROJECT_FILE)).unwrap().file_type().is_symlink());
        // The temp opener refuses a link at its exact path, whether the link's target exists
        // or dangles (a dangling link would otherwise CREATE the file it points to).
        use record_render_protocol::safe_fs::open_new_nofollow;
        let link = d.join(format!("{PROJECT_FILE}.tmp"));
        assert!(open_new_nofollow(&link).is_err());
        let fresh = d.join("not-yet-there.txt");
        std::os::unix::fs::symlink(&fresh, d.join("dangling")).unwrap();
        assert!(open_new_nofollow(&d.join("dangling")).is_err());
        assert!(!fresh.exists(), "a dangling link is not followed to create its target");
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
        // Temp files have unique names and none are left behind.
        let left: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp") && !n.starts_with(PROJECT_FILE) && n != &format!(".{PROJECT_FILE}.tmp") && n != &format!(".{THUMB_FILE}.tmp")).collect();
        assert!(left.is_empty(), "{left:?}");
        let _ = std::fs::remove_dir_all(&d);
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

    #[test]
    fn region_source_round_trips_and_stays_out_of_other_takes() {
        assert_eq!(serde_json::to_string(&SourceKind::Region).unwrap(), "\"region\"");
        let plain = RecordingMeta::default();
        let v = serde_json::to_value(&plain).unwrap();
        assert!(v.get("region").is_none(), "a display/window take writes no region key");
        let m = RecordingMeta {
            source: SourceKind::Region,
            region: Some(RegionRect { x: 10.0, y: 20.0, w: 300.0, h: 200.0 }),
            ..Default::default()
        };
        let back: RecordingMeta = serde_json::from_value(serde_json::to_value(&m).unwrap()).unwrap();
        assert_eq!(back, m);
        // An older project without the key still loads.
        let old: RecordingMeta = serde_json::from_str(r#"{"source":"window","fps":60}"#).unwrap();
        assert_eq!(old.region, None);
        assert_eq!(old.source, SourceKind::Window);
    }
}
