//! `nosleep` — toggle the PERSISTENT AC idle-sleep profile (v0.124.0, macOS).
//!
//! Unlike `wakelock dark` (a session-scoped `caffeinate` assertion), this
//! writes the stored power profile: `pmset -c sleep 0` makes the Mac never
//! idle-sleep on AC power, and it survives reboots until turned back off. That
//! needs admin rights, so the write goes through one `osascript … with
//! administrator privileges` prompt (no terminal sudo).
//!
//! ⚠️ `pmset sleep` is MINUTES-until-idle-sleep (0 = never), NOT a boolean.
//! Turning the lock OFF must restore a real timeout — so `nosleep on` remembers
//! the AC value it overwrote (in settings, done by the command wrapper) and
//! `off` restores it; the fallback when nothing was remembered is
//! [`DEFAULT_RESTORE_MIN`].
//!
//! House style: `parse_profile` is pure + unit-tested against real
//! `pmset -g custom` output; the `pmset`/`osascript` spawns are the thin impure
//! shell. macOS only — other platforms report `supported: false`.

use serde::Serialize;

/// Fallback AC sleep timeout (minutes) for `off` when no prior value was
/// remembered (e.g. the profile was already `sleep 0` before IR touched it).
pub const DEFAULT_RESTORE_MIN: i64 = 1;

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct NoSleepStatus {
    /// false on non-macOS or when `pmset` couldn't be read.
    pub supported: bool,
    /// AC idle-sleep minutes (0 = never); `None` if unparized.
    pub ac_sleep: Option<i64>,
    /// Battery idle-sleep minutes (for the readout).
    pub battery_sleep: Option<i64>,
    /// The lock is active: AC profile is `sleep 0`.
    pub ac_disabled: bool,
}

/// Parse `pmset -g custom` into `(ac_sleep, battery_sleep)` minutes. The output
/// has a `Battery Power:` and an `AC Power:` section, each with an indented
/// `sleep <N>` line. Pure + tested. The exact-token match on `sleep` keeps
/// `displaysleep`, `disksleep` and `Sleep On Power Button` out.
pub fn parse_profile(out: &str) -> (Option<i64>, Option<i64>) {
    #[derive(PartialEq)]
    enum Sec {
        None,
        Battery,
        Ac,
    }
    let mut sec = Sec::None;
    let (mut ac, mut battery) = (None, None);
    for line in out.lines() {
        let t = line.trim();
        if t.starts_with("Battery Power") {
            sec = Sec::Battery;
            continue;
        }
        if t.starts_with("AC Power") {
            sec = Sec::Ac;
            continue;
        }
        // The sleep line is exactly `sleep <N>` (lowercase token).
        let mut it = t.split_whitespace();
        if it.next() == Some("sleep") {
            if let Some(v) = it.next().and_then(|n| n.parse::<i64>().ok()) {
                match sec {
                    Sec::Battery => battery = Some(v),
                    Sec::Ac => ac = Some(v),
                    Sec::None => {}
                }
            }
        }
    }
    (ac, battery)
}

/// Read the current profile via `pmset -g custom`. macOS only.
#[cfg(target_os = "macos")]
pub fn status() -> NoSleepStatus {
    let out = std::process::Command::new("/usr/bin/pmset")
        .args(["-g", "custom"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let (ac, battery) = parse_profile(&String::from_utf8_lossy(&o.stdout));
            NoSleepStatus {
                supported: true,
                ac_sleep: ac,
                battery_sleep: battery,
                ac_disabled: ac == Some(0),
            }
        }
        _ => NoSleepStatus { supported: true, ..Default::default() },
    }
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> NoSleepStatus {
    NoSleepStatus::default()
}

/// Set the AC idle-sleep timeout (minutes; 0 = never) via an admin prompt.
/// One `osascript … with administrator privileges` dialog per call. macOS only.
#[cfg(target_os = "macos")]
pub fn set_ac_sleep(minutes: i64) -> Result<(), String> {
    let minutes = minutes.max(0);
    // pmset path is fixed; the value is an integer we produced — no injection
    // surface, but keep it a bare number regardless.
    let script = format!(
        "do shell script \"/usr/bin/pmset -c sleep {minutes}\" with administrator privileges"
    );
    let out = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("osascript: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        // User cancelled the admin dialog → a friendly message, not a raw code.
        if err.contains("-128") || err.to_lowercase().contains("cancel") {
            Err("Abgebrochen (kein Admin-Recht erteilt).".into())
        } else {
            Err(format!("pmset fehlgeschlagen: {}", err.trim()))
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_ac_sleep(_minutes: i64) -> Result<(), String> {
    Err("Nur auf macOS verfügbar.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real `pmset -g custom` output shape (captured this session).
    const REAL: &str = "Battery Power:\n\
 lowpowermode         0\n\
 standby              1\n\
 Sleep On Power Button 1\n\
 hibernatemode        3\n\
 displaysleep         5\n\
 sleep                1\n\
 disksleep            10\n\
AC Power:\n\
 lowpowermode         0\n\
 standby              1\n\
 Sleep On Power Button 1\n\
 hibernatemode        3\n\
 displaysleep         10\n\
 sleep                0\n\
 disksleep            10\n";

    #[test]
    fn parses_ac_and_battery_sleep_from_the_right_sections() {
        let (ac, battery) = parse_profile(REAL);
        assert_eq!(ac, Some(0)); // never on AC
        assert_eq!(battery, Some(1));
    }

    #[test]
    fn the_exact_sleep_token_ignores_displaysleep_disksleep_and_power_button() {
        // A section with ONLY the decoys must yield no sleep value.
        let decoys = "AC Power:\n displaysleep 10\n disksleep 20\n Sleep On Power Button 1\n";
        assert_eq!(parse_profile(decoys), (None, None));
    }

    #[test]
    fn handles_missing_sections_gracefully() {
        assert_eq!(parse_profile(""), (None, None));
        assert_eq!(parse_profile("AC Power:\n sleep 30\n"), (Some(30), None));
        assert_eq!(parse_profile("Battery Power:\n sleep 5\n"), (None, Some(5)));
    }

    #[test]
    fn a_line_before_any_section_is_ignored() {
        // Defensive: a stray `sleep 9` outside a section must not become ac/batt.
        assert_eq!(parse_profile(" sleep 9\nAC Power:\n sleep 0\n"), (Some(0), None));
    }
}
