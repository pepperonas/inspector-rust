//! Waveforms for the editor timeline: peaks of each recording's mic and system sound and of
//! every music/voiceover file, projected onto the programme clock (R12 — the UI never maps
//! source time itself).
//!
//! Peaks are `[min, max]` f32 pairs, 200 per second, at most 24 000 per file
//! (`record_render_protocol::peaks`). They are cached next to the project as
//! `peaks-<hash>.f32`, keyed by the file's path, size and modification time. `mic.wav` is read
//! here (`wav::read_mono`); anything else (the system sound inside `screen.mp4`, music files)
//! is decoded by the render helper (`Command::Peaks`).

use super::audio_tracks::TrackSpan;
use super::editor::{clip_source_at, programme_duration_ms, source_to_programme, EditDoc};
use super::wav;
use record_render_protocol::peaks::{decode_peaks, encode_peaks, PeakBuilder};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

/// The timeline gets at most this many blocks per second of programme…
pub const VIEW_BLOCKS_PER_SEC: f64 = 200.0;
/// …and at most this many blocks over all its waveforms together (one editor update carries
/// them; a long project gets a coarser waveform, never a heavier update).
pub const MAX_VIEW_BLOCKS: f64 = 40_000.0;

/// Cache file name for `path` as it is now (`audioPeaks.ts` `cacheKey`):
/// `peaks-<source>-<version>.f32`. `source` hashes the path, `version` the path, size and
/// modification time — a changed or replaced file gets a new name (stale peaks are never
/// served), and its older caches are found by the shared `source` part.
pub fn peaks_cache_name(path: &Path, size: u64, mtime_ns: u128) -> String {
    let p = path.to_string_lossy();
    format!("peaks-{}-{}.f32", hex16(&format!("src:{p}")), hex16(&format!("v1:{p}:{size}:{mtime_ns}")))
}

fn hex16(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Deletes `peaks-<same source>-*.f32` files in `dir` other than `current` (older versions of
/// the same file). Best effort.
pub fn remove_older_caches(dir: &Path, current: &str) {
    let Some(prefix) = current.strip_suffix(".f32").and_then(|n| n.rsplit_once('-')).map(|(src, _)| format!("{src}-")) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name != current && name.starts_with(&prefix) && name.ends_with(".f32") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// [`peaks_cache_name`] from the file's metadata; `None` when it does not exist.
pub fn cache_name_for(path: &Path) -> Option<String> {
    let m = std::fs::metadata(path).ok()?;
    let mtime = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    Some(peaks_cache_name(path, m.len(), mtime))
}

/// A file's waveform and the length of its audio.
#[derive(Debug, Clone, PartialEq)]
pub struct Peaks {
    pub data: Vec<f32>,
    pub duration_ms: u64,
}

impl Peaks {
    fn blocks(&self) -> usize {
        self.data.len() / 2
    }

    /// `[min, max]` over file time `[from_ms, to_ms)`; `(0, 0)` outside the audio.
    fn range(&self, from_ms: f64, to_ms: f64) -> (f32, f32) {
        let (b, d) = (self.blocks(), self.duration_ms as f64);
        if b == 0 || d <= 0.0 || to_ms <= 0.0 || from_ms >= d {
            return (0.0, 0.0);
        }
        let i0 = ((from_ms.max(0.0) / d * b as f64).floor() as usize).min(b - 1);
        let i1 = ((to_ms.min(d) / d * b as f64).ceil() as usize).clamp(i0 + 1, b);
        let (mut lo, mut hi) = (0.0f32, 0.0f32);
        for i in i0..i1 {
            lo = lo.min(self.data[2 * i]);
            hi = hi.max(self.data[2 * i + 1]);
        }
        (lo, hi)
    }
}

/// The peaks of a cache file, if it holds whole pairs.
pub fn read_cache(cache: &Path) -> Option<Vec<f32>> {
    decode_peaks(&std::fs::read(cache).ok()?)
}

/// Peaks of a WAV (`mic.wav`), through the cache file `cache`.
pub fn wav_peaks(path: &Path, cache: &Path) -> std::io::Result<Peaks> {
    // The duration first (one header read); a cache of the matching size is used as is.
    let info = wav::wav_info(path)?;
    let duration_sec = info.frames as f64 / info.sample_rate.max(1) as f64;
    let duration_ms = (duration_sec * 1000.0).round() as u64;
    let want = record_render_protocol::peaks::peak_block_count(duration_sec) as usize * 2;
    if let Some(data) = read_cache(cache).filter(|d| d.len() == want) {
        return Ok(Peaks { data, duration_ms });
    }
    let mut b = PeakBuilder::new(duration_sec, info.sample_rate);
    wav::read_mono(path, |v| b.push(v))?;
    let data = b.finish();
    let tmp = cache.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, encode_peaks(&data))?;
    std::fs::rename(&tmp, cache)?;
    Ok(Peaks { data, duration_ms })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WaveKind {
    Mic,
    System,
    Track,
}

/// One waveform on the programme clock: `peaks` holds `[min, max]` pairs spread evenly over
/// `prog_start_ms..prog_end_ms`. `clip` is set for mic/system, `track` (+ `repeat`) for tracks.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaveSpan {
    pub kind: WaveKind,
    pub clip: Option<usize>,
    pub track: Option<usize>,
    pub repeat: u32,
    pub prog_start_ms: f64,
    pub prog_end_ms: f64,
    pub peaks: Vec<f32>,
}

/// The audio of one recording: its mic (with `offset_ms`, file time = source − offset) and its
/// system sound (file time = source time).
#[derive(Debug, Clone, Default)]
pub struct SourcePeaks {
    pub mic: Option<(Arc<Peaks>, i64)>,
    pub system: Option<Arc<Peaks>>,
}

/// Programme ms → file ms for one waveform.
type FileTime<'a> = Box<dyn Fn(f64) -> f64 + 'a>;

/// One waveform before it is sampled.
struct Item<'a> {
    kind: WaveKind,
    clip: Option<usize>,
    track: Option<usize>,
    repeat: u32,
    start: f64,
    end: f64,
    map: FileTime<'a>,
    peaks: Arc<Peaks>,
}

/// Every waveform the timeline draws. `doc` is the document as it plays; `source` gives a
/// clip's recording's peaks (`None` = the project's take), `track` a track file's peaks.
pub fn project_waveforms(
    doc: &EditDoc,
    tracks: &[TrackSpan],
    source: impl Fn(Option<&str>) -> SourcePeaks,
    track: impl Fn(usize) -> Option<Arc<Peaks>>,
) -> Vec<WaveSpan> {
    let mut items: Vec<Item> = Vec::new();
    for (i, c) in doc.clips.iter().enumerate() {
        if c.end_ms <= c.start_ms {
            continue;
        }
        let (start, end) = (source_to_programme(doc, i, c.start_ms) as f64, source_to_programme(doc, i, c.end_ms) as f64);
        let s = source(c.source.as_deref());
        if let Some((p, off)) = s.mic {
            let map: FileTime = Box::new(move |t| clip_source_at(doc, i, t) - off as f64);
            items.push(Item { kind: WaveKind::Mic, clip: Some(i), track: None, repeat: 0, start, end, map, peaks: p });
        }
        if let Some(p) = s.system {
            let map: FileTime = Box::new(move |t| clip_source_at(doc, i, t));
            items.push(Item { kind: WaveKind::System, clip: Some(i), track: None, repeat: 0, start, end, map, peaks: p });
        }
    }
    let programme = programme_duration_ms(doc) as f64;
    for e in tracks {
        let Some(p) = track(e.index) else { continue };
        let end = e.prog_end_ms.min(programme.max(e.prog_start_ms));
        let (ps, fs) = (e.prog_start_ms, e.file_start_ms);
        items.push(Item { kind: WaveKind::Track, clip: None, track: Some(e.index), repeat: e.repeat, start: ps, end, map: Box::new(move |t| fs + (t - ps)), peaks: p });
    }
    let total: f64 = items.iter().map(|it| (it.end - it.start).max(0.0)).sum();
    let per_ms = if total > 0.0 { (VIEW_BLOCKS_PER_SEC / 1000.0).min(MAX_VIEW_BLOCKS / total) } else { 0.0 };
    items
        .into_iter()
        .filter(|it| it.end > it.start)
        .map(|Item { kind, clip, track, repeat, start, end, map, peaks: p }| {
            let n = (((end - start) * per_ms).ceil() as usize).max(1);
            let step = (end - start) / n as f64;
            let mut peaks = Vec::with_capacity(n * 2);
            for j in 0..n {
                let (a, b) = (map(start + j as f64 * step), map(start + (j + 1) as f64 * step));
                let (lo, hi) = p.range(a.min(b), a.max(b));
                peaks.push(lo);
                peaks.push(hi);
            }
            WaveSpan { kind, clip, track, repeat, prog_start_ms: start, prog_end_ms: end, peaks }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::audio_tracks::track_entries;
    use crate::record::editor::{EditClip, SpeedRegion};

    fn doc(clips: &[(u64, u64)]) -> EditDoc {
        let mut d = crate::record::editor::default_doc(&crate::record::editor::tests::meta(60_000));
        d.clips = clips.iter().map(|&(s, e)| EditClip::main(s, e)).collect();
        d.crop = vec![None; clips.len()];
        d
    }

    /// 1 s per block-pair index: block i spans [i, i+1) s, min = −(i+1)/100, max = (i+1)/100.
    fn ramp(seconds: usize) -> Arc<Peaks> {
        let data = (0..seconds).flat_map(|i| [-((i + 1) as f32) / 100.0, (i + 1) as f32 / 100.0]).collect();
        Arc::new(Peaks { data, duration_ms: seconds as u64 * 1000 })
    }

    #[test]
    fn peaks_block_count_and_cap() {
        let dir = std::env::temp_dir().join(format!("ir-peaks-core-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("mic.wav");
        let mut s = wav::WavSink::create(&wav, 48_000).unwrap();
        s.set_origin(0);
        s.push(0, &vec![0.25f32; 48_000 * 2 + 24_000]).unwrap(); // 2.5 s
        s.finish().unwrap();
        let cache = dir.join("peaks-mic.f32");
        let p = wav_peaks(&wav, &cache).unwrap();
        assert_eq!(p.duration_ms, 2500);
        assert_eq!(p.blocks(), 500, "200 blocks per second");
        assert!((p.data[1] - 0.25).abs() < 1e-3);
        assert_eq!(read_cache(&cache).unwrap(), p.data);
        // The cap: an hour is 24 000 blocks, not 720 000.
        assert_eq!(record_render_protocol::peaks::peak_block_count(3600.0), 24_000);
        // A cache of the right size is used as is.
        std::fs::write(&cache, encode_peaks(&vec![0.5; 1000])).unwrap();
        assert_eq!(wav_peaks(&wav, &cache).unwrap().data[0], 0.5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn peaks_cache_name_changes_with_mtime() {
        let p = Path::new("/p/screen.mp4");
        let a = peaks_cache_name(p, 100, 1);
        assert!(a.starts_with("peaks-") && a.ends_with(".f32") && a.len() == "peaks-".len() + 16 + 1 + 16 + 4, "{a}");
        // The same file in another version shares the source part.
        assert_eq!(a[..22], peaks_cache_name(p, 999, 7)[..22]);
        assert_eq!(a, peaks_cache_name(p, 100, 1));
        assert_ne!(a, peaks_cache_name(p, 100, 2));
        assert_ne!(a, peaks_cache_name(p, 101, 1));
        assert_ne!(a, peaks_cache_name(Path::new("/p/mic.wav"), 100, 1));
        assert_eq!(cache_name_for(Path::new("/no/such/file")), None);
    }

    #[test]
    fn older_caches_of_the_same_source_are_removed() {
        let dir = std::env::temp_dir().join(format!("ir-peaks-prune-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = Path::new("/p/screen.mp4");
        let (old, new, other) = (peaks_cache_name(p, 1, 1), peaks_cache_name(p, 2, 2), peaks_cache_name(Path::new("/p/mic.wav"), 1, 1));
        for n in [&old, &new, &other, &"notes.txt".to_string()] {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        remove_older_caches(&dir, &new);
        assert!(!dir.join(&old).exists(), "the older version is gone");
        assert!(dir.join(&new).exists() && dir.join(&other).exists() && dir.join("notes.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn waveforms_follow_cuts_and_speed() {
        // Clip 0: source 0–4 s with 2× over 2–4 s (programme 0–3 s); clip 1: source 6–8 s.
        let mut d = doc(&[(0, 4_000), (6_000, 8_000)]);
        d.speed = vec![SpeedRegion { clip: 0, start_ms: 2_000, end_ms: 4_000, speed: 2.0 }];
        let sys = ramp(10);
        let w = project_waveforms(&d, &[], |_| SourcePeaks { mic: None, system: Some(sys.clone()) }, |_| None);
        assert_eq!(w.len(), 2);
        assert_eq!((w[0].prog_start_ms, w[0].prog_end_ms, w[0].clip), (0.0, 3_000.0, Some(0)));
        assert_eq!((w[1].prog_start_ms, w[1].prog_end_ms), (3_000.0, 5_000.0));
        // 200 blocks/s of programme.
        assert_eq!(w[0].peaks.len(), 600 * 2);
        // The last programme block of clip 0 holds source second 3 (sped up), clip 1's first
        // holds source second 6 — the cut 4–6 s is gone.
        assert_eq!(w[0].peaks[w[0].peaks.len() - 1], 0.04);
        assert_eq!(w[1].peaks[1], 0.07);
    }

    #[test]
    fn waveforms_of_mic_and_tracks() {
        let mut d = doc(&[(0, 5_000)]);
        let mut t = crate::record::editor::AudioTrack {
            id: "m".into(),
            kind: crate::record::editor::TrackKind::Music,
            file: "m.wav".into(),
            clip: 0,
            start_ms: 1_000,
            duration_ms: 3_000,
            offset_ms: 1_000,
            span_ms: None,
            gain_db: 0.0,
            looped: false,
            fade_in_ms: 0,
            fade_out_ms: 0,
            muted: false,
            label: String::new(),
        };
        t.looped = false;
        d.audio_tracks = vec![t];
        let entries = track_entries(&d);
        let mic = ramp(10);
        let music = ramp(3);
        let w = project_waveforms(&d, &entries, |_| SourcePeaks { mic: Some((mic.clone(), 1_000)), system: None }, |_| Some(music.clone()));
        let m = w.iter().find(|w| w.kind == WaveKind::Mic).unwrap();
        // Mic file time = source − 1 s: programme 0 s is before the file (silence), 1.5 s is file second 0.
        assert_eq!((m.peaks[0], m.peaks[1]), (0.0, 0.0));
        assert_eq!(m.peaks[2 * 300 + 1], 0.01);
        let tr = w.iter().find(|w| w.kind == WaveKind::Track).unwrap();
        assert_eq!((tr.track, tr.prog_start_ms, tr.prog_end_ms), (Some(0), 1_000.0, 3_000.0));
        // It plays file 1–3 s: the first block is file second 1.
        assert_eq!(tr.peaks[1], 0.02);
    }

    #[test]
    fn waveforms_respect_the_total_budget() {
        let d = doc(&[(0, 3_600_000)]);
        let sys = Arc::new(Peaks { data: vec![0.0; 48_000], duration_ms: 3_600_000 });
        let w = project_waveforms(&d, &[], |_| SourcePeaks { mic: Some((sys.clone(), 0)), system: Some(sys.clone()) }, |_| None);
        let blocks: usize = w.iter().map(|w| w.peaks.len() / 2).sum();
        assert!(blocks <= MAX_VIEW_BLOCKS as usize + w.len(), "{blocks}");
        assert!(blocks >= MAX_VIEW_BLOCKS as usize - 2);
    }
}
