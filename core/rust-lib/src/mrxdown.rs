//! Markdown → PDF conversion via the user-installed `mrxdown` CLI.
//!
//! mrxdown is an Electron app (`/Applications/MrxDown.app`) shipping a
//! `/usr/local/bin/mrxdown` shell wrapper. Calling `mrxdown <file.md>`
//! writes `<file>.pdf` next to the source — exactly the layout we want,
//! so we just pass the path through and let mrxdown handle the output
//! convention.
//!
//! The hotkey (`Ctrl+Shift+M`, registered in `hotkey::register`) reads
//! the current Finder selection via [`crate::finder_selection::read`],
//! filters to `.md`/`.markdown` extensions, and runs `mrxdown` once per
//! file. We resolve via `PATH` rather than hard-coding
//! `/usr/local/bin/mrxdown` so a user with a custom install location
//! (`~/bin/mrxdown`, Homebrew prefix, …) still works.
//!
//! Conversions run **serially** in the hotkey's worker thread —
//! mrxdown spawns Electron per invocation (~150 MB RSS, ~1-3 s per
//! file). Parallel would invite RAM spikes + races on writing the
//! output side-by-side. Serial with a single summary notification is
//! the right trade-off.

use std::path::{Path, PathBuf};
use std::process::Command;

const MD_EXTENSIONS: &[&str] = &["md", "markdown"];

/// Result of a batch conversion call. `skipped` covers Finder
/// selections that aren't markdown (PNG, folder, …) — we don't treat
/// them as errors, just filter them out + report the count.
/// `mrxdown_missing` is the special "mrxdown isn't installed" state:
/// every md file goes into `failed` (so callers can still iterate),
/// but the notification surfaces a single actionable message instead
/// of N×"fehlgeschlagen".
#[derive(Debug, Default)]
pub struct ConvertSummary {
    pub converted: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
    pub mrxdown_missing: bool,
}

/// Convert every markdown file in `paths` to a sibling PDF. Non-md
/// paths land in `skipped`; mrxdown failures land in `failed` with
/// the trimmed stderr as the reason. Never panics or short-circuits —
/// one failed file doesn't stop the rest.
///
/// **No-mrxdown path:** if `mrxdown_available()` returns false (PATH
/// scan), we skip the loop entirely and flag every md file as failed
/// with `"mrxdown nicht installiert"` + set the dedicated flag so the
/// notification can render one clean message. The hotkey still works
/// in this case — the user just gets a "install mrxdown" pointer
/// instead of silence.
pub fn convert_files(paths: &[PathBuf]) -> ConvertSummary {
    let mut summary = ConvertSummary::default();
    let mut md_files = Vec::new();
    for p in paths {
        if is_markdown(p) {
            md_files.push(p.clone());
        } else {
            summary.skipped.push(p.clone());
        }
    }

    if md_files.is_empty() {
        return summary;
    }

    if !mrxdown_available() {
        summary.mrxdown_missing = true;
        for p in md_files {
            summary.failed.push((p, "mrxdown nicht installiert".to_string()));
        }
        return summary;
    }

    for p in md_files {
        match run_mrxdown(&p) {
            Ok(()) => summary.converted.push(p),
            Err(e) => summary.failed.push((p, e)),
        }
    }
    summary
}

/// PATH-scan check for an `mrxdown` executable. Cheap (no spawn) so
/// it's safe to call once per hotkey press as a pre-flight. On
/// Windows we also probe common executable extensions since the user
/// might have installed `mrxdown.cmd` / `mrxdown.exe`.
fn mrxdown_available() -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    let extensions: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    for dir in std::env::split_paths(&path_var) {
        for ext in extensions {
            let candidate = if ext.is_empty() {
                dir.join("mrxdown")
            } else {
                dir.join(format!("mrxdown{ext}"))
            };
            if candidate.is_file() {
                return true;
            }
        }
    }
    false
}

fn is_markdown(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|ext| MD_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Shell out to `mrxdown <path>`. mrxdown writes `<path>.pdf` next to
/// the source by replacing `.md`/`.markdown` with `.pdf`; we just
/// fire-and-wait for the process to exit.
fn run_mrxdown(path: &Path) -> Result<(), String> {
    let output = Command::new("mrxdown")
        .arg(path)
        .output()
        .map_err(|e| {
            // The most common cause: mrxdown not in PATH. Surface that
            // explicitly so the user knows what to fix.
            format!("mrxdown nicht aufrufbar (PATH? installiert?): {e}")
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // mrxdown's CLI mode logs PDF-generation errors to stdout
        // ("PDF erstellt: …" on success, error text on failure), so
        // include both streams in the surfaced message.
        let combined = match (stderr.is_empty(), stdout.is_empty()) {
            (true, true) => format!("exit code {:?}", output.status.code()),
            (true, false) => stdout,
            (false, true) => stderr,
            (false, false) => format!("{stderr} | {stdout}"),
        };
        return Err(combined);
    }
    Ok(())
}

/// Native notification + sound for a completed conversion batch.
/// Called from the `Ctrl+Shift+M` hotkey worker thread.
pub fn notify(summary: &ConvertSummary) {
    let msg = build_notification_message(summary);
    notify_visual(&msg);
    if !summary.failed.is_empty() {
        notify_audio_failure();
    } else if !summary.converted.is_empty() {
        notify_audio_success();
    }
}

/// One-line user-facing summary. Localised in German to match the
/// existing timer.rs notification copy. Public for unit tests.
pub fn build_notification_message(summary: &ConvertSummary) -> String {
    let total = summary.converted.len() + summary.skipped.len() + summary.failed.len();
    if total == 0 {
        return "Keine Dateien selektiert".to_string();
    }
    // Distinct "tool missing" message — actionable (tells the user
    // what to install), short, doesn't mention per-file counts that
    // would just confuse.
    if summary.mrxdown_missing {
        return "mrxdown ist nicht installiert — siehe https://github.com/pepperonas/mrxdown".to_string();
    }
    if summary.converted.is_empty() && summary.failed.is_empty() {
        return format!(
            "Keine Markdown-Dateien in der Selektion ({n} übersprungen)",
            n = summary.skipped.len()
        );
    }
    let mut parts = Vec::new();
    if !summary.converted.is_empty() {
        parts.push(format!("{} konvertiert", summary.converted.len()));
    }
    if !summary.skipped.is_empty() {
        parts.push(format!("{} übersprungen", summary.skipped.len()));
    }
    if !summary.failed.is_empty() {
        parts.push(format!("{} fehlgeschlagen", summary.failed.len()));
    }
    parts.join(", ")
}

#[cfg(target_os = "macos")]
fn notify_visual(msg: &str) {
    // Mirrors timer.rs::notify_visual — single-quote-escape the
    // message so the user's filename can't break the AppleScript
    // string literal.
    let safe = msg.replace('"', "'").replace('\\', "/");
    let script = format!(
        r#"display notification "{safe}" with title "Inspector Rust" subtitle "Markdown → PDF""#
    );
    let _ = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(&script)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(not(target_os = "macos"))]
fn notify_visual(_msg: &str) {
    // Win/Linux native notifications: not in v1. mrxdown isn't
    // typically installed there anyway; if a user provides it, the
    // conversion runs silently + the log shows the summary.
}

#[cfg(target_os = "macos")]
fn notify_audio_success() {
    let _ = Command::new("/usr/bin/afplay")
        .arg("/System/Library/Sounds/Glass.aiff")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(target_os = "macos")]
fn notify_audio_failure() {
    let _ = Command::new("/usr/bin/afplay")
        .arg("/System/Library/Sounds/Funk.aiff")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(not(target_os = "macos"))]
fn notify_audio_success() {}

#[cfg(not(target_os = "macos"))]
fn notify_audio_failure() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_markdown_accepts_md_and_markdown_case_insensitive() {
        assert!(is_markdown(Path::new("foo.md")));
        assert!(is_markdown(Path::new("foo.MD")));
        assert!(is_markdown(Path::new("foo.markdown")));
        assert!(is_markdown(Path::new("foo.Markdown")));
        assert!(is_markdown(Path::new("/tmp/path with space.md")));
    }

    #[test]
    fn is_markdown_rejects_non_md() {
        assert!(!is_markdown(Path::new("foo.txt")));
        assert!(!is_markdown(Path::new("foo.pdf")));
        assert!(!is_markdown(Path::new("foo")));
        assert!(!is_markdown(Path::new("README"))); // no extension
        assert!(!is_markdown(Path::new("foo.md.bak")));
    }

    #[test]
    fn empty_summary_says_nothing_selected() {
        let s = ConvertSummary::default();
        assert_eq!(build_notification_message(&s), "Keine Dateien selektiert");
    }

    #[test]
    fn only_non_md_says_nothing_to_convert() {
        let s = ConvertSummary {
            skipped: vec!["a.png".into(), "b.txt".into()],
            ..Default::default()
        };
        assert_eq!(
            build_notification_message(&s),
            "Keine Markdown-Dateien in der Selektion (2 übersprungen)"
        );
    }

    #[test]
    fn pure_success_reports_count() {
        let s = ConvertSummary {
            converted: vec!["a.md".into(), "b.md".into(), "c.md".into()],
            ..Default::default()
        };
        assert_eq!(build_notification_message(&s), "3 konvertiert");
    }

    #[test]
    fn mixed_summary_lists_all_buckets() {
        let s = ConvertSummary {
            converted: vec!["a.md".into()],
            skipped: vec!["b.png".into()],
            failed: vec![("c.md".into(), "EACCES".into())],
            ..Default::default()
        };
        assert_eq!(
            build_notification_message(&s),
            "1 konvertiert, 1 übersprungen, 1 fehlgeschlagen"
        );
    }

    #[test]
    fn convert_files_filters_non_markdown_without_running_mrxdown() {
        // Files don't exist on disk + extensions are non-md → all skipped.
        // mrxdown is never invoked (the function never reaches run_mrxdown),
        // so this test doesn't require mrxdown to be installed.
        let paths = vec![
            PathBuf::from("/nonexistent/a.png"),
            PathBuf::from("/nonexistent/b.txt"),
        ];
        let summary = convert_files(&paths);
        assert_eq!(summary.converted.len(), 0);
        assert_eq!(summary.skipped.len(), 2);
        assert_eq!(summary.failed.len(), 0);
        assert!(!summary.mrxdown_missing);
    }

    #[test]
    fn missing_mrxdown_message_is_actionable() {
        let s = ConvertSummary {
            mrxdown_missing: true,
            failed: vec![("a.md".into(), "mrxdown nicht installiert".into())],
            ..Default::default()
        };
        let msg = build_notification_message(&s);
        assert!(msg.contains("mrxdown ist nicht installiert"));
        // Don't surface per-file count when it's the global tool-missing case.
        assert!(!msg.contains("fehlgeschlagen"));
    }
}
