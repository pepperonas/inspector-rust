//! Waveform peaks — the port of OpenScreen `electron/media/audioPeaks.ts` (`peakBlockCount`,
//! `PeakFolder`). Shared by the helper (decoded files) and the app (`mic.wav`), so both fold
//! the same way and a waveform never changes shape with where it was computed.
//!
//! The audio is mono 16 kHz; it is cut into at most [`MAX_PEAK_BLOCKS`] blocks,
//! [`PEAK_BLOCKS_PER_SEC`] per second below that, and each block keeps its `[min, max]`.

/// Sample rate the peaks are computed at (OpenScreen decodes `-ac 1 -ar 16000`).
pub const PEAK_RATE: u32 = 16_000;
pub const PEAK_BLOCKS_PER_SEC: f64 = 200.0;
pub const MAX_PEAK_BLOCKS: u32 = 24_000;

/// Number of `[min, max]` blocks for audio of `duration_sec` (`peakBlockCount`): at least one.
pub fn peak_block_count(duration_sec: f64) -> u32 {
    let d = if duration_sec.is_finite() { duration_sec.max(0.0) } else { 0.0 };
    ((d * PEAK_BLOCKS_PER_SEC).ceil() as u64).clamp(1, MAX_PEAK_BLOCKS as u64) as u32
}

/// Folds mono samples (−1..1) into `[min0, max0, min1, max1, …]` (`PeakFolder`). The block of
/// sample `i` is `floor(i / (total / blocks))`, the last block takes any overrun.
pub struct PeakFolder {
    peaks: Vec<f32>,
    blocks: usize,
    samples_per_block: f64,
    index: u64,
}

impl PeakFolder {
    pub fn new(blocks: u32, total_samples: u64) -> Self {
        let blocks = blocks.max(1) as usize;
        PeakFolder {
            peaks: vec![0.0; blocks * 2],
            blocks,
            samples_per_block: (total_samples as f64 / blocks as f64).max(1.0),
            index: 0,
        }
    }

    pub fn push(&mut self, value: f32) {
        let block = ((self.index as f64 / self.samples_per_block).floor() as usize).min(self.blocks - 1);
        let lo = block * 2;
        if value < self.peaks[lo] {
            self.peaks[lo] = value;
        }
        if value > self.peaks[lo + 1] {
            self.peaks[lo + 1] = value;
        }
        self.index += 1;
    }

    pub fn result(self) -> Vec<f32> {
        self.peaks
    }
}

/// Peaks of audio at `source_rate` (mono samples): brought to [`PEAK_RATE`] by averaging
/// whole groups (48 kHz → groups of three), then folded. The helper and the app both use it.
pub struct PeakBuilder {
    folder: PeakFolder,
    group: usize,
    sum: f32,
    n: usize,
}

impl PeakBuilder {
    pub fn new(duration_sec: f64, source_rate: u32) -> Self {
        let d = if duration_sec.is_finite() { duration_sec.max(0.0) } else { 0.0 };
        let total = (d * PEAK_RATE as f64).round() as u64;
        PeakBuilder {
            folder: PeakFolder::new(peak_block_count(d), total),
            group: (source_rate / PEAK_RATE).max(1) as usize,
            sum: 0.0,
            n: 0,
        }
    }

    pub fn push(&mut self, value: f32) {
        self.sum += value;
        self.n += 1;
        if self.n == self.group {
            self.folder.push(self.sum / self.n as f32);
            self.sum = 0.0;
            self.n = 0;
        }
    }

    pub fn finish(mut self) -> Vec<f32> {
        if self.n > 0 {
            self.folder.push(self.sum / self.n as f32);
        }
        self.folder.result()
    }
}

/// The peaks as the cache file holds them: f32 little endian, no header.
pub fn encode_peaks(peaks: &[f32]) -> Vec<u8> {
    peaks.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// The inverse of [`encode_peaks`]; `None` for a length that is not whole `[min, max]` pairs.
pub fn decode_peaks(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(8) {
        return None;
    }
    Some(bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_count_is_200_per_second_capped_at_24000() {
        assert_eq!(peak_block_count(0.0), 1);
        assert_eq!(peak_block_count(f64::NAN), 1);
        assert_eq!(peak_block_count(1.0), 200);
        assert_eq!(peak_block_count(1.001), 201);
        assert_eq!(peak_block_count(120.0), 24_000);
        assert_eq!(peak_block_count(3600.0), 24_000);
    }

    #[test]
    fn folder_keeps_min_and_max_per_block() {
        // 4 samples into 2 blocks.
        let mut f = PeakFolder::new(2, 4);
        for v in [0.5, -0.25, -0.75, 0.125] {
            f.push(v);
        }
        assert_eq!(f.result(), vec![-0.25, 0.5, -0.75, 0.125]);
        // Overrun lands in the last block, never out of range.
        let mut f = PeakFolder::new(1, 1);
        f.push(0.3);
        f.push(-0.9);
        assert_eq!(f.result(), vec![-0.9, 0.3]);
    }

    #[test]
    fn builder_averages_down_to_16k() {
        // 1 s at 48 kHz: 200 blocks of 80 averaged samples each.
        let mut b = PeakBuilder::new(1.0, 48_000);
        for i in 0..48_000 {
            b.push(if i < 240 { 0.6 } else { -0.3 });
        }
        let p = b.finish();
        assert_eq!(p.len(), 400);
        // Blocks start at 0 (`PeakFolder`), so a block always spans zero.
        assert_eq!((p[0], p[1]), (0.0, 0.6));
        assert!((p[2] + 0.3).abs() < 1e-6 && p[3] == 0.0);
        // A trailing partial group still counts.
        let mut b = PeakBuilder::new(0.001, 48_000);
        b.push(0.9);
        assert_eq!(b.finish(), vec![0.0, 0.9]);
    }

    #[test]
    fn cache_bytes_roundtrip() {
        let p = vec![-1.0, 1.0, -0.5, 0.25];
        assert_eq!(decode_peaks(&encode_peaks(&p)), Some(p));
        assert_eq!(decode_peaks(&[0; 4]), None);
        assert_eq!(decode_peaks(&[]), None);
    }
}
