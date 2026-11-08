//! The editable form of a saved screenshot: the document (annotations, crop,
//! orientation) plus the backdrop, stored next to the original capture so
//! the result can be reopened with every element still movable. Pure.

import type { Annotation, BackgroundSpec, EditorDoc, Orient, Rect } from "./editor-geometry";
import { isSafeImageSrc } from "./editor-image";

export const PROJECT_VERSION = 1;

export type EditorProject = { doc: EditorDoc; background: BackgroundSpec | null };

const KNOWN = new Set([
  "arrow", "line", "rect", "ellipse", "highlight", "blur", "redact", "text", "step", "pen", "emoji", "spotlight", "image",
]);

export function serializeProject(doc: EditorDoc, background: BackgroundSpec | null): string {
  return JSON.stringify({ version: PROJECT_VERSION, doc, background });
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

function readAnnotation(v: unknown): Annotation | null {
  if (!isObj(v) || typeof v.id !== "string" || typeof v.type !== "string" || !KNOWN.has(v.type)) return null;
  if (v.type === "image" && !isSafeImageSrc(v.src)) return null;
  return v as unknown as Annotation;
}

function readRect(v: unknown): Rect | null {
  if (!isObj(v)) return null;
  const { x, y, w, h } = v;
  return [x, y, w, h].every((n) => typeof n === "number" && Number.isFinite(n)) ? { x: x as number, y: y as number, w: w as number, h: h as number } : null;
}

function readOrient(v: unknown): Orient | undefined {
  if (!isObj(v) || ![0, 90, 180, 270].includes(v.rot as number) || typeof v.flip !== "boolean") return undefined;
  return { rot: v.rot as Orient["rot"], flip: v.flip };
}

function readBackground(v: unknown): BackgroundSpec | null {
  if (!isObj(v) || (v.kind !== "solid" && v.kind !== "gradient") || typeof v.color !== "string") return null;
  return v as unknown as BackgroundSpec;
}

/** `null` for anything that isn't a project of this version. Elements it
 *  can't draw safely (unknown kinds, pictures from outside) are dropped. */
export function parseProject(json: string): EditorProject | null {
  let raw: unknown;
  try {
    raw = JSON.parse(json);
  } catch {
    return null;
  }
  if (!isObj(raw) || raw.version !== PROJECT_VERSION || !isObj(raw.doc) || !Array.isArray(raw.doc.annotations)) return null;
  const annotations = raw.doc.annotations.map(readAnnotation).filter((a): a is Annotation => a !== null);
  const doc: EditorDoc = { annotations, crop: readRect(raw.doc.crop), background: null };
  const orient = readOrient(raw.doc.orient);
  if (orient) doc.orient = orient;
  return { doc, background: readBackground(raw.background) };
}
