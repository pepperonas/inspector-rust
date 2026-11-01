# Vendored: OpenScreen compositor

- **Upstream:** https://github.com/getopenscreen/openscreen, `crates/compositor`
- **Version / commit:** v2.1.2, `bfd5bc32924508a6515e7ae8cd1febcb0e84833b`
- **License:** MIT — `LICENSE` (OpenScreen) and `THIRD-PARTY-NOTICES.md` (incl. the Recordly MIT
  notice for parts of the zoom/smoothing/cursor code), both copied verbatim from the repo root.
- **Vendored on:** 2026-11-01 for Inspector Rust `record` stage 2 (editor + export).
- **Recon / API map:** `RECON.md`.
- **ffmpeg:** not vendored here. `scripts/build-ffmpeg-macos.sh` builds LGPL ffmpeg 8.1.2 (shared)
  into `third_party/ffmpeg/8.1.2` (gitignored).

Inspector Rust builds this crate only for **macOS on Apple Silicon with an ffmpeg tree**. Everywhere
else (Linux CI, Windows, Intel macOS, or macOS without `third_party/ffmpeg/8.1.2`) it compiles to an
empty library with no runtime dependencies.

## What upstream's build.rs and shim.c do (read, not executed)

`build.rs` (501 LOC upstream), per target:
- macOS: sets `LIBCLANG_PATH` to the Xcode/CLT toolchain (`xcrun --find clang` → `../lib`) because
  upstream's `crates/.cargo/config.toml` forces a Windows value; picks the ffmpeg tree from
  `MAC_FFMPEG_DIR`, else `crates/thirdparty/ffmpeg-n8.1.2-macos64-lgpl-shared`, else `FFMPEG_DIR`;
  **panics** without one.
- Links dylibs avformat, avcodec, avutil, swscale, swresample, avfilter from `<tree>/lib`.
- Compiles `shim.c` with `cc`.
- Runs bindgen 0.70 over `wrapper_macos.h` (Windows/Linux: their own wrapper) with allowlists
  `av.*`, `avcodec_.*`, `avformat_.*`, `avio_.*`, `swr_.*`, `sws_.*`, types `AV.*`/Swr/Sws, vars
  `SWS_/AV_/AVERROR/FF_/AVIO_`, ModuleConsts enums, no layout tests; on macOS clang args
  `--target=aarch64-apple-darwin` (hard-coded) and `-isysroot $(xcrun --show-sdk-path)`.
  Output: `$OUT_DIR/ffi.rs`, included by `src/ffi.rs`.
- Linux only: gcc freestanding headers for clang, drops an unusable `LIBCLANG_PATH`, optional
  symbol renaming with `OPENSCREEN_FFMPEG_SYMBOL_PREFIX`.
- Windows only: compiles `src/shaders.hlsl` with `D3DCompile` into `.cso` blobs.
- No network access, no files written outside `OUT_DIR`. Metal shaders are compiled at runtime.

`shim.c` (23 LOC): accessors for `AVFormatContext` fields that bindgen leaves opaque
(`streams[i]`, `nb_streams`, `duration`, `pb` get/set) and `AVERROR(EAGAIN)` / `AVERROR_EOF`
computed in C — EAGAIN is −35 on macOS, not −11; upstream hard-coded −11 once and decoded nothing.

## Copied

- `src/` complete (all three backends; the Windows/Linux files are never compiled here, but
  `sculpt.rs` tests `include_str!` `shaders.hlsl` and `vk_shaders/*.wgsl`, and keeping them
  makes diffs against upstream trivial).
- `shim.c`, `wrapper_macos.h`, `wrapper_linux.h`, `wrapper_windows.h`.
- `tests/`: only the macOS-capable integration tests — `export_timing`, `held_frame_clock`,
  `hevc10_decode`, `no_camera_stand_in`, `output_geometry_golden`, `remux_seek_index`,
  `render_frames_match_export` — plus `tests/common/mod.rs`. All skip without their media env vars.
- `fixtures/upstream/`: files the unit tests read from outside the crate upstream —
  `docs/shader-effects.md`, `electron/mcp/effects-contract.ts`,
  `src/lib/ai-edition/annotations/arrowBounds.ts`, `public/cursors/default/*.png` (OpenScreen's own
  cursor sprites, same MIT licence).

## Not copied

- The 16 Windows-only and 2 Linux-only integration tests, `tests/common/nv12.rs` (Windows-only).
- `examples/` (three macOS benches; not needed, and `cargo test` would build them).
- `public/fonts` (OFL, 2.3 MB) and `public/mediapipe` (Apache-2.0 model, 940 KB): only used by
  three tests, which are ignored instead (see patch 7). Needed once captions/segmentation ship.

## Patches (each one, with the reason)

1. **`Cargo.toml` rewritten.** Standalone package (no upstream workspace), `version = "2.1.2"`,
   `publish = false`.
   - All runtime deps under `cfg(all(target_os = "macos", target_arch = "aarch64"))` — the crate is
     empty elsewhere, so Linux CI compiles nothing.
   - **wgpu, pollster, cosmic-text, ash, wgpu-hal, windows dropped entirely** (upstream declares
     wgpu/pollster/cosmic-text for every target, used only by `*_linux.rs`). The plan said "move to
     `cfg(target_os = "linux")`"; since this crate's Linux backend is never built in Inspector Rust,
     keeping them would only add wgpu & co. to the Linux CI build and the lockfile.
   - `naga = 24` with `wgsl-in, hlsl-out, msl-out` merged into one line (upstream splits msl-out
     into the macOS table).
   - **`segmentation` feature and its `ort`/`ndarray` deps removed**, `default = []`: `ort =
     2.0.0-rc.13` would unify with Inspector Rust's `ort` rc.12 and change its version (and add
     `load-dynamic` next to `download-binaries`). `segmentation.rs` compiles its non-feature path.
     `[lints.rust] unexpected_cfgs` declares `feature = "segmentation"` and `openscreen_ffmpeg`.
   - `bindgen 0.70` / `cc 1` as build-dependencies only for a macOS **host**.
2. **`build.rs` rewritten to the macOS path** (Windows HLSL, Linux freestanding/libclang/symbol
   prefix code removed — unreachable here).
   - bindgen target from `CARGO_CFG_TARGET_ARCH` instead of hard-coded `aarch64`.
   - ffmpeg tree: `MAC_FFMPEG_DIR`, else `<repo>/third_party/ffmpeg/8.1.2`, else `FFMPEG_DIR`.
   - **No panic without a tree:** prints a `cargo:warning` and builds an empty library, so
     `cargo test --workspace` on a Mac that never ran the ffmpeg script stays green.
   - Emits `cfg(openscreen_ffmpeg)` only after bindgen succeeded.
3. **`src/lib.rs`:** crate-level
   `#![cfg(all(target_os = "macos", target_arch = "aarch64", openscreen_ffmpeg))]`.
4. **Kept integration tests:** their `#![cfg(...)]` replaced by (or, for `remux_seek_index` and
   `render_frames_match_export`, given) the same crate cfg — they would not compile against the
   empty library.
5. **`include_str!` paths** in `src/regions.rs` (1), `src/shader_fx.rs` (2),
   `src/compositor_macos.rs` (1): `../../../…` → `../fixtures/upstream/…`.
6. **Cursor sprite paths** in tests of `src/compositor_macos.rs`, `src/cursor_sdf.rs` (2),
   `src/frame_geometry.rs`: `$CARGO_MANIFEST_DIR/../../public/cursors/default` →
   `$CARGO_MANIFEST_DIR/fixtures/upstream/public/cursors/default`. (`compositor_linux.rs` untouched.)
7. **Three tests `#[ignore]`d** (need un-vendored `public/` assets):
   `segmentation::tests::the_vendored_model_is_where_the_loader_expects_it`,
   `text_fonts::tests::only_the_font_files_of_the_directory_are_registered`,
   `text_macos::tests::every_shipped_family_draws_from_its_own_files`.

8. **Per-clip extra audio track (stage 2, Task 2 — the recorder's `mic.wav`).** RECON §6 showed
   `audioTracks` are placed, never speed-stretched, so a mic under a speed region would drift;
   this is option (c) there.
   - `src/scene.rs`: `SceneClip.extra_audio: Option<ExtraAudio>` (serde `extraAudio`, default
     `None`) and the new `ExtraAudio { path, offset_sec, gain_db }` (`offsetSec`, `gainDb`).
     Convention: file time = source time − `offset_sec`. + parse test.
   - `src/audio.rs`: `clamp_extra_gain_db` (−60…+12, non-finite → 0) and
     `decode_extra_audio_window` — decodes `[start − offset, end − offset]` of the file, pads
     silence before the file start / past its end to exactly the window length, applies the
     gain. Missing/undecodable file → stderr warning + `None`, never an export error.
   - `src/audio_jobs.rs`: `decode_and_stretch_clip_audio_with_extra` sums clip audio (loudness
     gain now applied before the stretch — linear, so equivalent) and the extra track, THEN
     stretches with the clip's speed segments. The original function is untouched. + 6 tests
     (synthetic WAVs decoded by the vendored LGPL ffmpeg, no GPU).
   - `src/pipeline_macos.rs`: `ClipSource.extra_audio`; the export spawns an audio job when
     `has_audio || extra_audio.is_some()` and the concat plan counts such clips as audible.
   - `tests/{export_timing,hevc10_decode,render_frames_match_export}.rs`: `extra_audio: None`
     added to the `ClipSource` literals so they compile.
   - Why not mix after the stretch: the mic has to follow the exact same tempo map as the
     picture; mixing first reuses the one stretch the clip audio already gets.

No other source line was changed. Compiler warnings of the upstream code (unused imports, dead
code, bindgen transmutes) are left as they are.

## Test

```sh
scripts/build-ffmpeg-macos.sh
MAC_FFMPEG_DIR=$PWD/third_party/ffmpeg/8.1.2 CARGO_INCREMENTAL=0 cargo test -p openscreen-compositor --lib
# 2026-11-01: 484 passed, 0 failed, 3 ignored (Metal tests ran on an M1 Pro)
# 2026-11-01 after patch 8: 491 passed, 0 failed, 3 ignored
```

## Updating from upstream

Copy the new `crates/compositor/src` over `src/`, then re-apply patches 3–7 (`grep -rn
'\.\./\.\./\.\./\|\.\./\.\./public' src` finds 5/6) and diff `build.rs`/`Cargo.toml` by hand.
Re-check `scripts/fetch-ffmpeg-macos.mjs` upstream for a new ffmpeg pin and hash.
