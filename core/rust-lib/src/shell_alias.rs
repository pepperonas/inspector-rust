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

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct AliasEntry {
    pub name: String,
    pub command: String,
}

/// Undo shell quoting on an alias VALUE (pure): single-quoted segments are
/// literal, double-quoted segments unescape `\` `` ` `` `"` `$`, a backslash
/// outside quotes escapes the next char, and an unquoted `#` or `;` ends the
/// value (trailing comment / next statement). Handles the close-reopen
/// `'\''` form `alias_line` produces.
pub fn unquote_shell(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    loop {
        match it.next() {
            None => break,
            Some('\'') => {
                // Single quotes: everything literal until the closing quote.
                for c in it.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    out.push(c);
                }
            }
            Some('"') => loop {
                match it.next() {
                    None | Some('"') => break,
                    Some('\\') => match it.next() {
                        Some(c @ ('\\' | '`' | '"' | '$')) => out.push(c),
                        Some(c) => {
                            out.push('\\');
                            out.push(c);
                        }
                        None => out.push('\\'),
                    },
                    Some(c) => out.push(c),
                }
            },
            Some('\\') => {
                if let Some(c) = it.next() {
                    out.push(c);
                }
            }
            Some('#') | Some(';') => break,
            Some(c) => out.push(c),
        }
    }
    out.trim_end().to_string()
}

/// All alias definitions in rc content (pure). Later definitions win, like
/// the shell; commented-out lines don't count; unparseable names are skipped.
pub fn parse_aliases(content: &str) -> Vec<AliasEntry> {
    let mut out: Vec<AliasEntry> = Vec::new();
    for line in content.lines() {
        let t = line.trim_start();
        let Some(rest) = t.strip_prefix("alias ") else {
            continue;
        };
        let Some(eq) = rest.find('=') else { continue };
        let name = rest[..eq].trim();
        if !valid_name(name) {
            continue;
        }
        let command = unquote_shell(rest[eq + 1..].trim());
        match out.iter_mut().find(|e| e.name == name) {
            Some(e) => e.command = command,
            None => out.push(AliasEntry {
                name: name.to_string(),
                command,
            }),
        }
    }
    out
}

/// Remove every definition line of `name` (pure). Returns the new content +
/// whether anything was removed; all other lines survive verbatim.
pub fn remove_alias(content: &str, name: &str) -> (String, bool) {
    let needle = format!("alias {name}=");
    let mut removed = false;
    let mut out = String::new();
    for line in content.lines() {
        if line.trim_start().starts_with(&needle) {
            removed = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    (out, removed)
}

/// Replace an existing definition IN PLACE (keeping its spot in the file) or
/// append when absent (pure). Duplicate definition lines collapse into one.
pub fn upsert_alias(content: &str, name: &str, command: &str) -> String {
    let needle = format!("alias {name}=");
    let line = alias_line(name, command);
    if already_defined(content, name) {
        let mut out = String::new();
        let mut replaced = false;
        for l in content.lines() {
            if l.trim_start().starts_with(&needle) {
                if !replaced {
                    out.push_str(&line);
                    out.push('\n');
                    replaced = true;
                }
                continue;
            }
            out.push_str(l);
            out.push('\n');
        }
        out
    } else {
        let mut out = content.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&line);
        out.push('\n');
        out
    }
}

/// Path to the current user's rc file. macOS/Linux only.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rc_path() -> Result<(std::path::PathBuf, &'static str), String> {
    let rc = rc_target(
        std::env::var("SHELL").ok().as_deref(),
        cfg!(target_os = "macos"),
    )?;
    let home = dirs::home_dir().ok_or("Kein Home-Verzeichnis gefunden.")?;
    Ok((home.join(rc), rc))
}

/// List the aliases defined in the current user's rc file.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn list() -> Result<Vec<AliasEntry>, String> {
    let (path, _) = rc_path()?;
    Ok(parse_aliases(
        &std::fs::read_to_string(&path).unwrap_or_default(),
    ))
}

#[cfg(target_os = "windows")]
pub fn list() -> Result<Vec<AliasEntry>, String> {
    // $PROFILE functions aren't enumerable without spawning PowerShell per
    // refresh — the management section is macOS/Linux for now (honest gap).
    Ok(Vec::new())
}

/// Delete an alias from the rc file.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn delete(name: &str) -> Result<String, String> {
    if !valid_name(name) {
        return Err("Ungültiger Alias-Name.".into());
    }
    let (path, rc) = rc_path()?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let (next, removed) = remove_alias(&existing, name);
    if !removed {
        return Err(format!("In ~/{rc} gibt es keinen Alias „{name}“."));
    }
    std::fs::write(&path, next).map_err(|e| format!("Konnte ~/{rc} nicht schreiben: {e}"))?;
    Ok(format!(
        "Alias „{name}“ aus ~/{rc} entfernt — gilt im nächsten Terminal."
    ))
}

#[cfg(target_os = "windows")]
pub fn delete(_name: &str) -> Result<String, String> {
    Err("Alias-Verwaltung ist auf Windows noch nicht verfügbar.".into())
}

/// Append the alias to the current user's shell config. Returns a human
/// success message naming the file + the reload hint.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn create(name: &str, command: &str, overwrite: bool) -> Result<String, String> {
    if !valid_name(name) {
        return Err("Ungültiger Alias-Name.".into());
    }
    if command.trim().is_empty() {
        return Err("Befehl fehlt.".into());
    }
    let (path, rc) = rc_path()?;
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if already_defined(&existing, name) {
        if !overwrite {
            // Race guard: the panel derives `overwrite` from its list — a
            // definition that appeared since the last refresh is refused,
            // never silently replaced.
            return Err(format!(
                "In ~/{rc} existiert bereits ein Alias „{name}“ — Liste aktualisieren und über „Aktualisieren“ ersetzen."
            ));
        }
        std::fs::write(&path, upsert_alias(&existing, name, command))
            .map_err(|e| format!("Konnte ~/{rc} nicht schreiben: {e}"))?;
        return Ok(format!(
            "Alias „{name}“ in ~/{rc} aktualisiert — gilt im nächsten Terminal (oder: source ~/{rc})."
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
pub fn create(name: &str, command: &str, _overwrite: bool) -> Result<String, String> {
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

    #[test]
    fn parse_aliases_round_trips_what_alias_line_writes() {
        // The list must show exactly the command the builder stored — incl.
        // the close-reopen quoting for embedded single quotes.
        for cmd in ["git status", "echo 'hi'", "printf \"%s\" x"] {
            let rc = alias_line("t", cmd);
            let got = parse_aliases(&rc);
            assert_eq!(got, vec![AliasEntry { name: "t".into(), command: cmd.into() }], "{cmd}");
        }
    }

    #[test]
    fn parse_aliases_handles_hand_written_forms_and_noise() {
        let rc = "\n# alias dead='x'\nalias ll='ls -la'  # note\nexport PATH=x\nalias g=\"git \\$1\"\nalias bare=htop\nalias ll='ls -lah'\nalias 2bad='x'\n";
        let got = parse_aliases(rc);
        assert_eq!(
            got,
            vec![
                // Later definition wins, like the shell; trailing comment stripped.
                AliasEntry { name: "ll".into(), command: "ls -lah".into() },
                AliasEntry { name: "g".into(), command: "git $1".into() },
                AliasEntry { name: "bare".into(), command: "htop".into() },
            ]
        );
    }

    #[test]
    fn remove_alias_removes_only_the_target_and_reports_misses() {
        let rc = "alias a='1'\nalias ab='2'\n# alias a='old'\nexport X=1\n";
        let (next, removed) = remove_alias(rc, "a");
        assert!(removed);
        // `ab` and the comment and the export survive verbatim.
        assert_eq!(next, "alias ab='2'\n# alias a='old'\nexport X=1\n");
        let (same, removed2) = remove_alias(&next, "zzz");
        assert!(!removed2);
        assert_eq!(same, next);
    }

    #[test]
    fn upsert_alias_replaces_in_place_or_appends() {
        let rc = "alias a='1'\nexport X=1\nalias b='2'\n";
        // Replace keeps a's spot (first line), collapses nothing else.
        let up = upsert_alias(rc, "a", "one");
        assert_eq!(up, "alias a='one'\nexport X=1\nalias b='2'\n");
        // Duplicate definition lines collapse into one on replace.
        let dup = "alias a='1'\nalias a='2'\n";
        assert_eq!(upsert_alias(dup, "a", "3"), "alias a='3'\n");
        // Absent → append with newline guard.
        assert_eq!(upsert_alias("export X=1", "n", "v"), "export X=1\nalias n='v'\n");
        assert_eq!(upsert_alias("", "n", "v"), "alias n='v'\n");
    }
}
