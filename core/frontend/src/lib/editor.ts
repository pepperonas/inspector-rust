/**
 * Pure helpers of the `record` editor window (stage 2).
 *
 * The programme ↔ source mapping lives in Rust (`record/editor.rs`, ruling R3): every
 * document change goes through `editor_update`, which answers with the normalised doc and
 * the programme spans of each clip and speed region. Nothing here re-derives those spans;
 * the edits below work in SOURCE time; `clipAt` only reads the clip spans Rust returned, and
 * the audio preview cues from the source position Rust reports with the playhead.
 */
import type { RecordAspect, RecordCropRect, RecordEditDoc, RecordEditorView } from "./ipc";

/** Clips shorter than this are dropped by Rust's `normalize` — a split never makes one. */
export const MIN_CLIP_MS = 100;
export const MIN_SPEED = 0.25;
export const MAX_SPEED = 16;
export const MIN_MIC_DB = -60;
export const MAX_MIC_DB = 12;
export const HISTORY_MAX = 50;
/** Speed values offered in the band menu. */
export const SPEED_STEPS = [0.25, 0.5, 0.75, 1.5, 2, 4, 8, 16] as const;
/** Crop presets in chip order; `free` keeps the output at the source aspect. */
export const CROP_PRESETS = ["free", "16:9", "9:16", "1:1", "4:3", "3:4", "21:9"] as const;
export type CropPreset = (typeof CROP_PRESETS)[number];

export const ASPECT_RATIO: Record<Exclude<RecordAspect, "source">, number> = {
  "16:9": 16 / 9,
  "9:16": 9 / 16,
  "1:1": 1,
  "4:3": 4 / 3,
  "3:4": 3 / 4,
  "21:9": 21 / 9,
};

const clampSpeed = (s: number) => (Number.isFinite(s) ? Math.min(MAX_SPEED, Math.max(MIN_SPEED, s)) : 1);

/** Splits clip `clip` at `sourceMs` (from `editor_seek`); regions and crop follow the halves. */
export function splitAt(doc: RecordEditDoc, clip: number, sourceMs: number): RecordEditDoc {
  const c = doc.clips[clip];
  if (!c || sourceMs - c.startMs < MIN_CLIP_MS || c.endMs - sourceMs < MIN_CLIP_MS) return doc;
  const at = Math.round(sourceMs);
  const clips = [
    ...doc.clips.slice(0, clip),
    { startMs: c.startMs, endMs: at },
    { startMs: at, endMs: c.endMs },
    ...doc.clips.slice(clip + 1),
  ];
  const speed = doc.speed.flatMap((r) => {
    if (r.clip < clip) return [r];
    if (r.clip > clip) return [{ ...r, clip: r.clip + 1 }];
    if (r.endMs <= at) return [r];
    if (r.startMs >= at) return [{ ...r, clip: clip + 1 }];
    return [
      { ...r, endMs: at },
      { ...r, clip: clip + 1, startMs: at },
    ];
  });
  const crop = [...doc.crop];
  while (crop.length < doc.clips.length) crop.push(null);
  crop.splice(clip + 1, 0, crop[clip] ?? null);
  return { ...doc, clips, speed, crop };
}

/** Removes a clip with its regions and crop. The last clip is never removed. */
export function deleteClip(doc: RecordEditDoc, clip: number): RecordEditDoc {
  if (doc.clips.length <= 1 || !doc.clips[clip]) return doc;
  return {
    ...doc,
    clips: doc.clips.filter((_, i) => i !== clip),
    speed: doc.speed.filter((r) => r.clip !== clip).map((r) => (r.clip > clip ? { ...r, clip: r.clip - 1 } : r)),
    crop: doc.crop.filter((_, i) => i !== clip),
  };
}

/** New source bounds for a clip; Rust clamps them into the recording. */
export function setTrim(doc: RecordEditDoc, clip: number, startMs: number, endMs: number): RecordEditDoc {
  if (!doc.clips[clip]) return doc;
  const a = Math.max(0, Math.round(Math.min(startMs, endMs)));
  const b = Math.max(0, Math.round(Math.max(startMs, endMs)));
  return { ...doc, clips: doc.clips.map((c, i) => (i === clip ? { startMs: a, endMs: b } : c)) };
}

/** Appends a speed region in source time; a later region wins an overlap (Rust resolves). */
export function addSpeed(doc: RecordEditDoc, clip: number, startMs: number, endMs: number, speed: number): RecordEditDoc {
  const a = Math.round(Math.min(startMs, endMs));
  const b = Math.round(Math.max(startMs, endMs));
  return { ...doc, speed: [...doc.speed, { clip, startMs: a, endMs: b, speed: clampSpeed(speed) }] };
}

/** Sets one region's value (drag or menu). */
export function setSpeedValue(doc: RecordEditDoc, index: number, speed: number): RecordEditDoc {
  if (!doc.speed[index]) return doc;
  return { ...doc, speed: doc.speed.map((r, i) => (i === index ? { ...r, speed: clampSpeed(speed) } : r)) };
}

export function removeSpeed(doc: RecordEditDoc, index: number): RecordEditDoc {
  return { ...doc, speed: doc.speed.filter((_, i) => i !== index) };
}

export function setCrop(doc: RecordEditDoc, clip: number, rect: RecordCropRect | null): RecordEditDoc {
  const crop = [...doc.crop];
  while (crop.length < doc.clips.length) crop.push(null);
  crop[clip] = rect;
  return { ...doc, crop };
}

export function setMicGain(doc: RecordEditDoc, db: number): RecordEditDoc {
  return { ...doc, micGainDb: Math.min(MAX_MIC_DB, Math.max(MIN_MIC_DB, db)) };
}

/** The largest centred rect of `aspect` inside a srcW×srcH frame, normalised to 0…1. */
export function cropForPreset(aspect: RecordAspect, srcW: number, srcH: number): RecordCropRect {
  if (aspect === "source" || srcW <= 0 || srcH <= 0) return { x: 0, y: 0, w: 1, h: 1 };
  const r = ASPECT_RATIO[aspect];
  if (r >= srcW / srcH) {
    const h = srcW / r / srcH;
    return { x: 0, y: (1 - h) / 2, w: 1, h };
  }
  const w = (srcH * r) / srcW;
  return { x: (1 - w) / 2, y: 0, w, h: 1 };
}

/** Undo/redo over whole documents (at most {@link HISTORY_MAX} undo steps). */
export class History {
  private past: RecordEditDoc[] = [];
  private future: RecordEditDoc[] = [];
  constructor(private current: RecordEditDoc) {}
  get present(): RecordEditDoc {
    return this.current;
  }
  push(doc: RecordEditDoc): void {
    this.past.push(this.current);
    if (this.past.length > HISTORY_MAX) this.past.shift();
    this.current = doc;
    this.future = [];
  }
  /** Replaces the present without a step (Rust's normalised copy of the same edit). */
  replace(doc: RecordEditDoc): void {
    this.current = doc;
  }
  undo(): RecordEditDoc | null {
    const prev = this.past.pop();
    if (!prev) return null;
    this.future.push(this.current);
    this.current = prev;
    return prev;
  }
  redo(): RecordEditDoc | null {
    const next = this.future.pop();
    if (!next) return null;
    this.past.push(this.current);
    this.current = next;
    return next;
  }
  canUndo(): boolean {
    return this.past.length > 0;
  }
  canRedo(): boolean {
    return this.future.length > 0;
  }
}

/** `m:ss.cc`, or `h:mm:ss.cc` from one hour. */
export function formatTimecode(ms: number): string {
  const total = Math.max(0, Math.floor((Number.isFinite(ms) ? ms : 0) / 10));
  const cs = total % 100;
  const s = Math.floor(total / 100) % 60;
  const m = Math.floor(total / 6000) % 60;
  const h = Math.floor(total / 360_000);
  const p2 = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${p2(m)}:${p2(s)}.${p2(cs)}` : `${m}:${p2(s)}.${p2(cs)}`;
}

/** The clip whose programme span holds `t` (the end of the programme belongs to the last). */
export function clipAt(view: RecordEditorView, t: number): number {
  let found = 0;
  view.clips.forEach((c, i) => {
    if (c.progStartMs <= t) found = i;
  });
  return found;
}

/**
 * Source position of a trim handle dragged to programme time `prog` (ruling R12: the mapping
 * is Rust's). Inside the clip's programme span `toSource` (`editor_to_source`) answers. Past
 * the span the handle uncovers source the clip does not play yet; Rust's `normalize` keeps
 * every speed region inside its clip, so uncovered source has no region and runs at 1× —
 * the overshoot in programme ms is the overshoot in source ms.
 */
export async function trimSourceAt(
  clip: number,
  edge: "start" | "end",
  span: { progStartMs: number; progEndMs: number },
  bounds: { startMs: number; endMs: number },
  prog: number,
  toSource: (programmeMs: number) => Promise<{ clip: number; sourceMs: number } | null>,
): Promise<number> {
  const p = Math.round(prog);
  if (p <= span.progStartMs) return Math.max(0, bounds.startMs - (span.progStartMs - p));
  if (p >= span.progEndMs) return bounds.endMs + (p - span.progEndMs);
  const at = await toSource(p);
  if (at && at.clip === clip) return at.sourceMs;
  return edge === "start" ? bounds.startMs : bounds.endMs;
}

/** Preview render size: `ratio` fitted into the panel, scaled down to the cap. */
export function fitPreview(pw: number, ph: number, ratio: number, maxW: number, maxH: number): { w: number; h: number } {
  if (pw <= 0 || ph <= 0 || !(ratio > 0)) return { w: 16, h: 16 };
  let w = Math.min(pw, ph * ratio);
  let h = w / ratio;
  const s = Math.min(1, maxW / w, maxH / h);
  w *= s;
  h *= s;
  return { w: Math.max(16, Math.round(w)), h: Math.max(16, Math.round(h)) };
}

/** Output aspect as a number (`source` = the recording's). */
export function aspectRatio(aspect: RecordAspect, srcW: number, srcH: number): number {
  return aspect === "source" ? (srcH > 0 ? srcW / srcH : 16 / 9) : ASPECT_RATIO[aspect];
}

/** Mic gain in dB as an `<audio>` volume (the element cannot boost above 1). */
export function dbToVolume(db: number): number {
  return Math.min(1, Math.max(0, 10 ** (db / 20)));
}

/** True when a key event belongs to a text field and must not drive the editor. */
export function isTypingTarget(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  const tag = t.tagName;
  if (tag === "TEXTAREA" || tag === "SELECT" || t.isContentEditable) return true;
  if (tag === "INPUT") {
    const type = (t as HTMLInputElement).type;
    return type !== "range" && type !== "checkbox" && type !== "radio" && type !== "button";
  }
  return false;
}
