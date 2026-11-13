//! Control-socket methods for record projects (stage 8, Task 2): list and inspect projects,
//! edit their documents op by op, export, transcribe, render single frames, rename, delete.
//!
//! Everything runs inside the app through the editor session (one writer of `project.json`):
//! a call opens its project as a window-less session when nothing is open, reuses the open
//! session of the same project, and is refused (`record.control.other_open`) while a visible
//! editor window shows another project. Edits to a project in a visible window reach it as one
//! undo step (`record-editor-external`).
//!
//! [`apply_ops`] is pure and all-or-nothing; the handlers are thin.

use super::editor::{
    self, Annotation, AnnotationBody, AudioTrack, CropRect, EditClip, EditDoc, FocusMode, LayoutRegion, SpeedRegion,
    TakeMetas, TrackKind, Transcript, ZoomOrigin, ZoomRegion, DEFAULT_ZOOM_DEPTH, MAX_SPEED, MIN_CLIP_MS, MIN_SPEED,
    MUSIC_DEFAULT_FADE_MS, MUSIC_DEFAULT_GAIN_DB, NEW_ZOOM_MS,
};
use super::effects::EffectRegion;
use super::project::CursorRecording;
use crate::control::{Call, ControlError};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

// ---- error codes ----------------------------------------------------------------------

pub const ERR_BAD_PARAMS: &str = "record.control.bad_params";
pub const ERR_BAD_OP: &str = "record.control.bad_op";
pub const ERR_BAD_PROJECT: &str = "record.control.bad_project";
pub const ERR_NO_REGION: &str = "record.control.no_region";
pub const ERR_BAD_CLIP: &str = "record.control.bad_clip";
pub const ERR_OUT_OF_RANGE: &str = "record.control.out_of_range";
pub const ERR_LAST_CLIP: &str = "record.control.last_clip";
pub const ERR_BAD_SPLIT: &str = "record.control.bad_split";
pub const ERR_NO_CURSOR: &str = "record.control.no_cursor";
pub const ERR_UNKNOWN_EFFECT: &str = "record.control.unknown_effect";
pub const ERR_DROPPED: &str = "record.control.dropped";
pub const ERR_ID_TAKEN: &str = "record.control.id_taken";
pub const ERR_CONFIRM: &str = "record.control.confirm_required";
pub const ERR_OUT_EXISTS: &str = "record.control.out_exists";
pub const ERR_BAD_OUT: &str = "record.control.bad_out";
pub const ERR_CANCELLED: &str = "record.control.cancelled";

/// Most frames one `record.frames` call renders.
pub const MAX_FRAMES: usize = 8;
/// The grid `record.frames` picks frames on.
pub const FRAMES_FPS: u32 = 30;
pub const DEFAULT_FRAME_WIDTH: u32 = 640;
pub const FRAME_WIDTH_RANGE: (u32, u32) = (64, 1920);

fn perr(msg: impl Into<String>) -> ControlError {
    ControlError::new(ERR_BAD_PARAMS, msg)
}

// ---- project argument -----------------------------------------------------------------

/// The project folder `arg` names: a folder name as `record.projects` lists it (resolved in
/// `root`; no separators, no `..`) or an absolute path to a project anywhere (the
/// `editor_open` rule: a directory holding a `project.json`). Comes back canonical.
pub fn resolve_project(root: &Path, arg: &str) -> Result<PathBuf, ControlError> {
    let arg = arg.trim();
    if arg.is_empty() {
        return Err(ControlError::new(ERR_BAD_PROJECT, "project is empty"));
    }
    let p = Path::new(arg);
    let candidate = if p.is_absolute() {
        p.to_path_buf()
    } else {
        if arg.contains('/') || arg.contains('\\') || arg == "." || arg == ".." || arg.contains('\0') {
            return Err(ControlError::new(
                ERR_BAD_PROJECT,
                "give a project folder name as listed by record.projects, or an absolute path",
            ));
        }
        root.join(arg)
    };
    super::editor_ipc::open_target(&candidate)
        .map_err(|code| ControlError::new(code, format!("{arg} is not a record project")))
}

// ---- pure document ops ----------------------------------------------------------------

/// What [`apply_ops`] needs besides the document.
pub struct OpCtx<'a> {
    /// Length of the project's own take.
    pub duration_ms: u64,
    pub takes: &'a TakeMetas,
    /// The project's own pointer recording (auto zoom).
    pub cursor: Option<&'a CursorRecording>,
    /// `preset.apply`: the document with the preset file applied (copies its files).
    pub preset: &'a dyn Fn(&str, &EditDoc) -> Result<EditDoc, ControlError>,
    /// `track.add`: the audio file given as `file`, already copied into the project
    /// (`(asset file name, length ms)`); `None` = not imported.
    pub audio: &'a dyn Fn(&str) -> Option<(String, u64)>,
    /// `format.add`: some clip has a camera (a new portrait format then starts stacked).
    pub has_camera: bool,
}

/// The outcome of [`apply_ops`].
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub doc: EditDoc,
    /// `(op index, id)` of every region an op created.
    pub created: Vec<(usize, String)>,
    /// A `rename` op's cleaned title (the last one wins).
    pub title: Option<String>,
}

/// One op's arguments; [`Args::finish`] refuses a key nothing read.
struct Args {
    map: Map<String, Value>,
}

impl Args {
    fn take(&mut self, k: &str) -> Option<Value> {
        self.map.remove(k).filter(|v| !v.is_null())
    }
    fn u64(&mut self, k: &str) -> Result<Option<u64>, String> {
        match self.take(k) {
            None => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("{k} must be a non-negative integer")),
        }
    }
    fn usize(&mut self, k: &str) -> Result<Option<usize>, String> {
        Ok(self.u64(k)?.map(|v| usize::try_from(v).unwrap_or(usize::MAX)))
    }
    fn need_usize(&mut self, k: &str) -> Result<usize, String> {
        self.usize(k)?.ok_or_else(|| format!("{k} is required"))
    }
    fn f64(&mut self, k: &str) -> Result<Option<f64>, String> {
        match self.take(k) {
            None => Ok(None),
            Some(v) => v.as_f64().filter(|f| f.is_finite()).map(Some).ok_or_else(|| format!("{k} must be a number")),
        }
    }
    fn str(&mut self, k: &str) -> Result<Option<String>, String> {
        match self.take(k) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            Some(_) => Err(format!("{k} must be a string")),
        }
    }
    fn need_str(&mut self, k: &str) -> Result<String, String> {
        self.str(k)?.ok_or_else(|| format!("{k} is required"))
    }
    fn bool(&mut self, k: &str) -> Result<Option<bool>, String> {
        match self.take(k) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(b)),
            Some(_) => Err(format!("{k} must be true or false")),
        }
    }
    fn object(&mut self, k: &str) -> Result<Option<Map<String, Value>>, String> {
        match self.take(k) {
            None => Ok(None),
            Some(Value::Object(m)) => Ok(Some(m)),
            Some(_) => Err(format!("{k} must be an object")),
        }
    }
    fn finish(self) -> Result<(), String> {
        match self.map.keys().next() {
            Some(k) => Err(format!("unknown field {k}")),
            None => Ok(()),
        }
    }
}

/// An op's failure: its code and a message naming the op.
struct OpErr {
    code: String,
    msg: String,
}

fn bad(msg: impl Into<String>) -> OpErr {
    OpErr { code: ERR_BAD_OP.into(), msg: msg.into() }
}
fn err(code: &str, msg: impl Into<String>) -> OpErr {
    OpErr { code: code.into(), msg: msg.into() }
}
impl From<String> for OpErr {
    fn from(msg: String) -> Self {
        bad(msg)
    }
}

/// Where a new region (or a split) goes: `atProgrammeMs` on the programme (mapped with
/// `programme_to_source` on the playing document), or `clip` + `startMs` in that clip's SOURCE
/// ms (default: the clip's start). `durationMs` is programme ms with `atProgrammeMs`, else
/// source ms. Returns `(clip, start, end)` with the end clamped into the clip.
fn place(a: &mut Args, doc: &EditDoc, play: &EditDoc, default_len: u64) -> Result<(usize, u64, u64), OpErr> {
    let at = a.u64("atProgrammeMs")?;
    let clip = a.usize("clip")?;
    let start = a.u64("startMs")?;
    let dur = a.u64("durationMs")?.unwrap_or(default_len);
    match (at, clip) {
        (Some(_), Some(_)) | (Some(_), None) if start.is_some() => Err(bad("give atProgrammeMs or clip+startMs, not both")),
        (Some(_), Some(_)) => Err(bad("give atProgrammeMs or clip, not both")),
        (Some(t), None) => {
            let (c, s) = editor::programme_to_source(play, t)
                .ok_or_else(|| err(ERR_OUT_OF_RANGE, format!("atProgrammeMs {t} is past the end")))?;
            let end = match editor::programme_to_source(play, t.saturating_add(dur)) {
                Some((c2, e)) if c2 == c => e,
                _ => doc.clips[c].end_ms,
            };
            Ok((c, s, end.min(doc.clips[c].end_ms).max(s)))
        }
        (None, Some(c)) => {
            let clip = doc.clips.get(c).ok_or_else(|| err(ERR_BAD_CLIP, format!("no clip {c}")))?;
            let s = start.unwrap_or(clip.start_ms);
            if s < clip.start_ms || s > clip.end_ms {
                return Err(err(
                    ERR_OUT_OF_RANGE,
                    format!("startMs {s} is outside clip {c} ({}..{})", clip.start_ms, clip.end_ms),
                ));
            }
            Ok((c, s, s.saturating_add(dur).min(clip.end_ms)))
        }
        (None, None) => Err(bad("give atProgrammeMs, or clip (and startMs)")),
    }
}

fn clip_index(doc: &EditDoc, c: usize) -> Result<usize, OpErr> {
    if c < doc.clips.len() { Ok(c) } else { Err(err(ERR_BAD_CLIP, format!("no clip {c} (there are {})", doc.clips.len()))) }
}

/// Merges `patch` into `base` (objects recursively, everything else replaced).
pub fn merge_json(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(k) {
                    Some(slot) if slot.is_object() && v.is_object() => merge_json(slot, v),
                    _ => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// `region` with `patch` merged in (id kept).
fn patched<T: serde::Serialize + serde::de::DeserializeOwned>(region: &T, patch: &Map<String, Value>) -> Result<T, OpErr> {
    if patch.contains_key("id") {
        return Err(bad("patch may not change id"));
    }
    let mut v = serde_json::to_value(region).map_err(|e| bad(e.to_string()))?;
    merge_json(&mut v, &Value::Object(patch.clone()));
    serde_json::from_value(v).map_err(|e| bad(format!("patch: {e}")))
}

fn fresh_id(given: Option<String>, taken: impl Fn(&str) -> bool, new_id: &mut dyn FnMut() -> String) -> Result<String, OpErr> {
    match given.map(|s| s.trim().to_string()) {
        Some(id) if id.is_empty() => Err(bad("id is empty")),
        Some(id) if taken(&id) => Err(err(ERR_ID_TAKEN, format!("id {id} is taken"))),
        Some(id) => Ok(id),
        None => {
            let mut id = new_id();
            while taken(&id) {
                id = new_id();
            }
            Ok(id)
        }
    }
}

/// Clip `clip` split at source `at`: regions and crop follow the halves; a zoom, annotation
/// or effect spanning the cut goes into BOTH halves (the second under a fresh id); the second
/// half starts with the layout in force at the cut (port of `splitAt` in lib/editor.ts).
pub fn split_clip(doc: &EditDoc, clip: usize, at: u64, new_id: &mut dyn FnMut() -> String) -> Option<EditDoc> {
    let c = doc.clips.get(clip)?.clone();
    if at < c.start_ms + MIN_CLIP_MS || c.end_ms < at + MIN_CLIP_MS {
        return None;
    }
    let mut next = doc.clone();
    next.clips[clip] = EditClip { end_ms: at, ..c.clone() };
    next.clips.insert(clip + 1, EditClip { start_ms: at, ..c });
    next.crop.resize(doc.clips.len(), None);
    let crop = next.crop[clip];
    next.crop.insert(clip + 1, crop);
    let shift = |k: usize| if k > clip { k + 1 } else { k };
    next.speed = doc
        .speed
        .iter()
        .flat_map(|r| {
            if r.clip != clip {
                vec![SpeedRegion { clip: shift(r.clip), ..*r }]
            } else if r.end_ms <= at {
                vec![*r]
            } else if r.start_ms >= at {
                vec![SpeedRegion { clip: clip + 1, ..*r }]
            } else {
                vec![SpeedRegion { end_ms: at, ..*r }, SpeedRegion { clip: clip + 1, start_ms: at, ..*r }]
            }
        })
        .collect();
    macro_rules! split_regions {
        ($list:expr) => {{
            let mut out = Vec::new();
            for r in $list.iter() {
                if r.clip != clip {
                    let mut r = r.clone();
                    r.clip = shift(r.clip);
                    out.push(r);
                } else if r.start_ms >= at {
                    let mut r = r.clone();
                    r.clip = clip + 1;
                    out.push(r);
                } else if r.end_ms <= at {
                    out.push(r.clone());
                } else {
                    let mut a = r.clone();
                    a.end_ms = at;
                    let mut b = r.clone();
                    b.id = new_id();
                    b.clip = clip + 1;
                    b.start_ms = at;
                    out.push(a);
                    out.push(b);
                }
            }
            out
        }};
    }
    next.zooms = split_regions!(doc.zooms);
    next.annotations = split_regions!(doc.annotations);
    next.effects = split_regions!(doc.effects);
    for t in &mut next.audio_tracks {
        if t.clip > clip || (t.clip == clip && t.start_ms >= at) {
            t.clip += 1;
        }
    }
    let mut layouts: Vec<LayoutRegion> = doc
        .layout_regions
        .iter()
        .map(|r| {
            let mut r = r.clone();
            if r.clip > clip || (r.clip == clip && r.start_ms >= at) {
                r.clip += 1;
            }
            r
        })
        .collect();
    let in_force = doc.layout_regions.iter().filter(|r| r.clip == clip && r.start_ms < at).max_by_key(|r| r.start_ms);
    let own_at_cut = doc.layout_regions.iter().any(|r| r.clip == clip && r.start_ms == at);
    if let (Some(r), false) = (in_force, own_at_cut) {
        layouts.push(LayoutRegion { id: new_id(), clip: clip + 1, start_ms: at, preset: r.preset });
    }
    next.layout_regions = layouts;
    Some(next)
}

/// Clip `clip` removed with everything on it (port of `deleteClip`). The last clip stays.
pub fn delete_clip(doc: &EditDoc, clip: usize) -> Option<EditDoc> {
    if doc.clips.len() <= 1 || clip >= doc.clips.len() {
        return None;
    }
    let mut next = doc.clone();
    next.clips.remove(clip);
    next.crop.resize(doc.clips.len(), None);
    next.crop.remove(clip);
    let keep = |k: usize| k != clip;
    let down = |k: usize| if k > clip { k - 1 } else { k };
    next.speed.retain(|r| keep(r.clip));
    next.speed.iter_mut().for_each(|r| r.clip = down(r.clip));
    next.zooms.retain(|r| keep(r.clip));
    next.zooms.iter_mut().for_each(|r| r.clip = down(r.clip));
    next.annotations.retain(|r| keep(r.clip));
    next.annotations.iter_mut().for_each(|r| r.clip = down(r.clip));
    next.effects.retain(|r| keep(r.clip));
    next.effects.iter_mut().for_each(|r| r.clip = down(r.clip));
    next.audio_tracks.retain(|r| keep(r.clip));
    next.audio_tracks.iter_mut().for_each(|r| r.clip = down(r.clip));
    next.layout_regions.retain(|r| keep(r.clip));
    next.layout_regions.iter_mut().for_each(|r| r.clip = down(r.clip));
    Some(next)
}

/// Transcripts laid into `doc` (one per recording, replaced by source), with the captions'
/// language; the first run with words turns the captions on (port of `mergeTranscripts`).
pub fn merge_transcripts(doc: &EditDoc, next: &[Transcript], lang: Option<transcribe_protocol::Lang>) -> EditDoc {
    let mut out = doc.clone();
    for t in next {
        match out.transcripts.iter_mut().find(|o| o.source == t.source) {
            Some(slot) => *slot = t.clone(),
            None => out.transcripts.push(t.clone()),
        }
    }
    let before = doc.transcripts.iter().any(|t| !t.words.is_empty());
    let now = out.transcripts.iter().any(|t| !t.words.is_empty());
    if let Some(l) = lang {
        out.captions.lang = l;
    }
    if !before && now {
        out.captions.enabled = true;
    }
    out
}

/// Nested fields that are left out of the serialised document while unset
/// (`skip_serializing_if`), so they cannot be found in it.
const OPTIONAL_LOOK_KEYS: [&str; 1] = ["camera.position"];

/// The first key path of `patch` that `base` (the serialised current value) does not have,
/// e.g. `appearance.paddin`. An object whose `kind` tag changes is a different enum variant
/// with its own fields: it is not checked further (deserialising it validates the variant).
pub fn unknown_key(base: &Value, patch: &Value, path: &str) -> Option<String> {
    let (Value::Object(b), Value::Object(p)) = (base, patch) else { return None };
    if p.get("kind").is_some_and(|k| Some(k) != b.get("kind")) {
        return None;
    }
    for (k, v) in p {
        let here = format!("{path}.{k}");
        match b.get(k) {
            Some(bv) => {
                if let Some(found) = unknown_key(bv, v, &here) {
                    return Some(found);
                }
            }
            None if OPTIONAL_LOOK_KEYS.contains(&here.as_str()) => {}
            None => return Some(here),
        }
    }
    None
}

/// Top-level fields `look.set` may change.
pub const LOOK_FIELDS: [&str; 8] = ["appearance", "cursor", "output", "camera", "captions", "micGainDb", "systemGainDb", "micDenoise"];

/// The `ops` applied to `doc` in order, each normalised before the next (indices in a later
/// op see the document as the earlier ops left it). All or nothing: the first failing op
/// returns its error and nothing of the others is kept.
pub fn apply_ops(
    doc: &EditDoc,
    ops: &[Value],
    ctx: &OpCtx,
    new_id: &mut dyn FnMut() -> String,
) -> Result<Applied, ControlError> {
    if ops.is_empty() {
        return Err(perr("ops is empty"));
    }
    let mut cur = editor::normalize(doc.clone(), ctx.duration_ms, ctx.takes);
    let mut created = Vec::new();
    let mut title = None;
    for (i, op) in ops.iter().enumerate() {
        let kind = op.get("op").and_then(Value::as_str).unwrap_or("?").to_string();
        let step = apply_one(&cur, op, ctx, new_id).map_err(|e| ControlError::new(&e.code, format!("op {i} ({kind}): {}", e.msg)))?;
        let normalized = editor::normalize(step.doc, ctx.duration_ms, ctx.takes);
        if let Some(id) = &step.check {
            if !region_exists(&normalized, &step.kind, id) {
                return Err(ControlError::new(
                    ERR_DROPPED,
                    format!("op {i} ({kind}): region {id} was dropped (too short or outside its clip)"),
                ));
            }
        }
        if let Some(id) = step.created {
            created.push((i, id));
        }
        if step.title.is_some() {
            title = step.title;
        }
        cur = normalized;
    }
    Ok(Applied { doc: cur, created, title })
}

struct Step {
    doc: EditDoc,
    kind: String,
    /// A region that must survive normalisation.
    check: Option<String>,
    created: Option<String>,
    title: Option<String>,
}

fn region_exists(doc: &EditDoc, kind: &str, id: &str) -> bool {
    match kind.split('.').next() {
        Some("zoom") => doc.zooms.iter().any(|z| z.id == id),
        Some("annotation") => doc.annotations.iter().any(|a| a.id == id),
        Some("effect") => doc.effects.iter().any(|e| e.id == id),
        _ => true,
    }
}

fn apply_one(doc: &EditDoc, op: &Value, ctx: &OpCtx, new_id: &mut dyn FnMut() -> String) -> Result<Step, OpErr> {
    let mut map = op.as_object().cloned().ok_or_else(|| bad("an op must be an object"))?;
    let kind = match map.remove("op") {
        Some(Value::String(s)) => s,
        _ => return Err(bad("missing op")),
    };
    let mut a = Args { map };
    let play = editor::playable(doc, ctx.takes);
    let mut next = doc.clone();
    let mut check = None;
    let mut created = None;
    let mut title = None;
    match kind.as_str() {
        "zoom.add" => {
            let (clip, start, end) = place(&mut a, doc, &play, NEW_ZOOM_MS)?;
            let id = fresh_id(a.str("id")?, |i| doc.zooms.iter().any(|z| z.id == i), new_id)?;
            let depth = a.u64("depth")?.unwrap_or(DEFAULT_ZOOM_DEPTH as u64);
            if !(1..=6).contains(&depth) {
                return Err(bad("depth must be 1..6"));
            }
            let fx = a.f64("focusX")?;
            let fy = a.f64("focusY")?;
            let rotation = match a.take("rotation") {
                None => None,
                Some(v) => Some(serde_json::from_value(v).map_err(|_| bad("rotation must be left, right or orbit"))?),
            };
            let z = ZoomRegion {
                id: id.clone(),
                clip,
                start_ms: start,
                end_ms: end,
                depth: depth as u8,
                custom_scale: a.f64("customScale")?,
                focus_x: fx.unwrap_or(0.5),
                focus_y: fy.unwrap_or(0.5),
                focus_mode: if fx.is_some() || fy.is_some() { FocusMode::Manual } else { FocusMode::Auto },
                origin: ZoomOrigin::Manual,
                hide_cursor: a.bool("hideCursor")?.unwrap_or(false),
                rotation,
            };
            next.zooms.push(z);
            check = Some(id.clone());
            created = Some(id);
        }
        "zoom.set" | "annotation.set" | "effect.set" => {
            let id = a.need_str("id")?;
            let patch = a.object("patch")?.unwrap_or_default();
            let moved = if a.map.contains_key("atProgrammeMs") || a.map.contains_key("clip") {
                let len = region_len(doc, &kind, &id);
                Some(place(&mut a, doc, &play, len.unwrap_or(NEW_ZOOM_MS))?)
            } else {
                None
            };
            let no = || err(ERR_NO_REGION, format!("no region {id}"));
            macro_rules! set {
                ($list:expr) => {{
                    let at = $list.iter().position(|r| r.id == id).ok_or_else(no)?;
                    let mut r = patched(&$list[at], &patch)?;
                    if let Some((c, s, e)) = moved {
                        r.clip = c;
                        r.start_ms = s;
                        r.end_ms = e;
                    }
                    clip_index(doc, r.clip)?;
                    $list[at] = r;
                    at
                }};
            }
            match kind.as_str() {
                "zoom.set" => {
                    let at = set!(next.zooms);
                    // An edited planned zoom is the user's now ("Remove auto-zooms" keeps it).
                    next.zooms[at].origin = ZoomOrigin::Manual;
                }
                "annotation.set" => {
                    set!(next.annotations);
                }
                _ => {
                    let at = set!(next.effects);
                    if super::effects::builtin(&next.effects[at].effect).is_none() {
                        return Err(err(ERR_UNKNOWN_EFFECT, format!("unknown effect {}", next.effects[at].effect)));
                    }
                }
            }
            check = Some(id);
        }
        "zoom.remove" | "annotation.remove" | "effect.remove" => {
            let id = a.need_str("id")?;
            let before = next.zooms.len() + next.annotations.len() + next.effects.len();
            match kind.as_str() {
                "zoom.remove" => next.zooms.retain(|r| r.id != id),
                "annotation.remove" => next.annotations.retain(|r| r.id != id),
                _ => next.effects.retain(|r| r.id != id),
            }
            if next.zooms.len() + next.annotations.len() + next.effects.len() == before {
                return Err(err(ERR_NO_REGION, format!("no region {id}")));
            }
        }
        "zoom.auto" => {
            next = super::editor_ipc::with_auto_zooms(doc, ctx.cursor, ctx.duration_ms, ctx.takes)
                .ok_or_else(|| err(ERR_NO_CURSOR, "this take has no pointer data to plan zooms from"))?;
        }
        "clip.split" => {
            let (clip, at) = match a.u64("atProgrammeMs")? {
                Some(t) => {
                    if a.map.contains_key("clip") || a.map.contains_key("sourceMs") {
                        return Err(bad("give atProgrammeMs or clip+sourceMs, not both"));
                    }
                    editor::programme_to_source(&play, t)
                        .ok_or_else(|| err(ERR_OUT_OF_RANGE, format!("atProgrammeMs {t} is past the end")))?
                }
                None => {
                    let c = clip_index(doc, a.need_usize("clip")?)?;
                    (c, a.u64("sourceMs")?.ok_or_else(|| bad("sourceMs (or atProgrammeMs) is required"))?)
                }
            };
            next = split_clip(doc, clip, at, new_id).ok_or_else(|| {
                err(ERR_BAD_SPLIT, format!("a split needs {MIN_CLIP_MS} ms on both sides inside clip {clip}"))
            })?;
        }
        "clip.delete" => {
            let c = clip_index(doc, a.need_usize("clip")?)?;
            next = delete_clip(doc, c).ok_or_else(|| err(ERR_LAST_CLIP, "the last clip cannot be deleted"))?;
        }
        "clip.move" => {
            let c = clip_index(doc, a.need_usize("clip")?)?;
            let to = clip_index(doc, a.need_usize("to")?)?;
            next = editor::move_clip(doc, c, to);
        }
        "clip.duplicate" => {
            let c = clip_index(doc, a.need_usize("clip")?)?;
            next = editor::duplicate_clip(doc, c);
        }
        "clip.speed" => {
            let speed = a.f64("speed")?.ok_or_else(|| bad("speed is required"))?;
            if !(MIN_SPEED..=MAX_SPEED).contains(&speed) {
                return Err(bad(format!("speed must be {MIN_SPEED}..{MAX_SPEED}")));
            }
            let (clip, start, end) = if a.map.contains_key("atProgrammeMs") {
                place(&mut a, doc, &play, u64::MAX / 4)?
            } else {
                let c = clip_index(doc, a.need_usize("clip")?)?;
                let cl = &doc.clips[c];
                let s = a.u64("startMs")?.unwrap_or(cl.start_ms).clamp(cl.start_ms, cl.end_ms);
                let e = a.u64("endMs")?.unwrap_or(cl.end_ms).clamp(cl.start_ms, cl.end_ms);
                (c, s.min(e), s.max(e))
            };
            if end <= start {
                return Err(err(ERR_OUT_OF_RANGE, "the speed range is empty"));
            }
            next.speed.push(SpeedRegion { clip, start_ms: start, end_ms: end, speed });
        }
        "clip.crop" => {
            let c = clip_index(doc, a.need_usize("clip")?)?;
            let crop: Option<CropRect> = match a.take("crop") {
                None => None,
                Some(v) => Some(serde_json::from_value(v).map_err(|_| bad("crop must be {x,y,w,h} in 0..1 or null"))?),
            };
            next.crop.resize(doc.clips.len(), None);
            next.crop[c] = crop;
        }
        "annotation.add" => {
            let (clip, start, end) = place(&mut a, doc, &play, NEW_ZOOM_MS)?;
            let id = fresh_id(a.str("id")?, |i| doc.annotations.iter().any(|x| x.id == i), new_id)?;
            let body: AnnotationBody = serde_json::from_value(a.take("body").ok_or_else(|| bad("body is required"))?)
                .map_err(|e| bad(format!("body: {e} (kind text, image, arrow or blur)")))?;
            let mut ann = Annotation::new(id.clone(), clip, start, end, body, &doc.annotations);
            for (k, slot) in [("x", &mut ann.x), ("y", &mut ann.y), ("w", &mut ann.w), ("h", &mut ann.h)] {
                if let Some(v) = a.f64(k)? {
                    *slot = v;
                }
            }
            next.annotations.push(ann);
            check = Some(id.clone());
            created = Some(id);
        }
        "effect.add" => {
            let effect = a.need_str("effect")?;
            if super::effects::builtin(&effect).is_none() {
                return Err(err(ERR_UNKNOWN_EFFECT, format!("unknown effect {effect}")));
            }
            let (clip, start, end) = place(&mut a, doc, &play, NEW_ZOOM_MS)?;
            let id = fresh_id(a.str("id")?, |i| doc.effects.iter().any(|x| x.id == i), new_id)?;
            let values = match a.take("values") {
                None => Default::default(),
                Some(v) => serde_json::from_value(v).map_err(|e| bad(format!("values: {e}")))?,
            };
            next.effects.push(EffectRegion { id: id.clone(), clip, start_ms: start, end_ms: end, effect, values });
            check = Some(id.clone());
            created = Some(id);
        }
        "format.add" | "format.switch" | "format.remove" => {
            use super::formats;
            let aspect: editor::Aspect = serde_json::from_value(a.take("aspect").ok_or_else(|| bad("aspect is missing"))?)
                .map_err(|_| bad("aspect must be one of 16:9, 9:16, 1:1, 4:3, 3:4, 21:9"))?;
            next = match kind.as_str() {
                "format.add" if formats::ADDABLE.contains(&aspect) => formats::add_variant(doc, aspect, ctx.has_camera),
                "format.add" => return Err(bad("aspect must be one of 16:9, 9:16, 1:1, 4:3, 3:4, 21:9")),
                "format.switch" => formats::switch_variant(doc, aspect).ok_or_else(|| bad("the project has no such format (format.add makes one)"))?,
                _ => formats::remove_variant(doc, aspect),
            };
        }
        "look.set" => {
            let mut v = serde_json::to_value(doc).map_err(|e| bad(e.to_string()))?;
            let mut any = false;
            for k in LOOK_FIELDS {
                if let Some(p) = a.take(k) {
                    any = true;
                    if let Some(bad_key) = v.get(k).and_then(|base| unknown_key(base, &p, k)) {
                        return Err(bad(format!("unknown field {bad_key}")));
                    }
                    let slot = v.as_object_mut().expect("a doc serialises to an object").entry(k).or_insert(Value::Null);
                    merge_json(slot, &p);
                }
            }
            if !any {
                return Err(bad(format!("give at least one of {}", LOOK_FIELDS.join(", "))));
            }
            next = serde_json::from_value(v).map_err(|e| bad(format!("look: {e}")))?;
            // Setting the aspect to one the project keeps as its own format would overwrite it.
            if next.output.aspect != doc.output.aspect && doc.variants.iter().any(|x| x.aspect == next.output.aspect) {
                return Err(bad("this aspect is one of the project's formats: use format.switch"));
            }
        }
        "track.add" => {
            let path = a.need_str("file")?;
            let (file, duration_ms) =
                (ctx.audio)(&path).ok_or_else(|| bad(format!("file {path} was not imported (an absolute path to an audio file)")))?;
            let kind = match a.str("kind")?.as_deref() {
                None | Some("voiceover") => TrackKind::Voiceover,
                Some("music") => TrackKind::Music,
                Some(o) => return Err(bad(format!("kind must be voiceover or music, not {o}"))),
            };
            let (clip, start_ms) = match (a.u64("atProgrammeMs")?, a.usize("clip")?) {
                (Some(_), Some(_)) => return Err(bad("give atProgrammeMs or clip (+ startMs), not both")),
                (Some(t), None) => editor::programme_to_source(&play, t)
                    .ok_or_else(|| err(ERR_OUT_OF_RANGE, format!("atProgrammeMs {t} is past the end")))?,
                (None, c) => {
                    let c = clip_index(doc, c.unwrap_or(0))?;
                    let clip = &doc.clips[c];
                    let s = a.u64("startMs")?.unwrap_or(clip.start_ms);
                    if s < clip.start_ms || s > clip.end_ms {
                        return Err(err(
                            ERR_OUT_OF_RANGE,
                            format!("startMs {s} is outside clip {c} ({}..{})", clip.start_ms, clip.end_ms),
                        ));
                    }
                    (c, s)
                }
            };
            let id = fresh_id(a.str("id")?, |i| doc.audio_tracks.iter().any(|t| t.id == i), new_id)?;
            let music = kind == TrackKind::Music;
            let fade = if music { MUSIC_DEFAULT_FADE_MS } else { 0 };
            next.audio_tracks.push(AudioTrack {
                id: id.clone(),
                kind,
                file,
                clip,
                start_ms,
                duration_ms,
                offset_ms: 0,
                span_ms: (!music).then_some(duration_ms),
                gain_db: a.f64("gainDb")?.unwrap_or(if music { MUSIC_DEFAULT_GAIN_DB } else { 0.0 }),
                looped: a.bool("looped")?.unwrap_or(false),
                fade_in_ms: a.u64("fadeInMs")?.unwrap_or(fade),
                fade_out_ms: a.u64("fadeOutMs")?.unwrap_or(fade),
                muted: false,
                label: a.str("label")?.unwrap_or_default(),
            });
            created = Some(id);
        }
        "track.set" => {
            let id = a.need_str("id")?;
            let t = next.audio_tracks.iter_mut().find(|t| t.id == id).ok_or_else(|| err(ERR_NO_REGION, format!("no track {id}")))?;
            if let Some(v) = a.f64("gainDb")? {
                t.gain_db = v;
            }
            if let Some(v) = a.bool("muted")? {
                t.muted = v;
            }
            if let Some(v) = a.bool("looped")? {
                t.looped = v;
            }
            if let Some(v) = a.u64("fadeInMs")? {
                t.fade_in_ms = v;
            }
            if let Some(v) = a.u64("fadeOutMs")? {
                t.fade_out_ms = v;
            }
            if let Some(v) = a.str("label")? {
                t.label = v;
            }
        }
        "track.remove" => {
            let id = a.need_str("id")?;
            let before = next.audio_tracks.len();
            next.audio_tracks.retain(|t| t.id != id);
            if next.audio_tracks.len() == before {
                return Err(err(ERR_NO_REGION, format!("no track {id}")));
            }
        }
        "preset.apply" => {
            let file = a.need_str("file")?;
            next = (ctx.preset)(&file, doc).map_err(|e| OpErr { code: e.code, msg: e.message })?;
        }
        "rename" => {
            let t = a.need_str("title")?;
            title = Some(super::project::clean_title(&t).map_err(|c| err(c, "the title is not usable"))?);
        }
        other => return Err(bad(format!("unknown op {other}"))),
    }
    a.finish()?;
    Ok(Step { doc: next, kind, check, created, title })
}

/// A region's current source length (`set` that moves it keeps its length).
fn region_len(doc: &EditDoc, kind: &str, id: &str) -> Option<u64> {
    let span = |s: u64, e: u64| Some(e.saturating_sub(s));
    match kind.split('.').next()? {
        "zoom" => doc.zooms.iter().find(|r| r.id == id).and_then(|r| span(r.start_ms, r.end_ms)),
        "annotation" => doc.annotations.iter().find(|r| r.id == id).and_then(|r| span(r.start_ms, r.end_ms)),
        "effect" => doc.effects.iter().find(|r| r.id == id).and_then(|r| span(r.start_ms, r.end_ms)),
        _ => None,
    }
}

/// Frame indices on the `fps` grid for programme times `ms` (each `<= programme_ms`).
pub fn frame_indices(ms: &[u64], programme_ms: u64, fps: u32) -> Result<Vec<u64>, ControlError> {
    if ms.is_empty() || ms.len() > MAX_FRAMES {
        return Err(perr(format!("atProgrammeMs needs 1..{MAX_FRAMES} times")));
    }
    let last = (programme_ms * fps as u64 / 1000).saturating_sub(1);
    ms.iter()
        .map(|&t| {
            if t > programme_ms {
                Err(ControlError::new(ERR_OUT_OF_RANGE, format!("{t} ms is past the end ({programme_ms} ms)")))
            } else {
                Ok((t * fps as u64 / 1000).min(last))
            }
        })
        .collect()
}

/// The export target `out`: an absolute path in an existing folder, with the format's
/// extension (added when missing), that does not exist yet (never overwrites).
pub fn export_out(out: &str, ext: &str) -> Result<PathBuf, ControlError> {
    let p = Path::new(out.trim());
    if !p.is_absolute() {
        return Err(ControlError::new(ERR_BAD_OUT, "out must be an absolute path"));
    }
    // A dot in the name (`demo.v2`) is part of the name; only another video/image
    // format's extension is a contradiction worth refusing.
    const OTHER_FORMATS: [&str; 7] = ["mp4", "m4v", "mov", "gif", "webm", "mkv", "avi"];
    let p = match p.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case(ext) => p.to_path_buf(),
        Some(e) if OTHER_FORMATS.iter().any(|f| f.eq_ignore_ascii_case(e)) => {
            return Err(ControlError::new(ERR_BAD_OUT, format!("out ends in .{e}, the format writes .{ext}")))
        }
        _ => {
            let mut name = p.as_os_str().to_os_string();
            name.push(".");
            name.push(ext);
            PathBuf::from(name)
        }
    };
    let parent = p.parent().filter(|d| d.is_dir()).ok_or_else(|| ControlError::new(ERR_BAD_OUT, "out's folder does not exist"))?;
    let name = p.file_name().ok_or_else(|| ControlError::new(ERR_BAD_OUT, "out has no file name"))?;
    let full = std::fs::canonicalize(parent).map_err(|_| ControlError::new(ERR_BAD_OUT, "out's folder is not readable"))?.join(name);
    if std::fs::symlink_metadata(&full).is_ok() {
        return Err(ControlError::new(ERR_OUT_EXISTS, format!("{} exists", full.display())));
    }
    Ok(full)
}

// ---- handlers -------------------------------------------------------------------------

fn root(app: &AppHandle) -> PathBuf {
    use tauri::Manager;
    app.try_state::<crate::db::DbHandle>()
        .map(|db| super::RecordConfig::load(&db).root())
        .unwrap_or_else(|| super::session::resolve_root(""))
}

fn param_str(c: &Call, k: &str) -> Result<String, ControlError> {
    match c.params.get(k) {
        Some(Value::String(s)) => Ok(s.clone()),
        None | Some(Value::Null) => Err(perr(format!("{k} is required"))),
        Some(_) => Err(perr(format!("{k} must be a string"))),
    }
}

fn project_dir(app: &AppHandle, c: &Call) -> Result<PathBuf, ControlError> {
    resolve_project(&root(app), &param_str(c, "project")?)
}

fn session_for(app: &AppHandle, c: &Call) -> Result<u64, ControlError> {
    let dir = project_dir(app, c)?;
    super::editor_ipc::control_session(app, &dir).map_err(ControlError::sentinel)
}

fn to_json(v: &impl serde::Serialize) -> Result<Value, ControlError> {
    serde_json::to_value(v).map_err(|e| ControlError::new("control.internal", e.to_string()))
}

fn projects(app: &AppHandle, _c: &mut Call) -> Result<Value, ControlError> {
    let open = super::editor_ipc::open_dir();
    let shown = open.is_some() && super::editor_ipc::editor_window_exists(app);
    // Through `list_items`, like the UI: the take being recorded right now is
    // `recording`, not the "interrupted" a bare `list_projects` makes of it.
    let active = tauri::Manager::try_state::<super::session::RecordState>(app).and_then(|s| s.active_dir());
    let list: Vec<Value> = super::session::list_items_with(&root(app), active.as_deref(), None, false)
        .into_iter()
        .map(|item| item.summary)
        .map(|p| {
            let folder = p.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let canon = std::fs::canonicalize(&p.dir).ok();
            let is_open = open.is_some() && canon == open;
            let mut v = serde_json::to_value(&p).unwrap_or(Value::Null);
            if let Some(o) = v.as_object_mut() {
                o.remove("thumb");
                o.insert("folder".into(), json!(folder));
                o.insert("open".into(), json!(is_open && shown));
            }
            v
        })
        .collect();
    Ok(Value::Array(list))
}

fn project(app: &AppHandle, c: &mut Call) -> Result<Value, ControlError> {
    let id = session_for(app, c)?;
    let (project, view) = super::editor_ipc::control_view(id).map_err(ControlError::sentinel)?;
    let mut p = to_json(&project)?;
    if let Some(o) = p.as_object_mut() {
        o.remove("editor");
    }
    Ok(json!({"project": p, "view": to_json(&view)?, "open": super::editor_ipc::editor_window_exists(app)}))
}

fn doc(app: &AppHandle, c: &mut Call) -> Result<Value, ControlError> {
    let id = session_for(app, c)?;
    let (_, view) = super::editor_ipc::control_view(id).map_err(ControlError::sentinel)?;
    Ok(json!({"doc": to_json(&view.doc)?, "view": to_json(&view)?}))
}

fn transcript(app: &AppHandle, c: &mut Call) -> Result<Value, ControlError> {
    let id = session_for(app, c)?;
    let (_, view) = super::editor_ipc::control_view(id).map_err(ControlError::sentinel)?;
    Ok(json!({"transcripts": to_json(&view.doc.transcripts)?}))
}

fn random_id() -> String {
    let b: [u8; 16] = rand::random();
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

/// The title of the last `rename` op (the one that wins), if any.
pub fn last_rename(ops: &[Value]) -> Option<String> {
    ops.iter()
        .rev()
        .find(|o| o.get("op").and_then(Value::as_str) == Some("rename"))
        .and_then(|o| o.get("title").and_then(Value::as_str))
        .map(str::to_string)
}

/// The distinct `file` values of the `track.add` ops, in order.
fn track_files(ops: &[Value]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for op in ops {
        if op.get("op").and_then(Value::as_str) != Some("track.add") {
            continue;
        }
        if let Some(f) = op.get("file").and_then(Value::as_str) {
            if !out.iter().any(|x| x == f) {
                out.push(f.to_string());
            }
        }
    }
    out
}

fn edit(app: &AppHandle, c: &mut Call) -> Result<Value, ControlError> {
    let ops = match c.params.get("ops") {
        Some(Value::Array(a)) => a.clone(),
        _ => return Err(perr("ops must be a list of ops")),
    };
    let dir = project_dir(app, c)?;
    // A rename that would be refused fails before the document changes (all or nothing).
    if let Some(t) = last_rename(&ops) {
        super::session::rename_check(app, &dir, &t).map_err(ControlError::sentinel)?;
    }
    let id = super::editor_ipc::control_session(app, &dir).map_err(ControlError::sentinel)?;
    // Audio for `track.add` is copied in and measured first, outside the session lock (the
    // helper decodes it); if the edit then fails, the copies go again.
    let mut imported: Vec<(String, String, u64)> = Vec::new();
    for path in track_files(&ops) {
        if !Path::new(&path).is_absolute() {
            remove_imported(&dir, &imported);
            return Err(perr(format!("track.add: file must be an absolute path, not {path}")));
        }
        match super::editor_ipc::import_audio_into(app, &dir, Path::new(&path)) {
            Ok((file, ms)) => imported.push((path, file, ms)),
            Err(e) => {
                remove_imported(&dir, &imported);
                let mut ce = ControlError::sentinel(&e);
                ce.message = format!("track.add {path}: {}", ce.message);
                return Err(ce);
            }
        }
    }
    let root = root(app);
    let result = super::editor_ipc::control_edit(app, id, |p| {
        let preset = |file: &str, d: &EditDoc| -> Result<EditDoc, ControlError> {
            let pr = super::presets::read_preset(&root, file).map_err(ControlError::sentinel)?;
            Ok(super::presets::apply_preset(d, &pr, &root, p.dir))
        };
        let audio = |path: &str| imported.iter().find(|(p, _, _)| p == path).map(|(_, f, ms)| (f.clone(), *ms));
        let ctx = OpCtx { duration_ms: p.duration_ms, takes: p.takes, cursor: p.cursor, preset: &preset, audio: &audio, has_camera: p.has_camera };
        let out = apply_ops(p.doc, &ops, &ctx, &mut random_id)?;
        Ok((out.doc, (out.created, out.title)))
    });
    let (view, (created, title)) = match result {
        Ok(r) => r,
        Err(e) => {
            remove_imported(&dir, &imported);
            return Err(e);
        }
    };
    if let Some(t) = title {
        // Checked above; what can still fail here is the write itself.
        super::session::rename_project(app, &dir, &t).map_err(ControlError::sentinel)?;
    }
    let created: Vec<Value> = created.into_iter().map(|(i, id)| json!({"index": i, "id": id})).collect();
    Ok(json!({"view": to_json(&view)?, "created": created}))
}

fn remove_imported(dir: &Path, imported: &[(String, String, u64)]) {
    for (_, file, _) in imported {
        super::editor_ipc::remove_imported_audio(dir, file);
    }
}

fn export(app: &AppHandle, c: &mut Call) -> Result<Value, ControlError> {
    use super::editor_ipc::{ExportFormat, ExportOptions};
    let p = c.params.clone();
    let mut opts = ExportOptions::default();
    opts.kind = match p.get("format").and_then(Value::as_str).unwrap_or("mp4") {
        "mp4" => ExportFormat::Mp4,
        "gif" => {
            opts.fps = 15;
            ExportFormat::Gif
        }
        other => return Err(perr(format!("format must be mp4 or gif, not {other}"))),
    };
    if let Some(q) = p.get("quality").filter(|v| !v.is_null()) {
        opts.target = serde_json::from_value(q.clone()).map_err(|_| perr("quality must be p720, p1080, p1440, p2160 or source"))?;
    }
    if let Some(v) = p.get("preset").filter(|v| !v.is_null()) {
        let id: super::platform::Platform =
            serde_json::from_value(v.clone()).map_err(|_| perr("preset must be youtube, shorts, reels or tiktok"))?;
        // A preset sets what the platform wants; explicit parameters below still win. The
        // aspect belongs to the document (record.edit look.set).
        let spec = super::platform::preset(id);
        opts.preset = Some(id);
        opts.fps = spec.fps;
        opts.quality = spec.quality;
        opts.loudness = spec.loudness;
        if p.get("quality").is_none_or(Value::is_null) {
            opts.target = spec.target;
        }
    }
    if let Some(v) = p.get("bitrate").filter(|v| !v.is_null()) {
        match v {
            Value::Number(n) => {
                opts.quality = super::platform::Quality::Custom;
                opts.bitrate_mbps = n.as_f64();
            }
            other => {
                opts.quality = serde_json::from_value(other.clone())
                    .ok()
                    .filter(|q| *q != super::platform::Quality::Custom)
                    .ok_or_else(|| perr("bitrate must be auto, small, standard, high, max or a number of Mbit/s"))?;
            }
        }
    }
    if let Some(v) = p.get("bitrateMbps").filter(|v| !v.is_null()) {
        opts.quality = super::platform::Quality::Custom;
        opts.bitrate_mbps = Some(v.as_f64().ok_or_else(|| perr("bitrateMbps must be a number"))?);
    }
    if let Some(v) = p.get("loudness").filter(|v| !v.is_null()) {
        opts.loudness = match v.as_i64() {
            Some(-16) => super::platform::Loudness::L16,
            Some(-14) => super::platform::Loudness::L14,
            _ => return Err(perr("loudness must be -16 or -14")),
        };
    }
    if let Some(v) = p.get("aspect").filter(|v| !v.is_null()) {
        opts.variant = Some(serde_json::from_value(v.clone()).map_err(|_| perr("aspect must be one of the project's formats, e.g. 9:16"))?);
    }
    if let Some(v) = p.get("captions").filter(|v| !v.is_null()) {
        opts.captions_sidecar = Some(serde_json::from_value(v.clone()).map_err(|_| perr("captions must be srt or vtt"))?);
    }
    if let Some(f) = p.get("fps").filter(|v| !v.is_null()) {
        opts.fps = f.as_u64().and_then(|f| u32::try_from(f).ok()).ok_or_else(|| perr("fps must be an integer"))?;
    }
    if let Some(cd) = p.get("codec").filter(|v| !v.is_null()) {
        opts.codec = serde_json::from_value(cd.clone()).map_err(|_| perr("codec must be h264 or hevc"))?;
    }
    if let Some(w) = p.get("gifWidth").filter(|v| !v.is_null()) {
        opts.gif_width = w.as_u64().and_then(|w| u32::try_from(w).ok()).ok_or_else(|| perr("gifWidth must be an integer"))?;
    }
    let out = match p.get("out") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err(perr("out must be a string")),
    };
    if c.is_cancelled() {
        return Err(ControlError::new(ERR_CANCELLED, "the connection closed"));
    }
    let id = session_for(app, c)?;
    let cancelled = c.cancelled;
    let mut sent: Option<std::time::Instant> = None;
    let mut forward = |frames: u64, total: u64| {
        // One event per ~200 ms (and the last frame): the helper reports every frame.
        let due = sent.is_none_or(|t| t.elapsed() >= std::time::Duration::from_millis(200)) || frames >= total;
        if due {
            sent = Some(std::time::Instant::now());
            c.emit(json!({"phase": "export", "frame": frames, "total": total}));
        }
    };
    let path = super::editor_ipc::control_export(app, id, opts, out.as_deref(), cancelled, &mut forward)?;
    Ok(json!({"path": path}))
}

fn transcribe(app: &AppHandle, c: &mut Call) -> Result<Value, ControlError> {
    let lang: transcribe_protocol::Lang = match c.params.get("lang") {
        None | Some(Value::Null) => transcribe_protocol::Lang::Auto,
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| perr("lang must be auto, de or en"))?,
    };
    let id = session_for(app, c)?;
    let (result, view) = super::editor_ipc::control_transcribe(app, id, lang, c.cancelled)?;
    let words: usize = result.transcripts.iter().map(|t| t.words.len()).sum();
    let per: Vec<Value> =
        result.transcripts.iter().map(|t| json!({"source": t.source, "lang": t.lang, "words": t.words.len()})).collect();
    Ok(json!({
        "words": words,
        "transcripts": per,
        "skipped": result.skipped,
        "error": result.error,
        "cancelled": result.cancelled,
        "view": to_json(&view)?,
    }))
}

fn frames(app: &AppHandle, c: &mut Call) -> Result<Value, ControlError> {
    let ms: Vec<u64> = match c.params.get("atProgrammeMs") {
        Some(Value::Array(a)) => a.iter().map(|v| v.as_u64().ok_or_else(|| perr("atProgrammeMs must be integers"))).collect::<Result<_, _>>()?,
        Some(v) if v.is_u64() => vec![v.as_u64().unwrap_or(0)],
        _ => return Err(perr("atProgrammeMs must be a list of programme ms")),
    };
    let width = match c.params.get("maxWidth") {
        None | Some(Value::Null) => DEFAULT_FRAME_WIDTH,
        Some(v) => v.as_u64().and_then(|w| u32::try_from(w).ok()).ok_or_else(|| perr("maxWidth must be an integer"))?,
    }
    .clamp(FRAME_WIDTH_RANGE.0, FRAME_WIDTH_RANGE.1);
    if ms.is_empty() || ms.len() > MAX_FRAMES {
        return Err(perr(format!("atProgrammeMs needs 1..{MAX_FRAMES} times")));
    }
    let id = session_for(app, c)?;
    let jpegs = super::editor_ipc::control_frames(app, id, &ms, width)?;
    let b64 = base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    Ok(Value::Array(
        ms.iter().zip(jpegs).map(|(t, j)| json!({"atProgrammeMs": t, "jpegBase64": b64.encode(j)})).collect(),
    ))
}

fn delete(app: &AppHandle, c: &mut Call) -> Result<Value, ControlError> {
    if c.params.get("confirm") != Some(&Value::Bool(true)) {
        return Err(ControlError::new(ERR_CONFIRM, "deleting a project needs confirm: true"));
    }
    let dir = project_dir(app, c)?;
    super::editor_ipc::control_release(app, &dir).map_err(ControlError::sentinel)?;
    super::session::delete_project(app, &dir).map_err(ControlError::sentinel)?;
    Ok(json!({"deleted": dir}))
}

fn rename(app: &AppHandle, c: &mut Call) -> Result<Value, ControlError> {
    let dir = project_dir(app, c)?;
    let title = param_str(c, "title")?;
    let t = super::session::rename_project(app, &dir, &title).map_err(ControlError::sentinel)?;
    Ok(json!({"title": t}))
}

/// Registers the record methods on the control socket.
pub fn register_all() {
    use super::control_rec as rec;
    use crate::control::register;
    register("record.sources", rec::sources);
    register("record.start", rec::start);
    register("record.stop", rec::stop);
    register("record.pause", rec::pause);
    register("record.resume", rec::resume);
    register("record.status", rec::status);
    register("record.projects", projects);
    register("record.project", project);
    register("record.doc", doc);
    register("record.transcript", transcript);
    register("record.edit", edit);
    register("record.export", export);
    register("record.transcribe", transcribe);
    register("record.frames", frames);
    register("record.delete", delete);
    register("record.rename", rename);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::editor::tests::meta;
    use crate::record::editor::{default_doc, EditClip, Transcript, TranscriptWord};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NO_TAKES: TakeMetas = TakeMetas::new();

    fn no_preset(_: &str, _: &EditDoc) -> Result<EditDoc, ControlError> {
        Err(ControlError::new("record.presets.unreadable", "no presets in tests"))
    }

    /// `/a/voice.m4a` is imported as `voice-1.m4a` (3 s); everything else was not.
    fn test_audio(path: &str) -> Option<(String, u64)> {
        (path == "/a/voice.m4a").then(|| ("voice-1.m4a".to_string(), 3_000))
    }

    #[test]
    fn formats_are_added_switched_and_removed() {
        let d = two_clips();
        let out = run(&d, json!([{"op": "format.add", "aspect": "9:16"}])).unwrap().doc;
        assert_eq!(out.output.aspect, editor::Aspect::W9H16);
        assert_eq!(out.variants.len(), 1);
        let back = run(&out, json!([{"op": "format.switch", "aspect": out.variants[0].aspect}])).unwrap().doc;
        assert_eq!(back.output.aspect, d.output.aspect);
        let one = run(&back, json!([{"op": "format.remove", "aspect": "9:16"}])).unwrap().doc;
        assert!(one.variants.is_empty());
        assert_eq!(run(&d, json!([{"op": "format.switch", "aspect": "1:1"}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&d, json!([{"op": "format.add", "aspect": "auto"}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&d, json!([{"op": "format.add"}])).unwrap_err().code, ERR_BAD_OP);
        // look.set never overwrites a kept format.
        let e = run(&out, json!([{"op": "look.set", "output": {"aspect": out.variants[0].aspect}}])).unwrap_err();
        assert!(e.message.contains("format.switch"), "{}", e.message);
    }

    fn ctx() -> OpCtx<'static> {
        OpCtx { duration_ms: 10_000, takes: &NO_TAKES, cursor: None, preset: &no_preset, audio: &test_audio, has_camera: false }
    }

    fn ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("id{n}")
        }
    }

    /// Two clips: [0, 4000) and [6000, 10000) of the own take.
    fn two_clips() -> EditDoc {
        let mut d = default_doc(&meta(10_000), None);
        d.clips = vec![EditClip::main(0, 4000), EditClip::main(6000, 10_000)];
        d.crop = vec![None, None];
        d
    }

    fn run(doc: &EditDoc, ops: Value) -> Result<Applied, ControlError> {
        apply_ops(doc, ops.as_array().unwrap(), &ctx(), &mut ids())
    }

    // ---- resolve_project ----

    struct Td(PathBuf);
    impl Drop for Td {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn td() -> Td {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!("ir-ctlops-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&p).unwrap();
        Td(std::fs::canonicalize(&p).unwrap())
    }
    fn project_at(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(super::super::project::PROJECT_FILE), b"{}").unwrap();
    }

    #[test]
    fn resolve_project_accepts_names_and_absolute_projects() {
        let t = td();
        let root = t.0.join("root");
        project_at(&root.join("Take 1"));
        project_at(&t.0.join("elsewhere"));
        std::fs::create_dir_all(root.join("plain")).unwrap();
        assert_eq!(resolve_project(&root, "Take 1").unwrap(), root.join("Take 1"));
        assert_eq!(resolve_project(&root, root.join("Take 1").to_str().unwrap()).unwrap(), root.join("Take 1"));
        // A project outside the recordings folder (moved, "Open project…") is fine by path.
        assert_eq!(resolve_project(&root, t.0.join("elsewhere").to_str().unwrap()).unwrap(), t.0.join("elsewhere"));
    }

    #[test]
    fn resolve_project_refuses_escapes_and_non_projects() {
        let t = td();
        let root = t.0.join("root");
        project_at(&root.join("Take 1"));
        project_at(&t.0.join("elsewhere"));
        std::fs::create_dir_all(root.join("plain")).unwrap();
        // A name may not walk out of the recordings folder, even onto a real project.
        for bad in ["../elsewhere", "Take 1/..", "a/b", "..", ".", "", "  "] {
            assert_eq!(resolve_project(&root, bad).unwrap_err().code, ERR_BAD_PROJECT, "{bad:?}");
        }
        assert_eq!(resolve_project(&root, "plain").unwrap_err().code, super::super::editor_ipc::ERR_NOT_PROJECT);
        assert_eq!(resolve_project(&root, "missing").unwrap_err().code, super::super::editor_ipc::ERR_NOT_PROJECT);
        assert_eq!(
            resolve_project(&root, root.join("plain").to_str().unwrap()).unwrap_err().code,
            super::super::editor_ipc::ERR_NOT_PROJECT
        );
    }

    // ---- ops ----

    #[test]
    fn zoom_add_at_programme_ms_lands_in_the_right_clip() {
        let mut d = two_clips();
        // Clip 0 at 2× plays in 2000 programme ms, so programme 3000 is 1000 ms into clip 1.
        d.speed = vec![SpeedRegion { clip: 0, start_ms: 0, end_ms: 4000, speed: 2.0 }];
        let out = run(&d, json!([{"op": "zoom.add", "atProgrammeMs": 3000, "durationMs": 1000}])).unwrap();
        let z = &out.doc.zooms[0];
        assert_eq!((z.clip, z.start_ms, z.end_ms), (1, 7000, 8000));
        assert_eq!(z.origin, ZoomOrigin::Manual);
        assert_eq!(out.created, vec![(0, "id1".to_string())]);
        // In clip 0 at 2×: 1000 programme ms are 2000 source ms.
        let out = run(&d, json!([{"op": "zoom.add", "atProgrammeMs": 500, "durationMs": 1000}])).unwrap();
        let z = &out.doc.zooms[0];
        assert_eq!((z.clip, z.start_ms, z.end_ms), (0, 1000, 3000));
    }

    #[test]
    fn zoom_add_by_clip_and_past_the_end() {
        let d = two_clips();
        let out = run(&d, json!([{"op": "zoom.add", "clip": 1, "startMs": 6500, "depth": 5, "focusX": 0.2}])).unwrap();
        let z = &out.doc.zooms[0];
        assert_eq!((z.clip, z.start_ms, z.end_ms, z.depth), (1, 6500, 8500, 5));
        assert_eq!(z.focus_mode, FocusMode::Manual);
        let e = run(&d, json!([{"op": "zoom.add", "atProgrammeMs": 999_999}])).unwrap_err();
        assert_eq!(e.code, ERR_OUT_OF_RANGE);
        let e = run(&d, json!([{"op": "zoom.add", "clip": 1, "startMs": 100}])).unwrap_err();
        assert_eq!(e.code, ERR_OUT_OF_RANGE);
        // 100 ms before the clip's end is under the 250 ms minimum: normalize drops it.
        let e = run(&d, json!([{"op": "zoom.add", "clip": 1, "startMs": 9900}])).unwrap_err();
        assert_eq!(e.code, ERR_DROPPED);
    }

    #[test]
    fn unknown_region_and_clip_errors() {
        let d = two_clips();
        assert_eq!(run(&d, json!([{"op": "zoom.set", "id": "nope", "patch": {"depth": 2}}])).unwrap_err().code, ERR_NO_REGION);
        assert_eq!(run(&d, json!([{"op": "annotation.remove", "id": "nope"}])).unwrap_err().code, ERR_NO_REGION);
        assert_eq!(run(&d, json!([{"op": "clip.delete", "clip": 7}])).unwrap_err().code, ERR_BAD_CLIP);
        assert_eq!(run(&d, json!([{"op": "clip.split", "clip": 2, "sourceMs": 10}])).unwrap_err().code, ERR_BAD_CLIP);
        assert_eq!(run(&d, json!([{"op": "zoom.add", "clip": 9}])).unwrap_err().code, ERR_BAD_CLIP);
    }

    #[test]
    fn ops_are_all_or_nothing() {
        let d = two_clips();
        let before = d.clone();
        let e = run(&d, json!([
            {"op": "zoom.add", "clip": 0},
            {"op": "clip.delete", "clip": 0},
            {"op": "clip.delete", "clip": 5},
        ]))
        .unwrap_err();
        assert_eq!(e.code, ERR_BAD_CLIP);
        assert!(e.message.starts_with("op 2 (clip.delete)"), "{}", e.message);
        assert_eq!(d, before);
    }

    #[test]
    fn later_ops_see_the_earlier_ones() {
        let d = two_clips();
        // After deleting clip 0, the old clip 1 is clip 0.
        let out = run(&d, json!([{"op": "clip.delete", "clip": 0}, {"op": "zoom.add", "clip": 0, "startMs": 6000}])).unwrap();
        assert_eq!(out.doc.clips, vec![EditClip::main(6000, 10_000)]);
        assert_eq!((out.doc.zooms[0].clip, out.doc.zooms[0].start_ms), (0, 6000));
        assert_eq!(out.created, vec![(1, "id1".to_string())]);
    }

    #[test]
    fn bad_ops_are_refused() {
        let d = two_clips();
        assert_eq!(run(&d, json!([{"op": "zoom.explode"}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&d, json!([{"op": "clip.delete", "clip": 0, "extra": 1}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&d, json!([{"op": "clip.delete", "clip": -1}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&d, json!([{"op": "clip.delete", "clip": "0"}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&d, json!([{"nope": 1}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&d, json!([5])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&d, json!([])).unwrap_err().code, ERR_BAD_PARAMS);
        assert_eq!(run(&d, json!([{"op": "clip.speed", "clip": 0, "speed": 99}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&d, json!([{"op": "zoom.add", "atProgrammeMs": 10, "clip": 0}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&d, json!([{"op": "effect.add", "clip": 0, "effect": "nope"}])).unwrap_err().code, ERR_UNKNOWN_EFFECT);
        assert_eq!(run(&d, json!([{"op": "zoom.auto"}])).unwrap_err().code, ERR_NO_CURSOR);
        assert_eq!(run(&d, json!([{"op": "preset.apply", "file": "x"}])).unwrap_err().code, "record.presets.unreadable");
    }

    #[test]
    fn region_set_merges_and_remove_works() {
        let d = two_clips();
        let out = run(&d, json!([
            {"op": "zoom.add", "clip": 0, "id": "z"},
            {"op": "annotation.add", "clip": 1, "startMs": 6000, "id": "a", "body": {"kind": "text", "content": "Hi"}, "x": 10},
            {"op": "effect.add", "clip": 0, "effect": "vignette", "id": "e"},
        ]))
        .unwrap();
        let mut d = out.doc;
        d.zooms[0].origin = ZoomOrigin::Auto;
        let out = run(&d, json!([
            {"op": "zoom.set", "id": "z", "patch": {"depth": 6}},
            {"op": "annotation.set", "id": "a", "patch": {"y": 20, "body": {"content": "Hello"}}},
        ]))
        .unwrap();
        let z = &out.doc.zooms[0];
        assert_eq!((z.depth, z.start_ms, z.end_ms, z.origin), (6, 0, 2000, ZoomOrigin::Manual));
        let a = &out.doc.annotations[0];
        assert_eq!((a.x, a.y), (10.0, 20.0));
        match &a.body {
            AnnotationBody::Text(t) => assert_eq!(t.content, "Hello"),
            other => panic!("{other:?}"),
        }
        assert_eq!(run(&out.doc, json!([{"op": "zoom.set", "id": "z", "patch": {"id": "q"}}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&out.doc, json!([{"op": "zoom.add", "clip": 0, "id": "z"}])).unwrap_err().code, ERR_ID_TAKEN);
        let out = run(&out.doc, json!([{"op": "zoom.remove", "id": "z"}, {"op": "effect.remove", "id": "e"}])).unwrap();
        assert!(out.doc.zooms.is_empty() && out.doc.effects.is_empty());
        assert_eq!(out.doc.annotations.len(), 1);
    }

    #[test]
    fn region_set_can_move_by_programme_ms() {
        let d = two_clips();
        let out = run(&d, json!([
            {"op": "zoom.add", "clip": 0, "id": "z", "durationMs": 1000},
            {"op": "zoom.set", "id": "z", "atProgrammeMs": 5000},
        ]))
        .unwrap();
        let z = &out.doc.zooms[0];
        assert_eq!((z.clip, z.start_ms, z.end_ms), (1, 7000, 8000));
    }

    #[test]
    fn split_keeps_spanning_regions_in_both_halves() {
        let mut d = two_clips();
        d.zooms.push(ZoomRegion {
            id: "z".into(),
            clip: 1,
            start_ms: 7000,
            end_ms: 9000,
            depth: 3,
            custom_scale: None,
            focus_x: 0.5,
            focus_y: 0.5,
            focus_mode: FocusMode::Auto,
            origin: ZoomOrigin::Manual,
            hide_cursor: false,
            rotation: None,
        });
        let out = run(&d, json!([{"op": "clip.split", "atProgrammeMs": 6000}])).unwrap();
        assert_eq!(out.doc.clips, vec![EditClip::main(0, 4000), EditClip::main(6000, 8000), EditClip::main(8000, 10_000)]);
        let zs: Vec<_> = out.doc.zooms.iter().map(|z| (z.id.as_str(), z.clip, z.start_ms, z.end_ms)).collect();
        assert_eq!(zs, vec![("z", 1, 7000, 8000), ("id1", 2, 8000, 9000)]);
        assert_eq!(out.doc.crop.len(), 3);
        assert_eq!(run(&d, json!([{"op": "clip.split", "clip": 0, "sourceMs": 50}])).unwrap_err().code, ERR_BAD_SPLIT);
    }

    #[test]
    fn track_add_lays_an_imported_file_under_the_programme() {
        let d = two_clips();
        // Programme 5000 ms = clip 1, source 7000 ms.
        let out = run(&d, json!([{"op": "track.add", "file": "/a/voice.m4a", "atProgrammeMs": 5000, "id": "q"}])).unwrap();
        let t = &out.doc.audio_tracks[0];
        assert_eq!((t.id.as_str(), t.file.as_str(), t.clip, t.start_ms), ("q", "voice-1.m4a", 1, 7000));
        assert_eq!((t.kind, t.duration_ms, t.span_ms, t.gain_db), (TrackKind::Voiceover, 3_000, Some(3_000), 0.0));
        assert_eq!(out.created, vec![(0, "q".to_string())]);
        // Music: the editor's defaults, then explicit values win.
        let out = run(&d, json!([{"op": "track.add", "file": "/a/voice.m4a", "kind": "music", "clip": 0}])).unwrap();
        let m = &out.doc.audio_tracks[0];
        assert_eq!((m.kind, m.start_ms, m.span_ms), (TrackKind::Music, 0, None));
        assert_eq!((m.gain_db, m.fade_in_ms, m.fade_out_ms), (MUSIC_DEFAULT_GAIN_DB, MUSIC_DEFAULT_FADE_MS, MUSIC_DEFAULT_FADE_MS));
        let out = run(&d, json!([{"op": "track.add", "file": "/a/voice.m4a", "clip": 1, "startMs": 6500, "gainDb": -6, "label": "Frage"}])).unwrap();
        let v = &out.doc.audio_tracks[0];
        assert_eq!((v.start_ms, v.gain_db, v.label.as_str()), (6500, -6.0, "Frage"));
        // Not imported, bad kind, outside the clip.
        assert_eq!(run(&d, json!([{"op": "track.add", "file": "/b/x.wav", "clip": 0}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&d, json!([{"op": "track.add", "file": "/a/voice.m4a", "kind": "noise", "clip": 0}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(
            run(&d, json!([{"op": "track.add", "file": "/a/voice.m4a", "clip": 0, "startMs": 5000}])).unwrap_err().code,
            ERR_OUT_OF_RANGE
        );
    }

    #[test]
    fn track_set_and_remove() {
        let d = two_clips();
        let out = run(&d, json!([
            {"op": "track.add", "file": "/a/voice.m4a", "clip": 0, "id": "q"},
            {"op": "track.set", "id": "q", "gainDb": 4, "muted": true, "fadeInMs": 200, "label": "x"}
        ]))
        .unwrap();
        let t = &out.doc.audio_tracks[0];
        assert_eq!((t.gain_db, t.muted, t.fade_in_ms, t.label.as_str()), (4.0, true, 200, "x"));
        let gone = run(&out.doc, json!([{"op": "track.remove", "id": "q"}])).unwrap();
        assert!(gone.doc.audio_tracks.is_empty());
        assert_eq!(run(&d, json!([{"op": "track.remove", "id": "q"}])).unwrap_err().code, ERR_NO_REGION);
        assert_eq!(run(&d, json!([{"op": "track.set", "id": "q", "gainDb": 1}])).unwrap_err().code, ERR_NO_REGION);
    }

    #[test]
    fn the_projects_list_knows_the_take_being_recorded() {
        let src = include_str!("control_ops.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap()];
        let f = &code[code.find("fn projects(").unwrap()..];
        let f = &f[..f.find("fn project(").unwrap()];
        assert!(f.contains("list_items_with(&root(app), active.as_deref(), None, false)"), "not bare list_projects, no thumbs");
    }

    #[test]
    fn track_files_are_the_op_files_once() {
        let ops = vec![
            json!({"op": "track.add", "file": "/a/voice.m4a"}),
            json!({"op": "zoom.add", "clip": 0}),
            json!({"op": "track.add", "file": "/a/voice.m4a"}),
            json!({"op": "track.add", "file": "/b/song.mp3"}),
            json!({"op": "track.add"}),
        ];
        assert_eq!(track_files(&ops), vec!["/a/voice.m4a".to_string(), "/b/song.mp3".to_string()]);
    }

    #[test]
    fn clip_ops() {
        let d = two_clips();
        let one = run(&d, json!([{"op": "clip.delete", "clip": 1}])).unwrap().doc;
        assert_eq!(run(&one, json!([{"op": "clip.delete", "clip": 0}])).unwrap_err().code, ERR_LAST_CLIP);
        let out = run(&d, json!([{"op": "clip.move", "clip": 1, "to": 0}])).unwrap().doc;
        assert_eq!(out.clips[0], EditClip::main(6000, 10_000));
        let out = run(&d, json!([{"op": "clip.duplicate", "clip": 0}])).unwrap().doc;
        assert_eq!(out.clips.len(), 3);
        let out = run(&d, json!([{"op": "clip.speed", "clip": 1, "speed": 2}])).unwrap().doc;
        assert_eq!(out.speed, vec![SpeedRegion { clip: 1, start_ms: 6000, end_ms: 10_000, speed: 2.0 }]);
        let out = run(&d, json!([{"op": "clip.crop", "clip": 0, "crop": {"x": 0.1, "y": 0.1, "w": 0.5, "h": 0.5}}])).unwrap().doc;
        assert_eq!(out.crop[0], Some(CropRect { x: 0.1, y: 0.1, w: 0.5, h: 0.5 }));
        let out = run(&out, json!([{"op": "clip.crop", "clip": 0, "crop": null}])).unwrap().doc;
        assert_eq!(out.crop[0], None);
    }

    #[test]
    fn look_set_merges_partially() {
        let d = two_clips();
        let out = run(&d, json!([{"op": "look.set", "appearance": {"padding": 10}, "captions": {"enabled": true}}])).unwrap().doc;
        assert_eq!(out.appearance.padding, 10.0);
        assert_eq!(out.appearance.radius_px, d.appearance.radius_px);
        assert_eq!(out.appearance.background, d.appearance.background);
        assert!(out.captions.enabled);
        assert_eq!(out.captions.font_size, d.captions.font_size);
        assert_eq!(out.cursor, d.cursor);
        assert_eq!(out.clips, d.clips);
        assert_eq!(run(&d, json!([{"op": "look.set"}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&d, json!([{"op": "look.set", "clips": []}])).unwrap_err().code, ERR_BAD_OP);
        assert_eq!(run(&d, json!([{"op": "look.set", "appearance": {"padding": "x"}}])).unwrap_err().code, ERR_BAD_OP);
    }

    #[test]
    fn look_set_refuses_unknown_nested_keys() {
        let d = two_clips();
        let e = run(&d, json!([{"op": "look.set", "appearance": {"paddin": 10}}])).unwrap_err();
        assert_eq!(e.code, ERR_BAD_OP);
        assert!(e.message.contains("appearance.paddin"), "{}", e.message);
        let e = run(&d, json!([{"op": "look.set", "captions": {"enabled": true, "colour": "#fff"}}])).unwrap_err();
        assert!(e.message.contains("captions.colour"), "{}", e.message);
        // A different background variant brings its own fields; an unset optional is allowed.
        let out = run(&d, json!([{"op": "look.set", "appearance": {"background": {"kind": "color", "color": "#112233"}}}])).unwrap().doc;
        assert_eq!(out.appearance.background, editor::Background::Color { color: "#112233".into() });
        run(&d, json!([{"op": "look.set", "camera": {"position": {"x": 0.3, "y": 0.4}}}])).unwrap();
    }

    #[test]
    fn last_rename_takes_the_last_title() {
        let ops = json!([{"op": "rename", "title": "a"}, {"op": "zoom.auto"}, {"op": "rename", "title": "b"}]);
        assert_eq!(last_rename(ops.as_array().unwrap()).as_deref(), Some("b"));
        assert_eq!(last_rename(json!([{"op": "zoom.auto"}]).as_array().unwrap()), None);
    }

    #[test]
    fn rename_cleans_the_title() {
        let d = two_clips();
        let out = run(&d, json!([{"op": "rename", "title": "  Demo  "}])).unwrap();
        assert_eq!(out.title.as_deref(), Some("Demo"));
        assert_eq!(out.doc, editor::normalize(d.clone(), 10_000, &NO_TAKES));
        assert!(run(&d, json!([{"op": "rename", "title": "   "}])).is_err());
    }

    #[test]
    fn merge_transcripts_replaces_by_source_and_turns_captions_on_once() {
        let word = |t: &str| TranscriptWord { id: "w0".into(), start_ms: 0, end_ms: 100, text: t.into(), original_text: None };
        let t = |src: Option<&str>, w: &str| Transcript { source: src.map(str::to_string), lang: "de".into(), words: vec![word(w)] };
        let d = two_clips();
        let out = merge_transcripts(&d, &[t(None, "a")], Some(transcribe_protocol::Lang::De));
        assert!(out.captions.enabled);
        assert_eq!(out.captions.lang, transcribe_protocol::Lang::De);
        let mut off = out.clone();
        off.captions.enabled = false;
        let out = merge_transcripts(&off, &[t(None, "b"), t(Some("k"), "c")], None);
        assert_eq!(out.transcripts.len(), 2);
        assert_eq!(out.transcripts[0].words[0].text, "b");
        assert!(!out.captions.enabled, "a later run never turns captions back on");
    }

    #[test]
    fn frame_indices_and_limits() {
        assert_eq!(frame_indices(&[0, 1000, 2000], 2000, 30).unwrap(), vec![0, 30, 59]);
        assert_eq!(frame_indices(&[2001], 2000, 30).unwrap_err().code, ERR_OUT_OF_RANGE);
        assert_eq!(frame_indices(&[], 2000, 30).unwrap_err().code, ERR_BAD_PARAMS);
        assert_eq!(frame_indices(&[0; 9], 2000, 30).unwrap_err().code, ERR_BAD_PARAMS);
    }

    #[test]
    fn export_out_rules() {
        let t = td();
        assert_eq!(export_out("rel.mp4", "mp4").unwrap_err().code, ERR_BAD_OUT);
        assert_eq!(export_out(t.0.join("a.gif").to_str().unwrap(), "mp4").unwrap_err().code, ERR_BAD_OUT);
        assert_eq!(export_out(t.0.join("no/such/a.mp4").to_str().unwrap(), "mp4").unwrap_err().code, ERR_BAD_OUT);
        assert_eq!(export_out(t.0.join("a").to_str().unwrap(), "mp4").unwrap(), t.0.join("a.mp4"));
        assert_eq!(export_out(t.0.join("b.MP4").to_str().unwrap(), "mp4").unwrap(), t.0.join("b.MP4"));
        assert_eq!(export_out(t.0.join("demo.v2").to_str().unwrap(), "mp4").unwrap(), t.0.join("demo.v2.mp4"));
        assert_eq!(export_out(t.0.join("2026.11.04 take").to_str().unwrap(), "gif").unwrap(), t.0.join("2026.11.04 take.gif"));
        assert_eq!(export_out(t.0.join("x.MOV").to_str().unwrap(), "mp4").unwrap_err().code, ERR_BAD_OUT);
        std::fs::write(t.0.join("c.mp4"), b"x").unwrap();
        assert_eq!(export_out(t.0.join("c.mp4").to_str().unwrap(), "mp4").unwrap_err().code, ERR_OUT_EXISTS);
    }

    #[test]
    fn session_choice_rules() {
        use super::super::editor_ipc::{session_choice, SessionChoice};
        let (a, b) = (Path::new("/r/a"), Path::new("/r/b"));
        assert_eq!(session_choice(None, a, false), SessionChoice::Open);
        assert_eq!(session_choice(None, a, true), SessionChoice::Open);
        assert_eq!(session_choice(Some(a), a, true), SessionChoice::Reuse);
        assert_eq!(session_choice(Some(a), a, false), SessionChoice::Reuse);
        assert_eq!(session_choice(Some(b), a, true), SessionChoice::Refuse);
        assert_eq!(session_choice(Some(b), a, false), SessionChoice::Open);
    }

    #[test]
    fn a_minimised_editor_window_still_owns_its_session() {
        use super::super::editor_ipc::{idle_close_target, session_choice, SessionChoice};
        // Existence, not visibility: a minimised window exists, so B is refused under it.
        assert_eq!(session_choice(Some(Path::new("/r/a")), Path::new("/r/b"), true), SessionChoice::Refuse);
        let src = include_str!("editor_ipc.rs");
        let f = src.split("pub fn editor_window_exists(").nth(1).unwrap().split("\n}\n").next().unwrap();
        assert!(f.contains(".is_some()") && !f.contains("is_visible"), "{f}");
        // The idle watcher closes only the control's own window-less session.
        assert_eq!(idle_close_target(7, Some(7), false), Some(7));
        assert_eq!(idle_close_target(7, Some(7), true), None, "a window (even minimised) owns it");
        assert_eq!(idle_close_target(7, Some(8), false), None, "editor_open installed another one");
        assert_eq!(idle_close_target(0, Some(8), false), None, "no control session");
        assert_eq!(idle_close_target(7, None, false), None);
    }

    #[test]
    fn methods_list_matches_the_registry() {
        let src = include_str!("control_ops.rs");
        let body = &src[src.find("pub fn register_all").unwrap()..];
        for m in super::super::CONTROL_METHODS {
            assert!(body.contains(&format!("register(\"{m}\"")), "{m} not registered");
        }
        assert_eq!(body.matches("register(\"record.").count(), super::super::CONTROL_METHODS.len());
    }
}
