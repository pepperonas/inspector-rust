//! `ir-record` command line: parsing, the commands, output, exit codes.
//!
//! Output: human text on stdout, errors and progress on stderr; with `--json`
//! one JSON object per line on stdout (`{"type":"progress"|"warning"|"done"|"error",…}`).
//! Exit codes: 0 ok, 1 failed, 2 usage error.

use crate::client::{self, ClientError, Conn, RemoteError, CONNECT_TIMEOUT};
use serde_json::{json, Map, Value};
use std::io::Write;
use std::time::{Duration, Instant};

pub const EXIT_OK: i32 = 0;
pub const EXIT_FAILED: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

pub const USAGE: &str = "usage: ir-record [--json] <command> [options]   (ir-record --help)";

pub const HELP: &str = "\
ir-record — drive Inspector Rust's screen recorder from the command line.

usage: ir-record [--json] <command> [options]

Projects (<project> = folder name from `list`, or an absolute path):
  list                              all recordings
  info <project>                    project and timeline
  doc <project>                     the edit document (region ids)
  export <project> [--format mp4|gif] [--quality p720|p1080|source]
         [--fps N] [--codec h264|hevc] [--gif-width N] [-o PATH]
  transcribe <project> [--lang de|en|auto]
  auto-zoom <project>               add zooms from the pointer clicks
  edit <project> --ops JSON|@file   apply edit ops (see docs/record-cli.md)
  rename <project> <title>
  delete <project> --yes

Recording:
  sources                           displays, windows, microphones, cameras
  record [--display Q | --window Q] [--mic] [--system-audio] [--camera]
         [--no-countdown] [--title T] [--duration S]
                                    without --duration, Ctrl+C stops
  stop | pause | resume | status

  mcp                               MCP server on stdio

Options:
  --json        one JSON object per line on stdout
  --help, -h    this text
  --version, -V version

Exit codes: 0 ok, 1 failed, 2 usage error.
The app is started in the background when it is not running.";

/// A parsed command line.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Help,
    Version,
    List,
    Info(String),
    Doc(String),
    Sources,
    Record(RecordArgs),
    Stop,
    Pause,
    Resume,
    Status,
    Export { project: String, params: Map<String, Value> },
    Transcribe { project: String, lang: Option<String> },
    AutoZoom(String),
    Edit { project: String, ops: Value },
    Rename { project: String, title: String },
    Delete(String),
    Mcp,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RecordArgs {
    /// `record.start` params (without `countdown`).
    pub params: Map<String, Value>,
    pub countdown: bool,
    pub duration: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub json: bool,
    pub cmd: Command,
}

/// A usage error: message for stderr, exit code 2.
#[derive(Debug, Clone, PartialEq)]
pub struct Usage(pub String);

fn usage<T>(m: impl Into<String>) -> Result<T, Usage> {
    Err(Usage(m.into()))
}

/// Options of one command: `--flag`, `--opt value` / `--opt=value`, `-o value`.
struct Args {
    positional: Vec<String>,
    opts: Vec<(String, Option<String>)>,
}

fn split(tokens: &[String], valued: &[&str], flags: &[&str]) -> Result<Args, Usage> {
    let mut a = Args { positional: Vec::new(), opts: Vec::new() };
    let mut i = 0;
    let mut only_positional = false;
    while i < tokens.len() {
        let t = &tokens[i];
        i += 1;
        if only_positional || !t.starts_with('-') || t == "-" {
            a.positional.push(t.clone());
            continue;
        }
        if t == "--" {
            only_positional = true;
            continue;
        }
        let (name, inline) = match t.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (t.clone(), None),
        };
        if flags.contains(&name.as_str()) {
            if inline.is_some() {
                return usage(format!("{name} takes no value"));
            }
            a.opts.push((name, None));
        } else if valued.contains(&name.as_str()) {
            let v = match inline {
                Some(v) => v,
                None => {
                    let Some(v) = tokens.get(i) else { return usage(format!("{name} needs a value")) };
                    i += 1;
                    v.clone()
                }
            };
            a.opts.push((name, Some(v)));
        } else {
            return usage(format!("unknown option {name}"));
        }
    }
    Ok(a)
}

impl Args {
    fn has(&self, name: &str) -> bool {
        self.opts.iter().any(|(n, _)| n == name)
    }
    fn get(&self, name: &str) -> Option<&str> {
        self.opts.iter().rev().find(|(n, _)| n == name).and_then(|(_, v)| v.as_deref())
    }
    /// Exactly `n` positionals named `names`.
    fn positionals(&self, cmd: &str, names: &[&str]) -> Result<Vec<String>, Usage> {
        if self.positional.len() < names.len() {
            return usage(format!("{cmd}: missing <{}>", names[self.positional.len()]));
        }
        if self.positional.len() > names.len() {
            return usage(format!("{cmd}: unexpected argument {}", self.positional[names.len()]));
        }
        Ok(self.positional.clone())
    }
}

fn one_of(name: &str, v: &str, allowed: &[&str]) -> Result<String, Usage> {
    if allowed.contains(&v) {
        Ok(v.to_string())
    } else {
        usage(format!("{name} must be one of {}", allowed.join(", ")))
    }
}

fn number(name: &str, v: &str) -> Result<u64, Usage> {
    v.parse::<u64>().map_err(|_| Usage(format!("{name} needs a whole number, got {v}")))
}

/// A source query: a number is an id, anything else a name substring.
fn source_query(v: &str) -> Value {
    match v.parse::<u64>() {
        Ok(n) => json!(n),
        Err(_) => json!(v),
    }
}

/// Read `--ops` (`JSON`, `@file`, `@-` = stdin) into an op list.
pub fn read_ops(arg: &str, stdin: &mut dyn FnMut() -> std::io::Result<String>) -> Result<Value, Usage> {
    let text = if let Some(path) = arg.strip_prefix('@') {
        if path == "-" {
            stdin().map_err(|e| Usage(format!("--ops @-: {e}")))?
        } else {
            std::fs::read_to_string(path).map_err(|e| Usage(format!("--ops @{path}: {e}")))?
        }
    } else {
        arg.to_string()
    };
    let v: Value = serde_json::from_str(&text).map_err(|e| Usage(format!("--ops is not JSON: {e}")))?;
    match v {
        Value::Array(a) if a.is_empty() => usage("--ops is an empty list"),
        Value::Array(_) => Ok(v),
        Value::Object(_) => Ok(Value::Array(vec![v])), // a single op
        _ => usage("--ops must be a JSON list of ops (or one op object)"),
    }
}

/// Parse the whole command line (without the program name).
pub fn parse(argv: &[String]) -> Result<Parsed, Usage> {
    parse_with(argv, &mut || {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map(|_| s)
    })
}

pub fn parse_with(
    argv: &[String],
    stdin: &mut dyn FnMut() -> std::io::Result<String>,
) -> Result<Parsed, Usage> {
    // `--json` / `--help` / `--version` are global, anywhere before `--`.
    let mut json = false;
    let mut rest = Vec::new();
    let mut seen_dashdash = false;
    for a in argv {
        if !seen_dashdash {
            match a.as_str() {
                "--json" => {
                    json = true;
                    continue;
                }
                "--help" | "-h" => return Ok(Parsed { json, cmd: Command::Help }),
                "--version" | "-V" => return Ok(Parsed { json, cmd: Command::Version }),
                "--" => seen_dashdash = true,
                _ => {}
            }
        }
        rest.push(a.clone());
    }
    let Some((name, tail)) = rest.split_first() else { return usage("missing command") };
    if name.starts_with('-') {
        return usage(format!("unknown option {name}"));
    }
    let none: &[&str] = &[];
    let cmd = match name.as_str() {
        "help" => Command::Help,
        "version" => Command::Version,
        "list" | "ls" | "projects" => {
            split(tail, none, none)?.positionals(name, &[])?;
            Command::List
        }
        "info" | "doc" | "auto-zoom" => {
            let p = split(tail, none, none)?.positionals(name, &["project"])?.remove(0);
            match name.as_str() {
                "info" => Command::Info(p),
                "doc" => Command::Doc(p),
                _ => Command::AutoZoom(p),
            }
        }
        "sources" | "stop" | "pause" | "resume" | "status" | "mcp" => {
            split(tail, none, none)?.positionals(name, &[])?;
            match name.as_str() {
                "sources" => Command::Sources,
                "stop" => Command::Stop,
                "pause" => Command::Pause,
                "resume" => Command::Resume,
                "status" => Command::Status,
                _ => Command::Mcp,
            }
        }
        "record" => {
            let a = split(
                tail,
                &["--display", "--window", "--title", "--duration"],
                &["--mic", "--system-audio", "--camera", "--no-countdown"],
            )?;
            a.positionals(name, &[])?;
            let mut params = Map::new();
            match (a.get("--display"), a.get("--window")) {
                (Some(_), Some(_)) => return usage("record: give --display or --window, not both"),
                (Some(d), None) => {
                    params.insert("display".into(), source_query(d));
                }
                (None, Some(w)) => {
                    params.insert("window".into(), source_query(w));
                }
                (None, None) => {}
            }
            for (flag, key) in [("--mic", "mic"), ("--system-audio", "systemAudio"), ("--camera", "camera")] {
                if a.has(flag) {
                    params.insert(key.into(), json!(true));
                }
            }
            if let Some(t) = a.get("--title") {
                params.insert("title".into(), json!(t));
            }
            let duration = match a.get("--duration") {
                None => None,
                Some(s) => match s.parse::<f64>() {
                    Ok(v) if v.is_finite() && v > 0.0 && v <= 24.0 * 3600.0 => Some(Duration::from_secs_f64(v)),
                    _ => return usage(format!("--duration needs seconds > 0, got {s}")),
                },
            };
            Command::Record(RecordArgs { params, countdown: !a.has("--no-countdown"), duration })
        }
        "export" => {
            let a = split(
                tail,
                &["--format", "--quality", "--fps", "--codec", "--gif-width", "-o", "--out"],
                none,
            )?;
            let project = a.positionals(name, &["project"])?.remove(0);
            let mut params = Map::new();
            if let Some(f) = a.get("--format") {
                params.insert("format".into(), json!(one_of("--format", f, &["mp4", "gif"])?));
            }
            if let Some(q) = a.get("--quality") {
                params.insert("quality".into(), json!(one_of("--quality", q, &["p720", "p1080", "source"])?));
            }
            if let Some(f) = a.get("--fps") {
                params.insert("fps".into(), json!(number("--fps", f)?));
            }
            if let Some(c) = a.get("--codec") {
                params.insert("codec".into(), json!(one_of("--codec", c, &["h264", "hevc"])?));
            }
            if let Some(w) = a.get("--gif-width") {
                params.insert("gifWidth".into(), json!(number("--gif-width", w)?));
            }
            let out = a.opts.iter().rev().find(|(n, _)| n == "-o" || n == "--out").and_then(|(_, v)| v.clone());
            if let Some(o) = out {
                params.insert("out".into(), json!(absolute(&o)));
            }
            Command::Export { project, params }
        }
        "transcribe" => {
            let a = split(tail, &["--lang"], none)?;
            let project = a.positionals(name, &["project"])?.remove(0);
            let lang = a.get("--lang").map(|l| one_of("--lang", l, &["de", "en", "auto"])).transpose()?;
            Command::Transcribe { project, lang }
        }
        "edit" => {
            let a = split(tail, &["--ops"], none)?;
            let project = a.positionals(name, &["project"])?.remove(0);
            let Some(ops) = a.get("--ops") else { return usage("edit: --ops is required") };
            Command::Edit { project, ops: read_ops(ops, stdin)? }
        }
        "rename" => {
            let a = split(tail, none, none)?;
            let mut p = a.positionals(name, &["project", "title"])?;
            let title = p.remove(1);
            if title.trim().is_empty() {
                return usage("rename: the title is empty");
            }
            Command::Rename { project: p.remove(0), title }
        }
        "delete" | "rm" => {
            let a = split(tail, none, &["--yes", "-y"])?;
            let p = a.positionals(name, &["project"])?.remove(0);
            if !a.has("--yes") && !a.has("-y") {
                return usage("delete removes the recording for good — add --yes");
            }
            Command::Delete(p)
        }
        other => return usage(format!("unknown command {other}")),
    };
    Ok(Parsed { json, cmd })
}

/// `-o` relative to the working directory (the app needs an absolute path).
fn absolute(p: &str) -> String {
    let path = std::path::Path::new(p);
    if path.is_absolute() {
        return p.to_string();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path).to_string_lossy().into_owned(),
        Err(_) => p.to_string(),
    }
}

/// The single request of a one-shot command (`None` for record / mcp / help / version).
pub fn request_of(cmd: &Command) -> Option<(&'static str, Value)> {
    Some(match cmd {
        Command::List => ("record.projects", json!({})),
        Command::Info(p) => ("record.project", json!({"project": p})),
        Command::Doc(p) => ("record.doc", json!({"project": p})),
        Command::Sources => ("record.sources", json!({})),
        Command::Stop => ("record.stop", json!({})),
        Command::Pause => ("record.pause", json!({})),
        Command::Resume => ("record.resume", json!({})),
        Command::Status => ("record.status", json!({})),
        Command::Export { project, params } => {
            let mut p = params.clone();
            p.insert("project".into(), json!(project));
            ("record.export", Value::Object(p))
        }
        Command::Transcribe { project, lang } => {
            let mut p = json!({"project": project});
            if let Some(l) = lang {
                p["lang"] = json!(l);
            }
            ("record.transcribe", p)
        }
        Command::AutoZoom(p) => ("record.edit", json!({"project": p, "ops": [{"op": "zoom.auto"}]})),
        Command::Edit { project, ops } => ("record.edit", json!({"project": project, "ops": ops})),
        Command::Rename { project, title } => ("record.rename", json!({"project": project, "title": title})),
        Command::Delete(p) => ("record.delete", json!({"project": p, "confirm": true})),
        Command::Record(_) | Command::Mcp | Command::Help | Command::Version => return None,
    })
}

/// Whether a command may start the app. Stopping / pausing a recording that
/// can't exist (app not running) should just say so.
pub fn may_launch(cmd: &Command) -> bool {
    !matches!(cmd, Command::Stop | Command::Pause | Command::Resume | Command::Status)
}

/// Sentinels that mean "you asked wrong" → exit 2; everything else → 1.
pub fn exit_code_for(code: &str) -> i32 {
    const USAGE_TAILS: &[&str] = &[
        ".bad_params",
        ".bad_op",
        ".bad_project",
        ".bad_out",
        ".bad_export",
        ".confirm_required",
        ".ambiguous",
        ".no_match",
        ".no_region",
        ".bad_clip",
        ".bad_split",
        ".unknown_effect",
        ".not_project",
    ];
    if code == "control.bad_request" || USAGE_TAILS.iter().any(|t| code.ends_with(t)) {
        EXIT_USAGE
    } else {
        EXIT_FAILED
    }
}

// ---------------------------------------------------------------------------
// Output

/// Pure: one NDJSON line for a progress event (the event's own fields, `type` first).
pub fn progress_line(ev: &Value) -> String {
    let mut o = Map::new();
    o.insert("type".into(), json!("progress"));
    match ev {
        Value::Object(m) => {
            for (k, v) in m {
                if k != "type" {
                    o.insert(k.clone(), v.clone());
                }
            }
        }
        other => {
            o.insert("event".into(), other.clone());
        }
    }
    Value::Object(o).to_string()
}

pub fn done_line(result: &Value) -> String {
    json!({"type": "done", "result": result}).to_string()
}

pub fn error_line(code: &str, message: &str) -> String {
    json!({"type": "error", "code": code, "message": message}).to_string()
}

pub fn warning_line(message: &str) -> String {
    json!({"type": "warning", "message": message}).to_string()
}

/// Pure: warnings a successful result carries (the call worked, but not all of it).
pub fn warnings_of(method: &str, r: &Value) -> Vec<String> {
    let mut w = Vec::new();
    match method {
        "record.transcribe" => {
            if let Some(e) = r.get("error").and_then(Value::as_str) {
                w.push(format!("transcription: {e}"));
            }
            if let Some(s) = r.get("skipped").and_then(Value::as_array).filter(|s| !s.is_empty()) {
                let names: Vec<String> =
                    s.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| "the recording".into())).collect();
                w.push(format!("no audio to transcribe in: {}", names.join(", ")));
            }
        }
        "record.status" | "record.start" => {
            let st = if method == "record.start" { r.get("status").unwrap_or(&Value::Null) } else { r };
            if let Some(c) = st.get("cameraWarning").and_then(Value::as_str) {
                w.push(format!("camera: {c}"));
            }
            if let Some(m) = st.get("lowDiskMinutes").and_then(Value::as_u64) {
                w.push(format!("low disk space: about {m} min of recording left"));
            }
        }
        _ => {}
    }
    w
}

/// Pure: the human text for a result.
pub fn human(method: &str, r: &Value) -> String {
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    match method {
        "record.projects" => {
            let Some(list) = r.as_array() else { return pretty(r) };
            if list.is_empty() {
                return "no recordings".into();
            }
            let rows: Vec<[String; 4]> = list
                .iter()
                .map(|p| {
                    let ms = p.get("durationMs").and_then(Value::as_u64).unwrap_or(0);
                    [s(p, "folder"), clock(ms), s(p, "status"), s(p, "title")]
                })
                .collect();
            let w0 = rows.iter().map(|r| r[0].chars().count()).max().unwrap_or(0).max(6);
            let mut out = format!("{:<w0$}  {:>8}  {:<10}  TITLE", "FOLDER", "LENGTH", "STATUS");
            for r in rows {
                out.push_str(&format!("\n{:<w0$}  {:>8}  {:<10}  {}", r[0], r[1], r[2], r[3]));
            }
            out
        }
        "record.export" => s(r, "path"),
        "record.rename" => format!("renamed to {}", s(r, "title")),
        "record.delete" => format!("deleted {}", s(r, "deleted")),
        "record.transcribe" => {
            let n = r.get("words").and_then(Value::as_u64).unwrap_or(0);
            if r.get("cancelled").and_then(Value::as_bool) == Some(true) {
                "transcription cancelled".into()
            } else {
                format!("{n} words transcribed")
            }
        }
        "record.edit" => {
            let created = r.get("created").and_then(Value::as_array).map(|c| c.len()).unwrap_or(0);
            if created == 0 {
                "edited".into()
            } else {
                let ids: Vec<String> = r["created"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|c| c.get("id").and_then(Value::as_str).map(str::to_string))
                    .collect();
                format!("edited; created {}", ids.join(", "))
            }
        }
        "record.status" | "record.pause" | "record.resume" => {
            let phase = s(r, "phase");
            let ms = r.get("elapsedMs").and_then(Value::as_u64).unwrap_or(0);
            match phase.as_str() {
                "recording" | "paused" => format!("{phase} {}", clock(ms)),
                "" => pretty(r),
                _ => phase,
            }
        }
        "record.stop" => {
            let p = s(r, "project");
            if r.get("ready").and_then(Value::as_bool) == Some(true) {
                p
            } else {
                format!("{p} ({})", s(r, "status"))
            }
        }
        _ => pretty(r),
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// `m:ss` / `h:mm:ss`.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Pure: the stderr line for a progress event in human mode.
pub fn human_progress(ev: &Value) -> Option<String> {
    match ev.get("phase").and_then(Value::as_str) {
        Some("countdown") => {
            let n = ev.get("seconds").and_then(Value::as_u64).unwrap_or(3);
            Some(format!("starting in {n} s…"))
        }
        Some(phase) => {
            let frame = ev.get("frame").and_then(Value::as_u64);
            let total = ev.get("total").and_then(Value::as_u64).filter(|t| *t > 0);
            match (frame, total) {
                (Some(f), Some(t)) => Some(format!("{phase} {f}/{t} ({} %)", (f * 100 / t).min(100))),
                _ => Some(phase.to_string()),
            }
        }
        None => None,
    }
}

/// Where output goes; real stdout/stderr in `run`, buffers in tests.
pub struct Out<'a> {
    pub json: bool,
    pub stdout: &'a mut dyn Write,
    pub stderr: &'a mut dyn Write,
    /// Whether a progress line is still open (`\r` overwriting) on stderr.
    open_line: bool,
}

impl<'a> Out<'a> {
    pub fn new(json: bool, stdout: &'a mut dyn Write, stderr: &'a mut dyn Write) -> Self {
        Out { json, stdout, stderr, open_line: false }
    }
    fn end_line(&mut self) {
        if self.open_line {
            let _ = writeln!(self.stderr);
            self.open_line = false;
        }
    }
    pub fn progress(&mut self, ev: &Value) {
        if self.json {
            let _ = writeln!(self.stdout, "{}", progress_line(ev));
            let _ = self.stdout.flush();
        } else if let Some(t) = human_progress(ev) {
            let _ = write!(self.stderr, "\r{t}\x1b[K");
            let _ = self.stderr.flush();
            self.open_line = true;
        }
    }
    pub fn info(&mut self, text: &str) {
        if !self.json {
            self.end_line();
            let _ = writeln!(self.stderr, "{text}");
        }
    }
    pub fn warning(&mut self, text: &str) {
        self.end_line();
        if self.json {
            let _ = writeln!(self.stdout, "{}", warning_line(text));
        } else {
            let _ = writeln!(self.stderr, "ir-record: warning: {text}");
        }
    }
    pub fn done(&mut self, method: &str, r: &Value) {
        self.end_line();
        for w in warnings_of(method, r) {
            self.warning(&w);
        }
        if self.json {
            let _ = writeln!(self.stdout, "{}", done_line(r));
        } else {
            let _ = writeln!(self.stdout, "{}", human(method, r));
        }
        let _ = self.stdout.flush();
    }
    /// Report an error; returns the exit code.
    pub fn error(&mut self, code: &str, message: &str, exit: i32) -> i32 {
        self.end_line();
        if self.json {
            let _ = writeln!(self.stdout, "{}", error_line(code, message));
            let _ = self.stdout.flush();
        } else {
            let _ = writeln!(self.stderr, "ir-record: {message}");
        }
        exit
    }
    fn remote(&mut self, e: &RemoteError) -> i32 {
        let msg = if e.code == client::ERR_INTERRUPTED {
            "cancelled".to_string()
        } else if e.message.is_empty() {
            e.code.clone()
        } else {
            format!("{} ({})", e.message, e.code)
        };
        self.error(&e.code, &msg, exit_code_for(&e.code))
    }
    fn client(&mut self, e: &ClientError) -> i32 {
        self.error(e.code(), &e.to_string(), EXIT_FAILED)
    }
}

// ---------------------------------------------------------------------------
// Running

/// The process entry point: parse, run, return the exit code.
pub fn run(argv: &[String]) -> i32 {
    let parsed = match parse(argv) {
        Ok(p) => p,
        Err(Usage(m)) => {
            let json = argv.iter().any(|a| a == "--json");
            if json {
                println!("{}", error_line("usage", &m));
            } else {
                eprintln!("ir-record: {m}\n{USAGE}");
            }
            return EXIT_USAGE;
        }
    };
    let (mut so, mut se) = (std::io::stdout(), std::io::stderr());
    let mut out = Out::new(parsed.json, &mut so, &mut se);
    match &parsed.cmd {
        Command::Help => {
            let _ = writeln!(out.stdout, "{HELP}");
            return EXIT_OK;
        }
        Command::Version => {
            let _ = writeln!(out.stdout, "ir-record {}", env!("CARGO_PKG_VERSION"));
            return EXIT_OK;
        }
        Command::Mcp => {
            return out.error("client.unsupported", "the MCP server is not part of this build yet", EXIT_FAILED);
        }
        _ => {}
    }
    client::install_interrupt_handler();
    let mut conn = match client::connect(may_launch(&parsed.cmd), CONNECT_TIMEOUT) {
        Ok(c) => c,
        Err(e) => return out.client(&e),
    };
    execute(&mut conn, &parsed.cmd, &mut out, &client::interrupted)
}

/// Run one command on a connection.
pub fn execute(conn: &mut Conn, cmd: &Command, out: &mut Out, stop: &dyn Fn() -> bool) -> i32 {
    if let Command::Record(args) = cmd {
        return record(conn, args, out, stop);
    }
    let Some((method, params)) = request_of(cmd) else { return EXIT_OK };
    let r = conn.call_until(method, params, &mut |ev| out.progress(&ev), stop);
    match r {
        Ok(v) => {
            out.done(method, &v);
            if method == "record.stop" && v.get("ready").and_then(Value::as_bool) == Some(false) {
                EXIT_FAILED
            } else {
                EXIT_OK
            }
        }
        Err(e) => out.remote(&e),
    }
}

/// How often `record` checks whether the take still runs in the app (stopped
/// from the control bar, app quit).
const STATUS_EVERY: Duration = Duration::from_millis(1000);

fn record(conn: &mut Conn, args: &RecordArgs, out: &mut Out, stop: &dyn Fn() -> bool) -> i32 {
    let mut params = args.params.clone();
    params.insert("countdown".into(), json!(args.countdown));
    // `record.start` itself is not interrupted: the countdown is short and the
    // take is stopped right after it if Ctrl+C came meanwhile.
    let started = match conn.call("record.start", Value::Object(params), &mut |ev| out.progress(&ev)) {
        Ok(v) => v,
        Err(e) => return out.remote(&e),
    };
    for w in warnings_of("record.start", &started) {
        out.warning(&w);
    }
    let t0 = Instant::now();
    let folder = started.get("folder").and_then(Value::as_str).unwrap_or("");
    if out.json {
        let _ = writeln!(out.stdout, "{}", progress_line(&json!({"phase": "recording", "folder": folder})));
        let _ = out.stdout.flush();
    } else {
        match args.duration {
            Some(d) => out.info(&format!("recording {folder} for {:.1} s — Ctrl+C stops earlier", d.as_secs_f64())),
            None => out.info(&format!("recording {folder} — Ctrl+C stops")),
        }
    }
    let mut last_status = Instant::now();
    loop {
        if stop() || args.duration.is_some_and(|d| t0.elapsed() >= d) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
        if last_status.elapsed() >= STATUS_EVERY {
            last_status = Instant::now();
            match conn.call("record.status", json!({}), &mut |_| {}) {
                Ok(st) => {
                    let phase = st.get("phase").and_then(Value::as_str).unwrap_or("");
                    if matches!(phase, "idle" | "picking") {
                        // Ended in the app (control bar, error). Nothing to stop.
                        let project = started.get("project").cloned().unwrap_or(Value::Null);
                        out.warning("the recording was stopped in the app");
                        out.done("record.stop", &json!({"project": project, "folder": folder, "ready": Value::Null, "status": "stopped in the app"}));
                        return EXIT_OK;
                    }
                }
                Err(e) => return out.remote(&e),
            }
        }
    }
    out.info("stopping…");
    match conn.call("record.stop", json!({}), &mut |ev| out.progress(&ev)) {
        Ok(v) => {
            out.done("record.stop", &v);
            if v.get("ready").and_then(Value::as_bool) == Some(true) {
                EXIT_OK
            } else {
                EXIT_FAILED
            }
        }
        Err(e) => out.remote(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Parsed, Usage> {
        let argv: Vec<String> = s.split_whitespace().map(str::to_string).collect();
        parse_with(&argv, &mut || Ok("[{\"op\":\"zoom.auto\"}]".into()))
    }
    fn cmd(s: &str) -> Command {
        p(s).unwrap_or_else(|e| panic!("{s}: {e:?}")).cmd
    }
    fn err(s: &str) -> String {
        p(s).err().unwrap_or_else(|| panic!("{s} parsed")).0
    }

    #[test]
    fn every_command_parses() {
        assert_eq!(cmd("list"), Command::List);
        assert_eq!(cmd("info a"), Command::Info("a".into()));
        assert_eq!(cmd("doc a"), Command::Doc("a".into()));
        assert_eq!(cmd("sources"), Command::Sources);
        assert_eq!(cmd("stop"), Command::Stop);
        assert_eq!(cmd("pause"), Command::Pause);
        assert_eq!(cmd("resume"), Command::Resume);
        assert_eq!(cmd("status"), Command::Status);
        assert_eq!(cmd("auto-zoom a"), Command::AutoZoom("a".into()));
        assert_eq!(cmd("rename a New"), Command::Rename { project: "a".into(), title: "New".into() });
        assert_eq!(cmd("delete a --yes"), Command::Delete("a".into()));
        assert_eq!(cmd("delete -y a"), Command::Delete("a".into()));
        assert_eq!(cmd("mcp"), Command::Mcp);
        assert_eq!(cmd("--help"), Command::Help);
        assert_eq!(cmd("list --help"), Command::Help);
        assert_eq!(cmd("-V"), Command::Version);
        assert_eq!(cmd("transcribe a --lang de"), Command::Transcribe { project: "a".into(), lang: Some("de".into()) });
        assert_eq!(cmd("transcribe a"), Command::Transcribe { project: "a".into(), lang: None });
        assert!(p("--json list").unwrap().json);
        assert!(p("list --json").unwrap().json);
        assert!(!p("list").unwrap().json);
    }

    #[test]
    fn record_options() {
        let Command::Record(r) = cmd("record --display 2 --mic --system-audio --camera --no-countdown --title Demo --duration 3")
        else {
            panic!()
        };
        assert_eq!(
            Value::Object(r.params),
            json!({"display": 2, "mic": true, "systemAudio": true, "camera": true, "title": "Demo"})
        );
        assert!(!r.countdown);
        assert_eq!(r.duration, Some(Duration::from_secs(3)));
        let Command::Record(r) = cmd("record --window=Safari") else { panic!() };
        assert_eq!(Value::Object(r.params), json!({"window": "Safari"}));
        assert!(r.countdown, "countdown is on unless --no-countdown");
        assert_eq!(r.duration, None);
        assert!(err("record --display 1 --window x").contains("not both"));
        assert!(err("record --duration 0").contains("--duration"));
        assert!(err("record --duration soon").contains("--duration"));
        assert!(err("record --display").contains("needs a value"));
        assert!(err("record --mic=yes").contains("takes no value"));
    }

    #[test]
    fn export_options() {
        let Command::Export { project, params } =
            cmd("export a --format gif --quality p720 --fps 15 --codec hevc --gif-width 640 -o /tmp/x.gif")
        else {
            panic!()
        };
        assert_eq!(project, "a");
        let (m, v) = request_of(&Command::Export { project, params }).unwrap();
        assert_eq!(m, "record.export");
        assert_eq!(
            v,
            json!({"project": "a", "format": "gif", "quality": "p720", "fps": 15, "codec": "hevc", "gifWidth": 640, "out": "/tmp/x.gif"})
        );
        let Command::Export { params, .. } = cmd("export a -o rel.mp4") else { panic!() };
        let out = params["out"].as_str().unwrap();
        assert!(std::path::Path::new(out).is_absolute() && out.ends_with("rel.mp4"), "{out}");
        assert!(err("export a --format webm").contains("mp4, gif"));
        assert!(err("export a --fps fast").contains("whole number"));
    }

    #[test]
    fn unknown_things_are_usage_errors() {
        assert_eq!(err("list --frobnicate"), "unknown option --frobnicate");
        assert_eq!(err("record --colour red"), "unknown option --colour");
        assert_eq!(err("export a --frobnicate"), "unknown option --frobnicate");
        assert_eq!(err("frobnicate"), "unknown command frobnicate");
        assert_eq!(err(""), "missing command");
        assert_eq!(err("--frob"), "unknown option --frob");
        assert!(err("info").contains("missing <project>"));
        assert!(err("info a b").contains("unexpected argument b"));
        assert!(err("delete a").contains("--yes"));
        assert!(err("edit a").contains("--ops"));
        assert!(err("transcribe a --lang fr").contains("de, en, auto"));
        // The exit code for all of these is 2.
        assert_eq!(run(&["list".into(), "--frobnicate".into()]), EXIT_USAGE);
        assert_eq!(run(&["delete".into(), "a".into()]), EXIT_USAGE);
    }

    #[test]
    fn edit_ops_inline_file_and_stdin() {
        let Command::Edit { ops, .. } = cmd(r#"edit a --ops [{"op":"zoom.remove","id":"z1"}]"#) else { panic!() };
        assert_eq!(ops, json!([{"op": "zoom.remove", "id": "z1"}]));
        let Command::Edit { ops, .. } = cmd(r#"edit a --ops {"op":"clip.delete","clip":0}"#) else { panic!() };
        assert_eq!(ops, json!([{"op": "clip.delete", "clip": 0}]), "one op object is wrapped");
        let f = std::env::temp_dir().join(format!("ir-ops-{}.json", std::process::id()));
        std::fs::write(&f, r#"[{"op":"clip.split","atProgrammeMs":1000}]"#).unwrap();
        let Command::Edit { ops, .. } = cmd(&format!("edit a --ops @{}", f.display())) else { panic!() };
        assert_eq!(ops, json!([{"op": "clip.split", "atProgrammeMs": 1000}]));
        let _ = std::fs::remove_file(&f);
        let Command::Edit { ops, .. } = cmd("edit a --ops @-") else { panic!() };
        assert_eq!(ops, json!([{"op": "zoom.auto"}]));
        assert!(err("edit a --ops @/nope/missing.json").contains("--ops @/nope/missing.json"));
        assert!(err("edit a --ops {broken").contains("not JSON"));
        assert!(err("edit a --ops 5").contains("list of ops"));
        assert!(err("edit a --ops []").contains("empty"));
    }

    #[test]
    fn requests_match_the_control_methods() {
        assert_eq!(request_of(&Command::List).unwrap(), ("record.projects", json!({})));
        assert_eq!(request_of(&Command::Info("a".into())).unwrap(), ("record.project", json!({"project": "a"})));
        assert_eq!(
            request_of(&Command::AutoZoom("a".into())).unwrap(),
            ("record.edit", json!({"project": "a", "ops": [{"op": "zoom.auto"}]}))
        );
        assert_eq!(
            request_of(&Command::Delete("a".into())).unwrap(),
            ("record.delete", json!({"project": "a", "confirm": true}))
        );
        assert_eq!(
            request_of(&Command::Transcribe { project: "a".into(), lang: Some("en".into()) }).unwrap(),
            ("record.transcribe", json!({"project": "a", "lang": "en"}))
        );
        assert!(request_of(&Command::Record(RecordArgs::default())).is_none());
        assert!(!may_launch(&Command::Stop) && !may_launch(&Command::Status));
        assert!(may_launch(&Command::List) && may_launch(&Command::Record(RecordArgs::default())));
    }

    #[test]
    fn exit_codes_from_sentinels() {
        assert_eq!(exit_code_for("record.control.bad_params"), EXIT_USAGE);
        assert_eq!(exit_code_for("record.control.bad_op"), EXIT_USAGE);
        assert_eq!(exit_code_for("record.control.ambiguous"), EXIT_USAGE);
        assert_eq!(exit_code_for("record.control.confirm_required"), EXIT_USAGE);
        assert_eq!(exit_code_for("record.editor.not_project"), EXIT_USAGE);
        assert_eq!(exit_code_for("control.bad_request"), EXIT_USAGE);
        assert_eq!(exit_code_for("record.busy"), EXIT_FAILED);
        assert_eq!(exit_code_for("record.permission"), EXIT_FAILED);
        assert_eq!(exit_code_for("record.unsupported"), EXIT_FAILED);
        assert_eq!(exit_code_for("control.bad_token"), EXIT_FAILED);
        assert_eq!(exit_code_for("client.interrupted"), EXIT_FAILED);
        assert_eq!(exit_code_for("record.control.helper_died"), EXIT_FAILED);
    }

    #[test]
    fn ndjson_lines() {
        let l: Value = serde_json::from_str(&progress_line(&json!({"phase": "export", "frame": 3, "total": 9}))).unwrap();
        assert_eq!(l, json!({"type": "progress", "phase": "export", "frame": 3, "total": 9}));
        // An event can't override the type.
        let l: Value = serde_json::from_str(&progress_line(&json!({"type": "done"}))).unwrap();
        assert_eq!(l["type"], "progress");
        let l: Value = serde_json::from_str(&progress_line(&json!(5))).unwrap();
        assert_eq!(l, json!({"type": "progress", "event": 5}));
        assert_eq!(
            serde_json::from_str::<Value>(&done_line(&json!({"path": "/x"}))).unwrap(),
            json!({"type": "done", "result": {"path": "/x"}})
        );
        assert_eq!(
            serde_json::from_str::<Value>(&error_line("record.busy", "busy")).unwrap(),
            json!({"type": "error", "code": "record.busy", "message": "busy"})
        );
        assert_eq!(serde_json::from_str::<Value>(&warning_line("w")).unwrap(), json!({"type": "warning", "message": "w"}));
    }

    #[test]
    fn json_output_is_one_object_per_line() {
        let (mut so, mut se) = (Vec::new(), Vec::new());
        {
            let mut out = Out::new(true, &mut so, &mut se);
            out.progress(&json!({"phase": "export", "frame": 1, "total": 2}));
            out.done("record.transcribe", &json!({"words": 0, "error": "no model", "skipped": [null]}));
            out.error("record.busy", "busy", 1);
        }
        let text = String::from_utf8(so).unwrap();
        let types: Vec<String> = text
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap()["type"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(types, ["progress", "warning", "warning", "done", "error"]);
        assert!(se.is_empty(), "nothing on stderr in --json mode");
    }

    #[test]
    fn human_texts() {
        let list = json!([
            {"folder": "2026-11-04 10-00-00", "title": "Demo", "durationMs": 65_000, "status": "ready"},
        ]);
        let t = human("record.projects", &list);
        assert!(t.starts_with("FOLDER"));
        assert!(t.contains("2026-11-04 10-00-00") && t.contains("1:05") && t.contains("Demo"));
        assert_eq!(human("record.projects", &json!([])), "no recordings");
        assert_eq!(human("record.export", &json!({"path": "/x.mp4"})), "/x.mp4");
        assert_eq!(human("record.status", &json!({"phase": "recording", "elapsedMs": 3_723_000})), "recording 1:02:03");
        assert_eq!(human("record.status", &json!({"phase": "idle", "elapsedMs": 0})), "idle");
        assert_eq!(human_progress(&json!({"phase": "export", "frame": 50, "total": 200})).unwrap(), "export 50/200 (25 %)");
        assert_eq!(human_progress(&json!({"phase": "countdown", "seconds": 3})).unwrap(), "starting in 3 s…");
        assert_eq!(
            warnings_of("record.status", &json!({"cameraWarning": "lost", "lowDiskMinutes": 4})),
            vec!["camera: lost".to_string(), "low disk space: about 4 min of recording left".to_string()]
        );
    }
}

#[cfg(all(test, unix))]
mod flow_tests {
    use super::*;
    use crate::test_support::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    fn conn(d: &Td, counter: Arc<AtomicUsize>) -> Conn {
        fake_server(&d.0, counter);
        client::connect_with(&d.0, false, CONNECT_TIMEOUT, &|_| panic!("no launch")).unwrap()
    }

    #[test]
    fn record_with_duration_stops_by_itself() {
        let d = td();
        let n = Arc::new(AtomicUsize::new(0));
        let mut c = conn(&d, n.clone());
        let (mut so, mut se) = (Vec::new(), Vec::new());
        let args = RecordArgs { duration: Some(Duration::from_millis(300)), countdown: true, ..Default::default() };
        let t = Instant::now();
        let code = execute(&mut c, &Command::Record(args), &mut Out::new(true, &mut so, &mut se), &|| false);
        assert_eq!(code, EXIT_OK);
        assert!(t.elapsed() >= Duration::from_millis(300));
        assert_eq!(n.load(Ordering::SeqCst), 1000, "exactly one record.stop");
        let types: Vec<String> = String::from_utf8(so)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap()["type"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(types, ["progress", "progress", "done"]); // countdown, recording, done
    }

    #[test]
    fn ctrl_c_stops_a_recording_without_duration() {
        let d = td();
        let n = Arc::new(AtomicUsize::new(0));
        let mut c = conn(&d, n.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500)); // past one status poll
            s2.store(true, Ordering::SeqCst);
        });
        let (mut so, mut se) = (Vec::new(), Vec::new());
        let code = execute(
            &mut c,
            &Command::Record(RecordArgs { countdown: true, ..Default::default() }),
            &mut Out::new(false, &mut so, &mut se),
            &|| stop.load(Ordering::SeqCst),
        );
        assert_eq!(code, EXIT_OK);
        assert_eq!(n.load(Ordering::SeqCst), 1000);
        assert_eq!(String::from_utf8(so).unwrap().trim(), "/r/take");
    }

    #[test]
    fn interrupting_an_export_exits_1_and_closes_the_connection() {
        let d = td();
        let n = Arc::new(AtomicUsize::new(0));
        let mut c = conn(&d, n.clone());
        let hit = AtomicBool::new(false);
        let (mut so, mut se) = (Vec::new(), Vec::new());
        // t.slow stands in for record.export: one event, then waits for the close.
        let mut out = Out::new(true, &mut so, &mut se);
        let r = c.call_until("t.slow", json!({}), &mut |ev| {
            out.progress(&ev);
            hit.store(true, Ordering::SeqCst);
        }, &|| hit.load(Ordering::SeqCst));
        let code = out.remote(&r.unwrap_err());
        assert_eq!(code, EXIT_FAILED);
        let t = Instant::now();
        while n.load(Ordering::SeqCst) == 0 && t.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(n.load(Ordering::SeqCst), 1, "the app saw the connection close");
        let last = String::from_utf8(so).unwrap().lines().last().unwrap().to_string();
        assert_eq!(serde_json::from_str::<Value>(&last).unwrap()["code"], client::ERR_INTERRUPTED);
    }

    #[test]
    fn remote_errors_map_to_exit_codes() {
        let d = td();
        let mut c = conn(&d, Arc::default());
        let (mut so, mut se) = (Vec::new(), Vec::new());
        // The fake answers record.busy for t.fail, unknown_method for others.
        let mut out = Out::new(false, &mut so, &mut se);
        let e = c.call("t.fail", json!({}), &mut |_| {}).unwrap_err();
        assert_eq!(out.remote(&e), EXIT_FAILED);
        assert!(String::from_utf8(se).unwrap().contains("busy (record.busy)"));
    }
}
