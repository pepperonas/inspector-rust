//! OCR via OS-native engines.
//!
//! macOS uses the Vision framework's `VNRecognizeTextRequest`, the
//! exact same engine that powers Apple's Live Text. No model bundles,
//! no network, no Python — same quality as Preview's "Copy Text from
//! Selection" but invoked from our region capture.
//!
//! Windows OCR (`Windows.Media.Ocr`) is stubbed for now — the macOS
//! path landed first because Vision is the path of least resistance
//! given the existing objc2 plumbing.

use anyhow::Result;

/// Run OCR on the supplied PNG bytes, returning the recognized text
/// joined with `\n` between observations (Vision returns one
/// `VNRecognizedTextObservation` per visual line). Empty string means
/// "the engine ran but found no text", which is a valid result — the
/// caller decides whether that's worth surfacing.
pub fn recognize(png_bytes: &[u8]) -> Result<String> {
    if png_bytes.is_empty() {
        anyhow::bail!("empty image data");
    }
    recognize_impl(png_bytes, Level::Accurate)
}

/// A tiny image with a few words. Recognising it makes the OS load the text
/// model before the real request needs it.
const WARMUP_PNG: &[u8] = include_bytes!("../assets/ocr_warmup.png");

/// Background warm-up attempts before giving up until the next trigger.
const WARMUP_ATTEMPTS: u32 = 3;

static WARMING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Load the recognition model in the background so the next [`recognize`] is
/// fast. Measured 2026-10-20 on an M1 Pro: warm 0.1–0.3 s, a process start
/// 0.3–0.4 s, and **69 s** once macOS had dropped the model — that is the
/// "OCR often takes too long". Called when an OCR starts (the model then loads
/// while the user still drags the region) and once after app start. A warm-up
/// already running is not started twice; a failure only costs the head start.
pub fn prewarm() {
    use std::sync::atomic::Ordering;
    // Only Vision keeps a model that the OS can drop; tesseract (Linux) is a
    // fresh process per call and Windows OCR is cheap to create.
    if !cfg!(target_os = "macos") {
        return;
    }
    if WARMING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("ir-ocr-warmup".into())
        .spawn(|| {
            // A first load after the model cache was lost can fail outright
            // (`CRImageReaderError 1`, 2026-10-29); a later attempt continues
            // from what the failed one cached. Try a few times here, in the
            // background, so the user's OCR does not pay for it.
            for attempt in 1..=WARMUP_ATTEMPTS {
                let t = std::time::Instant::now();
                match recognize_impl(WARMUP_PNG, Level::Accurate) {
                    Ok(_) => {
                        tracing::info!("ocr: model warm-up {} ms (attempt {attempt})", t.elapsed().as_millis());
                        break;
                    }
                    Err(e) => tracing::warn!(
                        "ocr: warm-up attempt {attempt} failed after {} ms: {e:#}",
                        t.elapsed().as_millis()
                    ),
                }
            }
            WARMING.store(false, Ordering::SeqCst);
        });
    if spawned.is_err() {
        WARMING.store(false, Ordering::SeqCst);
    }
}

/// Vision's two recognition paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Level {
    Accurate,
    Fast,
}

/// macOS: Vision, run in the bundled `ir-ocr` helper process when it is
/// there, in-process otherwise (dev runs, tests, DMG builds without it).
///
/// Why a helper: macOS compiles Vision's text model per **binary** and caches
/// it keyed by the binary's content. The app's own binary changes with every
/// release, so in-process OCR paid 70–110 s on the first recognition after
/// each update (measured 2026-10-29, M1 Pro; the warm path is 0.1–0.3 s).
/// `ir-ocr` is built from a crate that rarely changes, so its cache survives
/// app updates. A helper that fails is replaced by a fresh one once — a
/// long-running process could get stuck in `CRImageReaderError 1` after macOS
/// purged the cache under it (log 2026-10-28); only a new process recovered.
#[cfg(target_os = "macos")]
fn recognize_impl(png_bytes: &[u8], level: Level) -> Result<String> {
    let lvl = match level {
        Level::Accurate => ir_ocr_helper::Level::Accurate,
        Level::Fast => ir_ocr_helper::Level::Fast,
    };
    let Some(path) = helper::path() else {
        return ir_ocr_helper::recognize(png_bytes, lvl).map_err(anyhow::Error::msg);
    };
    match helper::recognize(&path, png_bytes, lvl) {
        Ok(text) => Ok(text),
        Err(first) => {
            tracing::warn!("ocr: helper failed ({first}) — retrying with a fresh helper");
            helper::recognize(&path, png_bytes, lvl).map_err(anyhow::Error::msg)
        }
    }
}

/// The client side of the `ir-ocr` helper: one process, started on first use
/// and kept for the app's lifetime (its warm model is the whole point).
#[cfg(target_os = "macos")]
mod helper {
    use std::io::BufReader;
    use std::path::{Path, PathBuf};
    use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
    use std::sync::Mutex;

    /// File name inside `Contents/MacOS`, next to the app binary.
    pub const NAME: &str = "ir-ocr";

    struct Running {
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
    }

    static HELPER: Mutex<Option<Running>> = Mutex::new(None);

    pub fn path() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let p = exe.parent()?.join(NAME);
        p.is_file().then_some(p)
    }

    fn spawn(path: &Path) -> Result<Running, String> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("start {}: {e}", path.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        Ok(Running { child, stdin, stdout })
    }

    /// Send one image. Any failure — I/O or an error reply — drops the
    /// helper, so the next call (the caller's retry) starts a fresh one.
    pub fn recognize(path: &Path, png: &[u8], level: ir_ocr_helper::Level) -> Result<String, String> {
        let mut guard = HELPER.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(spawn(path)?);
        }
        let run = guard.as_mut().expect("just set");
        let tag = match level {
            ir_ocr_helper::Level::Accurate => ir_ocr_helper::REQ_ACCURATE,
            ir_ocr_helper::Level::Fast => ir_ocr_helper::REQ_FAST,
        };
        let reply = ir_ocr_helper::write_frame(&mut run.stdin, tag, png)
            .and_then(|_| ir_ocr_helper::read_frame(&mut run.stdout));
        let result = match reply {
            Ok(Some((ir_ocr_helper::REPLY_TEXT, body))) => {
                String::from_utf8(body).map_err(|_| "helper sent invalid UTF-8".to_string())
            }
            Ok(Some((_, body))) => Err(String::from_utf8_lossy(&body).into_owned()),
            Ok(None) => Err("helper exited".to_string()),
            Err(e) => Err(format!("helper I/O: {e}")),
        };
        if result.is_err() {
            if let Some(mut dead) = guard.take() {
                let _ = dead.child.kill();
                let _ = dead.child.wait();
            }
        }
        result
    }
}

#[cfg(target_os = "windows")]
fn recognize_impl(png_bytes: &[u8], _level: Level) -> Result<String> {
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    // WinRT requires a COM apartment; worker threads aren't initialised by
    // default. S_FALSE = already done, RPC_E_CHANGED_MODE = STA — both fine.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };

    // ── 1. Write PNG bytes into an in-memory stream ──────────────────
    let stream = InMemoryRandomAccessStream::new()?;
    let output = stream.GetOutputStreamAt(0)?;
    let writer = DataWriter::CreateDataWriter(&output)?;
    writer.WriteBytes(png_bytes)?;
    writer.StoreAsync()?.get()?;
    let _ = writer.DetachStream(); // leave stream in a usable state
    stream.Seek(0)?;

    // ── 2. Decode PNG → SoftwareBitmap ───────────────────────────────
    let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
    let bitmap = decoder.GetSoftwareBitmapAsync()?.get()?;

    // ── 3. Create OCR engine from the user's installed language packs ─
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| {
        anyhow::anyhow!(
            "Windows OCR: no language pack available — \
             install a language in Settings → Time & Language → Language"
        )
    })?;

    // ── 4. Recognise text (blocking) ─────────────────────────────────
    let ocr_result = engine.RecognizeAsync(&bitmap)?.get()?;

    // ── 5. Collect lines ─────────────────────────────────────────────
    let lines = ocr_result.Lines()?;
    let count = lines.Size()?;
    let mut parts: Vec<String> = Vec::with_capacity(count as usize);
    for i in 0..count {
        parts.push(lines.GetAt(i)?.Text()?.to_string());
    }
    Ok(parts.join("\n"))
}

/// Linux: Tesseract CLI (`apt install tesseract-ocr`). Offline, no extra Rust deps.
#[cfg(target_os = "linux")]
fn recognize_impl(png_bytes: &[u8], _level: Level) -> Result<String> {
    use anyhow::Context;
    use chrono::Utc;
    use std::process::Command;

    let langs = tesseract_lang_arg()?;

    let tmp = std::env::temp_dir().join(format!(
        "inspector-rust-ocr-{}.png",
        Utc::now().timestamp_millis()
    ));
    std::fs::write(&tmp, png_bytes).context("write OCR temp png")?;

    let output = Command::new("tesseract")
        .arg(&tmp)
        .arg("stdout")
        .args(["-l", langs])
        .output()
        .with_context(|| format!("spawn tesseract -l {langs} ({TESSERACT_INSTALL_HINT})"))?;

    let _ = std::fs::remove_file(&tmp);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tesseract failed: {stderr} ({TESSERACT_INSTALL_HINT})");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
const TESSERACT_INSTALL_HINT: &str =
    "install: sudo apt install tesseract-ocr tesseract-ocr-eng; optional German: tesseract-ocr-deu";

/// Cached `-l` value: `eng`, `eng+deu`, or the best single pack found.
#[cfg(target_os = "linux")]
fn tesseract_lang_arg() -> Result<&'static str> {
    use std::sync::OnceLock;

    static LANGS: OnceLock<Result<String, String>> = OnceLock::new();
    LANGS
        .get_or_init(discover_tesseract_langs)
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{e}"))
        .map(|s| s.as_str())
}

#[cfg(target_os = "linux")]
fn discover_tesseract_langs() -> Result<String, String> {
    use std::process::Command;

    let output = Command::new("tesseract")
        .arg("--list-langs")
        .output()
        .map_err(|_| format!("tesseract not found ({TESSERACT_INSTALL_HINT})"))?;

    let langs: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let lang = line.trim();
            if lang.is_empty() || lang.starts_with("List of available") {
                None
            } else {
                Some(lang.to_string())
            }
        })
        .collect();

    pick_tesseract_langs(langs)
        .map_err(|_| format!("no OCR language packs found ({TESSERACT_INSTALL_HINT})"))
}

/// Prefer English; add German when installed; never require `deu` for English OCR.
#[cfg(target_os = "linux")]
fn pick_tesseract_langs(langs: Vec<String>) -> Result<String, &'static str> {
    if langs.is_empty() {
        return Err("no packs");
    }
    let has = |code: &str| langs.iter().any(|l| l == code);
    if has("eng") && has("deu") {
        return Ok("eng+deu".into());
    }
    if has("eng") {
        return Ok("eng".into());
    }
    if has("deu") {
        return Ok("deu".into());
    }
    langs.into_iter().find(|l| l != "osd").ok_or("no packs")
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tesseract_tests {
    use super::pick_tesseract_langs;

    #[test]
    fn prefers_eng_without_deu() {
        let langs = vec!["eng".to_string(), "osd".to_string()];
        assert_eq!(pick_tesseract_langs(langs).unwrap(), "eng");
    }

    #[test]
    fn uses_both_when_present() {
        let langs = vec!["deu".to_string(), "eng".to_string(), "osd".to_string()];
        assert_eq!(pick_tesseract_langs(langs).unwrap(), "eng+deu");
    }

    #[test]
    fn falls_back_to_deu_only() {
        let langs = vec!["deu".to_string(), "osd".to_string()];
        assert_eq!(pick_tesseract_langs(langs).unwrap(), "deu");
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn recognize_impl(_png_bytes: &[u8], _level: Level) -> Result<String> {
    anyhow::bail!("OCR is not implemented on this platform")
}

#[cfg(test)]
mod wiring_tests {
    /// The warm-up only helps if it starts BEFORE the user drags the region.
    #[test]
    fn the_pipeline_warms_the_model_before_the_capture() {
        let src = include_str!("commands.rs");
        let body = &src[src.find("pub fn run_ocr_pipeline").unwrap()..];
        let warm = body.find("ocr::prewarm()").expect("prewarm call");
        let capture = body.find("region_picker::capture()").unwrap();
        assert!(warm < capture);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod helper_tests {
    use super::helper;
    use std::path::PathBuf;

    /// A stand-in helper: answers exactly ONE request (a 3-byte body) with
    /// the given reply, then exits — like a helper that dies after a request.
    fn one_shot(dir: &std::path::Path, name: &str, reply: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        let script = format!("#!/bin/sh\nhead -c 8 >/dev/null\nprintf '{reply}'\n");
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    // One test, because the helper is one process-wide slot.
    #[test]
    fn a_dead_or_failing_helper_is_replaced_on_the_next_call() {
        let dir = std::env::temp_dir().join(format!("ir-ocr-helper-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = one_shot(&dir, "ok", r"T\005\000\000\000hello");
        let err = one_shot(&dir, "err", r"E\004\000\000\000boom");
        let lvl = ir_ocr_helper::Level::Accurate;

        assert_eq!(helper::recognize(&ok, b"png", lvl).unwrap(), "hello");
        // Same slot, process has exited: this call fails and clears the slot …
        assert!(helper::recognize(&ok, b"png", lvl).is_err());
        // … so the next one starts a fresh helper and works again.
        assert_eq!(helper::recognize(&ok, b"png", lvl).unwrap(), "hello");
        // A replaced helper that answers with an error drops the slot as well.
        let _ = helper::recognize(&ok, b"png", lvl); // retire the exited one
        assert_eq!(helper::recognize(&err, b"png", lvl).unwrap_err(), "boom");
        assert_eq!(helper::recognize(&ok, b"png", lvl).unwrap(), "hello");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// End to end with the real helper binary:
    /// `cargo build --release -p ir-ocr-helper && cargo test --release -p inspector-rust-core --lib real_helper -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn the_real_helper_reads_the_warm_up_image() {
        let exe = std::env::current_exe().unwrap();
        let bin = exe.parent().unwrap().parent().unwrap().join(helper::NAME);
        for i in 0..2 {
            let t = std::time::Instant::now();
            let text = helper::recognize(&bin, super::WARMUP_PNG, ir_ocr_helper::Level::Accurate).unwrap();
            println!("run {i}: {} ms", t.elapsed().as_millis());
            assert!(text.to_lowercase().contains("warm"), "got {text:?}");
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod bench {
    use super::*;

    /// Needs Apple's text model: a cold load takes ~70 s and can fail outright
    /// in a test process (`CRImageReaderError 1`), so it runs on request only:
    /// `cargo test -p inspector-rust-core --lib the_warm_up_image -- --ignored`
    #[test]
    #[ignore]
    fn the_warm_up_image_is_read_as_text() {
        let text = recognize_impl(WARMUP_PNG, Level::Accurate).unwrap();
        assert!(text.to_lowercase().contains("warm"), "got {text:?}");
    }

    /// `IR_OCR_BENCH=<dir of .png> cargo test --release -p inspector-rust-core --lib ocr_bench -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn ocr_bench() {
        let dir = std::env::var("IR_OCR_BENCH").expect("IR_OCR_BENCH");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "png") {
                continue;
            }
            let png = std::fs::read(&path).unwrap();
            // A warm-up on another thread first: does it help the real call?
            let t = std::time::Instant::now();
            std::thread::spawn(|| recognize_impl(WARMUP_PNG, Level::Accurate).map(|_| ()).ok()).join().unwrap();
            println!("warm-up (other thread): {} ms", t.elapsed().as_millis());
            for level in [Level::Accurate, Level::Fast, Level::Accurate] {
                let t = std::time::Instant::now();
                let text = recognize_impl(&png, level).unwrap();
                println!(
                    "{:?} {:?}: {} ms, {} chars",
                    path.file_name().unwrap(),
                    level,
                    t.elapsed().as_millis(),
                    text.chars().count()
                );
            }
        }
    }
}
