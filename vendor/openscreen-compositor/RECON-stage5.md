# Stage 5 recon — webcam: capture, layouts, PiP styling, background removal (OpenScreen v2.1.2 → Inspector Rust)

Read-only. Nothing in the clone was executed (grep/sed only). Path prefixes:
- `V/` = `/Users/martin/claude/inspector-rust/vendor/openscreen-compositor/` (vendored compositor + patches 1–15)
- `C/` = `<scratchpad>/openscreen-src/new/` (untrusted upstream clone, TS/Electron + `crates/`)
- `IR/` = `/Users/martin/claude/inspector-rust/`

Cargo behaviour in §3 is reasoned from cargo's rules, not run.

## 0. TL;DR

- **Naming trap:** `V/src/camera.rs` is the **3D orbit camera** for the `orbit` zoom preset
  (`V/src/camera.rs:1-24`), not the webcam. Webcam geometry lives in `V/src/frame_geometry.rs`,
  `V/src/scene.rs` (`SceneLayout`, `SceneWebcamEffect`), the draw in `V/src/compositor_macos.rs`.
- **The compositor does NOT lay the webcam out.** For PiP / side-by-side / top-bottom / camera-only
  the native consumes rects the APP resolved with `computeCompositeLayout` (TS):
  `layout.webcamRect`, `screenRect`, `webcamRadiusFrac`, `screenRadiusFrac`, `screenCover`,
  `layoutByClip[]` (`V/src/scene.rs:105-170`). Its own `preset_placements` is a fixture fallback
  for payloads without `webcamRect` (`V/src/frame_geometry.rs:1547-1601`; doesn't even know
  `camera-only`). → IR must **port the PiP + block branches of `C/src/lib/compositeLayout.ts:447-646`**
  into `IR/core/rust-lib/src/record/layout.rs` (today it ports only the no-webcam branch,
  `layout.rs:4`). Only the *per-frame* parts are native: reactive shrink, layout-region morphs,
  Full-Camera lerp, mirror, mask, shadow.
- **Upstream records the macOS webcam in the renderer** (`getUserMedia` + `MediaRecorder`,
  WebM VP8/VP9/H.264, 30 fps, ideal 2160p) — not natively (`C/src/hooks/webcamRecorderOptions.ts:1-24`,
  `C/src/hooks/webcamCaptureTarget.ts:12,46,70-80`; the Swift SCK helper never reads its `webcam`
  config). Nothing to port; IR builds a native `AVCaptureSession` → `AVAssetWriter` (`camera.mp4`, H.264).
  Only the *contract* matters: one file per take + `webcamOffsetSec` (webcam time = screen time − offset).
- **Export truncates a clip to the camera's length** (`V/src/timeline_walk.rs:418-447`) and refuses
  a clip whose camera ends before it starts (`:422-427`). A camera that drops out mid-take would
  shorten/kill the export while the preview holds the last frame. → IR must make the camera file
  cover the screen duration (end the writer session at the screen's end) or vendor-patch this.
- **Segmentation:** `V/src/segmentation.rs` compiles to stubs (feature removed, patch 1). The model
  (`selfie_segmentation_landscape.onnx`, 457 447 B, Apache-2.0, derived from MediaPipe tflite) is in
  `C/public/mediapipe/selfie_segmentation/` only. **Recommendation: port segmentation onto IR's
  `ort` 2.0.0-rc.12 (static, `download-binaries`) inside `ir-record-render`** — every ort call in
  `segmentation.rs` exists with the same shape in rc.12 (verified in the registry source), and two
  ort versions cannot coexist in this workspace (same `links = "onnxruntime"`, semver-compatible
  pre-releases). Small vendor patch (Cargo dep → `ort = { workspace = true }`, `runtime_available()`
  → true when statically linked, model path via scene `webcamEffect.modelPath`).
- **Camera permission is completely absent today:** no `NSCameraUsageDescription` in
  `IR/scripts/install-macos.sh:425-445` / `IR/scripts/pack-macos-release-dmg.sh:70-80`, no
  `com.apple.security.device.camera` in `IR/macos/src-tauri/entitlements.plist` (only
  `device.audio-input`, `:52`). Without the usage string TCC **kills the process** on first camera access.

## 1. Upstream webcam recording (C)

### 1a. Capture
- macOS/Linux: renderer `navigator.mediaDevices.getUserMedia({audio:false, video: webcamVideoConstraints(id, quality)})`
  (`C/src/hooks/useScreenRecorder.ts:505-508`) → `MediaRecorder` with `webcamRecorderOptions`.
  The native macOS helper (Swift, `C/electron/native/screencapturekit/…/ScreenCaptureRecorder.swift:74`)
  declares `let webcam: Webcam` but no `webcam.` member is ever read (grep empty) — the camera is not
  native on macOS. (Windows has a native Media Foundation path, `webcamCaptureTarget.ts` comments.)
- Constraints (`C/src/hooks/webcamCaptureTarget.ts:70-80`): `deviceId: {exact}` when chosen,
  `width/height: {ideal}` from the quality preset, `frameRate: {ideal: 30, max: 30}`
  (`WEBCAM_TARGET_FRAME_RATE = 30`, `:12`). Quality presets `1080p 1920×1080 | 1440p 2560×1440 |
  2160p 3840×2160` (`:14,42-46`), **default `2160p`** (`:48`) — "ideal, never exact" so a 720p
  camera still records.
- Codec (`C/src/hooks/webcamRecorderOptions.ts:4-24`): container `video/webm`; on darwin prefer
  VP8 when the delivered frame ≤ 1920×1080 (`MAX_VP8_PIXELS`), else H.264 (Chromium's macOS H.264
  camera encoder "can stop delivering frames"). Bitrate ladder `webcamBitrateFor`
  (`webcamCaptureTarget.ts:87-97`): ≥2160p 40 Mb/s, ≥1440p 24, ≥1080p 16, ≥720p 8, else 4.
- File name `<prefix><id>-webcam<ext>` (`useScreenRecorder.ts:60,623`). WebM duration patched with
  `fixWebmDuration` (`:229,637`).

### 1b. Time alignment
- The webcam MediaRecorder starts BEFORE the native screen recording; `webcamOffsetMsFrom` =
  `-round(nativeStartedAtMs − webcamStartedAtMs)` → **negative, whole ms** (int in schema)
  (`useScreenRecorder.ts:160-193`). The WebM keeps the extra lead (`:797-807`, duration =
  screen duration + head start).
- Document: `asset.cameraTrack = { sourcePath, startMs=0, offsetMs:int=0, visible=true,
  width?, height? }` (`C/src/lib/ai-edition/schema/index.ts:142-162`), on the ASSET since v4 (P4,
  per-asset camera; `:782-811` migration from v3's project-level field). Schema v9 is the
  `layoutRegions` bump (`:47-48`); `cameraTrack.width/height` were additive without a bump (`:76,148-160`).
- Scene: `assetCameraSource(asset)` → `{path, offsetSec: (startMs + offsetMs)/1000}`, `path:""`
  when no camera or `visible:false` (`C/src/lib/ai-edition/timeline/camera.ts:27-56`). That is
  `SceneClip.webcamPath` / `webcamOffsetSec` (`C/src/native/sceneDescription.ts:725-739,1052-1066`).
- Compositor convention: **webcam source time = screen source time − `webcam_offset_sec`**
  (`V/src/scene.rs:26-27`; `V/src/live.rs:56-58` `webcam_seek_time` clamps at 0;
  `V/src/timeline_walk.rs:481-483,535`).

### 1c. Pause/resume
- macOS native path: on pause the screen helper pauses, then `MediaRecorder.pause()`; resume the
  reverse (`useScreenRecorder.ts:2187-2222`). MediaRecorder drops paused time, so the WebM stays
  aligned with the screen file (whose paused time is also removed). No per-pause offset bookkeeping.

### 1d. Device selection + persistence
- Prefs in the HUD prefs file: `camEnabled`, `camDeviceId`, `camDeviceName`, `camQuality`
  (`C/electron/ipc/handlers.ts:691-708`; restored `useScreenRecorder.ts:295-313`). Identity is read
  off the live track (`webcamDeviceIdentityFrom`, `:453-461`); a saved device that vanished is
  "recoverable" (`isRecoverableWebcamConstraintError`, `:173-182`).
- `canTurnCameraOn` gates enabling (`:485`, `C/src/lib/cameraAvailability.ts`).

### 1e. Unavailable / denied
- At acquire: permission/device errors → camera switched off, persisted off, toast
  `recording.cameraBlocked` / `recording.cameraNotFound` (`useScreenRecorder.ts:530-552`).
- Mid-session: `track.onended` → stream cleared, camera state off, toast `recording.cameraDisconnected`
  (`:519-526`). The recorder blob already captured is still finalized; an empty blob → error
  "the webcam produced no data" (`:226-227`) and the asset is saved without camera.
- Camera mirror is **render-time only** (`webcamMirrored` setting → `layout.webcamMirror`); the file
  is never mirrored.

## 2. Compositor camera rendering (V)

### 2a. Scene JSON fields (serde camelCase)
`SceneClip` (`V/src/scene.rs:18-49`): `webcamPath: String` (required; `""` = no camera),
`webcamOffsetSec: f64` (required).

`SceneLayout` (`V/src/scene.rs:80-173`):
| field | type | meaning |
|---|---|---|
| `preset` | String (req.) | `"picture-in-picture" \| "dual-frame" \| "vertical-stack" \| "no-webcam"` + `"camera-only"` (TS `C/src/lib/layoutTypes.ts:5-11`) |
| `webcamSize` | f32 (req.) | `webcamSizeToFraction(pct)` (0.15–0.60); ignored for size when `webcamRect` present (`V/src/frame_geometry.rs:3305-3313`) |
| `webcamShape` | String (req.) | RESOLVED shape `rectangle\|circle\|square\|rounded`; `webcam_shape_code` 0/1/2/3, default 3 (`frame_geometry.rs:1842-1849`) |
| `webcamMirror` | bool (req.) | swaps u0/u1 (`compositor_macos.rs:3139`) |
| `webcamPosition` | `{cx,cy}`\|null (req.) | informational only now |
| `webcamReactiveZoom` | bool (req.) | shrink during zoom |
| `webcamAnchor` | String? | `top-left, top, top-right, left, right, bottom-left, bottom, bottom-right`; fractions 0/0.5/1 (`scene.rs:177-186`) |
| `webcamCrop` | SceneCrop? | user framing, fractions of the camera frame |
| `webcamRect`, `screenRect` | SceneRect? | app-resolved boxes, fractions of the output frame |
| `screenRadiusFrac`, `webcamRadiusFrac` | f32? | radius / short side of own box |
| `screenCover`, `screenFollow` | bool | block layouts set cover |
| `layoutByClip` | `Vec<Option<ResolvedClipLayout>>` | per-clip `{screenRect, webcamRect?, screenRadiusFrac?, webcamRadiusFrac?, webcamShape?, screenCover}` (`:194-208`), copied into the scalars by `for_clip_window` (`:1065-1081`) |

`Scene.layoutRegions: Vec<SceneLayoutRegion>` (`scene.rs:212-232,991`): `clipIndex, startSec, endSec
(source s), preset, layout: ResolvedClipLayout, webcamReactiveZoom, fromLayout?, fromPreset?,
fromWebcamReactiveZoom?, keepScreenClipFromSec?, transitionStartSec, transitionSec`. `sample()` does a
smoothstep morph of both rects; a missing camera collapses to its centre; shape forced to
`rectangle` mid-morph (`:246-294`). `LAYOUT_TRANSITION_SECONDS = 0.2` (`C/src/lib/layoutTypes.ts:2`).
`Scene.cameraFullscreenRegions` (`scene.rs:667-690`) = legacy "Full Camera" envelopes.

`Scene.webcamEffect: Option<SceneWebcamEffect>` (`scene.rs:933-968,1005-1007`): `mode`
(`none|transparent|blur|custom` → shader code 0/1/2/3), `blurIntensity` 0..1, `background:
SceneBackground?` (custom), **`modelPath: String?`** — absent ⇒ no segmentation, effect off.
Upstream TS never sends `modelPath`; Electron injects it (`C/electron/native-bridge/services/compositorViewService.ts:152,202-208`).

### 2b. Layout presets (TS, `C/src/lib/compositeLayout.ts`)
- PiP (`:447-646`): `margin = round(min(W,H)·0.02)` (`MARGIN_FRACTION`, `:217`); `ref = min(W,H)`;
  `max = ref · webcamSizePreset/100`; `scale = min(max/camW, max/camH)`; circle/square → side =
  min(w,h); position `margin + f·(W − 2·margin − w)` with anchor fractions; radius =
  `round(clamp01(roundness)·min(w,h)/2)`. Screen = `centerScreen` (padding/frame), unaffected by the camera.
  Shadow (web) `rgba(0,0,0,.35)` blur 24 offY 10 (`:251-256`); native fixed `WEBCAM_SHADOW_OPACITY 0.35`,
  spread 32/1080, offset 12/1080 of the frame's short side (`V/src/frame_geometry.rs:452-459`), not
  driven by the Shadow slider; drawn only in PiP (`webcam_shadow` weight, `:3760-3761`) and dropped
  in cutout mode (`compositor_macos.rs:3157-3168`).
- Block (`dual-frame` row, `vertical-stack` column, `:497-586`): screen + camera welded, gap 0.02 of
  screen width (`BLOCK_GAP_FRACTION`, `:227`), camera shares the screen's cross edge, its free side
  chosen to make the block match the padded area's aspect, clamped to `[1/1.25, 1.25]` of square
  (`BLOCK_CAMERA_ASPECT_TOLERANCE`, `:237`); one contain-fit; radius rule dual `{max 24, min 12,
  frac .06}`, vertical `{max 24, min 8, frac .06}` (`:258-283`) applied to BOTH screen and camera;
  `screenCover: true`; mask shape forced `rectangle`. Camera **source aspect plays no part** (cover-crop).
  Under a device frame the camera lines up with the frame's body (`framedScreen`, `:331-345`).
  No camera on the clip → degenerates to screen-only (`:499-506`).
- `camera-only` (`:466-479`): webcamRect = whole canvas, radius 0, rectangle; screen still centred
  under it (drawn, then covered). `no-webcam` → `webcamRect: null`.
- `isWebcamBlockLayout` (`:115-117`): size slider, shape picker, reactive shrink are inert there.
  `supportsWebcamReactiveZoom` = PiP only (`:126-130`). `resolveWebcamLayoutPreset(preset, hasCamera)`
  → `no-webcam` when the clip has no camera (`:151-156`) — resolved PER CLIP in the scene
  (`sceneDescription.ts:1268-1309`).
- **Auto aspect depends on the preset**: `restingCompositionAspect` adds a square camera for block
  layouts (`:362-382`); `autoFormatAspect(screen, preset, frame, padding)` (`:432-445`). IR's
  `layout::auto_aspect(screen_aspect, padding, frame)` (`IR/.../record/layout.rs:167`) has no preset.
- Zoom limit per clip uses the (smaller) block screenRect (`sceneDescription.ts:1443-1449`).
- Defaults (`C/src/lib/projectDefaults.ts:253-262`): preset PiP, mask `square`, roundness
  `DEFAULT_WEBCAM_ROUNDNESS = {square: 0.7, rectangle: 0.4}` (`:144`), mirrored false, reactive
  **true**, size 40 % (range `WEBCAM_SIZE_MIN 15`–`MAX 60`, `:154-155`), anchor `bottom-right`,
  background mode `none`, wallpaper `/wallpapers/wallpaper11.jpg`, blur 0.5. Ranges `:303-305`.
  Legacy read: `circle`→square+1, `rounded`→rectangle+0.6, old rect/square → 0.3 (`:163-187`).

### 2c. Per-frame native parts (`V/src/frame_geometry.rs`)
- Reactive shrink (`:3292-3316`): `scale = (1 − 0.3·strength·layout_weight).clamp(0.7, 1.0)` when
  `layout_weight>0 && full_camera_progress<=0 && zoom>1`, else 1 — i.e. **70 % during a zoom**,
  following the zoom's ease-in/out envelope; skipped under Full Camera. With an app `webcamRect`,
  the size slider is already baked (`base_size_scale = 1`), only the reactive factor applies, and the
  box shrinks **toward its anchor** (`scale_anchored`, `:3355-3364,3601-3609`).
- Full Camera lerps the box to `[0,0,1,1]` and dissolves the radius (`:3617-3631,3740-3757`).
- Camera radius = `shape_fade · min side · webcamRadiusFrac` (fallback table 0.5/0.3/0.12 without the frac).
- Source rect: user crop first, then centred cover to the box aspect (`webcam_source_rect`, `:336-349`).
- `webcam_is_real(path, screen)` = non-empty and ≠ screen path (`:1805-1817`).

### 2d. Decoding, preview vs export
- Same decoder type as the screen (`Decoder`, VideoToolbox hwaccel, software fallback,
  `V/src/pipeline_macos.rs:147-340`). Compose requires a CVPixelBuffer frame (`compositor_macos.rs:1191-1240,2724`).
  IR ffmpeg is LGPL with `--enable-videotoolbox` (`IR/scripts/build-ffmpeg-macos.sh:39`); an H.264
  MP4 from AVAssetWriter is the safe input (VP8/WebM would go through the software path — avoid).
- Preview: `live::Player` decodes screen + webcam in lock-step, each by its own pts; webcam target
  = `screen_time − offset` (`V/src/live.rs:556-630`); the camera-less case uses a never-decoded
  stand-in (`open_webcam_or_stand_in`, `:95-156`; test `V/tests/no_camera_stand_in.rs`). An
  unreadable declared camera → warning, no PiP drawn (`:134-140`).
- Export: per clip `webcam_decs` keyed by path, seek to `(start − offset).max(0)` (`timeline_walk.rs:391-493`),
  advance per output frame through speed segments with the offset (`:529-540`). **Same `compose_frame`
  for preview and export** (`compositor_macos.rs:2716`), so the camera looks identical.
- Clips/trims/speed/takes: everything is per `SceneClip`; regions are filtered per clip by
  `clipIndex` (`scene.rs:1029-1083`). A take from another project is simply another clip with its
  own `webcamPath`/`webcamOffsetSec` — no new mechanism, but IR must emit them per take
  (`IR/.../record/scene_json.rs:250-287` hard-codes `"webcamPath": ""`, `"webcamOffsetSec": 0.0`).
- Helper already forwards `webcam_path`/`webcam_offset_sec` (`IR/record-render/src/session.rs:349,422-426,714-717`)
  and the live params `webcamSize/Mirror/Shape` (`:593-624,1165-1185`).
- ⚠ Length coupling (`timeline_walk.rs:418-447`): `source_end = min(clip end, screen end, camera end + offset)`;
  camera ending before the clip start → hard export error (`:422-427`). The preview instead holds
  the last frame. `cover_container_duration` takes `max(stream, container)` (`:83-96`), so a camera
  MP4 whose container/track lasts as long as the screen avoids the truncation.

## 3. Segmentation (V + C)

- **What it does** (`V/src/segmentation.rs`): `Segmenter::load(model_path)` → ort `Session`
  (2 intra-op threads, 1 inter-op, CPU EP; `:36,73-101`); `run(rgb)` takes a 256×144 RGB8 frame
  (`MODEL_WIDTH/HEIGHT`, `:32-33`), NHWC 0..1, input `input_1`, output `segment_back`, returns an R8
  mask 0 = background (`:112-149`). `SegmentationWorker` (latest-frame mailbox, drops stale frames)
  and `RateLimiter` (`:320-425`). Without the feature `load`/`run` bail, `runtime_available()` = false (`:55-58,103-106`).
- **Model:** `C/public/mediapipe/selfie_segmentation/selfie_segmentation_landscape.onnx`,
  **457 447 B, sha256 `daa5d389…9e94`**, derived from `selfie_segmentation_landscape.tflite`
  (249 792 B) by `C/scripts/convert-selfie-segmentation-to-onnx.py` (non-mechanical repair of
  HardSwish/TransposeConvBias ops); **Apache-2.0** (README there; `C/THIRD-PARTY-NOTICES.md:90-100`).
  Not in V (patch 7; `V/VENDORED.md:57-59`). The test expects it at `../../public/mediapipe/…`
  (`segmentation.rs:160-176`, ignored). Dimensions must be multiples of 16; quality degrades below 192×112.
- **Compositor integration** (`V/src/compositor_macos.rs`): `pump_segmentation` runs before each
  compose when the scene's effect code > 0 AND a `modelPath` is set (lazy load; failure logs once
  and disables, `:2399-2493`). Capture = the FULL visible camera frame scaled to 256×144 on the GPU
  (`capture_webcam_rgb`), never the cropped part. Preview: worker at **`SEGMENTATION_HZ = 30`**
  (`:271`), mask one frame late. Export: `set_segmentation_deterministic(true)` (`:2539-2559`,
  called by `timeline_walk.rs:357`, reset `:594`) → synchronous inference per frame (~3 ms).
  Public `set_webcam_mask(data,w,h)` exists (`:2339-2370`).
- **Shader** (`V/src/shaders.metal:4023-4043`): `fx.z` = mode, mask sampled at `uv / fx.xy` (valid
  texture extent): 1 cutout → alpha = person (transparent over the background); 2 blur →
  `mix(blur_webcam_bg(…21 Vogel taps…), rgb, person)` (`:443-495`); 3 custom → the compositor draws
  the background in the bubble first (`draw_webcam_bg`, colour/gradient/image) and sends mode 1
  (`compositor_macos.rs:3170-3180`). Without a mask the camera draws unchanged (`:3143-3151`).
- **Cost (upstream doc, M1, 1280×720 preview, "shape not quotable")**: off 3.04 ms, cutout 2.57 ms,
  blur 3.47 ms per frame (`C/technical-documentation/engineering/webcam-segmentation.md:300-311`);
  inference ~3 ms CPU EP; CoreML/DirectML not used (CPU EP is flat in resolution).
- **ort versions:** upstream `ort 2.0.0-rc.13`, `default-features=false`, `std, ndarray, load-dynamic,
  api-27` (ORT 1.27.1 dylib, 38.5 MB on darwin-arm64, staged via `ORT_DYLIB_PATH`)
  (`C/crates/Cargo.toml:50-56`; `C/scripts/fetch-onnxruntime.mjs:54`; doc `:110-123`). IR:
  `ort = { version = "2.0.0-rc.12", features = ["ndarray"] }` with defaults (`std, ndarray, tracing,
  download-binaries, tls-native, copy-dylibs, api-24`) (`IR/Cargo.toml:73-74`; registry
  `ort-2.0.0-rc.12/Cargo.toml [features]`), only off Intel-mac (`IR/core/rust-lib/Cargo.toml:106-111`).
  `ort-sys` declares `links = "onnxruntime"` (rc.12 `Cargo.toml:19`).
- **Can rc.12 and rc.13 coexist?** No. Caret on `2.0.0-rc.12` accepts rc.13, so cargo unifies on
  rc.13 (what patch 1 avoided); pinning `=2.0.0-rc.12` makes the requirements unsatisfiable together
  (one version per semver-compatible bucket), and even two buckets would collide on `links =
  "onnxruntime"` (cargo's resolver allows one package per `links` value in the graph — the workspace
  shares one resolve/lockfile, so a separate binary doesn't help). Also unified features would add
  `load-dynamic` (`ort-sys/disable-linking`) to IR's static build and break `cutout_ml`.
- **API check rc.12 vs what `segmentation.rs` calls** (rc.13 source not available locally):
  `Session::builder()?`, `SessionBuilder::with_intra_threads/with_inter_threads(mut self) -> BuilderResult`
  (`ort-2.0.0-rc.12/src/session/builder/impl_options.rs:52,66`, `BuilderResult = Result<SessionBuilder,
  Error<SessionBuilder>>` `mod.rs:51` — matches the "error generic over the builder" comment),
  `commit_from_file(&mut self, path)` (`impl_commit.rs:45`), `TensorRef::from_array_view((shape, &[f32]))`
  (`value/impl_tensor/create.rs:214,526`), `session.run(ort::inputs![…])` (`session/mod.rs:212`),
  `try_extract_tensor::<f32>() -> (&Shape, &[T])` (`extract.rs:155`). `IR/core/rust-lib/src/cutout_ml.rs`
  uses the same calls (`:50-55,103,109`). → expect `segmentation.rs` to compile against rc.12
  unchanged (to be confirmed by building; `[i64;4]: ToShape` not checked).
- **Options:**
  - (a) bump IR to rc.13: touches `cutout_ml`, lockfile churn, rc.13 source/ORT version unseen,
    still must drop `load-dynamic`. No gain.
  - **(b) port segmentation onto IR's ort in the helper (recommended).** Vendor patch: compositor
    Cargo `ort = { workspace = true, optional = true }` + `segmentation = ["dep:ort"]` under the
    macOS-arm64 cfg (no `ndarray`, segmentation.rs does not use it); `runtime_available()` → true
    when not `load-dynamic`; fix the test's model path. `record-render` enables the feature. One
    ORT, statically linked, no dylib staging/`ORT_DYLIB_PATH`; IR already accepts the size cost in
    the app binary. Costs: helper binary grows by the static ORT (not measured — measure in the
    commit), ~3 ms/frame export inference.
  - (c) elsewhere: the mask is needed per decoded camera frame inside the helper; computing it in the
    app would mean shipping every frame over IPC — no. A variant: Apple Vision
    `VNGeneratePersonSegmentationRequest` in the helper (no model, no ORT) via a patched `Segmenter`
    backend — diverges from the spec ("MediaPipe via ONNX") and from upstream's look; keep as fallback.

## 4. Upstream editor UI for the camera (C)

- One pane "Layout (webcam)" in `C/src/components/ai-edition/RightPanes.tsx:2672-3175`: preset
  picker `LayoutPicker` (icons, `C/src/components/ai-edition/v4/LayoutPicker.tsx:6`), "whole video"
  vs "from…until" when layout regions exist (`:2876-2899`); mirror toggle (`:2908-2916`); "Shrink on
  zoom" only when PiP (`:2920-2930`); mask shape `square|rectangle` + roundness 0–100 % (default per
  shape, reset when untouched, `:2934-2990`); size 15–60 %, step 1 (`:2992-3008`); position = 8-anchor
  grid with icons (`:3011-3025`); background mode chips none/transparent/blur/custom (gated by
  `useCanSegmentCamera`), blur 0–100 %, custom wallpaper picker (`:3034-3100`); framing
  `WebcamFraming` = zoom + pan square crop dragged on a camera still (`:2855-2870,3178`;
  `webcamCropRegion {x,y,width,height}` + `webcamCropPan {x,y}`). Controls disabled + "no webcam"
  help when no clip has a camera (`:2791-2844`). Live slider pushes `setNativeParam("webcamSize",
  pct/16.7)` (`NATIVE_WEBCAM_BASE_PCT`, `:2677,3003-3006`).
- Settings live in `legacyEditor` (passthrough blob, `schema/index.ts:701-715`); the layout partition
  in `document.layoutRegions` (`:717-746,777-778`).
- Shortcut: `addCameraFullscreen` = **C**, label "Change layout here" (`C/src/lib/shortcuts.ts:6,116,132`)
  — splits a layout region at the playhead. IR's **C = crop** (`IR/core/frontend/src/lib/editor-shortcuts.ts:52`).
- Presets: upstream style presets carry `webcamLayoutPreset, webcamMaskShape, webcamRoundness,
  webcamMirrored, webcamReactiveZoom, webcamSizePreset, webcamBackgroundMode, webcamWallpaper,
  webcamBlurIntensity` — but NOT anchor, framing/crop (`C/src/lib/ai-edition/stylePresets.ts:9,65-73,326-330`).

## 5. IR current state

- **Project:** `project.json` schema 1, `RecordingMeta{source, width, height, fps, duration_ms,
  system_audio, mic: Option<MicMeta{offset_ms, sample_rate}>, cursor_mode, pauses}`
  (`IR/core/rust-lib/src/record/project.rs:55-76`); files `screen.mp4`, `mic.wav`, `cursor.json`
  (`:8-11`). Camera → add `CAMERA_FILE` and `camera: Option<CameraMeta>` (serde default, no schema bump).
  `TakeMeta` (`editor.rs:808-817`) needs `has_camera/camera_offset_ms/camera size`; `import_take_into`
  (`session.rs:484-551`) must copy the camera file; `mark_interrupted` (`:369`).
- **Session/pause model:** one `PauseShared` clock; samples captured inside a pause are dropped,
  later ones shifted by the pauses that ended before them (`core.rs:132,156-173,195`); screen PTS
  retimed + `VideoTimestampGate` + `startSessionAtSourceTime` at the first frame
  (`capture_macos.rs:13-19,216-271`); mic and cursor start at `origin_ns` = first admitted frame
  (`session.rs:763-817`, `mic.rs:10-37` uses `set_origin(origin_ns)`); stop = capture first, then
  mic `pad_to(end)`/`truncate_to(end)` (`session.rs:834-876`). **A camera writer slots in exactly
  like the screen writer** (retime by `clock.offset_at`, gate, own `AVAssetWriter`) with its session
  started at `origin_ns` (→ `webcamOffsetSec = 0` and pauses identical) or at its own first frame
  (→ positive offset). End it with `endSessionAtSourceTime(screen end)` so its duration covers the screen (§2d ⚠).
- **objc2 deps:** `objc2-av-foundation` 0.3.2 with only `AVAssetWriter, AVAssetWriterInput,
  AVMediaFormat, AVVideoSettings, AVAudioSettings, block2, objc2-core-media` (`IR/core/rust-lib/Cargo.toml:193`);
  needs `AVCaptureSession, AVCaptureDevice, AVCaptureInput (DeviceInput), AVCaptureOutputBase,
  AVCaptureVideoDataOutput, AVCaptureSessionPreset, AVCaptureDeviceDiscoverySession` (+ `dispatch2`
  queue, already used by SCK). `objc2-core-video`/`core-media` already present (`:189-190`).
  `mic_capture` (cpal) is unrelated.
- **Config/HUD:** `RecordConfig{project_dir, countdown, cursor_mode, system_audio, mic, open_editor,
  auto_zoom}` + `StartOptions{system_audio, mic, cursor_mode, countdown, title}` (`session.rs:176-205,322-331`);
  HUD toggles system audio / mic / cursor / countdown (`IR/core/frontend/src/components/RecordHud.tsx:113-152`);
  settings boxes (`RecordSettingsSection.tsx:32-91`). Camera on/off + device picker (+ quality)
  belong in both.
- **Editor doc:** `EDITOR_VERSION = 3` (`editor.rs:15`), `EditClip.source` = take id, `Appearance`
  (`:210`), presets = `{appearance, cursor, aspect}` (`presets.rs:41-47`). No camera fields.
- **Scene emission:** always `preset:"no-webcam"`, `webcamSize 1, webcamShape "rectangle",
  webcamMirror false, webcamPosition null, webcamReactiveZoom false`, `screenRect` + `layoutByClip`
  from the no-webcam layout port (`scene_json.rs:363-378`); clips `webcamPath ""`, `webcamOffsetSec 0`
  (`:265,268`); no `webcamEffect`, no `layoutRegions`. Contract test `scene_json.rs:718-726` pins these keys.
- **Editor UI:** Inspector tabs `background | frame | cursor | zoom | annotation | audio`
  (`IR/core/frontend/src/components/editor/Inspector.tsx:77-195`) — a `camera` tab is the natural home.
- **Permissions:** no `NSCameraUsageDescription` (install `:425-445` injects AppleEvents/Mic/Bluetooth;
  DMG `:70-80` AppleEvents/Mic only), no camera entitlement (`entitlements.plist:52` is audio-input).
  The DMG is ad-hoc without hardened runtime (`pack-macos-release-dmg.sh:82-87`); the local install
  uses `--options runtime` (`install-macos.sh:291`) → **entitlement required there**.

## 6. Risks and expected vendor patches

Risks:
1. **Missing `NSCameraUsageDescription` = crash** (TCC aborts) — add to both scripts + entitlement
   `com.apple.security.device.camera` before any camera code runs.
2. **Camera shorter than screen → export truncates / fails** (§2d). Mitigate in IR (writer
   `endSessionAtSourceTime(screen end)`, verify `available_duration_sec` reads ≥ screen) or patch.
3. **Camera start latency** (AVCaptureSession 0.3–1 s): starting it before the screen and timing its
   PTS on the shared clock keeps offset 0; otherwise a positive offset shows the first camera frame
   held at the start. Decide.
4. **Disconnect/interrupt mid-take** (`AVCaptureSessionWasInterrupted`, runtime error): the file must
   still finalize; whether to re-open (like the mic) or end the camera is a decision.
5. **Two hardware encoders at once** (SCK 60 fps up to 4K + camera 1080p30) — measure; upstream's
   2160p default is Windows-measured, 1080p is the safer default here.
6. **Layout is app-side**: porting PiP + block + camera-only + per-clip resolution + Auto-aspect with
   preset + zoom limit with the block screen box; must stay pixel-identical to the helper (golden
   fixtures like stage 3).
7. **ORT in the helper**: binary size and build time grow; model must ship (Resources) with Apache-2.0
   attribution in `Contents/Resources/THIRD-PARTY/`; static linking means the helper's ad-hoc sign
   covers it.
8. **Mirror at capture**: make sure the AVCaptureConnection does not mirror (`videoMirrored = false`,
   `automaticallyAdjustsVideoMirroring = false`); mirror only via `webcamMirror` (unverified default on macOS data outputs).
9. **Presets/takes:** camera style in presets (upstream excludes anchor/framing); per-take camera for
   inserted takes.
10. **Shortcut clash:** upstream C = layout here vs IR C = crop.

Expected vendor patches:
- **P16 segmentation on IR's ort** (Cargo, `runtime_available`, test model path) — §3 (b).
- Possibly **P17 camera length**: don't truncate the clip to the camera's end / don't bail when the
  camera ended earlier; hold the last frame (parity with the preview). Avoidable if IR guarantees coverage.
- Possibly model path resolution (like `OPENSCREEN_FONTS_DIR`) — not needed if IR sends
  `webcamEffect.modelPath` (field already parsed, `scene.rs:953-955`).
- No patch needed for layouts, PiP styling, reactive shrink, mirror, cutout/blur/custom shader,
  layout regions, Full Camera, multi-clip cameras.

## 7. Decisions for the plan

1. Camera capture: native AVCaptureSession → AVAssetWriter H.264 `camera.mp4`; resolution default
   (1080p? device max?), 30 fps, bitrate ladder (upstream 16 Mb/s @1080p).
2. Alignment: camera on the shared `PauseShared` timeline with session start at `origin_ns`
   (offset 0) vs own first frame (offset > 0); end at screen end (risk 2).
3. Mid-take disconnect: re-open vs end; and how the HUD reports it.
4. Segmentation backend: (b) IR ort rc.12 in the helper (recommended) vs Apple Vision; default
   background mode `none`.
5. Scope of layouts: global preset only, or also upstream's `layoutRegions` partition ("change
   layout here") with 0.2 s morphs; legacy Full Camera not needed.
6. Editor doc v4 fields (mirror upstream names/defaults: preset, size 15–60 = 40, mask square/
   rectangle, roundness defaults 0.7/0.4, mirror false, reactive true, anchor bottom-right, crop/pan,
   background mode/blur 0.5/wallpaper) and per-take camera meta.
7. Whether camera style joins `.irpreset` (upstream: yes for style, no for anchor/framing).
8. Shortcut for "layout here" (C is taken by crop).
9. Camera UI: Inspector `camera` tab; HUD toggle + device picker + live camera preview or not.
10. Whether to vendor-patch the export length coupling or guarantee coverage in the recorder.
