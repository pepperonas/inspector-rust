import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";

const recordStart = vi.fn().mockResolvedValue({});
vi.mock("../lib/ipc", () => ({
  recordStatus: async () => ({ supported: true, reason: null, phase: "idle", elapsedMs: 0, projectDir: null, source: "Display", lowDiskMinutes: null }),
  getRecordConfig: async () => ({ projectDir: "", countdown: true, cursorMode: "editable-overlay", systemAudio: true, mic: false }),
  setRecordConfig: async (c: unknown) => c,
  recordStart: (o: unknown) => recordStart(o),
  recordPickSource: async () => undefined,
  recordOpenNotes: async () => undefined,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({ getCurrentWebviewWindow: () => ({ close: async () => undefined }) }));

import { RecordHud } from "./RecordHud";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
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
