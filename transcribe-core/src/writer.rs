//! The crash-safe transcript writer.
//!
//! Every final segment is appended to `transkript.txt` and `transkript.jsonl`
//! and synced (`flush` + `sync_data`) before the next one. Files are opened
//! per append, so a folder that vanished or became unwritable is noticed at
//! once. On a write error the session moves to a fresh folder under the
//! fallback root (once) and reports a warning; if that fails too, the error is
//! fatal and the pipeline stops — never a silently missing transcript.

use crate::protocol::session::{
    self, free_session_dir, session_dir_name, txt_line, write_meta_atomic, Line, SessionMeta,
    SessionStatus,
};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq)]
pub enum WriteError {
    Fatal(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Fatal(e) => write!(f, "{e}"),
        }
    }
}

pub struct Writer {
    dir: PathBuf,
    fallback_root: PathBuf,
    meta: SessionMeta,
    keep_audio: bool,
    seq: u64,
    fallback: bool,
    warnings: Vec<String>,
    offset_s: i32,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn local_offset_s() -> i32 {
    chrono::Local::now().offset().local_minus_utc()
}

fn create_session(root: &Path, meta: &SessionMeta) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let started = chrono::DateTime::from_timestamp_millis(meta.started_ms as i64)
        .unwrap_or_default()
        .with_timezone(&chrono::Local)
        .naive_local();
    let dir = free_session_dir(root, &session_dir_name(started));
    fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_meta_atomic(&dir, meta).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

fn append_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = OpenOptions::new().append(true).create(true).open(path)?;
    f.write_all(bytes)?;
    f.flush()?;
    f.sync_data()
}

fn write_wav(path: &Path, samples: &[f32]) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())
}

impl Writer {
    /// Create the session folder under `root`, else under `fallback_root`.
    pub fn open(root: &Path, fallback_root: &Path, mut meta: SessionMeta, keep_audio: bool) -> Result<Writer, WriteError> {
        let mut warnings = Vec::new();
        let (dir, fallback) = match create_session(root, &meta) {
            Ok(d) => (d, false),
            Err(e) => {
                meta.fallback_from = Some(root.to_path_buf());
                let d = create_session(fallback_root, &meta)
                    .map_err(|e2| WriteError::Fatal(format!("{e}; fallback: {e2}")))?;
                warnings.push(format!("transcript folder not writable ({e}) — writing to {}", d.display()));
                (d, true)
            }
        };
        Ok(Writer {
            dir,
            fallback_root: fallback_root.to_path_buf(),
            meta,
            keep_audio,
            seq: 0,
            fallback,
            warnings,
            offset_s: local_offset_s(),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn used_fallback(&self) -> bool {
        self.fallback
    }

    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    fn write_into(&self, dir: &Path, line: &Line, audio: Option<&[f32]>) -> Result<(), String> {
        let json = serde_json::to_string(line).map_err(|e| e.to_string())? + "\n";
        append_file(&dir.join(session::JSONL), json.as_bytes()).map_err(|e| e.to_string())?;
        append_file(&dir.join(session::TXT), txt_line(line, self.offset_s).as_bytes()).map_err(|e| e.to_string())?;
        if let (true, Some(a)) = (self.keep_audio, audio) {
            let adir = dir.join(session::AUDIO_DIR);
            // Audio is a convenience: its failure never stops the transcript.
            if fs::create_dir_all(&adir).is_ok() {
                let _ = write_wav(&adir.join(format!("{:06}-{}.wav", self.seq, line.source.slug())), a);
            }
        }
        Ok(())
    }

    /// Append one final segment. Moves to the fallback root on the first
    /// failure; a second failure is fatal.
    pub fn append(&mut self, line: &Line, audio: Option<&[f32]>) -> Result<(), WriteError> {
        self.seq += 1;
        let first = match self.write_into(&self.dir, line, audio) {
            Ok(()) => return Ok(()),
            Err(e) => e,
        };
        if self.fallback {
            return Err(WriteError::Fatal(format!("{}: {first}", self.dir.display())));
        }
        let mut meta = self.meta.clone();
        meta.fallback_from = Some(self.dir.clone());
        meta.note = Some(format!("continued here after: {first}"));
        let dir = create_session(&self.fallback_root, &meta)
            .map_err(|e| WriteError::Fatal(format!("{first}; fallback: {e}")))?;
        self.warnings.push(format!("cannot write {} ({first}) — continuing in {}", self.dir.display(), dir.display()));
        self.dir = dir;
        self.meta = meta;
        self.fallback = true;
        self.write_into(&self.dir, line, audio)
            .map_err(|e| WriteError::Fatal(format!("{}: {e}", self.dir.display())))
    }

    /// Mark the session done; returns its folder.
    pub fn finish(mut self, status: SessionStatus) -> Result<PathBuf, WriteError> {
        self.meta.status = status;
        self.meta.ended_ms = Some(now_ms());
        write_meta_atomic(&self.dir, &self.meta)
            .map_err(|e| WriteError::Fatal(format!("{}: {e}", self.dir.display())))?;
        Ok(self.dir)
    }
}

pub enum WriterMsg {
    Line(Line, Option<Vec<f32>>),
    Finish(SessionStatus),
}

#[derive(Debug, Clone, PartialEq)]
pub enum WriterReport {
    Written,
    Warning(String),
    Fatal(String),
    Finished(PathBuf),
}

/// Run a writer on its own thread: the pipeline never waits for the disk.
pub fn spawn_writer(mut w: Writer) -> (mpsc::Sender<WriterMsg>, mpsc::Receiver<WriterReport>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<WriterMsg>();
    let (rtx, rrx) = mpsc::channel();
    let h = std::thread::Builder::new()
        .name("ir-transcribe-writer".into())
        .spawn(move || {
            for wmsg in w.take_warnings() {
                let _ = rtx.send(WriterReport::Warning(wmsg));
            }
            for msg in rx {
                match msg {
                    WriterMsg::Line(line, audio) => {
                        let r = w.append(&line, audio.as_deref());
                        for wmsg in w.take_warnings() {
                            let _ = rtx.send(WriterReport::Warning(wmsg));
                        }
                        match r {
                            Ok(()) => {
                                let _ = rtx.send(WriterReport::Written);
                            }
                            Err(WriteError::Fatal(e)) => {
                                let _ = rtx.send(WriterReport::Fatal(e));
                                return;
                            }
                        }
                    }
                    WriterMsg::Finish(status) => {
                        let _ = rtx.send(match w.finish(status) {
                            Ok(d) => WriterReport::Finished(d),
                            Err(WriteError::Fatal(e)) => WriterReport::Fatal(e),
                        });
                        return;
                    }
                }
            }
            // Sender dropped without Finish: the session ended abruptly. The
            // status stays `running`; recovery marks it at the next start.
        })
        .expect("spawn writer thread");
    (tx, rrx, h)
}

/// Delete raw audio older than `keep_days`; transcripts are never touched.
pub fn prune_audio(root: &Path, keep_days: u32, now: SystemTime) {
    let limit = Duration::from_secs(u64::from(keep_days) * 86_400);
    let Ok(sessions) = fs::read_dir(root) else { return };
    for s in sessions.flatten() {
        let adir = s.path().join(session::AUDIO_DIR);
        let Ok(files) = fs::read_dir(&adir) else { continue };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("wav") {
                continue;
            }
            let old = f
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .is_some_and(|age| age > limit);
            if old {
                let _ = fs::remove_file(&p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::session::{read_lines, read_meta, recover};
    use crate::protocol::{Lang, Mode, Source};

    fn meta() -> SessionMeta {
        SessionMeta {
            version: 1,
            started_ms: now_ms(),
            ended_ms: None,
            status: SessionStatus::Running,
            model: "m".into(),
            lang: Lang::Auto,
            sources: vec![Source::Mic, Source::Sys],
            mode: Mode::Meeting,
            fallback_from: None,
            note: None,
        }
    }

    fn line(i: u64, src: Source) -> Line {
        Line {
            start_ms: i * 1000,
            end_ms: i * 1000 + 800,
            source: src,
            lang: "de".into(),
            confidence: 0.8,
            text: format!("Satz {i}"),
        }
    }

    fn roots() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let main = t.path().join("Transkripte");
        let fb = t.path().join("fallback");
        (t, main, fb)
    }

    #[test]
    fn append_writes_txt_and_jsonl() {
        let (_t, main, fb) = roots();
        let mut w = Writer::open(&main, &fb, meta(), false).unwrap();
        w.append(&line(1, Source::Mic), None).unwrap();
        w.append(&line(2, Source::Sys), None).unwrap();
        let txt = fs::read_to_string(w.dir().join(session::TXT)).unwrap();
        assert_eq!(txt.lines().count(), 2);
        assert!(txt.lines().next().unwrap().ends_with("] MIC: Satz 1"));
        assert!(txt.lines().nth(1).unwrap().ends_with("] SYS: Satz 2"));
        assert_eq!(read_lines(w.dir()).len(), 2);
        let dir = w.finish(SessionStatus::Completed).unwrap();
        assert_eq!(read_meta(&dir).unwrap().status, SessionStatus::Completed);
    }

    #[test]
    fn kill_after_append_keeps_everything() {
        let (_t, main, fb) = roots();
        let mut w = Writer::open(&main, &fb, meta(), false).unwrap();
        for i in 1..=3 {
            w.append(&line(i, Source::Mic), None).unwrap();
        }
        std::mem::forget(w); // the process dies here
        let r = recover(&main);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].lines, 3);
        assert_eq!(r[0].ended_ms, Some(3800));
    }

    #[cfg(unix)]
    #[test]
    fn unwritable_root_falls_back() {
        use std::os::unix::fs::PermissionsExt;
        let (_t, main, fb) = roots();
        fs::create_dir_all(&main).unwrap();
        fs::set_permissions(&main, fs::Permissions::from_mode(0o500)).unwrap();
        let mut w = Writer::open(&main, &fb, meta(), false).unwrap();
        fs::set_permissions(&main, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(w.used_fallback());
        assert!(w.dir().starts_with(&fb));
        assert_eq!(read_meta(w.dir()).unwrap().fallback_from, Some(main.clone()));
        assert_eq!(w.take_warnings().len(), 1);
    }

    #[test]
    fn folder_gone_midway_moves_to_fallback_and_warns() {
        let (_t, main, fb) = roots();
        let mut w = Writer::open(&main, &fb, meta(), false).unwrap();
        w.append(&line(1, Source::Mic), None).unwrap();
        let first = w.dir().to_path_buf();
        fs::remove_dir_all(&first).unwrap(); // the folder disappears
        w.append(&line(2, Source::Mic), None).unwrap();
        assert!(w.used_fallback());
        assert!(w.dir().starts_with(&fb));
        assert_eq!(read_lines(w.dir()).len(), 1);
        let warn = w.take_warnings();
        assert_eq!(warn.len(), 1, "{warn:?}");
    }

    #[test]
    fn both_roots_broken_is_fatal() {
        let (t, _main, _fb) = roots();
        // A regular file where a folder must go: nothing can be created.
        let blocker = t.path().join("blocker");
        fs::write(&blocker, b"x").unwrap();
        let r = Writer::open(&blocker.join("a"), &blocker.join("b"), meta(), false);
        assert!(matches!(r, Err(WriteError::Fatal(_))));
    }

    #[test]
    fn failure_in_the_fallback_is_fatal() {
        let (_t, main, fb) = roots();
        let mut w = Writer::open(&main, &fb, meta(), false).unwrap();
        fs::remove_dir_all(w.dir()).unwrap();
        w.append(&line(1, Source::Mic), None).unwrap(); // moved to fallback
        fs::remove_dir_all(w.dir()).unwrap();
        assert!(matches!(w.append(&line(2, Source::Mic), None), Err(WriteError::Fatal(_))));
    }

    #[test]
    fn wav_per_segment_is_complete() {
        let (_t, main, fb) = roots();
        let mut w = Writer::open(&main, &fb, meta(), true).unwrap();
        let audio: Vec<f32> = (0..16_000).map(|i| (i as f32 / 50.0).sin() * 0.5).collect();
        w.append(&line(1, Source::Sys), Some(&audio)).unwrap();
        let wav = w.dir().join("audio").join("000001-sys.wav");
        let r = hound::WavReader::open(&wav).unwrap();
        assert_eq!(r.spec().sample_rate, 16_000);
        assert_eq!(r.len(), 16_000);
    }

    #[test]
    fn prune_keeps_transcripts() {
        let (_t, main, fb) = roots();
        let mut w = Writer::open(&main, &fb, meta(), true).unwrap();
        w.append(&line(1, Source::Mic), Some(&[0.0; 160])).unwrap();
        let dir = w.dir().to_path_buf();
        let later = SystemTime::now() + Duration::from_secs(8 * 86_400);
        prune_audio(&main, 7, later);
        assert!(!dir.join("audio").join("000001-mic.wav").exists());
        assert!(dir.join(session::TXT).exists());
        prune_audio(&main, 7, SystemTime::now()); // nothing old: nothing happens
    }

    #[test]
    fn writer_thread_reports_and_finishes() {
        let (_t, main, fb) = roots();
        let w = Writer::open(&main, &fb, meta(), false).unwrap();
        let (tx, rx, h) = spawn_writer(w);
        tx.send(WriterMsg::Line(line(1, Source::Mic), None)).unwrap();
        tx.send(WriterMsg::Finish(SessionStatus::Completed)).unwrap();
        h.join().unwrap();
        let reports: Vec<WriterReport> = rx.iter().collect();
        assert_eq!(reports[0], WriterReport::Written);
        assert!(matches!(reports[1], WriterReport::Finished(_)));
    }
}
