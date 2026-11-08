# Screenshot editor stage 1 — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Arrow styles A–D, selection that hits what you point at (edge hits,
ranking, click-cycling, multi-select, layer order), per-tool remembered
settings, editing basics, fill/text styles and favourite colours in the
screenshot editor.

**Architecture:** All new logic lands as pure, unit-tested functions in three
new `lib/` modules (arrow geometry, selection, preferences) plus additive
fields on the existing annotation model in `lib/editor-geometry.ts`. The
component `components/ScreenshotEditor.tsx` only wires pointer/keyboard events
and canvas drawing to them. Preferences persist as one JSON string in the Rust
`settings` table through two new IPC commands.

**Tech Stack:** React 19 + TypeScript + Vitest (happy-dom), Canvas 2D, Rust
(Tauri commands, rusqlite settings table).

**Spec:** `docs/superpowers/specs/2026-11-08-screenshot-editor-design.md`

## Global Constraints

- No new npm or crate dependency.
- Old annotations render exactly as before: an arrow without `style` is style
  `"classic"`, a rect/ellipse without `fill` is `"outline"`, text without
  `textStyle` is `"plain"`.
- ⌘C always copies the whole image (unchanged). Annotation copy/paste is
  ⌥⌘C / ⌥⌘V.
- Outline and "outline + light fill" (`"tint"`) shapes are hit only near
  their stroke; equal-distance area hits prefer the smaller element; newest
  is the last tie-break.
- Every user action is exactly one undo step (one `commit` to the history).
- Export never contains selection chrome; preview and export both draw through
  `paintScene` → `drawAnnotation`.
- Settings key `screenshot.editor_styles`; it must NOT be added to
  `settings_sync::LOCAL_KEYS` (it syncs between Macs). Malformed JSON falls
  back to defaults per field.
- Existing tool keys stay: V A L T R E H B X N P C S (and M emoji if present).
- Every user-visible string in `locales/en.json` and `locales/de.json`.
- `.claude/rules/animation.md` applies to any new motion (none required).

## Review Focus

1. Documents made before this change (no `style`/`fill`/`textStyle`/control
   point): must draw and hit exactly as before — Task 1 pins
   `normalizeAnnotation` defaults, Task 2 pins classic polygon unchanged.
2. A curved/marker arrow that is moved or has an endpoint dragged: the bend
   must travel with it, not stay behind at the old position — Task 2.
3. Click-cycling when the pointer jitters by a pixel or two, or the element
   last picked was deleted: still a cycle, never stuck or throwing — Task 3.
4. Deleting a multi-selection that contains step badges: remaining badges
   renumber (`reflowSteps`) in one undo step — Task 3 (pure) + Task 5.
5. Corrupt, partial or hostile prefs JSON (wrong types, unknown tools,
   non-hex colours, huge favourites list): defaults per field, colours
   validated, favourites capped at 8 — Task 4; Rust caps the stored string
   size — Task 4.

---

### Task 1: Model extensions

**Files:**
- Modify: `core/frontend/src/lib/editor-geometry.ts`
- Test: `core/frontend/src/lib/editor-geometry.test.ts`

**Interfaces:**
- Produces:
  - `export type ArrowStyle = "classic" | "bold" | "curved" | "marker";`
  - `export type FillMode = "outline" | "filled" | "tint";`
  - `export type TextStyle = "plain" | "outline" | "pill" | "box";`
  - `ArrowAnnotation` gains `style?: ArrowStyle; cx?: number; cy?: number`
    (control point; absent = straight).
  - `RectAnnotation`, `EllipseAnnotation` gain `fill?: FillMode`.
  - `TextAnnotation` gains `textStyle?: TextStyle`.
  - `Style` gains `arrowStyle: ArrowStyle; fill: FillMode; textStyle: TextStyle`.
  - `Handle` gains `"mid"`.
  - `DEFAULT_STYLE` = `{ color "#ef4444", width 6, opacity 1, shadow false,
    fontSize 28, arrowStyle "classic", fill "outline", textStyle "plain" }`
    (width 4 → 6: the arrow no longer starts thin).
  - `makeDragAnnotation` copies `arrowStyle` / `fill` / `textStyle` from the
    style; for `"curved"`/`"marker"` it sets the control point to
    `defaultBend(x1,y1,x2,y2)` from Task 2.
  - `restyleAnnotation(a, patch)` applies `arrowStyle` to arrows (switching to
    curved/marker without a control point adds `defaultBend`; switching to
    classic/bold drops `cx/cy`), `fill` to rect/ellipse, `textStyle` to text.

- [ ] **Step 1: Failing tests**
  - `old annotations keep their look`: `normalizeAnnotation` of an arrow
    without `style` returns `style: "classic"` and no `cx/cy`; rect/ellipse
    without `fill` → `"outline"`; text without `textStyle` → `"plain"`.
  - `drag arrow uses the style`: `makeDragAnnotation("arrow", …, {…DEFAULT_STYLE, arrowStyle: "curved"})`
    has `style "curved"` and numeric `cx/cy` not on the straight line.
  - `restyle switches arrow style both ways`: classic → curved adds `cx/cy`,
    curved → bold removes them.
  - `DEFAULT_STYLE.width === 6`.
- [ ] **Step 2:** `pnpm --filter inspector-rust-frontend exec vitest run src/lib/editor-geometry.test.ts` → FAIL.
- [ ] **Step 3:** Implement the fields and the three function changes. Task 2's
  `defaultBend` is needed here — implement Task 2 Step 3's `defaultBend`
  first or stub it in `editor-arrows.ts` and import.
- [ ] **Step 4:** Same command → PASS, and the whole geometry file stays green.
- [ ] **Step 5:** Commit `feat(editor): arrow/fill/text style fields`.

### Task 2: Arrow geometry

**Files:**
- Create: `core/frontend/src/lib/editor-arrows.ts`
- Test: `core/frontend/src/lib/editor-arrows.test.ts`
- Modify: `core/frontend/src/lib/editor-geometry.ts` (hit, bounds, handles,
  move, resize for arrows)

**Interfaces:**
- Produces:
  - `defaultBend(x1: number, y1: number, x2: number, y2: number): Point` —
    a control point offset perpendicular from the midpoint by 18 % of the
    length (bend to the left of the direction of travel).
  - `arrowOutline(a: ArrowAnnotation): Point[]` — filled polygon for
    `"classic"` (exactly today's `drawArrow` geometry, moved here) and
    `"bold"` (tail half-width ≈ 0.25·w, body ≈ 0.75·w, head length
    `max(26, 5.5·w)` capped at 60 % of the length, head half-width ≈ 0.5·head length).
  - `arrowCurve(a: ArrowAnnotation, segments = 32): Point[]` — sampled
    quadratic from (x1,y1) via (cx,cy) to the point where the head starts.
  - `arrowHead(a: ArrowAnnotation): { tip: Point; left: Point; right: Point; open: boolean }`
    oriented on the end tangent (control → tip); `open` true for `"marker"`.
  - `distToArrow(a: ArrowAnnotation, p: Point): number` — distance to the
    shaft (straight segment or sampled curve).
- In `editor-geometry.ts`: `hitOne` for arrows uses `distToArrow`;
  `boundsOf` includes the control point; `handlesFor` adds `{pos:"mid"}` at
  the curve's apex (t = 0.5) for curved/marker; `moveAnnotation` shifts
  `cx/cy` too; `resizeAnnotation` with `"start"`/`"end"` moves that endpoint
  and shifts the control point by half the delta (bend keeps its shape);
  with `"mid"` it sets the control point so the curve's apex follows the
  pointer (`c = 2·apex − (p1+p2)/2`).

- [ ] **Step 1: Failing tests**
  - `classic outline is unchanged`: for a fixed arrow (0,0)→(100,0), w 4, the
    polygon equals the points today's `drawArrow` produces (copy the current
    numbers into the test before refactoring).
  - `bold is wider than classic` at the same width (max |y| larger).
  - `curve passes near the control point's apex` and starts at (x1,y1).
  - `head follows the end tangent`: for a curve bending up, the head
    direction differs from the straight x1→x2 direction.
  - `hit along a curve`: a point on the sampled curve hits; the straight
    midpoint of x1→x2 (off the curve) does not.
  - `move carries the bend`, `dragging an endpoint keeps the bend`,
    `dragging mid moves the apex to the pointer` (apex within 0.5 px).
- [ ] **Step 2:** run `vitest run src/lib/editor-arrows.test.ts src/lib/editor-geometry.test.ts` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Same → PASS.
- [ ] **Step 5:** Commit `feat(editor): arrow styles geometry`.

### Task 3: Selection logic

**Files:**
- Create: `core/frontend/src/lib/editor-selection.ts`
- Test: `core/frontend/src/lib/editor-selection.test.ts`
- Modify: `core/frontend/src/lib/editor-geometry.ts` (`hitOne` for
  rect/ellipse respects `fill`; `hitTest` delegates to `pickAt`)

**Interfaces:**
- Produces:
  - `type HitCandidate = { id: string; dist: number; area: number; index: number }`
  - `hitCandidates(anns: ReadonlyArray<Annotation>, p: Point, tol?: number): HitCandidate[]`
    sorted best-first: `dist` ascending, then `area` ascending, then `index`
    descending (newest). Outline/tint rect and ellipse: `dist` = distance to
    the stroke (ellipse: radial approximation), hit if ≤ `tol + width/2`;
    filled rect/ellipse, highlight, blur, redact, spotlight, text, emoji,
    step: `dist` 0 inside.
  - `pickAt(anns, p, prev: { id: string; at: Point } | null, tol?: number): string | null`
    — best candidate; if `prev` is within 4 px of `p` and `prev.id` is among
    the candidates, return the candidate after it (wrap around).
  - `marqueeIds(anns, r: Rect): string[]` — ids whose `boundsOf` intersects
    `r`, in document order.
  - `reorder(anns, ids: ReadonlyArray<string>, op: "forward" | "backward" | "front" | "back"): Annotation[]`
    — keeps the relative order of the moved elements.
  - `duplicate(anns, ids, offset: number, newId: () => string): { annotations: Annotation[]; ids: string[] }`
    — copies appended on top, moved by `offset`, step badges renumbered via
    `reflowSteps`.
  - `pasteClip(anns, clip: ReadonlyArray<Annotation>, offset: number, newId: () => string): { annotations: Annotation[]; ids: string[] }`
  - `removeIds(anns, ids): Annotation[]` — with `reflowSteps`.
  - `axisLock(dx: number, dy: number, on: boolean): { dx: number; dy: number }` — zeroes the smaller axis when `on`.
  - `resizeBy(a: Annotation, dir: 1 | -1): Annotation` — width ±1 (2–24),
    text/step/emoji size ±2 (8–200); others unchanged.

- [ ] **Step 1: Failing tests**
  - `the reported bug`: arrow (100,100)→(200,150), then an outline rect
    (50,50,250,200) drawn after it; `pickAt` at a point on the arrow returns
    the arrow id; at a point on the rect's edge returns the rect.
  - `tint behaves like outline`, `filled rect is hit inside`.
  - `smaller filled element wins`: filled rect inside a larger filled rect →
    inner id.
  - `click again cycles`: two overlapping filled rects, second `pickAt` with
    `prev` 2 px away returns the other; a third returns the first again.
  - `stale prev is ignored`: `prev.id` not under the pointer (or deleted) →
    best candidate.
  - `marquee touches`, `reorder front/back/forward/backward` (incl. a
    multi-id case keeping relative order), `duplicate renumbers steps`,
    `removeIds renumbers steps`, `axisLock`, `resizeBy clamps`.
- [ ] **Step 2:** run `vitest run src/lib/editor-selection.test.ts` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** PASS; run `src/lib/editor-geometry.test.ts` too (the old
  `hitTest` tests may need updating only where the spec changed behaviour —
  note each changed assertion in the commit message).
- [ ] **Step 5:** Commit `feat(editor): selection ranking, cycling, marquee, layer order`.

### Task 4: Preferences (pure + Rust slot)

**Files:**
- Create: `core/frontend/src/lib/editor-prefs.ts`
- Test: `core/frontend/src/lib/editor-prefs.test.ts`
- Modify: `core/rust-lib/src/screenshot_editor.rs`, `core/rust-lib/src/lib.rs`
  (register commands), `core/frontend/src/lib/ipc.ts`,
  `core/frontend/src/lib/ipc.test.ts`

**Interfaces:**
- Produces (TS):
  - `type StyledTool = "arrow" | "line" | "rect" | "ellipse" | "text" | "highlight" | "pen" | "step" | "emoji"`
  - `type EditorPrefs = { version: 1; lastTool: Tool; styles: Partial<Record<StyledTool, Style>>; favorites: string[] }`
  - `DEFAULT_PREFS: EditorPrefs` (`lastTool "arrow"`, empty styles/favorites)
  - `parsePrefs(json: string | null): EditorPrefs` — field-wise tolerant;
    colours must match `/^#[0-9a-f]{6}$/i`; numbers clamped to the slider
    ranges; unknown tools/keys dropped.
  - `styleFor(p: EditorPrefs, tool: Tool): Style` — stored style or
    `DEFAULT_STYLE`.
  - `rememberStyle(p, tool, patch: Partial<Style>): EditorPrefs` — no-op for
    non-styled tools (select, crop, blur, redact, spotlight).
  - `withLastTool(p, tool): EditorPrefs`
  - `addFavorite(p, color): EditorPrefs` — dedupe (case-insensitive), newest
    first, max 8; `removeFavorite(p, color)`.
  - `serializePrefs(p): string`
- Produces (Rust): `#[tauri::command] get_editor_prefs(db) -> Result<Option<String>, String>`,
  `#[tauri::command] set_editor_prefs(db, json: String) -> Result<(), String>`
  (rejects > 16 KiB and non-JSON-object input), key
  `"screenshot.editor_styles"`.
- Produces (ipc.ts): `getEditorPrefs(): Promise<string | null>`,
  `setEditorPrefs(json: string): Promise<void>`.

- [ ] **Step 1: Failing tests**
  - TS: `garbage falls back`, `partial keeps valid fields`, `bad colour
    dropped`, `width clamped`, `remember is per tool`, `favorites dedupe and
    cap at 8`, `round-trip parse(serialize(p)) == p`.
  - Rust (`screenshot_editor::tests`): `set_rejects_oversized_and_non_object`,
    `get_returns_what_set_stored` (in-memory DB).
  - `ipc.test.ts`: command names `get_editor_prefs` / `set_editor_prefs` and
    arg shape `{ json }`.
  - Pin: `settings_sync::syncable("screenshot.editor_styles")` is true.
- [ ] **Step 2:** `vitest run src/lib/editor-prefs.test.ts src/lib/ipc.test.ts`
  and `cargo test -p inspector-rust-core --lib screenshot_editor settings_sync` → FAIL.
- [ ] **Step 3:** Implement; register both commands in `lib.rs`'s
  `invoke_handler!` next to `set_editor_size`.
- [ ] **Step 4:** Same → PASS.
- [ ] **Step 5:** Commit `feat(editor): remembered per-tool styles`.

### Task 5: Wire selection into the editor

**Files:**
- Modify: `core/frontend/src/components/ScreenshotEditor.tsx`
- Modify: `core/frontend/src/locales/en.json`, `de.json`

**Interfaces:**
- Consumes: Task 3 (`pickAt`, `marqueeIds`, `reorder`, `duplicate`,
  `pasteClip`, `removeIds`, `axisLock`, `resizeBy`), Task 1 types.
- Changes: `selectedId: string | null` → `selectedIds: string[]`;
  `Interaction` gains `{ kind: "marquee"; start: Point; current: Point; additive: boolean }`
  and `move` becomes `{ kind: "move"; ids: string[]; startPt; orig: Annotation[]; live: Annotation[] }`;
  a `lastPick` ref holds `{ id, at }` for cycling.

- [ ] **Step 1:** Behaviour:
  - Mouse down with any tool on an element → `pickAt` (cycling); Shift
    toggles membership; on empty canvas with the select tool → marquee
    (Shift adds); other tools start drawing as today.
  - Drag moves all selected; Shift during move → `axisLock`.
  - Resize handles and the mid handle only with exactly one selected.
  - Keys: Delete/Backspace → `removeIds`; arrows nudge all; ⌘D duplicate;
    ⌥⌘C copies selected into a component-level ref, ⌥⌘V pastes (offset 16,
    pasted become the selection; each paste offsets again); ⌘] / ⌘[ / ⌘⇧] /
    ⌘⇧[ → forward/backward/front/back; `+`/`=` and `-` → `resizeBy` on the
    selection (or adjust the active tool's width when nothing is selected);
    ⌘C unchanged (whole image). Escape clears the selection first.
  - Every action = one `commit`.
  - Draw the marquee rectangle on the on-screen canvas only (never in
    `paintScene`).
- [ ] **Step 2:** Add the shortcuts to the header comment of the component and
  to the toolbar tooltips; strings in both locales.
- [ ] **Step 3:** `pnpm --filter inspector-rust-frontend exec tsc --noEmit`,
  `pnpm lint`, `vitest run src/lib/editor-*.test.ts src/lib/i18n.test.ts` → PASS.
- [ ] **Step 4:** Commit `feat(editor): multi-select, cycling, layer order, duplicate/paste`.

### Task 6: Drawing styles and toolbar controls

**Files:**
- Modify: `core/frontend/src/components/ScreenshotEditor.tsx`, locales

**Interfaces:**
- Consumes: Task 2 (`arrowOutline`, `arrowCurve`, `arrowHead`), Task 1 types.

- [ ] **Step 1:** `drawArrow` dispatches on style:
  - classic/bold: fill `arrowOutline`; bold additionally strokes the outline
    in white (≈ 0.6·w, round join) under the fill and always gets the drop
    shadow.
  - curved: white under-stroke + coloured stroke along `arrowCurve`
    (round cap), filled head triangle with white rim.
  - marker: coloured stroke along `arrowCurve`, open chevron head
    (`left→tip→right`), round caps/joins, no rim.
  Match the mockup `.superpowers/brainstorm/95736-1794105260/content/arrow-styles.html`.
- [ ] **Step 2:** Rect/ellipse fill: `filled` fills with the colour;
  `tint` strokes plus a fill at 18 % alpha; `outline` as today.
- [ ] **Step 3:** Text styles: `plain` today; `outline` white text with a dark
  stroke (~0.15·size); `pill` text on a fully rounded box in the colour with
  white text; `box` text on a 70 % black rounded box.
- [ ] **Step 4:** Toolbar: a segmented control (4 icons) for arrow style
  when the arrow tool or a selected arrow is active; fill-mode control for
  rect/ellipse; text-style control for text. Changing one restyles the
  selection (one commit) and becomes the tool's remembered style (Task 7).
  `drawSelection` draws the `mid` handle.
- [ ] **Step 5:** `tsc`, `lint`, editor tests → PASS. Commit
  `feat(editor): arrow, fill and text styles`.

### Task 7: Remembered settings and favourites in the UI

**Files:**
- Modify: `core/frontend/src/components/ScreenshotEditor.tsx`, locales

**Interfaces:**
- Consumes: Task 4 (`parsePrefs`, `styleFor`, `rememberStyle`,
  `withLastTool`, `addFavorite`, `removeFavorite`, `serializePrefs`,
  `getEditorPrefs`, `setEditorPrefs`).

- [ ] **Step 1:** On mount read prefs once; start with `lastTool` and
  `styleFor(lastTool)`. Switching tools loads that tool's style. Every
  control change → `rememberStyle`; tool switch → `withLastTool`. Save with a
  400 ms debounce and once more on unmount; failures are logged, never shown.
- [ ] **Step 2:** Favourites row next to the colour presets: a "+" pins the
  current colour, right-click (or ⌥-click) a favourite removes it.
- [ ] **Step 3:** A component test isn't practical with canvas; instead add
  `editor-prefs.test.ts` case `tool switch round-trip`: remember arrow red/8,
  rect blue/3 → `styleFor` returns each unchanged.
- [ ] **Step 4:** `tsc`, `lint`, tests → PASS. Commit
  `feat(editor): load and save remembered settings, favourite colours`.

### Task 8: Verify, document, release

**Files:**
- Modify: `CHANGELOG.md`, `features.txt`, `CLAUDE.md` (Screenshot preview +
  editor section), version files (Cargo.toml, 3× tauri.conf.json, root /
  macos / win / linux / core/frontend package.json → 0.245.0),
  `core/frontend/src/mockup/backend.ts` (if needed for the sight check).

- [ ] **Step 1: Sight check.** Run the mockup harness with
  `?window=screenshot-editor` (add mock handlers for the editor's IPC with an
  invented screenshot if missing), draw all four arrow styles, fill modes and
  text styles, and take a Playwright screenshot. Compare against the mockup.
  Use only the test page — never synthetic input on the real desktop.
- [ ] **Step 2:** `bash scripts/check.sh` → `✓ all checks passed`;
  `pnpm test` → all green, badges updated.
- [ ] **Step 3:** Mutation probes (at least): remove the edge-only rule for
  outline rects (the bug test must fail); remove cycling; drop the
  `cx/cy` shift in `moveAnnotation`; drop the favourites cap. Each must turn a
  test red; restore and confirm by checksum.
- [ ] **Step 4:** Docs: CHANGELOG `[0.245.0]` (English), features.txt line for
  the editor updated, CLAUDE.md editor paragraph (styles, selection rules,
  prefs key, ⌘C vs ⌥⌘C).
- [ ] **Step 5:** `rm -rf target/debug && bash scripts/install-macos.sh`;
  confirm version 0.245.0 runs.
- [ ] **Step 6:** Commit `feat(editor): stage 1 — arrow styles, selection, remembered settings (v0.245.0)` and push.
