//! Linux desktop integration: register system shortcuts when Tauri global
//! shortcuts do not work (GNOME + Wayland). Uses gsettings on GNOME/Cinnamon.

#[cfg(target_os = "linux")]
mod imp {
    use anyhow::{Context, Result};
    use std::process::Command;

    use crate::{db::DbHandle, settings};

    const SETTINGS_KEY: &str = "linux.desktop_shortcuts_profile";

    const GNOME_MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
    const GNOME_CUSTOM_SCHEMA: &str =
        "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";

    const CINNAMON_KEYBINDINGS: &str = "org.cinnamon.desktop.keybindings";
    const CINNAMON_CUSTOM_SCHEMA: &str = "org.cinnamon.keybindings.custom-keybinding";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum LinuxDesktop {
        /// Built-in Tauri shortcuts usually work; no OS bindings needed.
        X11,
        /// GNOME on Wayland — install gsettings custom shortcuts.
        GnomeWayland,
        /// Cinnamon (Mint) on Wayland — cinnamon gsettings schema.
        CinnamonWayland,
        /// KDE — not automated yet.
        Kde,
        Unknown,
    }

    pub fn detect() -> LinuxDesktop {
        let session = std::env::var("XDG_SESSION_TYPE")
            .unwrap_or_default()
            .to_lowercase();
        let desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_lowercase();

        if session == "x11" {
            return LinuxDesktop::X11;
        }

        if desktop.contains("kde") || desktop.contains("plasma") {
            return LinuxDesktop::Kde;
        }

        if desktop.contains("cinnamon") || desktop.contains("x-cinnamon") {
            if session == "wayland" || std::env::var_os("WAYLAND_DISPLAY").is_some() {
                return LinuxDesktop::CinnamonWayland;
            }
            return LinuxDesktop::X11;
        }

        if desktop.contains("gnome")
            || desktop.contains("ubuntu")
            || desktop.contains("unity")
            || desktop.is_empty()
        {
            if session == "wayland" || std::env::var_os("WAYLAND_DISPLAY").is_some() {
                return LinuxDesktop::GnomeWayland;
            }
            return LinuxDesktop::X11;
        }

        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            return LinuxDesktop::Unknown;
        }

        LinuxDesktop::X11
    }

    fn inspector_command() -> String {
        if let Ok(p) = which_inspector_rust() {
            return p;
        }
        "/usr/bin/inspector-rust".into()
    }

    fn which_inspector_rust() -> Result<String> {
        let output = Command::new("which")
            .arg("inspector-rust")
            .output()
            .context("which inspector-rust")?;
        if !output.status.success() {
            anyhow::bail!("inspector-rust not in PATH");
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path.is_empty() {
            anyhow::bail!("empty path from which");
        }
        Ok(path)
    }

    fn gsettings(args: &[&str]) -> Result<String> {
        let output = Command::new("gsettings")
            .args(args)
            .output()
            .with_context(|| format!("gsettings {}", args.join(" ")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("gsettings failed: {stderr}");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn gsettings_set(args: &[&str]) -> Result<()> {
        let status = Command::new("gsettings")
            .args(args)
            .status()
            .with_context(|| format!("gsettings set {}", args[2..].join(" ")))?;
        if !status.success() {
            anyhow::bail!("gsettings set failed");
        }
        Ok(())
    }

    struct ShortcutSpec {
        id: &'static str,
        name: &'static str,
        arg: &'static str,
        binding: &'static str,
    }

    const SHORTCUTS: &[ShortcutSpec] = &[
        ShortcutSpec {
            id: "toggle",
            name: "Inspector Rust — Open",
            arg: "--toggle-popup",
            binding: "<Control><Shift>v",
        },
        ShortcutSpec {
            id: "ocr",
            name: "Inspector Rust — OCR",
            arg: "--ocr",
            binding: "<Control><Shift>o",
        },
        ShortcutSpec {
            id: "screenshot",
            name: "Inspector Rust — Screenshot",
            arg: "--screenshot",
            binding: "<Control><Shift>s",
        },
        ShortcutSpec {
            id: "color",
            name: "Inspector Rust — Pick color",
            arg: "--pick-color",
            binding: "<Control><Shift>c",
        },
    ];

    fn parse_gsettings_array(raw: &str) -> Vec<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed == "@as []" {
            return Vec::new();
        }
        let inner = trimmed.strip_prefix("@as ").unwrap_or(trimmed).trim();
        // Format: ['path1', 'path2'] — simple parse for our use case.
        inner
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .map(|s| s.trim().trim_matches('\'').to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    fn format_gsettings_array(paths: &[String]) -> String {
        if paths.is_empty() {
            return "@as []".into();
        }
        let inner: Vec<String> = paths.iter().map(|p| format!("'{p}'")).collect();
        format!("@as [{}]", inner.join(", "))
    }

    fn remove_our_paths(paths: &mut Vec<String>, prefix: &str) {
        paths.retain(|p| !p.contains(prefix));
    }

    fn install_gnome_family(
        list_key: &str,
        list_schema: &str,
        custom_schema: &str,
        path_prefix: &str,
        id_prefix: &str,
    ) -> Result<()> {
        let cmd = inspector_command();
        let mut paths = parse_gsettings_array(&gsettings(&["get", list_schema, list_key])?);
        remove_our_paths(&mut paths, id_prefix);

        for spec in SHORTCUTS {
            let path = format!("{path_prefix}{id_prefix}{}/", spec.id);
            if !paths.contains(&path) {
                paths.push(path.clone());
            }
            let schema = format!("{custom_schema}:{path}");
            gsettings_set(&["set", &schema, "name", spec.name])?;
            gsettings_set(&["set", &schema, "command", &format!("{cmd} {}", spec.arg)])?;
            gsettings_set(&["set", &schema, "binding", spec.binding])?;
        }

        gsettings_set(&[
            "set",
            list_schema,
            list_key,
            &format_gsettings_array(&paths),
        ])?;
        Ok(())
    }

    pub fn install_gnome() -> Result<()> {
        install_gnome_family(
            "custom-keybindings",
            GNOME_MEDIA_KEYS,
            GNOME_CUSTOM_SCHEMA,
            "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/",
            "inspector-rust-",
        )
    }

    pub fn install_cinnamon() -> Result<()> {
        install_gnome_family(
            "custom-list",
            CINNAMON_KEYBINDINGS,
            CINNAMON_CUSTOM_SCHEMA,
            "/org/cinnamon/desktop/keybindings/custom-keybindings/",
            "inspector-rust-",
        )
    }

    pub fn profile_id(desktop: LinuxDesktop) -> &'static str {
        match desktop {
            LinuxDesktop::GnomeWayland => "gnome-wayland-v1",
            LinuxDesktop::CinnamonWayland => "cinnamon-wayland-v1",
            LinuxDesktop::X11 => "x11-skip",
            LinuxDesktop::Kde => "kde-manual",
            LinuxDesktop::Unknown => "unknown-skip",
        }
    }

    pub fn try_auto_install(db: &DbHandle) -> Result<()> {
        let desktop = detect();
        let profile = profile_id(desktop);

        if let Ok(existing) = settings::get(db, SETTINGS_KEY) {
            if existing.as_deref() == Some(profile) {
                tracing::info!(
                    "Desktop shortcuts already configured for {profile} (Settings → Keyboard → Custom Shortcuts)"
                );
                return Ok(());
            }
        }

        match desktop {
            LinuxDesktop::GnomeWayland => {
                install_gnome().context("install GNOME custom shortcuts")?;
                tracing::info!(
                    "Installed GNOME custom shortcuts (Ctrl+Shift+V/O/S/C → inspector-rust CLI)"
                );
            }
            LinuxDesktop::CinnamonWayland => {
                install_cinnamon().context("install Cinnamon custom shortcuts")?;
                tracing::info!("Installed Cinnamon custom shortcuts for Inspector Rust");
            }
            LinuxDesktop::X11 => {
                tracing::info!("X11 session: using in-app global shortcuts (Ctrl+Shift+V/O/S/C)");
                settings::set(db, SETTINGS_KEY, profile)?;
            }
            LinuxDesktop::Kde => {
                tracing::warn!(
                    "KDE Plasma: automatic shortcuts not supported yet — use System Settings → \
                     Shortcuts, or bind to `inspector-rust --toggle-popup` etc."
                );
                return Ok(());
            }
            LinuxDesktop::Unknown => {
                tracing::warn!(
                    "Unknown Linux desktop (Wayland): tray menu and `inspector-rust --help` CLI work; \
                     configure shortcuts manually if needed"
                );
                return Ok(());
            }
        }

        settings::set(db, SETTINGS_KEY, profile)?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub use imp::*;

#[cfg(not(target_os = "linux"))]
#[derive(Debug, Clone, Copy)]
pub enum LinuxDesktop {
    Other,
}

#[cfg(not(target_os = "linux"))]
pub fn detect() -> LinuxDesktop {
    LinuxDesktop::Other
}

#[cfg(not(target_os = "linux"))]
pub fn try_auto_install(_db: &DbHandle) -> anyhow::Result<()> {
    Ok(())
}
