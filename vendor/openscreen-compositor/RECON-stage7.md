# Stage 7 recon — 3D zoom camera, 3D cursor, device frames, shader effects

Read-only; nothing in the clone was executed. Prefixes: `V/` = `IR/vendor/openscreen-compositor/`,
`C/` = `<scratchpad>/openscreen-src/new/` (untrusted upstream clone), `IR/` = repo root.
Upstream design docs: `C/docs/3d-effects-v2.md` (camera A, cursor B, frames C), `C/docs/shader-effects.md`.
Where docs and code disagree, code wins.

## 0. Summary

1. **V/ already renders all four features, on Metal, with no new native work.** Camera presets
   `left`/`right`/`orbit` (`V/src/regions.rs:540-560,657-662`, `V/src/camera.rs`), 3D device frames
   `laptop`/`phone`/`monitor` (`V/src/scene.rs:368-411`, mode 17), 3D cursor (`scene.rs:768-772`,
   mode 15), and runtime shader effects (`V/src/shader_fx.rs`, naga 24 → MSL). All are
   procedural; **no runtime assets** except the cursor sprites IR already bundles.
2. **IR already emits the "off" values of every stage-7 key**, so goldens are safe: `"rotation": null`
   (`IR/core/rust-lib/src/record/scene_json.rs:364`), `"model3d": false` (`:392`),
   `"depthOfField": false` (`:211`). The header comment says so (`:9-10`). Stage 7 = make these
   values data-driven; no key needs to appear/disappear for the existing goldens.
3. **Device frames are already shipped in IR** (stage 3): `Frame::{Laptop,Phone,Monitor}` +
   `frameTheme` are in the document (`editor.rs:191-206`) and emitted (`scene_json.rs:192-219`).
   Stage 7 "3D device frame" is therefore **UI/verification only** (confirm the picker offers all
   three; they render via mode 17 already).
4. **Upstream ships NO built-in shader effects.** `shaderEffects` are agent-written WGSL stored in
   the document (`C/src/lib/ai-edition/schema/index.ts:538-634`); there is no "add effect" button
   (`C/docs/shader-effects.md:667-671`, decision K.4) and the effect library is explicitly out of
   scope (`:J`). "Built-in effects only" therefore means **IR must curate its own small catalogue of
   WGSL snippets** written against contract v1 and feed them through the existing
   `shaderEffects` scene path. This is the only genuinely new design work.
5. Build: `naga` with `wgsl-in, msl-out` is already a dependency of V/ and compiled
   (`V/Cargo.toml:~48`); `ort` irrelevant. No Cargo/bundle change expected.

## 1. What V/ renders

### 1.1 3D zoom / camera tilt / orbit
- Scene field: `zoomRegions[i].rotation: string|null` — `"left" | "right" | "orbit"` (legacy `"iso"` reads
  as `left`; unknown/null → flat). Parsed `V/src/scene.rs:634-637` (`Option<String>`), next to
  `scale, focusX, focusY, focusMode ("manual"|"auto"|null), hideCursor, underTrim, clipIndex, id`
  (`scene.rs:608-651`).
- Fixed presets, degrees (rotX, rotY, rotZ): `left`/`iso` = **[-12, -18, -2]**, `right` = **[-12, 18, 2]**
  (`V/src/regions.rs:657-662`). Perspective = `min(w,h) × 1.6` (`PERSPECTIVE_FACTOR`, `regions.rs:1247`);
  warp is bilinear for fixed presets, projective homography for orbit (`camera.rs` header; tests
  `compositor_macos.rs:5349`).
- Dynamic extras on fixed presets: parallax from cursor velocity + click-impact tilt, shared budget
  `DYNAMIC_TILT_BUDGET = [1.9, 3.0, 0.0]` deg (`regions.rs:1444`), gated by `tilt_gate` (`:1529`).
  Click impact is `cursor.clickImpact` (IR already has it: `editor.rs:278`, `scene_json.rs:397-399`).
- Orbit: real camera orbiting an immobile screen, no roll. Azimuth ±22° with pointer x, elevation
  rest 4° ± 12° with pointer y (`camera.rs:33-37`), dolly `zoom^0.5` (`:42`), full-view→aim crossfade
  to zoom 2.0 (`:45`), click press 0.04 (`:47`). Pointer-driven in `focusMode:"auto"` (smoothed
  cursor, `camera::follow`, `:280-330`), focus-point-driven in manual (`camera::fixed`, `:311`).
  **Orbit needs cursor telemetry in auto mode** — upstream hides the option when the zoom has no
  cursor data (`C/src/components/ai-edition/v4/FloatingInspector.tsx:868-880`).
- Depth of field: `effects.depthOfField: bool`, `#[serde(default = true)]` (`scene.rs:358-361`),
  defocuses the far side of a tilted screen (mode 8); no effect without a tilted zoom.
- Fixed presets use `computeRotation3DContainScale` port so the tilted quad stays inside the frame
  (`regions.rs:1136-1140`). Edges must stay ≥ 2° off-axis (test `no_preset_has_an_axis_aligned_edge`).

### 1.2 3D device frame
- `effects.frame`: `"window"` (flat chrome, mode 14) | `"laptop"|"phone"|"monitor"` (modelled 3D, mode 17:
  body with thickness, bevel, bezel; screen face exactly on the footage plane) | legacy
  `"window-light"/"window-dark"`; unknown → none (`scene.rs:368-411`, `SceneFrame`).
- `effects.frameTheme`: `"light"` (default) | `"dark"` (`scene.rs:414-451`). Key omitted when none.
- Layout makes room for the frame (`C/docs/3d-effects-v2.md` C.2); IR's `layout.rs`/`composite_layout`
  already receives `a.frame` (`scene_json.rs:99`). Corner-radius ceiling per frame (C.5) is upstream
  UI math — check IR's slider range if it shows a %-of-frame-range (`effects.roundnessFrameHelp`).
- Frames are procedural geometry (`prism_mesh.rs`, shaders); no model files.

### 1.3 3D cursor
- `cursor.model3d: bool` (`scene.rs:768-772`, `serde(default)`) → mode 15 raymarch. Sprites with a
  `sculpt` name are modelled; other states are the sprite **extruded**.
- `cursor.cursorSprites[state].sculpt: "<theme>/<state>"` and `.glass: "<theme>/<state>"`
  (`scene.rs:807-815`, both `Option<String>`, `serde(default)`).
- Sculpted set (`V/src/sculpt.rs:54,119-140`): themes `studio-ink, prism-glow, pop-coral, pixel-candy,
  star-sprout, glass-lens`, states `arrow` and `pointer` only. Anything else → extrude.
- `glass` is independent of `model3d`: **glass-lens draws as a glass lens over the image even with 3D
  off** (`scene.rs:811-814`, mode 19). Upstream sets `glass` on both glass-lens states
  (`C/src/lib/cursor/cursorThemes.ts:264-283`, `:355-356`).
- `clickImpact` relation: see 1.1. Sculpt/prism meshes are code (`sculpt.rs` 355 loc, `prism_mesh.rs`
  876 loc), no data files.

### 1.4 Shader effects
- Pipeline: user WGSL → `shader_fx::analyze` (literal loop bounds, 4096 iter/pixel cap, no
  `@group/@binding`) → naga 24 `wgsl-in` → `msl-out` (`V/src/shader_fx.rs:333,690`) → Metal
  `new_library_with_source` on a background thread (`spawn_compile` `:774`, `compile_metal` `:753`).
  One extra full-screen pass after composition, **same code path for preview and export**
  (`compose_frame`; `docs/shader-effects.md` A.1, D.1). One effect at a time (K.6).
- Preview vs export: `Compositor::set_shader_fx_blocking(false)` for preview (draw un-effected until
  the compile lands, `shader_fx_landed()` recompose) and blocking for export
  (`V/src/compositor_macos.rs:1004-1050`). Compile cache keyed by `source_hash` with LRU eviction
  (`shader_fx.rs:790,804`). Failed compile → effect skipped, error reported.
- Scene field `shaderEffects: []` (`scene.rs:1008-1010`), each `SceneShaderEffect` (`:688-715`):
  `clipIndex?, startSec, endSec` (clip source seconds), `programmeStartSec?/programmeEndSec?` (whole
  pill in programme time → `ctx.region`), `target: "image"|"text"|"both"` (default image), `contract: 1`,
  `code`, `params: [{name,type: number|toggle|color|point}]` (order = `FxParams` struct), `values: {name:
  number | [f32;2|4]}` (already normalised: color → 4 floats, toggle → 0/1, point → 2 floats,
  default filled in) (`:706-718`, `ShaderParamValue::Scalar|Vector`). Fragments clipped/retained by
  `belongs()` at `scene.rs:1063`.
- Contract v1 prelude (`shader_fx.rs:25-180`): `frame(uv)`, `ctx.{resolution,aspect,time,region,cursor,
  clicks[4],footage,camera,footage_aspect}`, `fx_to_footage/fx_from_footage/footage(p)`, `params.<name>`,
  `fx_px(n)`, `fx_hash(p)`. Limits: `MAX_CLICKS=4`, `CLICK_WINDOW_S=2.0`, `MAX_ITERATIONS_PER_PIXEL=4096`,
  `PARAMS_BUFFER_BYTES=32*16`, ≤ 32 params (`:182-189`).
- Other "effects" in the Scene that are NOT the WGSL path: `effects.{padding,blur,shadow,roundnessFrac,
  motionBlur}`, `webcamEffect`, wallpaper `motion`, `clickImpact`. Already handled by earlier stages.

## 2. What upstream's document stores

### 2.1 Camera
- Per zoom: `ZoomRange.rotationPreset?: "left"|"right"|"orbit"` (schema input also accepts `iso`, transformed to
  `left`) — `C/src/lib/ai-edition/schema/index.ts:523-526`. Absent = flat. **Per zoom, not global.**
  `follow-cursor` (old) reads as `orbit` + auto focus (`readFollowCursorAsAutoOrbit`, schema doc `:520-522`).
- Alongside: `depth` 1..6, `customScale`, `focus{cx,cy}`, `focusMode`, `hideCursor` (`:500-527`).
- UI constants: `FIXED_ROTATION_3D_PRESETS=["left","right"]`, `MOVING_ROTATION_3D_PRESETS=["orbit"]`,
  order orbit,left,right (`C/src/components/video-editor/types.ts:81-100`).
- Scene mapping: `zoomRegions[].rotation = region.rotationPreset ?? null`
  (`C/src/native/sceneDescription.ts:1564`; type `:108`).
- Global setting `depthOfField` (default **true**, `C/src/lib/ai-edition/store/editorSettings.ts:107,295`)
  → `effects.depthOfField` (`sceneDescription.ts:1508`). Only shown when some zoom has a rotation
  (`RightPanes.tsx:2391,2655-2664`). Click impact is a cursor setting (not per zoom).

### 2.2 Device frame
- Settings `frame: none|window|laptop|phone|monitor` and `frameTheme: light|dark`
  (`sceneDescription.ts:1505-1507`: `frame` omitted when none, `frameTheme` omitted when `light`).
  Matches IR exactly.

### 2.3 Cursor
- `settings.cursor.model3d: boolean` (default off) (`sceneDescription.ts:1517`), grey-out when cursor hidden.
- Per-state sculpt/glass strings are built in `C/src/lib/cursor/cursorThemes.ts:350-356`:
  `...(model3d && asset.sculpted ? { sculpt: "${themeId}/${type}" } : {})`,
  `...(asset.glass ? { glass: "${themeId}/${type}" } : {})`. Assets flagged `sculpted` (arrow+pointer of the five
  non-default themes + glass-lens, `cursorThemes.ts:160-285`) and `glass` (glass-lens arrow+pointer only).
  **Note:** `sculpt` is only emitted when `model3d` is on; `glass` always.

### 2.4 Shader effects
- Definitions `shaderEffects[]` {id, name, description, contract, code ≤ 65 536, params ≤ 32 (number{default,min,max}
  | toggle | color #rrggbb[aa] | point{x,y}), origin agent|user, cost?} and ranges `shaderEffectRanges[]` {id,
  startMs, endMs, clip anchor, effectId, values (only overrides), target} — `schema/index.ts:538-634,772-773`.
  Additive (no schema bump), one effect at a time (timeline lane rule).
- Scene (`docs/shader-effects.md` B.5, `sceneDescription.ts`): per fragment `clipIndex,startSec,endSec,
  programmeStartSec,programmeEndSec,target,contract,code,params:[{name,type}],values` as in 1.4.
- **No built-ins upstream.** Effects are authored by the agent/MCP only.

## 3. Upstream UI (brief)
- **3D camera**: zoom inspector (`FloatingInspector.tsx:860-910`), tile row "3D camera": Off, 3D Orbit (only when
  the zoom has cursor data + cursor shown, or manual focus), Screen turned left, Screen turned right; each tile
  has a 32×22 SVG icon (`CAMERA_ICONS`). Strings `zoom.camera.*` (`en/settings.json:370-377`).
- **Depth of field**: switch in Effects pane, shown only if any zoom is tilted (`RightPanes.tsx:2655`;
  "Blurs the far side of a tilted screen").
- **Frame + Theme**: Effects pane button rows (`RightPanes.tsx:2523-2565`).
- **3D cursor / click impact**: Cursor pane switches (`RightPanes.tsx:3784-3791`; "Draws the default cursor as a 3D
  object with a shadow"; "Click impact — The screen moves a little with each click").
- **Shader effects**: timeline "Effects" lane (only if the project has any), pill → inspector with one control per
  declared param, target (Image/Texts/Both), cost tier, read-only code, delete (`docs/shader-effects.md` F.1-F.2).
- No dedicated keyboard shortcuts for any of these (only the generic timeline ones).

## 4. What IR emits / lacks today
| Item | IR now | Gap |
|---|---|---|
| zoom `rotation` | always `null` (`scene_json.rs:364`) | `ZoomRegion` (`editor.rs:326-345`) has no preset field |
| `effects.depthOfField` | always `false` (`:211`; upstream default true) | no setting |
| `cursor.model3d` | always `false` (`:392`) | no setting in `CursorStyle` (`editor.rs:269-281`) |
| `cursorSprites[].sculpt/glass` | never emitted (`cursor_sprites` `:224-243`) | needs per-theme tables; the six theme dirs are bundled (`core/rust-lib/assets/record/cursors/{default,glass-lens,pixel-candy,pop-coral,prism-glow,star-sprout,studio-ink}`); `themes.json` has hotspots only (checked first 700 B) |
| `effects.frame/frameTheme` | emitted (`:213-217`) | none — shipped; verify UI exposes laptop/phone/monitor |
| `shaderEffects` | not emitted at all (key absent) | no model, no catalogue |
| editor doc version | `EDITOR_VERSION = 5` (`editor.rs:15`) | new fields must be `#[serde(default)]` additive (upstream K.1 does the same); decide whether to bump to 6 (IR bumps per stage: v2..v5) |

**Golden safety.** `scene-golden*.json` (`IR/core/rust-lib/src/record/fixtures/`) already contain
`"rotation": null`, `"model3d": false`, `"depthOfField": false` (asserted in tests `scene_json.rs:882-883,907-924`).
So: emit these exactly as now when the features are off.
**Emit-only-when-set keys (must NOT appear in goldens):** `zoomRegions[].rotation` stays `null` (present), but
`cursorSprites[].sculpt` and `.glass` (absent today) and the whole top-level `shaderEffects` must be omitted
when empty; `effects.depthOfField` stays `false` unless a zoom is tilted AND the user keeps the setting on — if
IR keeps its global default `false`, nothing changes; if IR adopts upstream default `true`, the **golden changes**
(`"depthOfField": true` only matters when a tilted zoom exists, but the key value is serialised), so either emit
`true` only when any zoom has a preset (recommended; mirrors upstream UI gating) or accept a golden update.
`glass` for `glass-lens` would change `scene-golden-styled.json` only if that fixture uses that theme (check
before generating); gate it the same way (only emit when theme is glass-lens).

## 5. Build / bundle implications
- `V/Cargo.toml:~48`: `naga = { version = "24", features = ["wgsl-in", "hlsl-out", "msl-out"] }` is an unconditional
  dependency on Apple-Silicon; `metal 0.29`, `objc`, `block`, `core-foundation` present. IR's helper links V/ with
  `default-features = false` (`IR/record-render/Cargo.toml`), which only drops `segmentation` (ort); the shader and 3D
  paths are not feature-gated, so **no Cargo change**. No `wgpu` anywhere on the Mac path (wgpu/WGSL files
  `vk_shaders/*.wgsl` are Linux-only and merely `include_str!`'d by tests).
- Metal shaders compile **at runtime** from `shaders.metal` (4175 lines; with `#define LAYER_MODELS 1` for the 3D
  model pipelines, `compositor_macos.rs:679-682,790`). The 3D pipelines are built eagerly or lazily inside V/; first
  open pays the MSL compile — measure helper start latency (stage 5 already did for segmentation).
- Assets: none new. Cursor PNG themes already in `core/rust-lib/assets/record/cursors/`. Sculpt/prism/frame geometry
  is Rust/MSL code. Licences: V/ is MIT (OpenScreen) and its cursor art is MIT (`VENDORED.md` "Copied"); the five
  themed PNG sets were transcribed with the themes (`themes.json` comment). `THIRD-PARTY-NOTICES.md` already ships.
  The IR-authored WGSL effect snippets are ours (note the contract prelude is upstream MIT code, already vendored).
- Optional new vendor patch only if IR wants `check_for_agent`-style validation of its own snippets at build time;
  `shader_fx::check_for_agent` exists (`:970`) and can be used in unit tests with no patch.

## 6. Risks
1. **GPU-only paths fail headless.** All 3D/shader tests in V/ need a Metal device; CI (Linux) builds V/ as an empty
   lib, so IR-side verification is: (a) pure Rust tests on `scene_json` output (field values), (b) `#[ignore]` live
   tests on Apple Silicon via the helper (`IR_RECORD_PROJECT=… helper_live`), (c) manual visual check of preview vs
   export. Plan an `#[ignore]` render test per feature rendering a frame and asserting non-flat pixels.
2. **Orbit semantic coupling**: auto focus orbit needs `cursor.json`; IR must hide/disable it when the take has no
   pointer data (same rule as upstream) or the camera silently holds still. Also `hideCursor`/cursor-off disables it.
3. **Compile latency / failures** of shader effects: preview must run `set_shader_fx_blocking(false)`; check how IR's
   helper sets it (`record-render/src/session.rs` — not inspected in detail; verify grep for `set_shader_fx_blocking`
   in IR, none found in my grep of `record-render/src`). Export must be blocking or the first frames render
   un-effected — verify the helper's export path.
4. **Performance**: upstream measured +0.04 ms/frame (1080p) for a heavy effect on D3D11 and the M1 is documented
   (H.3); tilted warp + DOF + 3D cursor + frame model together is the real cost — measure preview fps on the M1 Pro.
5. **Version/forward-compat**: a v6 document opened by an older build loses stage-7 fields (same accepted trade-off as
   upstream K.1). Keep fields `#[serde(default)]` and skip-serialise when default so v5 round-trips byte-identically.
6. **Sprite-name coupling**: `sculpt` strings must equal entries of `sculpt::sculpted_shape` (`"<theme>/arrow|pointer"`
   for the six themes); a typo silently falls back to extrusion. Table-drive from one list and test it against V's
   `sculpted_shape`.
7. **Effect catalogue scope**: because upstream has none, the "1:1 port" claim does not apply; we define our own
   effects (names, params, ranges) — contract rule "pure, loops with literal bounds, resolution independent" must hold
   (`docs/shader-effects.md` C.4) or `analyze` rejects them. Cursor/click inputs (`ctx.clicks`) require pointer data.
8. **Text target** (`target:"text"`/`both`) renders the annotation layer separately; captions are annotations (stage 6),
   so effects on `both` change captions. Default `image`.

## 7. Recommended task breakdown
1. **Document v6 + scene wiring (Rust core, TDD).** `ZoomRegion.camera: Option<CameraPreset{Left,Right,Orbit}>`
   (serde default, skip when none), `Appearance`/global `depth_of_field: bool` (default on, emitted `true` only when
   some zoom has a preset), `CursorStyle.model3d: bool`; v5→v6 migration is identity (version bump only). `scene_json`:
   `rotation` from the preset, `depthOfField`, `model3d`, and `sculpt`/`glass` strings from a single `SCULPTED` table
   (arrow/pointer of the 6 themes; `glass` for glass-lens). Tests: goldens unchanged with features off; new goldens
   for on; `sculpt` names accepted by `openscreen_compositor::sculpt::sculpted_shape` (macOS-only test).
2. **Editor ops + TS mirror.** `set_zoom_camera`, normalise (orbit allowed only with pointer data or manual focus;
   reset otherwise), `editor.ts` types/`view` fields (`zoom.camera`, `has_tilted_zoom` for DOF visibility); copy/paste
   of zooms keeps the preset (check `clipboard-regions.ts`); presets (`presets.rs`) carry model3d/depthOfField as global
   look.
3. **UI: zoom inspector "3D camera" tile row** (Off/Orbit/Left/Right with SVG icons), Cursor tab "3D cursor" switch
   (disabled when cursor hidden), Effects: depth-of-field switch (visible only with a tilted zoom), confirm Frame/Theme
   pickers expose laptop/phone/monitor; i18n en+de (+ strings in `locales`).
4. **Shader-effect model + catalogue.** `editor.rs`: `effects: Vec<EffectRegion{id, clip, start_ms, end_ms, kind,
   values}>` (one at a time, no overlap — like upstream K.6) with a Rust built-in registry `BUILTIN_EFFECTS` (id, name,
   WGSL source, params with ranges/defaults). Suggested small set written against contract v1: vignette, film grain
   (`fx_hash`), chromatic aberration, click ripple (`ctx.clicks`), spotlight on cursor, glitch/scanline. Validate each
   with `shader_fx::check_for_agent` in a unit test (macOS). `scene_json` emits `shaderEffects` (normalised `values`,
   `programmeStart/EndSec`, `target:"image"`) only when non-empty.
5. **Effect lane + inspector UI** (timeline lane, add menu listing the catalogue, per-param controls, delete, snap;
   auto-hide lane when empty; undo via existing rustEdit path). Confirm preview non-blocking / export blocking compile
   in the helper.
6. **Verification pass on Apple Silicon**: `#[ignore]` helper_live renders for each camera/frame/cursor/effect,
   preview-vs-export parity check, fps measurement, vendor-note + CLAUDE.md/docs/features.txt/CHANGELOG updates.
   (Optionally fold 6 into 3 and 5 as acceptance steps → 5 tasks.)
