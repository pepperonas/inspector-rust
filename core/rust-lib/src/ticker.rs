//! The ticker overlay window (filled in by the ticker task).

use crate::meeting::TickerConfig;
use tauri::AppHandle;

pub fn apply(_app: &AppHandle, _cfg: &TickerConfig, _running: bool) {}
