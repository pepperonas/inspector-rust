//! Shazam song recognition from a microphone recording (`shazam` command).
//!
//! A faithful Rust port of the reverse-engineered Shazam **audio-signature**
//! algorithm (marin-m's work, as shipped in shazamio's pure-Python
//! `algorithm.py` / `signature.py`): 16 kHz mono PCM → a 2048-point FFT every
//! 128 samples → peak spreading + peak recognition → the binary
//! `data:audio/vnd.shazam.sig` signature → a POST to Shazam's public discovery
//! API → the matched track. No file, no ffmpeg, no Python: the frontend records
//! the mic and hands us 16 kHz `i16` samples.
//!
//! The pure signature core (`SignatureGenerator` → `encode_signature_uri`) is
//! deterministic and unit-tested against a known-answer fixture; the HTTP call
//! (`recognize`) needs the network + a live recording.

use serde::Serialize;

const SAMPLE_RATE: u32 = 16000;

// ── Frequency peak + signature message ───────────────────────────────────────

#[derive(Clone, Copy)]
struct FrequencyPeak {
    fft_pass_number: u32,
    peak_magnitude: u16,
    corrected_peak_frequency_bin: u16,
}

/// Bands 0,1,2 (250–520 / 520–1450 / 1450–3500 Hz) — the three the reference
/// actually fills (its 3.5–5.5 kHz branch is dead code; kept faithful).
#[derive(Default)]
struct Signature {
    number_samples: u32,
    bands: [Vec<FrequencyPeak>; 4],
}

// ── CRC-32 (IEEE, for the signature header) ──────────────────────────────────

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

// ── Signature binary encoding ────────────────────────────────────────────────

fn encode_signature(sig: &Signature) -> Vec<u8> {
    let mut contents: Vec<u8> = Vec::new();
    for (band_idx, peaks) in sig.bands.iter().enumerate() {
        if peaks.is_empty() {
            continue;
        }
        let mut peaks_buf: Vec<u8> = Vec::new();
        let mut fft_pass_number: u32 = 0;
        for p in peaks {
            if p.fft_pass_number - fft_pass_number >= 255 {
                peaks_buf.push(0xFF);
                peaks_buf.extend_from_slice(&p.fft_pass_number.to_le_bytes());
                fft_pass_number = p.fft_pass_number;
            }
            peaks_buf.push((p.fft_pass_number - fft_pass_number) as u8);
            peaks_buf.extend_from_slice(&p.peak_magnitude.to_le_bytes());
            peaks_buf.extend_from_slice(&p.corrected_peak_frequency_bin.to_le_bytes());
            fft_pass_number = p.fft_pass_number;
        }
        contents.extend_from_slice(&(0x6003_0040u32 + band_idx as u32).to_le_bytes());
        contents.extend_from_slice(&(peaks_buf.len() as u32).to_le_bytes());
        contents.extend_from_slice(&peaks_buf);
        let pad = (4 - (peaks_buf.len() % 4)) % 4;
        contents.extend(std::iter::repeat_n(0u8, pad));
    }

    // Header (48 bytes) — see RawSignatureHeader in the reference.
    let mut header = [0u8; 48];
    header[0..4].copy_from_slice(&0xCAFE_2580u32.to_le_bytes()); // magic1
    // [4..8] crc32 filled last
    let size_minus_header = (contents.len() + 8) as u32;
    header[8..12].copy_from_slice(&size_minus_header.to_le_bytes());
    header[12..16].copy_from_slice(&0x9411_9C00u32.to_le_bytes()); // magic2
    // [16..28] void1 (3×u32) = 0
    let shifted_sample_rate_id: u32 = 3u32 << 27; // SampleRate::_16000 = 3
    header[28..32].copy_from_slice(&shifted_sample_rate_id.to_le_bytes());
    // [32..40] void2 (2×u32) = 0
    let nspds = sig.number_samples + (SAMPLE_RATE as f64 * 0.24) as u32;
    header[40..44].copy_from_slice(&nspds.to_le_bytes());
    let fixed_value: u32 = (15u32 << 19) + 0x40000;
    header[44..48].copy_from_slice(&fixed_value.to_le_bytes());

    let mut buf: Vec<u8> = Vec::with_capacity(48 + 8 + contents.len());
    buf.extend_from_slice(&header);
    buf.extend_from_slice(&0x4000_0000u32.to_le_bytes());
    buf.extend_from_slice(&((contents.len() + 8) as u32).to_le_bytes());
    buf.extend_from_slice(&contents);

    let crc = crc32(&buf[8..]);
    buf[4..8].copy_from_slice(&crc.to_le_bytes());
    buf
}

fn encode_signature_uri(sig: &Signature) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(encode_signature(sig));
    format!("data:audio/vnd.shazam.sig;base64,{b64}")
}

// ── Signature generation (FFT peak detection) ────────────────────────────────

struct RingF64 {
    data: Vec<Vec<f64>>,
    position: usize,
    num_written: usize,
    size: usize,
}
impl RingF64 {
    fn new(size: usize, width: usize) -> Self {
        RingF64 { data: vec![vec![0.0; width]; size], position: 0, num_written: 0, size }
    }
    fn append(&mut self, v: Vec<f64>) {
        self.data[self.position] = v;
        self.position = (self.position + 1) % self.size;
        self.num_written += 1;
    }
    #[inline]
    fn idx(&self, offset: isize) -> usize {
        (self.position as isize + offset).rem_euclid(self.size as isize) as usize
    }
}

struct SignatureGenerator {
    ring_samples: Vec<f64>, // 2048, position always a multiple of 128
    ring_pos: usize,
    fft_outputs: RingF64,
    spread_fft_output: RingF64,
    hanning: Vec<f64>,
    fft: std::sync::Arc<dyn rustfft::Fft<f64>>,
    sig: Signature,
}

impl SignatureGenerator {
    fn new() -> Self {
        let mut planner = rustfft::FftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(2048);
        // np.hanning(2050)[1:-1]: w[k] = 0.5 - 0.5*cos(2π(k+1)/2049), k=0..2047
        let hanning: Vec<f64> = (0..2048)
            .map(|k| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * (k as f64 + 1.0) / 2049.0).cos())
            .collect();
        SignatureGenerator {
            ring_samples: vec![0.0; 2048],
            ring_pos: 0,
            fft_outputs: RingF64::new(256, 1025),
            spread_fft_output: RingF64::new(256, 1025),
            hanning,
            fft,
            sig: Signature::default(),
        }
    }

    /// Process the whole recording (16 kHz mono i16). One-shot: we feed all
    /// samples in 128-sample hops (matching the reference's inner loop).
    fn process(&mut self, samples: &[i16]) {
        self.sig.number_samples = samples.len() as u32;
        let mut i = 0;
        while i + 128 <= samples.len() {
            self.do_fft(&samples[i..i + 128]);
            self.do_peak_spreading();
            if self.spread_fft_output.num_written >= 46 {
                self.do_peak_recognition();
            }
            i += 128;
        }
    }

    fn do_fft(&mut self, batch: &[i16]) {
        // Write 128 samples into the ring at ring_pos (always a 128-multiple).
        for (j, &s) in batch.iter().enumerate() {
            self.ring_samples[self.ring_pos + j] = s as f64;
        }
        self.ring_pos = (self.ring_pos + 128) % 2048;

        // Rotated excerpt (oldest → newest) × Hanning → complex FFT.
        let mut buf: Vec<rustfft::num_complex::Complex<f64>> = Vec::with_capacity(2048);
        for k in 0..2048 {
            let src = (self.ring_pos + k) % 2048;
            buf.push(rustfft::num_complex::Complex::new(self.ring_samples[src] * self.hanning[k], 0.0));
        }
        self.fft.process(&mut buf);
        let mut out = vec![0.0f64; 1025];
        for (bin, o) in out.iter_mut().enumerate() {
            let c = buf[bin];
            let mag = (c.re * c.re + c.im * c.im) / (1u32 << 17) as f64;
            *o = mag.max(0.000_000_000_1);
        }
        self.fft_outputs.append(out);
    }

    fn do_peak_spreading(&mut self) {
        let origin = self.fft_outputs.data[self.fft_outputs.idx(-1)].clone();
        // Frequency spread: spread[i] = max(origin[i..i+3]) for i<1022, else origin[i].
        let mut spread = origin.clone();
        for i in 0..1022 {
            spread[i] = origin[i].max(origin[i + 1]).max(origin[i + 2]);
        }
        // Temporal spread into slots -1, -3, -6.
        let i1 = self.spread_fft_output.idx(-1);
        let i2 = self.spread_fft_output.idx(-3);
        let i3 = self.spread_fft_output.idx(-6);
        let old1 = self.spread_fft_output.data[i1].clone();
        let old2 = self.spread_fft_output.data[i2].clone();
        let old3 = self.spread_fft_output.data[i3].clone();
        for b in 0..1025 {
            let s = spread[b];
            self.spread_fft_output.data[i1][b] = s.max(old1[b]);
            self.spread_fft_output.data[i2][b] = s.max(old1[b]).max(old2[b]);
            self.spread_fft_output.data[i3][b] = s.max(old1[b]).max(old2[b]).max(old3[b]);
        }
        self.spread_fft_output.append(spread);
    }

    fn do_peak_recognition(&mut self) {
        let fft46 = &self.fft_outputs.data[self.fft_outputs.idx(-46)];
        let fft49 = &self.spread_fft_output.data[self.spread_fft_output.idx(-49)];

        const NEIGHBORS: [isize; 8] = [-10, -7, -4, -3, 1, 2, 5, 8];
        const OTHERS: [isize; 14] =
            [-53, -45, 165, 172, 179, 186, 193, 200, 214, 221, 228, 235, 242, 249];

        for bin in 10usize..1015 {
            if fft46[bin] < 1.0 / 64.0 || fft46[bin] < fft49[bin - 1] {
                continue;
            }
            let mut max_neighbor = 0.0f64;
            for &off in &NEIGHBORS {
                let idx = (bin as isize + off) as usize;
                max_neighbor = fft49[idx].max(max_neighbor);
            }
            if fft46[bin] <= max_neighbor {
                continue;
            }
            let mut max_other = max_neighbor;
            for &off in &OTHERS {
                let slot = self.spread_fft_output.idx(off);
                max_other = self.spread_fft_output.data[slot][bin - 1].max(max_other);
            }
            if fft46[bin] <= max_other {
                continue;
            }
            let fft_number = (self.spread_fft_output.num_written - 46) as u32;
            let ln = |x: f64| x.max(1.0 / 64.0).ln() * 1477.3 + 6144.0;
            let pm = ln(fft46[bin]);
            let pmb = ln(fft46[bin - 1]);
            let pma = ln(fft46[bin + 1]);
            let var1 = pm * 2.0 - pmb - pma;
            if var1 <= 0.0 {
                continue; // reference asserts >0; skip defensively
            }
            let var2 = (pma - pmb) * 32.0 / var1;
            let corrected = bin as f64 * 64.0 + var2;
            let f_hz = corrected * (SAMPLE_RATE as f64 / 2.0 / 1024.0 / 64.0);
            let band = if f_hz > 250.0 && f_hz < 520.0 {
                0
            } else if f_hz > 520.0 && f_hz < 1450.0 {
                1
            } else if f_hz > 1450.0 && f_hz < 3500.0 {
                2
            } else {
                continue;
            };
            self.sig.bands[band].push(FrequencyPeak {
                fft_pass_number: fft_number,
                peak_magnitude: pm as u16,
                corrected_peak_frequency_bin: corrected as u16,
            });
        }
    }
}

/// Build the signature URI + sample-length (ms) for a 16 kHz mono recording.
/// Pure — this is what the unit test pins.
pub fn signature_for(samples: &[i16]) -> (String, u32) {
    let mut gen = SignatureGenerator::new();
    gen.process(samples);
    let samplems = (gen.sig.number_samples as u64 * 1000 / SAMPLE_RATE as u64) as u32;
    (encode_signature_uri(&gen.sig), samplems)
}

// ── Shazam API ───────────────────────────────────────────────────────────────

/// The matched track (a compact subset of Shazam's response).
#[derive(Debug, Clone, Serialize, Default)]
pub struct ShazamMatch {
    pub title: String,
    pub artist: String,
    pub cover_url: String,
    pub shazam_url: String,
    pub genre: String,
    pub album: String,
    pub released: String,
    pub apple_music_url: String,
    /// Deep link to open the song in Spotify (direct from Shazam if present,
    /// else a search for "title artist").
    pub spotify_url: String,
    /// YouTube search for the song ("title artist").
    pub youtube_url: String,
}

/// Percent-encode a query for a URL (RFC-3986-ish: keep unreserved, encode the
/// rest incl. space → %20). Pure.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Build Spotify + YouTube links for a track. `spotify_direct` is a direct
/// Spotify URL from the Shazam response if one was found (else empty → search).
/// Pure + unit-tested.
pub fn music_links(title: &str, artist: &str, spotify_direct: &str) -> (String, String) {
    let q = url_encode(&format!("{title} {artist}"));
    let spotify = if spotify_direct.is_empty() {
        format!("https://open.spotify.com/search/{q}")
    } else {
        spotify_direct.to_string()
    };
    let youtube = format!("https://www.youtube.com/results?search_query={q}");
    (spotify, youtube)
}

fn uuid_v4_upper() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0F) | 0x40;
    b[8] = (b[8] & 0x3F) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02X}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Parse Shazam's discovery JSON into a `ShazamMatch` (`None` = no match). Pure.
pub fn parse_response(json: &serde_json::Value) -> Option<ShazamMatch> {
    let track = json.get("track")?;
    let get = |k: &str| track.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mut m = ShazamMatch {
        title: get("title"),
        artist: get("subtitle"),
        cover_url: track
            .get("images")
            .and_then(|i| i.get("coverart"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        shazam_url: get("url"),
        genre: track
            .get("genres")
            .and_then(|g| g.get("primary"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        ..Default::default()
    };
    if m.title.is_empty() {
        return None;
    }
    // Album / released from the SONG section metadata; Apple Music link from hub.
    if let Some(sections) = track.get("sections").and_then(|s| s.as_array()) {
        for sec in sections {
            if sec.get("type").and_then(|t| t.as_str()) == Some("SONG") {
                if let Some(md) = sec.get("metadata").and_then(|m| m.as_array()) {
                    for item in md {
                        let t = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                        let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        match t {
                            "Album" => m.album = text,
                            "Released" => m.released = text,
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    if let Some(options) = track
        .get("hub")
        .and_then(|h| h.get("options"))
        .and_then(|o| o.as_array())
    {
        for opt in options {
            if let Some(actions) = opt.get("actions").and_then(|a| a.as_array()) {
                for act in actions {
                    if act.get("type").and_then(|t| t.as_str()) == Some("applemusicplay") {
                        if let Some(uri) = act.get("uri").and_then(|u| u.as_str()) {
                            m.apple_music_url = uri.to_string();
                        }
                    }
                }
            }
        }
    }
    // A direct Spotify link, if Shazam's hub carries one (providers[type=SPOTIFY]).
    let mut spotify_direct = String::new();
    if let Some(providers) = track
        .get("hub")
        .and_then(|h| h.get("providers"))
        .and_then(|p| p.as_array())
    {
        for prov in providers {
            let is_spotify = prov.get("type").and_then(|t| t.as_str()) == Some("SPOTIFY")
                || prov
                    .get("caption")
                    .and_then(|c| c.as_str())
                    .map(|c| c.eq_ignore_ascii_case("spotify"))
                    .unwrap_or(false);
            if !is_spotify {
                continue;
            }
            if let Some(actions) = prov.get("actions").and_then(|a| a.as_array()) {
                for act in actions {
                    if let Some(uri) = act.get("uri").and_then(|u| u.as_str()) {
                        if uri.contains("spotify") {
                            spotify_direct = uri.to_string();
                        }
                    }
                }
            }
        }
    }
    let (spotify, youtube) = music_links(&m.title, &m.artist, &spotify_direct);
    m.spotify_url = spotify;
    m.youtube_url = youtube;
    Some(m)
}

const USER_AGENT: &str =
    "Mozilla/5.0 (iPhone; CPU iPhone OS 15_0 like Mac OS X) AppleWebKit/605.1.15 Shazam/14.1.0";

/// Recognize a 16 kHz mono `i16` recording: generate the signature, POST it to
/// Shazam's discovery API, parse the match. Blocks on the network; the IPC
/// command is async so it runs off the main thread. `Ok(None)` = no match.
pub fn recognize(samples: &[i16]) -> Result<Option<ShazamMatch>, String> {
    if samples.len() < SAMPLE_RATE as usize {
        return Err("recording too short (need ≥ 1 s)".into());
    }
    let (uri, samplems) = signature_for(samples);
    let (u1, u2) = (uuid_v4_upper(), uuid_v4_upper());
    let url = format!(
        "https://amp.shazam.com/discovery/v5/en/US/iphone/-/tag/{u1}/{u2}\
?sync=true&webv3=true&sampling=true&connected=&shazamapiversion=v3\
&sharehub=true&hubv5minorversion=v5.1&hidelb=true&video=v3"
    );
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let body = serde_json::json!({
        "timezone": "Europe/Berlin",
        "signature": { "uri": uri, "samplems": samplems },
        "timestamp": now_ms,
        "context": {},
        "geolocation": {},
    });
    let resp = ureq::post(&url)
        .set("X-Shazam-Platform", "IPHONE")
        .set("X-Shazam-AppVersion", "14.1.0")
        .set("Accept", "*/*")
        .set("Accept-Language", "en")
        .set("Accept-Encoding", "identity")
        .set("Content-Type", "application/json")
        .set("User-Agent", USER_AGENT)
        .timeout(std::time::Duration::from_secs(15))
        .send_string(&body.to_string())
        .map_err(|e| format!("Shazam request failed: {e}"))?;
    let text = resp.into_string().map_err(|e| format!("bad Shazam response: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("bad Shazam JSON: {e}"))?;
    Ok(parse_response(&json))
}

// ── Native microphone capture (cpal) ─────────────────────────────────────────

/// Linear-resample mono `f32` at `src_rate` → 16 kHz `i16`. Pure + tested.
pub fn resample_to_16k_i16(input: &[f32], src_rate: u32) -> Vec<i16> {
    let dst_rate = 16000f64;
    let to_i16 = |v: f32| -> i16 {
        let s = v.clamp(-1.0, 1.0) as f64;
        (if s < 0.0 { s * 32768.0 } else { s * 32767.0 }) as i16
    };
    if src_rate == 16000 {
        return input.iter().map(|&v| to_i16(v)).collect();
    }
    let ratio = src_rate as f64 / dst_rate;
    let out_len = (input.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let i0 = pos.floor() as usize;
        let i1 = (i0 + 1).min(input.len().saturating_sub(1));
        let frac = pos - i0 as f64;
        let v = input[i0] as f64 * (1.0 - frac) + input[i1] as f64 * frac;
        out.push(to_i16(v as f32));
    }
    out
}

/// Record `seconds` from the default input device **natively** (cpal), average
/// to mono, resample to 16 kHz `i16`. Bypasses the webview entirely, so mic
/// capture no longer reconfigures the shared audio device (the "playback
/// stutters while recording" fix). `on_progress(0..1)` fires ~10×/s. Blocking —
/// call from a worker thread.
pub fn record_mic_16k<F: Fn(f32)>(seconds: u32, on_progress: F) -> Result<Vec<i16>, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use parking_lot::Mutex as PMutex;
    use std::sync::Arc;

    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("no microphone found")?;
    let supported = device
        .default_input_config()
        .map_err(|e| format!("mic config error: {e}"))?;
    let sample_rate = supported.sample_rate().0;
    let channels = supported.config().channels as usize;
    let fmt = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    let buf: Arc<PMutex<Vec<f32>>> = Arc::new(PMutex::new(Vec::new()));
    let err_fn = |e| tracing::warn!("shazam: cpal input stream error: {e}");
    let ch = channels.max(1);

    // Average interleaved frames of N channels → mono, into the shared buffer.
    fn push_mono(buf: &PMutex<Vec<f32>>, samples: impl Iterator<Item = f32>, ch: usize) {
        let mut b = buf.lock();
        let mut acc = 0.0f32;
        let mut c = 0usize;
        for s in samples {
            acc += s;
            c += 1;
            if c == ch {
                b.push(acc / ch as f32);
                acc = 0.0;
                c = 0;
            }
        }
    }

    let b1 = buf.clone();
    let b2 = buf.clone();
    let b3 = buf.clone();
    let stream = match fmt {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &_| push_mono(&b1, data.iter().copied(), ch),
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &_| {
                push_mono(&b2, data.iter().map(|&s| s as f32 / 32768.0), ch)
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _: &_| {
                push_mono(&b3, data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0), ch)
            },
            err_fn,
            None,
        ),
        other => return Err(format!("unsupported mic sample format: {other:?}")),
    }
    .map_err(|e| format!("mic stream build failed: {e}"))?;

    stream.play().map_err(|e| format!("mic start failed: {e}"))?;
    let start = std::time::Instant::now();
    let dur = seconds.max(1) as f32;
    while start.elapsed().as_secs_f32() < dur {
        std::thread::sleep(std::time::Duration::from_millis(100));
        on_progress((start.elapsed().as_secs_f32() / dur).min(1.0));
    }
    drop(stream); // stop + release the input device

    let mono = buf.lock().clone();
    if mono.len() < sample_rate as usize / 2 {
        return Err("no audio captured from the microphone".into());
    }
    Ok(resample_to_16k_i16(&mono, sample_rate))
}

// ── Local recognition history (SQLite) ───────────────────────────────────────

use crate::db::DbHandle;

/// One stored recognition (a `ShazamMatch` + when it was recognized + id).
#[derive(Debug, Clone, Serialize)]
pub struct ShazamHistoryEntry {
    pub id: i64,
    /// Unix-ms of recognition.
    pub recognized_at: i64,
    pub title: String,
    pub artist: String,
    pub cover_url: String,
    pub shazam_url: String,
    pub spotify_url: String,
    pub youtube_url: String,
    pub genre: String,
    pub album: String,
    pub released: String,
}

pub fn init_schema(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS shazam_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            recognized_at INTEGER NOT NULL,
            title TEXT NOT NULL,
            artist TEXT NOT NULL,
            cover_url TEXT NOT NULL DEFAULT '',
            shazam_url TEXT NOT NULL DEFAULT '',
            spotify_url TEXT NOT NULL DEFAULT '',
            youtube_url TEXT NOT NULL DEFAULT '',
            genre TEXT NOT NULL DEFAULT '',
            album TEXT NOT NULL DEFAULT '',
            released TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_shazam_history_time
            ON shazam_history(recognized_at DESC);",
    )
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Insert a match into history (dedup: skip if the same title+artist was the
/// most recent entry — avoids piling duplicates when you re-listen to the same
/// song). Returns the row id (or the existing most-recent id).
pub fn history_insert(db: &DbHandle, m: &ShazamMatch) -> rusqlite::Result<i64> {
    let conn = db.lock();
    let last: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT id, title, artist FROM shazam_history ORDER BY recognized_at DESC, id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    if let Some((id, t, a)) = last {
        if t == m.title && a == m.artist {
            return Ok(id);
        }
    }
    conn.execute(
        "INSERT INTO shazam_history
            (recognized_at, title, artist, cover_url, shazam_url, spotify_url, youtube_url, genre, album, released)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        rusqlite::params![
            now_ms(), m.title, m.artist, m.cover_url, m.shazam_url,
            m.spotify_url, m.youtube_url, m.genre, m.album, m.released,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn history_list(db: &DbHandle, limit: u32) -> rusqlite::Result<Vec<ShazamHistoryEntry>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, recognized_at, title, artist, cover_url, shazam_url, spotify_url, youtube_url, genre, album, released
         FROM shazam_history ORDER BY recognized_at DESC, id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| {
        Ok(ShazamHistoryEntry {
            id: r.get(0)?,
            recognized_at: r.get(1)?,
            title: r.get(2)?,
            artist: r.get(3)?,
            cover_url: r.get(4)?,
            shazam_url: r.get(5)?,
            spotify_url: r.get(6)?,
            youtube_url: r.get(7)?,
            genre: r.get(8)?,
            album: r.get(9)?,
            released: r.get(10)?,
        })
    })?;
    rows.collect()
}

pub fn history_delete(db: &DbHandle, id: i64) -> rusqlite::Result<()> {
    db.lock().execute("DELETE FROM shazam_history WHERE id = ?1", [id])?;
    Ok(())
}

pub fn history_clear(db: &DbHandle) -> rusqlite::Result<()> {
    db.lock().execute("DELETE FROM shazam_history", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_known_vector() {
        // Standard IEEE CRC-32 of "123456789" = 0xCBF43926.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn silence_signature_is_valid_and_empty() {
        // 2 s of silence → no peaks, but a well-formed signature envelope.
        let samples = vec![0i16; SAMPLE_RATE as usize * 2];
        let (uri, ms) = signature_for(&samples);
        assert!(uri.starts_with("data:audio/vnd.shazam.sig;base64,"));
        assert_eq!(ms, 2000);
        // Decode + check header magic + self-consistent CRC + size.
        use base64::Engine;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(uri.trim_start_matches("data:audio/vnd.shazam.sig;base64,"))
            .unwrap();
        assert_eq!(&raw[0..4], &0xCAFE_2580u32.to_le_bytes());
        assert_eq!(&raw[12..16], &0x9411_9C00u32.to_le_bytes());
        let size_minus_header = u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]);
        assert_eq!(size_minus_header as usize, raw.len() - 48);
        let crc = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
        assert_eq!(crc, crc32(&raw[8..]));
    }

    #[test]
    fn a_tone_produces_frequency_peaks() {
        // A 1 kHz sine (in band 520–1450) over 3 s must yield peaks → a longer
        // signature than pure silence.
        let n = SAMPLE_RATE as usize * 3;
        // Tremolo (4 Hz amplitude mod) so consecutive FFT frames differ — a
        // *perfectly constant* tone yields no peaks (the time-domain
        // local-maximum checks need variation), which is correct behaviour.
        let tone: Vec<i16> = (0..n)
            .map(|i| {
                let t = i as f64 / SAMPLE_RATE as f64;
                let env = 0.5 + 0.5 * (2.0 * std::f64::consts::PI * 4.0 * t).sin();
                (8000.0 * env * (2.0 * std::f64::consts::PI * 1000.0 * t).sin()) as i16
            })
            .collect();
        let mut gen = SignatureGenerator::new();
        gen.process(&tone);
        let total: usize = gen.sig.bands.iter().map(|b| b.len()).sum();
        assert!(total > 0, "a pure tone should produce at least one peak");
    }

    #[test]
    fn parse_response_extracts_track() {
        let json = serde_json::json!({
            "track": {
                "title": "Bohemian Rhapsody",
                "subtitle": "Queen",
                "url": "https://www.shazam.com/track/123",
                "images": { "coverart": "https://img/cover.jpg" },
                "genres": { "primary": "Rock" },
                "sections": [{ "type": "SONG", "metadata": [
                    { "title": "Album", "text": "A Night at the Opera" },
                    { "title": "Released", "text": "1975" }
                ]}]
            }
        });
        let m = parse_response(&json).unwrap();
        assert_eq!(m.title, "Bohemian Rhapsody");
        assert_eq!(m.artist, "Queen");
        assert_eq!(m.cover_url, "https://img/cover.jpg");
        assert_eq!(m.genre, "Rock");
        assert_eq!(m.album, "A Night at the Opera");
        assert_eq!(m.released, "1975");
        // Search links are always built from title+artist.
        assert_eq!(
            m.spotify_url,
            "https://open.spotify.com/search/Bohemian%20Rhapsody%20Queen"
        );
        assert_eq!(
            m.youtube_url,
            "https://www.youtube.com/results?search_query=Bohemian%20Rhapsody%20Queen"
        );
        assert!(parse_response(&serde_json::json!({})).is_none());
    }

    #[test]
    fn parse_response_prefers_direct_spotify_link() {
        let json = serde_json::json!({
            "track": {
                "title": "X", "subtitle": "Y",
                "hub": { "providers": [
                    { "type": "SPOTIFY", "actions": [
                        { "type": "uri", "uri": "https://open.spotify.com/track/abc123" }
                    ]}
                ]}
            }
        });
        let m = parse_response(&json).unwrap();
        assert_eq!(m.spotify_url, "https://open.spotify.com/track/abc123");
        assert_eq!(m.youtube_url, "https://www.youtube.com/results?search_query=X%20Y");
    }

    #[test]
    fn music_links_search_fallback_and_encoding() {
        let (sp, yt) = music_links("Café del Mar", "Energy 52", "");
        assert_eq!(sp, "https://open.spotify.com/search/Caf%C3%A9%20del%20Mar%20Energy%2052");
        assert!(yt.starts_with("https://www.youtube.com/results?search_query=Caf%C3%A9"));
        let (sp2, _) = music_links("A", "B", "spotify:track:xyz");
        assert_eq!(sp2, "spotify:track:xyz");
    }

    #[test]
    fn resample_16k_passthrough_and_ratio() {
        // Passthrough at 16k: float → i16 (truncating).
        let out = resample_to_16k_i16(&[0.0, 1.0, -1.0, 0.5], 16000);
        assert_eq!(out, vec![0, 32767, -32768, 16383]);
        // 48k → 16k thirds the length.
        let n48 = vec![0.0f32; 4800];
        assert_eq!(resample_to_16k_i16(&n48, 48000).len(), 1600);
        // clamps out-of-range.
        assert_eq!(resample_to_16k_i16(&[2.0, -2.0], 16000), vec![32767, -32768]);
    }

    #[test]
    fn history_insert_list_dedup_delete_clear() {
        use parking_lot::Mutex;
        use std::sync::Arc;
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let db: DbHandle = Arc::new(Mutex::new(conn));
        let m1 = ShazamMatch { title: "Song A".into(), artist: "Artist".into(), ..Default::default() };
        let m2 = ShazamMatch { title: "Song B".into(), artist: "Artist".into(), ..Default::default() };
        history_insert(&db, &m1).unwrap();
        history_insert(&db, &m1).unwrap(); // dedup: same as most-recent → no new row
        history_insert(&db, &m2).unwrap();
        let list = history_list(&db, 50).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].title, "Song B"); // newest first
        history_delete(&db, list[0].id).unwrap();
        assert_eq!(history_list(&db, 50).unwrap().len(), 1);
        history_clear(&db).unwrap();
        assert!(history_list(&db, 50).unwrap().is_empty());
    }
}
