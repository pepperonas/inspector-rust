#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    inspector_rust_core::run(tauri::generate_context!());
}
