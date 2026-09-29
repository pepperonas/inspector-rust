//! Stand-in for `cutout_ml.rs` on Intel Macs (`x86_64-apple-darwin`).
//!
//! `ort-sys` ships no prebuilt ONNX Runtime for Intel macOS, so the `ort`
//! dependency is target-gated off there (see `core/rust-lib/Cargo.toml`).
//! Everything else builds unchanged; only the ML background cut-out answers
//! with an honest error instead of a result.

use anyhow::Result;

pub const UNAVAILABLE: &str =
    "Background cut-out isn't available on Intel Macs (ONNX Runtime has no Intel-macOS build).";

pub fn cut_out_subject(_image_bytes: &[u8]) -> Result<Vec<u8>> {
    anyhow::bail!(UNAVAILABLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_unavailable_instead_of_pretending() {
        let err = cut_out_subject(&[1, 2, 3]).unwrap_err().to_string();
        assert!(err.contains("Intel"), "{err}");
    }
}
