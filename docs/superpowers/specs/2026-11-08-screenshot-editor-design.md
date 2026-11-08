# Screenshot editor — stage 1 redesign

Date: 2026-11-08. Scope: `core/frontend/src/components/ScreenshotEditor.tsx`,
`core/frontend/src/lib/editor-geometry.ts`, `lib/editor-history.ts`, plus a small
Rust settings slot. Stage 2 (below) is out of scope for this document.

## Why

The user annotated a screenshot and hit three problems:

1. **The arrow looked thin and rigid** ("zu starr, zu wenig dynamisch").
2. **The selection tool could not grab the arrow** after a rectangle had been
   drawn around it. Cause (confirmed in code): `hitTest` walks newest-first
   and `hitOne` treats a rectangle/ellipse/highlight/blur/redact as hit
   anywhere inside its bounding box (`pointInRect`), so an outlined rectangle
   drawn later swallows every click on what lies inside it.
3. **Settings are forgotten**: colour, width, opacity, shadow and font size are
   plain `useState` in the component, one shared set for all tools, reset to
   `#ef4444` / 4 px on every open.

Goal set by the user: at least as good as CleanShot X's Annotate, and better.
Research (cleanshot.com features + changelog, 2026-11-08) found CleanShot X has
4 arrow styles incl. curved, filled vs outlined rectangles, 7 text styles,
favourite colours, ⌘D duplicate, ⌘C copy object, Shift axis-lock while moving,
`+`/`` ` `` to change tool size. CleanShot documents nothing about multi-select,
layer order, alignment guides or per-tool memory — that is where we go further.

## Goals (stage 1)

### G1 Arrow styles
- Four styles, chosen per arrow and shown as a segmented control while the
  arrow tool (or a selected arrow) is active:
  - **A "Classic"** — today's tapered filled polygon (`drawArrow`). **Default.**
  - **B "Bold"** — tapered, broader body and head, white rim + drop shadow.
  - **C "Curved"** — quadratic curve with a **midpoint handle** to bend it;
    the head follows the end tangent. A fresh curved arrow starts with a gentle
    default bend so it is visibly curved.
  - **D "Marker"** — hand-drawn look: soft cubic curve, open chevron head,
    round caps. Also bendable via the midpoint handle.
  - Reference mockup: `.superpowers/brainstorm/…/content/arrow-styles.html`
    (user picked A as default, B/C/D as options).
- The default width rises so a fresh arrow no longer looks thin (builder picks
  the value; today 4 px on a 2–16 range).
- Arrow style is part of the remembered arrow settings (G3).

### G2 Selection that hits what you point at
- Outlined shapes (rect, ellipse) — **including "outline + light fill"** — are
  hit **only near their stroke** (stroke width + tolerance). Fully filled shapes and regions (filled rect, highlight, blur,
  redact, spotlight, emoji, text, step) keep area hits.
- Among several hits under the pointer, prefer the one whose stroke is
  closest; among area hits (distance 0) prefer the **smaller** element (it is
  usually the one drawn on top of / inside the bigger one); newest only as the
  last tie-break.
- **Click again on the same spot** (without moving) cycles to the next element
  underneath.
- **Multi-select**: Shift-click toggles an element in the selection; dragging
  on empty canvas with the select tool draws a marquee that selects every
  element it touches. Move, delete, nudge, restyle and z-order apply to the
  whole selection. Resize handles only when exactly one element is selected.
- **Layer order**: bring forward / send backward / to front / to back
  (⌘] ⌘[ ⌘⇧] ⌘⇧[ — builder may adjust if they collide).
- **Grab-to-edit in every tool** stays (existing behaviour, see CLAUDE.md).

### G3 Remembered settings
- Each tool remembers its own last style: colour, width, opacity, shadow, font
  size, and tool-specific options (arrow style, rect fill mode, text style).
  Changing a control while an element is selected restyles it **and** becomes
  that tool's remembered style.
- Also remembered: favourite colours (G6). The last active tool is remembered
  too (my call).
- Stored in the Rust `settings` table as one JSON value
  (`screenshot.editor_styles`), read on editor open, written debounced. It is a
  preference, so device sync (`settings_sync`) carries it across Macs — do NOT
  add it to `LOCAL_KEYS`. Unknown/garbage JSON falls back to defaults per field
  (same tolerant style as `meeting.config`).

### G4 Editing basics
- ⌘D duplicates the selection (offset a few px). ⌥⌘C copies the selected
  annotations to an in-memory clipboard, ⌥⌘V pastes them (offset, selected).
  **⌘C always copies the whole image, exactly as today** (user decision
  2026-11-08: the editor auto-selects every freshly drawn element, so a
  selection-dependent ⌘C would break "draw, ⌘C").
- Shift while moving locks to the dominant axis.
- `+` / `-` change the size of the selected element(s) or the active tool's
  remembered width.
- Every one of these is one undo step.

### G5 Fill and text styles
- Rectangle (and ellipse) get a fill mode: outline · filled · outline + light
  fill. Hit-testing follows the mode (G2).
- Text gets a small set of styles (builder decides how many, 3–6), e.g. plain,
  white with dark outline, on a solid pill, on a translucent box.

### G6 Favourite colours
- Pin the current colour as a favourite; favourites show next to the presets
  and are remembered (G3). Max ~8, oldest drops.

## Non-goals (stage 1)
- Stage 2 items: OCR smart highlighter, automatic redaction of emails/keys/phone
  numbers, alignment guides, crop aspect presets, rotate/flip, second image on
  the canvas, re-editable project files, pixelate mode.
- No change to capture, the floating preview, save/copy/export paths.
- No new npm/crate dependency.

## Anti-goals
- Hotkey → paste latency must not change (the editor is a separate window;
  nothing here touches the popup).
- A saved PNG must never contain selection chrome (the existing
  `paintScene`/export split must stay the single composition path).
- No regressions for existing annotations: old arrows render exactly as today
  (style A) and old rectangles as outline.
- Animation rules in `.claude/rules/animation.md` apply to any new UI motion.

## Constraints
- Geometry and model stay pure and unit-tested in `lib/editor-geometry.ts`
  (house style); the canvas drawing stays in the component.
- History stays a whole-document snapshot (`lib/editor-history.ts`); every
  user action is exactly one undo step.
- All new UI strings in `locales/en.json` + `de.json`.
- Arrow styles B–D must look like the mockup; drawing goes through the same
  `drawAnnotation` path for preview and export.

## Left to the builder
- Exact widths, head proportions, curve defaults, handle visuals.
- Shortcut letters for new controls where not given above (avoid the existing
  tool keys V A L T R E H B X N P C S).
- Whether the marquee selects on "touches" or "fully inside" (touches is the
  default stated above; deviate only with a reason).
- Toolbar layout for the new controls, within the existing bar.

## Calls I made (not discussed with the user)
- Remember the last active tool.
- Store settings in the synced `settings` table, not localStorage.
- "Outline + light fill" counts as outline for hit-testing; equal-distance area
  hits prefer the smaller element.
- Fill modes also for ellipse.

## Stage 2 (later, separate design)
OCR smart highlighter; automatic redaction of sensitive text; alignment guides;
crop presets, rotate/flip; second image on the canvas; reopen a screenshot for
editing.
