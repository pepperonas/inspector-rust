//! `alias` — create a shell alias on THIS machine (v0.127.0).
//!
//! The AliasPanel shows the per-OS one-liners (built in the frontend,
//! `lib/alias.ts`); this module is the "Anlegen" button's backend: append the
//! alias to the current user's shell config. macOS/Linux write the rc file
//! directly (no subshell — quoting stays ours); Windows spawns PowerShell so
//! `$PROFILE` resolves to the real profile path (runtime-unverified, per house
//! convention). Duplicate protection: an existing `alias <name>=` line in the
//! target rc is refused with a clear message, never silently overwritten.
//!
//! House style: the deciders — name validation, the rc-file choice from
//! `$SHELL`, the alias line itself, duplicate detection — are pure + tested;
//! the file append / process spawn is the thin impure shell.

/// Shell-safe alias names (mirrors `lib/alias.ts::validAliasName` — the
/// cross-language pin lives in the tests).
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The rc line: `alias gs='git status'` — POSIX single-quoted with the
/// close-reopen `'\''` pattern (single quotes have NO backslash escaping).
pub fn alias_line(name: &str, command: &str) -> String {
    format!("alias {name}='{}'", command.replace('\'', "'\\''"))
}

/// Which rc file the alias belongs in, from `$SHELL`'s basename. Fish is
/// refused honestly (its alias syntax differs — writing a bash line into
/// fish's config would be silent breakage); anything unknown falls back to
/// the OS default shell (zsh on macOS since Catalina, bash on Linux).
pub fn rc_target(shell_env: Option<&str>, macos: bool) -> Result<&'static str, String> {
    let base = shell_env
        .map(|s| s.rsplit('/').next().unwrap_or(s))
        .unwrap_or("");
    if base.contains("fish") {
        return Err(
            "Fish wird nicht unterstützt — Alias manuell in ~/.config/fish/config.fish anlegen \
             (Syntax: alias name 'befehl')"
                .into(),
        );
    }
    Ok(match base {
        "zsh" => ".zshrc",
        "bash" => ".bashrc",
        _ if macos => ".zshrc",
        _ => ".bashrc",
    })
}

/// Whether the rc content already defines this alias (an exact `alias NAME=`
/// line start after trimming — commented-out lines don't count).
pub fn already_defined(rc_content: &str, name: &str) -> bool {
    let needle = format!("alias {name}=");
    rc_content.lines().any(|l| l.trim_start().starts_with(&needle))
}

/// Append the alias to the current user's shell config. Returns a human
/// success message naming the file + the reload hint.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn create(name: &str, command: &str) -> Result<String, String> {
    if !valid_name(name) {
        return Err("Ungültiger Alias-Name.".into());
    }
    if command.trim().is_empty() {
        return Err("Befehl fehlt.".into());
    }
    let rc = rc_target(
        std::env::var("SHELL").ok().as_deref(),
        cfg!(target_os = "macos"),
    )?;
    let home = dirs::home_dir().ok_or("Kein Home-Verzeichnis gefunden.")?;
    let path = home.join(rc);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if already_defined(&existing, name) {
        return Err(format!(
            "In ~/{rc} existiert bereits ein Alias „{name}“ — bitte dort ändern oder einen anderen Namen wählen."
        ));
    }
    // Append with a leading newline guard so we never glue onto a file that
    // doesn't end in one.
    let mut out = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&alias_line(name, command));
    out.push('\n');
    use std::io::Write as _;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(out.as_bytes()))
        .map_err(|e| format!("Konnte ~/{rc} nicht schreiben: {e}"))?;
    Ok(format!(
        "Alias „{name}“ in ~/{rc} angelegt — gilt im nächsten Terminal (oder: source ~/{rc})."
    ))
}

/// Windows: run the same PowerShell one-liner the panel displays, so
/// `$PROFILE` resolves inside a real PowerShell. Runtime-unverified.
#[cfg(target_os = "windows")]
pub fn create(name: &str, command: &str) -> Result<String, String> {
    if !valid_name(name) {
        return Err("Ungültiger Alias-Name.".into());
    }
    if command.trim().is_empty() {
        return Err("Befehl fehlt.".into());
    }
    let func = format!("function {name} {{ {command} $args }}");
    let ps = format!(
        "if (!(Test-Path $PROFILE)) {{ New-Item -ItemType File -Force $PROFILE | Out-Null }}; \
         if (Select-String -Path $PROFILE -Pattern ('^\\s*function\\s+' + [regex]::Escape('{name}') + '\\b') -Quiet) {{ exit 3 }}; \
         Add-Content $PROFILE '{}'",
        func.replace('\'', "''"),
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .output()
        .map_err(|e| format!("PowerShell: {e}"))?;
    match out.status.code() {
        Some(0) => Ok(format!(
            "Funktion „{name}“ in $PROFILE angelegt — gilt im nächsten PowerShell-Fenster."
        )),
        Some(3) => Err(format!(
            "In $PROFILE existiert bereits eine Funktion „{name}“."
        )),
        _ => Err(format!(
            "PowerShell fehlgeschlagen: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_name_mirrors_the_frontend_rule() {
        // Cross-language pin with lib/alias.ts::validAliasName — same samples.
        for ok in ["gs", "git-st", "_x2"] {
            assert!(valid_name(ok), "{ok} should be valid");
        }
        for bad in ["", "2fast", "a b", "a=b", "rm;", "a$b", "ä"] {
            assert!(!valid_name(bad), "{bad} should be rejected");
        }
    }

    #[test]
    fn alias_line_survives_embedded_single_quotes() {
        assert_eq!(alias_line("gs", "git status"), "alias gs='git status'");
        // POSIX close-reopen — no backslash escaping inside single quotes.
        assert_eq!(alias_line("say", "echo 'hi'"), "alias say='echo '\\''hi'\\'''");
    }

    #[test]
    fn rc_target_follows_the_shell_and_falls_back_per_os() {
        assert_eq!(rc_target(Some("/bin/zsh"), true), Ok(".zshrc"));
        assert_eq!(rc_target(Some("/usr/bin/bash"), true), Ok(".bashrc"));
        assert_eq!(rc_target(Some("bash"), false), Ok(".bashrc"));
        // Unknown/absent shell → the OS default.
        assert_eq!(rc_target(None, true), Ok(".zshrc"));
        assert_eq!(rc_target(None, false), Ok(".bashrc"));
        assert_eq!(rc_target(Some("/opt/weird/sh9"), false), Ok(".bashrc"));
        // Fish is refused honestly, never mis-written.
        assert!(rc_target(Some("/usr/local/bin/fish"), true).is_err());
    }

    #[test]
    fn already_defined_matches_real_lines_but_not_comments_or_prefixes() {
        let rc = "# alias gs='old'\nalias gsl='git log'\n  alias gs='git status'\n";
        assert!(already_defined(rc, "gs"));
        assert!(already_defined(rc, "gsl"));
        // `gs` must not match the longer `gsl` definition-only file.
        assert!(!already_defined("alias gsl='git log'\n", "gs"));
        // A commented-out line is not a definition.
        assert!(!already_defined("# alias dead='x'\n", "dead"));
        assert!(!already_defined("", "gs"));
    }
}
