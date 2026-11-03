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
  three tests, which are ignored instead (see patch 7). The fonts ship since stage 4 under
  `core/rust-lib/assets/record/fonts` (patch 13); `public/mediapipe` is still not copied.

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
   The two font tests run again since patch 13 (the fonts now ship); only the segmentation
   one stays ignored.

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

9. **Mono extra audio at unity (stage 2, Task 2 fix round 1).** The recorder's `mic.wav` is mono
   48 kHz i16; swresample's default mono→stereo upmix puts it at −3 dB (√½) per side, so the mic
   came out 3 dB below its `gainDb`. Ruling: mono goes to L and R at unity.
   - `src/audio.rs`: `AudioResampler::from_frame` takes `mono_at_unity`; when set and the input
     is mono it calls `swr_set_matrix` with `[1.0, 1.0]` before `swr_init`. The flag is threaded
     through `decode_clip_audio_inner`; upstream's paths (`decode_clip_audio`,
     `decode_audio_window`/loudness) pass `false`, so clip-audio behaviour is unchanged. New
     private `decode_extra_clip_audio` passes `true`; `decode_extra_audio_window` uses it.
   - Side change: a failing `swr_set_matrix`/`swr_init` now frees the `SwrContext` (upstream
     leaked it on `swr_init` failure).
   - `src/audio_jobs.rs`: tests `mono_48k_mic_is_mixed_at_unity` (mono 48 kHz mic + 44.1 kHz
     stereo clip audio → 0.5 peak in both channels ±1 %) and
     `mic_shorter_than_clip_pads_silence` (same length, silent tail).

10. **Build metadata for dependents (stage 2, Task 4 — the `ir-record-render` helper).**
   - `Cargo.toml`: `links = "openscreen_ffmpeg"`, so dependents' build scripts can read
     `DEP_OPENSCREEN_FFMPEG_*`.
   - `build.rs`: after bindgen succeeded (next to `cfg(openscreen_ffmpeg)`) it emits
     `cargo:enabled=1` and `cargo:lib_dir=<ffmpeg lib dir>`.
   - Why: without an ffmpeg tree this crate is an empty library. The helper's `build.rs` panics
     with a clear message when `DEP_OPENSCREEN_FFMPEG_ENABLED` is missing on macOS/arm64,
     instead of failing on dozens of unresolved imports (or shipping a helper that can do
     nothing). `lib_dir` is for bundling (Task 8).
   - Caveat (Task 1 behaviour, unchanged): the "tree found" decision is cached by cargo. A tree
     that appears AFTER an empty build needs `touch third_party/ffmpeg/8.1.2/.ir-ffmpeg-stamp`
     (the script writes that stamp, so a fresh script run does it).

11. **No clippy on upstream code (stage 2, Task 8 — ruling R13).** `src/lib.rs` carries
   `#![cfg_attr(clippy, allow(warnings, clippy::all))]`. Why: `scripts/check.sh` runs
   `cargo clippy --workspace --exclude openscreen-compositor --all-targets -- -D warnings`, but
   `--exclude` only drops the crate as a TARGET — clippy still lints it as a path dependency of
   `ir-record-render` and fails on ~136 upstream findings. The attribute applies only under
   clippy; normal builds keep upstream's compiler warnings visible.

12. **Explicit cursor telemetry path (stage 3, Task 1).** Upstream loads the cursor sidecar from
   `{screenPath}.cursor.json` only; Inspector Rust keeps `cursor.json` in the project directory.
   - `src/scene.rs`: `SceneClip.cursor_path: Option<String>` (`#[serde(default)]`, JSON
     `cursorPath`); new `pub fn cursor_sidecar(screen_path, cursor_path)` = the explicit path when
     given and non-empty, else `{screen_path}.cursor.json` (old behaviour). Test
     `cursor_path_overrides_sidecar_location`.
   - `src/pipeline_{macos,windows,linux}.rs`: `ClipSource.cursor_path: Option<String>`;
     `src/timeline_walk.rs` (export walk) loads and caches cursor tracks by the resolved
     telemetry file, not by video path.
   - `src/live.rs`: `open_and_seek_clip` gained a `cursor_path` argument (prefetch passes the
     next clip's; the explicit-request paths pass `None` because the render loop reloads from the
     scene); `advance_to_next_scene_clip` and the active-clip-request branch resolve through
     `cursor_sidecar` using the scene clip. `LiveView::create` still takes a full telemetry path.
   - `tests/*.rs`: `ClipSource` literals gained `cursor_path: None`. `record-render/src/session.rs`
     `clip_sources` forwards the field.

13. **Free-form blur drawn as its outline (stage 4, Task 3).** Upstream offers the
   `freehand` blur shape but masks only its bounding box (`compositor_macos.rs`, mode 10) — a
   promise the picture does not keep. Inspector Rust draws the closed polygon.
   - Units (our ruling; upstream contradicts itself, RECON-stage4 §1d): `freehandPoints` are
     fractions (0..1) of the ANNOTATION BOX, not of the screen rect.
   - `src/frame_geometry.rs`: `PrivacyMask.content` = the annotation box inside the mask in the
     shader's `n` coordinates (upright, frame space, and tilted — `[bx0,bx1]` in the same plane
     fractions as the warp); `freehand_sdf` / `freehand_sdf_size` / `polygon_signed_distance`:
     the polygon as a signed distance field over the box (256 texels on the longer side,
     square texels, values in units of the box's shorter side, negative inside, even-odd);
     `rect_within`. + tests `privacy_mask_content_is_the_annotation_box`,
     `freehand_sdf_signs_and_scale`.
   - `src/compositor_macos.rs`: `freehand_cache` (R16F field per annotation id, rebuilt when
     the points or texture size change); mode 10 gets `fx.z = 2`, `src` = the box, the field in
     `texture(3)`. Falls back to the bounding box (upstream behaviour) when fewer than three
     finite points or `!mask.oval_ok` (mask widened to the motion-blur trail or clipped by a
     slot — the oval's own rule). + tests `freehand_mask_excludes_outside_polygon` (triangle:
     outside unchanged, inside blurred, ~1-px monotone edge) and
     `freehand_mask_rectangle_unchanged` (behaviour, no GPU hash: rectangle blurs its box and
     nothing outside the pad, oval keeps the box corners and masks its centre; two points =
     the rectangle). The cache drops outlines of annotations no longer in the scene (`set_scene`).
   - `src/shaders.metal`: mode 10 branch `fx.z > 1.5`: distance → px with the box's current
     size, `cov = 1 − smoothstep(0, 1, d_px)` — the oval's ~1 px soft edge, outside the outline.
     Rectangle and oval paths unchanged. Under a tilted screen the box size used for the px
     scale and the field's aspect come from the axis-aligned bounding box of the projected quad
     (`src.zw * quad_px`), so the edge softness and the field's aspect are approximate there;
     inside/outside stays exact (the warp inverse maps `n` exactly).
   - `src/cursor_sdf.rs`: `f16_bits` is `pub(crate)` (reused to encode the field).
   - Fonts (patch 7 partly undone): `src/text_fonts.rs` `repo_fonts_dir()` points at
     `../../core/rust-lib/assets/record/fonts`, where Inspector Rust now ships the ten OFL
     fonts; the two font tests (`only_the_font_files_of_the_directory_are_registered`,
     `every_shipped_family_draws_from_its_own_files`) run again.

14. **The voice is normalised, not the system sound (stage 4, Task 4).** Upstream assumes the
   screen file IS the voice: its audio is loudness-normalised (−16 LUFS, ≤ +12 dB) and an
   extra track is not. For Inspector Rust the voice is the mic (`extraAudio`) and the screen
   audio is the recorded system sound — the export did the opposite of what a demo needs.
   - `src/scene.rs`: `SceneClip.normalise: AudioNormalise` (`screen` | `voice`, serde default
     `screen` = upstream) and `SceneClip.system_gain_db` (JSON `systemGainDb`, default 0). +
     parse assertions in `clip_extra_audio_parses_camel_case_and_defaults_to_none`.
   - `src/audio_jobs.rs`: `ClipAudioLevels { normalise, system_gain_db }` and
     `decode_and_stretch_clip_audio_levelled`. Under `voice` the screen audio gets no loudness
     gain and the extra track gets `voice_loudness_gain_db` on top of its own gain; under
     `screen` it is upstream's behaviour. `system_gain_db` (clamped ±12) applies to the screen
     audio under both. `decode_and_stretch_clip_audio_with_extra` delegates with the defaults.
   - `src/audio.rs`: `voice_loudness_gain_db` — the loudness measured on the unity-upmixed decode
     (patch 9), i.e. as the mono voice is played (otherwise it lands 3 dB above the target); the
     cache key carries the flag. Music and voiceover tracks (`overlay_external_track`) decode
     through the unity path too (a mono file was −3 dB per side), and a voiceover track is
     levelled with `voice_loudness_gain_db`. Two public helpers for the waveform peaks of the
     `ir-record-render` helper: `decode_clip_audio_unity` and `audio_file_duration_sec`
     (container duration through `sn_fmt_duration`).
   - `src/pipeline_macos.rs`: `ClipSource.audio_levels`, passed to the clip audio job.
   - `tests/{export_timing,hevc10_decode,render_frames_match_export}.rs`: `audio_levels:
     Default::default()` in the `ClipSource` literals.
   - Tests (`audio_jobs.rs`): `voice_normalised_screen_untouched` (a −34 dBFS mic is lifted by
     the +12 dB cap and a −6 dBFS system sound kept under `voice`; the reverse under `screen`;
     the system level −6 dB and its −12 dB clamp), `voice_loudness_measures_mono_as_played`
     (3.01 dB apart), `mono_music_track_mixes_at_unity`,
     `audio_file_duration_is_read_from_the_container`.
   - The master `SceneAudio.gainDb` stays 0: Inspector Rust's "system sound level" is the screen
     audio's alone, not the whole mix's.

15. **Annotation images reload when their file changes (stage 4, Task 6 fix).** Upstream's
   `ann_img_cache` (`src/compositor_macos.rs`) reused a texture per annotation id as long as
   the source string had the same LENGTH. Inspector Rust imports images as `<uuid>.<ext>`, so
   every imported path has the same length and a new picture on the same annotation kept the
   old texture in the preview. The entry now keeps the full source string and is reused only
   for an equal string (`String ==` compares the length first, so data-URLs of another size
   stay cheap); `set_scene` also drops entries of annotations no longer in the scene (a
   deleted image kept its texture for the session). macOS backend only (the Windows/Linux
   backends carry the same guard but are not built in Inspector Rust). + test
   `swapped_image_of_equal_path_length_is_reloaded`.

No other source line was changed. Compiler warnings of the upstream code (unused imports, dead
code, bindgen transmutes) are left as they are.

## Test

```sh
scripts/build-ffmpeg-macos.sh
MAC_FFMPEG_DIR=$PWD/third_party/ffmpeg/8.1.2 CARGO_INCREMENTAL=0 cargo test -p openscreen-compositor --lib
# 2026-11-01: 484 passed, 0 failed, 3 ignored (Metal tests ran on an M1 Pro)
# 2026-11-01 after patch 8: 491 passed, 0 failed, 3 ignored
# 2026-11-01 after patch 9: 493 passed, 0 failed, 3 ignored
# 2026-11-02 after patch 12: 494 passed, 0 failed, 3 ignored
# 2026-11-03 after patch 13: 501 passed, 0 failed, 1 ignored
# 2026-11-03 after patch 14: 505 passed, 0 failed, 1 ignored
# 2026-11-03 after patch 15: 506 passed, 0 failed, 1 ignored
```

## Updating from upstream

Copy the new `crates/compositor/src` over `src/`, then re-apply patches 3–15 (`grep -rn
'\.\./\.\./\.\./\|\.\./\.\./public' src` finds 5/6) and diff `build.rs`/`Cargo.toml` by hand.
Re-check `scripts/fetch-ffmpeg-macos.mjs` upstream for a new ffmpeg pin and hash.
