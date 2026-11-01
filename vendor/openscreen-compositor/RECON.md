# OpenScreen compositor — recon for Inspector Rust `record` stage 2

Read from a local, read-only clone of https://github.com/getopenscreen/openscreen at v2.1.2
(commit `bfd5bc32`, MIT), made in a scratch directory of the 2026-11-01 planning session and not
kept. Paths below are relative to that clone's root
(`C/` = `crates/compositor/`, `N/` = `crates/compositor-view-napi/`). Read-only; nothing executed.

## 0. TL;DR

- One crate `openscreen-compositor` (lib `openscreen_compositor`, C/Cargo.toml:1-10), ~60k LOC src,
  three backends selected by `cfg` and re-exported under portable names `d3d`, `compositor`,
  `pipeline`, `text` (C/src/lib.rs:63-146). macOS = Metal (`metal` 0.29 + `objc` 0.2) + ffmpeg
  8.1.2 (VideoToolbox decode/encode) linked as **shared dylibs**.
- Input is NOT the TS project document. It is a separate render model, `scene::Scene`
  (C/src/scene.rs:157-202), JSON built in TS by `buildSceneDescription(document)`
  (src/native/sceneDescription.ts:874) from the zod doc (`axcutSchemaVersion = 9`,
  src/lib/ai-edition/schema/index.ts:49, `documentSchemaShape` :744). Plus a separate
  clip list `ClipSource[]` (C/src/pipeline_macos.rs:746-753) built by `buildNativeClipList`
  (src/native/sceneDescription.ts:718).
- Preview on macOS = **CPU readback RGBA8 → IPC → `<canvas>` (`createImageBitmap`/`putImageData`)**.
  Shared GPU textures exist only on Windows (C/src/shared_frames.rs:12-13, C/src/live.rs:1103-1105).
- Trim = the clip list itself (trim-narrowed source windows); speed = `scene.speedRegions`
  (source time, any positive finite factor); crop = `scene.cropByClip[i]` (normalized rect).
- Export MP4 (h264/hevc VideoToolbox, AAC audio) and GIF (hand-rolled GIF89a) both take
  `(clips, scene, Gpu, Compositor, Cfg, params, progress, ExportControl)` and are cancellable.
- External audio tracks are **placed, not speed-stretched** → our separate `mic.wav` should
  NOT go in as `audioTracks` if speed regions exist (details §6).

## 1. Public API surface (Rust)

### 1a. Device / compositor / scene (macOS)
- `d3d::Gpu { device: metal::Device, context: metal::CommandQueue, backend, feature_level }`
  (C/src/d3d_macos.rs:66-71); `Gpu::create_auto(_debug: bool) -> Result<Gpu>` (:136),
  `Gpu::probe()` (:149, cached `OnceLock` :82). Only `Backend::Hardware` exists on macOS.
- `compositor::Compositor` (C/src/compositor_macos.rs:310) — holds `RefCell`s ⇒ **!Sync/!Send**
  in practice; confine to one thread.
  - `Compositor::new_sized(gpu: &Gpu, w: u32, h: u32) -> Result<Compositor>` (:762) —
    compiles `shaders.metal` (4164 LOC) at runtime via `new_library_with_source` (:783-786),
    plus a second "models" variant (:829). Expect a one-off compile cost per construction.
  - `set_scene(&self, Option<Scene>)` (:985), `set_live_params(&self, LiveParams)` (:972) with
    `frame_geometry::live_params_from_scene(&Scene) -> LiveParams` (C/src/frame_geometry.rs:1856).
  - `unsafe compose_frame(...)` (:2635), `unsafe readback_direct(&self) -> Result<(u32,u32,Vec<u8>)>`
    (:3358-3382: blit RT→shared texture, `sync()`, `get_bytes`, RGBA8 tightly packed).
  - `set_programme_time / set_timeline_time / set_cursor_time` (:1142-1160), `scene_snapshot()` (:1165).
- `scene::Scene::from_json(&str) -> anyhow::Result<Scene>` (C/src/scene.rs:210).

### 1b. Preview frame at time t
Two entry points:
1. **Live view (interactive)** `live::LiveView` (C/src/live.rs:983):
   - `LiveView::create(w, h, screen: &str, webcam: &str, cursor_json: &str) -> Result<LiveView>`
     (:1003-1037) spawns its own render thread (`std::thread::spawn` :1028) that owns Gpu +
     Compositor + decoders and free-runs/holds.
   - Control: `set_rect(w,h)` (:1047), `set_scene(json)` (:1195), `set_time(source_sec)` (:1213,
     **source time of the active clip**, not programme time), `set_playing(bool)` (:1182),
     `set_active_clip(screen, webcam, webcam_offset_sec, clip_index, source_time_sec)` (:1221),
     `set_param_bool/num/str` (:1130-1166), `fatal_error()` (:1055).
   - Output: `latest_frame_since(since_gen: u64) -> Option<LatestFrame>` (:1081) — takes (moves)
     the newest `(gen, w, h, Vec<u8> RGBA, footage, FramePosition)`; `None` if nothing newer.
   - The HOST drives clip switching: the live view only knows one clip at a time; programme→
     (clip, source time) mapping is done in TS. It applies speed regions inside the active clip
     while playing (`speed_at` C/src/live.rs:1928-1934). **No audio** in live (no cpal/AudioUnit in
     crate) — OpenScreen plays audio in the renderer.
2. **Deterministic frames (stills / thumbnails)**:
   `unsafe timeline_walk::render_frames(clips: &[ClipSource], gpu: &Gpu, comp: &Compositor,
   cfg: &Cfg, out_fps: i32, wanted: &[u64]) -> Result<Vec<RenderedFrame>>`
   (C/src/timeline_walk.rs:221-254). Frame indices on the OUTPUT grid (programme time incl.
   speed); pixel-identical to export (test C/tests/render_frames_match_export.rs). `RenderedFrame
   { index, width, height, rgba }` + `to_jpeg(max_width)` (:188-217). Walks the timeline from
   the start each call (with keyframe seeks/catch-up `CATCH_UP_DECODE_SEC=2` :258) — fine for
   thumbnails/scrub stills, not a 30 fps player.
   napi wrapper shows the full setup (N/src/lib.rs:800-841): `Gpu::create_auto` →
   `export_cfg_and_scene(json)` (N:756-770: C8 config with zoom/layout_anim/mblur off) →
   `Compositor::new_sized` → `set_live_params(live_params_from_scene)` → `set_scene` → render.

### 1c. Export with progress + cancel
- MP4: `pipeline::run_composited_multi_cancellable(clips: &[ClipSource], out: &str, gpu: &Gpu,
  comp: &Compositor, cfg: &Cfg, params: &ExportParams, progress: &mut dyn FnMut(u64),
  control: &ExportControl) -> Result<Stats>` (C/src/pipeline_macos.rs:1236-1249). Writes to a
  staged file, renamed only when complete (`with_validated_mp4_output`).
  `ExportParams { width, height, fps: Option<u32>, codec: ExportCodec::{H264,H265},
  bit_rate: Option<i64> }` default 1920×1080/h264 (:831-857). `Stats { frames, wall_s, fps,
  video_duration_s }` (:48-53). Scene is taken from `comp` (`set_scene` before).
- GIF: `gif_export::export_gif_cancellable(clips, out_path: &Path, gpu, comp, cfg,
  params: &GifExportParams, progress, control) -> Result<GifStats>` (C/src/gif_export.rs:193);
  `GifExportParams { width, height, fps, loop_count, dither }` default 854×480@12 (:131-150).
- Progress: callback gets frames encoded (u64), caller computes %. Cancel:
  `export_control::ExportControl(Arc<AtomicU8>)` (C/src/export_control.rs:14) `.cancel()` (:30),
  checked between frames → `Err(ExportCancelled)` (:17), no file left.
- Output size: export sizes the compositor to the encoded size; napi `resolve_output_size` takes
  `scene.output` as the shape (N/src/lib.rs:565ff). Reproduce that, don't composite 1080p then scale.

### 1d. Decode / probe
No dedicated "probe" API (`export_probe.rs` is a profiler). Available:
`pipeline::Decoder::open(path, &Gpu)` / `open_for_export` (C/src/pipeline_macos.rs:147-158),
`seek_to`, `next`, `cur_time_sec`, `fps()`, `available_duration_sec()` (:401-636 range),
`pipeline::probe_frame_count(path)` (:1497), `audio::decode_clip_audio(path, start, end)`
(C/src/audio.rs:657), `audio::loudness_gain_db(path)` (:455), `remux::remux_to_seekable_matroska`
(C/src/remux.rs:111). Stage 1 already knows width/height/fps/duration from project.json, so
probing is mostly unnecessary.

### 1e. The input "document": Scene (render model) — required vs defaulted
Required (no serde default) in `Scene` (C/src/scene.rs:160-201): `clips`, `layout`, `effects`,
`background`, `zoomRegions`, `cursor`, `output`. Defaulted: `annotations`, `speedRegions`,
`cameraFullscreenRegions`, `layoutRegions`, `audio{gainDb}`, `audioTracks`, `clickSound`,
`cropByClip`, `webcamEffect`, `shaderEffects`.
- `layout` required keys: `preset` ("no-webcam" for us), `webcamSize`, `webcamShape`,
  `webcamMirror`, `webcamPosition` (nullable), `webcamReactiveZoom` (:34-47).
- `effects` required: `padding`, `blur` (bool or 0..1), `shadow`, `roundnessFrac`, `motionBlur`
  (:276-297); `frame`, `frameTheme`, `depthOfField` defaulted.
- `background`: tagged `kind`: `color{color}` | `gradient{angleDeg, stops, offsets?, motion?}` |
  `image{path, motion?}` (:385-410).
- `cursor` required: `show`, `size`, `smoothing`, `motionBlur`, `clickBounce`, `clipToBounds`,
  `theme` (:709-744).
- `output { width, height, fps: Option<f64> }` (scene.rs block at ~:875).
- `SceneClip { screenPath, webcamPath, sourceStartSec, sourceEndSec, webcamOffsetSec, hasAudio }` (:12-22).
A complete minimal scene exists: `crates/fixture/scene-gradient-still.json` (clips [],
no-webcam-capable layout block, effects, gradient bg, cursor.show=false, cropByClip, output
1920×1080@60). Stage 2 should build exactly this shape in Rust from our `project.json.editor`
(serde structs are `Deserialize` only — we either emit JSON or add `Serialize`/construct directly;
fields are `pub`, but `active_clip_index` is `pub(crate)`, so `from_json` is the clean route).
Cursor note: `cfg.cursor = scene.cursor.show` (N/src/lib.rs:766); if on, the walk loads
`format!("{}.cursor.json", clip.screen)` (C/src/timeline_walk.rs:498) and expects an OpenScreen
`{"samples":[...]}` sidecar (C/src/cursor.rs:191-196). Keep `show:false` in stage 2.

## 2. Preview delivery: OpenScreen vs options for Tauri

OpenScreen (macOS): render thread composes → `readback_direct` (RGBA8 at preview size) →
`Shared::latest_frame` (C/src/live.rs:2090-2118) → napi `read_frame(id, sinceGen)` returns
`{gen,width,height,data:Buffer,...}` only if newer (N/src/lib.rs:229-264) → Electron main
`ipcMain.handle` structured-clone (electron/ipc/nativeBridge.ts:404-416) → renderer polls every
rAF, `ImageData` → `createImageBitmap` → `drawImage` (src/native/hooks/useNativeCompositorView.ts
:290-345). Windows only: ring of D3D11 shared textures imported by Chromium
(`sharedTexture.importSharedTexture`, electron/native-bridge/services/compositorViewService.ts:782);
"macOS (IOSurface) et Linux (dmabuf) restent sur le readback" (C/src/shared_frames.rs:12-13).
They measured readback transport at 37–55 % of the renderer main thread at 30 fps (shared_frames.rs:4-6).

Options for a Tauri WKWebView:
- (ii) **Frames over IPC (what OpenScreen does on macOS) — lowest risk, works day 1.**
  `LiveView::latest_frame_since` already gives move-out RGBA + generation. Ship via a Tauri
  command returning `tauri::ipc::Response` (raw bytes, no JSON) or a custom URI scheme
  (`irframe://<view>/<gen>`) fetched as `ArrayBuffer`; draw with `ImageData`+`createImageBitmap`.
  Cost: 1280×720 = 3.7 MB/frame → ~110 MB/s at 30 fps through WKWebView IPC; acceptable for an
  editor preview sized to the panel (render at panel device pixels, `set_rect`). JPEG (the
  `to_jpeg` helper, q85) cuts bytes ~20× but adds ~5–10 ms CPU encode/decode per frame — good for
  scrub stills/thumbnails, worse for playback. Add a pause-idle path (gen check → `null`).
- (i) **Native CAMetalLayer/NSView over/under the WKWebView — best quality/perf, more work.**
  Needs (a) an accessor for the compositor's RT (`rt: metal::Texture`, C/src/compositor_macos.rs:349,
  private, `StorageMode::Private` per :21) or a new "present to drawable" method (blit RT →
  `CAMetalDrawable.texture`), (b) interop metal-rs `Texture` ↔ objc2-metal via raw pointer
  (`as_ptr()` → `Retained::retain`), both on the same `MTLDevice` (`Device::system_default`),
  (c) an NSView sibling positioned in window coords synced with the DOM rect (IR already does
  CAMetalLayer overlays in `retro/macos.rs`). Over-the-webview kills DOM overlays on the preview
  (crop handles must then be drawn natively or in a transparent webview layer above); under it
  requires the WKWebView to be non-opaque with a transparent hole. Live render thread is not on
  the main thread → drawable presentation from that thread is OK for CAMetalLayer, but layout
  changes (frame) must happen on main.
- (iii) **IOSurface**: WKWebView cannot import an IOSurface into canvas/WebGL; usable only as
  `CALayer.contents` in a native layer → same plumbing as (i). The RT is not IOSurface-backed today.
Recommendation: stage 2 = (ii) raw-RGBA with generation pull + `render_frames` JPEGs for the
timeline filmstrip; keep (i) as a later optimisation once the API is stable.

## 3. Build requirements (macOS)

build.rs (C/build.rs, 501 LOC) executes on the host:
- `point_libclang_at_the_xcode_toolchain()` runs `xcrun --find clang` and sets `LIBCLANG_PATH`
  to its `../lib` unless already valid (build.rs:12, :370-395 region).
- ffmpeg tree: `MAC_FFMPEG_DIR` if it has `include/`+`lib/`, else vendored
  `crates/thirdparty/ffmpeg-n8.1.2-macos64-lgpl-shared` (relative to CARGO_MANIFEST_DIR/..),
  else `FFMPEG_DIR` (build.rs:27-40). **Panics without a tree** (:57-68). crates/.cargo/config.toml
  sets a Windows `FFMPEG_DIR`+`LIBCLANG_PATH` globally — irrelevant if we vendor only the crate.
- Links `dylib` avformat, avcodec, avutil, swscale, swresample, avfilter from `<ff>/lib` (:71-77).
- `cc` compiles `shim.c` (23 LOC: 5 accessors for opaque `AVFormatContext` fields + EAGAIN/EOF) (:99-102).
- bindgen 0.70 over `wrapper_macos.h` (avformat/avcodec/hwcontext_videotoolbox/imgutils/opt/
  pixdesc/swresample/swscale/avfilter buffersrc/buffersink) with allowlists `av.*`,`AV.*`,`sws_/swr_`
  (:107-127), clang args `--target=aarch64-apple-darwin` **hard-coded** + `-isysroot $(xcrun
  --show-sdk-path --sdk macosx)` (:144-157). ⇒ **x86_64 (Intel DMG) build would get wrong
  layouts**; must patch to use `CARGO_CFG_TARGET_ARCH`. Env vars read: `MAC_FFMPEG_DIR`,
  `FFMPEG_DIR`, `LIBCLANG_PATH`, `BINDGEN_EXTRA_CLANG_ARGS` (linux), `OPENSCREEN_FFMPEG_SYMBOL_PREFIX`
  (linux symbol renaming), `CARGO_CFG_TARGET_OS`.
- No Metal shader build step on macOS (MSL compiled at runtime). No HLSL (Windows-only `compile_hlsl`).
- ffmpeg: **8.1.2 built from source**, LGPL shared, recipe in scripts/fetch-ffmpeg-macos.mjs:230-270:
  `--enable-shared --disable-static --disable-doc --disable-debug --disable-xlib --disable-libxcb
  --disable-sdl2 --disable-lzma --enable-videotoolbox --enable-audiotoolbox --disable-x86asm
  --arch=arm64 --cc=clang -mmacosx-version-min=13.0`, tarball SHA256 pinned (:47). Homebrew ffmpeg
  is GPL (x264) → dev-only, never ship. Dylibs must be bundled + install_name rewritten to
  `@rpath`/`@executable_path/../Frameworks` (scripts/build-macos-compositor-addon.mjs:251ff checks
  "no absolute build-machine paths"). Also codesign each dylib for notarization.
- Can the Metal/VideoToolbox path build without ffmpeg? **No.** Demux, VT decode (via
  `AV_HWDEVICE_TYPE_VIDEOTOOLBOX`), encode (`h264_videotoolbox` via `avcodec_find_encoder_by_name`),
  mux, audio decode/resample/atempo, GIF all go through libav*. `ffi.rs` is the bindgen output.
- Dependencies (C/Cargo.toml + workspace crates/Cargo.toml):
  - all targets: anyhow 1, serde 1, serde_json 1, image 0.25 (jpeg,png), naga 24 (wgsl-in,
    hlsl-out; macOS adds msl-out), **wgpu 24, pollster 0.4, cosmic-text 0.19 — declared for all
    targets but only used by Linux files** (grep: only compositor_linux/d3d_linux/pipeline_linux/
    linux_frames/text_linux) ⇒ move to `[target.'cfg(target_os="linux")']` when vendoring to drop
    wgpu-core/hal (+ a second `metal 0.31` that wgpu-hal pulls; Cargo.lock has metal 0.29 AND 0.31).
  - macOS: metal 0.29, objc 0.2.7, block 0.1.6, core-foundation 0.9.4.
  - optional `segmentation` feature (**default ON**, C/Cargo.toml:21-27): ort 2.0.0-rc.13
    (`std, ndarray, load-dynamic, api-27`), ndarray 0.16.
  - build: bindgen 0.70, cc 1.
  - Whole lock: 216 packages incl. napi/windows/ash. Realistic macOS weight after trimming:
    bindgen+clang-sys (build), naga, image, metal, objc, core-foundation 0.9 (IR has 0.10 → two
    versions, harmless) ≈ +30–40 crates, plus ~60k LOC to compile (frame_geometry 9.7k,
    compositor_macos 5.5k, regions 3.7k, audio 3.3k, live 2.7k …) and ~25–35 MB of ffmpeg dylibs.
- **ort**: must use `default-features = false` (no segmentation). IR's workspace has
  `ort = "2.0.0-rc.12"` with default features (download-binaries, static); cargo would unify to
  rc.13 with BOTH `load-dynamic` and `download-binaries` — undefined/conflicting linking. Turning
  the feature off is clean: `segmentation.rs` compiles regardless and its entries fail gracefully
  (C/src/lib.rs:52-55).

## 4. Module map (C/src, LOC) — stage 2 need

| module | LOC | role | stage 2? |
|---|---|---|---|
| lib.rs | 153 | cfg re-exports | yes |
| ffi.rs | 80 | bindgen include + helpers | yes |
| config.rs | 59 | `Cfg` presets C0..C8 | yes (use C8 w/ zoom/layout/mblur off) |
| scene.rs | 1399 | render-model JSON | yes |
| d3d_macos.rs | 160 | Metal device/queue | yes |
| compositor_macos.rs | 5472 | Metal engine (layers, bg, crop, cursor, text…) | yes (monolith) |
| shaders.metal | 4164 | MSL uber-shader | yes |
| pipeline_macos.rs | 1506 | Decoder (VT), VideoEncoder, MP4 export | yes |
| mac_frames.rs | 441 | software decode → CVPixelBuffer fallback | yes (linked) |
| timeline_walk.rs | 745 | shared export walk, `render_frames` | yes |
| regions.rs | 3718 | speed segments, zoom/annotation timing | yes (speed) |
| frame_geometry.rs | 9730 | layout/crop/padding/3D geometry, LiveParams | yes (crop/layout) |
| audio.rs | 3285 | decode/mix/atempo/WSOLA/AAC/loudness | yes |
| audio_jobs.rs | 248 | per-clip audio threads | yes |
| export_control.rs | 426 | cancel + staged output | yes |
| gif_export.rs | 1708 | GIF89a, median-cut, LZW, dither | yes |
| live.rs | 2725 | LiveView player thread | yes (preview) |
| export_probe.rs | 144 | profiling | trivial |
| remux.rs | 248 | to seekable MKV | optional |
| text_macos.rs / text_fonts.rs / text_plate.rs / text_anim.rs | 1153/100/94/248 | CoreText text, captions/annotations | later (compiled in) |
| cursor.rs / cursor_sdf.rs / sculpt.rs | 1154/416/355 | cursor track, SDF, 3D sculpt | later |
| camera.rs / prism_mesh.rs | 769/876 | 3D camera/tilt, device frames | later |
| shader_fx.rs | 1394 | agent WGSL effects via naga→MSL | later (but compositor references it) |
| segmentation.rs | 424 | ONNX webcam mask | later/off |
| shared_frames.rs | 365 | Windows texture ring (SlotBook portable) | no (compiled) |
| *_windows / *_linux / linux_decode / cpu_frames_windows / vk_shaders | — | other OS | no (cfg'd out) |

Later-stage features are woven into compositor_macos/frame_geometry; they are not separable
modules — vendor whole, drive features off via scene values (zoomRegions [], cursor.show false,
no webcam, effects 0). Unwanted cost is compile time, not runtime.

## 5. Tests

- Unit tests (`#[test]` in src): ~600, e.g. frame_geometry 128, regions 67, audio 52,
  compositor_macos 34 (`#[cfg(test)]` at C/src/compositor_macos.rs:3464), live 27, gif_export 21,
  scene 17, timeline_walk 16. Pure ones (regions, scene, gif encoder, audio planning) need no GPU;
  compositor_macos tests need a Metal device; several `include_str!` repo-relative files
  (`../../../docs/shader-effects.md` C/src/compositor_macos.rs:5420, shader_fx.rs:1214/1246,
  regions.rs:2380 → `src/lib/ai-edition/annotations/arrowBounds.ts`) ⇒ those tests break when the
  crate is vendored alone — strip or copy fixtures.
- Integration tests C/tests/*.rs: almost all `cfg(windows)`; macOS-only: `export_timing.rs`
  (needs `OPENSCREEN_TEST_MEDIA` dir with ffmpeg-generated mp4s, skips otherwise, :13-27) and
  `hevc10_decode.rs`. `render_frames_match_export.rs` needs `OPENSCREEN_RENDER_SOURCE` video.
  Media are never committed; generation commands are in file headers / crates/fixture/fixture.json.
  All need ffmpeg dylibs present at test runtime (DYLD path) — CI on Linux would need its own tree.

## 6. Export details

- Video codecs (macOS): H264 candidates `h264_videotoolbox` (AV_PIX_FMT_VIDEOTOOLBOX zero-copy),
  `h264_videotoolbox` (NV12), `libopenh264` fallback; H265 `hevc_videotoolbox` (NV12), `libkvazaar`
  (C/src/pipeline_macos.rs:775-816). Software fallbacks only exist if the ffmpeg build includes
  them (the LGPL recipe does not add libopenh264/kvazaar) ⇒ effectively VideoToolbox only; VP9
  rejected (N/src/lib.rs:677-684). Override env `OPENSCREEN_EXPORT_ENCODER`.
- Audio: AAC encoder opened on the MP4 (`AacEncoder::open` ~pipeline_macos.rs:1330), per-clip
  decode+stretch on worker threads (`decode_and_stretch_clip_audio`, C/src/audio_jobs.rs:47-79),
  concat by frame counts (`build_audio_concat_plan`/`assemble_concatenated_pcm`, audio.rs:1743/1768),
  then `finish_audio(mix_click_hits(mix_external_tracks(prog, &audio_tracks), &click), scene.audio)`
  (pipeline_macos.rs ~1437-1445). 48 kHz stereo (audio.rs:19-20). Speed: libavfilter atempo chain,
  WSOLA fallback (audio.rs:1640-1730). Clip audio is loudness-normalised per file
  (`apply_gain_db(loudness_gain_db(screen_path))`, audio_jobs.rs:57-58).
- **All audio streams in the screen file are decoded and mixed** (audio.rs:647-657, issue #108).
- `mix_external_tracks(programme, &[SceneAudioTrack])` (audio.rs:1833): `SceneAudioTrack { path,
  startSec (OUTPUT time), gainDb (-60..+12 clamp), trimStartSec, trimEndSec: Option (REQUIRED in
  practice — None ⇒ track skipped, :1858-1863), fadeIn/OutSec, kind: music|voiceover }`
  (scene.rs:802-830). Voiceover gets loudness normalisation; music beds are ducked under voice.
  Returns early if programme PCM is empty (:1834). **Tracks are placed, never time-stretched**
  (scene.rs:~880 comment on click sounds) ⇒ a mic under a speed region drifts; trims must be
  projected into multiple pieces by the caller.
- For our `mic.wav` (with `mic.offset_ms`): three options, best first:
  (a) pre-mux `mic.wav` as a 2nd audio stream into an edit proxy of `screen.mp4` (stream copy
  video, AAC/PCM audio with `-itsoffset` for offset_ms) → trim/speed/atempo apply automatically
  because decode mixes all streams; loses independent mic gain unless we patch `decode_clip_audio`
  to take per-stream gains (small patch).
  (b) per-clip, per-speed-segment `SceneAudioTrack` pieces (kind voiceover, startSec = output
  time of segment, trim window = source window + offset) — only correct at speed 1.0.
  (c) patch `ClipSource`/`audio_jobs` with an extra audio path + gain (cleanest long term).
- GIF: entirely in `gif_export.rs` (hand-rolled GIF89a, LZW, median-cut 256 palette,
  Floyd–Steinberg, readback per frame), driven by the same `walk_composited_timeline`.

## 7. Trim / speed / crop representation

- **Trim** = clip list. Each `ClipSource`/`SceneClip` is one kept source window
  `[source_start_sec, source_end_sec]` of a file; the programme is their concatenation in order.
  TS derives it from doc `timeline.clips` + `trimRanges` (`resolvePlaybackSegments` →
  `buildNativeClipList`, src/native/sceneDescription.ts:705-745). A cut in the middle of one
  recording ⇒ two ClipSources on the same `screen.mp4`. `scene.clips` must equal the export clip
  list (indices are shared).
- **Speed** = `scene.speedRegions: [{clipIndex?, startSec, endSec, speed}]` in **source seconds of
  that clip** (scene.rs:606-616); any finite speed > 0 (no clamp; 0.25–16 fine, atempo chains
  stages). Mapping: `regions::speed_segments_for_window(regions, src_start, src_end,
  output_start_sec, fps) -> Vec<SpeedSegment>` (regions.rs:61-106) builds contiguous spans
  (1× gaps filled), each with `output_start_sec`, `first_frame`, `frame_count`;
  `SpeedSegment::source_time(i, fps)` (:40-43) and `output_end_sec()` (:34-36) are the
  source↔output conversions; spans < `MIN_SPEED_SEGMENT_SEC` dropped (:305). Regions are filtered
  per clip by `Scene::for_clip_window` (scene.rs:224-235; `clipIndex` preferred, overlap fallback).
  Live preview: `speed_at(&speed_regions, clip_index, source_t)` scales the source clock (live.rs:1928).
  Our timeline UI should reuse the same math (port or call `speed_segments_for_window`).
- **Crop** = `scene.cropByClip: Vec<Option<{x,y,width,height}>>` normalized 0..1 of the source
  frame, index-aligned with `clips` (scene.rs:189-191, 771-776), applied via
  `frame_geometry::normalized_crop` (frame_geometry.rs:212) and read at :3163 for the active clip.
  Aspect presets are the OUTPUT shape: `scene.output.{width,height}`; with `effects.padding=0`,
  `layout.screenCover=true` the cropped screen fills the frame (cover) vs contain (scene.rs:92).

## 8. Risks / conflicts

- **objc 0.2 / metal 0.29 alongside objc2 0.6**: separate crates over the same libobjc runtime —
  coexist fine; no `ClassDecl`/class registration in the compositor (grep), so no class-name
  clashes. Interop of Metal objects needs raw-pointer casts (`ForeignType::as_ptr`). Two
  `MTLDevice`s (ours via objc2 in retro, theirs via `system_default`) are the same singleton.
- **No autorelease pools** on the compositor's own threads (only shader_fx.rs:749-755 uses
  `objc::rc::autoreleasepool`); metal-rs `new_command_buffer` (compositor_macos.rs:2201, 2695,
  3215, 3361) returns autoreleased objects ⇒ unbounded growth on long-lived threads unless every
  frame/export loop is wrapped in `objc::rc::autoreleasepool` (or objc2's). Wrap our calls.
- **Threads**: `LiveView` spawns a render thread per view and joins on Drop (live.rs:1028, 1278);
  audio_jobs spawns up to 4 workers during export. Compositor is `!Sync` (RefCell) — keep all
  calls on one owning thread; Tauri commands must hop via a channel (like OpenScreen's registry
  `Mutex<HashMap<i32, LiveView>>`, N/src/lib.rs:40). Exports block for minutes → `spawn_blocking`
  + IR's busy flags. OpenScreen pauses live previews during export (`PreviewPause`, N:510-535) to
  free the GPU — replicate.
- **Panics**: render-thread `Err` is captured into `fatal` (live.rs:1029-1035) but a *panic* just
  kills the thread silently (no catch_unwind) → preview freezes with no `fatal_error`. Several
  `panic!`/`expect` in non-test code paths (e.g. text_macos.rs:934 when an embedded font is
  missing). Wrap entry points in `catch_unwind` and map to an error. Logs go to `eprintln!`
  (invisible in a bundled app) — redirect or patch to `tracing`.
- **Unsafe surface**: `render_frames`, `compose_frame`, `readback_direct`, Decoder ops are
  `unsafe fn`; keep them inside one wrapper module.
- **Global/env state**: `PROBE` OnceLock (d3d_macos.rs:82); env knobs `OPENSCREEN_FONTS_DIR`
  (text_fonts.rs:20 — set it or text uses system fonts), `ORT_DYLIB_PATH`, `OPENSCREEN_EXPORT_ENCODER`,
  `OPENSCREEN_EXPORT_PROFILE`, `OPENSCREEN_CLIPSWITCH_TIMING`. Harmless.
- **Build**: aarch64 hard-coded bindgen target (build.rs:145) breaks IR's x86_64 macOS DMG —
  gate the compositor dependency to `cfg(all(target_os="macos", target_arch="aarch64"))` (IR
  already does this for ort) or patch build.rs. Linux CI of IR would try to build it unless the
  dependency is macOS-target-only (the Linux backend wants a Vulkan ffmpeg tree). bindgen needs
  libclang from Xcode CLT on build machines. ffmpeg build (~5 min) must be cached in CI; dylib
  bundling + `install_name_tool` + codesign in `install-macos.sh` / release workflow.
- **Cargo**: ort unification (§3) → `default-features=false`. naga pinned at 24 ("not above 24",
  C/Cargo.toml:41-42) — fine if IR doesn't use naga. core-foundation 0.9 vs IR 0.10 coexist.
  `[profile.release] lto=true` in their workspace is not inherited when vendored as a member.
- **Runtime MSL compile** of a 4k-line shader on every `Compositor::new_sized` (and a 2nd variant):
  cache one Compositor per size; first preview has a visible compile delay.
- **License**: OpenScreen code MIT (LICENSE: Siddharth Vaddem + contributors); parts of zoom/
  smoothing/cursor code from Recordly under MIT (THIRD-PARTY-NOTICES.md:168-174) — keep both
  notices. ffmpeg LGPL-2.1 shared: ship dylibs dynamically linked, offer source/relink notice,
  never `--enable-gpl`. Fonts (Inter, Lora, Oswald, Caveat, IBM Plex Mono) OFL-1.1 live in
  `public/fonts` (not in the crate) — only needed for captions/annotations stages. Kenney UI
  audio click sounds (CC0, :253) and MediaPipe model (Apache-2.0, :88) only for later stages.
  shaders.metal is OpenScreen's own (MIT).
- **Semantics drift**: the Scene model is versioned implicitly by serde defaults, not a version
  field; if we vendor and later pull upstream, our emitted JSON must keep up with new required
  fields. Pin the vendored commit and pin a golden scene JSON in IR tests.
