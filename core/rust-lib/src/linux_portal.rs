//! xdg-desktop-portal integration for GNOME/Cinnamon on Wayland.
//!
//! `grim` + `slurp` work on wlroots compositors (Sway, Hyprland) but **slurp
//! hangs with no UI on GNOME** — region capture must go through the portal
//! (same pipeline as Ubuntu's built-in screenshot tool).

use anyhow::{anyhow, Context, Result};
use ashpd::desktop::ResponseError;
use ashpd::PortalError;
use std::path::PathBuf;

pub const ERR_PORTAL_CANCELLED: &str = "portal_cancelled";

/// GNOME/Cinnamon Wayland sessions should use the portal, not slurp.
pub fn prefer_portal_capture() -> bool {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return false;
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_uppercase();
    desktop.contains("GNOME") || desktop.contains("CINNAMON") || desktop.contains("UBUNTU")
}

fn uri_to_path(uri: &url::Url) -> Result<PathBuf> {
    if uri.scheme() == "file" {
        return uri
            .to_file_path()
            .map_err(|_| anyhow!("portal returned non-local file URI: {uri}"));
    }
    Err(anyhow!("portal returned unsupported URI scheme: {uri}"))
}

fn portal_cancelled(err: &ashpd::Error) -> bool {
    matches!(
        err,
        ashpd::Error::Response(ResponseError::Cancelled)
            | ashpd::Error::Portal(PortalError::Cancelled(_))
    )
}

/// Interactive region/window/screen capture via org.freedesktop.portal.Screenshot.
pub fn capture_region() -> Result<Vec<u8>> {
    use ashpd::desktop::screenshot::Screenshot;

    let response = pollster::block_on(async {
        Screenshot::request()
            .interactive(true)
            .modal(true)
            .send()
            .await?
            .response()
    })
    .map_err(|e| {
        if portal_cancelled(&e) {
            anyhow!(ERR_PORTAL_CANCELLED)
        } else {
            anyhow!("portal screenshot: {e}")
        }
    })?;

    let path = uri_to_path(response.uri())?;
    let bytes = std::fs::read(&path)
        .with_context(|| format!("read portal screenshot {}", path.display()))?;
    let _ = std::fs::remove_file(&path);
    if bytes.is_empty() {
        return Err(anyhow!(ERR_PORTAL_CANCELLED));
    }
    Ok(bytes)
}

/// System eyedropper via org.freedesktop.portal.Screenshot.PickColor.
pub fn pick_color() -> Result<Option<String>> {
    use ashpd::desktop::Color;

    match pollster::block_on(async { Color::pick().send().await?.response() }) {
        Ok(c) => {
            let hex = format!(
                "#{:02x}{:02x}{:02x}",
                c.red().round() as u8,
                c.green().round() as u8,
                c.blue().round() as u8,
            );
            Ok(Some(hex))
        }
        Err(e) if portal_cancelled(&e) => Ok(None),
        Err(e) => Err(anyhow!("portal pick color: {e}")),
    }
}

pub fn is_portal_cancelled(err: &anyhow::Error) -> bool {
    err.to_string() == ERR_PORTAL_CANCELLED
}
