//! Live transcription engine, free of Tauri. See
//! `docs/superpowers/specs/2026-11-03-live-transcription-design.md`.
pub use transcribe_protocol as protocol;
pub mod text;
pub mod writer;
pub mod segment;
pub mod dedupe;
pub mod queue;
pub mod model;
pub mod lang;
pub mod vad;
pub mod decode;
pub mod resample;
pub mod capture;
pub mod pipeline;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod engine;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod activity_macos;

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    /// A 16 kHz mono WAV from `tests/fixtures` as f32 samples.
    pub fn fixture(name: &str) -> Vec<f32> {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        let mut r = hound::WavReader::open(&p).unwrap();
        assert_eq!(r.spec().sample_rate, 16_000);
        r.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect()
    }
}
