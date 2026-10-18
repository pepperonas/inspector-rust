//! Pure geometry + data model for the screenshot annotation editor
//! (`ScreenshotEditor.tsx`). Kept free of canvas / React so it can be
//! unit-tested. The component owns the actual `CanvasRenderingContext2D`
//! drawing; this module only produces + transforms the annotation *data*.
//!
//! As of the CleanShot-X-parity rework the editor is **object-based**: every
//! annotation carries a stable `id`, and the pure functions here answer the
//! questions an interactive editor needs — what are an annotation's bounds,
//! which annotation is under the cursor (`hitTest`), where its resize handles
//! sit (`handlesFor`), and how to move/resize/restyle it. The component only
//! wires pointer events to these.

/** The drawing tools. `select` is the pointer (move/resize existing
 *  annotations); drag tools produce a shape from mousedown→mouseup; `text`,
 *  `step` and `emoji` are click-placed; `pen` accumulates a freehand path;
 *  `crop` adjusts the export viewport. */
export type Tool =
  | "select"
  | "arrow"
  | "line"
  | "text"
  | "rect"
  | "ellipse"
  | "highlight"
  | "blur"
  | "redact"
  | "step"
  | "pen"
  | "crop"
  | "spotlight"
  | "emoji";

export type Point = { x: number; y: number };
export type Rect = { x: number; y: number; w: number; h: number };

/** Resize-handle positions. Box-like shapes expose the eight compass
 *  handles; arrow/line expose their two endpoints; text/step/emoji a single
 *  bottom-right scale handle; pen the four bbox corners. */
export type Handle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w" | "start" | "end";

/** The live drawing style the toolbar holds. Applied to a new annotation at
 *  commit time, and to the selected annotation when a control changes. */
export type Style = {
  color: string;
  width: number;
  /** 0..1, multiplies the annotation's own alpha. */
  opacity: number;
  shadow: boolean;
  /** Font/badge/emoji size for text/step/emoji. */
  fontSize: number;
};

type WithId = { id: string };
export type AnnotationCommon = { color: string; width: number };
/** Stroke-style extras shared by the vector shapes. */
type Styled = { opacity?: number; shadow?: boolean };

export type ArrowAnnotation = WithId & AnnotationCommon & Styled & {
  type: "arrow";
  x1: number;
  y1: number;
  x2: number;
  y2: number;
};
export type LineAnnotation = WithId & AnnotationCommon & Styled & {
  type: "line";
  x1: number;
  y1: number;
  x2: number;
  y2: number;
};
export type RectAnnotation = WithId & AnnotationCommon & Styled & {
  type: "rect";
  x: number;
  y: number;
  w: number;
  h: number;
};
export type EllipseAnnotation = WithId & AnnotationCommon & Styled & {
  type: "ellipse";
  x: number;
  y: number;
  w: number;
  h: number;
};
export type HighlightAnnotation = WithId & {
  type: "highlight";
  x: number;
  y: number;
  w: number;
  h: number;
  color: string;
};
export type BlurAnnotation = WithId & {
  type: "blur";
  x: number;
  y: number;
  w: number;
  h: number;
  blockSize: number;
};
/** Opaque redaction block (vs. blur = pixelated). Always solid black. */
export type RedactAnnotation = WithId & {
  type: "redact";
  x: number;
  y: number;
  w: number;
  h: number;
};
export type TextAnnotation = WithId & Styled & {
  type: "text";
  x: number;
  y: number;
  text: string;
  color: string;
  size: number;
};
/** Monotonically-numbered badge (CleanShot's signature feature). */
export type StepAnnotation = WithId & Styled & {
  type: "step";
  x: number;
  y: number;
  number: number;
  color: string;
  size: number;
};
/** Freehand pen stroke through a point path. */
export type PenAnnotation = WithId & AnnotationCommon & Styled & {
  type: "pen";
  points: Point[];
};
/** A placed emoji / sticker, drawn centred on (x,y). */
export type EmojiAnnotation = WithId & Styled & {
  type: "emoji";
  x: number;
  y: number;
  char: string;
  size: number;
};
/** Spotlight: highlights its rect by dimming everything OUTSIDE it. */
export type SpotlightAnnotation = WithId & {
  type: "spotlight";
  x: number;
  y: number;
  w: number;
  h: number;
  /** Outside-dim strength 0..1 (default 0.6). */
  dim?: number;
};
export type Annotation =
  | ArrowAnnotation
  | LineAnnotation
  | RectAnnotation
  | EllipseAnnotation
  | HighlightAnnotation
  | BlurAnnotation
  | RedactAnnotation
  | TextAnnotation
  | StepAnnotation
  | PenAnnotation
  | EmojiAnnotation
  | SpotlightAnnotation;

/** Backdrop + padding spec (CleanShot-style "beautiful background"). The
 *  padding/radius/shadow fields are in canvas (image) pixels. Filled by the
 *  Background panel; `null` on the doc means "no backdrop" (bare screenshot). */
export type BackgroundSpec = {
  kind: "solid" | "gradient";
  /** Solid fill, or the first stop of a gradient. */
  color: string;
  /** Second gradient stop (diagonal top-left → bottom-right). */
  color2?: string;
  padding: number;
  radius: number;
  shadow: boolean;
};

/** The whole editable document — the unit the undo/redo history snapshots.
 *  `crop` restricts the exported viewport (null = full image); `background`
 *  wraps the shot in a padded backdrop (null = none). */
export type EditorDoc = {
  annotations: Annotation[];
  crop: Rect | null;
  background: BackgroundSpec | null;
};

export const EMPTY_DOC: EditorDoc = { annotations: [], crop: null, background: null };

export const COLOR_PRESETS = ["#ef4444", "#facc15", "#22c55e", "#3b82f6", "#a855f7", "#ffffff", "#000000"] as const;

export const DEFAULT_STYLE: Style = {
  color: COLOR_PRESETS[0],
  width: 4,
  opacity: 1,
  shadow: false,
  fontSize: 28,
};

/** Normalise a drag into a top-left-anchored rect (always non-negative w/h). */
export function dragRect(start: Point, end: Point): Rect {
  return {
    x: Math.min(start.x, end.x),
    y: Math.min(start.y, end.y),
    w: Math.abs(end.x - start.x),
    h: Math.abs(end.y - start.y),
  };
}

/** Normalise a (possibly negative-w/h) rect to top-left anchored. */
export function normRect(r: Rect): Rect {
  return {
    x: r.w < 0 ? r.x + r.w : r.x,
    y: r.h < 0 ? r.y + r.h : r.y,
    w: Math.abs(r.w),
    h: Math.abs(r.h),
  };
}

/** Build the annotation for a drag-based tool, with a caller-supplied `id`
 *  and live `style`. Returns `null` for tools that aren't drag-based
 *  (`select`, `text`, `step`, `pen`, `crop`, `emoji`) or unknown tools. */
export function makeDragAnnotation(
  tool: Tool,
  start: Point,
  end: Point,
  style: Style,
  id: string,
): Annotation | null {
  const { color, width, opacity, shadow } = style;
  switch (tool) {
    case "arrow":
      return { id, type: "arrow", x1: start.x, y1: start.y, x2: end.x, y2: end.y, color, width, opacity, shadow };
    case "line":
      return { id, type: "line", x1: start.x, y1: start.y, x2: end.x, y2: end.y, color, width, opacity, shadow };
    case "rect": {
      const r = dragRect(start, end);
      return { id, type: "rect", ...r, color, width, opacity, shadow };
    }
    case "ellipse": {
      const r = dragRect(start, end);
      return { id, type: "ellipse", ...r, color, width, opacity, shadow };
    }
    case "highlight": {
      const r = dragRect(start, end);
      // Highlight is always yellow — like a marker.
      return { id, type: "highlight", ...r, color: "#facc15" };
    }
    case "blur": {
      const r = dragRect(start, end);
      // Block size scales with stroke width; min 6px so it reads as blurred.
      return { id, type: "blur", ...r, blockSize: Math.max(6, width * 3) };
    }
    case "redact": {
      const r = dragRect(start, end);
      return { id, type: "redact", ...r };
    }
    case "spotlight": {
      const r = dragRect(start, end);
      return { id, type: "spotlight", ...r };
    }
    default:
      return null;
  }
}

/** Next step-badge number = one more than the highest existing badge.
 *  Robust to undo/redo (derives from the current annotation set). */
export function nextStepNumber(annotations: ReadonlyArray<Annotation>): number {
  let max = 0;
  for (const a of annotations) {
    if (a.type === "step" && a.number > max) max = a.number;
  }
  return max + 1;
}

// ── Object-editor geometry ──────────────────────────────────────────────────

/** Box-like annotations carry an {x,y,w,h} rect. */
type BoxLike = RectAnnotation | EllipseAnnotation | HighlightAnnotation | BlurAnnotation | RedactAnnotation | SpotlightAnnotation;
function isBox(a: Annotation): a is BoxLike {
  return (
    a.type === "rect" ||
    a.type === "ellipse" ||
    a.type === "highlight" ||
    a.type === "blur" ||
    a.type === "redact" ||
    a.type === "spotlight"
  );
}

function pointInRect(p: Point, r: Rect, tol: number): boolean {
  return p.x >= r.x - tol && p.x <= r.x + r.w + tol && p.y >= r.y - tol && p.y <= r.y + r.h + tol;
}

function distToSegment(p: Point, v: Point, w: Point): number {
  const l2 = (w.x - v.x) ** 2 + (w.y - v.y) ** 2;
  if (l2 === 0) return Math.hypot(p.x - v.x, p.y - v.y);
  let t = ((p.x - v.x) * (w.x - v.x) + (p.y - v.y) * (w.y - v.y)) / l2;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(p.x - (v.x + t * (w.x - v.x)), p.y - (v.y + t * (w.y - v.y)));
}

/** Axis-aligned bounding box of an annotation, normalised (w,h ≥ 0). */
export function boundsOf(a: Annotation): Rect {
  switch (a.type) {
    case "arrow":
    case "line":
      return normRect({ x: a.x1, y: a.y1, w: a.x2 - a.x1, h: a.y2 - a.y1 });
    case "rect":
    case "ellipse":
    case "highlight":
    case "blur":
    case "redact":
    case "spotlight":
      return normRect({ x: a.x, y: a.y, w: a.w, h: a.h });
    case "text":
      return { x: a.x, y: a.y - a.size / 2, w: Math.max(a.size, a.text.length * a.size * 0.6), h: a.size };
    case "step": {
      const r = a.size;
      return { x: a.x - r, y: a.y - r, w: 2 * r, h: 2 * r };
    }
    case "emoji": {
      const r = a.size;
      return { x: a.x - r / 2, y: a.y - r / 2, w: r, h: r };
    }
    case "pen": {
      if (a.points.length === 0) return { x: 0, y: 0, w: 0, h: 0 };
      const xs = a.points.map((p) => p.x);
      const ys = a.points.map((p) => p.y);
      const x = Math.min(...xs);
      const y = Math.min(...ys);
      return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y };
    }
  }
}

function hitOne(a: Annotation, p: Point, tol: number): boolean {
  switch (a.type) {
    case "arrow":
    case "line":
      return distToSegment(p, { x: a.x1, y: a.y1 }, { x: a.x2, y: a.y2 }) <= tol + a.width / 2;
    case "pen": {
      if (a.points.length === 1) return Math.hypot(p.x - a.points[0].x, p.y - a.points[0].y) <= tol + a.width / 2;
      for (let i = 1; i < a.points.length; i++) {
        if (distToSegment(p, a.points[i - 1], a.points[i]) <= tol + a.width / 2) return true;
      }
      return false;
    }
    case "step":
      return Math.hypot(p.x - a.x, p.y - a.y) <= a.size + tol;
    default:
      return pointInRect(p, boundsOf(a), tol);
  }
}

/** The id of the top-most annotation under `p` (iterating newest-first), or
 *  `null`. `tol` widens thin shapes (lines/arrows/pen) and box edges so they
 *  are grabbable. */
export function hitTest(
  annotations: ReadonlyArray<Annotation>,
  p: Point,
  tol = 6,
): string | null {
  for (let i = annotations.length - 1; i >= 0; i--) {
    if (hitOne(annotations[i], p, tol)) return annotations[i].id;
  }
  return null;
}

function eightHandles(b: Rect): { pos: Handle; x: number; y: number }[] {
  const { x, y, w, h } = b;
  return [
    { pos: "nw", x, y },
    { pos: "n", x: x + w / 2, y },
    { pos: "ne", x: x + w, y },
    { pos: "e", x: x + w, y: y + h / 2 },
    { pos: "se", x: x + w, y: y + h },
    { pos: "s", x: x + w / 2, y: y + h },
    { pos: "sw", x, y: y + h },
    { pos: "w", x, y: y + h / 2 },
  ];
}

/** Resize handles for the selection UI, in canvas coords. */
export function handlesFor(a: Annotation): { pos: Handle; x: number; y: number }[] {
  if (a.type === "arrow" || a.type === "line") {
    return [
      { pos: "start", x: a.x1, y: a.y1 },
      { pos: "end", x: a.x2, y: a.y2 },
    ];
  }
  if (a.type === "text" || a.type === "step" || a.type === "emoji") {
    const b = boundsOf(a);
    return [{ pos: "se", x: b.x + b.w, y: b.y + b.h }];
  }
  if (a.type === "pen") {
    const b = boundsOf(a);
    return [
      { pos: "nw", x: b.x, y: b.y },
      { pos: "ne", x: b.x + b.w, y: b.y },
      { pos: "se", x: b.x + b.w, y: b.y + b.h },
      { pos: "sw", x: b.x, y: b.y + b.h },
    ];
  }
  return eightHandles(boundsOf(a));
}

/** Translate an annotation by (dx,dy). Immutable. */
export function moveAnnotation(a: Annotation, dx: number, dy: number): Annotation {
  if (a.type === "arrow" || a.type === "line") {
    return { ...a, x1: a.x1 + dx, y1: a.y1 + dy, x2: a.x2 + dx, y2: a.y2 + dy };
  }
  if (a.type === "pen") {
    return { ...a, points: a.points.map((p) => ({ x: p.x + dx, y: p.y + dy })) };
  }
  return { ...a, x: a.x + dx, y: a.y + dy };
}

/** Resize an annotation by dragging `handle` by (dx,dy). Immutable. May
 *  produce a negative-w/h rect mid-drag; call `normalizeAnnotation` on commit. */
export function resizeAnnotation(a: Annotation, handle: Handle, dx: number, dy: number): Annotation {
  if (a.type === "arrow" || a.type === "line") {
    if (handle === "start") return { ...a, x1: a.x1 + dx, y1: a.y1 + dy };
    return { ...a, x2: a.x2 + dx, y2: a.y2 + dy };
  }
  if (a.type === "text" || a.type === "step" || a.type === "emoji") {
    return { ...a, size: Math.max(8, a.size + (dx + dy) / 2) };
  }
  if (a.type === "pen") {
    const b = boundsOf(a);
    if (b.w <= 0 || b.h <= 0) return a;
    // Scale the whole path around the corner OPPOSITE the dragged handle.
    const anchorX = handle.includes("w") ? b.x + b.w : b.x;
    const anchorY = handle.includes("n") ? b.y + b.h : b.y;
    const newW = handle.includes("w") ? b.w - dx : b.w + dx;
    const newH = handle.includes("n") ? b.h - dy : b.h + dy;
    const sx = newW / b.w;
    const sy = newH / b.h;
    if (!isFinite(sx) || !isFinite(sy)) return a;
    return { ...a, points: a.points.map((p) => ({ x: anchorX + (p.x - anchorX) * sx, y: anchorY + (p.y - anchorY) * sy })) };
  }
  // Box-like: adjust the edges the handle touches.
  let { x, y, w, h } = a;
  if (handle.includes("w")) {
    x += dx;
    w -= dx;
  }
  if (handle.includes("e")) {
    w += dx;
  }
  if (handle.includes("n")) {
    y += dy;
    h -= dy;
  }
  if (handle.includes("s")) {
    h += dy;
  }
  return { ...a, x, y, w, h };
}

/** Apply a style patch to the fields an annotation actually supports. */
export function restyleAnnotation(a: Annotation, patch: Partial<Style>): Annotation {
  const out = { ...a } as Annotation & { color?: string; width?: number; opacity?: number; shadow?: boolean; size?: number };
  // color — not highlight (forced yellow), not emoji/blur/redact/spotlight.
  const hasColor =
    a.type === "arrow" || a.type === "line" || a.type === "rect" || a.type === "ellipse" || a.type === "text" || a.type === "step" || a.type === "pen";
  const hasWidth = a.type === "arrow" || a.type === "line" || a.type === "rect" || a.type === "ellipse" || a.type === "pen";
  const hasOpacity = hasColor || a.type === "emoji";
  const hasShadow = hasColor || a.type === "emoji";
  const hasFont = a.type === "text" || a.type === "step" || a.type === "emoji";
  if (patch.color !== undefined && hasColor) out.color = patch.color;
  if (patch.width !== undefined && hasWidth) out.width = patch.width;
  if (patch.opacity !== undefined && hasOpacity) out.opacity = patch.opacity;
  if (patch.shadow !== undefined && hasShadow) out.shadow = patch.shadow;
  if (patch.fontSize !== undefined && hasFont) out.size = patch.fontSize;
  return out as Annotation;
}

/** Renumber every step badge 1..N in array order (after a delete). */
export function reflowSteps(annotations: ReadonlyArray<Annotation>): Annotation[] {
  let n = 0;
  return annotations.map((a) => (a.type === "step" ? { ...a, number: ++n } : a));
}

/** Normalise a box-like annotation's rect (post-resize, when w/h may be
 *  negative). Non-box annotations are returned unchanged. */
export function normalizeAnnotation(a: Annotation): Annotation {
  if (isBox(a)) return { ...a, ...normRect({ x: a.x, y: a.y, w: a.w, h: a.h }) };
  return a;
}

/** Clamp a (possibly negative / out-of-bounds) rect to the image, so a crop
 *  can never export pixels outside the screenshot. */
export function clampRectToImage(r: Rect, imgW: number, imgH: number): Rect {
  const n = normRect(r);
  const x0 = Math.max(0, n.x);
  const y0 = Math.max(0, n.y);
  const x1 = Math.min(imgW, n.x + n.w);
  const y1 = Math.min(imgH, n.y + n.h);
  return { x: x0, y: y0, w: Math.max(0, x1 - x0), h: Math.max(0, y1 - y0) };
}
