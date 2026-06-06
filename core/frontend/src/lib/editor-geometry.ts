//! Pure geometry + data model for the screenshot annotation editor
//! (`ScreenshotEditor.tsx`). Kept free of canvas / React so it can be
//! unit-tested. The component owns the actual `CanvasRenderingContext2D`
//! drawing; this module only produces the annotation *data*.

/** The drawing tools. Drag tools produce a shape from a mousedown→mouseup
 *  rectangle/line; `text` and `step` are click-placed. */
export type Tool =
  | "arrow"
  | "line"
  | "text"
  | "rect"
  | "ellipse"
  | "highlight"
  | "blur"
  | "redact"
  | "step";

export type Point = { x: number; y: number };

export type AnnotationCommon = { color: string; width: number };
export type ArrowAnnotation = AnnotationCommon & {
  type: "arrow";
  x1: number;
  y1: number;
  x2: number;
  y2: number;
};
export type LineAnnotation = AnnotationCommon & {
  type: "line";
  x1: number;
  y1: number;
  x2: number;
  y2: number;
};
export type RectAnnotation = AnnotationCommon & {
  type: "rect";
  x: number;
  y: number;
  w: number;
  h: number;
};
export type EllipseAnnotation = AnnotationCommon & {
  type: "ellipse";
  x: number;
  y: number;
  w: number;
  h: number;
};
export type HighlightAnnotation = {
  type: "highlight";
  x: number;
  y: number;
  w: number;
  h: number;
  color: string;
};
export type BlurAnnotation = {
  type: "blur";
  x: number;
  y: number;
  w: number;
  h: number;
  blockSize: number;
};
/** Opaque redaction block (vs. blur = pixelated). Always solid black. */
export type RedactAnnotation = {
  type: "redact";
  x: number;
  y: number;
  w: number;
  h: number;
};
export type TextAnnotation = {
  type: "text";
  x: number;
  y: number;
  text: string;
  color: string;
  size: number;
};
/** Monotonically-numbered badge (CleanShot's signature feature). */
export type StepAnnotation = {
  type: "step";
  x: number;
  y: number;
  number: number;
  color: string;
  size: number;
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
  | StepAnnotation;

export const COLOR_PRESETS = ["#ef4444", "#facc15", "#ffffff", "#000000"] as const;

/** Normalise a drag into a top-left-anchored rect (always non-negative w/h). */
export function dragRect(
  start: Point,
  end: Point,
): { x: number; y: number; w: number; h: number } {
  return {
    x: Math.min(start.x, end.x),
    y: Math.min(start.y, end.y),
    w: Math.abs(end.x - start.x),
    h: Math.abs(end.y - start.y),
  };
}

/** Build the annotation for a drag-based tool. Returns `null` for tools
 *  that aren't drag-based (`text`, `step`) or unknown tools. */
export function makeDragAnnotation(
  tool: Tool,
  start: Point,
  end: Point,
  color: string,
  width: number,
): Annotation | null {
  switch (tool) {
    case "arrow":
      return { type: "arrow", x1: start.x, y1: start.y, x2: end.x, y2: end.y, color, width };
    case "line":
      return { type: "line", x1: start.x, y1: start.y, x2: end.x, y2: end.y, color, width };
    case "rect": {
      const r = dragRect(start, end);
      return { type: "rect", ...r, color, width };
    }
    case "ellipse": {
      const r = dragRect(start, end);
      return { type: "ellipse", ...r, color, width };
    }
    case "highlight": {
      const r = dragRect(start, end);
      // Highlight is always yellow — like a marker.
      return { type: "highlight", ...r, color: "#facc15" };
    }
    case "blur": {
      const r = dragRect(start, end);
      // Block size scales with stroke width; min 6px so it reads as blurred.
      return { type: "blur", ...r, blockSize: Math.max(6, width * 3) };
    }
    case "redact": {
      const r = dragRect(start, end);
      return { type: "redact", ...r };
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
