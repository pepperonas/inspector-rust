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

/// Why a definition has to be a shell FUNCTION instead of an alias.
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FnReason {
    /// `cd` followed by another command. As an alias this leaves the CALLER's
    /// shell sitting in the target directory — you run it once and every later
    /// command in that terminal happens somewhere you did not choose. A
    /// function can put the `cd` in a subshell, so the directory change dies
    /// with the command.
    ChangesDirectory,
    /// The command references positional parameters. An alias cannot receive
    /// them at all: the shell expands the alias and APPENDS what you typed, so
    /// `$1` stays empty and your argument lands at the end of the line.
    TakesArguments,
}

/// Alias or function — and, when a function, why.
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(tag = "kind", content = "reason", rename_all = "kebab-case")]
pub enum Form {
    Alias,
    Function(FnReason),
}

/// Split a command into its top-level segments at `;` `&&` `||` `|`.
/// Quoted regions are skipped so a separator inside quotes doesn't split.
fn segments(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let bytes: Vec<char> = command.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
                cur.push(c);
                i += 1;
            }
            None => {
                if c == '\'' || c == '"' {
                    quote = Some(c);
                    cur.push(c);
                    i += 1;
                } else if c == ';' || c == '|' || c == '&' {
                    // `&&`/`||` consume two, `;`/`|` one.
                    let two = i + 1 < bytes.len() && bytes[i + 1] == c;
                    out.push(std::mem::take(&mut cur));
                    i += if two { 2 } else { 1 };
                } else {
                    cur.push(c);
                    i += 1;
                }
            }
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// Does `command` reference positional parameters (`$1`…`$9`, `$@`, `$*`)?
fn uses_positional(command: &str) -> bool {
    let c: Vec<char> = command.chars().collect();
    (0..c.len()).any(|i| {
        if c[i] != '$' {
            return false;
        }
        let mut j = i + 1;
        if j < c.len() && c[j] == '{' {
            j += 1;
        }
        matches!(c.get(j), Some(n) if n.is_ascii_digit() || *n == '@' || *n == '*')
    })
}

/// Decide whether a command must become a function, and why (pure).
///
/// ⚠️ **`cd` ALONE stays an alias.** `work='cd ~/projects'` is supposed to
/// leave you in that directory — wrapping it in a subshell would make it do
/// nothing at all. Only a `cd` with something AFTER it wants the subshell: you
/// go there to run one thing and expect your shell to stay where it was.
pub fn choose_form(command: &str) -> Form {
    if uses_positional(command) {
        return Form::Function(FnReason::TakesArguments);
    }
    let segs = segments(command);
    let cd_at = segs.iter().position(|s| s == "cd" || s.starts_with("cd "));
    match cd_at {
        Some(i) if i + 1 < segs.len() => Form::Function(FnReason::ChangesDirectory),
        _ => Form::Alias,
    }
}

/// The rc line for a function definition, on ONE line — the whole module is
/// line-based (append, replace in place, delete), and a multi-line body would
/// break every one of those.
///
/// ⚠️ `"$@"` is appended for the directory case because an alias forwards
/// trailing arguments for free and a function does NOT. Converting without it
/// would silently take away behaviour the user already had.
pub fn function_line(name: &str, command: &str, reason: FnReason) -> String {
    match reason {
        FnReason::ChangesDirectory => format!("{name}() {{ ( {command} \"$@\" ); }}"),
        FnReason::TakesArguments => format!("{name}() {{ {command}; }}"),
    }
}

/// The definition line for `command`, alias or function as the command needs.
pub fn definition_line(name: &str, command: &str) -> String {
    match choose_form(command) {
        Form::Alias => alias_line(name, command),
        Form::Function(r) => function_line(name, command, r),
    }
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
    rc_content.lines().any(|l| defines(l, name))
}

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct AliasEntry {
    pub name: String,
    pub command: String,
    /// Display label of the defining file, e.g. `~/.claude/aliases/aliases.zsh`.
    pub file: String,
    /// Whether it lives in the primary rc file (where new aliases are appended).
    pub primary: bool,
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

/// One `alias NAME=value` line → `(name, command)` (pure). Commented-out
/// lines and unparseable names are `None`.
pub fn parse_alias_line(line: &str) -> Option<(String, String)> {
    let rest = line.trim_start().strip_prefix("alias ")?;
    let eq = rest.find('=')?;
    let name = rest[..eq].trim();
    if !valid_name(name) {
        return None;
    }
    Some((name.to_string(), unquote_shell(rest[eq + 1..].trim())))
}

/// Parse a one-line function definition back to the command the user typed.
///
/// The inverse of [`function_line`], so the manager can list, edit and delete
/// a function exactly like an alias: the subshell wrapper and the appended
/// `"$@"` are stripped, because the user never typed them — they are how the
/// definition is SPELLED, not what it does.
pub fn parse_function_line(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    let open = t.find("()")?;
    let name = t[..open].trim();
    if !valid_name(name) {
        return None;
    }
    let rest = t[open + 2..].trim_start();
    let body = rest.strip_prefix('{')?.strip_suffix('}')?.trim();
    let body = body.strip_suffix(';').unwrap_or(body).trim();
    // Subshell form: `( cmd "$@" )`.
    let body = match body.strip_prefix('(').and_then(|b| b.strip_suffix(')')) {
        Some(inner) => inner.trim(),
        None => body,
    };
    let body = body.strip_suffix(r#""$@""#).unwrap_or(body).trim();
    let body = body.strip_suffix(';').unwrap_or(body).trim();
    if body.is_empty() {
        return None;
    }
    Some((name.to_string(), body.to_string()))
}

/// Parse either form — the manager treats both the same way.
pub fn parse_definition_line(line: &str) -> Option<(String, String)> {
    parse_alias_line(line).or_else(|| parse_function_line(line))
}

/// Does `line` define `name`, in either form?
pub fn defines(line: &str, name: &str) -> bool {
    let t = line.trim_start();
    t.starts_with(&format!("alias {name}=")) || t.starts_with(&format!("{name}()"))
}

/// The path tokens a line `source`s (pure): every token following a bare
/// `source` or `.` word — which also catches the guarded
/// `[ -f ~/.fzf.zsh ] && source ~/.fzf.zsh` form; flag tokens are skipped.
/// ⚠️ Paths containing spaces don't tokenise — accepted limitation.
pub fn source_targets(line: &str) -> Vec<String> {
    let t = line.trim_start();
    if t.starts_with('#') {
        return Vec::new();
    }
    let toks: Vec<&str> = t.split_whitespace().collect();
    let mut out = Vec::new();
    for w in toks.windows(2) {
        if (w[0] == "source" || w[0] == ".") && !w[1].starts_with('-') {
            out.push(w[1].to_string());
        }
    }
    out
}

/// Expand a (possibly quoted) `~` / `$HOME` / `${HOME}` path token (pure).
pub fn expand_home(token: &str, home: &std::path::Path) -> std::path::PathBuf {
    let p = token.trim_matches('"').trim_matches('\'');
    if p == "~" {
        return home.to_path_buf();
    }
    for prefix in ["~/", "$HOME/", "${HOME}/"] {
        if let Some(rest) = p.strip_prefix(prefix) {
            return home.join(rest);
        }
    }
    std::path::PathBuf::from(p)
}

/// Display label for a file: home-relative as `~/…`, else the full path.
fn display_label(path: &std::path::Path, home: &std::path::Path) -> String {
    match path.strip_prefix(home) {
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => path.display().to_string(),
    }
}

/// Walk the shell-startup files IN ORDER, following `source` lines (pure over
/// an injected reader): aliases from every reachable file, later definitions
/// winning across files exactly like the live shell. Guards: only files under
/// `$HOME` are followed (plugin trees like /opt/homebrew stay out), depth ≤ 3,
/// a visited set breaks cycles. Returns the entries + every visited file (the
/// delete path sweeps all of them).
pub fn collect_aliases(
    seeds: &[std::path::PathBuf],
    home: &std::path::Path,
    read: &dyn Fn(&std::path::Path) -> Option<String>,
) -> (Vec<AliasEntry>, Vec<std::path::PathBuf>) {
    fn walk(
        path: &std::path::Path,
        home: &std::path::Path,
        read: &dyn Fn(&std::path::Path) -> Option<String>,
        visited: &mut Vec<std::path::PathBuf>,
        out: &mut Vec<AliasEntry>,
        depth: u8,
    ) {
        if depth > 3 || visited.iter().any(|v| v == path) {
            return;
        }
        visited.push(path.to_path_buf());
        let Some(content) = read(path) else { return };
        let label = display_label(path, home);
        for line in content.lines() {
            for tok in source_targets(line) {
                let target = expand_home(&tok, home);
                if target.starts_with(home) {
                    walk(&target, home, read, visited, out, depth + 1);
                }
            }
            if let Some((name, command)) = parse_definition_line(line) {
                match out.iter_mut().find(|e| e.name == name) {
                    Some(e) => {
                        e.command = command;
                        e.file = label.clone();
                    }
                    None => out.push(AliasEntry {
                        name,
                        command,
                        file: label.clone(),
                        primary: false,
                    }),
                }
            }
        }
    }
    let mut visited = Vec::new();
    let mut out = Vec::new();
    for s in seeds {
        walk(s, home, read, &mut visited, &mut out, 0);
    }
    (out, visited)
}

/// The shell-startup files to seed the walk with, in the shell's own read
/// order (later files win) — so aliases in `.zshenv`/`.zprofile` are seen too.
pub fn seed_files(rc: &str, home: &std::path::Path) -> Vec<std::path::PathBuf> {
    let names: &[&str] = if rc == ".zshrc" {
        &[".zshenv", ".zprofile", ".zshrc"]
    } else {
        &[".profile", ".bash_profile", ".bashrc"]
    };
    names.iter().map(|n| home.join(n)).collect()
}

/// Remove every definition line of `name` (pure). Returns the new content +
/// whether anything was removed; all other lines survive verbatim.
pub fn remove_alias(content: &str, name: &str) -> (String, bool) {
    let mut removed = false;
    let mut out = String::new();
    for line in content.lines() {
        if defines(line, name) {
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
    // ⚠️ The FORM is re-decided on every write: editing `cd x` into
    // `cd x && run` must turn the alias into a function, and back again.
    let line = definition_line(name, command);
    if already_defined(content, name) {
        let mut out = String::new();
        let mut replaced = false;
        for l in content.lines() {
            if defines(l, name) {
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

/// Inverse of `display_label`: `~/x` → `$HOME/x`, else the literal path.
fn label_to_path(label: &str, home: &std::path::Path) -> std::path::PathBuf {
    match label.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => std::path::PathBuf::from(label),
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

/// List the aliases across the whole shell-startup chain — the rc file plus
/// everything it `source`s (the fix for "my aliases live in a sourced file
/// and weren't listed"). Each entry names its defining file.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn list() -> Result<Vec<AliasEntry>, String> {
    let (_, rc) = rc_path()?;
    let home = dirs::home_dir().ok_or("Kein Home-Verzeichnis gefunden.")?;
    let read = |p: &std::path::Path| std::fs::read_to_string(p).ok();
    let (mut entries, _) = collect_aliases(&seed_files(rc, &home), &home, &read);
    let rc_label = format!("~/{rc}");
    for e in &mut entries {
        e.primary = e.file == rc_label;
    }
    Ok(entries)
}

#[cfg(target_os = "windows")]
pub fn list() -> Result<Vec<AliasEntry>, String> {
    // $PROFILE functions aren't enumerable without spawning PowerShell per
    // refresh — the management section is macOS/Linux for now (honest gap).
    Ok(Vec::new())
}

/// Delete an alias from EVERY startup file that defines it — removing only
/// the last-wins definition would silently resurrect a shadowed one.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn delete(name: &str) -> Result<String, String> {
    if !valid_name(name) {
        return Err("Ungültiger Alias-Name.".into());
    }
    let (_, rc) = rc_path()?;
    let home = dirs::home_dir().ok_or("Kein Home-Verzeichnis gefunden.")?;
    let read = |p: &std::path::Path| std::fs::read_to_string(p).ok();
    let (_, visited) = collect_aliases(&seed_files(rc, &home), &home, &read);
    let mut touched: Vec<String> = Vec::new();
    for path in &visited {
        let Ok(existing) = std::fs::read_to_string(path) else {
            continue;
        };
        let (next, removed) = remove_alias(&existing, name);
        if removed {
            std::fs::write(path, next)
                .map_err(|e| format!("Konnte {} nicht schreiben: {e}", path.display()))?;
            touched.push(display_label(path, &home));
        }
    }
    if touched.is_empty() {
        return Err(format!("Kein Alias „{name}“ in den Shell-Startdateien gefunden."));
    }
    Ok(format!(
        "Alias „{name}“ aus {} entfernt — gilt im nächsten Terminal.",
        touched.join(" + ")
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
    let home = dirs::home_dir().ok_or("Kein Home-Verzeichnis gefunden.")?;
    let read = |p: &std::path::Path| std::fs::read_to_string(p).ok();
    let (entries, _) = collect_aliases(&seed_files(rc, &home), &home, &read);
    if let Some(defined) = entries.iter().find(|e| e.name == name) {
        if !overwrite {
            // Race guard: the panel derives `overwrite` from its list — a
            // definition that appeared since the last refresh is refused,
            // never silently replaced.
            return Err(format!(
                "In {} existiert bereits ein Alias „{name}“ — Liste aktualisieren und über „Aktualisieren“ ersetzen.",
                defined.file
            ));
        }
        // Update IN the defining file (which may be a sourced one), keeping
        // the definition's spot — not a second copy in the rc that the
        // sourced file's line would then shadow-fight with.
        let target = label_to_path(&defined.file, &home);
        let existing = std::fs::read_to_string(&target).unwrap_or_default();
        std::fs::write(&target, upsert_alias(&existing, name, command))
            .map_err(|e| format!("Konnte {} nicht schreiben: {e}", defined.file))?;
        return Ok(format!(
            "Alias „{name}“ in {} aktualisiert — gilt im nächsten Terminal.",
            defined.file
        ));
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
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
    fn a_function_round_trips_to_the_command_the_user_typed() {
        // The subshell and the `"$@"` are how the definition is SPELLED, not
        // what the user asked for — the editor must show their command back.
        for cmd in ["cd ~/x && ./y", "cd /tmp && ls -la"] {
            let line = definition_line("f", cmd);
            assert_eq!(parse_definition_line(&line), Some(("f".into(), cmd.into())), "{line}");
        }
        let line = definition_line("g", "echo $1");
        assert_eq!(parse_definition_line(&line), Some(("g".into(), "echo $1".into())));
    }

    #[test]
    fn defines_matches_both_forms_and_not_a_longer_name() {
        assert!(defines("alias bb='x'", "bb"));
        assert!(defines("bb() { ( cd /x ); }", "bb"));
        assert!(defines("  bb() { x; }", "bb"));
        assert!(!defines("alias bbx='x'", "bb"));
        assert!(!defines("bbx() { x; }", "bb"));
        assert!(!defines("# bb() { x; }", "bb"));
    }

    #[test]
    fn editing_re_decides_the_form_in_both_directions() {
        // Adding a command after the `cd` must turn the alias into a
        // function; taking it away again must turn it back.
        let rc = "alias bb='cd ~/x'\n";
        let grown = upsert_alias(rc, "bb", "cd ~/x && ./y");
        assert!(grown.contains("bb() { ( cd ~/x && ./y"), "{grown}");
        assert!(!grown.contains("alias bb="), "the old alias line must be gone");
        let shrunk = upsert_alias(&grown, "bb", "cd ~/x");
        assert!(shrunk.contains("alias bb='cd ~/x'"), "{shrunk}");
        assert!(!shrunk.contains("bb() {"), "the function must be gone");
    }

    #[test]
    fn a_function_can_be_deleted_like_an_alias() {
        let rc = "alias a='1'\nbb() { ( cd /x && ./y \"$@\" ); }\nalias c='3'\n";
        let (out, removed) = remove_alias(rc, "bb");
        assert!(removed);
        assert!(!out.contains("bb()"));
        assert!(out.contains("alias a='1'") && out.contains("alias c='3'"));
    }

    #[test]
    fn a_plain_command_stays_an_alias() {
        assert_eq!(choose_form("git status"), Form::Alias);
        assert_eq!(definition_line("gs", "git status"), "alias gs='git status'");
    }

    #[test]
    fn cd_alone_stays_an_alias() {
        // ⚠️ The load-bearing distinction. `work='cd ~/projects'` exists to
        // LEAVE you there — a subshell would make it do nothing at all.
        assert_eq!(choose_form("cd ~/projects"), Form::Alias);
        assert_eq!(choose_form("cd /tmp"), Form::Alias);
    }

    #[test]
    fn cd_followed_by_a_command_becomes_a_subshell_function() {
        // The other half: you go there to run one thing, and your shell must
        // stay where it was. As an alias every later command in that terminal
        // would happen somewhere you did not choose.
        assert_eq!(
            choose_form("cd ~/claude/beat-bytes && ./target/release/beatbyte"),
            Form::Function(FnReason::ChangesDirectory)
        );
        assert_eq!(
            definition_line("bb", "cd ~/x && ./y"),
            r#"bb() { ( cd ~/x && ./y "$@" ); }"#
        );
    }

    #[test]
    fn the_subshell_form_forwards_arguments() {
        // An alias forwards trailing arguments for free; a function does not.
        // Converting without `"$@"` would silently REMOVE behaviour.
        let line = definition_line("bb", "cd ~/x && ./y");
        assert!(line.contains(r#""$@""#), "arguments must still reach the command");
        assert!(line.contains("( ") && line.contains(" )"), "the cd belongs in a subshell");
    }

    #[test]
    fn positional_parameters_force_a_function() {
        // An alias cannot receive them: the shell appends what you typed, so
        // `$1` stays empty and the argument lands at the end of the line.
        assert_eq!(choose_form("echo $1"), Form::Function(FnReason::TakesArguments));
        assert_eq!(choose_form("mkdir -p $1 && cd ${1}"), Form::Function(FnReason::TakesArguments));
        assert_eq!(choose_form(r#"git commit -m "$@""#), Form::Function(FnReason::TakesArguments));
    }

    #[test]
    fn a_command_that_already_uses_arguments_does_not_get_another_set() {
        let line = definition_line("c", r#"git commit -m "$1""#);
        assert_eq!(line.matches("$1").count(), 1);
        assert!(!line.contains(r#""$@""#), "appending would duplicate the arguments");
    }

    #[test]
    fn a_separator_inside_quotes_does_not_split_the_command() {
        // `echo 'a;b'` is ONE segment — otherwise a quoted semicolon would
        // fake a second command and turn a plain alias into a function.
        assert_eq!(choose_form("echo 'a;b'"), Form::Alias);
        assert_eq!(choose_form(r#"cd /x && echo "a && b""#), Form::Function(FnReason::ChangesDirectory));
    }

    #[test]
    fn a_dollar_that_is_not_positional_is_left_alone() {
        assert_eq!(choose_form("echo $HOME"), Form::Alias);
        assert_eq!(choose_form("echo ${EDITOR}"), Form::Alias);
    }

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
    fn parse_alias_line_round_trips_what_alias_line_writes() {
        // The list must show exactly the command the builder stored — incl.
        // the close-reopen quoting for embedded single quotes.
        for cmd in ["git status", "echo 'hi'", "printf \"%s\" x"] {
            let rc = alias_line("t", cmd);
            assert_eq!(parse_alias_line(&rc), Some(("t".into(), cmd.into())), "{cmd}");
        }
    }

    #[test]
    fn hand_written_forms_and_noise_parse_correctly() {
        let rc = "\n# alias dead='x'\nalias ll='ls -la'  # note\nexport PATH=x\nalias g=\"git \\$1\"\nalias bare=htop\nalias ll='ls -lah'\nalias 2bad='x'\n";
        let home = std::path::Path::new("/Users/t");
        let files: std::collections::HashMap<std::path::PathBuf, String> =
            [(home.join(".zshrc"), rc.to_string())].into_iter().collect();
        let read = |p: &std::path::Path| files.get(p).cloned();
        let (got, _) = collect_aliases(&seed_files(".zshrc", home), home, &read);
        let names: Vec<(&str, &str)> = got.iter().map(|e| (e.name.as_str(), e.command.as_str())).collect();
        // Later definition wins, like the shell; trailing comment stripped;
        // comments + invalid names skipped.
        assert_eq!(names, vec![("ll", "ls -lah"), ("g", "git $1"), ("bare", "htop")]);
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

    #[test]
    fn source_targets_catches_plain_dot_and_guarded_forms() {
        assert_eq!(source_targets(r#"source "$HOME/.claude/aliases/aliases.zsh""#), vec![r#""$HOME/.claude/aliases/aliases.zsh""#]);
        assert_eq!(source_targets(". ~/env"), vec!["~/env"]);
        // The guarded one-liner real rc files use.
        assert_eq!(source_targets("[ -f ~/.fzf.zsh ] && source ~/.fzf.zsh"), vec!["~/.fzf.zsh"]);
        // Comments and flags don't source anything.
        assert!(source_targets("# source ~/dead").is_empty());
        assert!(source_targets("source -h").is_empty());
        // A `source` glued into a quoted alias value doesn't tokenise as a
        // bare `source` word → not followed.
        assert!(source_targets("alias s='source ~/x'").is_empty());
    }

    #[test]
    fn expand_home_handles_quotes_tilde_and_home_vars() {
        let home = std::path::Path::new("/Users/t");
        assert_eq!(expand_home(r#""$HOME/a/b""#, home), std::path::PathBuf::from("/Users/t/a/b"));
        assert_eq!(expand_home("~/x", home), std::path::PathBuf::from("/Users/t/x"));
        assert_eq!(expand_home("${HOME}/y", home), std::path::PathBuf::from("/Users/t/y"));
        assert_eq!(expand_home("'~'", home), std::path::PathBuf::from("/Users/t"));
        assert_eq!(expand_home("/abs/z", home), std::path::PathBuf::from("/abs/z"));
    }

    #[test]
    fn collect_aliases_follows_source_lines_and_labels_the_defining_file() {
        // The exact field report: yolo/celox live in a sourced file, cl in the rc.
        let home = std::path::Path::new("/Users/t");
        let files: std::collections::HashMap<std::path::PathBuf, &str> = [
            (home.join(".zshrc"), "source \"$HOME/.claude/aliases/aliases.zsh\"\nalias cl='clear'\n"),
            (home.join(".claude/aliases/aliases.zsh"), "alias yolo=\"claude --dangerously-skip-permissions\"\nalias celox=\"ssh celox\"\n"),
        ]
        .into_iter()
        .collect();
        let read = |p: &std::path::Path| files.get(p).map(|s| s.to_string());
        let (entries, visited) = collect_aliases(&seed_files(".zshrc", home), home, &read);
        let by_name: std::collections::HashMap<&str, &AliasEntry> =
            entries.iter().map(|e| (e.name.as_str(), e)).collect();
        assert_eq!(by_name["yolo"].command, "claude --dangerously-skip-permissions");
        assert_eq!(by_name["yolo"].file, "~/.claude/aliases/aliases.zsh");
        assert_eq!(by_name["cl"].file, "~/.zshrc");
        // The sweep list covers both real files (plus the missing seeds).
        assert!(visited.contains(&home.join(".claude/aliases/aliases.zsh")));
    }

    #[test]
    fn collect_aliases_later_definition_wins_across_files_and_cycles_terminate() {
        let home = std::path::Path::new("/Users/t");
        let files: std::collections::HashMap<std::path::PathBuf, &str> = [
            // a sources b, b sources a (cycle); rc redefines x AFTER sourcing.
            (home.join(".zshrc"), "source ~/a\nalias x='from-rc'\n"),
            (home.join("a"), "source ~/b\nalias x='from-a'\nalias only_a='1'\n"),
            (home.join("b"), "source ~/a\nalias from_b='2'\n"),
        ]
        .into_iter()
        .collect();
        let read = |p: &std::path::Path| files.get(p).map(|s| s.to_string());
        let (entries, _) = collect_aliases(&seed_files(".zshrc", home), home, &read);
        let x = entries.iter().find(|e| e.name == "x").unwrap();
        // The rc's own line comes AFTER the source → it wins, like the shell.
        assert_eq!(x.command, "from-rc");
        assert_eq!(x.file, "~/.zshrc");
        assert!(entries.iter().any(|e| e.name == "only_a"));
        assert!(entries.iter().any(|e| e.name == "from_b"));
    }

    #[test]
    fn collect_aliases_never_follows_files_outside_home() {
        let home = std::path::Path::new("/Users/t");
        let files: std::collections::HashMap<std::path::PathBuf, &str> = [
            (home.join(".zshrc"), "source /opt/homebrew/share/plugin.zsh\nalias ok='1'\n"),
            (std::path::PathBuf::from("/opt/homebrew/share/plugin.zsh"), "alias evil='x'\n"),
        ]
        .into_iter()
        .collect();
        let read = |p: &std::path::Path| files.get(p).map(|s| s.to_string());
        let (entries, visited) = collect_aliases(&seed_files(".zshrc", home), home, &read);
        assert!(entries.iter().any(|e| e.name == "ok"));
        assert!(!entries.iter().any(|e| e.name == "evil"));
        assert!(!visited.iter().any(|p| p.starts_with("/opt")));
    }
}
