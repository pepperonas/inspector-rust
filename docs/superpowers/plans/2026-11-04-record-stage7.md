# record — Stage 7: 3D camera, 3D cursor, depth of field, shader effects — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the stage-7 features the vendored compositor already renders (tilted/orbit zoom camera, depth of field, 3D cursor, glass cursor, shader effects) editable in the record editor, plus a small catalogue of built-in effects.

**Architecture:** Editor document v6 adds per-zoom `rotation`, global `depth_of_field`, cursor `model3d` and a list of effect regions. `scene_json.rs` turns them into the Scene keys the compositor already parses (`zoomRegions[].rotation`, `effects.depthOfField`, `cursor.model3d`, `cursorSprites[].sculpt/glass`, `shaderEffects`). The effect code is our own WGSL written against the compositor's contract v1, shipped as files and validated in the helper crate's tests (the tray app never links the compositor). UI lives in the existing Zoom/Cursor/Look tabs plus a new Effects tab and timeline lane.

**Tech Stack:** Rust (core + record-render), React 19 + TS, Vitest, vendored `openscreen-compositor` (naga 24 → MSL).

**Spec:** `docs/superpowers/specs/2026-11-01-record-design.md` (stage 7). Recon: `vendor/openscreen-compositor/RECON-stage7.md` — read §1 and §4 before any task.

## Global Constraints

- Golden fixtures `core/rust-lib/src/record/fixtures/scene-golden*.json` stay byte-identical when no stage-7 feature is used. Keys already present stay as they are (`"rotation": null`, `"model3d": false`, `"depthOfField": false`); `cursorSprites[].sculpt`, `cursorSprites[].glass` and top-level `shaderEffects` are emitted ONLY when set.
- `EDITOR_VERSION` = 6. A v5 document opens unchanged (identity migration, version bump only). New fields are `#[serde(default)]`.
- The tray app (`core/rust-lib`) never links the compositor or ffmpeg. Anything that needs `openscreen_compositor::*` (sculpt names, `shader_fx::check_for_agent`) is tested in `record-render`.
- R12: programme↔source time mapping happens only in Rust (`editor_to_source` / `editor_to_programme`, `EditorView` spans). TS never derives programme time.
- Every user-visible string in `core/frontend/src/locales/en.json` AND `de.json`. Never `window.confirm`. Follow `.claude/rules/animation.md` (no `transition-all`, tokens only).
- Rotation presets: exactly `left`, `right`, `orbit` (strings as the compositor parses them). Sculpted themes: `studio-ink`, `prism-glow`, `pop-coral`, `pixel-candy`, `star-sprout`, `glass-lens`; sculpted states `arrow`, `pointer`; `glass` only for `glass-lens` arrow/pointer.
- One shader effect at a time on the programme timeline (no overlapping effect regions); target always `"image"`.
- Upstream ships no built-in effects; the catalogue is ours (the spec forbids AI-written effects and user WGSL files).

## Review Focus

1. **Cuts and clip operations carry effect regions** like zooms: split at S, transcript cut, restore gap, move/duplicate clip, delete clip, insert take — an effect spanning a split must end up in both halves, never vanish or double. Tests in Task 3.
2. **Orbit without pointer data** in auto focus holds the camera still silently. The UI must not offer Orbit for a zoom whose clip has no pointer data unless its focus is manual; a stored orbit on such a zoom keeps working (no data loss). Test in Task 4.
3. **Preview equals export** for model3d/depth of field: the LiveView takes `cursorModel3d` as a live param (already mapped in `record-render/src/session.rs::live_params`) — pin that a model3d scene pushes `cursorModel3d=true`. Test in Task 1.
4. **Effect values out of range / unknown effect id** (hand-edited or older catalogue): normalise clamps values into the param's range, drops unknown param names, drops regions whose effect id is unknown. Test in Task 2.
5. **WGSL that the compositor rejects** would silently skip the effect: every catalogue effect must pass `shader_fx::check_for_agent` with its declared params. Test in Task 2.

---

### Task 1: Document v6 camera/cursor fields and scene wiring

**Files:**
- Modify: `core/rust-lib/src/record/editor.rs` (`EDITOR_VERSION`, `ZoomRegion`, `Appearance`, `CursorStyle`, migration, normalize)
- Modify: `core/rust-lib/src/record/scene_json.rs` (rotation, depthOfField, model3d, sculpt/glass)
- Create: `core/rust-lib/src/record/fixtures/scene-golden-3d.json`
- Modify: `record-render/src/session.rs` (tests only)
- Modify: `core/frontend/src/lib/editor.ts` (types + defaults mirror, pinned like stage 6)

**Interfaces:**
- Produces (Rust, serde camelCase):
  - `pub enum ZoomRotation { Left, Right, Orbit }` (serde lowercase), `ZoomRegion.rotation: Option<ZoomRotation>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`).
  - `Appearance.depth_of_field: bool`, default `true`.
  - `CursorStyle.model3d: bool`, default `false`.
  - `pub const SCULPTED_THEMES: [&str; 6]` and `pub const SCULPTED_STATES: [&str; 2]` in `scene_json.rs`; `pub fn sculpt_name(theme: &str, state: &str, model3d: bool) -> Option<String>`; `pub fn glass_name(theme: &str, state: &str) -> Option<String>`.
- Produces (TS): `RecordZoomRegion.rotation?: "left" | "right" | "orbit" | null`, `appearance.depthOfField: boolean`, `cursor.model3d: boolean`, defaults mirrored.

- [ ] **Step 1: Write failing Rust tests** in `scene_json.rs` / `editor.rs` test modules:
  - `v5_document_opens_as_v6_unchanged` — a stored v5 doc (no new keys) loads with `rotation: None`, `depth_of_field: true`, `model3d: false`, version 6.
  - `rotation_presets_reach_the_scene` — zooms with `Left/Right/Orbit/None` emit `"left"`, `"right"`, `"orbit"`, `null`.
  - `depth_of_field_is_emitted_only_with_a_tilted_zoom` — `depth_of_field: true` + no rotated zoom → `false`; + one rotated zoom → `true`; `depth_of_field: false` + rotated zoom → `false`.
  - `model3d_reaches_the_scene` and `sculpt_is_emitted_only_for_sculpted_states_with_model3d` — theme `studio-ink`, model3d on: arrow/pointer sprites carry `"sculpt": "studio-ink/arrow"` / `"studio-ink/pointer"`, other states have no `sculpt` key; model3d off: no `sculpt` anywhere; theme `default`: never.
  - `glass_lens_carries_glass_with_or_without_model3d` — theme `glass-lens`: arrow/pointer have `"glass": "glass-lens/<state>"` regardless of model3d; other themes never.
  - `goldens_are_unchanged_without_stage7_features` — the existing golden tests keep passing untouched (run them; if `scene-golden-styled.json` uses `glass-lens`, record a ruling and regenerate that one golden — the `glass` key is correct upstream behaviour).
  - `stage7_golden` — a doc with a left zoom, orbit zoom, model3d on, theme `studio-ink` matches `fixtures/scene-golden-3d.json` (generate once, then pin).
- [ ] **Step 2: Run** `cargo test -p inspector-rust-core --lib record::` — expect the new tests to FAIL.
- [ ] **Step 3: Implement** the fields, migration (v5 → v6: set version, nothing else), and emission. Sprite state names come from the existing `cursor_sprites` function in `scene_json.rs`.
- [ ] **Step 4: record-render tests** (macOS, needs ffmpeg tree): `every_sculpt_name_is_known_to_the_compositor` — for each theme × state, `openscreen_compositor::sculpt::sculpted_shape(&format!("{t}/{s}")).is_some()`; `a_model3d_scene_pushes_cursor_model3d_to_the_preview` — `live_params` on a scene with `cursor.model3d: true` contains `("cursorModel3d", true)`. Run: `MAC_FFMPEG_DIR=third_party/ffmpeg/8.1.2 cargo test -p record-render --lib`.
- [ ] **Step 5: TS mirror** — extend types + defaults in `lib/editor.ts`; extend the existing pin test that compares TS defaults with Rust (find it with `grep -rn "EDITOR_VERSION\|DEFAULT_CURSOR" core/frontend/src/lib/*.test.ts`). Run `pnpm --filter inspector-rust-frontend exec vitest run src/lib/editor`.
- [ ] **Step 6: Mutation-check** two pins (drop the tilted-zoom gate in depthOfField; emit sculpt without model3d) — both must go red. Then commit `feat(record): 3D camera presets, depth of field and 3D cursor in the scene`.

### Task 2: Built-in shader effects — model, catalogue, scene emission

**Files:**
- Create: `core/rust-lib/assets/record/effects/{vignette,grain,chromatic,ripple,spotlight,scanlines}.wgsl` + `core/rust-lib/assets/record/effects/README.md` (one line per effect: what it does, params)
- Create: `core/rust-lib/src/record/effects.rs` (registry, normalisation)
- Modify: `core/rust-lib/src/record/mod.rs`, `editor.rs` (`effects` field, normalize, clip ops), `scene_json.rs` (`shaderEffects`)
- Test: `record-render/tests/effects_compile.rs` (or a `#[cfg(test)]` module in `record-render/src`)

**Interfaces:**
- Produces (Rust):
  - `pub enum EffectParamKind { Number { min: f64, max: f64, default: f64 }, Toggle { default: bool }, Color { default: &'static str } }`
  - `pub struct EffectParam { pub name: &'static str, pub kind: EffectParamKind }`
  - `pub struct BuiltinEffect { pub id: &'static str, pub code: &'static str, pub params: &'static [EffectParam], pub needs_pointer: bool }`
  - `pub static BUILTIN_EFFECTS: &[BuiltinEffect]` (ids exactly: `vignette`, `grain`, `chromatic`, `ripple`, `spotlight`, `scanlines`; `code: include_str!(…)`), `pub fn builtin(id: &str) -> Option<&'static BuiltinEffect>`.
  - `pub enum EffectValue { Number(f64), Toggle(bool), Color(String) }` (serde untagged) and `pub struct EffectRegion { id: String, clip: usize, start_ms: u64, end_ms: u64, effect: String, values: BTreeMap<String, EffectValue> }` (camelCase); `EditDoc.effects: Vec<EffectRegion>` (`#[serde(default, deserialize_with = "lenient_vec")]`).
  - `pub fn normalize_effects(regions: Vec<EffectRegion>, …) -> Vec<EffectRegion>` used by `editor::normalize`.
  - `EditorView.effects: Vec<EffectSpan>` (`{ id, effect, programme_start_ms, programme_end_ms, row }`) computed in Rust like zoom spans.
- Contract source: read the prelude at the top of `vendor/openscreen-compositor/src/shader_fx.rs` (lines ~25-190) and `analyze`'s rules (`:333`): no `@group/@binding`, loops with literal bounds, ≤ 4096 iterations per pixel, ≤ 32 params, resolution-independent (`fx_px`). `ripple` uses `ctx.clicks`, `spotlight` uses `ctx.cursor` (`needs_pointer: true` for both).

- [ ] **Step 1: Write failing tests** (core):
  - `catalogue_ids_are_unique_and_known` — six ids exactly as above; each has ≤ 32 params, unique param names, number defaults inside their range.
  - `effects_without_overlap` — two regions overlapping in programme time: the later one in the list wins, the earlier is cut to end where the later starts (dropped if < `MIN_CLIP_MS`).
  - `unknown_effect_or_param_is_dropped_and_values_clamped` — unknown effect id → region dropped; unknown value name → removed; `Number` above max → max; wrong type → default.
  - `effect_regions_follow_clip_operations` — on `move_clip`, `duplicate_clip`, `append_take` and clip deletion the effect regions behave exactly like zoom regions (find every place `zooms` is re-indexed in `editor.rs` and do the same for `effects`).
  - `shader_effects_absent_without_regions` (goldens unchanged) and `shader_effects_fragment` — one vignette region emits one `shaderEffects` entry with `clipIndex`, `startSec`/`endSec` (source s), `programmeStartSec`/`programmeEndSec`, `"target": "image"`, `"contract": 1`, `code`, `params: [{name,type}]`, `values` with every param filled (colour as 4 floats 0..1, toggle 0/1). Field names per recon §1.4 / `V/src/scene.rs:688-718`.
- [ ] **Step 2: Run** core tests, expect FAIL.
- [ ] **Step 3: Write the six WGSL effects** and implement registry, normalisation, emission. Keep each effect small (≤ 60 lines) and visually subtle by default.
- [ ] **Step 4: record-render test** `every_builtin_effect_compiles` — for each entry of a table `[(path, params_json)]` built from the same `.wgsl` files (`include_str!("../../core/rust-lib/assets/record/effects/<id>.wgsl")`) call `openscreen_compositor::shader_fx::check_for_agent(code, &params)` and assert `Ok`. Params are declared in the test from a JSON string that must match the Rust registry — add a core test `catalogue_params_json` that prints/compares the same JSON (store it in `assets/record/effects/params.json` and have both crates read it, so there is ONE declaration). Run with `MAC_FFMPEG_DIR=…`.
- [ ] **Step 5: Mutation-check** (break a loop bound in one WGSL file → compile test red; skip clamping → value test red). Commit `feat(record): built-in shader effects`.

### Task 3: TS mirror and editor operations for rotation and effects

**Files:**
- Modify: `core/frontend/src/lib/editor.ts`, `lib/clipboard-regions.ts`, `lib/ipc.ts` (types), `lib/timeline-snap.ts` if region kinds are listed there
- Create: `core/frontend/src/lib/effects.ts` (catalogue mirror for the UI: ids, param kinds, defaults — read from `assets/record/effects/params.json` via import so it is not a copy)
- Test: `lib/editor.test.ts`, `lib/effects.test.ts`, `lib/clipboard-regions.test.ts`

**Interfaces:**
- Consumes: Task 1/2 Rust types (camelCase JSON).
- Produces: `setZoomRotation(doc, zoomId, rotation | null)`, `addEffect(doc, clip, startMs, endMs, effectId)` (fresh id, values = defaults), `setEffectValue(doc, regionId, name, value)`, `moveEffect(doc, regionId, clip, startMs, endMs)`, `deleteEffect(doc, regionId)`; `splitAt`, `cutSourceRange`, `restoreGap` handle `effects` exactly like `zooms` (spanning region in both halves, rejoin on restore); clipboard copy/cut/paste for effect regions.

- [ ] **Step 1: Failing tests** — `splitAt` with an effect spanning the cut → two regions, second with fresh id; `restoreGap` rejoins them; `cutSourceRange` keeps an effect outside the cut and shortens one crossing it; clipboard paste of an effect lands at the playhead with a new id; `setZoomRotation` keeps the zoom otherwise unchanged and is one doc change; the TS catalogue equals the Rust one (`effects.test.ts` imports `params.json` and checks ids/kinds/defaults against `catalogue_ids_are_unique_and_known`'s list).
- [ ] **Step 2: Run** `pnpm --filter inspector-rust-frontend exec vitest run src/lib` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, then mutation-check** the split-in-both-halves rule. Commit `feat(record): editor operations for 3D camera and effects`.

### Task 4: Editor UI — 3D camera, 3D cursor, depth of field, Effects tab and lane

**Files:**
- Modify: `components/editor/Inspector.tsx` (+ the Zoom section, Cursor section, Look/appearance section — find them in `Inspector.tsx`/`InspectorParts.tsx`)
- Create: `components/editor/EffectsTab.tsx`, `components/editor/EffectsTab.test.tsx`
- Modify: `components/editor/Timeline.tsx` (effects lane, only when the project has effects; blocks from `EditorView.effects`), `Timeline.test.tsx`
- Modify: `locales/en.json`, `locales/de.json`, `src/mockup/backend.ts` (invented data showing one tilted zoom and one effect)

**Interfaces:**
- Consumes: Task 3 operations; `EditorView.effects`, `EditorView.click_hits` / clip pointer data (to decide whether Orbit is offered).

- [ ] **Step 1: Failing component tests:**
  - Zoom section shows a "3D camera" row: Off · Orbit · Turned left · Turned right; choosing one calls `setZoomRotation`; Orbit is hidden when the zoom's clip has no pointer data and its focus mode is auto, and a stored orbit is still shown as selected.
  - Cursor section has a "3D cursor" switch (disabled while the pointer is hidden).
  - Look section shows "Depth of field" only when some zoom has a rotation.
  - Frame picker offers none/window/laptop/phone/monitor + light/dark (verify, add what is missing).
  - Effects tab: "Add effect" menu lists the six catalogue effects (localised name + one-line description); adding places it at the playhead (2 s, clipped to the clip); a selected effect shows one control per param (number slider with the param's range, toggle switch, colour field); delete is inline two-stage; `ripple`/`spotlight` show a note when the clip has no pointer data.
  - Timeline: effects lane hidden with zero effects; a block per `EditorView.effects` span; drag with snapping; selection opens the Effects tab.
- [ ] **Step 2: Run** the component tests — FAIL.
- [ ] **Step 3: Implement.** Strings: `record.editor.camera3d.*`, `record.editor.cursor.model3d*`, `record.editor.dof*`, `record.editor.effects.*`, one name + description per effect id in BOTH locales.
- [ ] **Step 4: Run** `pnpm --filter inspector-rust-frontend test` (IR_SKIP_BADGES=1), `pnpm typecheck`, `pnpm lint`, `node scripts/check-bundle.mjs`. Check `mockup.html?lang=de` at 700 px with the Playwright browser (no synthetic keystrokes into the real app). Commit `feat(record): editor UI for 3D camera, 3D cursor and effects`.

### Task 5: Live verification, docs, version (controller)

- [ ] `#[ignore]` helper live renders (`record-render/tests/helper_live.rs`): a left-tilted zoom frame differs from the flat frame; a vignette frame darkens the corners vs. no effect; export of a 2-s clip with model3d + orbit + grain finishes. Run with `IR_RECORD_PROJECT=…`.
- [ ] Docs: CHANGELOG (new version), `features.txt` line, CLAUDE.md stage-7 section, `commandDocs.ts` record entry tip, `vendor/openscreen-compositor/VENDORED.md` (no new patch expected — say so), README badges via `node scripts/update-badges.mjs`.
- [ ] `bash scripts/check.sh`, `pnpm test`, install via `scripts/install-macos.sh`, commit, push.
