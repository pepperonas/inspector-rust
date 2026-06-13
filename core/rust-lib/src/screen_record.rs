//! Screen recording — the `Ctrl+Shift+R` flow. Region select → audio-track
//! choice (system / mic / both / none) → 3 s countdown → ffmpeg records the
//! region to an MP4 → floating stop bar → Downloads + preview.
//!
//! **Engine: ffmpeg** (same workflow on macOS + Windows, MP4/H.264). macOS uses
//! the `avfoundation` input + a `crop` filter; Windows uses `gdigrab` (region
//! offset/size) + `dshow` audio. System audio needs a loopback device
//! (macOS: BlackHole; Windows: a virtual capturer / "Stereo Mix") — captured by
//! name when present. The pure argv builders + device-list parsers are
//! unit-tested; the spawn/stop is a thin process wrapper.
//!
//! **Windows is runtime-unverified** (built compile-clean; not yet exercised on
//! a real Windows box).

use parking_lot::Mutex;
use std::path::PathBuf;
use std::process::Child;

/// A capture rectangle in **physical pixels** of the target display (the
/// frontend overlay multiplies CSS px by `devicePixelRatio` before sending).
#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct RecordRegion {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Which audio tracks to mix into the recording.
#[derive(Debug, Clone, Copy, serde::Deserialize, Default)]
pub struct AudioChoice {
    pub system: bool,
    pub mic: bool,
}

const FPS: u32 = 30;

/// Gain applied to the (typically quiet) microphone input. macOS built-in mics
/// record well below line level; +10 dB lifts speech to a usable range while
/// staying clear of clipping for normal voice. System/loopback audio is left
/// untouched (it's already at proper level).
const MIC_GAIN: &str = "10dB";

// ── Device-list parsers (pure, unit-tested) ──────────────────────────────────

/// One `(index, name)` device row.
pub type Device = (u32, String);

/// Parse `ffmpeg -f avfoundation -list_devices true -i ""` stderr into the
/// `(video, audio)` device lists. Lines look like:
/// `[AVFoundation indev @ 0x..] [2] Capture screen 0`
#[cfg(any(target_os = "macos", test))]
pub fn parse_avf_devices(stderr: &str) -> (Vec<Device>, Vec<Device>) {
    let mut video = Vec::new();
    let mut audio = Vec::new();
    let mut in_audio = false;
    for line in stderr.lines() {
        let lower = line.to_lowercase();
        if lower.contains("video devices") {
            in_audio = false;
            continue;
        }
        if lower.contains("audio devices") {
            in_audio = true;
            continue;
        }
        // Extract a trailing `[<n>] <name>` (skip the leading `[AVFoundation …]`).
        if let Some((idx, name)) = parse_indexed_tail(line) {
            if in_audio {
                audio.push((idx, name));
            } else {
                video.push((idx, name));
            }
        }
    }
    (video, audio)
}

/// From a line, take the LAST `[<digits>] <rest>` occurrence as `(idx, name)`.
fn parse_indexed_tail(line: &str) -> Option<Device> {
    let open = line.rfind('[')?;
    let close = line[open..].find(']')? + open;
    let idx: u32 = line[open + 1..close].trim().parse().ok()?;
    let name = line[close + 1..].trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some((idx, name))
}

/// Pick the avfoundation **screen** capture video index ("Capture screen N").
#[cfg(any(target_os = "macos", test))]
pub fn pick_screen_index(video: &[Device]) -> Option<u32> {
    video
        .iter()
        .find(|(_, n)| n.to_lowercase().contains("capture screen"))
        .map(|(i, _)| *i)
}

/// Pick the microphone audio index (name contains "microphone"/"mikrofon",
/// else the first audio device).
#[cfg(any(target_os = "macos", test))]
pub fn pick_mic_index(audio: &[Device]) -> Option<u32> {
    audio
        .iter()
        .find(|(_, n)| {
            let l = n.to_lowercase();
            l.contains("microphone") || l.contains("mikrofon") || l.contains("mic")
        })
        .or_else(|| audio.first())
        .map(|(i, _)| *i)
}

/// Pick a loopback/system-audio capture index (BlackHole / Loopback / Soundflower).
#[cfg(any(target_os = "macos", test))]
pub fn pick_system_index(audio: &[Device]) -> Option<u32> {
    audio
        .iter()
        .find(|(_, n)| {
            let l = n.to_lowercase();
            l.contains("blackhole") || l.contains("loopback") || l.contains("soundflower")
        })
        .map(|(i, _)| *i)
}

/// Parse `ffmpeg -list_devices true -f dshow -i dummy` stderr → audio device
/// names. Lines look like: `[dshow @ 0x..] "Microphone (Realtek)" (audio)`.
#[cfg(any(target_os = "windows", test))]
pub fn parse_dshow_audio(stderr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_audio = false;
    for line in stderr.lines() {
        let lower = line.to_lowercase();
        if lower.contains("directshow audio devices") {
            in_audio = true;
            continue;
        }
        if lower.contains("directshow video devices") {
            in_audio = false;
            continue;
        }
        if !in_audio {
            continue;
        }
        // Take the first double-quoted token as the device name.
        if let Some(start) = line.find('"') {
            if let Some(end) = line[start + 1..].find('"') {
                let name = &line[start + 1..start + 1 + end];
                if !name.is_empty() {
                    out.push(name.to_string());
                }
            }
        }
    }
    out
}

/// Best-effort pick of a Windows microphone capture device name.
#[cfg(any(target_os = "windows", test))]
pub fn pick_dshow_mic(devices: &[String]) -> Option<String> {
    devices
        .iter()
        .find(|n| {
            let l = n.to_lowercase();
            l.contains("microphone") || l.contains("mikrofon") || l.contains("mic")
        })
        .or_else(|| devices.first())
        .cloned()
}

/// Best-effort pick of a Windows system/loopback capture device name.
#[cfg(any(target_os = "windows", test))]
pub fn pick_dshow_system(devices: &[String]) -> Option<String> {
    devices
        .iter()
        .find(|n| {
            let l = n.to_lowercase();
            l.contains("stereo mix")
                || l.contains("stereomix")
                || l.contains("virtual-audio-capturer")
                || l.contains("loopback")
                || l.contains("what u hear")
                || l.contains("was sie hören")
        })
        .cloned()
}

// ── Pure argv builders (unit-tested) ─────────────────────────────────────────

/// Build the macOS (avfoundation) ffmpeg argv. `system`/`mic` are resolved
/// avfoundation audio indices (None = unavailable / not wanted).
#[cfg(any(target_os = "macos", test))]
pub fn build_args_macos(
    region: RecordRegion,
    screen_idx: u32,
    system: Option<u32>,
    mic: Option<u32>,
    out: &str,
) -> Vec<String> {
    let mut a: Vec<String> = vec!["-y".into(), "-hide_banner".into()];
    let crop = format!("crop={}:{}:{}:{}", region.w, region.h, region.x, region.y);

    // Primary screen input. Pair the first audio source onto it; a second
    // source (for "both") comes as a separate audio-only avfoundation input.
    let (primary_audio, second_audio): (Option<u32>, Option<u32>) = match (system, mic) {
        (Some(s), Some(m)) => (Some(s), Some(m)),
        (Some(s), None) => (Some(s), None),
        (None, Some(m)) => (Some(m), None),
        (None, None) => (None, None),
    };
    // When the *single* paired audio source is the mic (mic-only), it gets the
    // gain boost. In the "both" case the mic is the second input and is boosted
    // inside the filter graph; system-only is never boosted.
    let primary_is_mic = system.is_none() && mic.is_some();

    a.push("-f".into());
    a.push("avfoundation".into());
    a.push("-capture_cursor".into());
    a.push("1".into());
    a.push("-framerate".into());
    a.push(FPS.to_string());
    a.push("-i".into());
    a.push(format!(
        "{screen_idx}:{}",
        primary_audio.map(|i| i.to_string()).unwrap_or_else(|| "none".into())
    ));

    if let Some(m) = second_audio {
        a.push("-f".into());
        a.push("avfoundation".into());
        a.push("-i".into());
        a.push(format!(":{m}"));
    }

    if second_audio.is_some() {
        // crop video + boost the mic (input 1) + mix the two audio inputs.
        a.push("-filter_complex".into());
        a.push(format!(
            "[0:v]{crop}[v];[1:a]volume={MIC_GAIN}[m];[0:a][m]amix=inputs=2:duration=longest[a]"
        ));
        a.push("-map".into());
        a.push("[v]".into());
        a.push("-map".into());
        a.push("[a]".into());
        a.push("-c:a".into());
        a.push("aac".into());
    } else if primary_audio.is_some() {
        a.push("-vf".into());
        a.push(crop);
        if primary_is_mic {
            a.push("-af".into());
            a.push(format!("volume={MIC_GAIN}"));
        }
        a.push("-c:a".into());
        a.push("aac".into());
    } else {
        a.push("-vf".into());
        a.push(crop);
        a.push("-an".into());
    }

    push_video_out(&mut a, out);
    a
}

/// Build the Windows (gdigrab + dshow) ffmpeg argv. `mic`/`system` are resolved
/// dshow device names. gdigrab coords are in desktop pixels.
#[cfg(any(target_os = "windows", test))]
pub fn build_args_windows(
    region: RecordRegion,
    system: Option<&str>,
    mic: Option<&str>,
    out: &str,
) -> Vec<String> {
    let mut a: Vec<String> = vec!["-y".into(), "-hide_banner".into()];
    a.push("-f".into());
    a.push("gdigrab".into());
    a.push("-framerate".into());
    a.push(FPS.to_string());
    a.push("-offset_x".into());
    a.push(region.x.to_string());
    a.push("-offset_y".into());
    a.push(region.y.to_string());
    a.push("-video_size".into());
    a.push(format!("{}x{}", region.w, region.h));
    a.push("-i".into());
    a.push("desktop".into());

    let audios: Vec<&str> = [system, mic].into_iter().flatten().collect();
    for dev in &audios {
        a.push("-f".into());
        a.push("dshow".into());
        a.push("-i".into());
        a.push(format!("audio={dev}"));
    }
    // Boost the mic only. With both, audios = [system, mic] → mic is input 2.
    let single_is_mic = system.is_none() && mic.is_some();
    match audios.len() {
        0 => {
            a.push("-an".into());
        }
        1 => {
            a.push("-map".into());
            a.push("0:v".into());
            a.push("-map".into());
            a.push("1:a".into());
            if single_is_mic {
                a.push("-af".into());
                a.push(format!("volume={MIC_GAIN}"));
            }
            a.push("-c:a".into());
            a.push("aac".into());
        }
        _ => {
            a.push("-filter_complex".into());
            a.push(format!(
                "[2:a]volume={MIC_GAIN}[m];[1:a][m]amix=inputs=2:duration=longest[a]"
            ));
            a.push("-map".into());
            a.push("0:v".into());
            a.push("-map".into());
            a.push("[a]".into());
            a.push("-c:a".into());
            a.push("aac".into());
        }
    }
    push_video_out(&mut a, out);
    a
}

/// Shared H.264 / MP4 output tail. `-r FPS` locks the output to constant frame
/// rate: the avfoundation screen input reports an undefined "1000k fps" nominal
/// rate (it's event-driven), so without this the output timebase is irregular,
/// which can play back too fast in some players and makes the pause/resume
/// concat unreliable. Forcing CFR keeps real-time playback + clean concat.
fn push_video_out(a: &mut Vec<String>, out: &str) {
    for s in [
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-pix_fmt",
        "yuv420p",
        "-r",
        &FPS.to_string(),
        "-movflags",
        "+faststart",
    ] {
        a.push(s.into());
    }
    a.push(out.into());
}

// ── ffmpeg discovery ─────────────────────────────────────────────────────────

/// Locate the `ffmpeg` binary on PATH or in common install locations.
pub fn ffmpeg_path() -> Option<PathBuf> {
    // Honour an explicit PATH lookup first.
    if let Ok(path) = std::env::var("PATH") {
        let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
        for dir in std::env::split_paths(&path) {
            let cand = dir.join(exe);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    // Common spots GUI apps (with a stripped PATH) miss.
    let extra: &[&str] = if cfg!(target_os = "macos") {
        &["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"]
    } else if cfg!(windows) {
        &[r"C:\ffmpeg\bin\ffmpeg.exe", r"C:\Program Files\ffmpeg\bin\ffmpeg.exe"]
    } else {
        &["/usr/bin/ffmpeg", "/usr/local/bin/ffmpeg"]
    };
    extra.iter().map(PathBuf::from).find(|p| p.is_file())
}

// ── Recording state + start/pause/resume/stop ────────────────────────────────
//
// Pause/resume is implemented as **segment + concat**: ffmpeg can't truly
// pause a live capture, so each contiguous run is recorded to its own temp
// segment file. Pause finalises the current segment; resume spawns a fresh
// ffmpeg into the next segment; stop finalises the last segment and concatenates
// them all losslessly (`-c copy`, no re-encode) into the final MP4. A single
// segment (never paused) is just moved to the output, skipping concat.

#[derive(Default)]
pub struct RecordState {
    inner: Mutex<Option<Session>>,
}

/// One recording session, possibly spanning several pause/resume segments.
struct Session {
    region: RecordRegion,
    audio: AudioChoice,
    ffmpeg: PathBuf,
    /// Final MP4 path in Downloads.
    final_out: PathBuf,
    /// Completed segment files (in capture order).
    segments: Vec<PathBuf>,
    /// The currently-recording ffmpeg + its segment file; `None` while paused.
    current: Option<(Child, PathBuf)>,
    /// Monotonic segment counter (names the temp files).
    seq: u32,
}

impl RecordState {
    pub fn is_recording(&self) -> bool {
        self.inner.lock().is_some()
    }
}

const ERR_NO_FFMPEG: &str =
    "record.no_ffmpeg"; // sentinel → frontend shows an install hint

/// Spawn an ffmpeg recording the `region`/`audio` into `out`. stdin is piped so
/// we can send `q` for a clean finalize.
fn spawn_segment(
    ffmpeg: &std::path::Path,
    region: RecordRegion,
    audio: AudioChoice,
    out: &std::path::Path,
) -> Result<Child, String> {
    let args = resolve_args(ffmpeg, region, audio, out)?;
    use std::process::{Command, Stdio};

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW (0x0800_0000) hides the console; CREATE_NEW_PROCESS_GROUP
        // (0x0000_0200) makes ffmpeg its own process-group leader so `finalize_child`
        // can target it with a CTRL+BREAK without the signal leaking back to us.
        Command::new(ffmpeg)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000 | 0x0000_0200)
            .spawn()
            .map_err(|e| format!("spawn ffmpeg: {e}"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new(ffmpeg)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn ffmpeg: {e}"))
    }
}

/// Cleanly finalize a running ffmpeg: ask it to stop gracefully so it flushes
/// the MP4 moov atom (trailer), wait up to 5 s, else hard-kill as a fallback.
fn finalize_child(mut child: Child) {
    request_graceful_stop(&mut child);
    let mut waited = std::time::Duration::ZERO;
    let step = std::time::Duration::from_millis(100);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if waited < std::time::Duration::from_secs(5) => {
                std::thread::sleep(step);
                waited += step;
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
}

/// macOS/Linux: ffmpeg reads `q` from the piped stdin fd (the `HAVE_TERMIOS_H`
/// keyboard path), which triggers a clean shutdown. Dropping stdin afterwards
/// signals EOF.
#[cfg(not(target_os = "windows"))]
fn request_graceful_stop(child: &mut Child) {
    use std::io::Write;
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(b"q\n");
        let _ = stdin.flush();
    }
    drop(child.stdin.take());
}

/// Windows: ffmpeg **ignores `q` on a piped stdin** — its keyboard poll uses
/// conio `_kbhit`/`_getch`, which only read a real console, never a pipe. (That
/// was the root cause of "stop not working on Windows": `q` was never seen, so
/// the segment was only ever ended by a hard kill → a truncated MP4 with no moov
/// atom.) Instead send a CTRL+BREAK to ffmpeg's process group; its console-ctrl
/// handler catches it and exits cleanly, writing the trailer. The child is
/// spawned with `CREATE_NEW_PROCESS_GROUP`, so its group id equals its pid and
/// the event can't propagate back to us.
#[cfg(target_os = "windows")]
fn request_graceful_stop(child: &mut Child) {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::System::Console::{
        AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, SetConsoleCtrlHandler,
        CTRL_BREAK_EVENT,
    };
    drop(child.stdin.take()); // close stdin (EOF); harmless, ffmpeg won't read it
    let pid = child.id();
    unsafe {
        // Attach to ffmpeg's (hidden) console so GenerateConsoleCtrlEvent targets
        // it; detach from anything we currently hold first.
        let _ = FreeConsole();
        if AttachConsole(pid).is_ok() {
            // Ignore the event ourselves while it's in flight, then restore.
            let _ = SetConsoleCtrlHandler(None, BOOL(1));
            let _ = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid);
            let _ = FreeConsole();
            let _ = SetConsoleCtrlHandler(None, BOOL(0));
        }
    }
}

/// Start a new recording. Returns the final MP4 path it will produce.
pub fn start(state: &RecordState, region: RecordRegion, audio: AudioChoice) -> Result<PathBuf, String> {
    if state.is_recording() {
        return Err("already recording".into());
    }
    let ffmpeg = ffmpeg_path().ok_or_else(|| ERR_NO_FFMPEG.to_string())?;
    let final_out = output_path()?;
    let seg0 = segment_path(0)?;
    let child = spawn_segment(&ffmpeg, region, audio, &seg0)?;

    *state.inner.lock() = Some(Session {
        region,
        audio,
        ffmpeg,
        final_out: final_out.clone(),
        segments: Vec::new(),
        current: Some((child, seg0)),
        seq: 0,
    });
    Ok(final_out)
}

/// Pause: finalize the current segment; the session stays alive.
///
/// `finalize_child` polls/waits up to 5 s, so it must run with the mutex
/// **released** — otherwise a concurrent `resume`/`stop`/`is_recording` (each a
/// synchronous IPC command) would block for that whole window. We take the
/// child out under the lock, drop the guard, finalize, then re-lock briefly to
/// record the finished segment — the same pattern `stop()` uses.
pub fn pause(state: &RecordState) -> Result<(), String> {
    let current = {
        let mut guard = state.inner.lock();
        let s = guard.as_mut().ok_or("not recording")?;
        s.current.take()
    };
    if let Some((child, path)) = current {
        finalize_child(child);
        if let Some(s) = state.inner.lock().as_mut() {
            s.segments.push(path);
        }
    }
    Ok(())
}

/// Resume: spawn a fresh ffmpeg into the next segment.
pub fn resume(state: &RecordState) -> Result<(), String> {
    let mut guard = state.inner.lock();
    let s = guard.as_mut().ok_or("not recording")?;
    if s.current.is_some() {
        return Ok(()); // already running
    }
    s.seq += 1;
    let seg = segment_path(s.seq)?;
    let child = spawn_segment(&s.ffmpeg, s.region, s.audio, &seg)?;
    s.current = Some((child, seg));
    Ok(())
}

/// Stop: finalize the last segment, concatenate all segments into the final
/// MP4, clean up the temps, and return the output path.
pub fn stop(state: &RecordState) -> Result<PathBuf, String> {
    let mut session = state.inner.lock().take().ok_or("not recording")?;
    if let Some((child, path)) = session.current.take() {
        finalize_child(child);
        session.segments.push(path);
    }
    // Drop segments that ffmpeg never actually wrote (e.g. instant pause).
    session.segments.retain(|p| p.is_file());
    if session.segments.is_empty() {
        return Err("nothing was recorded".into());
    }

    let out = session.final_out.clone();
    if session.segments.len() == 1 {
        move_file(&session.segments[0], &out)?;
    } else {
        concat_segments(&session.ffmpeg, &session.segments, &out)?;
        for seg in &session.segments {
            let _ = std::fs::remove_file(seg);
        }
    }
    Ok(out)
}

/// Move a file, falling back to copy+remove across volumes (the cache dir and
/// Downloads can live on different mounts).
fn move_file(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).map_err(|e| format!("copy recording: {e}"))?;
    let _ = std::fs::remove_file(from);
    Ok(())
}

/// Concatenate MP4 segments losslessly via ffmpeg's concat demuxer (`-c copy`).
fn concat_segments(
    ffmpeg: &std::path::Path,
    segments: &[PathBuf],
    out: &std::path::Path,
) -> Result<(), String> {
    let list_path = segment_dir()?.join(format!("concat-{}.txt", std::process::id()));
    std::fs::write(&list_path, concat_list_contents(segments))
        .map_err(|e| format!("write concat list: {e}"))?;

    #[cfg(target_os = "windows")]
    let status = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new(ffmpeg)
            .args(["-y", "-hide_banner", "-f", "concat", "-safe", "0", "-i"])
            .arg(&list_path)
            .args(["-c", "copy", "-movflags", "+faststart"])
            .arg(out)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .status()
            .map_err(|e| format!("concat ffmpeg: {e}"))?
    };

    #[cfg(not(target_os = "windows"))]
    let status = {
        std::process::Command::new(ffmpeg)
            .args(["-y", "-hide_banner", "-f", "concat", "-safe", "0", "-i"])
            .arg(&list_path)
            .args(["-c", "copy", "-movflags", "+faststart"])
            .arg(out)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| format!("concat ffmpeg: {e}"))?
    };

    let _ = std::fs::remove_file(&list_path);
    if !status.success() {
        return Err("concat failed".into());
    }
    Ok(())
}

/// Build the concat-demuxer list file body. Pure (unit-tested). Each line is
/// `file '<abs-path>'`; embedded single quotes are escaped the ffmpeg way.
fn concat_list_contents(segments: &[PathBuf]) -> String {
    segments
        .iter()
        .map(|p| {
            let s = p.to_string_lossy().replace('\'', "'\\''");
            format!("file '{s}'\n")
        })
        .collect()
}

/// Temp directory for in-progress recording segments.
fn segment_dir() -> Result<PathBuf, String> {
    let dir = dirs::cache_dir()
        .ok_or("no cache dir")?
        .join("InspectorRust")
        .join("recordings");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create segment dir: {e}"))?;
    Ok(dir)
}

fn segment_path(seq: u32) -> Result<PathBuf, String> {
    Ok(segment_dir()?.join(format!("seg-{}-{seq}.mp4", std::process::id())))
}

fn output_path() -> Result<PathBuf, String> {
    let dir = dirs::download_dir().ok_or("no Downloads folder")?;
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    Ok(dir.join(format!("Recording-{ts}.mp4")))
}

/// Kill any orphaned recording ffmpeg left over from a crash or a failed stop,
/// and clear stale segment files. Matched by our segment-cache path, so it's
/// unambiguously ours — safe to run at startup (no recording is ever active
/// then; the app is single-instance). Mirrors `wakelock::cleanup_orphans`.
pub fn cleanup_orphans() {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        // `pkill -f` matches the full command line against a regex; constrain it
        // to an ffmpeg process whose args reference our recordings cache, so we
        // never kill an unrelated process that merely mentions that path.
        let _ = std::process::Command::new("pkill")
            .args(["-f", "ffmpeg.*InspectorRust/recordings"])
            .status();
    }
    #[cfg(target_os = "windows")]
    {
        // taskkill can't filter by command line, so use PowerShell to find any
        // ffmpeg whose CommandLine references our cache (separator-agnostic:
        // `*InspectorRust*recordings*`) and kill it. CREATE_NO_WINDOW so no
        // console flashes at startup.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let script = "Get-CimInstance Win32_Process -Filter \"Name='ffmpeg.exe'\" | \
             Where-Object { $_.CommandLine -like '*InspectorRust*recordings*' } | \
             ForEach-Object { Stop-Process -Id $_.ProcessId -Force }";
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    // Remove any leftover segment files (incomplete, never concatenated).
    if let Ok(dir) = segment_dir() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn resolve_args(
    ffmpeg: &std::path::Path,
    region: RecordRegion,
    audio: AudioChoice,
    out: &std::path::Path,
) -> Result<Vec<String>, String> {
    let listing = std::process::Command::new(ffmpeg)
        .args(["-f", "avfoundation", "-list_devices", "true", "-i", ""])
        .output()
        .map_err(|e| format!("list avfoundation devices: {e}"))?;
    let stderr = String::from_utf8_lossy(&listing.stderr);
    let (video, audio_devs) = parse_avf_devices(&stderr);
    let screen = pick_screen_index(&video).ok_or("no 'Capture screen' device found")?;
    let system = if audio.system { pick_system_index(&audio_devs) } else { None };
    let mic = if audio.mic { pick_mic_index(&audio_devs) } else { None };
    Ok(build_args_macos(region, screen, system, mic, &out.to_string_lossy()))
}

#[cfg(target_os = "windows")]
fn resolve_args(
    ffmpeg: &std::path::Path,
    region: RecordRegion,
    audio: AudioChoice,
    out: &std::path::Path,
) -> Result<Vec<String>, String> {
    let listing = std::process::Command::new(ffmpeg)
        .args(["-list_devices", "true", "-f", "dshow", "-i", "dummy"])
        .output()
        .map_err(|e| format!("list dshow devices: {e}"))?;
    let stderr = String::from_utf8_lossy(&listing.stderr);
    let devs = parse_dshow_audio(&stderr);
    let system = if audio.system { pick_dshow_system(&devs) } else { None };
    let mic = if audio.mic { pick_dshow_mic(&devs) } else { None };
    Ok(build_args_windows(
        region,
        system.as_deref(),
        mic.as_deref(),
        &out.to_string_lossy(),
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn resolve_args(
    _ffmpeg: &std::path::Path,
    _region: RecordRegion,
    _audio: AudioChoice,
    _out: &std::path::Path,
) -> Result<Vec<String>, String> {
    Err("screen recording isn't supported on this platform yet".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RGN: RecordRegion = RecordRegion { x: 100, y: 200, w: 640, h: 480 };

    const AVF_SAMPLE: &str = "\
[AVFoundation indev @ 0x1] AVFoundation video devices:
[AVFoundation indev @ 0x1] [0] FaceTime HD Camera
[AVFoundation indev @ 0x1] [2] Capture screen 0
[AVFoundation indev @ 0x1] AVFoundation audio devices:
[AVFoundation indev @ 0x1] [1] BlackHole 2ch
[AVFoundation indev @ 0x1] [3] MacBook Pro Microphone";

    #[test]
    fn avf_parser_splits_video_and_audio() {
        let (v, a) = parse_avf_devices(AVF_SAMPLE);
        assert_eq!(v, vec![(0, "FaceTime HD Camera".into()), (2, "Capture screen 0".into())]);
        assert_eq!(a, vec![(1, "BlackHole 2ch".into()), (3, "MacBook Pro Microphone".into())]);
        assert_eq!(pick_screen_index(&v), Some(2));
        assert_eq!(pick_system_index(&a), Some(1));
        assert_eq!(pick_mic_index(&a), Some(3));
    }

    #[test]
    fn macos_none_audio_uses_an_and_crop() {
        let args = build_args_macos(RGN, 2, None, None, "/tmp/o.mp4");
        let j = args.join(" ");
        assert!(j.contains("avfoundation"));
        assert!(j.contains("-i 2:none"));
        assert!(j.contains("crop=640:480:100:200"));
        assert!(j.contains("-an"));
        assert!(j.ends_with("/tmp/o.mp4"));
        assert!(j.contains("libx264"));
    }

    #[test]
    fn macos_system_only_pairs_audio_on_screen_input() {
        let args = build_args_macos(RGN, 2, Some(1), None, "/tmp/o.mp4");
        let j = args.join(" ");
        assert!(j.contains("-i 2:1"));
        assert!(j.contains("-c:a aac"));
        assert!(!j.contains("amix"));
    }

    #[test]
    fn macos_both_uses_two_inputs_and_amix() {
        let args = build_args_macos(RGN, 2, Some(1), Some(3), "/tmp/o.mp4");
        let j = args.join(" ");
        assert!(j.contains("-i 2:1")); // screen + system
        assert!(j.contains("-i :3")); // mic-only second input
        assert!(j.contains("amix=inputs=2"));
        assert!(j.contains("[0:v]crop=640:480:100:200[v]"));
        assert!(j.contains("[1:a]volume=10dB[m]")); // mic boosted, system isn't
        assert!(j.contains("-map [v]"));
        assert!(j.contains("-map [a]"));
    }

    #[test]
    fn macos_mic_only_is_boosted_system_only_is_not() {
        let mic = build_args_macos(RGN, 2, None, Some(3), "/tmp/o.mp4").join(" ");
        assert!(mic.contains("-i 2:3"));
        assert!(mic.contains("-af volume=10dB"));
        let sys = build_args_macos(RGN, 2, Some(1), None, "/tmp/o.mp4").join(" ");
        assert!(!sys.contains("volume=")); // system audio left untouched
    }

    #[test]
    fn output_is_locked_to_cfr() {
        let args = build_args_macos(RGN, 2, None, None, "/tmp/o.mp4").join(" ");
        assert!(args.contains("-r 30"));
    }

    #[test]
    fn windows_region_uses_gdigrab_offsets() {
        let args = build_args_windows(RGN, None, Some("Microphone (X)"), "C:/o.mp4");
        let j = args.join(" ");
        assert!(j.contains("gdigrab"));
        assert!(j.contains("-offset_x 100"));
        assert!(j.contains("-offset_y 200"));
        assert!(j.contains("-video_size 640x480"));
        assert!(j.contains("audio=Microphone (X)"));
        assert!(j.contains("-map 1:a"));
    }

    #[test]
    fn windows_both_amix_two_dshow_inputs() {
        let args = build_args_windows(RGN, Some("Stereo Mix"), Some("Mic"), "C:/o.mp4");
        let j = args.join(" ");
        assert!(j.contains("audio=Stereo Mix"));
        assert!(j.contains("audio=Mic"));
        // mic (input 2) is boosted, then mixed with system (input 1).
        assert!(j.contains("[2:a]volume=10dB[m];[1:a][m]amix=inputs=2"));
        assert!(j.contains("-map 0:v"));
    }

    #[test]
    fn windows_mic_only_is_boosted() {
        let mic = build_args_windows(RGN, None, Some("Mic"), "C:/o.mp4").join(" ");
        assert!(mic.contains("-af volume=10dB"));
        let sys = build_args_windows(RGN, Some("Stereo Mix"), None, "C:/o.mp4").join(" ");
        assert!(!sys.contains("volume="));
    }

    #[test]
    fn windows_none_is_an() {
        let args = build_args_windows(RGN, None, None, "C:/o.mp4");
        assert!(args.join(" ").contains("-an"));
    }

    #[test]
    fn concat_list_one_line_per_segment() {
        let segs = vec![PathBuf::from("/tmp/seg-0.mp4"), PathBuf::from("/tmp/seg-1.mp4")];
        let body = concat_list_contents(&segs);
        assert_eq!(body, "file '/tmp/seg-0.mp4'\nfile '/tmp/seg-1.mp4'\n");
    }

    #[test]
    fn concat_list_escapes_single_quotes() {
        let segs = vec![PathBuf::from("/tmp/a'b.mp4")];
        // ffmpeg single-quote escape: ' -> '\''
        assert_eq!(concat_list_contents(&segs), "file '/tmp/a'\\''b.mp4'\n");
    }

    #[test]
    fn dshow_parser_extracts_audio_names() {
        let sample = "\
[dshow @ 0x1] DirectShow video devices
[dshow @ 0x1]  \"HD WebCam\"
[dshow @ 0x1] DirectShow audio devices
[dshow @ 0x1]  \"Microphone (Realtek)\"
[dshow @ 0x1]  \"Stereo Mix (Realtek)\"";
        let devs = parse_dshow_audio(sample);
        assert_eq!(devs, vec!["Microphone (Realtek)", "Stereo Mix (Realtek)"]);
        assert_eq!(pick_dshow_mic(&devs).as_deref(), Some("Microphone (Realtek)"));
        assert_eq!(pick_dshow_system(&devs).as_deref(), Some("Stereo Mix (Realtek)"));
    }
}
