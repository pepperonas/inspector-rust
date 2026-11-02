# Stage 4 recon — annotations, audio, shortcuts, presets, reopening projects (OpenScreen v2.1.2 → Inspector Rust)

Read-only. Nothing in the clone was executed. Path prefixes:
- `V/` = `/Users/martin/claude/inspector-rust/vendor/openscreen-compositor/` (vendored compositor + patches 1–12)
- `C/` = `<scratchpad>/openscreen-src/new/` (untrusted upstream clone, TS/Electron side)
- `IR/` = `/Users/martin/claude/inspector-rust/`

Upstream citations came from three read-only sweeps of C/ and V/; the load-bearing ones were spot-checked
(`V/src/audio.rs:91`, `V/src/text_fonts.rs:21`, `V/VENDORED.md:57-58`, `V/src/scene.rs:963,975,979`,
`V/src/compositor_macos.rs:1973`). IR citations are from direct reading.

## 0. TL;DR

- **Annotations are 100 % compositor-side.** Text, image, arrow and blur/mosaic are Scene fields
  (`Scene.annotations`, `V/src/scene.rs:963`, structs `:473-592`) drawn by `compose_frame` — the
  same function for export AND our LiveView preview (`V/src/live.rs:1196-1210,1524,1745,1854` →
  `V/src/compositor_macos.rs:2924` privacy pass, `:3178/3192` top layer). Upstream's DOM only draws
  selection/drag handles (`C/src/components/ai-edition/AnnotationOverlay.tsx:1-12`). Work for us =
  doc model + `scene_json` emission + editing UI. **No vendor patch needed** for the basic feature.
- **Two compositor gaps:** (a) **freehand blur is NOT implemented** — it masks the bounding box
  (`V/src/compositor_macos.rs:1969-1973`; `freehand_points` parsed, never read outside tests,
  `V/src/scene.rs:1425`) → either drop "frei" from scope or write a vendor patch; (b) **fonts are not
  vendored** (`V/VENDORED.md:57-58`) — without `OPENSCREEN_FONTS_DIR` (`V/src/text_fonts.rs:21`)
  all five families fall back to system fonts (`V/src/text_macos.rs:610-622`). The 10 TTFs + OFL
  texts exist in `C/public/fonts/` (SIL OFL 1.1, `C/THIRD-PARTY-NOTICES.md:144-166`) → shippable.
- **Audio mixing is 100 % compositor-side, export only.** `Scene.audio` (`SceneAudio{gainDb}`,
  `V/src/scene.rs:817-820,975`) and `Scene.audioTracks` (`SceneAudioTrack`, `:833-867,979`) are
  already parsed and honoured by the macOS export pipeline (`V/src/pipeline_macos.rs:1355-1366,
  1443-1457`) — we send neither today (`IR/core/rust-lib/src/record/scene_json.rs:288-306`).
  LiveView has **no audio at all**; preview audio stays our `AudioPreview` `<audio>` elements.
- **Limiter correction:** v2.1.2's true-peak ceiling is **−2 dBTP**, not the spec's −1.5 dBFS
  (`V/src/audio.rs:81-91`, `LIMITER_CEILING = 0.794_328_2`; comment explains −1.5 overshot after
  AAC). It **already runs on every export** (`finish_audio`, `V/src/audio.rs:64-79`). Keep −2.
- **Loudness inversion bug in our current export** (pre-existing, stage 2): system audio in
  `screen.mp4` is loudness-normalised to −16 LUFS with up to +12 dB boost
  (`V/src/audio_jobs.rs:99`), the mic `extraAudio` is NOT (`V/src/audio.rs:508-569`). Upstream
  assumes the screen file IS the voice. Needs a vendor patch (§2f).
- **Shortcuts:** upstream = 12 configurable actions as `{key, ctrl?, shift?, alt?}` objects in
  `userData/shortcuts.json` (`C/src/lib/shortcuts.ts:1-14,18-24,111-140`;
  `C/electron/ipc/handlers.ts:173,5004-5021`). Ours are hard-coded (Space/←→/S/Delete/C/⌘Z,
  `IR/core/frontend/src/components/RecordEditor.tsx:471-516`; Z + Delete-on-zoom in
  `components/editor/Timeline.tsx:132-145`). **Note: upstream S = Add Speed, our S = Split; upstream
  C = layout, our C = crop** — a mapping decision.
- **Undo depth 50 already done** (`IR/core/frontend/src/lib/editor.ts:27,187-224`). Missing:
  snapping (8 px), waveform, clip reorder/duplicate/insert, copy/paste, Tab cycling.
- **Presets:** upstream `.openscreenpreset` = `{format, version:1, name, appearance}` with ~25 look
  fields (`C/src/lib/ai-edition/stylePresets.ts:41-97`), stored as files in `Documents/OpenScreen
  Presets/`. Ours: build own versioned format over our `Appearance` + `CursorStyle`
  (`IR/core/rust-lib/src/record/editor.rs:160-256`); nothing preset-like exists yet.
- **Reopening projects already mostly works:** the `record` panel lists projects (newest first,
  max 20 rows) and Enter opens the editor (`IR/core/frontend/src/components/RecordPanel.tsx:27,
  111-114,147-152`; `IR/core/rust-lib/src/record/editor_ipc.rs:798-857`). The project-folder setting
  **exists** (`IR/core/rust-lib/src/record/session.rs:178-190,207,263-275`;
  `IR/core/frontend/src/components/RecordSettingsSection.tsx:46-63`). Missing: search, rename,
  "open from elsewhere"/browse, opening another project from inside the editor, list beyond 20,
  sort by last edit (`updated_at` is written, `editor_ipc.rs:716`, but listing sorts by
  `created_at`, `project.rs:244`).

## 1. Annotations

### 1a. Upstream data model (zod, `C/src/lib/ai-edition/schema/index.ts`)

`annotationRegionSchema` (`:457-496`):
| field | type / range | default |
|---|---|---|
| `id` | string | — |
| `startMs`, `endMs` | ≥0, end ≥ start (`endGteStart` `:54ff`, applied `:488-489`) | — |
| `clipId?`, `sourceStartSec?`, `sourceEndSec?` | clip anchor (`clipAnchorShape` `:451-455`) — since schema v5 the anchor is the truth, ms is a cache (`:29-33`, upgrader `:830-873`); a span over clips = one piece per clip | — |
| `type` | `text \| image \| figure \| blur` (`:463`) | — |
| `content` | string (`:464`), `textContent?`, `imageContent?` (`:465-466`) | `""` |
| `space?` | `"frame"` (`:472`) — text/image/arrow are frame-relative; absent = relative to the unzoomed screen rect (blur always) (`C/src/lib/ai-edition/annotations/placement.ts:41-44`) | — |
| `position {x,y}` | 0..100 (figures may go <0) (`:474-477,490-496`) | 50/50 (`C/src/components/video-editor/types.ts:425`) |
| `size {width,height}` | >0, percent (`:478-481`) | 30×20 (`types.ts:430`) |
| `style` | `annotationStyleSchema` (`:420-443`) | `types.ts:435-445` |
| `zIndex` | int ≥0 (`:483`) — new = max+1 (`C/src/lib/ai-edition/store/useTimeline.ts:539`) | — |
| `annotationSource?` | `"auto-caption"` (`:484`) — stage 6 | — |
| `figureData` | `:410-418` | dir right, `#34B27B`, stroke 4 (`types.ts:447-451`) |
| `blurData` | `:397-408` | `types.ts:465-472` |

`annotationStyleSchema` (`:420-443`): `color` `#ffffff`; `backgroundColor` `transparent` (new text gets
`DEFAULT_TEXT_PLATE = "rgba(0, 0, 0, 0.7)"`, `C/src/lib/ai-edition/annotations/background.ts:20`);
`fontSize` 32 clamped [8,200] (`C/src/lib/projectDefaults.ts:321`); `fontFamily` "Inter";
`fontWeight` normal|bold (default bold); `fontStyle` normal|italic; `textDecoration` none|underline;
`textAlign` left|center|right (default center); `textAnimation?` ∈ `none, fade, rise, pop,
slide-left, typewriter, pulse` (`:440-442`). Font size to compositor = `fontSize/1080`
(`C/src/lib/ai-edition/annotationScale.ts:26-33`).

Figure (arrow): `arrowDirection` ∈ up, down, left, right, up-right, up-left, down-right, down-left
(default right); `color` `#34B27B`; `strokeWidth` 4 (`schema :410-418`).

Blur: `type` blur|mosaic (default mosaic), `shape` rectangle|oval|freehand (default rectangle), `color`
white|black (default white), `intensity` 12, `blockSize` 12, `freehandPoints?: {x,y}[]`
(`schema :397-408`). UI ranges only (NOT clamped by schema or compositor): `BLUR_INTENSITY` 2..40,
`BLUR_BLOCK_SIZE` 4..48 (`types.ts:357-362`, normalisers `C/src/lib/blurEffects.ts:23-38`).
Default freehand = 9 points (`types.ts:453-463`).

Fonts list: `TEXT_FONT_FAMILIES = ["Inter","Lora","Oswald","Caveat","IBM Plex Mono"]`
(`C/src/lib/textFonts.ts:13`), unknown → Inter (`:24-28`), file pattern
`fonts/<FamilyNoSpaces>-{Regular|Bold}.ttf` (`:31-33`).

Animations: TS `TEXT_ANIMATION_DURATION_MS = 700`, entry only, easeOutCubic
(`C/src/lib/ai-edition/annotations/textAnimation.ts:16,55-108`): fade (opacity), rise (translateY
(1−e)·18 px), pop (scale max(0.72, easeOutBack)), slide-left (translateX (1−e)·−28 px),
typewriter (linear reveal), pulse (scale 1+sin(πp)·0.06). **The Rust port (what we render) adds a
300 ms exit** (`V/src/text_anim.rs:12-107`, `TEXT_EXIT_DURATION_MS` `:18`), short regions split
time proportionally (`:74-78`), pulse has no exit (`:70-72`), offsets in px @1080 (`:12`). → Since the
compositor draws text in our preview too, no TS animation code is needed.

Creation: new text box auto-fit via `fitTextBox`, localized default content, `space:"frame"`
(`useTimeline.ts:506-560`). Shortcut A adds an annotation (§3); Tab/Shift+Tab cycles annotations
sorted by `startMs` (`C/src/components/ai-edition/NewEditorShell.tsx:1496-1512`).

### 1b. How they reach the compositor

`Scene` has `#[serde(default)] pub annotations: Vec<SceneAnnotation>` (`V/src/scene.rs:955-963`).
All camelCase, Deserialize:
| struct | lines | fields |
|---|---|---|
| `SceneAnnotation` | `:473-509` | `id` (default), `clipIndex: Option<usize>`, `startSec`/`endSec: f64` (**source seconds of that clip**), `kind: String` (`text\|image\|figure\|blur`), `space: Option<String>`, `x,y,w,h: f32` (0..1), `zIndex: i32` (default), options `text`, `imagePath`, `figure`, `blur`; helpers `anchor_rect`/`in_frame_space` `:511-527` |
| `SceneAnnotationText` | `:529-561` | `content, color, backgroundColor, fontSizeRel (f32), fontFamily, fontWeight, fontStyle, textDecoration, textAlign`, `verticalAlign: Option`, `animation: Option` |
| `SceneAnnotationFigure` | `:563-570` | `direction, color, strokeWidth: f32` |
| `SceneAnnotationBlur` | `:572-586` | `style, shape, color, intensity, blockSize, freehandPoints: Option<Vec<SceneAnnotationPoint>>` |
| `SceneAnnotationPoint` | `:588-592` | `x, y` |

TS fill: `C/src/native/sceneDescription.ts:203-274` (interface), `:1569-1664` (fill): regions projected
to source per clip (`projectRegionsToSource`, `:1144-1152`), rect = percent/100 (`:1592-1595`), text
`content || textContent` + `fontSizeRel` + `resolveTextFontFamily` (`:1598-1623`), image
`imagePath = content || imageContent` (`:1630`), figure defaults (`:1632-1641`), blur defaults
(`:1643-1661`), freehand points /100 (`:1652-1658`), sorted ascending by zIndex (`:1663-1664`).

Visibility: `source_t ∈ [startSec, endSec)` (`V/src/compositor_macos.rs:1918-1920`); LiveView keeps
only the active clip's annotations (`V/src/scene.rs:1042-1044`, `V/src/live.rs:884-893`); paint order =
list order (`compositor_macos.rs:1932-1934`). Blur always in an earlier "privacy" pass under the
cursor regardless of zIndex (`:2924`). Stale doc comment "Pas encore rendu" at `:2632` — ignore.

Our helper drives the preview through `LiveView` (`IR/record-render/src/session.rs:20,246,354`) and
`set_scene` (`:414`); annotations are scene data (not `set_param_*`, cf. the stage-3 effects gotcha
at `IR/record-render/src/session.rs:498-524`), so **preview = export by construction**.

### 1c. Fonts

- Files: `C/public/fonts/{Inter,Lora,Oswald,Caveat,IBMPlexMono}-{Regular,Bold}.ttf` (10) + one
  `<Family>-OFL.txt` each; all **SIL OFL 1.1**; notice + Reserved Font Names "Lora", "Plex" in
  `C/THIRD-PARTY-NOTICES.md:144-166` (already copied to `V/THIRD-PARTY-NOTICES.md:144-166`). ~2.3 MB
  (`V/VENDORED.md:57`). Shippable (OFL allows bundling; keep OFL.txt + RFN untouched — don't rename).
- Loading: runtime dir from env `OPENSCREEN_FONTS_DIR` (`V/src/text_fonts.rs:21-46`), upstream sets
  it in `C/electron/native-bridge/services/compositorViewService.ts:501-509`; macOS builds CT
  descriptors per file (`CTFontManagerCreateFontDescriptorsFromURL`, `V/src/text_macos.rs:463-499`),
  cached in a `OnceLock` per process (`:509-513`) → **must be set before the first text draw**, i.e.
  the client sets the env when spawning `ir-record-render` (`IR/core/rust-lib/src/record/render_client.rs`).
  Italic is CoreText-synthesized (`text_macos.rs:628-652`); no italic files.
- Plan: vendor fonts to e.g. `IR/core/rust-lib/assets/record/fonts/` (or the existing `AssetRoot`
  tree used for wallpapers/cursors/sounds, `IR/core/rust-lib/src/record/assets.rs`), bundle via
  the same path as cursors/sounds, OFL texts into `THIRD-PARTY/`, un-ignore the two font tests
  (`V/VENDORED.md` patch 7). Upstream bundles them via `extraResources` (`C/electron-builder.json5:167-174`).
- The editor UI (font picker preview in the webview) needs the same TTFs via `@font-face` from the
  asset scope, else the dropdown shows system fonts.

### 1d. Free-form blur path

Format: closed polygon of raw points, no smoothing, default 9 points. **Unit ambiguity upstream**:
TS says 0..100 within the annotation bounds (`types.ts:370`), `V/src/scene.rs:583` and
`sceneDescription.ts:264` say screen-rect fractions. **Not rendered** (bounding box,
`compositor_macos.rs:1969-1973`). Shader: mode 10 (`V/src/shaders.metal:3905-3969`): blur = mip lod
`log2(intensity)` (`:3946`, needs `mip_filter::linear` `:109-114`), mosaic = UV quantised to
`blockSize` px (`:3953-3960`), oval = smoothstep AA (`:3929-3937`), tint 50 % white/black mosaic
only (`:3963-3966`, flag `compositor_macos.rs:1981-1988`), amount `max(1)·mask.strength` (`:1996`).
Placement follows zoom/tilt: `FrameGeometry::privacy_mask` (`V/src/frame_geometry.rs:2710-2764+`),
pad `PRIVACY_MASK_PAD_PX = 1.0` (`:2959`); oval → rect under motion-blur trail (`:2762`).

### 1e. Image + arrow

- Image: upstream stores a **data URL** (`C/src/components/ai-edition/v4/FloatingInspector.tsx:1069-1082`);
  compositor accepts data URI (`V/src/frame_geometry.rs:90-115`) or file path (`image::open`,
  `compositor_macos.rs:1345-1375`); texture cache key = annotation id + **string length**
  (`:2007-2016`) → a swapped image of equal length won't refresh (use a project-copied file with a
  content-hashed name, like our own-cursor / own-background import); contain-fit, centred (`:2029-2050`).
  Recommendation: copy into the project dir (our `importAsset` path, `RecordEditor.tsx:307`) and send
  an absolute path — keeps scene JSON small (every `set_scene` re-sends it).
- Arrow: 3 round-capped segments in a 0..100 viewBox (`V/src/regions.rs:677-721`), scaled to the
  box's shorter side (`:730-758`), shader mode 9 (`shaders.metal:3894-3903`); unknown colour → white
  (`compositor_macos.rs:1958`). Colour parser accepts `#hex`, `rgb()/rgba()`, `transparent`
  (`frame_geometry.rs:120-136`).

### 1f. Our side — what's needed

- `EditDoc` v3 (`IR/core/rust-lib/src/record/editor.rs:13,307-329`, migrate `:398-416`): add
  `annotations: Vec<Annotation>` (SOURCE ms per clip like `ZoomRegion`, `:276-296`; normalise into its
  clip like `normalize_zooms`) with v2→v3 migration = empty list.
- `scene_json.rs` (`:288-306`): emit `"annotations"`; timeline track via `EditorView` programme spans
  computed in Rust (R12 rule, like `ZoomSpan`, `editor.rs:351-357`).
- Preview overlay: selection/drag/resize handles in `Preview`'s `overlay` slot like `FocusOverlay`
  (`IR/core/frontend/src/components/editor/FocusOverlay.tsx`); geometry must map frame-space vs
  screen-space (blur) using `screen_rects` (`editor.rs:372`) — screen-space boxes move with zoom in
  the compositor (`privacy_mask`), the overlay can only be exact when no zoom is active.
- Clamp blur intensity/block size ourselves (compositor does not).

## 2. Audio

### 2a. Upstream voiceover
- Recorded in the renderer with `MediaRecorder` (`C/src/components/ai-edition/v4/AddAudioLayerDialog.tsx:23-30,168-185`),
  `getUserMedia` with echo cancel + noise suppress, no device picker; saved as
  `RECORDINGS_DIR/voiceover-<ISO>.webm` ≤512 MB (`C/electron/ipc/handlers.ts:190,4381-4410`);
  registered as `kind:"audio"` asset (`C/src/lib/ai-edition/store/projectStore.ts:444-487`).
- Placement: playhead at record start; video plays, other tracks muted
  (`NewEditorShell.tsx:1039,1056-1070,1727`); max length = timeline end − playhead (`:1049`); then
  `addAudioTrack(assetId, startSec, {kind:"voiceover", spanSec})` (`:1077-1089`).
- **For us:** don't use `MediaRecorder` (house rule: webview mic-open disturbs CoreAudio — see the
  bpm/shazam native-capture lesson). Reuse native `MicStream` + `WavSink` from the recorder
  (`IR/core/rust-lib/src/record/session.rs:391-399,627-628`, `wav.rs`) → `voiceover-<ts>.wav` in the
  project dir. The bundled ffmpeg decodes WAV (no `--disable-decoders` in
  `IR/scripts/build-ffmpeg-macos.sh`).

### 2b. Upstream music/audio tracks
`audioTrackSchema` (`C/src/lib/ai-edition/schema/index.ts:678-705`): `id`, `trackId?` (groups per-clip
fragments), `startMs/endMs` (raw ruler), clip anchor (`:451-455`), `assetId`, `kind` voiceover|music
(default music), `durationSec` (0), `offsetMs` int ≥0 (start offset into the file; replaced
trimStart/trimEnd `:666-670`), `gainDb` −60..+12 (0), `loop` (false), `fadeInMs/fadeOutMs` int ≥0 (0),
`muted` (false), `label` (""), `origin`. Document `audioTracks` default `[]` (`:776`).
Creation defaults (`:1490-1536`): music −18 dB, 1000 ms fades; voiceover 0 dB no fades.
UI: gain −60/+12 (`C/src/lib/ai-edition/store/editorSettings.ts:54-55`), output gain limit ±12
(`:53`), `AUDIO_FADE_MAX_MS = 5000` (`C/src/lib/ai-edition/document/audioTracks.ts:353`); sliders gain
step 0.5, fade step 50, loop checkbox (`C/src/components/ai-edition/RightPanes.tsx:3549-3620`); fades
scaled down proportionally when longer than the span (`resolveFadeSecs`, `audioTracks.ts:367-378`).
Shortcut M opens a file picker (`NewEditorShell.tsx:1474-1477`).

### 2c. Gain, limiter
- No per-device mic gain upstream; fixed `MIC_GAIN_BOOST = 1.4` only when system audio is also
  captured + 20 ms mic fade-in (`C/src/lib/audioMix.ts:6,16-18,25`). Editor has one global
  `audioGainDb` ±12 (`editorSettings.ts:125,146,318-321`). Ours: `mic_gain_db` −60..+12
  (`IR/core/rust-lib/src/record/editor.rs:20-21,318`).
- Limiter (export only), `V/src/audio.rs`: `finish_audio` gain ±12 then `limit_peaks` (`:64-79`);
  `LIMITER_CEILING = 0.794_328_2` = −2 dBTP (`:81-91`); lookahead 240 samples (~5 ms) (`:93`); release
  80 ms (`:95`); makeup ≤3 dB in ≤6 passes to 0.1 LU (`:107-114`); 4× oversampled windowed-sinc 6 taps
  (`:117-145`); `true_peak_envelope` `:147`; `limiter_gains` `:188-229`; `limit_peaks` `:292-308`. Voice
  loudness −16 LUFS, ≤+12 dB boost, BS.1770 gating (`:320-408`); `loudness_gain_db(path)` public,
  cached by path/size/mtime (`:455-476`). Upstream preview's WaveShaper still says −1.5 dBFS
  (`C/src/components/ai-edition/VirtualPreview.tsx:357-392`) — stale.

### 2d. Where mixing happens + Scene fields
All in the compositor (no ffmpeg CLI): `V/src/pipeline_macos.rs:1355-1366,1443-1457`:
1. per-clip decode + loudness-normalise + speed-stretch (`decode_and_stretch_clip_audio_with_extra`,
   `V/src/audio_jobs.rs:86-140`);
2. `assemble_concatenated_pcm` (`V/src/audio.rs:1872`);
3. `mix_external_tracks` (`:1937`): voiceovers summed + normalised (`:1994-1998`), music ducked −10 dB
   under the whole programme (threshold −35 dBFS, hold 0.5 s, attack 0.25, release 0.6, `:2073-2091`);
4. `mix_click_hits`; 5. `finish_audio`; 6. AAC 256 kb/s (`:26`; encoder always opens,
   `pipeline_macos.rs:1334`).
Scene: `SceneAudio{gainDb}` (`V/src/scene.rs:817-820`) as `audio` (`:975`);
`SceneAudioTrack{path, startSec, gainDb, trimStartSec, trimEndSec: Option, fadeInSec, fadeOutSec, kind}`
(`:833-858`), kind `music|voiceover` (`:863-867`) as `audioTracks` (`:979`); `clickSound` (`:983`);
`SceneClip.extraAudio` (`:34`, `ExtraAudio{path, offsetSec, gainDb}` `:47-53`) = our patch 8.
**`startSec` is OUTPUT (programme) time** — TS projects raw ruler → playback through trims+speed
(`C/src/native/sceneDescription.ts:923-1047`): each take mixed once from its head fragment
(`takesByHeadFragment` `:915`), non-looping voiceover split around cuts (`takeProgramme`,
`:980-1016`; its words are cut with the video), non-looping music one entry (`:1018-1028`), loop = one
entry per repeat ≤1000 with fades only on the outer edges (`:1033-1046`). Speed regions only shift a
track's start, never stretch it. ⚠️ `trimEndSec` must be set or the track is silently skipped
(`V/src/audio.rs` overlay ≈`:1966`). Our pipeline already reads `audio_tracks` from
`comp.scene_snapshot()` → **sending `audioTracks` needs no patch**.

### 2e. What ours does today
- Mic: `extraAudio = {path: <dir>/mic.wav, offsetSec, gainDb: doc.mic_gain_db}` on every clip
  (`IR/core/rust-lib/src/record/scene_json.rs:197-219`); system audio = `hasAudio:
  meta.system_audio` (`:214`); `clickSound` (`:309-320`); **no `audio`, no `audioTracks`**.
- Helper forwards `has_audio`/`extra_audio` (`IR/record-render/src/session.rs:607-622`), export via
  `run_composited_multi_cancellable` (`:465-475`); audio job runs when
  `has_audio || extra_audio.is_some()` (`V/src/pipeline_macos.rs:1393,1446`).
- Preview: `AudioPreview` = two `<audio>` elements (system + mic) + click `new Audio`
  (`IR/core/frontend/src/components/editor/AudioPreview.tsx:24-27,116-123,133-134`); volume ≤1 so mic
  gain >0 dB is inaudible in preview; no loudness gain, no limiter. No waveform anywhere.

### 2f. Waveform
Upstream: ffmpeg CLI decode mono 16 kHz s16 (`C/electron/media/audioPeaks.ts:1-28,41`), Float32
`[min,max,…]` at 200 blocks/s capped 24 000 (`:45-46,214,225-268`), cache
`userData/audio-peaks/<sha(v2:path:size:mtime)>.f32` (`:336-395`), IPC `get-audio-peaks`
(`handlers.ts:4524-4541`); drawn smoothed by `waveformPaths`/`ClipWaveform`
(`C/src/components/ai-edition/v4/V4Timeline.tsx:339-372`, in clips `:2640` and audio pills `:529,569`).
For us: `mic.wav` → read directly with `wav.rs` in the app (no helper); system audio inside
`screen.mp4` and music files need decoding → a new helper command using public
`audio::decode_clip_audio` (`V/src/audio.rs:749`). Cache next to the project (`peaks-*.f32`) keyed
by size+mtime. Peaks are SOURCE-time; the timeline must project them through clips/speed (Rust, R12).

### 2g. Risks / patches
1. **Loudness inversion** (§0) — patch `decode_and_stretch_clip_audio_with_extra`
   (`V/src/audio_jobs.rs:86-140`): option to normalise `extraAudio` (the voice) and NOT the system
   track. Today a quiet mic + loud system sound gets the opposite of what's wanted.
2. **Mono music at −3 dB/side:** `overlay_external_track` uses `decode_clip_audio` with
   `mono_at_unity=false`; route tracks through the patch-9 path (`V/VENDORED.md` patch 9).
3. **Ducking key** is the whole programme incl. system audio (`V/src/audio.rs:1946-1948`) — music
   ducks under any loud system sound; voice-keyed ducking = patch (optional).
4. Ceiling: keep −2 dBTP; update the spec text (−1.5 is upstream's own abandoned value).
5. Preview parity needs WebAudio (GainNode >1, fades, a −2 dBTP WaveShaper, optional ducking via
   analyser) + a helper/IPC exposing `loudness_gain_db`. Separate `<audio>` per track seeked on
   programme time. Watch the CLAUDE.md native-mic rule: preview playback through WebAudio is fine
   (output only); voiceover capture must stay native.
6. GIF export drops audio (expected).

## 3. Editor timeline features vs inventory

| feature | upstream | ours |
|---|---|---|
| Clip reorder | pointer drag → `moveClip` (`V4Timeline.tsx:1558-1575,1644,1742`; `useTimeline.ts:1548-1556`) | **missing**. `EditDoc.clips` is an ordered Vec, `normalize` keeps order and does not sort (`IR/core/rust-lib/src/record/editor.rs:452-480`); mapping walks clips in vec order (`:669-702`) — reorder is a pure Rust op re-indexing `speed`/`crop`/`zooms` (all keyed by clip index, `:44-49,314,279`) |
| Duplicate | ⌘C/⌘V → `duplicateClip`, inserted after the original (`NewEditorShell.tsx:1403-1432`; `useTimeline.ts:1560-1572`) | **missing** (same re-index concern) |
| Insert media | drag asset / "Add to timeline", always appended (`NewEditorShell.tsx:576-597`; `useTimeline.ts:1504-1543`) | **missing**. Compositor supports per-clip `screenPath` (each `SceneClip` has its own path, `V/src/scene.rs:~34`), but our `EditClip` has no source (`editor.rs:37-40`, `scene_json.rs:205-221` uses one `screen` for all). Needs `EditClip.source` + per-source meta/cursor + LiveView source switching. **Recommend deferring** (out of stage-4 core; heavy) — reorder/duplicate within one take is cheap |
| Snapping | `PILL_SNAP_PX = 8` (`V4Timeline.tsx:160-161`), → `8/pxPerSec` s (`:1063`); targets = timeline start, end, every clip start/end (`:1054-1059`) — NOT playhead/regions; applies to region-pill move/resize (`:1119-1126`) and audio drags (`:1284-1300`); guide line `snapPct` (`:2399-2400`) | **missing**; our zoom drags `moveZoomProg`/`trimZoomProg` (`IR/core/frontend/src/lib/editor.ts:451-486`) are the hook point (pure, testable) |
| Waveform | §2f | **missing** |
| Undo 50 | `MAX_HISTORY = 50` snapshot stack (`C/src/lib/ai-edition/store/undoStack.ts:29,42-46`); drags = one step via `historyBase` (`projectStore.ts:113-128`); preset apply = one step | **done**: `HISTORY_MAX = 50` (`lib/editor.ts:27,187-224`), live gestures one step (`RecordEditor.tsx:149-154,518-535`) |
| Track order | annotation, speed, trim, zoom, layout, shader, audio, clips (`V4Timeline.tsx:2410-2545`, edit mode `:771`) | ours: filmstrip+clips+speed band, zoom track (`components/editor/Timeline.tsx:272-415`). Add annotation + audio lanes |
| Copy/paste regions | ⌘C/⌘V/⌘X (`NewEditorShell.tsx:1403-1432,1413`) | missing |

### 3a. Shortcuts — every upstream binding
Configurable (`C/src/lib/shortcuts.ts:1-14,111-140`):
| id | default | action |
|---|---|---|
| openApp | ⌘⇧O (global) | open app (n/a for us — IR has its own global hotkeys) |
| addZoom | Z | add zoom |
| addTrim | T | add trim region |
| addSpeed | S | add speed region |
| addCameraFullscreen | C | "change layout here" (event `openscreen:change-layout`, `NewEditorShell.tsx:1489-1492`) — stage 5 |
| addAnnotation | A | add annotation |
| addAudio | M | add audio (file picker, `:1474-1477`) |
| addVoiceover | V | record voiceover |
| deleteSelected | ⌘D | delete selection |
| playPause | Space | play/pause |
| copySelected | ⌘C | copy selection |
| paste | ⌘V | paste |

Fixed (`shortcuts.ts:35-78`): ⌘Z undo; ⌘⇧Z / ⌘Y redo; Tab/⇧Tab cycle annotations; Delete/Backspace
delete; scroll pans; ⌘scroll zooms timeline; ←/→ one frame. Hard-coded in the handler
(`NewEditorShell.tsx:1318-1523`): ⌘S save (`:1330`), ⌘N new (`:1335`), ⌘O open (`:1349`), `?` /⇧/ shortcuts
dialog (`:1365`), ⌘X cut (`:1413`), arrows step 1/60 s (`:1513-1519`). Ignored while typing / modal open
(`:1320-1328`). Panel-local: crop modal +/=/− zoom + arrow nudge (`RightPanes.tsx:3374-3390`); layout
lane ←/→ 0.1 s, ⇧ 1 s (`v4/LayoutLane.tsx:63-68`). No J/K/L.

Format: `{ key: string; ctrl?; shift?; alt? }`, `ctrl` = ⌘ on Mac (`shortcuts.ts:18-24,142-156`).
Storage: `userData/shortcuts.json`, 2-space JSON of the whole config (`handlers.ts:173,5004-5021`),
merged over defaults on load (`mergeWithDefaults`, `shortcuts.ts:206-214`;
`C/src/contexts/ShortcutsContext.tsx:47-56`). Conflicts: fixed key → refused "reserved"; other
configurable → offer Swap/Cancel (`findConflict` `shortcuts.ts:93-109`;
`C/src/components/video-editor/ShortcutsConfigDialog.tsx:66-127`). Empty lanes show the binding as a hint
(`V4Timeline.tsx:2405-2441`).

Ours today: Space play, ←/→ frame (⇧ 1 s), S **split**, Delete/Backspace delete clip (or selected zoom),
C **crop**, Esc leaves crop, ⌘Z/⌘⇧Z (`RecordEditor.tsx:471-516`), Z new zoom (`Timeline.tsx:132-145`).
Recommendation: store in our `settings` table as JSON (`record.shortcuts`, same `{key,ctrl,shift,alt}`
shape, merged over a Rust- or TS-owned default table + a test pinning defaults), Settings → Recording
section or a `?` dialog in the editor; keep our S=split / C=crop as **our** defaults (documented
deviation) or adopt upstream — a user decision. One matcher function owned by `lib/` (pure, tested);
both `Timeline` and `RecordEditor` handlers must use it (the `defaultPrevented` ordering in
`RecordEditor.tsx:473` must survive).

## 4. Presets

Upstream (`C/src/lib/ai-edition/stylePresets.ts`): ext `.openscreenpreset`, `format:
"openscreen-style-preset"`, `version: 1`, name ≤80 chars, reserved id `openscreen-factory`, data-URL
limit 15 MB (`:41-48`). File = `{format, version, name, appearance}`, 2-space JSON + trailing newline
(`:92-97,400-411`). `appearance` (`:53-79`): wallpaper, wallpaperMotion, frame, frameTheme, aspectRatio,
shadowIntensity, backgroundBlur, motionBlurAmount, depthOfField, borderRadius, padding, webcam* (9 fields),
cursor{size, smoothing, motionBlur, clickBounce, model3d, asArrow, clickImpact, clickSound,
clickSoundGainDb}, cursorShow, cursorAutoHide, cursorTheme. Excluded on purpose: crop, webcam position,
audio, captions, timeline (`:8-11`). Validation: version ≠1 rejected (`:490-508`), all fields required
(back-compat exceptions `:276-360`), numbers clamped, unknown cursor theme → default, wallpaper only
CSS colour/gradient, bundled `/wallpapers/…`, or PNG/JPEG data URL — `file://`, `http`, `url(` refused
(`:247-274`). Storage `Documents/OpenScreen Presets/`, one file per preset, sanitized name
(`handlers.ts:5029-5033`; `:383-391`); folder re-read on list = only "import"
(`C/electron/ai-edition/style-preset-service.ts:1-4,97-103,250-273`); case-insensitive unique names
(`:277-293`), atomic writes. Ops: list/create/rename/update/delete/setForNewProjects/reveal
(`C/electron/ipc/nativeBridge.ts:826-868`); marker `.new-project-preset` (`style-preset-service.ts:47`)
chooses the look of new projects, else the most recently edited project's look
(`document-service.ts:408-424`); aspect never copied to new projects (`stylePresets.ts:413-470`).
Apply = one patch, one undo step (`StylePresetsMenu.tsx:191-203`; `stylePresetsEditor.ts:78-90`).
Built-in: only "factory" (`stylePresetsEditor.ts:74-76`).

Ours (proposal, own format per spec): `{format:"inspector-rust-record-preset", version:1, name,
appearance: Appearance, cursor: CursorStyle, aspect?: Aspect}` — reuse the serde types
(`IR/core/rust-lib/src/record/editor.rs:63,160-256`) so normalisation/clamps are shared; refuse own-image
backgrounds / own cursors that point outside the preset (copy them into a preset folder, or drop to
default on apply — upstream allows only data URLs). Store in the settings table or
`<project root>/.presets/` (house style prefers DB for app state; files only if user-shareable is a
goal). "Use for new projects" = setting `record.default_preset`, applied in `default_doc`
(`editor.rs:381-395`) — careful: `default_doc` is pure today; pass the preset in. Apply = one
`History.push` (`lib/editor.ts:193`).

## 5. Reopening projects

Upstream: no recents file, no cap; "Open project" lists every `.openscreen` in `userData/projects/`
sorted by `updatedAt` (`handlers.ts:5041`; `document-service.ts:2,34,42-47,376-386`); `OpenProjectModal`
with search, ↑/↓+Enter, inline-confirm delete, date, "Browse files" (`C/src/components/ai-edition/Modals.tsx:117-231`);
editor auto-opens the newest project on launch (`NewEditorShell.tsx:417-430`); browse =
`load-project-file` dialog (`handlers.ts:4711-4779`); project folder fixed (the `projectFolder` pref is
dead code, `C/src/lib/userPreferences.ts:8,21-22,131-133`; `NewEditorShell.tsx:699`), only the recordings
folder is changeable (`C/electron/main.ts:123-148`). Autosave: every edit saves immediately
(`projectStore.ts:547-561`); ⌘S saves again (`NewEditorShell.tsx:915-919`).

Ours:
- List: `record_list_projects` → `list_items` → `list_projects` (`IR/core/rust-lib/src/commands.rs:7154-7163`;
  `record/session.rs:343-356`; `record/project.rs:197-245`), newest first by `created_at` (`project.rs:244`),
  broken → Failed; panel renders `MAX_ROWS = 20` (`RecordPanel.tsx:27`), Enter opens editor
  (`:147-152`), ⌘Enter reveals, two-stage delete.
- Open: `editor_open` IPC (`commands.rs:7193-7207`) → `open_editor` (`editor_ipc.rs:798-857`): any dir
  (no root check — note `project_dir_inside_root` guards only delete/reveal, `session.rs:91,143,1260`),
  refuses failed / missing `screen.mp4` (`:804-806`), saves the previous session first (`:816-844`).
- Save: debounced `schedule_save` → `save_session` writes `editor` + `updated_at`
  (`editor_ipc.rs:709-735`).
- Project folder setting: **exists** (`record.project_dir`, `session.rs:178-190,207,237-275`; UI
  `RecordSettingsSection.tsx:46-63`). Changing it hides older projects from the list (no migration,
  no multi-root).
- Missing for "reopen" parity: (1) sort/"recent" by `updated_at` (cheap; summary lacks the field,
  `project.rs:179-186`); (2) search/filter in the panel; (3) list beyond 20 / scroll; (4) rename
  (`title` exists, `project.rs:68`); (5) "Open…" browse for a project folder elsewhere (needs a root-free
  but validated open — `is_project_dir`, `project.rs:191`); (6) an in-editor "Open project" menu
  (⌘O) so switching doesn't need the popup; (7) project thumbnails (filmstrip exists in helper,
  `TAG_THUMB`). Upstream's "open newest on launch" doesn't fit a tray app — skip.

## 6. Per feature: compositor-side or new code, risks, patches

| feature | compositor | new code | vendor patch? | risks |
|---|---|---|---|---|
| Text annotations | Scene `annotations` | doc v3, scene emit, overlay handles, inspector, font assets + env in helper spawn | none (un-ignore font tests) | env must be set before first text draw (`OnceLock`); bundle +2.3 MB; OFL RFN |
| Image annotations | yes | import into project dir, path | none | cache key = id+len (`compositor_macos.rs:2009`) → content-hashed file names |
| Arrow | yes | UI | none | — |
| Blur/mosaic rect/oval | yes | UI + clamps | none | always under cursor/other annotations; screen-space box moves with zoom (overlay mismatch under zoom) |
| Blur freehand | **no** (bbox) | polygon editor | **yes** (mask polygon in mode 10 or a stencil) | unit ambiguity; or drop from scope |
| Text animations | yes (7 incl. none, +300 ms exit) | picker | none | Rust ≠ TS entry-only; we only use Rust |
| Music tracks | `audioTracks` | doc model, programme projection (port `sceneDescription.ts:923-1047`), preview playback | optional (mono unity, voice-keyed ducking) | `trimEndSec` required; loops ≤1000 entries |
| Voiceover | `audioTracks kind voiceover` | native capture to WAV during playback | none | must split around cuts like `takeProgramme`; preview mute other tracks while recording |
| Mic/system gain | `extraAudio.gainDb`, `audio.gainDb` | system gain field (we have none) | **yes** for correct loudness (§2g.1) | current export normalises the wrong track |
| Limiter | already on | preview curve | none | −2 dBTP, not −1.5 |
| Waveform | `decode_clip_audio` | helper cmd + cache + timeline draw | none | decode cost on long takes → background + cache |
| Snapping / reorder / duplicate | n/a | pure TS/Rust | none | clip-index re-keying of speed/crop/zooms/annotations |
| Insert other media | per-clip `screenPath` | `EditClip.source`, meta per source, LiveView | maybe | LiveView created with first clip's paths (`record-render/src/session.rs:354`) |
| Shortcuts | n/a | matcher + settings + dialog | none | S/C conflict with our split/crop |
| Presets | n/a | format, store, UI | none | own-asset references |
| Reopen | n/a | sort, search, rename, browse, ⌘O | none | open without root check |

## 7. Editor/Export inventory items not built by stages 1–3 and not owned by 5–8

Candidates for stage 4 to own (beyond annotations/audio/shortcuts/presets/reopen):
1. **Multi-clip reorder / duplicate / insert** (Editor inventory line 1) — reorder+duplicate cheap;
   insert-other-media heavy (see §3).
2. **Trim track as regions** — upstream has a T "add trim" lane (`trimRanges`); ours trims via split +
   delete + edge drags. Decide whether region-trims are needed (behaviour equivalent).
3. **Snapping 8 px, waveform, copy/paste/cut of regions, Tab-cycle selection.**
4. **Export: VP9** — in the spec (`docs/superpowers/specs/2026-11-01-record-design.md:116`) but
   upstream removed VP9 encode as too slow (`V/src/pipeline_windows.rs:1367-1368`); our dialog offers
   H.264/HEVC (`IR/core/frontend/src/components/editor/ExportDialog.tsx:126-129`). Recommend: drop VP9
   from the spec, explicitly.
5. **Export 24 fps / GIF 15–30 fps / sizes / loop** — present (`ExportDialog.tsx:9-10,30-34,101-157`);
   verify 24 is offered (fps chips at `:120`).
6. **True-peak limiter** — present; spec number to correct (−2 dBTP).
7. **Global/system-audio gain** (`audio.gainDb`) — not exposed today.
8. **"Zuletzt geöffnete Projekte" + Projektordner** — folder done, recents partial (§5).
9. **Recording-side leftovers** (not editor but unowned): Speicherplatzprüfung exists
   (`ERR_DISK_FULL`, `session.rs:57`); notes/teleprompter exists (`RecordNotes.tsx`). Per-device mic gain
   at record time (spec "Gain je Gerät") is not built — upstream has none either (fixed 1.4 boost) →
   recommend dropping or folding into the editor mic gain.
Owned elsewhere (do not take): webcam/layout lane/C=layout (5), captions/transcript cut/auto-caption
annotations (6), 3D zoom/frame/cursor + shader effects + depth of field on tilted (7), MCP/CLI (8).
