import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render } from "@testing-library/react";

const setRecordShortcuts = vi.fn(async (s: unknown) => s);
const setRecordConfig = vi.fn(async (c: unknown) => c);
const recordCameraRequest = vi.fn(async () => "authorized");
let cameraAuth = "authorized";
let cliPath: string | null = "/Applications/InspectorRust.app/Contents/MacOS/ir-record";
vi.mock("../lib/ipc", () => ({
  getRecordConfig: async () => ({
    projectDir: "/Users/ana/Movies/Inspector Rust",
    countdown: true,
    cursorMode: "editable-overlay",
    systemAudio: true,
    mic: false,
    openEditor: true,
    autoZoom: true,
    camera: false,
    cameraDevice: "",
    cameraQuality: "1080p",
  }),
  setRecordConfig: (c: unknown) => setRecordConfig(c),
  recordCliPath: async () => cliPath,
  recordCameraAuth: async () => cameraAuth,
  recordCameraRequest: () => recordCameraRequest(),
  recordOpenCameraSettings: async () => undefined,
  recordCameraDevices: async () => [
    { id: "mac", name: "FaceTime HD Camera", builtin: true },
    { id: "usb", name: "USB Cam", builtin: false },
  ],
  EV_RECORD_SHORTCUTS: "record-shortcuts-changed",
  getRecordShortcuts: async () => ({}),
  setRecordShortcuts: (s: unknown) => setRecordShortcuts(s),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));

import { RecordSettingsSection } from "./RecordSettingsSection";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  cameraAuth = "authorized";
});

describe("Settings → Recording → camera", () => {
  it("saves camera on/off, the device and the quality", async () => {
    const r = render(<RecordSettingsSection />);
    await act(async () => {});
    fireEvent.click(r.getByTestId("set-record-camera"));
    await act(async () => {});
    expect(setRecordConfig).toHaveBeenLastCalledWith(expect.objectContaining({ camera: true }));
    const dev = r.getByTestId("set-record-camera-device") as HTMLSelectElement;
    expect(Array.from(dev.options).map((o) => o.value)).toEqual(["", "mac", "usb"]);
    fireEvent.change(dev, { target: { value: "usb" } });
    expect(setRecordConfig).toHaveBeenLastCalledWith(expect.objectContaining({ cameraDevice: "usb" }));
    await act(async () => {});
    const q = r.getByTestId("set-record-camera-quality") as HTMLSelectElement;
    expect(Array.from(q.options).map((o) => o.value)).toEqual(["1080p", "1440p", "2160p"]);
    fireEvent.change(q, { target: { value: "2160p" } });
    expect(setRecordConfig).toHaveBeenLastCalledWith(expect.objectContaining({ cameraQuality: "2160p", cameraDevice: "usb" }));
  });

  it("goes through the permission flow: asks once when undecided, refused stays off with the hint", async () => {
    cameraAuth = "not-determined";
    const r = render(<RecordSettingsSection />);
    await act(async () => {});
    fireEvent.click(r.getByTestId("set-record-camera"));
    await act(async () => {});
    expect(recordCameraRequest).toHaveBeenCalledTimes(1);
    expect(setRecordConfig).toHaveBeenLastCalledWith(expect.objectContaining({ camera: true }));
    cleanup();
    setRecordConfig.mockClear();

    cameraAuth = "denied";
    const d = render(<RecordSettingsSection />);
    await act(async () => {});
    const box = d.getByTestId("set-record-camera") as HTMLInputElement;
    fireEvent.click(box);
    await act(async () => {});
    expect(box.checked).toBe(false);
    expect(d.getByTestId("rec-camera-denied")).toBeTruthy();
    expect(setRecordConfig).not.toHaveBeenCalled();
  });
});

describe("Settings → Recording → editor shortcuts", () => {
  it("opens the shortcuts dialog, and no key reaches the popup's own handlers while it is open", async () => {
    const r = render(<RecordSettingsSection />);
    await act(async () => {});
    fireEvent.click(r.getByTestId("set-record-shortcuts"));
    expect(r.getByTestId("ed-shortcuts")).toBeTruthy();

    // Stand-ins for the popup: the bubble-phase Esc fallback and the list navigation.
    const popup = vi.fn();
    window.addEventListener("keydown", popup);
    document.addEventListener("keydown", popup);
    try {
      for (const k of ["ArrowDown", "Enter", "s", "Tab"]) {
        const ev = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true });
        act(() => {
          document.body.dispatchEvent(ev);
        });
        expect(ev.defaultPrevented, k).toBe(true);
      }
      // A rebind made here is saved as a deviation.
      fireEvent.click(r.getByTestId("ed-key-split"));
      act(() => {
        document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "x", bubbles: true, cancelable: true }));
      });
      await act(async () => {});
      expect(setRecordShortcuts).toHaveBeenCalledWith({ split: { key: "x" } });
      // Escape closes the dialog, not the popup.
      const esc = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
      act(() => {
        document.body.dispatchEvent(esc);
      });
      expect(esc.defaultPrevented).toBe(true);
      expect(r.queryByTestId("ed-shortcuts")).toBeNull();
      expect(popup).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("keydown", popup);
      document.removeEventListener("keydown", popup);
    }
  });
});

describe("Settings → Recording → command line", () => {
  it("shows the two setup lines with the bundle path", async () => {
    const r = render(<RecordSettingsSection />);
    await act(async () => {});
    const lines = r.getAllByTestId("set-record-cli-line").map((e) => e.textContent);
    expect(lines).toEqual([
      'claude mcp add inspector-rust-record -- "/Applications/InspectorRust.app/Contents/MacOS/ir-record" mcp',
      'ln -s "/Applications/InspectorRust.app/Contents/MacOS/ir-record" ~/.local/bin/ir-record',
    ]);
  });

  it("explains a dev build instead", async () => {
    cliPath = null;
    const r = render(<RecordSettingsSection />);
    await act(async () => {});
    expect(r.queryByTestId("set-record-cli")).toBeNull();
    expect(r.getByTestId("set-record-cli-dev")).toBeTruthy();
    cliPath = "/Applications/InspectorRust.app/Contents/MacOS/ir-record";
  });
});
