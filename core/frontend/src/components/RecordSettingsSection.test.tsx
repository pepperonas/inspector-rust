import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render } from "@testing-library/react";

const setRecordShortcuts = vi.fn(async (s: unknown) => s);
vi.mock("../lib/ipc", () => ({
  getRecordConfig: async () => ({
    projectDir: "/Users/ana/Movies/Inspector Rust",
    countdown: true,
    cursorMode: "editable-overlay",
    systemAudio: true,
    mic: false,
    openEditor: true,
    autoZoom: true,
  }),
  setRecordConfig: async (c: unknown) => c,
  EV_RECORD_SHORTCUTS: "record-shortcuts-changed",
  getRecordShortcuts: async () => ({}),
  setRecordShortcuts: (s: unknown) => setRecordShortcuts(s),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));

import { RecordSettingsSection } from "./RecordSettingsSection";

afterEach(cleanup);

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
