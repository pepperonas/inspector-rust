//! boom on Windows — Equalizer APO backend.
//!
//! No virtual driver (Windows audio drivers are kernel-mode + WHQL-signed, not
//! ad-hoc shippable): boom drives **Equalizer APO** instead. `apply_config`
//! renders the saved [`BoomConfig`](super::BoomConfig) into
//! `<EqAPO config dir>\inspector-rust-boom.txt` and (once) appends the
//! `Include:` line to `config.txt`; EqAPO watches its config files and
//! hot-reloads on write, so changes are live immediately. EqAPO's installer
//! grants Users write access to the config directory, so no elevation is
//! needed. Disabling boom rewrites the include file as a passthrough comment —
//! the user's own config.txt content is never touched beyond the one Include
//! line. Runtime-unverified (compile-validated; no Windows box here).

use std::path::PathBuf;

/// EqAPO's config directory: registry `HKLM\SOFTWARE\EqualizerAPO@ConfigPath`
/// (written by its installer), with a `%ProgramFiles%\EqualizerAPO\config`
/// existence-check fallback.
pub(crate) fn config_dir() -> Option<PathBuf> {
    if let Some(p) = registry_config_path() {
        let pb = PathBuf::from(p);
        if pb.is_dir() {
            return Some(pb);
        }
    }
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let fallback = PathBuf::from(pf).join("EqualizerAPO").join("config");
    fallback.is_dir().then_some(fallback)
}

fn registry_config_path() -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY,
    };

    let subkey: Vec<u16> = "SOFTWARE\\EqualizerAPO\0".encode_utf16().collect();
    let value: Vec<u16> = "ConfigPath\0".encode_utf16().collect();
    let mut buf = [0u16; 512];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            None,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    if rc.is_err() {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).trim_end_matches('\0').to_string())
}

/// Whether Equalizer APO is installed (config directory found).
pub(crate) fn available() -> bool {
    config_dir().is_some()
}

/// Render + write the include file for `cfg` and make sure `config.txt` pulls
/// it in. EqAPO hot-reloads on write.
pub(crate) fn apply_config(cfg: &super::BoomConfig) -> Result<(), String> {
    let dir = config_dir().ok_or_else(|| "Equalizer APO not found".to_string())?;
    let include = dir.join(super::APO_INCLUDE_FILE);
    std::fs::write(&include, super::apo_config_text(cfg))
        .map_err(|e| format!("write {}: {e}", include.display()))?;

    let main = dir.join("config.txt");
    let existing = std::fs::read_to_string(&main).unwrap_or_default();
    if let Some(updated) = super::apo_ensure_include(&existing) {
        std::fs::write(&main, updated).map_err(|e| format!("write {}: {e}", main.display()))?;
    }
    Ok(())
}
