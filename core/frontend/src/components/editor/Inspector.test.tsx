// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { EditorAssets, RecordEditDoc } from "../../lib/ipc";
import { Inspector, type InspectorProps } from "./Inspector";

const doc0 = (over: Partial<RecordEditDoc["appearance"]> = {}): RecordEditDoc => ({
  version: 2,
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
    ...over,
  },
  cursor: {
    show: true, autoHide: false, theme: "default", custom: null, size: 3.6, smoothing: 0.3,
    motionBlur: 0.35, clickBounce: 1, clickImpact: false, clickSound: false, clickSoundGainDb: 0,
  },
  zooms: [],
  autoZoomDone: true,
});

const assets = (hasCursorData = true): EditorAssets => ({
  wallpapers: ["01", "02", "03", "04", "05", "06", "07", "08"].map((id) => ({ id, thumb: `/a/thumbs/${id}.jpg` })),
  themes: ["default", "studio-ink", "prism-glow", "pop-coral", "pixel-candy", "star-sprout", "glass-lens"].map((id) => ({
    id,
    preview: `/a/cursors/${id}/arrow.png`,
  })),
  hasCursorData,
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
});
