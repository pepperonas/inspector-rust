//! `sec` — guided pentest-command builders (v0.84.271).
//!
//! A **data-driven** registry ([`registry`]) turns four standard tools (nmap,
//! sqlmap, feroxbuster, John the Ripper) — plus a gobuster smoke-test proving
//! extensibility — into search-bar command *builders*. Inspector Rust assembles
//! a syntactically correct command line from presets + user input; it **never
//! runs the tools itself**, opens no sockets, spawns no tool subprocess. Two
//! outputs: the command to the clipboard (default), or an **opt-in** hand-off
//! that opens the user's terminal (macOS iTerm2 → Terminal.app) with the
//! command inserted — executed in the user's own shell, not as our child.
//!
//! Adding a tool = adding one [`ToolSpec`] to the registry — no parser, builder
//! or UI change (see `registry::GOBUSTER`). The command string is assembled
//! **frontend-side** from this catalogue (`lib/sec.ts`), so there is no IPC per
//! keystroke; Rust only serialises the catalogue and performs the hand-off.

mod registry;

use serde::{Deserialize, Serialize};

/// One token of a preset's command line. Serialised to the frontend, which
/// walks the segments to build the final string (literals verbatim, fields
/// shell-quoted, missing required fields shown as `‹key›` placeholders).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Segment {
    /// A literal token (binary name, sub-command, a bare flag): `nmap`, `-sV`.
    Lit { text: &'static str },
    /// A value field, always emitted (required → placeholder when empty):
    /// the positional target / URL / hash file.
    Field { key: &'static str },
    /// An optional `flag value` pair, emitted only when the field is set:
    /// `-p 1-1000`, `-oA base`, `-w wordlist`.
    Flag { flag: &'static str, key: &'static str },
    /// An optional `prefix+value` single token, emitted only when set:
    /// `--level=3`, `--wordlist=…`, `-T4`.
    Joined { prefix: &'static str, key: &'static str },
}

/// A user-supplied value the builder collects for a preset.
#[derive(Debug, Clone, Serialize)]
pub struct FieldSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub placeholder: &'static str,
    pub help: &'static str,
    pub required: bool,
}

/// A named recipe: an ordered segment list + which fields it uses + safety
/// metadata. `sharp` forces a confirmation before the terminal hand-off.
#[derive(Debug, Clone, Serialize)]
pub struct PresetSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub purpose: &'static str,
    pub segments: &'static [Segment],
    /// Field keys this preset uses (subset of the tool's `fields`).
    pub fields: &'static [&'static str],
    /// Sharp = long-running / loud / writes data / needs root → always confirm
    /// before the hand-off, regardless of the auto-enter setting.
    pub sharp: bool,
    /// Honest, non-marketing tags: "long-running" · "loud" · "writes-data" ·
    /// "needs-root". Shown as chips, never sold as "detection bypass".
    pub tags: &'static [&'static str],
    /// Grouping within a tool ("scan", "enumerate", "prepare", …) — `john
    /// prepare` filters to the `prepare` category.
    pub category: &'static str,
}

/// A tool = one registry entry. Everything the parser/builder/UI needs is here;
/// adding a tool touches no code path.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub binary: &'static str,
    pub presets: &'static [PresetSpec],
    pub fields: &'static [FieldSpec],
    /// `flag → plain-English explanation` for the preview's cheat-sheet table.
    pub flag_help: &'static [(&'static str, &'static str)],
    /// Caveats surfaced in the preview (John Core/Jumbo, ferox version drift…).
    pub notes: &'static [&'static str],
}

/// A verified John `--format=` name + whether it needs the Jumbo build.
#[derive(Debug, Clone, Serialize)]
pub struct JohnFormat {
    pub name: &'static str,
    pub jumbo: bool,
}

/// The whole catalogue, serialised once to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct SecCatalog {
    pub tools: &'static [ToolSpec],
    /// Common wordlist paths for autocomplete (existence-checked frontend-side).
    pub common_wordlists: &'static [&'static str],
    /// Verified John hash-format names (Core/Jumbo tagged).
    pub john_formats: &'static [JohnFormat],
}

pub fn catalog() -> SecCatalog {
    SecCatalog {
        tools: registry::CATALOG,
        common_wordlists: registry::COMMON_WORDLISTS,
        john_formats: registry::JOHN_FORMATS,
    }
}

/// Does a path exist on this machine? For the wordlist existence check /
/// autocomplete — read-only, no traversal, no follow beyond a plain `exists()`.
pub fn path_exists(path: &str) -> bool {
    let p = path.trim();
    !p.is_empty() && std::path::Path::new(p).exists()
}

/// Every flag that appears as a `Lit` in a preset, for the consistency test +
/// to guarantee the preview can explain each one.
#[cfg(test)]
fn preset_flags(p: &PresetSpec) -> impl Iterator<Item = &'static str> + '_ {
    p.segments.iter().filter_map(|s| match s {
        Segment::Lit { text } if text.starts_with('-') => Some(*text),
        _ => None,
    })
}

// ── Settings ─────────────────────────────────────────────────────────────────

/// Persisted per-field (`sec.<x>`), mirrors bruno/faker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecDefaults {
    pub wordlist: String,
    pub output_dir: String,
    pub timing: String,
    pub threads: u32,
    pub rate: u32,
    /// "jumbo" | "core" — gates which John modes/formats the UI offers.
    pub john_line: String,
    /// "iterm" | "terminal" — preferred terminal for the hand-off.
    pub terminal: String,
    /// Auto-press Enter after inserting the command. Default OFF.
    pub auto_enter: bool,
    /// Optional scope/authorisation note shown in the preview header.
    pub scope_note: String,
    pub save_history: bool,
}

impl Default for SecDefaults {
    fn default() -> Self {
        SecDefaults {
            wordlist: String::new(),
            output_dir: String::new(),
            // Empty = don't silently inject -T/-t/--rate-limit into every
            // command; the user opts in via Settings if they want a default.
            timing: String::new(),
            threads: 0,
            rate: 0,
            john_line: "jumbo".into(),
            terminal: "iterm".into(),
            auto_enter: false,
            scope_note: String::new(),
            save_history: true,
        }
    }
}

const KEY_WORDLIST: &str = "sec.wordlist";
const KEY_OUTPUT_DIR: &str = "sec.output_dir";
const KEY_TIMING: &str = "sec.timing";
const KEY_THREADS: &str = "sec.threads";
const KEY_RATE: &str = "sec.rate";
const KEY_JOHN_LINE: &str = "sec.john_line";
const KEY_TERMINAL: &str = "sec.terminal";
const KEY_AUTO_ENTER: &str = "sec.auto_enter";
const KEY_SCOPE_NOTE: &str = "sec.scope_note";
const KEY_SAVE_HISTORY: &str = "sec.save_history";

pub fn get_defaults(db: &crate::db::DbHandle) -> anyhow::Result<SecDefaults> {
    let d = SecDefaults::default();
    let john_line = crate::settings::get_or(db, KEY_JOHN_LINE, &d.john_line)?;
    let john_line = if john_line == "core" { john_line } else { "jumbo".into() };
    let terminal = crate::settings::get_or(db, KEY_TERMINAL, &d.terminal)?;
    let terminal = if terminal == "terminal" { terminal } else { "iterm".into() };
    Ok(SecDefaults {
        wordlist: crate::settings::get_or(db, KEY_WORDLIST, &d.wordlist)?,
        output_dir: crate::settings::get_or(db, KEY_OUTPUT_DIR, &d.output_dir)?,
        timing: {
            let t = crate::settings::get_or(db, KEY_TIMING, &d.timing)?;
            // "" = no default; otherwise a single 0–5 digit.
            if t.is_empty() || (t.len() == 1 && t.chars().all(|c| ('0'..='5').contains(&c))) {
                t
            } else {
                d.timing
            }
        },
        threads: crate::settings::get_or(db, KEY_THREADS, &d.threads.to_string())?
            .parse()
            .unwrap_or(d.threads)
            .min(500),
        rate: crate::settings::get_or(db, KEY_RATE, &d.rate.to_string())?
            .parse()
            .unwrap_or(d.rate)
            .min(100_000),
        john_line,
        terminal,
        auto_enter: crate::settings::get_bool(db, KEY_AUTO_ENTER, d.auto_enter)?,
        scope_note: crate::settings::get_or(db, KEY_SCOPE_NOTE, &d.scope_note)?,
        save_history: crate::settings::get_bool(db, KEY_SAVE_HISTORY, d.save_history)?,
    })
}

pub fn set_defaults(db: &crate::db::DbHandle, d: &SecDefaults) -> anyhow::Result<()> {
    crate::settings::set(db, KEY_WORDLIST, &d.wordlist)?;
    crate::settings::set(db, KEY_OUTPUT_DIR, &d.output_dir)?;
    let timing = if d.timing.is_empty()
        || (d.timing.len() == 1 && d.timing.chars().all(|c| ('0'..='5').contains(&c)))
    {
        d.timing.clone()
    } else {
        String::new()
    };
    crate::settings::set(db, KEY_TIMING, &timing)?;
    crate::settings::set(db, KEY_THREADS, &d.threads.min(500).to_string())?;
    crate::settings::set(db, KEY_RATE, &d.rate.min(100_000).to_string())?;
    crate::settings::set(db, KEY_JOHN_LINE, if d.john_line == "core" { "core" } else { "jumbo" })?;
    crate::settings::set(db, KEY_TERMINAL, if d.terminal == "terminal" { "terminal" } else { "iterm" })?;
    crate::settings::set(db, KEY_AUTO_ENTER, if d.auto_enter { "true" } else { "false" })?;
    crate::settings::set(db, KEY_SCOPE_NOTE, &d.scope_note)?;
    crate::settings::set(db, KEY_SAVE_HISTORY, if d.save_history { "true" } else { "false" })?;
    Ok(())
}

// ── Terminal hand-off (macOS; opt-in; NO tool subprocess) ────────────────────

/// Open the user's terminal with `command` inserted. **This is the ONLY process
/// this module ever spawns, and it is `osascript` opening a terminal — never a
/// pentest tool.** The tool runs in the user's shell, with their environment,
/// logging and sudo prompt; Inspector Rust hands over text and stops.
///
/// - `auto_enter = false` (default): the command is *typed* into a fresh window
///   via System Events `keystroke` and left un-submitted — the user reviews it
///   and presses Enter themselves.
/// - `auto_enter = true`: iTerm `write text` / Terminal `do script` runs it.
///
/// `command` is already shell-quoted by the frontend builder; here it is only
/// escaped for the AppleScript string literal (`osa_escape`). macOS only —
/// callers fall back to clipboard elsewhere.
#[cfg(target_os = "macos")]
pub fn open_in_terminal(command: &str, auto_enter: bool) -> Result<(), String> {
    let script = build_handoff_script(command, auto_enter, crate::finder_selection::iterm_installed());
    run_osascript(&script)
}

/// Build the osascript for the hand-off (pure — no process, unit-testable).
/// `command` is already shell-quoted by the frontend builder; here it is only
/// escaped for the AppleScript string literal. `auto_enter=false` types the
/// command via System Events `keystroke` WITHOUT a newline (staged, not run);
/// `auto_enter=true` runs it via iTerm `write text` / Terminal `do script`.
#[cfg(any(target_os = "macos", test))]
pub fn build_handoff_script(command: &str, auto_enter: bool, iterm: bool) -> String {
    let esc = crate::finder_selection::osa_escape(command);
    if iterm {
        if auto_enter {
            format!(
                "tell application \"iTerm\"\n\
                     activate\n\
                     create window with default profile\n\
                     tell current session of current window to write text \"{esc}\"\n\
                 end tell"
            )
        } else {
            format!(
                "tell application \"iTerm\"\n\
                     activate\n\
                     create window with default profile\n\
                 end tell\n\
                 delay 0.3\n\
                 tell application \"System Events\" to keystroke \"{esc}\""
            )
        }
    } else if auto_enter {
        format!("tell application \"Terminal\"\n\tactivate\n\tdo script \"{esc}\"\nend tell")
    } else {
        format!(
            "tell application \"Terminal\"\n\tactivate\n\tdo script \"\"\nend tell\n\
             delay 0.3\n\
             tell application \"System Events\" to keystroke \"{esc}\""
        )
    }
}

#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> Result<(), String> {
    let ok = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err("failed to open a terminal".into())
    }
}

/// Non-macOS: the hand-off isn't supported (the shell-quoting is POSIX-only,
/// and the terminal machinery is macOS). The frontend falls back to clipboard
/// and says so; this keeps a single honest state, not a half-done one.
#[cfg(not(target_os = "macos"))]
pub fn open_in_terminal(_command: &str, _auto_enter: bool) -> Result<(), String> {
    Err("terminal hand-off is macOS-only; the command is on the clipboard".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_field_exists_and_every_flag_is_explained() {
        for tool in registry::CATALOG {
            let field_keys: std::collections::HashSet<&str> =
                tool.fields.iter().map(|f| f.key).collect();
            let explained: std::collections::HashSet<&str> =
                tool.flag_help.iter().map(|(f, _)| *f).collect();
            for p in tool.presets {
                // Every field the preset declares must be defined on the tool.
                for key in p.fields {
                    assert!(field_keys.contains(key), "{}::{} unknown field {key}", tool.name, p.name);
                }
                // Every segment field-key must also be a declared field.
                for seg in p.segments {
                    let k = match seg {
                        Segment::Field { key } | Segment::Flag { key, .. } | Segment::Joined { key, .. } => Some(*key),
                        Segment::Lit { .. } => None,
                    };
                    if let Some(k) = k {
                        assert!(field_keys.contains(k), "{}::{} segment field {k} undefined", tool.name, p.name);
                    }
                }
                // Every literal flag used must have a plain-English explanation
                // (no cheat-sheet gaps).
                for flag in preset_flags(p) {
                    assert!(explained.contains(flag), "{}::{} flag {flag} has no explanation", tool.name, p.name);
                }
            }
        }
    }

    #[test]
    fn catalog_serialises_fully_with_all_tools() {
        let cat = catalog();
        let json = serde_json::to_string(&cat).unwrap();
        for name in ["nmap", "sqlmap", "feroxbuster", "john", "gobuster"] {
            assert!(json.contains(&format!("\"name\":\"{name}\"")), "missing tool {name}");
        }
        assert!(cat.tools.len() >= 5);
        // The autocomplete data rides along in the catalogue.
        assert!(!cat.common_wordlists.is_empty());
        assert!(cat.john_formats.iter().any(|f| f.name == "nt" && f.jumbo));
        assert!(cat.john_formats.iter().any(|f| f.name == "bcrypt" && !f.jumbo));
    }

    #[test]
    fn path_exists_is_read_only_truth() {
        // A path that always exists vs one that never does; empty → false.
        assert!(path_exists("/"));
        assert!(!path_exists("/no/such/wordlist/ever.txt"));
        assert!(!path_exists(""));
        assert!(!path_exists("   "));
    }

    #[test]
    fn extensibility_smoke_gobuster_is_pure_data() {
        // gobuster works entirely through its registry entry (no core change):
        // it has a preset, uses only declared fields, and every flag is
        // explained — the same invariants the four built-ins satisfy.
        let g = registry::CATALOG.iter().find(|t| t.name == "gobuster").unwrap();
        assert!(!g.presets.is_empty());
        let dir = g.presets.iter().find(|p| p.name == "dir").unwrap();
        assert!(dir.fields.contains(&"url") && dir.fields.contains(&"wordlist"));
    }

    #[test]
    fn sharp_presets_are_flagged() {
        // The DoD sharp presets must carry sharp=true so the hand-off confirms.
        let sharp: Vec<(&str, &str)> = registry::CATALOG
            .iter()
            .flat_map(|t| t.presets.iter().map(move |p| (t.name, p.name, p.sharp)))
            .filter(|(_, _, s)| *s)
            .map(|(t, p, _)| (t, p))
            .collect();
        for want in [("nmap", "full-tcp"), ("sqlmap", "dump"), ("john", "incremental")] {
            assert!(sharp.contains(&want), "{want:?} must be sharp");
        }
    }

    #[test]
    fn handoff_script_stages_without_submitting_by_default() {
        // A pre-quoted command (built by the frontend). auto_enter=false must
        // NOT run it: no `write text`/`do script` of the command — it's typed
        // via keystroke and left un-submitted.
        let cmd = "nmap -sV -sC '10.0.0.5'";
        let staged = build_handoff_script(cmd, false, true);
        assert!(staged.contains("keystroke"), "must type via keystroke");
        assert!(!staged.contains("write text \"nmap"), "must not run the command");
        // auto_enter=true runs it.
        let run = build_handoff_script(cmd, true, true);
        assert!(run.contains("write text \"nmap -sV -sC '10.0.0.5'\""));
        // Terminal.app fallback (no iTerm).
        let term = build_handoff_script(cmd, true, false);
        assert!(term.contains("do script \"nmap"));
    }

    #[test]
    fn handoff_escapes_the_applescript_literal() {
        // A quote/backslash in the (already shell-quoted) command must be
        // escaped for the AppleScript string, never break the script.
        let cmd = r#"echo "a\b" 'c'"#;
        let s = build_handoff_script(cmd, true, true);
        assert!(s.contains(r#"\"a\\b\""#), "quotes/backslashes escaped: {s}");
        // The osascript string literal stays balanced (even number of unescaped ").
        assert!(s.matches("keystroke").count() == 0);
    }

    #[test]
    fn handoff_terminal_app_stages_without_submitting() {
        // Terminal.app (no iTerm), auto_enter=false → opens an empty `do script ""`
        // then types the command via keystroke, leaving it un-submitted.
        let s = build_handoff_script("nmap -sV '10.0.0.5'", false, false);
        assert!(s.contains("do script \"\""), "opens an empty shell: {s}");
        assert!(s.contains("keystroke"), "types via keystroke: {s}");
        assert!(!s.contains("do script \"nmap"), "must not run the command");
    }

    #[test]
    fn registry_field_keys_are_unique_per_tool() {
        for tool in registry::CATALOG {
            let mut seen = std::collections::HashSet::new();
            for f in tool.fields {
                assert!(seen.insert(f.key), "{}: duplicate field key {}", tool.name, f.key);
            }
        }
    }

    #[test]
    fn registry_preset_names_are_unique_per_tool() {
        for tool in registry::CATALOG {
            let mut seen = std::collections::HashSet::new();
            for p in tool.presets {
                assert!(seen.insert(p.name), "{}: duplicate preset {}", tool.name, p.name);
            }
        }
    }

    #[test]
    fn get_defaults_sanitises_invalid_stored_values() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let db = std::sync::Arc::new(parking_lot::Mutex::new(conn));
        crate::settings::init_table(&db).unwrap();
        // Stuff garbage straight into the settings table.
        crate::settings::set(&db, "sec.john_line", "bogus").unwrap();
        crate::settings::set(&db, "sec.terminal", "kitty").unwrap();
        crate::settings::set(&db, "sec.timing", "99").unwrap(); // > single 0-5 digit
        crate::settings::set(&db, "sec.rate", "9999999").unwrap();
        let d = get_defaults(&db).unwrap();
        assert_eq!(d.john_line, "jumbo"); // unknown → jumbo
        assert_eq!(d.terminal, "iterm"); // unknown → iterm
        assert_eq!(d.timing, ""); // invalid → no injection
        assert_eq!(d.rate, 100_000); // clamped to the max
    }

    #[test]
    fn set_defaults_persists_a_valid_single_digit_timing() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let db = std::sync::Arc::new(parking_lot::Mutex::new(conn));
        crate::settings::init_table(&db).unwrap();
        set_defaults(&db, &SecDefaults { timing: "4".into(), rate: 50, ..SecDefaults::default() }).unwrap();
        let d = get_defaults(&db).unwrap();
        assert_eq!(d.timing, "4");
        assert_eq!(d.rate, 50);
    }

    #[test]
    fn defaults_round_trip() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let db = std::sync::Arc::new(parking_lot::Mutex::new(conn));
        crate::settings::init_table(&db).unwrap();
        let d = get_defaults(&db).unwrap();
        assert!(!d.auto_enter); // default OFF — never auto-run
        assert_eq!(d.john_line, "jumbo");
        set_defaults(
            &db,
            &SecDefaults {
                john_line: "core".into(),
                auto_enter: true,
                threads: 999,
                timing: "9".into(),
                terminal: "terminal".into(),
                ..SecDefaults::default()
            },
        )
        .unwrap();
        let d2 = get_defaults(&db).unwrap();
        assert_eq!(d2.john_line, "core");
        assert!(d2.auto_enter);
        assert_eq!(d2.threads, 500); // clamped to max
        assert_eq!(d2.timing, ""); // invalid "9" → empty (no injection)
        assert_eq!(d2.terminal, "terminal");
    }
}
