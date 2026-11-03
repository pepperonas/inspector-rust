//! File mode: an audio or video file → a sibling TXT (`name.txt`, then
//! `name 2.txt` …, never overwriting). Audio is decoded by Core Audio
//! (`ExtAudioFile`: wav, m4a, mp3, aac, flac, mp4 audio …) and, for
//! containers Core Audio can't open (mov, mkv, webm, ogg …), by `ffmpeg` when
//! it is installed. The samples go through the same VAD, segmenter and final
//! decode as a live session, streamed — a two-hour file never sits in memory.

use crate::decode::DecodeOpts;
use crate::pipeline::Transcriber;
use crate::protocol::{Lang, Source};
use crate::queue::JobKind;
use crate::protocol::Word;
use crate::segment::{Seg, Segmenter, MAX_SEG, RATE, VAD_FRAME};
use crate::text::{drop_loops, normalize};
use crate::vad::Vad;
use crate::words::{filter_words, words_from_tokens, WordTranscriber};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const ERR_DECODE: &str = "transcribe.file_decode";
pub const ERR_NEEDS_FFMPEG: &str = "transcribe.file_needs_ffmpeg";
pub const ERR_CANCELLED: &str = "transcribe.file_cancelled";
pub const ERR_TOO_MANY: &str = "transcribe.file_too_many";
/// Samples read per step (one second).
pub const CHUNK: usize = RATE;
/// Same rule as the live writer: at most two identical lines in a row.
pub const MAX_REPEAT: usize = 2;
const PROMPT_CHARS: usize = 200;

/// A decoded audio stream, 16 kHz mono f32.
pub trait AudioSource {
    /// Length in ms, 0 when unknown.
    fn total_ms(&self) -> u64;
    /// Append up to `CHUNK` samples; `Ok(false)` at the end.
    fn read(&mut self, out: &mut Vec<f32>) -> Result<bool, String>;
}

/// In-memory source (tests, already decoded audio).
pub struct Samples {
    data: Vec<f32>,
    pos: usize,
}

impl Samples {
    pub fn new(data: Vec<f32>) -> Self {
        Samples { data, pos: 0 }
    }
}

impl AudioSource for Samples {
    fn total_ms(&self) -> u64 {
        self.data.len() as u64 * 1000 / RATE as u64
    }
    fn read(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
        if self.pos >= self.data.len() {
            return Ok(false);
        }
        let end = (self.pos + CHUNK).min(self.data.len());
        out.extend_from_slice(&self.data[self.pos..end]);
        self.pos = end;
        Ok(true)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileText {
    pub text: String,
    /// The language most of the speech was in (`de`/`en`), if any.
    pub lang: Option<String>,
}

/// Read, VAD and segment a whole stream; `on_seg(seg, samples)` runs for every
/// speech segment (the last one flushed at the end). `progress(done_ms,
/// total_ms)` runs after every segment and once at the end (at 100 %).
fn drive(
    src: &mut dyn AudioSource,
    vad: &mut dyn Vad,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
    mut on_seg: impl FnMut(Seg, &[f32]) -> Result<(), String>,
) -> Result<(), String> {
    let total = src.total_ms();
    let mut seg = Segmenter::new(MAX_SEG);
    let mut pending: Vec<f32> = Vec::new();
    let mut read_samples: u64 = 0;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(ERR_CANCELLED.into());
        }
        let before = pending.len();
        let more = src.read(&mut pending)?;
        read_samples += (pending.len() - before) as u64;
        let n = pending.len() / VAD_FRAME * VAD_FRAME;
        if n > 0 || !more {
            let frames: Vec<f32> = pending.drain(..n).collect();
            let probs = vad.probs(&frames)?;
            for (s, samples) in seg.push(&frames, &probs) {
                if cancel.load(Ordering::SeqCst) {
                    return Err(ERR_CANCELLED.into());
                }
                on_seg(s, &samples)?;
                progress(read_samples * 1000 / RATE as u64, total);
            }
        }
        if !more {
            break;
        }
    }
    if let Some((s, samples)) = seg.flush() {
        on_seg(s, &samples)?;
    }
    let done = read_samples * 1000 / RATE as u64;
    progress(done, total.max(done));
    Ok(())
}

/// Count `n` characters of text for `lang`; the language with the most wins.
fn tally(langs: &mut Vec<(String, usize)>, lang: &str, n: usize) {
    match langs.iter_mut().find(|(l, _)| l == lang) {
        Some((_, c)) => *c += n,
        None => langs.push((lang.to_string(), n)),
    }
}

/// The last `PROMPT_CHARS` of everything so far, the next decode's prompt.
fn next_prompt(tail: &str, text: &str) -> String {
    let joined = format!("{tail} {text}");
    let start = joined.char_indices().rev().nth(PROMPT_CHARS).map(|(i, _)| i).unwrap_or(0);
    joined[start..].trim().to_string()
}

/// Decode, segment and transcribe a whole stream. `progress(done_ms, total_ms)`
/// runs after every segment and at the end.
pub fn transcribe_stream(
    src: &mut dyn AudioSource,
    vad: &mut dyn Vad,
    tr: &mut dyn Transcriber,
    lang: Lang,
    threads: usize,
    cancel: &AtomicBool,
    progress: impl FnMut(u64, u64),
) -> Result<FileText, String> {
    let mut lines: Vec<String> = Vec::new();
    let mut langs: Vec<(String, usize)> = Vec::new();
    let mut tail = String::new();
    let mut last_lang: Option<String> = None;

    drive(src, vad, cancel, progress, |_, samples| {
        let opts = DecodeOpts { kind: JobKind::Final, lang, last_lang: last_lang.clone(), prompt: tail.clone(), threads };
        let d = tr.decode(Source::Mic, samples, &opts)?;
        if d.text.is_empty() {
            return Ok(());
        }
        tally(&mut langs, &d.lang, d.text.chars().count());
        last_lang = Some(d.lang.clone());
        tail = next_prompt(&tail, &d.text);
        lines.push(d.text);
        Ok(())
    })?;
    let lang = langs.into_iter().max_by_key(|(_, n)| *n).map(|(l, _)| l);
    let lines = drop_loops(lines, MAX_REPEAT);
    // Drop exact repeats Whisper sometimes produces across a segment cut.
    let mut kept: Vec<String> = Vec::new();
    for l in lines {
        if kept.last().is_some_and(|p| normalize(p) == normalize(&l)) {
            continue;
        }
        kept.push(l);
    }
    Ok(FileText { text: kept.join("\n"), lang })
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileWords {
    pub words: Vec<Word>,
    /// The language most of the speech was in (`de`/`en`), if any.
    pub lang: Option<String>,
}

fn samples_to_ms(n: u64) -> u64 {
    n * 1000 / RATE as u64
}

/// Like `transcribe_stream`, but every word with its time in ms of the file:
/// each segment's own start (from the segmenter) is the offset its token
/// times are added to.
pub fn transcribe_words_stream(
    src: &mut dyn AudioSource,
    vad: &mut dyn Vad,
    tr: &mut dyn WordTranscriber,
    lang: Lang,
    threads: usize,
    cancel: &AtomicBool,
    progress: impl FnMut(u64, u64),
) -> Result<FileWords, String> {
    let mut words: Vec<Word> = Vec::new();
    let mut langs: Vec<(String, usize)> = Vec::new();
    let mut tail = String::new();
    let mut last_lang: Option<String> = None;

    drive(src, vad, cancel, progress, |s, samples| {
        let opts = DecodeOpts { kind: JobKind::Final, lang, last_lang: last_lang.clone(), prompt: tail.clone(), threads };
        let d = tr.decode_tokens(samples, &opts)?;
        let ws = words_from_tokens(samples_to_ms(s.start), samples_to_ms(s.end), &d.tokens);
        if ws.is_empty() {
            return Ok(());
        }
        let text = ws.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        tally(&mut langs, &d.lang, text.chars().count());
        last_lang = Some(d.lang.clone());
        tail = next_prompt(&tail, &text);
        words.extend(ws);
        Ok(())
    })?;
    let lang = langs.into_iter().max_by_key(|(_, n)| *n).map(|(l, _)| l);
    Ok(FileWords { words: filter_words(words), lang })
}

/// `name.txt`, `name 2.txt`, … — the first that doesn't exist yet (same rule as
/// before; gaps are reused).
pub fn txt_candidates(source: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "transcript".into());
    (1..=10_000).map(move |n| {
        let name = if n == 1 { format!("{stem}.txt") } else { format!("{stem} {n}.txt") };
        source.with_file_name(name)
    })
}

/// Create the TXT exclusively (a parallel run can't overwrite it) and write
/// the text with a trailing newline.
pub fn write_txt(source: &Path, text: &str) -> Result<PathBuf, String> {
    for target in txt_candidates(source) {
        let mut f = match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", target.display())),
        };
        let body = format!("{}\n", text.trim());
        if let Err(e) = f.write_all(body.as_bytes()).and_then(|_| f.sync_data()) {
            let _ = std::fs::remove_file(&target);
            return Err(format!("{}: {e}", target.display()));
        }
        return Ok(target);
    }
    Err(ERR_TOO_MANY.into())
}

/// Mix interleaved frames down to mono.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect()
}

// --- ffmpeg fallback ---------------------------------------------------------

pub fn ffmpeg() -> Option<PathBuf> {
    for p in ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg", "/usr/bin/ffmpeg"] {
        if Path::new(p).is_file() {
            return Some(PathBuf::from(p));
        }
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).map(|d| d.join("ffmpeg")).find(|p| p.is_file())
    })
}

/// The argv for ffmpeg: 16 kHz mono f32 on stdout. A relative path starting
/// with `-` would be read as an option, so it gets a `./`.
pub fn ffmpeg_args(path: &Path) -> Vec<std::ffi::OsString> {
    let input = if path.is_relative() && path.to_string_lossy().starts_with('-') {
        Path::new(".").join(path)
    } else {
        path.to_path_buf()
    };
    let mut a: Vec<std::ffi::OsString> =
        ["-nostdin", "-hide_banner", "-loglevel", "error", "-i"].iter().map(Into::into).collect();
    a.push(input.into_os_string());
    for s in ["-vn", "-ac", "1", "-ar", "16000", "-f", "f32le", "pipe:1"] {
        a.push(s.into());
    }
    a
}

pub struct FfmpegSource {
    child: std::process::Child,
    out: std::process::ChildStdout,
    rest: Vec<u8>,
    total_ms: u64,
}

impl FfmpegSource {
    pub fn open(path: &Path, total_ms: u64) -> Result<Self, String> {
        let bin = ffmpeg().ok_or(ERR_NEEDS_FFMPEG)?;
        let mut child = std::process::Command::new(bin)
            .args(ffmpeg_args(path))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("ffmpeg: {e}"))?;
        let out = child.stdout.take().ok_or("ffmpeg stdout")?;
        Ok(FfmpegSource { child, out, rest: Vec::new(), total_ms })
    }
}

impl AudioSource for FfmpegSource {
    fn total_ms(&self) -> u64 {
        self.total_ms
    }
    fn read(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
        use std::io::Read;
        let mut buf = vec![0u8; CHUNK * 4];
        let n = self.out.read(&mut buf).map_err(|e| format!("ffmpeg: {e}"))?;
        if n == 0 {
            let status = self.child.wait().map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(ERR_DECODE.into());
            }
            return Ok(false);
        }
        self.rest.extend_from_slice(&buf[..n]);
        let whole = self.rest.len() / 4 * 4;
        out.extend(self.rest[..whole].as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)));
        self.rest.drain(..whole);
        Ok(true)
    }
}

impl Drop for FfmpegSource {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Open `path` for transcription: Core Audio first, ffmpeg for the rest.
pub fn open_audio(path: &Path) -> Result<Box<dyn AudioSource>, String> {
    #[cfg(target_os = "macos")]
    match core_audio::ExtSource::open(path) {
        Ok(s) => return Ok(Box::new(s)),
        Err(e) => tracing::info!("transcribe file: Core Audio can't read {}: {e}; trying ffmpeg", path.display()),
    }
    let s = FfmpegSource::open(path, 0)?;
    Ok(Box::new(s))
}

#[cfg(target_os = "macos")]
mod core_audio {
    use super::*;
    use std::ffi::c_void;

    type ExtAudioFileRef = *mut c_void;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Asbd {
        sample_rate: f64,
        format_id: u32,
        format_flags: u32,
        bytes_per_packet: u32,
        frames_per_packet: u32,
        bytes_per_frame: u32,
        channels_per_frame: u32,
        bits_per_channel: u32,
        reserved: u32,
    }

    #[repr(C)]
    struct AudioBuffer {
        channels: u32,
        size: u32,
        data: *mut c_void,
    }

    #[repr(C)]
    struct AudioBufferList {
        count: u32,
        buffers: [AudioBuffer; 1],
    }

    const PROP_CLIENT_FORMAT: u32 = u32::from_be_bytes(*b"cfmt");
    const PROP_FILE_FORMAT: u32 = u32::from_be_bytes(*b"ffmt");
    const PROP_LENGTH_FRAMES: u32 = u32::from_be_bytes(*b"#frm");
    const LPCM: u32 = u32::from_be_bytes(*b"lpcm");
    const FLOAT_PACKED: u32 = 1 | 8;

    #[link(name = "AudioToolbox", kind = "framework")]
    extern "C" {
        fn ExtAudioFileOpenURL(url: *const c_void, out: *mut ExtAudioFileRef) -> i32;
        fn ExtAudioFileGetProperty(f: ExtAudioFileRef, id: u32, size: *mut u32, data: *mut c_void) -> i32;
        fn ExtAudioFileSetProperty(f: ExtAudioFileRef, id: u32, size: u32, data: *const c_void) -> i32;
        fn ExtAudioFileRead(f: ExtAudioFileRef, frames: *mut u32, list: *mut AudioBufferList) -> i32;
        fn ExtAudioFileDispose(f: ExtAudioFileRef) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            alloc: *const c_void,
            buf: *const u8,
            len: isize,
            is_dir: u8,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    pub struct ExtSource {
        file: ExtAudioFileRef,
        channels: usize,
        total_ms: u64,
        buf: Vec<f32>,
    }

    // SAFETY: the handle is only used from the thread that owns the source.
    unsafe impl Send for ExtSource {}

    impl ExtSource {
        pub fn open(path: &Path) -> Result<Self, String> {
            use std::os::unix::ffi::OsStrExt;
            let bytes = path.as_os_str().as_bytes();
            // SAFETY: the buffer outlives the call; a null URL is checked.
            let url = unsafe {
                CFURLCreateFromFileSystemRepresentation(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize, 0)
            };
            if url.is_null() {
                return Err(ERR_DECODE.into());
            }
            let mut file: ExtAudioFileRef = std::ptr::null_mut();
            // SAFETY: `url` is a valid CFURL; `file` receives the handle.
            let st = unsafe { ExtAudioFileOpenURL(url, &mut file) };
            // SAFETY: we own the URL reference from the Create call.
            unsafe { CFRelease(url) };
            if st != 0 || file.is_null() {
                return Err(format!("ExtAudioFileOpenURL {st}"));
            }
            let mut src = ExtSource { file, channels: 1, total_ms: 0, buf: Vec::new() };
            let mut ff = Asbd::default();
            let mut size = std::mem::size_of::<Asbd>() as u32;
            // SAFETY: `ff` is a properly sized ASBD.
            let st = unsafe { ExtAudioFileGetProperty(file, PROP_FILE_FORMAT, &mut size, &mut ff as *mut _ as *mut c_void) };
            if st != 0 || ff.channels_per_frame == 0 || ff.sample_rate <= 0.0 {
                return Err(format!("file format {st}"));
            }
            let mut frames: i64 = 0;
            let mut size = std::mem::size_of::<i64>() as u32;
            // SAFETY: the property is an SInt64.
            let st = unsafe { ExtAudioFileGetProperty(file, PROP_LENGTH_FRAMES, &mut size, &mut frames as *mut _ as *mut c_void) };
            if st == 0 && frames > 0 {
                src.total_ms = (frames as f64 * 1000.0 / ff.sample_rate) as u64;
            }
            let channels = ff.channels_per_frame.min(8);
            let client = Asbd {
                sample_rate: RATE as f64,
                format_id: LPCM,
                format_flags: FLOAT_PACKED,
                bytes_per_packet: 4 * channels,
                frames_per_packet: 1,
                bytes_per_frame: 4 * channels,
                channels_per_frame: channels,
                bits_per_channel: 32,
                reserved: 0,
            };
            // SAFETY: `client` is a valid ASBD for the call.
            let st = unsafe {
                ExtAudioFileSetProperty(file, PROP_CLIENT_FORMAT, std::mem::size_of::<Asbd>() as u32, &client as *const _ as *const c_void)
            };
            if st != 0 {
                return Err(format!("client format {st}"));
            }
            src.channels = channels as usize;
            src.buf = vec![0.0; CHUNK * src.channels];
            Ok(src)
        }
    }

    impl AudioSource for ExtSource {
        fn total_ms(&self) -> u64 {
            self.total_ms
        }
        fn read(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
            let mut frames = CHUNK as u32;
            let mut list = AudioBufferList {
                count: 1,
                buffers: [AudioBuffer {
                    channels: self.channels as u32,
                    size: (self.buf.len() * 4) as u32,
                    data: self.buf.as_mut_ptr() as *mut c_void,
                }],
            };
            // SAFETY: the buffer list points at `self.buf`, sized for `frames`.
            let st = unsafe { ExtAudioFileRead(self.file, &mut frames, &mut list) };
            if st != 0 {
                return Err(format!("ExtAudioFileRead {st}"));
            }
            if frames == 0 {
                return Ok(false);
            }
            out.extend(downmix(&self.buf[..frames as usize * self.channels], self.channels));
            Ok(true)
        }
    }

    impl Drop for ExtSource {
        fn drop(&mut self) {
            // SAFETY: the handle came from ExtAudioFileOpenURL and is disposed once.
            unsafe { ExtAudioFileDispose(self.file) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::Decoded;
    use crate::vad::EnergyVad;

    struct Echo(usize);
    impl Transcriber for Echo {
        fn decode(&mut self, _: Source, s: &[f32], o: &DecodeOpts) -> Result<Decoded, String> {
            assert_eq!(o.kind, JobKind::Final);
            self.0 += 1;
            Ok(Decoded {
                text: format!("Satz {} mit {} ms.", self.0, s.len() * 1000 / RATE),
                lang: if self.0 == 1 { "en".into() } else { "de".into() },
                confidence: 0.9,
                mean_logprob: -0.1,
                no_speech: 0.0,
                rejected: vec![],
            })
        }
    }

    fn speech_with_pauses() -> Vec<f32> {
        let tone = |n: usize| (0..n).map(|i| (i as f32 * 0.05).sin() * 0.3).collect::<Vec<_>>();
        let mut v = vec![0.0; RATE / 2];
        v.extend(tone(RATE * 2));
        v.extend(vec![0.0; RATE * 2]);
        v.extend(tone(RATE * 3));
        v.extend(vec![0.0; RATE]);
        v
    }

    #[test]
    fn a_file_becomes_one_line_per_segment_with_progress() {
        let mut src = Samples::new(speech_with_pauses());
        let mut vad = EnergyVad::new(0.05);
        let mut tr = Echo(0);
        let mut seen = Vec::new();
        let r = transcribe_stream(&mut src, &mut vad, &mut tr, Lang::Auto, 2, &AtomicBool::new(false), |d, t| {
            seen.push((d, t))
        })
        .unwrap();
        assert_eq!(r.text.lines().count(), 2, "{}", r.text);
        assert_eq!(r.lang.as_deref(), Some("de"), "the language with the most text wins");
        let (d, t) = *seen.last().unwrap();
        assert_eq!(d, t, "ends at 100 %");
        assert_eq!(t, 8500);
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "never backwards");
    }

    struct FakeWords(usize);
    impl WordTranscriber for FakeWords {
        fn decode_tokens(&mut self, _: &[f32], o: &DecodeOpts) -> Result<crate::words::TokDecoded, String> {
            assert_eq!(o.kind, JobKind::Final);
            self.0 += 1;
            let t = |text: &str, cs| crate::words::Tok { text: text.into(), t_dtw_cs: cs, p: 0.9, special: false };
            Ok(crate::words::TokDecoded {
                tokens: vec![t(" Satz", 20), t(&format!(" {}", self.0), 50)],
                lang: "de".into(),
            })
        }
    }

    #[test]
    fn words_carry_the_segment_offset() {
        let mut src = Samples::new(speech_with_pauses());
        let r = transcribe_words_stream(
            &mut src,
            &mut EnergyVad::new(0.05),
            &mut FakeWords(0),
            Lang::Auto,
            2,
            &AtomicBool::new(false),
            |_, _| {},
        )
        .unwrap();
        let t: Vec<&str> = r.words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(t, ["Satz", "1", "Satz", "2"]);
        assert_eq!(r.lang.as_deref(), Some("de"));
        // Speech starts at 0.5 s and 4.5 s; each segment carries PAD before it.
        let (a, b) = (r.words[0].start_ms, r.words[2].start_ms);
        assert!((150..=500).contains(&a), "{a}");
        assert!((4150..=4500).contains(&b), "second segment offset missing: {b}");
        assert_eq!(r.words[1].start_ms, a + 200, "word 2 starts at word 1's end");
        assert_eq!(r.words[3].start_ms, b + 200);
        assert_eq!(r.words[3].end_ms, b + 500);
        assert!(r.words.windows(2).all(|w| w[0].end_ms <= w[1].start_ms));
    }

    #[test]
    fn cancel_stops_without_text() {
        let mut src = Samples::new(speech_with_pauses());
        let r = transcribe_stream(&mut src, &mut EnergyVad::new(0.05), &mut Echo(0), Lang::De, 2, &AtomicBool::new(true), |_, _| {});
        assert_eq!(r.unwrap_err(), ERR_CANCELLED);
    }

    #[test]
    fn silence_gives_empty_text() {
        let mut src = Samples::new(vec![0.0; RATE * 3]);
        let r = transcribe_stream(&mut src, &mut EnergyVad::new(0.05), &mut Echo(0), Lang::De, 2, &AtomicBool::new(false), |_, _| {}).unwrap();
        assert_eq!(r, FileText { text: String::new(), lang: None });
    }

    #[test]
    fn txt_names_count_up_and_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("Rede.m4a");
        let a = write_txt(&src, "eins").unwrap();
        let b = write_txt(&src, "zwei").unwrap();
        assert_eq!(a.file_name().unwrap(), "Rede.txt");
        assert_eq!(b.file_name().unwrap(), "Rede 2.txt");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "eins\n");
        std::fs::remove_file(&a).unwrap();
        assert_eq!(write_txt(&src, "drei").unwrap().file_name().unwrap(), "Rede.txt", "gaps are reused");
    }

    #[test]
    fn downmix_averages_channels() {
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix(&[0.2, 0.4], 1), vec![0.2, 0.4]);
    }

    #[test]
    fn ffmpeg_never_reads_a_path_as_an_option() {
        let a = ffmpeg_args(Path::new("-x.mov"));
        let i = a.iter().position(|s| s == "-i").unwrap();
        assert_eq!(a[i + 1], std::ffi::OsString::from("./-x.mov"));
        let a = ffmpeg_args(Path::new("/a/b c.mkv"));
        assert_eq!(a[i + 1], std::ffi::OsString::from("/a/b c.mkv"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn core_audio_reads_the_fixture_at_16k_mono() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/de.wav");
        let mut s = open_audio(&p).unwrap();
        let mut all = Vec::new();
        while s.read(&mut all).unwrap() {}
        let want = crate::test_support::fixture("de.wav");
        assert!((all.len() as i64 - want.len() as i64).abs() < 64, "{} vs {}", all.len(), want.len());
        assert!(s.total_ms() > 1000);
    }
}
