//! Control-socket methods for recordings (stage 8, Task 3): list the sources, start a take
//! with a given source (no picker), pause, resume, stop, status.
//!
//! A control start runs the normal session (`session::start_with`): the HUD closes, the
//! countdown shows when asked, the control bar appears, the take ends through `record.stop`
//! or the bar. The source comes from the call, never from the picker slot, so a pick the
//! HUD holds stays untouched. Recording always runs inside the app (its TCC grants).
//!
//! [`pick_source`] and [`start_plan`] are pure; the handlers are thin.

use super::control_ops::ERR_BAD_PARAMS;
use super::session::{self, next_phase, Action, Phase, RecordConfig, StartOptions};
use crate::control::{Call, ControlError};
use serde_json::{json, Map, Value};
use tauri::AppHandle;

pub const ERR_AMBIGUOUS: &str = "record.control.ambiguous";
pub const ERR_NO_MATCH: &str = "record.control.no_match";
/// Most candidates an ambiguity error names.
pub const MAX_CANDIDATES: usize = 10;

fn perr(msg: impl Into<String>) -> ControlError {
    ControlError::new(ERR_BAD_PARAMS, msg)
}

// ---- pure: picking a source -----------------------------------------------------------

/// Something [`pick_source`] can choose: a display, a window, a camera.
pub trait Candidate {
    fn cand_id(&self) -> String;
    fn cand_name(&self) -> String;
}

/// The entry `query` names: an exact id first, then one whose name equals the query (case
/// ignored), then the one whose name contains it. Several equal names or several containing
/// names are `record.control.ambiguous` (the message lists them, id first, so the caller
/// can repeat the call with an id); nothing is `record.control.no_match`.
pub fn pick_source<'a, T: Candidate>(list: &'a [T], query: &str, what: &str) -> Result<&'a T, ControlError> {
    let q = query.trim();
    if q.is_empty() {
        return Err(perr(format!("{what} is empty")));
    }
    if let Some(hit) = list.iter().find(|c| c.cand_id() == q) {
        return Ok(hit);
    }
    let ql = q.to_lowercase();
    let equal: Vec<&T> = list.iter().filter(|c| c.cand_name().to_lowercase() == ql).collect();
    let matches: Vec<&T> = if equal.is_empty() {
        list.iter().filter(|c| c.cand_name().to_lowercase().contains(&ql)).collect()
    } else {
        equal
    };
    match matches.as_slice() {
        [one] => Ok(one),
        [] => {
            let avail: Vec<String> = list.iter().take(MAX_CANDIDATES).map(describe).collect();
            Err(ControlError::new(
                ERR_NO_MATCH,
                if avail.is_empty() {
                    format!("no {what} matches \"{q}\" (none available)")
                } else {
                    format!("no {what} matches \"{q}\"; available: {}", avail.join("; "))
                },
            ))
        }
        many => {
            let mut names: Vec<String> = many.iter().take(MAX_CANDIDATES).map(|c| describe(*c)).collect();
            if many.len() > MAX_CANDIDATES {
                names.push(format!("… {} more", many.len() - MAX_CANDIDATES));
            }
            Err(ControlError::new(
                ERR_AMBIGUOUS,
                format!("\"{q}\" matches {} {what}s, give an id: {}", many.len(), names.join("; ")),
            ))
        }
    }
}

fn describe<T: Candidate>(c: &T) -> String {
    format!("{} = {}", c.cand_id(), c.cand_name())
}

#[cfg(target_os = "macos")]
impl Candidate for super::capture_macos::ShareableDisplay {
    fn cand_id(&self) -> String {
        self.id.to_string()
    }
    fn cand_name(&self) -> String {
        display_name(self)
    }
}

#[cfg(target_os = "macos")]
impl Candidate for super::capture_macos::ShareableWindow {
    fn cand_id(&self) -> String {
        self.id.to_string()
    }
    fn cand_name(&self) -> String {
        window_name(&self.app, &self.title)
    }
}

impl Candidate for super::camera::CameraDevice {
    fn cand_id(&self) -> String {
        self.id.clone()
    }
    fn cand_name(&self) -> String {
        self.name.clone()
    }
}

/// "Google Chrome — Inbox"; one of the two alone when the other is empty.
pub fn window_name(app: &str, title: &str) -> String {
    match (app.trim().is_empty(), title.trim().is_empty()) {
        (false, false) => format!("{app} — {title}"),
        (false, true) => app.to_string(),
        _ => title.to_string(),
    }
}

/// The display's own name, else "Display <id>".
#[cfg(target_os = "macos")]
fn display_name(d: &super::capture_macos::ShareableDisplay) -> String {
    if d.name.trim().is_empty() {
        format!("Display {}", d.id)
    } else {
        d.name.clone()
    }
}

// ---- pure: start options --------------------------------------------------------------

/// The source a `record.start` call names (resolved against the live list later).
#[derive(Debug, Clone, PartialEq)]
pub enum SourceQuery {
    /// Nothing given: the main display.
    MainDisplay,
    Display(String),
    Window(String),
    /// Display-local points; `display` = None is the main display.
    Area { x: f64, y: f64, w: f64, h: f64, display: Option<String> },
}

/// What the call asks of the camera.
#[derive(Debug, Clone, PartialEq)]
pub enum CameraChoice {
    /// The camera setting decides (on/off and device).
    Setting,
    Off,
    /// On, with the configured device.
    Configured,
    /// On, with the device this query names.
    Device(String),
}

#[derive(Debug, Clone)]
pub struct StartPlan {
    pub source: SourceQuery,
    /// The session's options; `camera_device` still holds the setting when `camera` is
    /// [`CameraChoice::Device`] (the handler resolves the device).
    pub opts: StartOptions,
    pub camera: CameraChoice,
}

/// Every key `record.start` reads.
pub const START_KEYS: [&str; 8] = ["display", "window", "area", "mic", "systemAudio", "camera", "countdown", "title"];

fn id_or_name(v: &Value, k: &str) -> Result<String, ControlError> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Ok(s.trim().to_string()),
        Value::Number(n) if n.as_u64().is_some() => Ok(n.to_string()),
        _ => Err(perr(format!("{k} must be an id or a name"))),
    }
}

fn opt_bool(m: &Map<String, Value>, k: &str) -> Result<Option<bool>, ControlError> {
    match m.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(perr(format!("{k} must be true or false"))),
    }
}

fn area_num(a: &Map<String, Value>, k: &str) -> Result<f64, ControlError> {
    a.get(k)
        .and_then(Value::as_f64)
        .filter(|f| f.is_finite())
        .ok_or_else(|| perr(format!("area.{k} must be a number")))
}

/// The session options a `record.start` call asks for, on top of the record settings `cfg`:
/// `mic` / `systemAudio` / `camera` default to the settings, `countdown` to off (the client
/// decides: the CLI asks for one, an agent does not), the pointer mode is the setting's.
/// At most one of `display`, `window`, `area`; none = the main display.
pub fn start_plan(params: &Value, cfg: &RecordConfig) -> Result<StartPlan, ControlError> {
    let empty = Map::new();
    let m = match params {
        Value::Object(m) => m,
        Value::Null => &empty,
        _ => return Err(perr("params must be an object")),
    };
    if let Some(k) = m.keys().find(|k| !START_KEYS.contains(&k.as_str())) {
        return Err(perr(format!("unknown parameter {k} (record.start takes {})", START_KEYS.join(", "))));
    }
    let given: Vec<&str> =
        ["display", "window", "area"].into_iter().filter(|k| m.get(*k).is_some_and(|v| !v.is_null())).collect();
    if given.len() > 1 {
        return Err(perr(format!("give one source, not {}", given.join(" and "))));
    }
    let source = match given.first().copied() {
        None => SourceQuery::MainDisplay,
        Some("display") => SourceQuery::Display(id_or_name(&m["display"], "display")?),
        Some("window") => SourceQuery::Window(id_or_name(&m["window"], "window")?),
        Some(_) => {
            let a = m["area"].as_object().ok_or_else(|| perr("area must be an object {x, y, w, h, display?}"))?;
            if let Some(k) = a.keys().find(|k| !["x", "y", "w", "h", "display"].contains(&k.as_str())) {
                return Err(perr(format!("unknown area key {k}")));
            }
            let (w, h) = (area_num(a, "w")?, area_num(a, "h")?);
            if w <= 0.0 || h <= 0.0 {
                return Err(perr("area.w and area.h must be positive"));
            }
            let display = match a.get("display") {
                None | Some(Value::Null) => None,
                Some(v) => Some(id_or_name(v, "area.display")?),
            };
            SourceQuery::Area { x: area_num(a, "x")?, y: area_num(a, "y")?, w, h, display }
        }
    };
    let camera = match m.get("camera") {
        None | Some(Value::Null) => CameraChoice::Setting,
        Some(Value::Bool(false)) => CameraChoice::Off,
        Some(Value::Bool(true)) => CameraChoice::Configured,
        Some(Value::String(s)) if !s.trim().is_empty() => CameraChoice::Device(s.trim().to_string()),
        Some(_) => return Err(perr("camera must be true, false or a camera id/name")),
    };
    let title = match m.get("title") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.trim().to_string(),
        Some(_) => return Err(perr("title must be a string")),
    };
    let opts = StartOptions {
        system_audio: opt_bool(m, "systemAudio")?.unwrap_or(cfg.system_audio),
        mic: opt_bool(m, "mic")?.unwrap_or(cfg.mic),
        cursor_mode: cfg.cursor_mode,
        countdown: opt_bool(m, "countdown")?.unwrap_or(false),
        title,
        camera: match camera {
            CameraChoice::Setting => cfg.camera,
            CameraChoice::Off => false,
            CameraChoice::Configured | CameraChoice::Device(_) => true,
        },
        camera_device: cfg.camera_device.clone(),
        camera_quality: cfg.camera_quality,
    };
    Ok(StartPlan { source, opts, camera })
}

/// Why a start is refused in `phase` before anything is enumerated (the session checks
/// again atomically when it starts): `record.busy` while a take counts down, runs, pauses
/// or finishes.
pub fn start_refusal(phase: Phase) -> Option<&'static str> {
    next_phase(phase, Action::Start).err()
}

// ---- handlers -------------------------------------------------------------------------

fn to_json(v: &impl serde::Serialize) -> Result<Value, ControlError> {
    serde_json::to_value(v).map_err(|e| ControlError::new("control.internal", e.to_string()))
}

fn state(app: &AppHandle) -> Result<tauri::State<'_, session::RecordState>, ControlError> {
    use tauri::Manager;
    app.try_state::<session::RecordState>().ok_or_else(|| ControlError::new(session::ERR_UNSUPPORTED, "no recorder"))
}

fn config(app: &AppHandle) -> RecordConfig {
    use tauri::Manager;
    app.try_state::<crate::db::DbHandle>().map(|db| RecordConfig::load(&db)).unwrap_or_default()
}

/// Sentinel for a missing Screen Recording grant, checked before any listing (which would
/// otherwise wait up to 5 s and fail with a guess).
pub const ERR_PERMISSION: &str = "record.permission";

fn screen_permission() -> Result<(), ControlError> {
    if crate::screen_recording::screen_recording_granted() {
        Ok(())
    } else {
        Err(ControlError::new(
            ERR_PERMISSION,
            "Inspector Rust has no Screen Recording permission (System Settings → Privacy & Security → Screen Recording)",
        ))
    }
}

fn sources_failed(e: String) -> ControlError {
    ControlError::new(session::ERR_START_FAILED, format!("cannot list the screen's sources (Screen Recording?): {e}"))
}

/// Input devices by name (the take records the system default input).
fn mics() -> Vec<Value> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| d.name().ok());
    let Ok(devs) = host.input_devices() else { return Vec::new() };
    devs.filter_map(|d| d.name().ok())
        .map(|n| json!({"id": n, "name": n, "default": Some(&n) == default.as_ref()}))
        .collect()
}

pub(crate) fn sources(_app: &AppHandle, _c: &mut Call) -> Result<Value, ControlError> {
    session::support_reason().map_err(|e| ControlError::new(session::ERR_UNSUPPORTED, crate::i18n::t(e)))?;
    screen_permission()?;
    let (displays, windows) = super::capture_macos::shareable_sources().map_err(sources_failed)?;
    let cameras: Vec<Value> = super::camera::devices()
        .into_iter()
        .map(|c| json!({"id": c.id, "name": c.name, "builtin": c.builtin}))
        .collect();
    Ok(json!({
        "displays": displays.iter().map(|d| json!({
            "id": d.id, "name": display_name(d), "width": d.width, "height": d.height, "main": d.main,
        })).collect::<Vec<_>>(),
        "windows": windows.iter().map(|w| json!({"id": w.id, "app": w.app, "title": w.title})).collect::<Vec<_>>(),
        "mics": mics(),
        "cameras": cameras,
    }))
}

/// The display `q` names, or the main display.
fn display_for<'a>(
    list: &'a [super::capture_macos::ShareableDisplay],
    q: Option<&str>,
) -> Result<&'a super::capture_macos::ShareableDisplay, ControlError> {
    match q {
        Some(q) => pick_source(list, q, "display"),
        None => list
            .iter()
            .find(|d| d.main)
            .or_else(|| list.first())
            .ok_or_else(|| ControlError::new(ERR_NO_MATCH, "no display available")),
    }
}

fn picked_for(source: &SourceQuery) -> Result<super::picker_macos::Picked, ControlError> {
    use super::picker_macos::Picked;
    screen_permission()?;
    let (displays, windows) = super::capture_macos::shareable_sources().map_err(sources_failed)?;
    let start_failed = |e: String| ControlError::new(session::ERR_START_FAILED, e);
    match source {
        SourceQuery::MainDisplay | SourceQuery::Display(_) => {
            let q = match source {
                SourceQuery::Display(q) => Some(q.as_str()),
                _ => None,
            };
            let d = display_for(&displays, q)?;
            let mut p = Picked::display(d.id).map_err(start_failed)?;
            p.label = display_name(d);
            Ok(p)
        }
        SourceQuery::Window(q) => {
            let w = pick_source(&windows, q, "window")?;
            Picked::window(w.id, &window_name(&w.app, &w.title)).map_err(start_failed)
        }
        SourceQuery::Area { x, y, w, h, display } => {
            let d = display_for(&displays, display.as_deref())?;
            let r = super::core::clamp_region((*x, *y, *w, *h), d.width, d.height, super::core::MIN_REGION_PT)
                .ok_or_else(|| {
                    perr(format!(
                        "the area must lie on the {}×{} display and be at least {} points per side",
                        d.width,
                        d.height,
                        super::core::MIN_REGION_PT
                    ))
                })?;
            Picked::region(d.id, r).map_err(start_failed)
        }
    }
}

fn folder_of(dir: &str) -> String {
    std::path::Path::new(dir).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub(crate) fn start(app: &AppHandle, c: &mut Call) -> Result<Value, ControlError> {
    session::support_reason().map_err(|e| ControlError::new(session::ERR_UNSUPPORTED, crate::i18n::t(e)))?;
    if let Some(code) = start_refusal(state(app)?.phase()) {
        return Err(ControlError::new(code, "a take is already running; stop it with record.stop"));
    }
    let mut plan = start_plan(c.params, &config(app))?;
    if let CameraChoice::Device(q) = &plan.camera {
        let list = super::camera::devices();
        plan.opts.camera_device = pick_source(&list, q, "camera")?.id.clone();
    }
    let picked = picked_for(&plan.source)?;
    if plan.opts.countdown {
        c.emit(json!({"phase": "countdown", "seconds": 3}));
    }
    let st = session::start_with(app, plan.opts, Some(picked)).map_err(ControlError::sentinel)?;
    let dir = st.project_dir.clone().unwrap_or_default();
    Ok(json!({"project": dir, "folder": folder_of(&dir), "status": to_json(&st)?}))
}

pub(crate) fn stop(app: &AppHandle, c: &mut Call) -> Result<Value, ControlError> {
    state(app)?;
    let follow_up = match c.params.get("open") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(perr("open must be true or false")),
    };
    let t = session::stop_take(app, follow_up).map_err(ControlError::sentinel)?;
    let status = match t.status {
        super::project::ProjectStatus::Ready => "ready",
        super::project::ProjectStatus::Failed => "failed",
        super::project::ProjectStatus::Incomplete => "incomplete",
        super::project::ProjectStatus::Recording => "recording",
    };
    Ok(json!({
        "project": t.dir,
        "folder": folder_of(&t.dir),
        "ready": t.status == super::project::ProjectStatus::Ready,
        "status": status,
        "failure": t.failure,
    }))
}

pub(crate) fn pause(app: &AppHandle, _c: &mut Call) -> Result<Value, ControlError> {
    state(app)?;
    to_json(&session::pause(app).map_err(ControlError::sentinel)?)
}

pub(crate) fn resume(app: &AppHandle, _c: &mut Call) -> Result<Value, ControlError> {
    state(app)?;
    to_json(&session::resume(app).map_err(ControlError::sentinel)?)
}

pub(crate) fn status(app: &AppHandle, _c: &mut Call) -> Result<Value, ControlError> {
    to_json(&state(app)?.status())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::camera::CameraDevice;

    fn cam(id: &str, name: &str) -> CameraDevice {
        CameraDevice { id: id.into(), name: name.into(), builtin: false }
    }

    fn cams() -> Vec<CameraDevice> {
        vec![cam("A1", "FaceTime HD Camera"), cam("B2", "Logitech BRIO"), cam("C3", "Logitech C920")]
    }

    #[test]
    fn pick_source_by_exact_id() {
        let l = cams();
        assert_eq!(pick_source(&l, "B2", "camera").unwrap().id, "B2");
        // An id wins over a name that merely contains it.
        let l = vec![cam("brio", "x"), cam("Z", "brio 2")];
        assert_eq!(pick_source(&l, "brio", "camera").unwrap().id, "brio");
    }

    #[test]
    fn pick_source_by_unique_substring_ignoring_case() {
        let l = cams();
        assert_eq!(pick_source(&l, "facetime", "camera").unwrap().id, "A1");
        assert_eq!(pick_source(&l, "  BRIO ", "camera").unwrap().id, "B2");
    }

    #[test]
    fn pick_source_ambiguous_lists_the_candidates() {
        let l = cams();
        let e = pick_source(&l, "logitech", "camera").unwrap_err();
        assert_eq!(e.code, ERR_AMBIGUOUS);
        assert!(e.message.contains("B2 = Logitech BRIO") && e.message.contains("C3 = Logitech C920"), "{}", e.message);
        assert!(!e.message.contains("A1"), "{}", e.message);
    }

    #[test]
    fn pick_source_an_equal_name_beats_longer_ones() {
        let l = vec![cam("1", "Chrome"), cam("2", "Chrome Canary")];
        assert_eq!(pick_source(&l, "chrome", "window").unwrap().id, "1");
        // Two equal names stay ambiguous.
        let l = vec![cam("1", "Terminal"), cam("2", "terminal")];
        assert_eq!(pick_source(&l, "Terminal", "window").unwrap_err().code, ERR_AMBIGUOUS);
    }

    #[test]
    fn pick_source_none_and_empty() {
        let l = cams();
        let e = pick_source(&l, "sony", "camera").unwrap_err();
        assert_eq!(e.code, ERR_NO_MATCH);
        assert!(e.message.contains("A1 = FaceTime HD Camera"), "{}", e.message);
        assert_eq!(pick_source::<CameraDevice>(&[], "x", "camera").unwrap_err().code, ERR_NO_MATCH);
        assert_eq!(pick_source(&l, "  ", "camera").unwrap_err().code, ERR_BAD_PARAMS);
    }

    #[test]
    fn pick_source_caps_long_ambiguity_lists() {
        let l: Vec<CameraDevice> = (0..15).map(|i| cam(&i.to_string(), &format!("Cam {i}"))).collect();
        let e = pick_source(&l, "cam", "camera").unwrap_err();
        assert_eq!(e.code, ERR_AMBIGUOUS);
        assert!(e.message.contains("15 cameras") && e.message.contains("… 5 more"), "{}", e.message);
    }

    fn cfg() -> RecordConfig {
        RecordConfig {
            mic: true,
            system_audio: false,
            camera: true,
            camera_device: "dev-1".into(),
            countdown: true,
            ..RecordConfig::default()
        }
    }

    #[test]
    fn start_plan_defaults_come_from_the_settings_except_the_countdown() {
        let p = start_plan(&json!({}), &cfg()).unwrap();
        assert_eq!(p.source, SourceQuery::MainDisplay);
        assert_eq!(p.camera, CameraChoice::Setting);
        let o = &p.opts;
        assert!(o.mic && !o.system_audio && o.camera, "{o:?}");
        assert_eq!(o.camera_device, "dev-1");
        assert!(!o.countdown, "the client asks for a countdown, the setting does not apply");
        assert_eq!(o.cursor_mode, cfg().cursor_mode);
        assert_eq!(o.title, "");
        assert_eq!(start_plan(&Value::Null, &cfg()).unwrap().source, SourceQuery::MainDisplay);
    }

    #[test]
    fn start_plan_overrides_map_onto_the_session_options() {
        let p = start_plan(
            &json!({"window": 4711, "mic": false, "systemAudio": true, "camera": false, "countdown": true, "title": " Demo "}),
            &cfg(),
        )
        .unwrap();
        assert_eq!(p.source, SourceQuery::Window("4711".into()));
        let o = &p.opts;
        assert!(!o.mic && o.system_audio && !o.camera && o.countdown, "{o:?}");
        assert_eq!(o.title, "Demo");
        let p = start_plan(&json!({"display": "Studio", "camera": "BRIO"}), &RecordConfig::default()).unwrap();
        assert_eq!(p.source, SourceQuery::Display("Studio".into()));
        assert_eq!(p.camera, CameraChoice::Device("BRIO".into()));
        assert!(p.opts.camera);
        let p = start_plan(&json!({"camera": true}), &RecordConfig::default()).unwrap();
        assert_eq!(p.camera, CameraChoice::Configured);
        assert!(p.opts.camera);
    }

    #[test]
    fn start_plan_area() {
        let p = start_plan(&json!({"area": {"x": 10, "y": 20.5, "w": 640, "h": 360, "display": 2}}), &cfg()).unwrap();
        assert_eq!(p.source, SourceQuery::Area { x: 10.0, y: 20.5, w: 640.0, h: 360.0, display: Some("2".into()) });
        let p = start_plan(&json!({"area": {"x": 0, "y": 0, "w": 100, "h": 100}}), &cfg()).unwrap();
        assert!(matches!(p.source, SourceQuery::Area { display: None, .. }));
        for bad in [
            json!({"area": {"x": 0, "y": 0, "w": 0, "h": 100}}),
            json!({"area": {"x": 0, "y": 0, "w": 100}}),
            json!({"area": {"x": "a", "y": 0, "w": 100, "h": 100}}),
            json!({"area": {"x": 0, "y": 0, "w": 100, "h": 100, "z": 1}}),
            json!({"area": [0, 0, 100, 100]}),
        ] {
            assert_eq!(start_plan(&bad, &cfg()).unwrap_err().code, ERR_BAD_PARAMS, "{bad}");
        }
    }

    #[test]
    fn start_plan_refuses_bad_input() {
        for bad in [
            json!({"display": 1, "window": 2}),
            json!({"window": 2, "area": {"x": 0, "y": 0, "w": 100, "h": 100}}),
            json!({"display": ""}),
            json!({"display": true}),
            json!({"window": -3}),
            json!({"mic": "yes"}),
            json!({"countdown": 1}),
            json!({"camera": 3}),
            json!({"camera": ""}),
            json!({"title": 5}),
            json!({"source": "main"}),
            json!([1]),
        ] {
            assert_eq!(start_plan(&bad, &cfg()).unwrap_err().code, ERR_BAD_PARAMS, "{bad}");
        }
    }

    #[test]
    fn start_while_a_take_runs_is_busy() {
        for p in [Phase::Countdown, Phase::Recording, Phase::Paused, Phase::Finishing] {
            assert_eq!(start_refusal(p), Some(session::ERR_BUSY), "{p:?}");
        }
        assert_eq!(start_refusal(Phase::Idle), None);
        assert_eq!(start_refusal(Phase::Picking), None);
    }

    #[test]
    fn listable_windows_are_normal_foreign_and_named() {
        use crate::record::capture_macos::listable_window;
        assert!(listable_window(0, 10, 1, 800.0, 600.0, "Safari", ""));
        assert!(listable_window(0, 10, 1, 800.0, 600.0, "", "Untitled"));
        assert!(!listable_window(25, 10, 1, 800.0, 600.0, "Dock", "Dock"), "above normal level");
        assert!(!listable_window(0, 1, 1, 800.0, 600.0, "Inspector Rust", "Popup"), "our own window");
        assert!(!listable_window(0, 10, 1, 20.0, 600.0, "Safari", "x"), "too narrow");
        assert!(!listable_window(0, 10, 1, 800.0, 600.0, " ", ""), "nameless");
        assert_eq!(window_name("Safari", "Inbox"), "Safari — Inbox");
        assert_eq!(window_name("Safari", ""), "Safari");
        assert_eq!(window_name("", "Inbox"), "Inbox");
    }

    #[test]
    fn start_keys_match_what_start_plan_reads() {
        let src = include_str!("control_rec.rs");
        let body = src.split("pub fn start_plan(").nth(1).unwrap().split("\npub fn start_refusal").next().unwrap();
        for k in START_KEYS {
            assert!(body.contains(&format!("\"{k}\"")), "{k} is never read");
        }
    }

    /// Lists the sources and records 3 s of the main display, picked by name, through the
    /// same picking path as `record.start` (`picked_for` → session capture). Needs Screen
    /// Recording for the test binary and an awake display (`caffeinate -u -t 1`).
    #[test]
    #[ignore]
    fn control_record_live_main_display_3s() {
        let (displays, windows) = crate::record::capture_macos::shareable_sources().expect("sources");
        println!("displays: {displays:?}");
        println!("{} windows, e.g. {:?}", windows.len(), windows.first());
        let main = displays.iter().find(|d| d.main).expect("main display");
        let picked = picked_for(&SourceQuery::Display(main.id.to_string())).expect("picked");
        assert_eq!(picked.display_id, Some(main.id));
        let root = std::env::temp_dir().join(format!("ir-control-rec-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let plan = start_plan(&json!({"mic": false, "systemAudio": false, "camera": false}), &RecordConfig::default())
            .unwrap();
        let out = session::record_headless(&root, picked, plan.opts, 3.0);
        println!("outcome: {:?} {:?}", out.status, out.failure);
        assert_eq!(out.status, crate::record::project::ProjectStatus::Ready, "{:?}", out.failure);
        assert!(out.dir.join(crate::record::project::SCREEN_FILE).is_file());
        let _ = std::fs::remove_dir_all(&root);
    }
}
