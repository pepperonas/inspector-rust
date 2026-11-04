//! The MCP tool table of `ir-record mcp`: name, description, input schema,
//! annotations, and how a call maps onto a control method.
//!
//! Schemas are hand-written JSON (no schema crate) and mirror the control
//! params of `core/rust-lib/src/record/control_ops.rs` / `control_rec.rs`
//! (camelCase, same names), so the mapping is the identity except for
//! `record_auto_zoom`. [`validate`] checks arguments against the subset of
//! JSON Schema these schemas use, so a wrong type becomes a tool error
//! before anything reaches the app.

use serde_json::{json, Map, Value};
use std::time::Duration;

/// Deadline for short read methods (the app answers them from memory or one
/// session open). Long methods (export, transcribe, stop, start) have none.
pub const READ_DEADLINE: Duration = Duration::from_secs(60);
/// Frames start the render helper on first use.
pub const FRAMES_DEADLINE: Duration = Duration::from_secs(180);

/// Op kinds of `record.edit` (Task 2's table).
pub const EDIT_OPS: [&str; 19] = [
    "zoom.add",
    "zoom.set",
    "zoom.remove",
    "zoom.auto",
    "clip.split",
    "clip.delete",
    "clip.move",
    "clip.duplicate",
    "clip.speed",
    "clip.crop",
    "annotation.add",
    "annotation.set",
    "annotation.remove",
    "effect.add",
    "effect.set",
    "effect.remove",
    "look.set",
    "preset.apply",
    "rename",
];

/// How a result is shaped into MCP content.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    /// Pretty JSON text + `structuredContent`; a non-object result is wrapped
    /// under this key (structuredContent must be an object).
    Json(&'static str),
    /// `record.frames`: a text summary + one `image` item per frame.
    Frames,
}

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub method: &'static str,
    /// May start the app when it is not running (stop/pause/resume/status: no —
    /// there is nothing to stop in an app that isn't running).
    pub launch: bool,
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub deadline: Option<Duration>,
    pub shape: Shape,
    pub schema: fn() -> Value,
}

fn project() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "description": "Project folder name as listed by record_projects (`folder`), or an absolute path to a project folder."
    })
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

fn none() -> Value {
    obj(json!({}), &[])
}

fn project_only() -> Value {
    obj(json!({"project": project()}), &["project"])
}

fn frames_schema() -> Value {
    obj(
        json!({
            "project": project(),
            "atProgrammeMs": {
                "type": ["number", "array"],
                "minimum": 0,
                "items": {"type": "number", "minimum": 0},
                "minItems": 1,
                "maxItems": 8,
                "description": "Programme time(s) in ms, one number or 1–8 numbers."
            },
            "maxWidth": {"type": "integer", "minimum": 64, "maximum": 1920, "description": "Largest image width in px (default 640)."}
        }),
        &["project", "atProgrammeMs"],
    )
}

fn edit_schema() -> Value {
    obj(
        json!({
            "project": project(),
            "ops": {
                "type": "array",
                "minItems": 1,
                "maxItems": 500,
                "description": "Edit ops, applied in order, all or nothing.",
                "items": {
                    "type": "object",
                    "required": ["op"],
                    "properties": {"op": {"type": "string", "enum": EDIT_OPS}}
                }
            }
        }),
        &["project", "ops"],
    )
}

fn transcribe_schema() -> Value {
    obj(
        json!({
            "project": project(),
            "lang": {"type": "string", "enum": ["auto", "de", "en"], "description": "Spoken language (default auto)."}
        }),
        &["project"],
    )
}

fn export_schema() -> Value {
    obj(
        json!({
            "project": project(),
            "format": {"type": "string", "enum": ["mp4", "gif"], "description": "Default mp4."},
            "quality": {"type": "string", "enum": ["p720", "p1080", "source"], "description": "Default p1080."},
            "fps": {"type": "integer", "minimum": 1, "maximum": 60, "description": "mp4: 24, 30 or 60 (30). gif: 15–30 (15)."},
            "codec": {"type": "string", "enum": ["h264", "hevc"], "description": "mp4 only."},
            "gifWidth": {"type": "integer", "enum": [480, 640, 854, 1080], "description": "gif only (640)."},
            "out": {"type": "string", "minLength": 1, "description": "Absolute output path (an existing folder + file name); never overwrites. Default: ~/Downloads."}
        }),
        &["project"],
    )
}

fn rename_schema() -> Value {
    obj(json!({"project": project(), "title": {"type": "string", "minLength": 1}}), &["project", "title"])
}

fn delete_schema() -> Value {
    obj(
        json!({
            "project": project(),
            "confirm": {"type": "boolean", "const": true, "description": "Must be true: the project folder is deleted."}
        }),
        &["project", "confirm"],
    )
}

fn start_schema() -> Value {
    let id = json!({"type": ["integer", "string"]});
    obj(
        json!({
            "display": {"type": ["integer", "string"], "description": "Display id or name (from record_sources)."},
            "window": {"type": ["integer", "string"], "description": "Window id or a name (`App — Title`) substring."},
            "area": {
                "type": "object",
                "description": "Area in display-local points (≥ 64 pt per side).",
                "properties": {
                    "x": {"type": "number"}, "y": {"type": "number"},
                    "w": {"type": "number", "minimum": 64}, "h": {"type": "number", "minimum": 64},
                    "display": id
                },
                "required": ["x", "y", "w", "h"],
                "additionalProperties": false
            },
            "mic": {"type": "boolean"},
            "systemAudio": {"type": "boolean"},
            "camera": {"type": ["boolean", "string"], "description": "true = the configured camera, or a camera id/name."},
            "countdown": {"type": "boolean", "description": "Show the 3-s countdown first (default false)."},
            "title": {"type": "string"}
        }),
        &[],
    )
}

fn stop_schema() -> Value {
    obj(json!({"open": {"type": "boolean", "description": "Open the editor afterwards like the control bar does (default false)."}}), &[])
}

const EDIT_DESC: &str = "Apply edit ops to a project's edit document, in order, all or nothing (one undo step in an open editor window). \
Call record_doc first: region ids (zooms, annotations, effects) and clip indices come from it. \
Placement for add ops (and *.set to move): `atProgrammeMs` (+ `durationMs`, programme ms) OR `clip` (+ `startMs`, `durationMs`, SOURCE ms of that clip); default duration 2000. \
Ops: zoom.add {placement, depth 1–6, focusX/focusY 0–1, hideCursor, rotation left|right|orbit, id?}; zoom.set {id, patch?, placement?}; zoom.remove {id}; zoom.auto {}; \
clip.split {atProgrammeMs | clip+sourceMs}; clip.delete {clip}; clip.move {clip, to}; clip.duplicate {clip}; clip.speed {speed 0.25–16, clip (+startMs,endMs source ms) | atProgrammeMs+durationMs}; clip.crop {clip, crop {x,y,w,h} fractions | null}; \
annotation.add {placement, body {kind: text|image|arrow|blur, …}, x,y,w,h percent, id?}; annotation.set / annotation.remove {id}; \
effect.add {placement, effect (built-in id e.g. vignette, grain, spotlight), values?}; effect.set / effect.remove {id}; \
look.set {appearance?, cursor?, output?, camera?, captions?, micGainDb?, systemGainDb?} (partial, deep-merged); preset.apply {file}; rename {title}. \
Returns the new view and the ids of created regions.";

pub static TOOLS: &[Tool] = &[
    Tool {
        name: "record_projects",
        description: "List all recordings: folder, title, dates, duration, status, and whether an editor window shows it.",
        method: "record.projects",
        launch: true,
        read_only: true,
        destructive: false,
        idempotent: true,
        deadline: Some(READ_DEADLINE),
        shape: Shape::Json("projects"),
        schema: none,
    },
    Tool {
        name: "record_project",
        description: "One project: metadata, recording info and the timeline view (clips, duration in programme ms).",
        method: "record.project",
        launch: true,
        read_only: true,
        destructive: false,
        idempotent: true,
        deadline: Some(READ_DEADLINE),
        shape: Shape::Json("project"),
        schema: project_only,
    },
    Tool {
        name: "record_doc",
        description: "The edit document and view of a project. Read this before record_edit: region ids and clip indices come from it.",
        method: "record.doc",
        launch: true,
        read_only: true,
        destructive: false,
        idempotent: true,
        deadline: Some(READ_DEADLINE),
        shape: Shape::Json("doc"),
        schema: project_only,
    },
    Tool {
        name: "record_transcript",
        description: "The project's transcripts: words with start/end in SOURCE ms per take (run record_transcribe first).",
        method: "record.transcript",
        launch: true,
        read_only: true,
        destructive: false,
        idempotent: true,
        deadline: Some(READ_DEADLINE),
        shape: Shape::Json("transcripts"),
        schema: project_only,
    },
    Tool {
        name: "record_sources",
        description: "What can be recorded: displays, windows, microphones and cameras (ids and names for record_start).",
        method: "record.sources",
        launch: true,
        read_only: true,
        destructive: false,
        idempotent: true,
        deadline: Some(READ_DEADLINE),
        shape: Shape::Json("sources"),
        schema: none,
    },
    Tool {
        name: "record_frames",
        description: "Render up to 8 frames of the edited video at programme times (ms) and return them as JPEG images.",
        method: "record.frames",
        launch: true,
        read_only: true,
        destructive: false,
        idempotent: true,
        deadline: Some(FRAMES_DEADLINE),
        shape: Shape::Frames,
        schema: frames_schema,
    },
    Tool {
        name: "record_edit",
        description: EDIT_DESC,
        method: "record.edit",
        launch: true,
        read_only: false,
        destructive: true,
        idempotent: false,
        deadline: None,
        shape: Shape::Json("edit"),
        schema: edit_schema,
    },
    Tool {
        name: "record_auto_zoom",
        description: "Add zooms planned from the pointer clicks of the recording (same as the editor's auto-zoom).",
        method: "record.edit",
        launch: true,
        read_only: false,
        destructive: false,
        idempotent: false,
        deadline: None,
        shape: Shape::Json("edit"),
        schema: project_only,
    },
    Tool {
        name: "record_transcribe",
        description: "Transcribe the project's audio on this Mac (Whisper) and merge the words into the document. Can take minutes.",
        method: "record.transcribe",
        launch: true,
        read_only: false,
        destructive: false,
        idempotent: false,
        deadline: None,
        shape: Shape::Json("transcribe"),
        schema: transcribe_schema,
    },
    Tool {
        name: "record_export",
        description: "Export the edited video as MP4 or GIF (default ~/Downloads, never overwrites). One export at a time; can take minutes; cancelled when the call is cancelled.",
        method: "record.export",
        launch: true,
        read_only: false,
        destructive: false,
        idempotent: false,
        deadline: None,
        shape: Shape::Json("export"),
        schema: export_schema,
    },
    Tool {
        name: "record_rename",
        description: "Set a project's title (the folder name stays).",
        method: "record.rename",
        launch: true,
        read_only: false,
        destructive: false,
        idempotent: true,
        deadline: Some(READ_DEADLINE),
        shape: Shape::Json("rename"),
        schema: rename_schema,
    },
    Tool {
        name: "record_delete",
        description: "Delete a project folder for good. Requires `confirm: true`. Refused while the project is open in an editor window.",
        method: "record.delete",
        launch: true,
        read_only: false,
        destructive: true,
        idempotent: true,
        deadline: Some(READ_DEADLINE),
        shape: Shape::Json("delete"),
        schema: delete_schema,
    },
    Tool {
        name: "record_start",
        description: "Start a screen recording in the app (at most one of display / window / area; none = main display). No countdown unless `countdown: true`. Stop it with record_stop.",
        method: "record.start",
        launch: true,
        read_only: false,
        destructive: false,
        idempotent: false,
        deadline: None,
        shape: Shape::Json("start"),
        schema: start_schema,
    },
    Tool {
        name: "record_stop",
        description: "Stop the running recording and wait until its files are written; returns the project folder.",
        method: "record.stop",
        launch: false,
        read_only: false,
        destructive: false,
        idempotent: false,
        deadline: None,
        shape: Shape::Json("stop"),
        schema: stop_schema,
    },
    Tool {
        name: "record_pause",
        description: "Pause the running recording.",
        method: "record.pause",
        launch: false,
        read_only: false,
        destructive: false,
        idempotent: true,
        deadline: Some(READ_DEADLINE),
        shape: Shape::Json("status"),
        schema: none,
    },
    Tool {
        name: "record_resume",
        description: "Resume a paused recording.",
        method: "record.resume",
        launch: false,
        read_only: false,
        destructive: false,
        idempotent: true,
        deadline: Some(READ_DEADLINE),
        shape: Shape::Json("status"),
        schema: none,
    },
    Tool {
        name: "record_status",
        description: "Recorder state: phase (idle, countdown, recording, paused, finishing), elapsed ms, project folder, warnings.",
        method: "record.status",
        launch: false,
        read_only: true,
        destructive: false,
        idempotent: true,
        deadline: Some(READ_DEADLINE),
        shape: Shape::Json("status"),
        schema: none,
    },
];

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

impl Tool {
    /// The `tools/list` entry. `annotations` only for protocol versions that
    /// know them (2025-03-26 and later).
    pub fn listing(&self, with_annotations: bool) -> Value {
        let mut v = json!({"name": self.name, "description": self.description, "inputSchema": (self.schema)()});
        if with_annotations {
            v["annotations"] = json!({
                "readOnlyHint": self.read_only,
                "destructiveHint": self.destructive,
                "idempotentHint": self.idempotent,
                "openWorldHint": false
            });
        }
        v
    }

    /// Control params for validated `args`.
    pub fn params(&self, args: &Map<String, Value>) -> Value {
        if self.name == "record_auto_zoom" {
            return json!({"project": args.get("project").cloned().unwrap_or(Value::Null), "ops": [{"op": "zoom.auto"}]});
        }
        Value::Object(args.clone())
    }
}

// ---------------------------------------------------------------------------
// Validation (the JSON Schema subset used above)

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn is_integer(v: &Value) -> bool {
    match v {
        Value::Number(n) => n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|f| f.is_finite() && f.fract() == 0.0),
        _ => false,
    }
}

fn type_ok(v: &Value, t: &str) -> bool {
    match t {
        "integer" => is_integer(v),
        other => type_name(v) == other,
    }
}

/// Check `v` against `schema`; the first problem as `<path>: <what>`.
pub fn validate(v: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if let Some(t) = schema.get("type") {
        let types: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if !types.is_empty() && !types.iter().any(|t| type_ok(v, t)) {
            return Err(format!("{path}: expected {}, got {}", types.join(" or "), type_name(v)));
        }
    }
    if let Some(c) = schema.get("const") {
        if v != c {
            return Err(format!("{path}: must be {c}"));
        }
    }
    if let Some(Value::Array(e)) = schema.get("enum") {
        if !e.iter().any(|x| x == v || (x.is_number() && v.is_number() && x.as_f64() == v.as_f64())) {
            let list: Vec<String> = e.iter().map(|x| x.to_string()).collect();
            return Err(format!("{path}: must be one of {}", list.join(", ")));
        }
    }
    if let Some(n) = v.as_f64() {
        if let Some(min) = schema.get("minimum").and_then(Value::as_f64) {
            if n < min {
                return Err(format!("{path}: must be ≥ {min}"));
            }
        }
        if let Some(max) = schema.get("maximum").and_then(Value::as_f64) {
            if n > max {
                return Err(format!("{path}: must be ≤ {max}"));
            }
        }
    }
    if let Value::String(s) = v {
        if let Some(min) = schema.get("minLength").and_then(Value::as_u64) {
            if (s.trim().chars().count() as u64) < min {
                return Err(format!("{path}: must not be empty"));
            }
        }
    }
    if let Value::Array(items) = v {
        if let Some(min) = schema.get("minItems").and_then(Value::as_u64) {
            if (items.len() as u64) < min {
                return Err(format!("{path}: needs at least {min} item(s)"));
            }
        }
        if let Some(max) = schema.get("maxItems").and_then(Value::as_u64) {
            if (items.len() as u64) > max {
                return Err(format!("{path}: at most {max} item(s)"));
            }
        }
        if let Some(is) = schema.get("items") {
            for (i, it) in items.iter().enumerate() {
                validate(it, is, &format!("{path}[{i}]"))?;
            }
        }
    }
    if let Value::Object(m) = v {
        if let Some(Value::Array(req)) = schema.get("required") {
            for r in req.iter().filter_map(Value::as_str) {
                if !m.contains_key(r) {
                    return Err(format!("{path}: missing `{r}`"));
                }
            }
        }
        let props = schema.get("properties").and_then(Value::as_object);
        for (k, val) in m {
            match props.and_then(|p| p.get(k)) {
                Some(s) => validate(val, s, &format!("{path}.{k}"))?,
                None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                    return Err(format!("{path}: unknown field `{k}`"));
                }
                None => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLOWED: [&str; 12] = [
        "type", "properties", "required", "additionalProperties", "items", "enum", "const",
        "minimum", "maximum", "minLength", "minItems", "maxItems",
    ];

    /// Every keyword used in a schema is one the validator understands (or a
    /// pure annotation): a new keyword would otherwise be silently unchecked.
    fn keywords_known(s: &Value, path: &str) {
        let m = s.as_object().unwrap_or_else(|| panic!("{path}: schema is not an object"));
        for (k, v) in m {
            assert!(ALLOWED.contains(&k.as_str()) || k == "description", "{path}: unknown keyword {k}");
            match k.as_str() {
                "properties" => {
                    for (p, ps) in v.as_object().unwrap() {
                        keywords_known(ps, &format!("{path}.{p}"));
                    }
                }
                "items" => keywords_known(v, &format!("{path}[]")),
                "required" => {
                    let props = m.get("properties").and_then(Value::as_object).unwrap();
                    for r in v.as_array().unwrap() {
                        assert!(props.contains_key(r.as_str().unwrap()), "{path}: required {r} has no property");
                    }
                }
                "type" => {
                    let ts: Vec<&str> = match v {
                        Value::String(s) => vec![s],
                        Value::Array(a) => a.iter().map(|x| x.as_str().unwrap()).collect(),
                        _ => panic!("{path}: bad type"),
                    };
                    for t in ts {
                        assert!(
                            ["object", "array", "string", "number", "integer", "boolean"].contains(&t),
                            "{path}: bad type {t}"
                        );
                    }
                }
                _ => {}
            }
        }
    }

    #[test]
    fn every_tool_has_an_object_schema_the_validator_understands() {
        assert_eq!(TOOLS.len(), 17);
        for t in TOOLS {
            let s = (t.schema)();
            assert_eq!(s["type"], "object", "{}", t.name);
            assert_eq!(s["additionalProperties"], false, "{}", t.name);
            keywords_known(&s, t.name);
            assert!(t.name.starts_with("record_"));
            assert!(t.method.starts_with("record."));
            assert!(!t.description.is_empty());
            assert!(!(t.read_only && t.destructive), "{}", t.name);
        }
    }

    #[test]
    fn names_are_unique_and_found() {
        for (i, t) in TOOLS.iter().enumerate() {
            assert!(TOOLS[i + 1..].iter().all(|o| o.name != t.name), "{}", t.name);
            assert_eq!(find(t.name).unwrap().name, t.name);
        }
        assert!(find("record_nope").is_none());
        assert!(find("").is_none());
    }

    #[test]
    fn hints_match_the_spec_table() {
        let ro: Vec<&str> = TOOLS.iter().filter(|t| t.read_only).map(|t| t.name).collect();
        assert_eq!(
            ro,
            ["record_projects", "record_project", "record_doc", "record_transcript", "record_sources", "record_frames", "record_status"]
        );
        assert!(find("record_delete").unwrap().destructive);
        // stop/pause/resume/status never launch the app.
        let no_launch: Vec<&str> = TOOLS.iter().filter(|t| !t.launch).map(|t| t.name).collect();
        assert_eq!(no_launch, ["record_stop", "record_pause", "record_resume", "record_status"]);
        // Long methods have no deadline.
        for n in ["record_export", "record_transcribe", "record_stop", "record_start", "record_edit"] {
            assert!(find(n).unwrap().deadline.is_none(), "{n}");
        }
    }

    #[test]
    fn listing_has_annotations_only_when_asked() {
        let t = find("record_delete").unwrap();
        let with = t.listing(true);
        assert_eq!(with["annotations"]["destructiveHint"], true);
        assert_eq!(with["annotations"]["readOnlyHint"], false);
        assert_eq!(with["inputSchema"]["required"], json!(["project", "confirm"]));
        assert!(t.listing(false).get("annotations").is_none());
    }

    #[test]
    fn params_mirror_the_arguments_and_auto_zoom_becomes_an_edit() {
        let args = json!({"project": "take", "format": "gif", "gifWidth": 480});
        let t = find("record_export").unwrap();
        assert_eq!(t.params(args.as_object().unwrap()), args);
        let z = find("record_auto_zoom").unwrap();
        assert_eq!(z.method, "record.edit");
        assert_eq!(
            z.params(json!({"project": "take"}).as_object().unwrap()),
            json!({"project": "take", "ops": [{"op": "zoom.auto"}]})
        );
    }

    #[test]
    fn edit_op_enum_has_every_kind_once() {
        let s = edit_schema();
        let e = s["properties"]["ops"]["items"]["properties"]["op"]["enum"].as_array().unwrap().clone();
        assert_eq!(e.len(), 19);
        for (i, op) in e.iter().enumerate() {
            assert!(!e[i + 1..].contains(op), "{op} twice");
        }
        assert!(e.contains(&json!("look.set")));
    }

    fn check(tool: &str, args: Value) -> Result<(), String> {
        validate(&args, &(find(tool).unwrap().schema)(), "arguments")
    }

    #[test]
    fn validation_accepts_good_arguments() {
        check("record_projects", json!({})).unwrap();
        check("record_export", json!({"project": "a", "fps": 30, "gifWidth": 640, "codec": "hevc"})).unwrap();
        check("record_export", json!({"project": "a", "fps": 30.0})).unwrap(); // 30.0 is an integer
        check("record_frames", json!({"project": "a", "atProgrammeMs": 1500.5})).unwrap();
        check("record_frames", json!({"project": "a", "atProgrammeMs": [0, 1000, 2000]})).unwrap();
        check("record_start", json!({"display": 2, "camera": "FaceTime", "countdown": true})).unwrap();
        check("record_start", json!({"area": {"x": 0, "y": 0, "w": 640, "h": 360, "display": "Built-in"}})).unwrap();
        check("record_edit", json!({"project": "a", "ops": [{"op": "zoom.add", "atProgrammeMs": 0, "depth": 3}]})).unwrap();
        check("record_delete", json!({"project": "a", "confirm": true})).unwrap();
    }

    #[test]
    fn validation_rejects_wrong_types_huge_numbers_and_typos() {
        let e = check("record_export", json!({"project": "a", "fps": "30"})).unwrap_err();
        assert_eq!(e, "arguments.fps: expected integer, got string");
        assert!(check("record_export", json!({"project": "a", "fps": 1e300})).unwrap_err().contains("≤ 60"));
        assert!(check("record_export", json!({"project": "a", "fps": 30.5})).is_err());
        assert!(check("record_export", json!({"project": "a", "gifWidth": 500})).unwrap_err().contains("one of"));
        assert!(check("record_export", json!({"project": "a", "fromat": "gif"})).unwrap_err().contains("unknown field `fromat`"));
        assert!(check("record_export", json!({})).unwrap_err().contains("missing `project`"));
        assert!(check("record_project", json!({"project": "  "})).unwrap_err().contains("empty"));
        assert!(check("record_project", json!({"project": 7})).is_err());
        assert!(check("record_frames", json!({"project": "a", "atProgrammeMs": -5})).unwrap_err().contains("≥ 0"));
        assert!(check("record_frames", json!({"project": "a", "atProgrammeMs": [1, -1]}))
            .unwrap_err()
            .starts_with("arguments.atProgrammeMs[1]"));
        assert!(check("record_frames", json!({"project": "a", "atProgrammeMs": vec![0; 9]})).unwrap_err().contains("at most 8"));
        assert!(check("record_frames", json!({"project": "a", "atProgrammeMs": []})).unwrap_err().contains("at least 1"));
        assert!(check("record_edit", json!({"project": "a", "ops": [{"op": "zoom.explode"}]})).is_err());
        assert!(check("record_edit", json!({"project": "a", "ops": [{"depth": 3}]})).unwrap_err().contains("missing `op`"));
        assert!(check("record_edit", json!({"project": "a", "ops": "zoom.add"})).is_err());
        assert!(check("record_delete", json!({"project": "a", "confirm": false})).unwrap_err().contains("must be true"));
        assert!(check("record_start", json!({"area": {"x": 0, "y": 0, "w": 10, "h": 360}})).is_err());
        assert!(check("record_start", json!({"camera": 3})).is_err());
        assert!(check("record_stop", json!({"open": "yes"})).is_err());
    }
}
