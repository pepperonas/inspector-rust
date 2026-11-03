/**
 * Pure UI logic of the record editor's camera tab and layout lane (stage 5).
 *
 * Programme ↔ source stays in Rust (R12): the layout lane draws `view.layoutSpans`, and a dragged
 * region start is mapped to source by the caller through `editor_to_source`. Whether a clip has a
 * camera is Rust's `clip_camera` answer (`view.clipCameras`), never derived here.
 */
import { snap, type Snapping } from "./timeline-snap";
import { CAMERA_ROUNDNESS_RECTANGLE, CAMERA_ROUNDNESS_SQUARE } from "./editor";
import type {
  RecordCameraAnchor,
  RecordCameraMask,
  RecordCameraStyle,
  RecordCropRect,
  RecordEditDoc,
  RecordEditorView,
  RecordLayoutPreset,
  RecordLayoutRegion,
} from "./ipc";
import type { MessageKey } from "./i18n";

/** The layout picker's order (upstream `LayoutPicker`). */
export const LAYOUT_PRESETS: readonly RecordLayoutPreset[] = [
  "picture-in-picture",
  "dual-frame",
  "vertical-stack",
  "camera-only",
  "no-webcam",
];

/** A preset's name. */
export const PRESET_LABEL: Record<RecordLayoutPreset, MessageKey> = {
  "picture-in-picture": "rec5.cam.preset.picture-in-picture",
  "dual-frame": "rec5.cam.preset.dual-frame",
  "vertical-stack": "rec5.cam.preset.vertical-stack",
  "camera-only": "rec5.cam.preset.camera-only",
  "no-webcam": "rec5.cam.preset.no-webcam",
};

/** The position grid: 3×3 with the middle left out (the camera never covers the centre). */
export const ANCHOR_GRID: readonly (readonly (RecordCameraAnchor | null)[])[] = [
  ["top-left", "top", "top-right"],
  ["left", null, "right"],
  ["bottom-left", "bottom", "bottom-right"],
];

/** The tightest framing: a third of the picture (300 % zoom, upstream's limit). */
export const MIN_FRAMING_SIZE = 1 / 3;

/** The rounding a mask starts with. */
export function roundnessDefault(mask: RecordCameraMask): number {
  return mask === "square" ? CAMERA_ROUNDNESS_SQUARE : CAMERA_ROUNDNESS_RECTANGLE;
}

/** The rounding in use: the stored one or the mask's standard. */
export function effectiveRoundness(style: Pick<RecordCameraStyle, "roundness" | "mask">): number {
  return style.roundness ?? roundnessDefault(style.mask);
}

/** Another mask: an untouched rounding (none stored, or the old mask's standard) follows it. */
export function withMask(style: RecordCameraStyle, mask: RecordCameraMask): RecordCameraStyle {
  const untouched = style.roundness == null || Math.abs(style.roundness - roundnessDefault(style.mask)) < 1e-9;
  return { ...style, mask, roundness: untouched ? null : style.roundness };
}

/** "Shrink during a zoom" exists for picture-in-picture only (a block layout would tear). */
export function canShrink(preset: RecordLayoutPreset): boolean {
  return preset === "picture-in-picture";
}

/** Whether any clip has a camera (Rust's per-clip answer). */
export function hasAnyCamera(view: Pick<RecordEditorView, "clipCameras">): boolean {
  return (view.clipCameras ?? []).some(Boolean);
}

type SpanView = Pick<RecordEditorView, "doc" | "layoutSpans">;

/** The layout region in force at programme ms `programmeMs` (Rust's spans), or `null`. */
export function regionAt(view: SpanView, programmeMs: number): RecordLayoutRegion | null {
  const p = passageAt(view, programmeMs);
  return p ? (view.doc.layoutRegions.find((r) => r.id === p.id) ?? null) : null;
}

/** The region's passage on the programme clock under `programmeMs`, or `null` (the whole-video preset). */
export function passageAt(view: SpanView, programmeMs: number): { id: string; progStartMs: number; progEndMs: number } | null {
  for (const s of view.layoutSpans ?? []) {
    const r = view.doc.layoutRegions[s.index];
    if (r && programmeMs >= s.progStartMs && programmeMs < s.progEndMs) {
      return { id: r.id, progStartMs: s.progStartMs, progEndMs: s.progEndMs };
    }
  }
  return null;
}

/** The layout requested at `programmeMs`: its region's or the whole video's. */
export function presetAt(view: SpanView, programmeMs: number): RecordLayoutPreset {
  return regionAt(view, programmeMs)?.preset ?? view.doc.camera.preset;
}

export function setLayoutRegionPreset(doc: RecordEditDoc, id: string, preset: RecordLayoutPreset): RecordEditDoc {
  if (!doc.layoutRegions.some((r) => r.id === id && r.preset !== preset)) return doc;
  return { ...doc, layoutRegions: doc.layoutRegions.map((r) => (r.id === id ? { ...r, preset } : r)) };
}

/** Region `id` starts at `startMs` (SOURCE ms, kept inside its clip). A region already starting
 *  there is replaced; no move = the same doc (no undo step). */
export function moveLayoutRegion(doc: RecordEditDoc, id: string, startMs: number): RecordEditDoc {
  const r = doc.layoutRegions.find((x) => x.id === id);
  const c = r ? doc.clips[r.clip] : undefined;
  if (!r || !c) return doc;
  const at = Math.min(c.endMs - 1, Math.max(c.startMs, Math.round(startMs)));
  if (at === r.startMs) return doc;
  return {
    ...doc,
    layoutRegions: doc.layoutRegions
      .filter((x) => x.id === id || !(x.clip === r.clip && x.startMs === at))
      .map((x) => (x.id === id ? { ...x, startMs: at } : x)),
  };
}

/** A region's start dragged by `deltaMs`: inside its clip (before its end), snapped within 8 px. */
export function layoutStartProg(
  span: { progStartMs: number },
  clip: { progStartMs: number; progEndMs: number },
  deltaMs: number,
  snapping?: Snapping,
): number {
  const clamp = (p: number) => Math.min(clip.progEndMs - 1, Math.max(clip.progStartMs, p));
  const raw = clamp(span.progStartMs + deltaMs);
  if (!snapping) return raw;
  const s = snap(raw, snapping.targets, snapping.pxPerMs);
  return s.target == null ? raw : clamp(s.ms);
}

/** Framing (zoom + pan, the way the control is used) → the doc's `crop`. Full size = `null`. */
export function cropFromFraming(size: number, pan: { x: number; y: number }): RecordCropRect | null {
  const s = Math.min(1, Math.max(MIN_FRAMING_SIZE, size));
  if (s >= 1) return null;
  const c = (v: number) => Math.min(1, Math.max(0, v));
  return { x: c(pan.x) * (1 - s), y: c(pan.y) * (1 - s), w: s, h: s };
}

/** The doc's `crop` → zoom + pan (a square framing; the whole frame = centred, size 1). */
export function framingFromCrop(crop: RecordCropRect | null): { size: number; pan: { x: number; y: number } } {
  if (!crop || crop.w >= 1) return { size: 1, pan: { x: 0.5, y: 0.5 } };
  const s = crop.w;
  const p = (v: number) => (s < 1 ? Math.min(1, Math.max(0, v / (1 - s))) : 0.5);
  return { size: s, pan: { x: p(crop.x), y: p(crop.y) } };
}
