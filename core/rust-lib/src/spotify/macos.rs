//! The macOS edge of the Spotify integration: everything that actually talks
//! to Spotify or the system. Kept thin — decisions live in `super::Tracker`.
//!
//! Sources of truth, in order of preference:
//! 1. **Spotify's own notification** `com.spotify.client.PlaybackStateChanged`
//!    (distributed, posted on every track/state change). Needs no permission,
//!    costs nothing while idle. This alone is enough for the history.
//! 2. **NSWorkspace launch/terminate** of `com.spotify.client` — quitting
//!    Spotify ends the current play at the right moment.
//! 3. **AppleScript** (`osascript`), only if the Automation permission for
//!    Spotify is already granted: reconciles the state every minute and adds
//!    the cover URL. The background recorder never triggers the permission
//!    prompt; only a user action (`spotify now`, a control command) can.
//!
//! The recorder thread (`ir-spotify`) ticks every 15 s without spawning
//! anything: it advances the played time, closes long pauses and checks that
//! Spotify still runs.

use super::store;
use super::{Change, Field, Observation, Play, PlayerState, Tracker};
use crate::db::DbHandle;
use crate::osascript_util::{run_osascript, OsaResult};
use block2::RcBlock;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::msg_send;
use objc2_foundation::NSString;
use serde::Serialize;
use std::ffi::{c_char, c_void, CStr};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Recorder tick.
pub const TICK: Duration = Duration::from_secs(15);
/// AppleScript reconcile while Spotify runs (only with permission), in ticks.
const POLL_EVERY_TICKS: u32 = 4;
const OSA_TIMEOUT: Duration = Duration::from_secs(4);
/// Event for the panel: something about playback or the history changed.
pub const CHANGED_EVENT: &str = "spotify-changed";

pub const ERR_NOT_INSTALLED: &str = "spotify.not_installed";
pub const ERR_NOT_RUNNING: &str = "spotify.not_running";
pub const ERR_DENIED: &str = "spotify.automation_denied";

const SETTING_HISTORY: &str = "spotify.history";
const SETTING_ARTWORK: &str = "spotify.artwork";

static HISTORY_ON: AtomicBool = AtomicBool::new(true);
/// Cached Automation permission (see `Permission`).
static PERMISSION: AtomicU8 = AtomicU8::new(Permission::Unknown as u8);
static SENDER: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();
/// Spotify's own volume from the last AppleScript reading (`NO_VOLUME` =
/// never read — no permission yet). Feeds `audible_now`.
static SPOTIFY_VOLUME: AtomicU8 = AtomicU8::new(NO_VOLUME);
const NO_VOLUME: u8 = u8::MAX;
/// What the recorder knows right now (for `now()` without AppleScript).
static LAST: Mutex<Option<Play>> = Mutex::new(None);

enum Msg {
    Obs(Observation),
    /// Spotify launched — reconcile.
    Launched,
    /// Re-read settings / close the open play when recording was disabled.
    Reconfigure,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ── Installed / running ─────────────────────────────────────────────────────

pub fn installed() -> bool {
    let home = dirs::home_dir().map(|h| h.join("Applications/Spotify.app"));
    std::path::Path::new("/Applications/Spotify.app").exists() || home.is_some_and(|p| p.exists())
}

/// `NSRunningApplication` is thread-safe; no process spawn.
pub fn running() -> bool {
    unsafe {
        let Some(cls) = AnyClass::get(c"NSRunningApplication") else {
            return false;
        };
        let id = NSString::from_str(super::BUNDLE_ID);
        let apps: *mut AnyObject = msg_send![cls, runningApplicationsWithBundleIdentifier: &*id];
        if apps.is_null() {
            return false;
        }
        let n: usize = msg_send![apps, count];
        n > 0
    }
}

// ── Automation permission ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Permission {
    Granted = 0,
    Denied = 1,
    /// Never asked — the first AppleScript will show the macOS prompt.
    Undetermined = 2,
    /// Can't tell (Spotify not running).
    Unknown = 3,
}

impl Permission {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Granted,
            1 => Self::Denied,
            2 => Self::Undetermined,
            _ => Self::Unknown,
        }
    }

    /// Map `AEDeterminePermissionToAutomateTarget`'s status.
    pub fn from_status(status: i32) -> Self {
        match status {
            0 => Self::Granted,
            -1743 => Self::Denied,
            -1744 => Self::Undetermined,
            _ => Self::Unknown,
        }
    }
}

#[repr(C)]
struct AEDesc {
    descriptor_type: u32,
    data_handle: *mut c_void,
}

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn AECreateDesc(type_code: u32, data: *const c_void, size: isize, result: *mut AEDesc) -> i16;
    fn AEDisposeDesc(desc: *mut AEDesc) -> i16;
    fn AEDeterminePermissionToAutomateTarget(
        target: *const AEDesc,
        event_class: u32,
        event_id: u32,
        ask_user_if_needed: u8,
    ) -> i32;
}

const TYPE_APPLICATION_BUNDLE_ID: u32 = u32::from_be_bytes(*b"bund");
const TYPE_WILDCARD: u32 = u32::from_be_bytes(*b"****");

/// Ask TCC whether Inspector may send Apple events to Spotify — WITHOUT
/// showing a prompt. Spotify must be running for a definite answer.
pub fn permission() -> Permission {
    let id = super::BUNDLE_ID.as_bytes();
    let mut desc = AEDesc {
        descriptor_type: 0,
        data_handle: std::ptr::null_mut(),
    };
    // SAFETY: plain C API; `desc` is initialised by AECreateDesc and disposed
    // below; the bundle id bytes outlive the call.
    let status = unsafe {
        if AECreateDesc(TYPE_APPLICATION_BUNDLE_ID, id.as_ptr().cast(), id.len() as isize, &mut desc) != 0 {
            return Permission::Unknown;
        }
        let s = AEDeterminePermissionToAutomateTarget(&desc, TYPE_WILDCARD, TYPE_WILDCARD, 0);
        AEDisposeDesc(&mut desc);
        s
    };
    let p = Permission::from_status(status);
    if p != Permission::Unknown {
        PERMISSION.store(p as u8, Ordering::Relaxed);
    }
    p
}

fn cached_permission() -> Permission {
    Permission::from_u8(PERMISSION.load(Ordering::Relaxed))
}

// ── AppleScript ─────────────────────────────────────────────────────────────

/// One query for everything (see `super::parse_query`). Numbers are turned
/// into integers in AppleScript — otherwise the locale's decimal comma
/// leaks in (`246,908`). `if … is running` keeps it from launching Spotify.
pub const QUERY_SCRIPT: &str = r#"if application id "com.spotify.client" is running then
tell application id "com.spotify.client"
set vSep to character id 31
set vState to (player state as string)
set vName to ""
set vArtist to ""
set vAlbum to ""
set vUri to ""
set vDuration to 0
set vArtwork to ""
try
set vTrack to current track
set vName to name of vTrack
set vArtist to artist of vTrack
set vAlbum to album of vTrack
set vUri to id of vTrack
set vDuration to (duration of vTrack) as integer
end try
try
set vArtwork to artwork url of current track
end try
set vPosition to 0
try
set vPosition to (player position * 1000) as integer
end try
set vVolume to sound volume
return vState & vSep & vName & vSep & vArtist & vSep & vAlbum & vSep & vUri & vSep & vDuration & vSep & vPosition & vSep & vVolume & vSep & vArtwork
end tell
else
return "not_running"
end if"#;

fn classify_osa_error(stderr: &str) -> String {
    if stderr.contains("-1743") || stderr.contains("not allowed") || stderr.contains("Not authorized") {
        PERMISSION.store(Permission::Denied as u8, Ordering::Relaxed);
        ERR_DENIED.into()
    } else if stderr.contains("-600") || stderr.contains("isn’t running") || stderr.contains("isn't running") {
        ERR_NOT_RUNNING.into()
    } else {
        let line = stderr.lines().last().unwrap_or("").trim();
        format!("Spotify antwortet nicht: {line}")
    }
}

fn osa(script: &str) -> Result<String, String> {
    match run_osascript(script, OSA_TIMEOUT) {
        OsaResult::Done(out) if out.status.success() => {
            PERMISSION.store(Permission::Granted as u8, Ordering::Relaxed);
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        }
        OsaResult::Done(out) => Err(classify_osa_error(&String::from_utf8_lossy(&out.stderr))),
        OsaResult::TimedOut => Err("Spotify antwortet nicht (Zeitüberschreitung)".into()),
        OsaResult::SpawnFailed(e) => Err(format!("osascript: {e}")),
    }
}

/// Query the player via AppleScript. May show the macOS permission prompt.
pub fn query() -> Result<(Observation, Option<u8>), String> {
    if !installed() {
        return Err(ERR_NOT_INSTALLED.into());
    }
    let answer = osa(QUERY_SCRIPT)?;
    let parsed = super::parse_query(&answer, now_ms()).ok_or_else(|| "Unerwartete Antwort von Spotify".to_string())?;
    if let Some(v) = parsed.1 {
        SPOTIFY_VOLUME.store(v, Ordering::Relaxed);
    }
    Ok(parsed)
}

/// Can the user hear Spotify right now? System output (CoreAudio, no
/// process spawn; boom's bridge devices included) plus Spotify's last known
/// own volume. Unknown readings count as audible (`super::is_audible`).
fn audible_now() -> bool {
    use crate::system_commands::ca_volume;
    let muted = ca_volume::read_mute().map(|m| m || crate::boom::output_muted());
    let spotify = match SPOTIFY_VOLUME.load(Ordering::Relaxed) {
        NO_VOLUME => None,
        v => Some(v),
    };
    super::is_audible(muted, ca_volume::read_volume(), spotify)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Volume,
    Seek,
}

/// The AppleScript for a control command. Pure, so the exact text is tested.
pub fn control_script(action: Control, value: Option<f64>) -> Result<String, String> {
    let tell = |cmd: &str| format!("tell application id \"{}\" to {cmd}", super::BUNDLE_ID);
    Ok(match action {
        Control::Play => tell("play"),
        Control::Pause => tell("pause"),
        Control::Toggle => tell("playpause"),
        Control::Next => tell("next track"),
        Control::Previous => tell("previous track"),
        Control::Volume => {
            let v = value.filter(|v| v.is_finite()).ok_or("Lautstärke 0–100 angeben")?;
            tell(&format!("set sound volume to {}", v.clamp(0.0, 100.0).round() as i64))
        }
        Control::Seek => {
            let s = value.filter(|v| v.is_finite() && *v >= 0.0).ok_or("Position in Sekunden angeben")?;
            tell(&format!("set player position to {}", s.round() as i64))
        }
    })
}

/// Run a control command. `play` may start Spotify; everything else needs it
/// running. Afterwards the recorder reconciles.
pub fn control(action: Control, value: Option<f64>) -> Result<(), String> {
    if !installed() {
        return Err(ERR_NOT_INSTALLED.into());
    }
    if action != Control::Play && !running() {
        return Err(ERR_NOT_RUNNING.into());
    }
    osa(&control_script(action, value)?)?;
    send(Msg::Launched);
    Ok(())
}

// ── Now ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct NowTrack {
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<u64>,
    pub artwork_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Now {
    pub installed: bool,
    pub running: bool,
    pub permission: Permission,
    pub state: PlayerState,
    pub track: Option<NowTrack>,
    pub position_ms: Option<u64>,
    pub volume: Option<u8>,
    /// When `position_ms` was read (the panel extrapolates from here).
    pub at_ms: i64,
    /// `applescript` = live answer; `recorder` = last notification only.
    pub source: &'static str,
    /// Played time of the current history entry.
    pub played_ms: Option<u64>,
    pub history_enabled: bool,
    pub artwork_enabled: bool,
    pub error: Option<String>,
}

fn now_track(t: &super::Track) -> NowTrack {
    NowTrack {
        uri: t.uri.clone(),
        name: t.name.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        duration_ms: t.duration_ms,
        artwork_url: t.artwork_url.clone(),
    }
}

/// The current player state. With `ask`, AppleScript is used even if the
/// permission was never asked (the user ran the command — the prompt is
/// expected). Without, only when already granted; otherwise the recorder's
/// last notification answers.
pub fn now(ask: bool) -> Now {
    let installed = installed();
    let running = installed && running();
    let permission = if running { permission() } else { cached_permission() };
    let last = LAST.lock().ok().and_then(|g| g.clone());
    let mut out = Now {
        installed,
        running,
        permission,
        state: if running { PlayerState::Stopped } else { PlayerState::NotRunning },
        track: None,
        position_ms: None,
        volume: None,
        at_ms: now_ms(),
        source: "recorder",
        played_ms: last.as_ref().map(|p| p.played_ms),
        history_enabled: HISTORY_ON.load(Ordering::Relaxed),
        artwork_enabled: ARTWORK_ON.load(Ordering::Relaxed),
        error: None,
    };
    if !running {
        return out;
    }
    let use_script = permission == Permission::Granted || (ask && permission != Permission::Denied);
    if use_script {
        match query() {
            Ok((obs, volume)) => {
                out.state = obs.state;
                out.track = obs.track.as_ref().map(now_track);
                out.position_ms = obs.position_ms;
                out.volume = volume;
                out.at_ms = obs.at_ms;
                out.source = "applescript";
                out.permission = cached_permission();
                send(Msg::Obs(obs));
                return out;
            }
            Err(e) => {
                out.permission = cached_permission();
                out.error = Some(e);
            }
        }
    }
    if let Some(p) = last {
        out.state = p.state;
        out.track = Some(now_track(&p.track));
        out.position_ms = p.position_ms;
        out.at_ms = p.last_at;
    }
    out
}

// ── Settings ────────────────────────────────────────────────────────────────

static ARTWORK_ON: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize)]
pub struct SpotifyConfig {
    /// Record the play history (default on).
    pub history: bool,
    /// Show covers (loaded from Spotify's CDN; default off).
    pub artwork: bool,
}

pub fn load_config(db: &DbHandle) -> SpotifyConfig {
    let get = |k: &str, default: bool| {
        crate::settings::get(db, k)
            .ok()
            .flatten()
            .map(|v| v == "1" || v == "true")
            .unwrap_or(default)
    };
    SpotifyConfig {
        history: get(SETTING_HISTORY, true),
        artwork: get(SETTING_ARTWORK, false),
    }
}

pub fn save_config(db: &DbHandle, cfg: SpotifyConfig) -> Result<(), String> {
    let b = |v: bool| if v { "1" } else { "0" };
    crate::settings::set(db, SETTING_HISTORY, b(cfg.history)).map_err(|e| e.to_string())?;
    crate::settings::set(db, SETTING_ARTWORK, b(cfg.artwork)).map_err(|e| e.to_string())?;
    HISTORY_ON.store(cfg.history, Ordering::Relaxed);
    ARTWORK_ON.store(cfg.artwork, Ordering::Relaxed);
    send(Msg::Reconfigure);
    Ok(())
}

// ── Notifications ───────────────────────────────────────────────────────────

fn send(msg: Msg) {
    if let Some(tx) = SENDER.get() {
        if let Ok(tx) = tx.lock() {
            let _ = tx.send(msg);
        }
    }
}

/// Read one `userInfo` value as text or number.
unsafe fn field(info: *mut AnyObject, key: &str) -> Option<Field> {
    let k = NSString::from_str(key);
    let v: *mut AnyObject = msg_send![info, objectForKey: &*k];
    if v.is_null() {
        return None;
    }
    let string_cls = AnyClass::get(c"NSString")?;
    let number_cls = AnyClass::get(c"NSNumber")?;
    let is_string: bool = msg_send![v, isKindOfClass: string_cls];
    if is_string {
        let p: *const c_char = msg_send![v, UTF8String];
        return (!p.is_null()).then(|| Field::Text(CStr::from_ptr(p).to_string_lossy().into_owned()));
    }
    let is_number: bool = msg_send![v, isKindOfClass: number_cls];
    if is_number {
        let n: f64 = msg_send![v, doubleValue];
        return Some(Field::Number(n));
    }
    None
}

/// Bundle id of the app in an NSWorkspace launch/terminate notification.
unsafe fn workspace_bundle(note: NonNull<AnyObject>) -> Option<String> {
    let info: *mut AnyObject = msg_send![note.as_ptr(), userInfo];
    if info.is_null() {
        return None;
    }
    let key = NSString::from_str("NSWorkspaceApplicationKey");
    let app: *mut AnyObject = msg_send![info, objectForKey: &*key];
    if app.is_null() {
        return None;
    }
    let id: *mut AnyObject = msg_send![app, bundleIdentifier];
    if id.is_null() {
        return None;
    }
    let p: *const c_char = msg_send![id, UTF8String];
    (!p.is_null()).then(|| CStr::from_ptr(p).to_string_lossy().into_owned())
}

/// Register the observers. Call once from the main thread (app setup); the
/// blocks only turn the notification into a message for the recorder.
fn install_observers() {
    unsafe {
        let observe = |center: *mut AnyObject, name: &str, f: Box<dyn Fn(NonNull<AnyObject>) + 'static>| {
            if center.is_null() {
                return;
            }
            let n = NSString::from_str(name);
            let block = RcBlock::new(move |note: NonNull<AnyObject>| f(note));
            let token: *mut AnyObject = msg_send![
                center,
                addObserverForName: &*n,
                object: std::ptr::null::<AnyObject>(),
                queue: std::ptr::null::<AnyObject>(),
                usingBlock: &*block
            ];
            if !token.is_null() {
                let _: *mut AnyObject = msg_send![token, retain];
            }
        };
        let dist: *mut AnyObject = AnyClass::get(c"NSDistributedNotificationCenter")
            .map(|c| msg_send![c, defaultCenter])
            .unwrap_or(std::ptr::null_mut());
        observe(
            dist,
            "com.spotify.client.PlaybackStateChanged",
            Box::new(|note| {
                let info: *mut AnyObject = msg_send![note.as_ptr(), userInfo];
                if info.is_null() {
                    return;
                }
                let obs = super::observation_from_notification(|k| field(info, k), now_ms());
                send(Msg::Obs(obs));
            }),
        );
        let ws: *mut AnyObject = AnyClass::get(c"NSWorkspace")
            .map(|c| {
                let w: *mut AnyObject = msg_send![c, sharedWorkspace];
                msg_send![w, notificationCenter]
            })
            .unwrap_or(std::ptr::null_mut());
        observe(
            ws,
            "NSWorkspaceDidTerminateApplicationNotification",
            Box::new(|note| {
                if workspace_bundle(note).as_deref() == Some(super::BUNDLE_ID) {
                    send(Msg::Obs(Observation {
                        at_ms: now_ms(),
                        state: PlayerState::NotRunning,
                        track: None,
                        position_ms: None,
                    }));
                }
            }),
        );
        observe(
            ws,
            "NSWorkspaceDidLaunchApplicationNotification",
            Box::new(|note| {
                if workspace_bundle(note).as_deref() == Some(super::BUNDLE_ID) {
                    send(Msg::Launched);
                }
            }),
        );
    }
}

// ── Recorder ────────────────────────────────────────────────────────────────

fn persist(app: &AppHandle, db: &DbHandle, tracker: &mut Tracker, changes: Vec<Change>) -> bool {
    let mut changed = false;
    for c in changes {
        let finished = matches!(c, Change::Finish(_));
        let res = {
            let conn = db.lock();
            store::apply(&conn, &c)
        };
        match res {
            Ok(Some(id)) => {
                let new_row = tracker.current().is_some_and(|p| p.row_id.is_none());
                tracker.set_row_id(id);
                changed |= new_row;
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("spotify history: {e}"),
        }
        changed |= finished;
    }
    if let Ok(mut last) = LAST.lock() {
        *last = tracker.current().cloned();
    }
    if changed {
        let _ = app.emit(CHANGED_EVENT, ());
    }
    changed
}

/// Start the recorder. Call once from app setup (main thread).
pub fn start(app: &AppHandle, db: &DbHandle) {
    let cfg = load_config(db);
    HISTORY_ON.store(cfg.history, Ordering::Relaxed);
    ARTWORK_ON.store(cfg.artwork, Ordering::Relaxed);
    let (tx, rx) = channel::<Msg>();
    if SENDER.set(Mutex::new(tx)).is_err() {
        return;
    }
    install_observers();
    let (app, db) = (app.clone(), db.clone());
    let spawned = std::thread::Builder::new().name("ir-spotify".into()).spawn(move || {
        let open = {
            let conn = db.lock();
            store::take_open(&conn).unwrap_or_else(|e| {
                tracing::warn!("spotify history: {e}");
                None
            })
        };
        let mut tracker = Tracker::with_open(open);
        let mut ticks: u32 = 0;
        let mut reconcile = true;
        loop {
            let msg = rx.recv_timeout(TICK);
            let enabled = HISTORY_ON.load(Ordering::Relaxed);
            if enabled {
                // Silent playback is not listening (Mac muted / at 0, or
                // Spotify at 0). Read before the observation/tick accounts time.
                tracker.set_audible(now_ms(), audible_now());
            }
            let changes = match msg {
                Ok(Msg::Obs(o)) => {
                    if !enabled {
                        continue;
                    }
                    let state_or_track_changed = tracker.current().map(|p| (p.track.key(), p.state))
                        != o.track.as_ref().map(|t| (t.key(), o.state));
                    let new_track = o.track.as_ref().is_some_and(|t| {
                        tracker.current().is_none_or(|p| p.track.key() != t.key())
                    });
                    reconcile |= new_track && o.track.as_ref().is_some_and(|t| t.artwork_url.is_none());
                    let c = tracker.observe(&o);
                    if state_or_track_changed {
                        let _ = app.emit(CHANGED_EVENT, ());
                    }
                    c
                }
                Ok(Msg::Launched) => {
                    reconcile = true;
                    Vec::new()
                }
                Ok(Msg::Reconfigure) => {
                    if enabled {
                        reconcile = true;
                        Vec::new()
                    } else {
                        // Recording turned off: close what's open.
                        tracker.observe(&Observation {
                            at_ms: now_ms(),
                            state: PlayerState::Stopped,
                            track: None,
                            position_ms: None,
                        })
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    ticks = ticks.wrapping_add(1);
                    if !enabled {
                        continue;
                    }
                    let now = now_ms();
                    let is_running = running();
                    if !is_running && tracker.current().is_some() {
                        tracker.observe(&Observation {
                            at_ms: now,
                            state: PlayerState::NotRunning,
                            track: None,
                            position_ms: None,
                        })
                    } else {
                        reconcile |= is_running && ticks.is_multiple_of(POLL_EVERY_TICKS);
                        tracker.tick(now)
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            };
            persist(&app, &db, &mut tracker, changes);
            if reconcile && enabled {
                reconcile = false;
                // Never prompt from the background: only with a granted
                // permission (checked without asking).
                if running() && permission() == Permission::Granted {
                    if let Ok((obs, _)) = query() {
                        // The query just refreshed Spotify's own volume.
                        tracker.set_audible(obs.at_ms, audible_now());
                        let c = tracker.observe(&obs);
                        persist(&app, &db, &mut tracker, c);
                    }
                }
            }
        }
    });
    if let Err(e) = spawned {
        tracing::warn!("spotify recorder: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_scripts_are_exact_and_clamped() {
        assert_eq!(
            control_script(Control::Toggle, None).unwrap(),
            "tell application id \"com.spotify.client\" to playpause"
        );
        assert_eq!(
            control_script(Control::Previous, None).unwrap(),
            "tell application id \"com.spotify.client\" to previous track"
        );
        assert!(control_script(Control::Volume, Some(250.0)).unwrap().ends_with("set sound volume to 100"));
        assert!(control_script(Control::Volume, Some(-3.0)).unwrap().ends_with("set sound volume to 0"));
        assert!(control_script(Control::Seek, Some(61.6)).unwrap().ends_with("set player position to 62"));
        assert!(control_script(Control::Volume, None).is_err());
        assert!(control_script(Control::Seek, Some(-1.0)).is_err());
        assert!(control_script(Control::Seek, Some(f64::NAN)).is_err());
    }

    #[test]
    fn permission_status_codes() {
        assert_eq!(Permission::from_status(0), Permission::Granted);
        assert_eq!(Permission::from_status(-1743), Permission::Denied);
        assert_eq!(Permission::from_status(-1744), Permission::Undetermined);
        assert_eq!(Permission::from_status(-600), Permission::Unknown);
    }

    #[test]
    fn osascript_errors_become_readable() {
        assert_eq!(classify_osa_error("execution error: Not authorized to send Apple events to Spotify. (-1743)"), ERR_DENIED);
        assert_eq!(classify_osa_error("Spotify got an error: Application isn’t running. (-600)"), ERR_NOT_RUNNING);
        assert!(classify_osa_error("weird\nlast line").ends_with("last line"));
    }

    /// Live: reads the real player (no control). Run with
    /// `cargo test -p inspector-rust-core --lib spotify_live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn spotify_live_query() {
        println!("installed={} running={} permission={:?}", installed(), running(), permission());
        if running() {
            println!("{:?}", query());
        }
    }
}
