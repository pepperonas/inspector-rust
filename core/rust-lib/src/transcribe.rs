//! Local audio/video → sibling TXT, selected in Finder or passed explicitly.
//! The embedded Python worker loads Whisper once per batch; no shell involved.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const RUNNER: &str = include_str!("../assets/transcribe/runner.py");
const EXTENSIONS: &[&str] = &[
    "mp4", "mp3", "m4a", "wav", "aac", "flac", "ogg", "opus", "webm", "mov", "mkv", "avi",
];

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub path: Option<String>,
    pub language: Option<String>,
}

/// Preserve literal path spaces/backslashes. Only a quote at a token's start
/// opens a quoted token (an apostrophe in `martin's video.mp4` is literal).
pub fn parse_args(arg: &str) -> Result<Args, String> {
    let mut tokens = Vec::new();
    let mut chars = arg.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        let quoted = ch == '"' || ch == '\'';
        let end = if quoted {
            let mut closing = None;
            for (i, c) in chars.by_ref() {
                if c == ch {
                    closing = Some(i + c.len_utf8());
                    break;
                }
            }
            let end = closing.ok_or("Nicht geschlossenes Anführungszeichen im Pfad")?;
            if chars.peek().is_some_and(|(_, c)| !c.is_whitespace()) {
                return Err("Nach einem zitierten Pfad muss ein Leerzeichen stehen".into());
            }
            end
        } else {
            let mut end = start + ch.len_utf8();
            while let Some(&(i, c)) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                chars.next();
                end = i + c.len_utf8();
            }
            end
        };
        tokens.push((start, end, quoted));
    }
    let mut result = Args::default();
    let mut path_tokens = Vec::new();
    let mut language_seen = false;
    let mut flags_done = false;
    let mut i = 0;
    while i < tokens.len() {
        let (start, end, quoted) = tokens[i];
        let token = &arg[start..end];
        if !quoted && !flags_done && token == "--" {
            flags_done = true;
        } else if !quoted
            && !flags_done
            && (token == "--language" || token == "-l" || token.starts_with("--language="))
        {
            if language_seen {
                return Err("Sprache nur einmal angeben".into());
            }
            language_seen = true;
            let value = if let Some(value) = token.strip_prefix("--language=") {
                value
            } else {
                i += 1;
                let &(s, e, _) = tokens.get(i).ok_or("Sprachvorgabe fehlt: --language de")?;
                &arg[s..e]
            };
            let value = value.trim_matches(['"', '\'']).to_lowercase();
            if value.is_empty() || !value.chars().all(|c| c.is_ascii_alphabetic()) {
                return Err(
                    "Ungültige Sprachvorgabe — z. B. --language de oder --language auto".into(),
                );
            }
            result.language = (value != "auto").then_some(value);
        } else if !quoted && !flags_done && token.starts_with('-') {
            return Err(format!(
                "Unbekannter Parameter: {token}. Verwende --language de."
            ));
        } else {
            path_tokens.push(i);
        }
        i += 1;
    }
    if let (Some(&first), Some(&last)) = (path_tokens.first(), path_tokens.last()) {
        if path_tokens.len() != last - first + 1 {
            return Err("Sprachparameter vor oder nach dem Pfad angeben".into());
        }
        if path_tokens.len() > 1 && path_tokens.iter().any(|&i| tokens[i].2) {
            return Err(
                "Bitte genau einen Pfad angeben oder mehrere Dateien im Finder markieren".into(),
            );
        }
        let path = arg[tokens[first].0..tokens[last].1].to_string();
        if path.trim_matches(['"', '\'']).is_empty() {
            return Err("Leerer Dateipfad".into());
        }
        result.path = Some(path);
    }
    Ok(result)
}

fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

pub fn choose_inputs(
    path: Option<PathBuf>,
    selection: Result<Vec<PathBuf>, String>,
    is_file: impl Fn(&Path) -> bool,
) -> Result<Vec<PathBuf>, String> {
    if let Some(path) = path {
        if !supported(&path) {
            return Err(format!(
                "Keine unterstützte Audio-/Videodatei: {}",
                path.display()
            ));
        }
        if !is_file(&path) {
            return Err(format!("Datei nicht gefunden: {}", path.display()));
        }
        return Ok(vec![path]);
    }
    let files: Vec<_> = selection?
        .into_iter()
        .filter(|p| supported(p) && is_file(p))
        .collect();
    if files.is_empty() {
        return Err("Keine Audio-/Videodatei im Finder ausgewählt — Datei markieren oder transcribe <pfad> angeben".into());
    }
    Ok(files)
}

pub fn environment_python() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("InspectorRust/transcribe/venv")
        .join(if cfg!(windows) {
            "Scripts/python.exe"
        } else {
            "bin/python3"
        })
}

fn python() -> PathBuf {
    if let Some(path) = std::env::var_os("IR_WHISPER_PYTHON") {
        return PathBuf::from(path);
    }
    let dedicated = environment_python();
    if dedicated.is_file() {
        return dedicated;
    }
    if cfg!(target_os = "macos") {
        for path in ["/opt/homebrew/bin/python3", "/usr/local/bin/python3"] {
            if Path::new(path).is_file() {
                return PathBuf::from(path);
            }
        }
    }
    PathBuf::from(if cfg!(windows) { "python" } else { "python3" })
}

pub struct Worker {
    python: PathBuf,
    path_env: std::ffi::OsString,
    request: Vec<u8>,
}

impl Worker {
    pub fn prepare(paths: &[PathBuf], language: Option<&str>) -> Result<Self, String> {
        let ffmpeg = crate::screen_record::ffmpeg_path()
            .ok_or("ffmpeg fehlt — bitte installieren (macOS: brew install ffmpeg)")?;
        let mut path_dirs = vec![ffmpeg.parent().unwrap_or(Path::new(".")).to_path_buf()];
        if let Some(current) = std::env::var_os("PATH") {
            path_dirs.extend(std::env::split_paths(&current));
        }
        let path_env = std::env::join_paths(path_dirs).map_err(|e| e.to_string())?;
        let worker = Self {
            python: python(),
            path_env,
            request: serde_json::to_vec(
                &serde_json::json!({ "paths": paths, "language": language }),
            )
            .map_err(|e| format!("Dateipfade: {e}"))?,
        };
        worker.invoke(true)?;
        Ok(worker)
    }

    fn invoke(&self, check: bool) -> Result<Vec<u8>, String> {
        let mut cmd = Command::new(&self.python);
        cmd.args(["-c", RUNNER])
            .env("PATH", &self.path_env)
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if check {
            cmd.arg("--check");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().map_err(|e| format!(
            "Python/Whisper nicht verfügbar: {e}. Einmal python3 scripts/setup-transcribe.py ausführen; alternativ IR_WHISPER_PYTHON setzen."
        ))?;
        let write_result = child
            .stdin
            .take()
            .ok_or("Whisper-Eingabe nicht verfügbar")?
            .write_all(&self.request);
        let output = child
            .wait_with_output()
            .map_err(|e| format!("Whisper: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        write_result.map_err(|e| format!("Whisper-Eingabe: {e}"))?;
        Ok(output.stdout)
    }

    pub fn run(&self) -> Result<Summary, String> {
        serde_json::from_slice(&self.invoke(false)?).map_err(|e| format!("Whisper-Antwort: {e}"))
    }
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct Summary {
    pub outputs: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
}

impl Summary {
    pub fn message(&self) -> String {
        let success = match self.outputs.as_slice() {
            [path] => format!(
                "{} erstellt",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            outputs => format!("{} Transkripte erstellt", outputs.len()),
        };
        if let Some((path, reason)) = self.failed.first() {
            format!(
                "{success}; {} fehlgeschlagen: {} — {reason}",
                self.failed.len(),
                path.file_name().unwrap_or_default().to_string_lossy(),
                reason = user_error(reason)
            )
        } else {
            success
        }
    }
}

pub fn user_error(error: &str) -> String {
    error
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Transkription fehlgeschlagen")
        .chars()
        .take(220)
        .collect()
}

pub fn notify_completion(message: &str) {
    #[cfg(target_os = "macos")]
    {
        // Message is argv data, including quotes/newlines in filenames.
        let _ = Command::new("/usr/bin/osascript")
            .args(["-e", "on run argv\ndisplay notification (item 1 of argv) with title \"Inspector Rust\" subtitle \"Audio/Video → Text\"\nend run", message])
            .stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = message;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_and_language_only() {
        assert_eq!(parse_args("").unwrap(), Args::default());
        assert_eq!(
            parse_args("--language DE").unwrap(),
            Args {
                language: Some("de".into()),
                path: None
            }
        );
        assert_eq!(parse_args("-l auto").unwrap(), Args::default());
        assert_eq!(
            parse_args("--language=english")
                .unwrap()
                .language
                .as_deref(),
            Some("english")
        );
    }

    #[test]
    fn preserves_spaces_unicode_and_windows_paths() {
        for path in [
            "~/Videos/Grüße  heute.mp4",
            r"C:\My Videos\clip.MP4",
            "/tmp/martin's video.mp4",
        ] {
            let args = parse_args(&format!("{path} --language de")).unwrap();
            assert_eq!(args.path.as_deref(), Some(path));
            assert_eq!(args.language.as_deref(), Some("de"));
        }
        let args = parse_args("-l en \"~/video --language de.mp4\"").unwrap();
        assert_eq!(args.path.as_deref(), Some("\"~/video --language de.mp4\""));
        assert_eq!(args.language.as_deref(), Some("en"));
        assert_eq!(
            parse_args("-- -clip.mp4").unwrap().path.as_deref(),
            Some("-clip.mp4")
        );
    }

    #[test]
    fn rejects_malformed_flags_and_multiple_paths() {
        for arg in [
            "--language",
            "--language=",
            "-l --bad",
            "--foo de",
            "-l de --language en",
            "-l auto -l de",
            "\"unfinished",
            "\"\"",
            "\"a.mp4\" \"b.mp4\"",
            "my -l de video.mp4",
        ] {
            assert!(parse_args(arg).is_err(), "{arg}");
        }
    }

    #[test]
    fn explicit_input_wins_and_errors_do_not_fall_back() {
        let video = PathBuf::from("/v.MP4");
        assert_eq!(
            choose_inputs(Some(video.clone()), Err("denied".into()), |_| true).unwrap(),
            vec![video]
        );
        assert!(choose_inputs(
            Some("/missing.mp4".into()),
            Ok(vec!["/other.mp4".into()]),
            |_| false
        )
        .unwrap_err()
        .contains("nicht gefunden"));
        assert!(choose_inputs(Some("/notes.md".into()), Ok(vec![]), |_| true).is_err());
    }

    #[test]
    fn selection_filters_media_and_propagates_permission_errors() {
        let selection = ["a.mp4", "b.mp3", "c.MOV", "d.txt", "gone.wav"];
        assert_eq!(
            choose_inputs(
                None,
                Ok(selection.iter().map(PathBuf::from).collect()),
                |p| p != Path::new("gone.wav")
            )
            .unwrap(),
            vec![PathBuf::from("a.mp4"), "b.mp3".into(), "c.MOV".into()]
        );
        assert_eq!(
            choose_inputs(None, Err("denied".into()), |_| true).unwrap_err(),
            "denied"
        );
        assert!(choose_inputs(None, Ok(vec![]), |_| true)
            .unwrap_err()
            .contains("transcribe <pfad>"));
    }

    #[test]
    fn summary_names_outputs_and_partial_failures() {
        let mut summary = Summary {
            outputs: vec!["/clip 2.txt".into()],
            failed: vec![],
        };
        assert_eq!(summary.message(), "clip 2.txt erstellt");
        summary
            .failed
            .push(("/broken.mp4".into(), "Keine Tonspur".into()));
        assert!(summary
            .message()
            .contains("1 fehlgeschlagen: broken.mp4 — Keine Tonspur"));
    }

    #[test]
    fn hud_errors_keep_last_line_and_cap_unicode_safely() {
        assert_eq!(
            user_error("decoder details\nNo audio track\n"),
            "No audio track"
        );
        assert_eq!(user_error(&"ü".repeat(300)).chars().count(), 220);
        assert_eq!(user_error("\n"), "Transkription fehlgeschlagen");
    }
}
