//! Running an approved script: written to a private file, started with the
//! interpreter for its language, killed (with everything it spawned) when it
//! outlives its time limit, output captured and capped.

use super::generate::Language;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Captured output kept per run (stdout and stderr together).
pub const OUTPUT_CAP: usize = 16 * 1024;
/// What a reader thread buffers at most before it starts discarding.
const READ_CAP: usize = 256 * 1024;
pub const DEFAULT_TIMEOUT_S: u32 = 120;
pub const MAX_TIMEOUT_S: u32 = 3600;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunOutcome {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: i64,
    pub output: String,
}

impl RunOutcome {
    pub fn ok(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }
}

/// The first existing path among `candidates`, else the bare name (PATH lookup).
fn first_existing(candidates: &[&str], fallback: &str) -> String {
    candidates
        .iter()
        .find(|p| std::path::Path::new(p).is_file())
        .map(|p| p.to_string())
        .unwrap_or_else(|| fallback.to_string())
}

/// Program + leading arguments for a language; the script path is appended.
pub fn interpreter(lang: Language) -> (String, Vec<String>) {
    match lang {
        Language::Zsh => (first_existing(&["/bin/zsh", "/usr/bin/zsh"], "zsh"), vec![]),
        Language::Bash => (first_existing(&["/bin/bash", "/usr/bin/bash"], "bash"), vec![]),
        // Prefer a real Python over macOS' /usr/bin/python3, which is only an
        // installer stub when the Command Line Tools are missing.
        Language::Python => (
            first_existing(&["/opt/homebrew/bin/python3", "/usr/local/bin/python3", "/usr/bin/python3"], "python3"),
            vec![],
        ),
        Language::AppleScript => ("/usr/bin/osascript".into(), vec![]),
        Language::PowerShell => (
            "powershell".into(),
            vec!["-NoProfile".into(), "-NonInteractive".into(), "-ExecutionPolicy".into(), "Bypass".into(), "-File".into()],
        ),
    }
}

/// A PATH that also finds Homebrew tools — a GUI app inherits a bare PATH.
pub fn script_path_env(current: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if cfg!(target_os = "macos") {
        parts.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(String::from));
    }
    if let Some(cur) = current {
        for p in cur.split(if cfg!(windows) { ';' } else { ':' }) {
            if !p.is_empty() && !parts.iter().any(|x| x == p) {
                parts.push(p.to_string());
            }
        }
    }
    if !cfg!(windows) {
        for p in ["/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
            if !parts.iter().any(|x| x == p) {
                parts.push(p.to_string());
            }
        }
    }
    parts.join(if cfg!(windows) { ";" } else { ":" })
}

/// stdout then stderr, each labelled when both have content, capped at `cap`
/// bytes on a char boundary. The END of a long output is kept — that's where
/// the error usually is.
pub fn combine_output(stdout: &[u8], stderr: &[u8], cap: usize) -> String {
    let out = String::from_utf8_lossy(stdout);
    let err = String::from_utf8_lossy(stderr);
    let joined = match (out.trim().is_empty(), err.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => out.into_owned(),
        (true, false) => err.into_owned(),
        (false, false) => format!("{out}\n── stderr ──\n{err}"),
    };
    if joined.len() <= cap {
        return joined;
    }
    let mut start = joined.len() - cap;
    while !joined.is_char_boundary(start) {
        start += 1;
    }
    format!("… (gekürzt)\n{}", &joined[start..])
}

fn script_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("InspectorRust").join("ai-tasks")
}

/// Run `script` in `lang` with `env`, at most `timeout`.
pub fn run_script(lang: Language, script: &str, env: &[(String, String)], timeout: Duration) -> Result<RunOutcome, String> {
    let dir = script_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Ordner für Skripte: {e}"))?;
    let file = dir.join(format!(
        "task-{}-{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        lang.extension()
    ));
    std::fs::write(&file, script).map_err(|e| format!("Skript schreiben: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700));
    }
    let (program, mut args) = interpreter(lang);
    args.push(file.to_string_lossy().into_owned());

    let mut cmd = Command::new(&program);
    cmd.args(&args)
        .current_dir(dirs::home_dir().unwrap_or_else(std::env::temp_dir))
        .env("PATH", script_path_env(std::env::var("PATH").ok().as_deref()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group, so a timeout can stop everything the script started.
        cmd.process_group(0);
    }
    let started = Instant::now();
    let result = cmd.spawn().map_err(|e| format!("„{program}“ ließ sich nicht starten: {e}"));
    let outcome = result.and_then(|child| {
        match wait_with_timeout(child, timeout) {
            Ok(out) => Ok(RunOutcome {
                exit_code: out.status.code(),
                timed_out: false,
                duration_ms: started.elapsed().as_millis() as i64,
                output: combine_output(&out.stdout, &out.stderr, OUTPUT_CAP),
            }),
            Err(TimeoutOr::Timeout(partial)) => {
                Ok(RunOutcome {
                    exit_code: None,
                    timed_out: true,
                    duration_ms: started.elapsed().as_millis() as i64,
                    output: combine_output(&partial.stdout, &partial.stderr, OUTPUT_CAP),
                })
            }
            Err(TimeoutOr::Io(e)) => Err(e),
        }
    });
    let _ = std::fs::remove_file(&file);
    outcome
}

#[cfg(unix)]
fn kill_group(pid: u32) {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    // Negative pid = the whole process group; 9 = SIGKILL.
    unsafe {
        kill(-(pid as i32), 9);
    }
}
#[cfg(not(unix))]
fn kill_group(_pid: u32) {}

pub enum TimeoutOr {
    Timeout(Output),
    Io(String),
}

impl std::fmt::Display for TimeoutOr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimeoutOr::Timeout(_) => write!(f, "Zeitlimit überschritten"),
            TimeoutOr::Io(e) => write!(f, "{e}"),
        }
    }
}

fn reader<R: Read + Send + 'static>(mut r: R) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match r.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    // Keep draining (a full pipe would block the child) but stop growing.
                    if buf.len() < READ_CAP {
                        buf.extend_from_slice(&chunk[..n.min(READ_CAP - buf.len())]);
                    }
                }
            }
        }
        buf
    })
}

/// Wait for `child`, reading its pipes on threads (reading after `wait` can
/// deadlock on a full pipe). On timeout the child is killed and what it
/// printed so far is returned.
pub fn wait_with_timeout(mut child: Child, timeout: Duration) -> Result<Output, TimeoutOr> {
    let out = child.stdout.take().map(reader);
    let err = child.stderr.take().map(reader);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() >= deadline => {
                // The whole group first: a grandchild holding the pipe open
                // would otherwise keep the readers below waiting.
                kill_group(child.id());
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(TimeoutOr::Io(e.to_string())),
        }
    };
    let stdout = out.and_then(|h| h.join().ok()).unwrap_or_default();
    let stderr = err.and_then(|h| h.join().ok()).unwrap_or_default();
    match status {
        Some(status) => Ok(Output { status, stdout, stderr }),
        None => {
            let status = std::process::ExitStatus::default();
            Err(TimeoutOr::Timeout(Output { status, stdout, stderr }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_keeps_both_streams_and_labels_stderr_only_when_needed() {
        assert_eq!(combine_output(b"ok\n", b"", 100), "ok\n");
        assert_eq!(combine_output(b"", b"boom", 100), "boom");
        assert!(combine_output(b"a", b"b", 100).contains("── stderr ──"));
        assert_eq!(combine_output(b"  ", b"\n", 100), "");
    }

    #[test]
    fn long_output_keeps_the_end_on_a_char_boundary() {
        let s = format!("{}ENDE", "ä".repeat(100));
        let c = combine_output(s.as_bytes(), b"", 21);
        assert!(c.starts_with("… (gekürzt)"));
        assert!(c.ends_with("ENDE"), "the error is usually at the end");
    }

    #[test]
    fn the_script_path_finds_homebrew_and_keeps_the_system_dirs() {
        let p = script_path_env(Some("/custom/bin:/usr/bin"));
        if cfg!(target_os = "macos") {
            assert!(p.starts_with("/opt/homebrew/bin:/usr/local/bin:"));
        }
        assert!(p.contains("/custom/bin"));
        assert_eq!(p.matches("/usr/bin").count(), 1, "no duplicates");
        if !cfg!(windows) {
            assert!(p.contains("/bin") && p.contains("/sbin"));
        }
    }

    #[test]
    fn interpreters_exist_for_every_language() {
        for l in [Language::Zsh, Language::Bash, Language::Python, Language::AppleScript, Language::PowerShell] {
            let (prog, _) = interpreter(l);
            assert!(!prog.is_empty());
        }
        assert!(interpreter(Language::PowerShell).1.contains(&"-NonInteractive".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn a_script_runs_with_the_task_environment() {
        let env = vec![("IR_TASK_NAME".to_string(), "T".to_string())];
        let o = run_script(Language::Bash, "echo \"hi $IR_TASK_NAME\"; exit 3\n", &env, Duration::from_secs(10)).unwrap();
        assert_eq!(o.exit_code, Some(3));
        assert_eq!(o.output.trim(), "hi T");
        assert!(!o.ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_runaway_script_is_killed_with_its_children() {
        let marker = std::env::temp_dir().join(format!("ir-task-orphan-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        // The child sleeps past the limit and would then write the marker.
        let script = format!("echo started\n(sleep 3; touch '{}') &\nsleep 30\n", marker.display());
        let o = run_script(Language::Bash, &script, &[], Duration::from_millis(600)).unwrap();
        assert!(o.timed_out);
        assert_eq!(o.output.trim(), "started");
        std::thread::sleep(Duration::from_secs(4));
        assert!(!marker.exists(), "a child of the script outlived the time limit");
    }
}
