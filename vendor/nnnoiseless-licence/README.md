# nnnoiseless

Noise suppression for the recording editor's microphone (`core/rust-lib/src/record/denoise.rs`)
uses the Rust crate [nnnoiseless](https://github.com/jneem/nnnoiseless) 0.5.2, a port of
Xiph's RNNoise, linked into the app. Licence: BSD-3-Clause, see `COPYING` (copied from the
crate). The app bundle carries both files under `Contents/Resources/THIRD-PARTY/nnnoiseless/`.
