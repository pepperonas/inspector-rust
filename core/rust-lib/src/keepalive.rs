//! Keep-alive: make sure Inspector Rust is **always running** — started at
//! login and **auto-relaunched** if it ever isn't (crash / quit / kill).
//!
//! It registers a **native OS supervisor** (no fragile watchdog process of our
//! own that could itself die), using a **periodic relaunch (poll)** model rather
//! than `KeepAlive`/`Restart=always`: the supervisor simply launches the exe
//! every ~30–60 s. Because a second launch routes through
//! `tauri-plugin-single-instance` with no args (a no-op — see
//! `cli_dispatch::parse_args`), relaunching while the app is alive does nothing;
//! if it's dead, the launch becomes the new live instance. This **cannot
//! respawn-loop** the way an event-driven `KeepAlive` would with single-instance
//! (which would exit-on-dup → get respawned → exit → …).
//!
//! - **macOS:** a LaunchAgent plist with `RunAtLoad` + `StartInterval=30`.
//! - **Linux:** a systemd `--user` `.timer` (every 60 s) + a `Type=oneshot`
//!   `.service` that launches the app detached (`KillMode=none` so it survives
//!   the unit going inactive). Needs a systemd user session.
//! - **Windows:** a Scheduled Task (`schtasks /SC MINUTE /MO 1`).
//!
//! Opt-in (settings key `keepalive.enabled`, surfaced in Settings → Startup).
//! **Note:** while on, the app relaunches even after you Quit — turn it off
//! first to quit for good.

#![allow(dead_code)] // platform-specific helpers are each only used under one cfg

const LABEL: &str = "io.celox.inspector-rust.keepalive";

/// Path used to launch the app — the currently-running executable.
fn exe_path() -> Result<String, String> {
    std::env::current_exe()
        .map_err(|e| format!("current_exe: {e}"))?
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "executable path is not valid UTF-8".to_string())
}

/// Is the keep-alive supervisor currently installed?
pub fn is_enabled() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::plist_path().map(|p| p.exists()).unwrap_or(false)
    }
    #[cfg(target_os = "linux")]
    {
        linux::timer_path().map(|p| p.exists()).unwrap_or(false)
    }
    #[cfg(target_os = "windows")]
    {
        windows::task_exists()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        false
    }
}

/// Install (or remove) the OS supervisor to match `enabled`.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let exe = exe_path()?;
    #[cfg(target_os = "macos")]
    {
        if enabled {
            macos::install(&exe)
        } else {
            macos::remove()
        }
    }
    #[cfg(target_os = "linux")]
    {
        if enabled {
            linux::install(&exe)
        } else {
            linux::remove()
        }
    }
    #[cfg(target_os = "windows")]
    {
        if enabled {
            windows::install(&exe)
        } else {
            windows::remove()
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (enabled, exe);
        Err("keep-alive not supported on this platform".to_string())
    }
}

// ── Pure content builders (unit-tested) ──────────────────────────────────────

/// Minimal XML escaping for a path embedded in the plist.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// LaunchAgent plist: run at login + every `interval_secs` (poll). No
/// `KeepAlive` (that would respawn-loop against single-instance).
fn macos_plist(exe: &str, interval_secs: u32) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>StartInterval</key>
    <integer>{interval}</integer>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
        label = LABEL,
        exe = xml_escape(exe),
        interval = interval_secs,
    )
}

/// systemd oneshot launcher service. `KillMode=none` keeps the detached app
/// alive after the (oneshot) unit goes inactive.
fn systemd_service(exe: &str) -> String {
    format!(
        "[Unit]\n\
         Description=Inspector Rust keep-alive (relaunch if not running)\n\n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart=/bin/sh -c '\"{exe}\" >/dev/null 2>&1 &'\n\
         KillMode=none\n"
    )
}

/// systemd timer: fire shortly after login, then every 60 s.
fn systemd_timer() -> String {
    "[Unit]\n\
     Description=Inspector Rust keep-alive timer\n\n\
     [Timer]\n\
     OnStartupSec=20\n\
     OnUnitActiveSec=60\n\
     Persistent=true\n\n\
     [Install]\n\
     WantedBy=timers.target\n"
        .to_string()
}

// ── macOS ────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos {
    use super::{macos_plist, LABEL};
    use std::path::PathBuf;

    const INTERVAL_SECS: u32 = 30;

    pub(super) fn plist_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
    }

    pub(super) fn install(exe: &str) -> Result<(), String> {
        let path = plist_path().ok_or("no home dir")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("mkdir LaunchAgents: {e}"))?;
        }
        std::fs::write(&path, macos_plist(exe, INTERVAL_SECS))
            .map_err(|e| format!("write plist: {e}"))?;
        // Reload: unload (ignore errors) then load -w so launchd picks up changes.
        let _ = launchctl(&["unload", "-w", &path.to_string_lossy()]);
        launchctl(&["load", "-w", &path.to_string_lossy()])
    }

    pub(super) fn remove() -> Result<(), String> {
        let path = plist_path().ok_or("no home dir")?;
        if path.exists() {
            let _ = launchctl(&["unload", "-w", &path.to_string_lossy()]);
            std::fs::remove_file(&path).map_err(|e| format!("remove plist: {e}"))?;
        }
        Ok(())
    }

    fn launchctl(args: &[&str]) -> Result<(), String> {
        let out = std::process::Command::new("/bin/launchctl")
            .args(args)
            .output()
            .map_err(|e| format!("launchctl: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "launchctl {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
}

// ── Linux ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    use super::{systemd_service, systemd_timer};
    use std::path::PathBuf;

    const UNIT: &str = "inspector-rust-keepalive";

    fn unit_dir() -> Option<PathBuf> {
        dirs::config_dir().map(|c| c.join("systemd/user"))
    }
    pub(super) fn timer_path() -> Option<PathBuf> {
        unit_dir().map(|d| d.join(format!("{UNIT}.timer")))
    }
    fn service_path() -> Option<PathBuf> {
        unit_dir().map(|d| d.join(format!("{UNIT}.service")))
    }

    pub(super) fn install(exe: &str) -> Result<(), String> {
        let dir = unit_dir().ok_or("no config dir")?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir systemd/user: {e}"))?;
        std::fs::write(dir.join(format!("{UNIT}.service")), systemd_service(exe))
            .map_err(|e| format!("write service: {e}"))?;
        std::fs::write(dir.join(format!("{UNIT}.timer")), systemd_timer())
            .map_err(|e| format!("write timer: {e}"))?;
        systemctl(&["daemon-reload"])?;
        systemctl(&["enable", "--now", &format!("{UNIT}.timer")])
    }

    pub(super) fn remove() -> Result<(), String> {
        let _ = systemctl(&["disable", "--now", &format!("{UNIT}.timer")]);
        if let Some(p) = timer_path() {
            let _ = std::fs::remove_file(p);
        }
        if let Some(p) = service_path() {
            let _ = std::fs::remove_file(p);
        }
        let _ = systemctl(&["daemon-reload"]);
        Ok(())
    }

    fn systemctl(args: &[&str]) -> Result<(), String> {
        let out = std::process::Command::new("systemctl")
            .arg("--user")
            .args(args)
            .output()
            .map_err(|e| format!("systemctl: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "systemctl --user {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
}

// ── Windows ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod windows {
    const TASK: &str = "InspectorRustKeepAlive";

    pub(super) fn install(exe: &str) -> Result<(), String> {
        // Run the exe every minute; single-instance dedups when alive,
        // relaunches when dead. Quote the path for the /TR argument.
        schtasks(&[
            "/Create",
            "/TN",
            TASK,
            "/TR",
            &format!("\"{exe}\""),
            "/SC",
            "MINUTE",
            "/MO",
            "1",
            "/F",
        ])
    }

    pub(super) fn remove() -> Result<(), String> {
        schtasks(&["/Delete", "/TN", TASK, "/F"])
    }

    pub(super) fn task_exists() -> bool {
        std::process::Command::new("schtasks")
            .args(["/Query", "/TN", TASK])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn schtasks(args: &[&str]) -> Result<(), String> {
        let out = std::process::Command::new("schtasks")
            .args(args)
            .output()
            .map_err(|e| format!("schtasks: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "schtasks {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_has_runatload_interval_no_keepalive() {
        let p = macos_plist("/Applications/InspectorRust.app/Contents/MacOS/inspector-rust", 30);
        assert!(p.contains("<key>RunAtLoad</key>"));
        assert!(p.contains("<key>StartInterval</key>"));
        assert!(p.contains("<integer>30</integer>"));
        assert!(p.contains(LABEL));
        assert!(p.contains("/Contents/MacOS/inspector-rust"));
        // Crucially NOT KeepAlive — that would respawn-loop vs single-instance.
        assert!(!p.contains("KeepAlive"));
    }

    #[test]
    fn plist_xml_escapes_the_path() {
        let p = macos_plist("/tmp/a&b/app", 30);
        assert!(p.contains("/tmp/a&amp;b/app"));
        assert!(!p.contains("a&b/app"));
    }

    #[test]
    fn systemd_service_oneshot_killmode_none() {
        let s = systemd_service("/usr/bin/inspector-rust");
        assert!(s.contains("Type=oneshot"));
        assert!(s.contains("KillMode=none")); // detached app survives the oneshot
        assert!(s.contains("/usr/bin/inspector-rust"));
        // Not a Restart=always service (would loop vs single-instance).
        assert!(!s.contains("Restart=always"));
    }

    #[test]
    fn systemd_timer_is_periodic_and_installable() {
        let t = systemd_timer();
        assert!(t.contains("OnUnitActiveSec=60"));
        assert!(t.contains("WantedBy=timers.target"));
    }
}
