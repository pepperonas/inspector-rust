import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";

const recordStart = vi.fn().mockResolvedValue({});
const setRecordConfig = vi.fn(async (c: unknown) => c);
const recordCameraRequest = vi.fn(async () => "authorized");
const recordOpenCameraSettings = vi.fn(async () => undefined);
let cameraAuth = "authorized";
let cameraDevices = [{ id: "mac", name: "FaceTime HD Camera", builtin: true }];
vi.mock("../lib/ipc", () => ({
  recordStatus: async () => ({ supported: true, reason: null, phase: "idle", elapsedMs: 0, projectDir: null, source: "Display", lowDiskMinutes: null }),
  getRecordConfig: async () => ({
    projectDir: "",
    countdown: true,
    cursorMode: "editable-overlay",
    systemAudio: true,
    mic: false,
    camera: false,
    cameraDevice: "",
    cameraQuality: "1080p",
  }),
  setRecordConfig: (c: unknown) => setRecordConfig(c),
  recordStart: (o: unknown) => recordStart(o),
  recordPickSource: async () => undefined,
  recordOpenNotes: async () => undefined,
  recordCameraAuth: async () => cameraAuth,
  recordCameraDevices: async () => cameraDevices,
  recordCameraRequest: () => recordCameraRequest(),
  recordOpenCameraSettings: () => recordOpenCameraSettings(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({ getCurrentWebviewWindow: () => ({ close: async () => undefined }) }));

import { RecordHud } from "./RecordHud";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  cameraAuth = "authorized";
  cameraDevices = [{ id: "mac", name: "FaceTime HD Camera", builtin: true }];
});

describe("RecordHud Enter", () => {
  it("does not start from a focused toggle, but from the body", async () => {
    const { findAllByRole } = render(<RecordHud />);
    await findAllByRole("switch");
    const sw = (await findAllByRole("switch"))[0];
    await waitFor(() => expect(document.body.textContent).toContain("Display"));
    fireEvent.keyDown(sw, { key: "Enter" });
    expect(recordStart).not.toHaveBeenCalled();
    fireEvent.keyDown(document.body, { key: "Enter" });
    expect(recordStart).toHaveBeenCalledTimes(1);
  });
});

describe("RecordHud camera switch", () => {
  const cameraSwitch = async (r: ReturnType<typeof render>) => r.findByRole("switch", { name: /camera/i });

  it("with the permission denied it shows the hint + settings button and stays off", async () => {
    cameraAuth = "denied";
    const r = render(<RecordHud />);
    const sw = await cameraSwitch(r);
    await act(async () => {});
    fireEvent.click(sw);
    await act(async () => {});
    expect(sw.getAttribute("aria-checked")).toBe("false");
    expect(r.getByTestId("rec-camera-denied")).toBeTruthy();
    expect(recordCameraRequest).not.toHaveBeenCalled();
    expect(setRecordConfig).not.toHaveBeenCalledWith(expect.objectContaining({ camera: true }));
    fireEvent.click(r.getByTestId("rec-camera-settings"));
    expect(recordOpenCameraSettings).toHaveBeenCalledTimes(1);
  });

  it("with the permission undecided it asks exactly once, then turns on", async () => {
    cameraAuth = "not-determined";
    const r = render(<RecordHud />);
    const sw = await cameraSwitch(r);
    await act(async () => {});
    fireEvent.click(sw);
    await act(async () => {});
    expect(recordCameraRequest).toHaveBeenCalledTimes(1);
    expect(sw.getAttribute("aria-checked")).toBe("true");
    expect(setRecordConfig).toHaveBeenCalledWith(expect.objectContaining({ camera: true }));
    expect(r.queryByTestId("rec-camera-denied")).toBeNull();
  });

  it("a 'no' in the dialog shows the hint and stays off", async () => {
    cameraAuth = "not-determined";
    recordCameraRequest.mockResolvedValueOnce("denied");
    const r = render(<RecordHud />);
    const sw = await cameraSwitch(r);
    await act(async () => {});
    fireEvent.click(sw);
    await act(async () => {});
    expect(recordCameraRequest).toHaveBeenCalledTimes(1);
    expect(sw.getAttribute("aria-checked")).toBe("false");
    expect(r.getByTestId("rec-camera-denied")).toBeTruthy();
  });

  it("offers a device choice only with more than one camera, and starts with the camera options", async () => {
    const one = render(<RecordHud />);
    fireEvent.click(await cameraSwitch(one));
    await act(async () => {});
    expect(one.queryByTestId("rec-camera-device")).toBeNull();
    cleanup();

    cameraDevices = [
      { id: "mac", name: "FaceTime HD Camera", builtin: true },
      { id: "usb", name: "USB Cam", builtin: false },
    ];
    const r = render(<RecordHud />);
    fireEvent.click(await cameraSwitch(r));
    await act(async () => {});
    const sel = r.getByTestId("rec-camera-device") as HTMLSelectElement;
    fireEvent.change(sel, { target: { value: "usb" } });
    await waitFor(() => expect(document.body.textContent).toContain("Display"));
    fireEvent.keyDown(document.body, { key: "Enter" });
    expect(recordStart).toHaveBeenCalledWith(
      expect.objectContaining({ camera: true, cameraDevice: "usb", cameraQuality: "1080p" }),
    );
  });
});
