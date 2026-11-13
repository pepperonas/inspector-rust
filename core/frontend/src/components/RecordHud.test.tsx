import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";

const recordStart = vi.fn().mockResolvedValue({});
const recordPickSource = vi.fn(async (_k: unknown) => undefined);
const recordPickRegion = vi.fn(async () => undefined);
const setRecordConfig = vi.fn(async (c: unknown) => c);
const recordCameraRequest = vi.fn(async () => "authorized");
const recordOpenCameraSettings = vi.fn(async () => undefined);
let cameraAuth = "authorized";
/** Set to keep the permission read pending (still loading). */
let authPending = false;
let storedCamera = false;
let cameraDevices = [{ id: "mac", name: "FaceTime HD Camera", builtin: true }];
let storedMic = false;
let storedMicDevice = "";
const micDevices = [
  { id: "MacBook Pro Microphone", name: "MacBook Pro Microphone", isDefault: true },
  { id: "Yeti", name: "Yeti", isDefault: false },
];
const recordMicMeterStart = vi.fn(async (_d: string) => undefined);
const recordMicMeterStop = vi.fn(async () => undefined);
const recordCameraPreviewStart = vi.fn(async (_d: string) => undefined);
const recordCameraPreviewStop = vi.fn(async () => undefined);
vi.mock("../lib/ipc", () => ({
  recordStatus: async () => ({ supported: true, reason: null, phase: "idle", elapsedMs: 0, projectDir: null, source: "Display", lowDiskMinutes: null }),
  getRecordConfig: async () => ({
    projectDir: "",
    countdown: true,
    cursorMode: "editable-overlay",
    systemAudio: true,
    mic: storedMic,
    micDevice: storedMicDevice,
    camera: storedCamera,
    cameraDevice: "",
    cameraQuality: "1080p",
  }),
  setRecordConfig: (c: unknown) => setRecordConfig(c),
  recordStart: (o: unknown) => recordStart(o),
  recordPickSource: (k: unknown) => recordPickSource(k),
  recordPickRegion: () => recordPickRegion(),
  recordOpenNotes: async () => undefined,
  recordCameraAuth: () => (authPending ? new Promise(() => undefined) : Promise.resolve(cameraAuth)),
  recordCameraDevices: async () => cameraDevices,
  recordCameraRequest: () => recordCameraRequest(),
  recordOpenCameraSettings: () => recordOpenCameraSettings(),
  recordMicDevices: async () => micDevices,
  recordMicMeterStart: (d: string) => recordMicMeterStart(d),
  recordMicMeterStop: () => recordMicMeterStop(),
  recordMicLevel: async () => ({ level: 0.5, dbfs: -30, fellBack: false, live: true }),
  recordCameraPreviewStart: (d: string) => recordCameraPreviewStart(d),
  recordCameraPreviewStop: () => recordCameraPreviewStop(),
  recordCameraPreviewFrame: async () => new ArrayBuffer(0),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({ getCurrentWebviewWindow: () => ({ close: async () => undefined }) }));

import { RecordHud } from "./RecordHud";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  cameraAuth = "authorized";
  authPending = false;
  storedCamera = false;
  cameraDevices = [{ id: "mac", name: "FaceTime HD Camera", builtin: true }];
  storedMic = false;
  storedMicDevice = "";
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

  it("a stored 'on' is off until the permission is granted (undecided or still loading)", async () => {
    storedCamera = true;
    for (const setup of [() => (cameraAuth = "not-determined"), () => (authPending = true)]) {
      setup();
      const r = render(<RecordHud />);
      const sw = await cameraSwitch(r);
      await act(async () => {});
      expect(sw.getAttribute("aria-checked")).toBe("false");
      await waitFor(() => expect(document.body.textContent).toContain("Display"));
      fireEvent.keyDown(document.body, { key: "Enter" });
      expect(recordStart).toHaveBeenLastCalledWith(expect.objectContaining({ camera: false }));
      cleanup();
      recordStart.mockClear();
    }
  });

  it("shows no denied hint on open unless the camera was stored on", async () => {
    cameraAuth = "denied";
    const r = render(<RecordHud />);
    await cameraSwitch(r);
    await act(async () => {});
    expect(r.queryByTestId("rec-camera-denied")).toBeNull();
    cleanup();
    storedCamera = true;
    const r2 = render(<RecordHud />);
    await cameraSwitch(r2);
    await act(async () => {});
    expect(r2.getByTestId("rec-camera-denied")).toBeTruthy();
  });

  it("a config change made while the dialog is open survives turning the camera on", async () => {
    cameraAuth = "not-determined";
    let answer: (a: string) => void = () => undefined;
    recordCameraRequest.mockImplementationOnce(() => new Promise<string>((res) => (answer = res)));
    const r = render(<RecordHud />);
    const sw = await cameraSwitch(r);
    await act(async () => {});
    fireEvent.click(sw);
    fireEvent.click(r.getByRole("switch", { name: /microphone/i }));
    await act(async () => answer("authorized"));
    expect(setRecordConfig).toHaveBeenLastCalledWith(expect.objectContaining({ camera: true, mic: true }));
  });

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

  it("a permission granted in System Settings after a refusal counts on the next try (no reopen)", async () => {
    cameraAuth = "denied";
    const r = render(<RecordHud />);
    const sw = await cameraSwitch(r);
    await act(async () => {});
    fireEvent.click(sw);
    await act(async () => {});
    expect(sw.getAttribute("aria-checked")).toBe("false");
    // The user allows the camera in System Settings while the HUD stays open.
    cameraAuth = "authorized";
    fireEvent.click(sw);
    await act(async () => {});
    expect(sw.getAttribute("aria-checked")).toBe("true");
    expect(r.queryByTestId("rec-camera-denied")).toBeNull();
    expect(recordCameraRequest).not.toHaveBeenCalled();
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

describe("RecordHud layout", () => {
  // happy-dom does no layout, so the measured fit lives in session.rs (`HUD_SIZE`); this pins
  // the structure that keeps Record reachable when the middle is taller than the window.
  it("keeps the Record row outside the scrolling middle, everything else inside it", async () => {
    storedCamera = true;
    cameraAuth = "denied";
    const { findByText, getByTestId } = render(<RecordHud />);
    await findByText("Display", { exact: false });
    const body = getByTestId("rec-hud-body");
    const footer = getByTestId("rec-hud-footer");
    expect(body.className).toContain("overflow-y-auto");
    expect(body.className).toContain("min-h-0");
    expect(footer.className).toContain("shrink-0");
    const record = [...footer.querySelectorAll("button")].find((b) => b.querySelector("span.rounded-full"));
    expect(record).toBeTruthy();
    expect(body.contains(record!)).toBe(false);
    // The option toggles, the denied hint and the title field scroll with the middle.
    await waitFor(() => expect(body.querySelector('[data-testid="rec-camera-denied"]')).toBeTruthy());
    expect(body.querySelectorAll('[role="switch"]').length).toBe(5);
    expect(body.querySelector("input")).toBeTruthy();
  });
});

describe("RecordHud sources", () => {
  it("offers display, window and a free area; the area goes to its own command", async () => {
    const { findByRole } = render(<RecordHud />);
    fireEvent.click(await findByRole("button", { name: /Area/ }));
    expect(recordPickRegion).toHaveBeenCalledTimes(1);
    expect(recordPickSource).not.toHaveBeenCalled();
    fireEvent.click(await findByRole("button", { name: /Window/ }));
    expect(recordPickSource).toHaveBeenLastCalledWith("window");
  });
});

describe("RecordHud microphone", () => {
  it("offers the microphones with a level meter only while the mic is on", async () => {
    const r = render(<RecordHud />);
    await waitFor(() => expect(document.body.textContent).toContain("Display"));
    expect(r.queryByTestId("rec-mic-device")).toBeNull();
    expect(recordMicMeterStart).not.toHaveBeenCalled();
    fireEvent.click(await r.findByRole("switch", { name: /microphone/i }));
    const select = (await r.findByTestId("rec-mic-device")) as HTMLSelectElement;
    await waitFor(() => expect(select.options.length).toBe(3));
    expect(r.getByTestId("rec-mic-meter")).toBeTruthy();
    await waitFor(() => expect(recordMicMeterStart).toHaveBeenCalledWith(""));
    fireEvent.change(select, { target: { value: "Yeti" } });
    await waitFor(() => expect(recordMicMeterStart).toHaveBeenCalledWith("Yeti"));
    await waitFor(() => expect(setRecordConfig).toHaveBeenLastCalledWith(expect.objectContaining({ mic: true, micDevice: "Yeti" })));
    fireEvent.keyDown(document.body, { key: "Enter" });
    expect(recordStart).toHaveBeenCalledWith(expect.objectContaining({ mic: true, micDevice: "Yeti" }));
    // The take needs the microphone: the meter lets go of it.
    await waitFor(() => expect(r.queryByTestId("rec-mic-meter")).toBeNull());
    expect(recordMicMeterStop).toHaveBeenCalled();
  });

  it("names a stored microphone that is not connected", async () => {
    storedMic = true;
    storedMicDevice = "Rode NT#2";
    const r = render(<RecordHud />);
    const hint = await r.findByTestId("rec-mic-missing");
    expect(hint.textContent).toContain("Rode NT");
    expect(hint.textContent).not.toContain("#2");
    expect(((await r.findByTestId("rec-mic-device")) as HTMLSelectElement).value).toBe("");
  });
});

describe("RecordHud camera picture", () => {
  it("shows the live picture only with the camera on, and frees the camera for the take", async () => {
    storedCamera = true;
    const r = render(<RecordHud />);
    await r.findByTestId("rec-camera-preview");
    await waitFor(() => expect(recordCameraPreviewStart).toHaveBeenCalledWith(""));
    expect(r.getByTestId("rec-camera-quality")).toBeTruthy();
    fireEvent.click(r.getByText("4K"));
    await waitFor(() => expect(setRecordConfig).toHaveBeenLastCalledWith(expect.objectContaining({ cameraQuality: "2160p" })));
    await waitFor(() => expect(document.body.textContent).toContain("Display"));
    fireEvent.keyDown(document.body, { key: "Enter" });
    await waitFor(() => expect(r.queryByTestId("rec-camera-preview")).toBeNull());
    expect(recordCameraPreviewStop).toHaveBeenCalled();
    expect(recordStart).toHaveBeenCalledWith(expect.objectContaining({ camera: true, cameraQuality: "2160p" }));
  });

  it("no picture and no camera access while the camera is off", async () => {
    const r = render(<RecordHud />);
    await waitFor(() => expect(document.body.textContent).toContain("Display"));
    expect(r.queryByTestId("rec-camera-preview")).toBeNull();
    expect(recordCameraPreviewStart).not.toHaveBeenCalled();
  });
});

describe("RecordHud camera only", () => {
  it("records the camera alone: no screen source needed, no system sound or pointer toggles", async () => {
    const r = render(<RecordHud />);
    await waitFor(() => expect(document.body.textContent).toContain("Display"));
    fireEvent.click(r.getByText("Camera only"));
    await waitFor(() => expect(r.getByTestId("rec-source").textContent).toContain("Camera only"));
    expect(r.queryByRole("switch", { name: /system audio/i })).toBeNull();
    expect(r.queryByRole("switch", { name: /camera/i })).toBeNull();
    expect(r.getByRole("switch", { name: /microphone/i })).toBeTruthy();
    await r.findByTestId("rec-camera-preview");
    fireEvent.keyDown(document.body, { key: "Enter" });
    expect(recordStart).toHaveBeenCalledWith(expect.objectContaining({ cameraOnly: true, camera: true }));
    // The stored camera setting is not touched by choosing this source.
    expect(setRecordConfig).not.toHaveBeenCalledWith(expect.objectContaining({ camera: true }));
  });

  it("picking a screen source again leaves camera-only", async () => {
    const r = render(<RecordHud />);
    await waitFor(() => expect(document.body.textContent).toContain("Display"));
    fireEvent.click(r.getByText("Camera only"));
    await waitFor(() => expect(r.getByTestId("rec-source").textContent).toContain("Camera only"));
    fireEvent.click(r.getByText("Window"));
    await waitFor(() => expect(r.getByTestId("rec-source").textContent).not.toContain("Camera only"));
    fireEvent.keyDown(document.body, { key: "Enter" });
    expect(recordStart).toHaveBeenCalledWith(expect.objectContaining({ cameraOnly: false }));
  });

  it("without the camera permission it stays a screen take and says why", async () => {
    cameraAuth = "denied";
    const r = render(<RecordHud />);
    await waitFor(() => expect(document.body.textContent).toContain("Display"));
    fireEvent.click(r.getByText("Camera only"));
    await r.findByTestId("rec-camera-denied");
    expect(r.getByTestId("rec-source").textContent).not.toContain("Camera only");
    expect(r.queryByTestId("rec-camera-preview")).toBeNull();
  });
});
