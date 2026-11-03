/**
 * Pure helpers of the `record` editor window (stage 2).
 *
 * The programme ↔ source mapping lives in Rust (`record/editor.rs`, ruling R3): every
 * document change goes through `editor_update`, which answers with the normalised doc and
 * the programme spans of each clip and speed region. Nothing here re-derives those spans;
 * the edits below work in SOURCE time; `clipAt` only reads the clip spans Rust returned, and
 * the audio preview cues from the source position Rust reports with the playhead.
 */
import { snapShift, type Snapping } from "./timeline-snap";
import type {
  RecordAnnotation,
  RecordAnnotationBody,
  RecordAnnotationKind,
  RecordAudioTrack,
  RecordTrackKind,
  RecordAppearance,
  RecordAspect,
  RecordBackground,
  RecordCropRect,
  RecordCursorStyle,
  RecordEditDoc,
  RecordEditorView,
  RecordTakeMeta,
  RecordZoomRegion,
} from "./ipc";

/** Clips shorter than this are dropped by Rust's `normalize` — a split never makes one. */
export const MIN_CLIP_MS = 100;
export const MIN_SPEED = 0.25;
export const MAX_SPEED = 16;
export const MIN_MIC_DB = -60;
export const MAX_MIC_DB = 12;
export const HISTORY_MAX = 50;
/** Zoom depth 1…6 → scale (mirrors `ZOOM_DEPTHS` in record/editor.rs; a test pins it). */
export const ZOOM_DEPTHS = [1.25, 1.5, 1.8, 2.2, 3.5, 5.0] as const;
export const DEFAULT_ZOOM_DEPTH = 3;
export const MIN_ZOOM_MS = 250;
export const NEW_ZOOM_MS = 2000;
/** Speed values offered in the band menu. */
export const SPEED_STEPS = [0.25, 0.5, 0.75, 1.5, 2, 4, 8, 16] as const;
/** Crop presets in chip order; `free` keeps the output at the source aspect. */
export const CROP_PRESETS = ["free", "16:9", "9:16", "1:1", "4:3", "3:4", "21:9"] as const;
export type CropPreset = (typeof CROP_PRESETS)[number];

export const ASPECT_RATIO: Record<Exclude<RecordAspect, "source" | "auto">, number> = {
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
    { ...c, startMs: c.startMs, endMs: at },
    { ...c, startMs: at, endMs: c.endMs },
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
  // A zoom belongs to the half holding its start and is cut at the edge (Rust re-clamps).
  const zooms = doc.zooms.map((z) => {
    if (z.clip < clip) return z;
    if (z.clip > clip) return { ...z, clip: z.clip + 1 };
    return z.startMs >= at ? { ...z, clip: clip + 1 } : { ...z, endMs: Math.min(z.endMs, at) };
  });
  // Annotations like zooms; a track's anchor follows the half holding its start.
  const annotations = doc.annotations.map((a) => {
    if (a.clip < clip) return a;
    if (a.clip > clip) return { ...a, clip: a.clip + 1 };
    return a.startMs >= at ? { ...a, clip: clip + 1 } : { ...a, endMs: Math.min(a.endMs, at) };
  });
  const audioTracks = doc.audioTracks.map((t) =>
    t.clip > clip || (t.clip === clip && t.startMs >= at) ? { ...t, clip: t.clip + 1 } : t,
  );
  const crop = [...doc.crop];
  while (crop.length < doc.clips.length) crop.push(null);
  crop.splice(clip + 1, 0, crop[clip] ?? null);
  return { ...doc, clips, speed, crop, zooms, annotations, audioTracks };
}

/** Removes a clip with its regions and crop. The last clip is never removed. */
export function deleteClip(doc: RecordEditDoc, clip: number): RecordEditDoc {
  if (doc.clips.length <= 1 || !doc.clips[clip]) return doc;
  return {
    ...doc,
    clips: doc.clips.filter((_, i) => i !== clip),
    speed: doc.speed.filter((r) => r.clip !== clip).map((r) => (r.clip > clip ? { ...r, clip: r.clip - 1 } : r)),
    crop: doc.crop.filter((_, i) => i !== clip),
    zooms: doc.zooms.filter((z) => z.clip !== clip).map((z) => (z.clip > clip ? { ...z, clip: z.clip - 1 } : z)),
    annotations: doc.annotations
      .filter((a) => a.clip !== clip)
      .map((a) => (a.clip > clip ? { ...a, clip: a.clip - 1 } : a)),
    audioTracks: doc.audioTracks
      .filter((t) => t.clip !== clip)
      .map((t) => (t.clip > clip ? { ...t, clip: t.clip - 1 } : t)),
  };
}

/** New source bounds for a clip; Rust clamps them into the recording. */
export function setTrim(doc: RecordEditDoc, clip: number, startMs: number, endMs: number): RecordEditDoc {
  if (!doc.clips[clip]) return doc;
  const a = Math.max(0, Math.round(Math.min(startMs, endMs)));
  const b = Math.max(0, Math.round(Math.max(startMs, endMs)));
  return { ...doc, clips: doc.clips.map((c, i) => (i === clip ? { ...c, startMs: a, endMs: b } : c)) };
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

/** A new manual zoom: 2 s from `sourceMs`, clamped into the clip; Rust drops it if under 250 ms. */
export function addZoom(doc: RecordEditDoc, clip: number, sourceMs: number): RecordEditDoc {
  const c = doc.clips[clip];
  if (!c) return doc;
  const startMs = Math.min(c.endMs, Math.max(c.startMs, Math.round(sourceMs)));
  const zoom: RecordZoomRegion = {
    id: crypto.randomUUID(),
    clip,
    startMs,
    endMs: Math.min(c.endMs, startMs + NEW_ZOOM_MS),
    depth: DEFAULT_ZOOM_DEPTH,
    customScale: null,
    focusX: 0.5,
    focusY: 0.5,
    focusMode: "auto",
    origin: "manual",
    hideCursor: false,
  };
  return { ...doc, zooms: [...doc.zooms, zoom] };
}

export function updateZoom(doc: RecordEditDoc, id: string, patch: Partial<Omit<RecordZoomRegion, "id">>): RecordEditDoc {
  if (!doc.zooms.some((z) => z.id === id)) return doc;
  return { ...doc, zooms: doc.zooms.map((z) => (z.id === id ? { ...z, ...patch } : z)) };
}

export function removeZoom(doc: RecordEditDoc, id: string): RecordEditDoc {
  return { ...doc, zooms: doc.zooms.filter((z) => z.id !== id) };
}

/** Removes the planner's zooms; the ones the user made stay. */
export function removeAutoZooms(doc: RecordEditDoc): RecordEditDoc {
  return { ...doc, zooms: doc.zooms.filter((z) => z.origin !== "auto") };
}

export function setAppearance(doc: RecordEditDoc, patch: Partial<RecordAppearance>): RecordEditDoc {
  return { ...doc, appearance: { ...doc.appearance, ...patch } };
}

export function setCursorStyle(doc: RecordEditDoc, patch: Partial<RecordCursorStyle>): RecordEditDoc {
  return { ...doc, cursor: { ...doc.cursor, ...patch } };
}

export function setMicGain(doc: RecordEditDoc, db: number): RecordEditDoc {
  return { ...doc, micGainDb: Math.min(MAX_MIC_DB, Math.max(MIN_MIC_DB, db)) };
}

/** The largest centred rect of `aspect` inside a srcW×srcH frame, normalised to 0…1. */
export function cropForPreset(aspect: RecordAspect, srcW: number, srcH: number): RecordCropRect {
  if (aspect === "source" || aspect === "auto" || srcW <= 0 || srcH <= 0) return { x: 0, y: 0, w: 1, h: 1 };
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

/** The recording a clip plays: its folder inside the project and what is in it. */
export interface ClipMedia {
  /** Relative folder: "" = the project's own take, else `takes/<id>`. */
  dir: string;
  width: number;
  height: number;
  systemAudio: boolean;
  hasMic: boolean;
  micOffsetMs: number;
  /** An inserted take that is not available right now (the clip plays for no time). */
  missing: boolean;
}

/**
 * The recording behind clip `clip`: the project's own (`main`) or the inserted take its
 * `source` names (from `view.takes`, Rust's list of takes that are there).
 */
export function clipMedia(
  doc: RecordEditDoc,
  takes: Record<string, RecordTakeMeta> | undefined,
  clip: number,
  main: { width: number; height: number; systemAudio: boolean; micOffsetMs: number | null },
): ClipMedia {
  const id = doc.clips[clip]?.source ?? null;
  const own: ClipMedia = {
    dir: "",
    width: main.width,
    height: main.height,
    systemAudio: main.systemAudio,
    hasMic: main.micOffsetMs != null,
    micOffsetMs: main.micOffsetMs ?? 0,
    missing: false,
  };
  if (!id) return own;
  const t = takes?.[id];
  if (!t) return { ...own, systemAudio: false, hasMic: false, missing: true };
  return {
    dir: `takes/${id}`,
    width: t.width || main.width,
    height: t.height || main.height,
    systemAudio: t.systemAudio,
    hasMic: t.hasMic,
    micOffsetMs: t.micOffsetMs,
    missing: false,
  };
}

/**
 * The gap a dragged clip would land in: 0 = before the first clip, `clips.length` = after the
 * last. Decided by clip middles (programme ms from Rust's view), so the mark jumps half-way.
 */
export function clipDropGap(clips: ProgSpan[], progMs: number): number {
  const i = clips.findIndex((c) => progMs < (c.progStartMs + c.progEndMs) / 2);
  return i < 0 ? clips.length : i;
}

/**
 * The index clip `from` ends up at when dropped into `gap` (the `to` of `editor_move_clip`:
 * remove, then insert) — `null` when the drop changes nothing (its own gaps).
 */
export function clipMoveTarget(from: number, gap: number, count: number): number | null {
  if (from < 0 || from >= count || gap < 0 || gap > count) return null;
  if (gap === from || gap === from + 1) return null;
  return gap > from ? gap - 1 : gap;
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

/**
 * Shape of the preview box and of the helper's render size: Rust's 1080p output size
 * (`EditorView.outputSize`). Under `auto` padding, frame and crop change that shape — the UI
 * never re-derives it (Rust is the only owner of the rule). Without a size: the source shape.
 */
export function previewRatio(view: Pick<RecordEditorView, "outputSize">, srcW: number, srcH: number): number {
  const [w, h] = view.outputSize ?? [0, 0];
  if (w > 0 && h > 0) return w / h;
  return srcH > 0 ? srcW / srcH : 16 / 9;
}

/** Output aspect as a number (`source` = the recording's). */
export function aspectRatio(aspect: RecordAspect, srcW: number, srcH: number): number {
  return aspect === "source" || aspect === "auto" ? (srcH > 0 ? srcW / srcH : 16 / 9) : ASPECT_RATIO[aspect];
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

// ── inspector (stage 3) ─────────────────────────────────────────────────────

/** Slider ranges; they mirror the clamps of Rust's `normalize` (a test pins them). */
export const INSPECTOR_RANGES = {
  padding: { min: 0, max: 100, step: 1 },
  radiusPx: { min: 0, max: 64, step: 1 },
  shadow: { min: 0, max: 1, step: 0.05 },
  backgroundBlur: { min: 0, max: 1, step: 0.05 },
  motionBlur: { min: 0, max: 1, step: 0.05 },
  cursorSize: { min: 1.5, max: 6, step: 0.1 },
  smoothing: { min: 0, max: 1, step: 0.05 },
  cursorMotionBlur: { min: 0, max: 1, step: 0.05 },
  clickBounce: { min: 0, max: 2, step: 1 },
  clickSoundGainDb: { min: -24, max: 0, step: 1 },
  hotspot: { min: 0, max: 1, step: 0.01 },
  zoomScale: { min: 1, max: 5, step: 0.05 },
} as const;
export type InspectorSetting = keyof typeof INSPECTOR_RANGES;

/** `v` inside the setting's range; a non-number becomes the minimum. */
export function clampSetting(key: InspectorSetting, v: number): number {
  const r = INSPECTOR_RANGES[key];
  if (!Number.isFinite(v)) return r.min;
  return Math.min(r.max, Math.max(r.min, v));
}

/** Named shadow levels (upstream chips). */
export const SHADOW_LEVELS = [0, 0.3, 0.6, 0.9] as const;
/** Named click-bounce levels. */
export const BOUNCE_LEVELS = [0, 1, 2] as const;

/** The 16 upstream gradient presets, `linear-gradient(135deg, a, b)`. */
export const GRADIENT_PRESETS: readonly Extract<RecordBackground, { kind: "gradient" }>[] = (
  [
    ["#eaebed", "#bcc0c6"],
    ["#3b82f6", "#8b5cf6"],
    ["#8b5cf6", "#ec4899"],
    ["#f97316", "#ec4899"],
    ["#f59e0b", "#f97316"],
    ["#10b981", "#3b82f6"],
    ["#22c55e", "#10b981"],
    ["#6b7280", "#16171d"],
    ["#ec4899", "#ef4444"],
    ["#3b82f6", "#22c55e"],
    ["#8b5cf6", "#3b82f6"],
    ["#f59e0b", "#ef4444"],
    ["#16171d", "#1e293b"],
    ["#34d399", "#3b82f6"],
    ["#ef4444", "#8b5cf6"],
    ["#bcc0c6", "#eaebed"],
  ] as const
).map(([a, b]) => ({ kind: "gradient" as const, angleDeg: 135, stops: [a, b] }));

/** The 16 upstream solid colours. */
export const COLOR_PRESETS = [
  "#16171d", "#6b7280", "#bcc0c6", "#eaebed", "#ffffff", "#10b981", "#0ea371", "#34d399",
  "#f59e0b", "#ef4444", "#3b82f6", "#8b5cf6", "#ec4899", "#f97316", "#22c55e", "#1e293b",
] as const;

/** Background motion only animates wallpapers, gradients and images (the compositor rule). */
export function motionAllowed(bg: RecordBackground): boolean {
  return bg.kind !== "color";
}

/** Live slider updates reach Rust at most this often (like the mic level). */
export const LIVE_SEND_MS = 150;

/**
 * One slider gesture: every move replaces the present document (no undo step) and reaches
 * Rust debounced; the release records ONE step from the document before the drag.
 */
export class LiveGesture {
  private start: RecordEditDoc | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private sentLive = false;
  constructor(
    private history: () => History | null,
    private send: (doc: RecordEditDoc, record: boolean) => void,
    private delayMs: number = LIVE_SEND_MS,
  ) {}
  get active(): boolean {
    return this.start != null;
  }
  move(next: RecordEditDoc): void {
    const h = this.history();
    if (!h) return;
    this.start ??= h.present;
    h.replace(next);
    if (this.timer != null) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      this.sentLive = true;
      this.send(h.present, false);
    }, this.delayMs);
  }
  /** Drops a pending live send (the editor unmounts). */
  cancel(): void {
    if (this.timer != null) clearTimeout(this.timer);
    this.timer = null;
    this.start = null;
    this.sentLive = false;
  }
  end(): void {
    if (this.timer != null) clearTimeout(this.timer);
    this.timer = null;
    const h = this.history();
    const start = this.start;
    const sentLive = this.sentLive;
    this.start = null;
    this.sentLive = false;
    if (!h || !start) return;
    const final = h.present;
    h.replace(start);
    if (JSON.stringify(final) !== JSON.stringify(start)) this.send(final, true);
    // Rust saw a live value: put it back to where the drag began.
    else if (sentLive) this.send(start, false);
  }
}

// ── zoom track (stage 3, Task 7) ────────────────────────────────────────────

/** `1.25×` — the factor a zoom depth or custom scale stands for. */
export function fmtZoom(scale: number): string {
  return `${Number(scale.toFixed(2))}×`;
}

/** The scale a region zooms to (mirrors Rust's `zoom_scale`). */
export function zoomScale(z: Pick<RecordZoomRegion, "depth" | "customScale">): number {
  if (z.customScale != null && Number.isFinite(z.customScale)) return Math.min(5, Math.max(1, z.customScale));
  return ZOOM_DEPTHS[Math.min(6, Math.max(1, Math.round(z.depth))) - 1];
}

type ProgSpan = { progStartMs: number; progEndMs: number };

/**
 * A zoom block dragged by `deltaMs` of programme time: the block keeps its length and stays
 * inside its clip's programme span (a drag past the clip edge clamps there). With `snapping`
 * the start or end — whichever is nearer — snaps onto a target first (programme ms only; the
 * caller maps the result to source through Rust).
 */
export function moveZoomProg(span: ProgSpan, clip: ProgSpan, deltaMs: number, snapping?: Snapping): ProgSpan {
  const at = (d: number) => {
    const len = Math.min(span.progEndMs - span.progStartMs, clip.progEndMs - clip.progStartMs);
    const start = Math.min(clip.progEndMs - len, Math.max(clip.progStartMs, span.progStartMs + d));
    return { progStartMs: start, progEndMs: start + len };
  };
  const raw = at(deltaMs);
  if (!snapping) return raw;
  const { shiftMs } = snapShift([raw.progStartMs, raw.progEndMs], snapping.targets, snapping.pxPerMs);
  return shiftMs ? at(deltaMs + shiftMs) : raw;
}

/** A zoom edge dragged by `deltaMs`: inside the clip, never shorter than `MIN_ZOOM_MS`; snaps like `moveZoomProg`. */
export function trimZoomProg(span: ProgSpan, clip: ProgSpan, edge: "start" | "end", deltaMs: number, snapping?: Snapping): ProgSpan {
  const at = (d: number): ProgSpan => {
    if (edge === "start") {
      const start = Math.min(span.progEndMs - MIN_ZOOM_MS, Math.max(clip.progStartMs, span.progStartMs + d));
      return { progStartMs: start, progEndMs: span.progEndMs };
    }
    const end = Math.max(span.progStartMs + MIN_ZOOM_MS, Math.min(clip.progEndMs, span.progEndMs + d));
    return { progStartMs: span.progStartMs, progEndMs: end };
  };
  const raw = at(deltaMs);
  if (!snapping) return raw;
  const moved = edge === "start" ? raw.progStartMs : raw.progEndMs;
  const { shiftMs } = snapShift([moved], snapping.targets, snapping.pxPerMs);
  return shiftMs ? at(deltaMs + shiftMs) : raw;
}

/**
 * Source position of programme time `prog` inside `clip` (ruling R12: Rust maps). At or past
 * the clip's programme edges the clip's own source edge answers — `editor_to_source` at a
 * boundary may already name the next clip.
 */
export async function zoomSourceAt(
  clip: number,
  span: ProgSpan,
  bounds: { startMs: number; endMs: number },
  prog: number,
  toSource: (programmeMs: number) => Promise<{ clip: number; sourceMs: number } | null>,
): Promise<number> {
  const p = Math.round(prog);
  if (p <= span.progStartMs) return bounds.startMs;
  if (p >= span.progEndMs) return bounds.endMs;
  const at = await toSource(p);
  if (at && at.clip === clip) return Math.min(bounds.endMs, Math.max(bounds.startMs, at.sourceMs));
  return bounds.endMs;
}

/** Click hits that fall into `(fromMs, toMs]` of the programme clock. */
export function dueClickHits<T extends { progMs: number }>(hits: readonly T[], fromMs: number, toMs: number): T[] {
  return hits.filter((h) => h.progMs > fromMs && h.progMs <= toMs);
}

/** Element volume of a click: its own gain × the user's dB (the export's level), at most 1. */
export function clickVolume(gain: number, db: number): number {
  return Math.min(1, Math.max(0, gain * 10 ** (db / 20)));
}

/**
 * Focus point (fractions of the screen) for a click at `(px, py)` (fractions of the output)
 * over the clip's screen box `[x, y, w, h]`; clamped into the screen.
 */
export function focusFromPoint(rect: readonly [number, number, number, number], px: number, py: number): { x: number; y: number } {
  const [x, y, w, h] = rect;
  const f = (v: number, o: number, l: number) => (l > 0 ? Math.min(1, Math.max(0, (v - o) / l)) : 0.5);
  return { x: f(px, x, w), y: f(py, y, h) };
}

// ── annotations + audio tracks (stage 4) ────────────────────────────────────
// Mirrors of the constants in record/editor.rs (a test reads that file and pins them).

export const TEXT_FONTS = ["Inter", "Lora", "Oswald", "Caveat", "IBM Plex Mono"] as const;
/** A new annotation's box `[x, y, w, h]` in percent of the frame. */
export const ANNOTATION_DEFAULT_RECT = [50, 50, 30, 20] as const;
export const FONT_SIZE_RANGE = [8, 200] as const;
export const DEFAULT_FONT_SIZE = 32;
export const BLUR_INTENSITY_RANGE = [2, 40] as const;
export const BLUR_BLOCK_RANGE = [4, 48] as const;
export const DEFAULT_BLUR_INTENSITY = 12;
export const DEFAULT_BLUR_BLOCK = 12;
export const DEFAULT_TEXT_COLOR = "#ffffff";
export const DEFAULT_TEXT_PLATE = "rgba(0, 0, 0, 0.7)";
export const DEFAULT_ARROW_COLOR = "#34B27B";
export const DEFAULT_ARROW_STROKE = 4;
export const DEFAULT_FREEHAND_POINTS: readonly (readonly [number, number])[] = [
  [10, 30], [25, 10], [55, 8], [82, 20], [90, 45], [78, 72], [52, 90], [22, 84], [8, 58],
];
export const TRACK_GAIN_RANGE = [-60, 12] as const;
export const FADE_MAX_MS = 5000;
export const SYSTEM_GAIN_RANGE = [-12, 12] as const;
export const MUSIC_DEFAULT_GAIN_DB = -18;
export const MUSIC_DEFAULT_FADE_MS = 1000;
/** Length of a freshly added annotation (upstream `DEFAULT_NEW_REGION_SEC`). */
export const NEW_ANNOTATION_MS = 2000;

/** The body a new annotation of `kind` starts with (upstream defaults). */
export function defaultAnnotationBody(kind: RecordAnnotationKind): RecordAnnotationBody {
  switch (kind) {
    case "text":
      return {
        kind: "text",
        content: "",
        color: DEFAULT_TEXT_COLOR,
        background: DEFAULT_TEXT_PLATE,
        fontSize: DEFAULT_FONT_SIZE,
        fontFamily: TEXT_FONTS[0],
        bold: true,
        italic: false,
        underline: false,
        align: "center",
        animation: "none",
      };
    case "image":
      return { kind: "image", file: "" };
    case "arrow":
      return { kind: "arrow", direction: "right", color: DEFAULT_ARROW_COLOR, strokeWidth: DEFAULT_ARROW_STROKE };
    case "blur":
      return {
        kind: "blur",
        style: "mosaic",
        shape: "rectangle",
        color: "white",
        intensity: DEFAULT_BLUR_INTENSITY,
        blockSize: DEFAULT_BLUR_BLOCK,
        points: DEFAULT_FREEHAND_POINTS.map(([x, y]) => [x, y] as [number, number]),
      };
  }
}

/**
 * A new annotation of `kind` at `sourceMs` of `clip` (SOURCE ms), 2 s long and clamped into the
 * clip; the upstream box, `zIndex` above every existing one. Rust clamps the rest.
 */
export function addAnnotation(doc: RecordEditDoc, kind: RecordAnnotationKind, clip: number, sourceMs: number): RecordEditDoc {
  const c = doc.clips[clip];
  if (!c) return doc;
  // At the clip end the box starts earlier so it keeps its length (a clip is ≥ 100 ms long).
  const endMs = Math.min(c.endMs, Math.max(c.startMs, Math.round(sourceMs)) + NEW_ANNOTATION_MS);
  const startMs = Math.max(c.startMs, endMs - NEW_ANNOTATION_MS);
  const [x, y, w, h] = ANNOTATION_DEFAULT_RECT;
  const annotation: RecordAnnotation = {
    id: crypto.randomUUID(),
    clip,
    startMs,
    endMs,
    x,
    y,
    w,
    h,
    zIndex: Math.max(0, ...doc.annotations.map((a) => a.zIndex)) + 1,
    body: defaultAnnotationBody(kind),
  };
  return { ...doc, annotations: [...doc.annotations, annotation] };
}

/** A body patch: fields of one kind, never the kind itself. */
export type AnnotationBodyPatch = {
  [K in RecordAnnotationKind]: Partial<Omit<Extract<RecordAnnotationBody, { kind: K }>, "kind">>;
}[RecordAnnotationKind];

/** Patches one annotation; a `body` patch merges into its body (the kind never changes). */
export function updateAnnotation(
  doc: RecordEditDoc,
  id: string,
  patch: Partial<Omit<RecordAnnotation, "id" | "body">> & { body?: AnnotationBodyPatch },
): RecordEditDoc {
  if (!doc.annotations.some((a) => a.id === id)) return doc;
  const { body, ...rest } = patch;
  return {
    ...doc,
    annotations: doc.annotations.map((a) =>
      a.id === id ? { ...a, ...rest, body: body ? ({ ...a.body, ...body, kind: a.body.kind } as RecordAnnotationBody) : a.body } : a,
    ),
  };
}

export function removeAnnotation(doc: RecordEditDoc, id: string): RecordEditDoc {
  return { ...doc, annotations: doc.annotations.filter((a) => a.id !== id) };
}

/**
 * A new track of `file` (`durationMs` long) starting at `sourceMs` of `clip`. Music: −18 dB with
 * 1 s fades; voiceover: 0 dB, no fades, the recorded length as its span.
 */
export function addTrack(
  doc: RecordEditDoc,
  kind: RecordTrackKind,
  file: string,
  durationMs: number,
  clip: number,
  sourceMs: number,
): RecordEditDoc {
  const c = doc.clips[clip];
  if (!c) return doc;
  const music = kind === "music";
  const duration = Math.max(0, Math.round(durationMs));
  const track: RecordAudioTrack = {
    id: crypto.randomUUID(),
    kind,
    file,
    clip,
    startMs: Math.min(c.endMs, Math.max(c.startMs, Math.round(sourceMs))),
    durationMs: duration,
    offsetMs: 0,
    spanMs: music ? null : duration,
    gainDb: music ? MUSIC_DEFAULT_GAIN_DB : 0,
    looped: false,
    fadeInMs: music ? MUSIC_DEFAULT_FADE_MS : 0,
    fadeOutMs: music ? MUSIC_DEFAULT_FADE_MS : 0,
    muted: false,
    label: "",
  };
  return { ...doc, audioTracks: [...doc.audioTracks, track] };
}

export function updateTrack(doc: RecordEditDoc, id: string, patch: Partial<Omit<RecordAudioTrack, "id">>): RecordEditDoc {
  if (!doc.audioTracks.some((t) => t.id === id)) return doc;
  return { ...doc, audioTracks: doc.audioTracks.map((t) => (t.id === id ? { ...t, ...patch } : t)) };
}

export function removeTrack(doc: RecordEditDoc, id: string): RecordEditDoc {
  return { ...doc, audioTracks: doc.audioTracks.filter((t) => t.id !== id) };
}

export function setSystemGain(doc: RecordEditDoc, db: number): RecordEditDoc {
  const [lo, hi] = SYSTEM_GAIN_RANGE;
  return { ...doc, systemGainDb: Number.isFinite(db) ? Math.min(hi, Math.max(lo, db)) : 0 };
}

// ── annotation editing (stage 4, Task 6) ────────────────────────────────────

/** An annotation box `{x, y, w, h}` in percent (0…100) of its anchor (frame or screen). */
export type AnnBox = { x: number; y: number; w: number; h: number };
/** Smallest box side, percent (Rust's `normalize` clamps w/h to 1…100). */
export const MIN_ANNOTATION_PCT = 1;
/** A free-form blur keeps at least this many points (fewer: Rust falls back to the default). */
export const MIN_FREEHAND_POINTS = 3;
/** What a pointer grabs on the box: its body, or one of the 8 size handles (compass points). */
export type BoxHandle = "move" | "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";
export const BOX_HANDLES: readonly Exclude<BoxHandle, "move">[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

/**
 * `box` after a drag of `(dx, dy)` percent on `handle`. Moving keeps the size and stays inside
 * 0…100; a size handle moves only its own edges — the opposite edge or corner stays put — and
 * stops at the anchor's edge and at `MIN_ANNOTATION_PCT`.
 */
export function dragAnnotationBox(box: AnnBox, handle: BoxHandle, dx: number, dy: number): AnnBox {
  const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
  const w = clamp(box.w, MIN_ANNOTATION_PCT, 100);
  const h = clamp(box.h, MIN_ANNOTATION_PCT, 100);
  if (handle === "move") {
    return { x: clamp(box.x + dx, 0, 100 - w), y: clamp(box.y + dy, 0, 100 - h), w, h };
  }
  let left = box.x;
  let top = box.y;
  let right = box.x + w;
  let bottom = box.y + h;
  if (handle.includes("w")) left = clamp(left + dx, 0, right - MIN_ANNOTATION_PCT);
  if (handle.includes("e")) right = clamp(right + dx, left + MIN_ANNOTATION_PCT, 100);
  if (handle.includes("n")) top = clamp(top + dy, 0, bottom - MIN_ANNOTATION_PCT);
  if (handle.includes("s")) bottom = clamp(bottom + dy, top + MIN_ANNOTATION_PCT, 100);
  return { x: left, y: top, w: right - left, h: bottom - top };
}

/**
 * The annotation Tab (`back`: Shift+Tab) selects next: ordered by programme start (Rust's
 * `annotationSpans`), ties by list order. Nothing selected: the first (Shift: the last).
 * `null` when there are none. Returns the annotation's id.
 */
export function nextAnnotationId(
  doc: Pick<RecordEditDoc, "annotations">,
  spans: readonly { index: number; progStartMs: number }[],
  current: string | null,
  back: boolean,
): string | null {
  const order = [...spans]
    .filter((s) => doc.annotations[s.index])
    .sort((a, b) => a.progStartMs - b.progStartMs || a.index - b.index)
    .map((s) => doc.annotations[s.index].id);
  if (order.length === 0) return null;
  const at = current ? order.indexOf(current) : -1;
  if (at < 0) return back ? order[order.length - 1] : order[0];
  return order[(at + (back ? -1 : 1) + order.length) % order.length];
}

/** Squared distance from `p` to the segment `a–b`. */
function segDist2(p: readonly [number, number], a: readonly [number, number], b: readonly [number, number]): number {
  const vx = b[0] - a[0];
  const vy = b[1] - a[1];
  const len2 = vx * vx + vy * vy;
  const t = len2 > 0 ? Math.min(1, Math.max(0, ((p[0] - a[0]) * vx + (p[1] - a[1]) * vy) / len2)) : 0;
  const dx = p[0] - (a[0] + t * vx);
  const dy = p[1] - (a[1] + t * vy);
  return dx * dx + dy * dy;
}

const clampPct = (v: number) => Math.min(100, Math.max(0, v));

/** The outline with `p` (percent of the box) inserted into its nearest edge (closed polygon). */
export function insertFreehandPoint(points: readonly (readonly [number, number])[], p: readonly [number, number]): [number, number][] {
  const q: [number, number] = [clampPct(p[0]), clampPct(p[1])];
  const out = points.map(([x, y]) => [x, y] as [number, number]);
  if (out.length < 2) return [...out, q];
  let best = 0;
  let bestD = Infinity;
  for (let i = 0; i < out.length; i++) {
    const d = segDist2(q, out[i], out[(i + 1) % out.length]);
    if (d < bestD) {
      bestD = d;
      best = i;
    }
  }
  out.splice(best + 1, 0, q);
  return out;
}

/** Point `i` moved to `p` (clamped into the box). */
export function moveFreehandPoint(points: readonly (readonly [number, number])[], i: number, p: readonly [number, number]): [number, number][] {
  return points.map(([x, y], j) => (j === i ? [clampPct(p[0]), clampPct(p[1])] : [x, y]));
}

/** Point `i` removed — unless that would leave fewer than `MIN_FREEHAND_POINTS` (unchanged then). */
export function removeFreehandPoint(points: readonly (readonly [number, number])[], i: number): [number, number][] {
  const copy = points.map(([x, y]) => [x, y] as [number, number]);
  if (copy.length <= MIN_FREEHAND_POINTS || i < 0 || i >= copy.length) return copy;
  copy.splice(i, 1);
  return copy;
}

/**
 * Where an annotation's box sits in the output, `[x, y, w, h]` in fractions: text, image and
 * arrow measure the frame (the output itself); a blur measures the clip's screen box (Rust's
 * `screenRects`, the unzoomed layout — never re-derived here).
 */
export function annotationOutputRect(
  a: Pick<RecordAnnotation, "x" | "y" | "w" | "h" | "body">,
  screen: readonly [number, number, number, number] | null,
): [number, number, number, number] {
  const [ox, oy, ow, oh] = a.body.kind === "blur" && screen ? screen : [0, 0, 1, 1];
  return [ox + (a.x / 100) * ow, oy + (a.y / 100) * oh, (a.w / 100) * ow, (a.h / 100) * oh];
}

/**
 * Whether a zoom of `clip` covers programme time `progMs` (Rust's zoom spans). A blur follows
 * the zoomed content there, so its box no longer sits where the overlay would draw it.
 */
export function zoomActiveAt(view: Pick<RecordEditorView, "doc" | "zooms">, clip: number, progMs: number): boolean {
  return view.zooms.some((s) => view.doc.zooms[s.index]?.clip === clip && s.progStartMs <= progMs && progMs < s.progEndMs);
}

/** Whether annotation `id` is on screen at programme time `progMs` (Rust's annotation spans). */
export function annotationShownAt(view: Pick<RecordEditorView, "doc" | "annotationSpans">, id: string, progMs: number): boolean {
  return (view.annotationSpans ?? []).some((s) => view.doc.annotations[s.index]?.id === id && s.progStartMs <= progMs && progMs < s.progEndMs);
}

/**
 * Row per span so overlapping annotations stack instead of hiding each other (greedy, by
 * programme start); at most `maxRows` rows — later overlaps share the last row.
 */
export function laneRows(spans: readonly { index: number; progStartMs: number; progEndMs: number }[], maxRows = 3): Map<number, number> {
  const rowEnds: number[] = [];
  const rows = new Map<number, number>();
  for (const s of [...spans].sort((a, b) => a.progStartMs - b.progStartMs || a.index - b.index)) {
    let r = rowEnds.findIndex((end) => end <= s.progStartMs);
    if (r < 0) r = rowEnds.length < maxRows ? rowEnds.length : maxRows - 1;
    rowEnds[r] = Math.max(rowEnds[r] ?? 0, s.progEndMs);
    rows.set(s.index, r);
  }
  return rows;
}
