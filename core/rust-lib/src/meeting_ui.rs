//! Tray indicator and ticker window for the live transcription (filled in
//! by the tray and ticker tasks).

use crate::meeting::MeetingState;
use tauri::AppHandle;

pub fn on_session_start(_app: &AppHandle, _state: &MeetingState) {}

pub fn on_session_end(_app: &AppHandle, _state: &MeetingState) {}

pub fn on_warning(_app: &AppHandle, _state: &MeetingState) {}

pub fn apply_config(_app: &AppHandle, _cfg: &crate::meeting::MeetingConfig) {}
