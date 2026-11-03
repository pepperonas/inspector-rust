import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import EDITOR_RS from "../../../rust-lib/src/record/editor.rs?raw";
import type { RecordEditDoc, RecordEditorView } from "./ipc";
import {
  COLOR_PRESETS,
  GRADIENT_PRESETS,
  History,
  INSPECTOR_RANGES,
  LIVE_SEND_MS,
  LiveGesture,
  clampSetting,
  motionAllowed,
  previewRatio,
  setAppearance,
} from "./editor";

const doc = (padding = 0): RecordEditDoc =>
  ({
    version: 3,
    clips: [{ startMs: 0, endMs: 10_000 }],
    speed: [],
    crop: [null],
    output: { aspect: "auto" },
    micGainDb: 0,
    appearance: {
      background: { kind: "wallpaper", id: "01" },
      backgroundMotion: "none",
      padding,
      radiusPx: 40,
      shadow: 0.6,
      backgroundBlur: 0,
      motionBlur: 0.2,
      frame: "none",
      frameTheme: "light",
    },
    cursor: {
      show: true, autoHide: false, theme: "default", custom: null, size: 3.6, smoothing: 0.3,
      motionBlur: 0.35, clickBounce: 1, clickImpact: false, clickSound: false, clickSoundGainDb: 0,
    },
    zooms: [],
    autoZoomDone: true,
    annotations: [],
    audioTracks: [],
    systemGainDb: 0,
    camera: {
      preset: "no-webcam",
      sizePct: 40,
      mask: "square",
      roundness: null,
      mirror: false,
      reactiveZoom: true,
      anchor: "bottom-right",
      crop: null,
      background: { mode: "none", blur: 0.5, custom: { kind: "wallpaper", id: "01" } },
    },
    layoutRegions: [],
  }) as RecordEditDoc;

describe("inspector presets", () => {
  it("has the 16 upstream gradient pairs at 135° and 16 colours", () => {
    expect(GRADIENT_PRESETS).toHaveLength(16);
    for (const g of GRADIENT_PRESETS) {
      expect(g.kind).toBe("gradient");
      expect(g.angleDeg).toBe(135);
      expect(g.stops).toHaveLength(2);
      for (const s of g.stops) expect(s).toMatch(/^#[0-9a-f]{6}$/);
    }
    expect(GRADIENT_PRESETS[0].stops).toEqual(["#eaebed", "#bcc0c6"]);
    expect(GRADIENT_PRESETS[15].stops).toEqual(["#bcc0c6", "#eaebed"]);
    expect(COLOR_PRESETS).toHaveLength(16);
    expect(new Set(COLOR_PRESETS).size).toBe(16);
    expect(COLOR_PRESETS[0]).toBe("#16171d");
  });

  it("motion is offered for wallpapers, gradients and images, never a plain colour", () => {
    expect(motionAllowed({ kind: "wallpaper", id: "01" })).toBe(true);
    expect(motionAllowed({ kind: "gradient", angleDeg: 135, stops: ["#000000", "#ffffff"] })).toBe(true);
    expect(motionAllowed({ kind: "image", file: "a.png" })).toBe(true);
    expect(motionAllowed({ kind: "color", color: "#000000" })).toBe(false);
  });
});

describe("clampSetting", () => {
  it("clamps into the range and falls back on garbage", () => {
    expect(clampSetting("padding", 500)).toBe(100);
    expect(clampSetting("padding", -3)).toBe(0);
    expect(clampSetting("cursorSize", 0.2)).toBe(1.5);
    expect(clampSetting("clickSoundGainDb", 9)).toBe(0);
    expect(clampSetting("clickSoundGainDb", Number.NaN)).toBe(INSPECTOR_RANGES.clickSoundGainDb.min);
  });

  it("every range mirrors Rust's normalize (record/editor.rs)", () => {
    const rust: Record<string, string> = {
      padding: "a.padding",
      radiusPx: "a.radius_px",
      shadow: "a.shadow",
      backgroundBlur: "a.background_blur",
      motionBlur: "a.motion_blur",
      cursorSize: "c.size",
      smoothing: "c.smoothing",
      cursorMotionBlur: "c.motion_blur",
      clickBounce: "c.click_bounce",
      clickSoundGainDb: "c.click_sound_gain_db",
    };
    for (const [key, field] of Object.entries(rust)) {
      const m = new RegExp(`clamp_or\\(${field.replace(".", "\\.")}, (-?[\\d.]+), (-?[\\d.]+)`).exec(EDITOR_RS);
      expect(m, field).toBeTruthy();
      const r = INSPECTOR_RANGES[key as keyof typeof INSPECTOR_RANGES];
      expect([r.min, r.max], key).toEqual([Number(m![1]), Number(m![2])]);
    }
  });
});

describe("previewRatio", () => {
  it("takes Rust's output size, not a TS copy of the aspect rule", () => {
    const view = { outputSize: [1080, 1080] } as unknown as RecordEditorView;
    expect(previewRatio(view, 1920, 1080)).toBe(1);
    const tall = { outputSize: [1080, 1920] } as unknown as RecordEditorView;
    expect(previewRatio(tall, 1920, 1080)).toBeCloseTo(0.5625);
  });

  it("falls back to the source shape without an output size", () => {
    expect(previewRatio({ outputSize: [0, 0] } as unknown as RecordEditorView, 1600, 1000)).toBeCloseTo(1.6);
    expect(previewRatio({} as unknown as RecordEditorView, 1600, 1000)).toBeCloseTo(1.6);
  });
});

describe("LiveGesture", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("moves send debounced without a step; the release is one undo step", () => {
    const h = new History(doc(0));
    const sent: [RecordEditDoc, boolean][] = [];
    const g = new LiveGesture(() => h, (d, record) => {
      if (record) h.push(d);
      sent.push([d, record]);
    });
    g.move(setAppearance(h.present, { padding: 10 }));
    g.move(setAppearance(h.present, { padding: 20 }));
    g.move(setAppearance(h.present, { padding: 30 }));
    expect(sent).toHaveLength(0);
    vi.advanceTimersByTime(LIVE_SEND_MS);
    expect(sent).toHaveLength(1);
    expect(sent[0][1]).toBe(false);
    expect(sent[0][0].appearance.padding).toBe(30);
    expect(h.canUndo()).toBe(false);

    g.move(setAppearance(h.present, { padding: 40 }));
    g.end();
    expect(sent).toHaveLength(2);
    expect(sent[1]).toEqual([expect.objectContaining({ appearance: expect.objectContaining({ padding: 40 }) }), true]);
    vi.advanceTimersByTime(LIVE_SEND_MS * 4);
    expect(sent).toHaveLength(2);
    // One step back lands on the document before the drag.
    expect(h.undo()?.appearance.padding).toBe(0);
    expect(h.canUndo()).toBe(false);
  });

  it("a release without a move records nothing", () => {
    const h = new History(doc(0));
    const send = vi.fn();
    const g = new LiveGesture(() => h, send);
    g.end();
    g.end();
    expect(send).not.toHaveBeenCalled();
  });

  it("a drag that ends where it started records nothing", () => {
    const h = new History(doc(5));
    const send = vi.fn();
    const g = new LiveGesture(() => h, send);
    g.move(setAppearance(h.present, { padding: 9 }));
    g.move(setAppearance(h.present, { padding: 5 }));
    g.end();
    expect(send.mock.calls.filter((c) => c[1] === true)).toHaveLength(0);
  });
});
