// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import type { RecordCameraStyle, RecordEditDoc } from "../../lib/ipc";
import { CameraTab, type CameraTabProps } from "./CameraTab";

afterEach(cleanup);

const camera = (over: Partial<RecordCameraStyle> = {}): RecordCameraStyle => ({
  preset: "picture-in-picture",
  sizePct: 40,
  mask: "square",
  roundness: null,
  mirror: false,
  reactiveZoom: true,
  anchor: "bottom-right",
  crop: null,
  background: { mode: "none", blur: 0.5, custom: { kind: "color", color: "#000000" } },
  ...over,
});

const doc = (over: Partial<RecordEditDoc> = {}): RecordEditDoc =>
  ({
    version: 4,
    clips: [{ startMs: 0, endMs: 10_000 }],
    speed: [],
    crop: [null],
    output: { aspect: "auto" },
    micGainDb: 0,
    appearance: {} as RecordEditDoc["appearance"],
    cursor: {} as RecordEditDoc["cursor"],
    zooms: [],
    autoZoomDone: true,
    annotations: [],
    audioTracks: [],
    systemGainDb: 0,
    camera: camera(),
    layoutRegions: [],
    ...over,
  }) as RecordEditDoc;

function mount(d: RecordEditDoc, over: Partial<CameraTabProps> = {}) {
  const props: CameraTabProps = {
    doc: d,
    view: { doc: d, layoutSpans: [], clipCameras: [true] },
    positionMs: 0,
    segmentation: true,
    cameraSrc: null,
    assets: null,
    assetUrl: (p) => p,
    projectAssetUrl: (f) => f,
    onImport: async () => null,
    onCommit: vi.fn(),
    onLive: vi.fn(),
    onLiveEnd: vi.fn(),
    onAddRegion: vi.fn(),
    ...over,
  };
  render(<CameraTab {...props} />);
  return props;
}

const lastOf = (f: unknown) => {
  const calls = (f as ReturnType<typeof vi.fn>).mock.calls;
  return calls[calls.length - 1]?.[0] as RecordEditDoc;
};
const lastCommit = (p: CameraTabProps) => lastOf(p.onCommit);

describe("camera tab", () => {
  it("no_camera_disables_tab", () => {
    mount(doc(), { view: { doc: doc(), layoutSpans: [], clipCameras: [false] } });
    expect(screen.getByTestId("ed-camera-none").textContent).toMatch(/no camera/i);
    const box = screen.getByTestId("ed-camera-controls") as HTMLFieldSetElement;
    expect(box.disabled).toBe(true);
    expect(screen.getByRole("button", { name: "Dual frame" }).closest("fieldset")).toBe(box);
  });

  it("picking a layout changes the whole video when there are no regions", () => {
    const p = mount(doc());
    expect(screen.getByTestId("ed-camera-scope").textContent).toMatch(/whole video/i);
    fireEvent.click(screen.getByRole("button", { name: "Dual frame" }));
    expect(lastCommit(p).camera.preset).toBe("dual-frame");
  });

  it("with regions: 'from here' edits the region under the playhead, 'whole video' the global preset", () => {
    const d = doc({ layoutRegions: [{ id: "r", clip: 0, startMs: 4_000, preset: "camera-only" }] });
    const view = { doc: d, layoutSpans: [{ index: 0, progStartMs: 4_000, progEndMs: 10_000 }], clipCameras: [true] };
    const p = mount(d, { view, positionMs: 5_000 });
    // The playhead sits in the region: "from here" is the default.
    expect(screen.getByRole("button", { name: "From here" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("button", { name: "Camera only" }).getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "Vertical stack" }));
    expect(lastCommit(p).layoutRegions[0].preset).toBe("vertical-stack");
    expect(lastCommit(p).camera.preset).toBe("picture-in-picture");
    fireEvent.click(screen.getByRole("button", { name: "Whole video" }));
    fireEvent.click(screen.getByRole("button", { name: "Dual frame" }));
    expect(lastCommit(p).camera.preset).toBe("dual-frame");
    expect(lastCommit(p).layoutRegions[0].preset).toBe("camera-only");
  });

  it("a selected region far from the playhead is the one edited", () => {
    const d = doc({ layoutRegions: [{ id: "far", clip: 0, startMs: 8_000, preset: "camera-only" }] });
    const view = { doc: d, layoutSpans: [{ index: 0, progStartMs: 8_000, progEndMs: 10_000 }], clipCameras: [true] };
    const p = mount(d, { view, positionMs: 2_000, selected: "far" });
    expect(screen.getByRole("button", { name: "From here" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByTestId("ed-camera-scope").textContent).toContain("0:08");
    expect(screen.getByRole("button", { name: "Camera only" }).getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "Dual frame" }));
    const next = lastCommit(p);
    expect(next.layoutRegions).toEqual([{ id: "far", clip: 0, startMs: 8_000, preset: "dual-frame" }]);
    expect(next.camera.preset).toBe("picture-in-picture");
    expect(p.onAddRegion).not.toHaveBeenCalled();
  });

  it("'from here' before any region adds one at the playhead", () => {
    const d = doc({ layoutRegions: [{ id: "r", clip: 0, startMs: 4_000, preset: "camera-only" }] });
    const view = { doc: d, layoutSpans: [{ index: 0, progStartMs: 4_000, progEndMs: 10_000 }], clipCameras: [true] };
    const p = mount(d, { view, positionMs: 1_000 });
    fireEvent.click(screen.getByRole("button", { name: "From here" }));
    fireEvent.click(screen.getByRole("button", { name: "Dual frame" }));
    expect(p.onAddRegion).toHaveBeenCalledWith("dual-frame");
    expect(p.onCommit).not.toHaveBeenCalled();
  });

  it("shrink_toggle_only_for_pip", () => {
    mount(doc());
    expect(screen.queryByLabelText("Shrink during a zoom")).not.toBeNull();
    cleanup();
    mount(doc({ camera: camera({ preset: "dual-frame" }) }));
    expect(screen.queryByLabelText("Shrink during a zoom")).toBeNull();
    // Mask, size and position belong to picture-in-picture too.
    expect(screen.queryByLabelText("Size")).toBeNull();
  });

  it("mask change resets an untouched rounding, keeps a tuned one; Standard resets", () => {
    const p = mount(doc());
    fireEvent.click(screen.getByRole("button", { name: "Rectangle" }));
    expect(lastCommit(p).camera).toMatchObject({ mask: "rectangle", roundness: null });
    cleanup();
    const q = mount(doc({ camera: camera({ roundness: 0.2 }) }));
    fireEvent.click(screen.getByRole("button", { name: "Rectangle" }));
    expect(lastCommit(q).camera).toMatchObject({ mask: "rectangle", roundness: 0.2 });
    fireEvent.click(screen.getByRole("button", { name: "Standard rounding" }));
    expect(lastCommit(q).camera.roundness).toBeNull();
  });

  it("size moves 15–60 % in steps of 1 and the anchor grid has no middle", () => {
    const p = mount(doc());
    const size = screen.getByLabelText("Size") as HTMLInputElement;
    expect([size.min, size.max, size.step]).toEqual(["15", "60", "1"]);
    fireEvent.change(size, { target: { value: "55" } });
    expect(lastOf(p.onLive).camera.sizePct).toBe(55);
    const grid = screen.getByRole("group", { name: "Position" });
    expect(within(grid).getAllByRole("button")).toHaveLength(8);
    fireEvent.click(within(grid).getByRole("button", { name: "Top left" }));
    expect(lastCommit(p).camera.anchor).toBe("top-left");
  });

  it("segmentation_unavailable_disables_chips", () => {
    mount(doc(), { segmentation: false });
    for (const name of ["Transparent", "Blur", "Own background"]) {
      expect((screen.getByRole("button", { name }) as HTMLButtonElement).disabled).toBe(true);
    }
    expect((screen.getByRole("button", { name: "No effect" }) as HTMLButtonElement).disabled).toBe(false);
    expect(screen.getByTestId("ed-camera-seg-off")).toBeTruthy();
    cleanup();
    const p = mount(doc(), { segmentation: true });
    fireEvent.click(screen.getByRole("button", { name: "Blur" }));
    expect(lastCommit(p).camera.background.mode).toBe("blur");
    expect(screen.queryByTestId("ed-camera-seg-off")).toBeNull();
  });

  it("while the helper is still asked, the effects stay offered (only a definite no locks them)", () => {
    mount(doc(), { segmentation: null });
    expect((screen.getByRole("button", { name: "Blur" }) as HTMLButtonElement).disabled).toBe(false);
    expect(screen.queryByTestId("ed-camera-seg-off")).toBeNull();
  });

  it("blur strength and the own background show with their mode", () => {
    mount(doc({ camera: camera({ background: { mode: "blur", blur: 0.5, custom: { kind: "color", color: "#000000" } } }) }));
    expect(screen.getByLabelText("Blur strength")).toBeTruthy();
    cleanup();
    const p = mount(doc({ camera: camera({ background: { mode: "custom", blur: 0.5, custom: { kind: "color", color: "#000000" } } }) }));
    expect(screen.queryByLabelText("Blur strength")).toBeNull();
    fireEvent.click(screen.getAllByRole("button", { name: /^Colour / })[1]);
    expect(lastCommit(p).camera.background.custom.kind).toBe("color");
  });

  it("mirror is one undo step", () => {
    const p = mount(doc());
    fireEvent.click(screen.getByLabelText("Mirror"));
    expect(lastCommit(p).camera.mirror).toBe(true);
  });
});
