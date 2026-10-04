import { describe, expect, it } from "vitest";
import {
  ago,
  centerRect,
  controlValue,
  DEFAULT_PAD_MM,
  describeEntry,
  differsFrom,
  edgeRects,
  edgesOf,
  freshFrame,
  FRAME_STALE_MS,
  GUARD_CONTROLS,
  MIN_TOUCH_PX,
  padMillimetres,
  parseGesturesArg,
  profileFor,
  touchEllipse,
  withControl,
  withEdge,
} from "./gestures-live";
import type { GestureGuardConfig, GestureLiveTouch, GestureLogEntry } from "./ipc";

const zones = { left: 0.03, right: 0.03, top: 0.05, bottom: 0.05 };
const guard: GestureGuardConfig = {
  settle_ms: 30,
  palm_size: 2,
  palm_major: 0,
  thumb_ratio: 1.7,
  thumb_min_size: 1.3,
  thumb_zone: 0.6,
  edges_builtin: zones,
  edges_external: { left: 0.1, right: 0.1, top: 0.1, bottom: 0.1 },
  palm_blocks_all: false,
  typing_single_ms: 250,
  typing_burst_ms: 600,
  burst_gap_ms: 500,
  release_by_center: false,
  center_size: 0.5,
  constant_count: true,
  coherence_min: 0.6,
  swipe_min_move: 0.06,
  early_min_move: 0.12,
  min_speed: 0,
  evenness_min: 0,
  tap_window_ms: 700,
  tap_hold_max_ms: 350,
  tap_max_move: 0.12,
  cooldown_ms: 150,
};

const entry = (over: Partial<GestureLogEntry>): GestureLogEntry => ({
  seq: 1,
  at_ms: 1000,
  device: 0,
  level: "typing",
  verdict: "typing_guard",
  accepted: false,
  kind: "tap",
  fingers: 3,
  action: "mute_toggle",
  via: "tick",
  touch_id: null,
  ...over,
});

describe("gestures argument", () => {
  it("opens the panel bare and switches only on a clear on/off", () => {
    expect(parseGesturesArg("")).toBe("panel");
    expect(parseGesturesArg(" On ")).toBe("on");
    expect(parseGesturesArg("aus")).toBe("off");
    expect(parseGesturesArg("of")).toBe("unknown");
    expect(parseGesturesArg("calibrate")).toBe("calibrate");
    expect(parseGesturesArg("Kalibrieren")).toBe("calibrate");
    expect(parseGesturesArg("record")).toBe("record");
    expect(parseGesturesArg("aufnahme")).toBe("record");
    expect(parseGesturesArg("recording")).toBe("unknown");
  });
});

describe("device profile", () => {
  it("uses the external profile only when the device says it is not built in", () => {
    expect(profileFor({ builtin: false, width_mm: null, height_mm: null })).toBe("external");
    expect(profileFor({ builtin: true, width_mm: null, height_mm: null })).toBe("builtin");
    expect(profileFor({ builtin: null, width_mm: null, height_mm: null })).toBe("builtin");
    expect(profileFor(undefined)).toBe("builtin");
  });

  it("changes exactly one side of one profile", () => {
    const g = withEdge(guard, "external", "top", 0.2);
    expect(g.edges_external).toEqual({ left: 0.1, right: 0.1, top: 0.2, bottom: 0.1 });
    expect(g.edges_builtin).toBe(guard.edges_builtin);
    expect(edgesOf(g, "external").top).toBe(0.2);
    expect(guard.edges_external.top).toBe(0.1); // input untouched
  });

  it("falls back to the built-in pad size", () => {
    expect(padMillimetres(undefined)).toEqual(DEFAULT_PAD_MM);
    expect(padMillimetres({ builtin: false, width_mm: 160, height_mm: 115 })).toEqual({ width: 160, height: 115 });
    expect(padMillimetres({ builtin: true, width_mm: 0, height_mm: 90 })).toEqual(DEFAULT_PAD_MM);
  });
});

describe("pad geometry", () => {
  it("draws the edge zones at their share of the pad", () => {
    const r = edgeRects(zones, 200, 100);
    expect(r.find((x) => x.side === "left")).toMatchObject({ x: 0, w: 6, h: 100 });
    expect(r.find((x) => x.side === "right")).toMatchObject({ x: 194, w: 6 });
    expect(r.find((x) => x.side === "bottom")).toMatchObject({ y: 95, h: 5 });
  });

  it("drops a zone set to zero", () => {
    expect(edgeRects({ ...zones, top: 0 }, 200, 100).map((r) => r.side)).not.toContain("top");
  });

  it("centres the centre area", () => {
    expect(centerRect(0.5, 200, 100)).toEqual({ x: 50, y: 25, w: 100, h: 50 });
  });

  const touch = (over: Partial<GestureLiveTouch>): GestureLiveTouch => ({
    id: 1, x: 0.5, y: 0.25, major: 16, minor: 8, angle: 0, size: 1, class: "finger", in_edge: false, ...over,
  });

  it("maps a contact to an ellipse in millimetres of the real pad", () => {
    const e = touchEllipse(touch({}), 200, 100, { width: 100, height: 50 });
    expect(e).toMatchObject({ cx: 100, cy: 25, rx: 16, ry: 8 });
  });

  it("never draws a contact smaller than the minimum", () => {
    const e = touchEllipse(touch({ major: 0, minor: -2 }), 200, 100, { width: 100, height: 50 });
    expect(e.rx).toBe(MIN_TOUCH_PX);
    expect(e.ry).toBe(MIN_TOUCH_PX);
  });

  it("turns the angle into degrees, counter-clockwise like the driver", () => {
    expect(touchEllipse(touch({ angle: Math.PI / 2 }), 200, 100, DEFAULT_PAD_MM).rotateDeg).toBeCloseTo(-90);
  });
});

describe("staleness", () => {
  it("hides a frame once it is older than the cutoff", () => {
    const f = { at_ms: 1000, device: 0, touches: [] };
    expect(freshFrame(f, 1000 + FRAME_STALE_MS)).toBe(f);
    expect(freshFrame(f, 1001 + FRAME_STALE_MS)).toBeNull();
    expect(freshFrame(null, 0)).toBeNull();
  });
});

describe("decision log", () => {
  it("says how long ago", () => {
    expect(ago(1000, 3300)).toBe("vor 2,3 s");
    expect(ago(0, 125_000)).toBe("vor 2 min");
    expect(ago(5000, 1000)).toBe("vor 0,0 s"); // clock skew never goes negative
  });

  it("describes a blocked gesture with its level and German labels", () => {
    const r = describeEntry(entry({}), 3300);
    expect(r).toMatchObject({ what: "Tippen ×3", action: "stumm", verdict: "blocked (typing)", level: "Stufe 3", tone: "blocked" });
  });

  it("marks an accepted gesture without a level", () => {
    const r = describeEntry(entry({ accepted: true, verdict: "dispatched", level: "plausibility" }), 1000);
    expect(r.tone).toBe("fired");
    expect(r.level).toBeNull();
  });

  it("names the contact for a contact decision", () => {
    const r = describeEntry(entry({ kind: null, fingers: null, action: null, touch_id: 7, verdict: "palm", level: "classify" }), 1000);
    expect(r).toMatchObject({ what: "Kontakt #7", action: null, verdict: "ignored (palm)", tone: "contact" });
  });
});

describe("threshold sliders", () => {
  it("stays inside the backend's clamp ranges", () => {
    // GuardConfig::normalized — a slider must never produce a value the
    // backend silently corrects.
    const clamp: Partial<Record<keyof GestureGuardConfig, [number, number]>> = {
      palm_size: [0.5, 10],
      thumb_ratio: [0, 5],
      settle_ms: [0, 300],
      typing_single_ms: [0, 3000],
      typing_burst_ms: [0, 5000],
      cooldown_ms: [0, 2000],
      swipe_min_move: [0.01, 0.5],
      tap_hold_max_ms: [50, 3000],
    };
    for (const c of GUARD_CONTROLS) {
      const [lo, hi] = clamp[c.key]!;
      expect(c.min / c.scale).toBeGreaterThanOrEqual(lo);
      expect(c.max / c.scale).toBeLessThanOrEqual(hi);
    }
  });

  it("round-trips a scaled value", () => {
    const c = GUARD_CONTROLS.find((x) => x.key === "swipe_min_move")!;
    expect(controlValue(guard, c)).toBe(6);
    const g = withControl(guard, c, 9);
    expect(g.swipe_min_move).toBeCloseTo(0.09);
    expect(controlValue(g, c)).toBe(9);
  });

  it("detects a changed config", () => {
    expect(differsFrom(guard, { ...guard })).toBe(false);
    expect(differsFrom(guard, { ...guard, cooldown_ms: 200 })).toBe(true);
  });
});
