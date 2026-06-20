//! Ship the browser extension *from* the app. Chrome (and every Chromium
//! browser) deliberately blocks programmatic installation of unpacked
//! extensions — there is no API an app can call to "install this for me". So the
//! realistic, supported flow is: write the extension files to a folder the user
//! can reach, then they load it unpacked (chrome://extensions → Developer mode →
//! Load unpacked). This module embeds the extension at compile time
//! (`include_str!`) and writes it out on demand.

use std::path::PathBuf;

// Embedded at build time from the repo's `extension/` folder (4 levels up from
// this file: tracking/ → src/ → rust-lib/ → core/ → repo root).
const MANIFEST: &str = include_str!("../../../../extension/manifest.json");
const BACKGROUND: &str = include_str!("../../../../extension/background.js");
const OPTIONS_HTML: &str = include_str!("../../../../extension/options.html");
const OPTIONS_JS: &str = include_str!("../../../../extension/options.js");
const README: &str = include_str!("../../../../extension/README.md");

const FILES: [(&str, &str); 5] = [
    ("manifest.json", MANIFEST),
    ("background.js", BACKGROUND),
    ("options.html", OPTIONS_HTML),
    ("options.js", OPTIONS_JS),
    ("README.md", README),
];

/// Write the extension to `~/Downloads/inspector-rust-timesheet-extension/` and
/// return that folder path (so the caller can reveal it).
pub fn write_to_downloads() -> Result<PathBuf, String> {
    let dir = dirs::download_dir()
        .ok_or_else(|| "no Downloads folder".to_string())?
        .join("inspector-rust-timesheet-extension");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create extension dir: {e}"))?;
    for (name, content) in FILES {
        std::fs::write(dir.join(name), content).map_err(|e| format!("write {name}: {e}"))?;
    }
    Ok(dir)
}
