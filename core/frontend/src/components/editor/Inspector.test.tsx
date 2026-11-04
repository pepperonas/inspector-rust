// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { EditorAssets, RecordEditDoc } from "../../lib/ipc";
import { Inspector, type InspectorProps } from "./Inspector";
import { CAPTION_DEFAULTS } from "../../lib/editor";

const doc0 = (over: Partial<RecordEditDoc["appearance"]> = {}): RecordEditDoc => ({
  version: 3,
  clips: [{ startMs: 0, endMs: 10_000 }],
  speed: [],
  crop: [null],
  output: { aspect: "auto" },
  micGainDb: 0,
  appearance: {
    background: { kind: "wallpaper", id: "01" },
    backgroundMotion: "none",
    padding: 50,
    radiusPx: 40,
    shadow: 0.6,
    backgroundBlur: 0,
    motionBlur: 0.2,
    frame: "none",
    frameTheme: "light",
    depthOfField: true,
    ...over,
  },
  cursor: {
    show: true, autoHide: false, theme: "default", custom: null, size: 3.6, smoothing: 0.3,
    motionBlur: 0.35, clickBounce: 1, clickImpact: false, clickSound: false, clickSoundGainDb: 0, model3d: false,
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
  transcripts: [],
  captions: { ...CAPTION_DEFAULTS },
});

const assets = (hasCursorData = true): EditorAssets => ({
  wallpapers: ["01", "02", "03", "04", "05", "06", "07", "08"].map((id) => ({ id, thumb: `/a/thumbs/${id}.jpg` })),
  themes: ["default", "studio-ink", "prism-glow", "pop-coral", "pixel-candy", "star-sprout", "glass-lens"].map((id) => ({
    id,
    preview: `/a/cursors/${id}/arrow.png`,
  })),
  hasCursorData,
  clickDown: null,
  clickUp: null,
});

function mount(over: Partial<InspectorProps> = {}) {
  const props: InspectorProps = {
    doc: doc0(),
    assets: assets(),
    assetUrl: (p) => `asset://${p}`,
    projectAssetUrl: (f) => `asset://proj/${f}`,
    onCommit: vi.fn(),
    onLive: vi.fn(),
    onLiveEnd: vi.fn(),
    onImport: vi.fn(async () => null),
    selectedZoom: null,
    onSelectZoom: vi.fn(),
    onAutoZoom: vi.fn(),
    ...over,
  };
  const r = render(<Inspector {...props} />);
  return { ...r, props };
}

afterEach(cleanup);

describe("Inspector", () => {
  it("a wallpaper click sets the background as one step", () => {
    const { props } = mount();
    fireEvent.click(screen.getByRole("button", { name: "Wallpaper 3" }));
    expect(props.onCommit).toHaveBeenCalledTimes(1);
    const d = (props.onCommit as ReturnType<typeof vi.fn>).mock.calls[0][0] as RecordEditDoc;
    expect(d.appearance.background).toEqual({ kind: "wallpaper", id: "03" });
    expect(props.onLive).not.toHaveBeenCalled();
  });

  it("gradient and colour swatches set those backgrounds", () => {
    const { props } = mount();
    fireEvent.click(screen.getByRole("button", { name: "Gradient 2" }));
    fireEvent.click(screen.getByRole("button", { name: "Colour #ef4444" }));
    const calls = (props.onCommit as ReturnType<typeof vi.fn>).mock.calls.map((c) => (c[0] as RecordEditDoc).appearance.background);
    expect(calls[0]).toEqual({ kind: "gradient", angleDeg: 135, stops: ["#3b82f6", "#8b5cf6"] });
    expect(calls[1]).toEqual({ kind: "color", color: "#ef4444" });
  });

  it("background motion is disabled for a plain colour", () => {
    mount({ doc: doc0({ background: { kind: "color", color: "#000000" } }) });
    for (const name of ["None", "Drift", "Aurora", "Waves"]) {
      expect((screen.getByRole("button", { name }) as HTMLButtonElement).disabled, name).toBe(true);
    }
    cleanup();
    mount();
    expect((screen.getByRole("button", { name: "Aurora" }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("an own image goes through the import and becomes the background", async () => {
    const onImport = vi.fn(async () => "abc.png");
    const { props } = mount({ onImport });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Choose image…" }));
    });
    expect(onImport).toHaveBeenCalledWith("image");
    const d = (props.onCommit as ReturnType<typeof vi.fn>).mock.calls[0][0] as RecordEditDoc;
    expect(d.appearance.background).toEqual({ kind: "image", file: "abc.png" });
  });

  it("a slider clamps to its range and moves live; the release is the undo step", () => {
    const { props } = mount();
    fireEvent.click(screen.getByRole("tab", { name: "Frame" }));
    const pad = screen.getByRole("slider", { name: "Padding" });
    // The DOM sanitises a range input's value; read raw values past it, so the component's
    // own clamp is what is tested (a webview may hand over an unsanitised value).
    const raw = (v: string) => {
      Object.defineProperty(pad, "value", { configurable: true, get: () => v, set: () => {} });
      fireEvent.input(pad);
    };
    raw("500");
    raw("-4");
    expect(props.onCommit).not.toHaveBeenCalled();
    const live = (props.onLive as ReturnType<typeof vi.fn>).mock.calls.map((c) => (c[0] as RecordEditDoc).appearance.padding);
    expect(live).toEqual([100, 0]);
    expect(props.onLiveEnd).not.toHaveBeenCalled();
    fireEvent.pointerUp(pad);
    expect(props.onLiveEnd).toHaveBeenCalledTimes(1);
  });

  it("shadow levels and device frames are one-step chips", () => {
    const { props } = mount();
    fireEvent.click(screen.getByRole("tab", { name: "Frame" }));
    fireEvent.click(screen.getByRole("button", { name: "Strong" }));
    fireEvent.click(screen.getByRole("button", { name: "Laptop" }));
    fireEvent.click(screen.getByRole("button", { name: "1:1" }));
    const calls = (props.onCommit as ReturnType<typeof vi.fn>).mock.calls.map((c) => c[0] as RecordEditDoc);
    expect(calls[0].appearance.shadow).toBe(0.9);
    expect(calls[1].appearance.frame).toBe("laptop");
    expect(calls[2].output.aspect).toBe("1:1");
  });

  it("without pointer data the pointer tab has no controls", () => {
    mount({ assets: assets(false) });
    fireEvent.click(screen.getByRole("tab", { name: "Pointer" }));
    expect(screen.getByText("This recording has no pointer data.")).toBeTruthy();
    expect(screen.queryAllByRole("slider")).toHaveLength(0);
    expect(screen.queryByRole("checkbox")).toBeNull();
    expect(screen.queryByRole("button", { name: "Studio Ink" })).toBeNull();
  });

  it("with pointer data the pointer tab offers the theme grid and the controls", () => {
    const { props } = mount();
    fireEvent.click(screen.getByRole("tab", { name: "Pointer" }));
    expect(document.querySelectorAll('img[src$="arrow.png"]')).toHaveLength(7);
    fireEvent.click(screen.getByRole("button", { name: "Studio Ink" }));
    fireEvent.click(screen.getByRole("button", { name: "Strong" }));
    const calls = (props.onCommit as ReturnType<typeof vi.fn>).mock.calls.map((c) => c[0] as RecordEditDoc);
    expect(calls[0].cursor.theme).toBe("studio-ink");
    expect(calls[1].cursor.clickBounce).toBe(2);
    const size = screen.getByRole("slider", { name: "Size" });
    fireEvent.change(size, { target: { value: "9" } });
    expect(((props.onLive as ReturnType<typeof vi.fn>).mock.calls[0][0] as RecordEditDoc).cursor.size).toBe(6);
  });

  it("an own pointer shows hotspot sliders", async () => {
    const onImport = vi.fn(async () => "p.png");
    const { props, rerender } = mount({ onImport });
    fireEvent.click(screen.getByRole("tab", { name: "Pointer" }));
    expect(screen.queryByRole("slider", { name: "Hotspot X" })).toBeNull();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Choose PNG…" }));
    });
    expect(onImport).toHaveBeenCalledWith("cursor");
    const d = (props.onCommit as ReturnType<typeof vi.fn>).mock.calls[0][0] as RecordEditDoc;
    expect(d.cursor.custom).toEqual({ file: "p.png", hotspotX: 0, hotspotY: 0 });
    rerender(<Inspector {...props} doc={d} />);
    expect(screen.getByRole("slider", { name: "Hotspot X" })).toBeTruthy();
    expect(screen.getByRole("slider", { name: "Hotspot Y" })).toBeTruthy();
  });

  it("selecting a layout region brings up the camera tab", () => {
    const { props, rerender } = mount();
    expect(screen.getByRole("tab", { name: "Camera" }).getAttribute("aria-selected")).toBe("false");
    rerender(<Inspector {...props} selectedLayout="r1" />);
    expect(screen.getByRole("tab", { name: "Camera" }).getAttribute("aria-selected")).toBe("true");
    expect(screen.getByTestId("ed-camera-controls")).toBeTruthy();
  });

  it("a layout block selected far from the playhead is the region the camera tab edits", () => {
    const base = doc0();
    const d = { ...base, camera: { ...base.camera, preset: "picture-in-picture" as const }, layoutRegions: [{ id: "far", clip: 0, startMs: 8_000, preset: "camera-only" as const }] };
    const view = { doc: d, layoutSpans: [{ index: 0, progStartMs: 8_000, progEndMs: 10_000 }], clipCameras: [true] };
    const { props } = mount({ doc: d, view, positionMs: 2_000, selectedLayout: "far" });
    fireEvent.click(screen.getByRole("button", { name: "Dual frame" }));
    const next = (props.onCommit as ReturnType<typeof vi.fn>).mock.calls[0][0] as RecordEditDoc;
    expect(next.layoutRegions).toEqual([{ id: "far", clip: 0, startMs: 8_000, preset: "dual-frame" }]);
    expect(next.camera.preset).toBe("picture-in-picture");
  });

  it("the 3D cursor is a switch, off while the pointer is hidden", () => {
    const { props } = mount();
    fireEvent.click(screen.getByRole("tab", { name: "Pointer" }));
    const sw = screen.getByRole("checkbox", { name: "3D cursor" }) as HTMLInputElement;
    expect(sw.disabled).toBe(false);
    fireEvent.click(sw);
    expect(((props.onCommit as ReturnType<typeof vi.fn>).mock.calls[0][0] as RecordEditDoc).cursor.model3d).toBe(true);
    cleanup();
    const hidden = doc0();
    mount({ doc: { ...hidden, cursor: { ...hidden.cursor, show: false } } });
    fireEvent.click(screen.getByRole("tab", { name: "Pointer" }));
    expect((screen.getByRole("checkbox", { name: "3D cursor" }) as HTMLInputElement).disabled).toBe(true);
  });

  it("depth of field appears in the frame tab only with a tilted zoom", () => {
    const flat = { id: "a", clip: 0, startMs: 0, endMs: 2_000, depth: 3, customScale: null, focusX: 0.5, focusY: 0.5, focusMode: "auto" as const, origin: "manual" as const, hideCursor: false };
    mount({ doc: { ...doc0(), zooms: [flat] } });
    fireEvent.click(screen.getByRole("tab", { name: "Frame" }));
    expect(screen.queryByRole("checkbox", { name: "Depth of field" })).toBeNull();
    cleanup();
    const { props } = mount({ doc: { ...doc0(), zooms: [{ ...flat, rotation: "left" as const }] } });
    fireEvent.click(screen.getByRole("tab", { name: "Frame" }));
    const dof = screen.getByRole("checkbox", { name: "Depth of field" }) as HTMLInputElement;
    expect(dof.checked).toBe(true);
    fireEvent.click(dof);
    expect(((props.onCommit as ReturnType<typeof vi.fn>).mock.calls[0][0] as RecordEditDoc).appearance.depthOfField).toBe(false);
  });

  it("the frame picker offers every device frame and both frame colours", () => {
    mount();
    fireEvent.click(screen.getByRole("tab", { name: "Frame" }));
    for (const n of ["None", "Window", "Laptop", "Phone", "Monitor", "Light", "Dark"]) expect(screen.getByRole("button", { name: n }), n).toBeTruthy();
  });

  it("selecting an effect brings up the effects tab; adding goes to the editor", () => {
    const fx = { id: "fx", clip: 0, startMs: 0, endMs: 2_000, effect: "vignette", values: {} };
    const onAddEffect = vi.fn();
    const { props, rerender } = mount({ doc: { ...doc0(), effects: [fx] }, onAddEffect });
    expect(screen.getByRole("tab", { name: "Effects" }).getAttribute("aria-selected")).toBe("false");
    rerender(<Inspector {...props} selectedEffect="fx" />);
    expect(screen.getByRole("tab", { name: "Effects" }).getAttribute("aria-selected")).toBe("true");
    expect(screen.getByRole("slider", { name: "Strength" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add effect" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Film grain/ }));
    expect(onAddEffect).toHaveBeenCalledWith("grain");
  });

  describe("zoom tab", () => {
    const z = (id: string, origin: "auto" | "manual", over: Partial<RecordEditDoc["zooms"][number]> = {}) => ({
      id,
      clip: 0,
      startMs: 1_000,
      endMs: 3_000,
      depth: 3,
      customScale: null,
      focusX: 0.5,
      focusY: 0.5,
      focusMode: "auto" as const,
      origin,
      hideCursor: false,
      ...over,
    });
    const withZooms = (...zooms: RecordEditDoc["zooms"]) => ({ ...doc0(), zooms });
    const committed = (props: InspectorProps) =>
      (props.onCommit as ReturnType<typeof vi.fn>).mock.calls.map((c) => c[0] as RecordEditDoc);

    it("opens on the zoom tab when a zoom gets selected", () => {
      const { props, rerender } = mount({ doc: withZooms(z("a", "manual")) });
      expect(screen.getByRole("tab", { name: "Zoom" }).getAttribute("aria-selected")).toBe("false");
      rerender(<Inspector {...props} selectedZoom="a" />);
      expect(screen.getByRole("tab", { name: "Zoom" }).getAttribute("aria-selected")).toBe("true");
    });

    it("depth chips show their factor and set the depth (custom scale cleared)", () => {
      const { props } = mount({ doc: withZooms(z("a", "manual", { customScale: 2.4 })), selectedZoom: "a" });
      for (const f of ["1.25×", "1.5×", "1.8×", "2.2×", "3.5×", "5×"]) expect(screen.getByRole("button", { name: f })).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: "3.5×" }));
      expect(committed(props)[0].zooms[0]).toMatchObject({ depth: 5, customScale: null });
    });

    it("the own scale slider moves live and clamps", () => {
      const { props } = mount({ doc: withZooms(z("a", "manual")), selectedZoom: "a" });
      fireEvent.change(screen.getByRole("slider", { name: "Own scale" }), { target: { value: "9" } });
      expect(((props.onLive as ReturnType<typeof vi.fn>).mock.calls[0][0] as RecordEditDoc).zooms[0].customScale).toBe(5);
    });

    it("focus mode, hiding the pointer and deleting are one step each; edits keep the zoom", () => {
      const { props } = mount({ doc: withZooms(z("a", "auto")), selectedZoom: "a" });
      fireEvent.click(screen.getByRole("button", { name: "Fixed" }));
      fireEvent.click(screen.getByRole("checkbox", { name: "Hide the pointer while zoomed" }));
      fireEvent.click(screen.getByRole("button", { name: "Delete zoom" }));
      const c = committed(props);
      expect(c[0].zooms[0]).toMatchObject({ focusMode: "manual", origin: "manual" });
      expect(c[1].zooms[0].hideCursor).toBe(true);
      expect(c[2].zooms).toEqual([]);
      expect(props.onSelectZoom).toHaveBeenCalledWith(null);
    });

    it("a fixed focus explains the click in the preview", () => {
      mount({ doc: withZooms(z("a", "manual", { focusMode: "manual" })), selectedZoom: "a" });
      expect(screen.getByTestId("ed-focus-hint")).toBeTruthy();
    });

    it("auto-zoom is off without pointer data and says why", () => {
      const { props } = mount({ assets: assets(false) });
      fireEvent.click(screen.getByRole("tab", { name: "Zoom" }));
      const btn = screen.getByRole("button", { name: "Auto-zoom" }) as HTMLButtonElement;
      expect(btn.disabled).toBe(true);
      expect(btn.title).toBe("This recording has no pointer data.");
      cleanup();
      const m = mount();
      fireEvent.click(screen.getByRole("tab", { name: "Zoom" }));
      fireEvent.click(screen.getByRole("button", { name: "Auto-zoom" }));
      expect(m.props.onAutoZoom).toHaveBeenCalledTimes(1);
      void props;
    });

    it("the 3D camera row sets the rotation; Off clears it; the zoom becomes manual", () => {
      const { props } = mount({ doc: withZooms(z("a", "auto")), selectedZoom: "a" });
      for (const n of ["Off", "Orbit", "Turned left", "Turned right"]) expect(screen.getByRole("button", { name: n })).toBeTruthy();
      expect(screen.getByRole("button", { name: "Off" }).getAttribute("aria-pressed")).toBe("true");
      fireEvent.click(screen.getByRole("button", { name: "Turned left" }));
      fireEvent.click(screen.getByRole("button", { name: "Orbit" }));
      const c = committed(props);
      expect(c[0].zooms[0]).toMatchObject({ rotation: "left", origin: "manual" });
      expect(c[1].zooms[0].rotation).toBe("orbit");
      cleanup();
      const m = mount({ doc: withZooms(z("a", "manual", { rotation: "right" })), selectedZoom: "a" });
      expect(screen.getByRole("button", { name: "Turned right" }).getAttribute("aria-pressed")).toBe("true");
      fireEvent.click(screen.getByRole("button", { name: "Off" }));
      expect(committed(m.props)[0].zooms[0].rotation ?? null).toBeNull();
    });

    it("Orbit is not offered without pointer data in auto focus; a fixed focus or a stored orbit keeps it", () => {
      mount({ assets: assets(false), doc: withZooms(z("a", "manual")), selectedZoom: "a" });
      expect(screen.queryByRole("button", { name: "Orbit" })).toBeNull();
      expect(screen.getByRole("button", { name: "Turned left" })).toBeTruthy();
      cleanup();
      mount({ assets: assets(false), doc: withZooms(z("a", "manual", { focusMode: "manual" })), selectedZoom: "a" });
      expect(screen.getByRole("button", { name: "Orbit" })).toBeTruthy();
      cleanup();
      mount({ assets: assets(false), doc: withZooms(z("a", "manual", { rotation: "orbit" })), selectedZoom: "a" });
      expect(screen.getByRole("button", { name: "Orbit" }).getAttribute("aria-pressed")).toBe("true");
      expect(screen.getByText(/the camera holds still/)).toBeTruthy();
    });

    it("pointer data is per clip: an inserted take without pointer data hides Orbit", () => {
      const base = withZooms(z("a", "manual", { clip: 1 }));
      const d = { ...base, clips: [...base.clips, { startMs: 0, endMs: 4_000, source: "t2" }], crop: [null, null] };
      const take = { durationMs: 4_000, width: 1920, height: 1080, systemAudio: false, hasMic: false, micOffsetMs: 0, hasCursor: false };
      const view = { doc: d, layoutSpans: [], clipCameras: [false, false], takes: { t2: take } };
      mount({ doc: d, view, selectedZoom: "a" });
      expect(screen.queryByRole("button", { name: "Orbit" })).toBeNull();
      cleanup();
      mount({ doc: d, view: { ...view, takes: { t2: { ...take, hasCursor: true } } }, selectedZoom: "a" });
      expect(screen.getByRole("button", { name: "Orbit" })).toBeTruthy();
    });

    it("removing auto-zooms keeps the manual ones", () => {
      const { props } = mount({ doc: withZooms(z("a", "manual"), z("b", "auto"), z("c", "auto")) });
      fireEvent.click(screen.getByRole("tab", { name: "Zoom" }));
      fireEvent.click(screen.getByRole("button", { name: "Remove auto-zooms" }));
      expect(committed(props)[0].zooms.map((x) => x.id)).toEqual(["a"]);
    });
  });
});
