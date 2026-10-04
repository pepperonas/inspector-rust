// Pure helpers for the `gestures` panel (gesture guard Phase 4): pad
// geometry, contact drawing, the threshold sliders and the decision log.
// Rendering lives in `components/GesturesPanel.tsx`.

import type {
  GestureDeviceInfo,
  GestureEdgeZones,
  GestureGuardConfig,
  GestureKindName,
  GestureLiveFrame,
  GestureLiveTouch,
  GestureLogEntry,
  GestureTouchClass,
} from "./ipc";
import { verdictLabel } from "./gesture-trace";

/** Poll cadence of the live view (≈30 fps — well under the 60 fps cap). */
export const LIVE_POLL_MS = 33;
/** A frame older than this is treated as "nothing on the pad". */
export const FRAME_STALE_MS = 400;
/** Built-in MacBook trackpad (measured 157.8 × 97.8 mm) — the fallback shape. */
export const DEFAULT_PAD_MM = { width: 157.8, height: 97.8 };

// The argument parser lives in `gestures-arg.ts`: App.tsx needs it, and
// importing this module there would pull the panel's labels and slider table
// into the start-up chunk.
export { parseGesturesArg, type GesturesArg } from "./gestures-arg";

export type PadProfile = "builtin" | "external";

/** The edge profile a device uses — mirrors `GuardConfig::edges_for`: only a
 *  device that says it is NOT built in gets the external profile. */
export function profileFor(device: GestureDeviceInfo | undefined): PadProfile {
  return device?.builtin === false ? "external" : "builtin";
}

/** Physical pad size, falling back to the built-in trackpad. */
export function padMillimetres(device: GestureDeviceInfo | undefined): { width: number; height: number } {
  const w = device?.width_mm;
  const h = device?.height_mm;
  if (w && h && w > 0 && h > 0) return { width: w, height: h };
  return DEFAULT_PAD_MM;
}

export function edgesOf(g: GestureGuardConfig, p: PadProfile): GestureEdgeZones {
  return p === "external" ? g.edges_external : g.edges_builtin;
}

/** A new config with one side of one profile's edge zones changed. */
export function withEdge(
  g: GestureGuardConfig,
  p: PadProfile,
  side: keyof GestureEdgeZones,
  value: number,
): GestureGuardConfig {
  const key = p === "external" ? "edges_external" : "edges_builtin";
  return { ...g, [key]: { ...g[key], [side]: value } };
}

/** The four edge zones as rectangles in a `w × h` drawing (y downwards). */
export function edgeRects(z: GestureEdgeZones, w: number, h: number) {
  return [
    { side: "left" as const, x: 0, y: 0, w: z.left * w, h },
    { side: "right" as const, x: w - z.right * w, y: 0, w: z.right * w, h },
    { side: "top" as const, x: 0, y: 0, w, h: z.top * h },
    { side: "bottom" as const, x: 0, y: h - z.bottom * h, w, h: z.bottom * h },
  ].filter((r) => r.w > 0 && r.h > 0);
}

/** The centre area of "release by a centre touch" (`center_size` per axis). */
export function centerRect(size: number, w: number, h: number) {
  const cw = size * w;
  const ch = size * h;
  return { x: (w - cw) / 2, y: (h - ch) / 2, w: cw, h: ch };
}

/** Smallest drawn radius — a contact must stay visible however small. */
export const MIN_TOUCH_PX = 5;

/** A contact as an ellipse in a `w × h` drawing of a `padMm` pad. Axes are
 *  in millimetres (half of them is the radius); the angle comes in radians. */
export function touchEllipse(
  t: GestureLiveTouch,
  w: number,
  h: number,
  padMm: { width: number; height: number },
) {
  const pxPerMm = w / padMm.width;
  const rx = Math.max(MIN_TOUCH_PX, (Math.max(t.major, 0) / 2) * pxPerMm);
  const ry = Math.max(MIN_TOUCH_PX, (Math.max(t.minor, 0) / 2) * pxPerMm);
  return {
    cx: t.x * w,
    cy: t.y * h,
    rx,
    ry,
    rotateDeg: (-t.angle * 180) / Math.PI,
  };
}

export const CLASS_STYLE: Record<GestureTouchClass, { color: string; label: string }> = {
  finger: { color: "#38bdf8", label: "Finger" },
  unclear: { color: "#94a3b8", label: "unklar" },
  thumb: { color: "#f59e0b", label: "Daumen" },
  palm: { color: "#f43f5e", label: "Handballen" },
};

/** The frame to draw: `null` when it is stale (fingers lifted, capture
 *  stopped) — a frozen contact would read as a finger that never lifts. */
export function freshFrame(frame: GestureLiveFrame | null, nowMs: number): GestureLiveFrame | null {
  if (!frame) return null;
  return nowMs - frame.at_ms <= FRAME_STALE_MS ? frame : null;
}

const KIND_LABEL: Record<GestureKindName, string> = {
  swipe_up: "Wischen hoch",
  swipe_down: "Wischen runter",
  swipe_left: "Wischen links",
  swipe_right: "Wischen rechts",
  tap: "Tippen",
  tip_tap_left: "Tip-Tap links",
  tip_tap_right: "Tip-Tap rechts",
};

const ACTION_LABEL: Record<string, string> = {
  volume_up: "lauter",
  volume_down: "leiser",
  mute_toggle: "stumm",
  next_tab: "nächster Tab",
  prev_tab: "vorheriger Tab",
};

const LEVEL_LABEL: Record<GestureLogEntry["level"], string> = {
  classify: "Stufe 1",
  spatial: "Stufe 2",
  typing: "Stufe 3",
  plausibility: "Stufe 4",
  config: "Einstellung",
};

/** "vor 2,3 s" / "vor 1 min" — how long ago, from the live clock. */
export function ago(atMs: number, nowMs: number): string {
  const s = Math.max(0, nowMs - atMs) / 1000;
  if (s < 60) return `vor ${s.toFixed(1).replace(".", ",")} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `vor ${m} min`;
  return `vor ${Math.floor(m / 60)} h`;
}

export type LogTone = "fired" | "blocked" | "contact";

/** One log entry as a display row. */
export function describeEntry(e: GestureLogEntry, nowMs: number) {
  const tone: LogTone = e.accepted ? "fired" : e.kind === null ? "contact" : "blocked";
  const what =
    e.kind === null
      ? `Kontakt #${e.touch_id ?? "?"}`
      : `${KIND_LABEL[e.kind] ?? e.kind} ×${e.fingers ?? "?"}`;
  const action = e.action ? ACTION_LABEL[e.action] ?? e.action : null;
  return {
    key: e.seq,
    when: ago(e.at_ms, nowMs),
    what,
    action,
    verdict: verdictLabel(e.verdict),
    level: e.accepted ? null : LEVEL_LABEL[e.level],
    tone,
  };
}

/** One slider of the panel. `scale` converts the stored value to what the
 *  slider shows (0.05 → 5 %), so the stored config keeps its own units. */
export interface GuardControl {
  key: keyof GestureGuardConfig;
  label: string;
  hint: string;
  min: number;
  max: number;
  step: number;
  unit: string;
  scale: number;
}

/** The sliders — every bound inside `GuardConfig::normalized`'s clamp, so the
 *  panel can't produce a value the backend would silently correct. */
export const GUARD_CONTROLS: GuardControl[] = [
  { key: "palm_size", label: "Handballen ab Größe", hint: "Kontakte ab dieser Fläche zählen nie als Finger", min: 0.5, max: 5, step: 0.1, unit: "", scale: 1 },
  { key: "thumb_ratio", label: "Daumen ab Seitenverhältnis", hint: "Länglich und unten gelandet = Daumen (0 = aus)", min: 0, max: 5, step: 0.1, unit: "", scale: 1 },
  { key: "settle_ms", label: "Einordnung nach", hint: "So lange gilt ein neuer Kontakt als unklar", min: 0, max: 300, step: 10, unit: "ms", scale: 1 },
  { key: "typing_single_ms", label: "Sperre nach Einzeltaste", hint: "Lautstärke und Stumm nach einem Tastendruck", min: 0, max: 3000, step: 50, unit: "ms", scale: 1 },
  { key: "typing_burst_ms", label: "Sperre beim Tippen", hint: "Wenn Tasten schnell aufeinander folgen", min: 0, max: 5000, step: 50, unit: "ms", scale: 1 },
  { key: "cooldown_ms", label: "Pause nach Geste", hint: "Keine zweite Geste so kurz danach", min: 0, max: 2000, step: 10, unit: "ms", scale: 1 },
  { key: "swipe_min_move", label: "Mindestweg Wischen", hint: "Anteil der Pad-Breite je Finger", min: 1, max: 50, step: 1, unit: "%", scale: 100 },
  { key: "tap_hold_max_ms", label: "Tippen höchstens", hint: "Länger gehalten ist kein Tippen", min: 50, max: 3000, step: 10, unit: "ms", scale: 1 },
];

/** Edge-zone sliders (per profile), in percent of the pad. */
export const EDGE_MAX_PCT = 25;
export const EDGE_SIDES: { side: keyof GestureEdgeZones; label: string }[] = [
  { side: "left", label: "links" },
  { side: "right", label: "rechts" },
  { side: "top", label: "oben" },
  { side: "bottom", label: "unten" },
];

/** Set one numeric control from its slider value (undoes `scale`). */
export function withControl(g: GestureGuardConfig, c: GuardControl, sliderValue: number): GestureGuardConfig {
  const v = sliderValue / c.scale;
  return { ...g, [c.key]: c.scale === 1 ? Math.round(v * 1000) / 1000 : v };
}

export function controlValue(g: GestureGuardConfig, c: GuardControl): number {
  const raw = g[c.key];
  return typeof raw === "number" ? Math.round(raw * c.scale * 1000) / 1000 : 0;
}

/** Does the config differ from the defaults (shows the reset button)? */
export function differsFrom(a: GestureGuardConfig, b: GestureGuardConfig): boolean {
  return JSON.stringify(a) !== JSON.stringify(b);
}
