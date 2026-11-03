// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";

const status = vi.fn(async () => [{ id: "m", file: "m.bin", bytes: 1, installed: false }]);
const listeners = new Map<string, (e: { payload: unknown }) => void>();

vi.mock("../../lib/ipc", () => ({
  EV_RECORD_TRANSCRIBE: "record-editor-transcribe",
  editorTranscribe: vi.fn(),
  editorTranscribeCancel: vi.fn(),
  getMeetingConfig: async () => ({ model: "m" }),
  meetingModelStatus: () => status(),
  meetingModelDownload: vi.fn(async () => undefined),
  meetingModelCancel: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, cb: (e: { payload: unknown }) => void) => {
    listeners.set(name, cb);
    return () => listeners.delete(name);
  },
}));

import { useTranscription } from "./useTranscription";

afterEach(() => {
  cleanup();
  status.mockClear();
});

const meeting = (download: unknown) => act(async () => listeners.get("meeting-changed")?.({ payload: { download } }));

describe("useTranscription", () => {
  it("re-reads the model status only when a download ends, not on every meeting event", async () => {
    renderHook(() => useTranscription(() => 1, () => undefined));
    await act(async () => undefined);
    expect(status).toHaveBeenCalledTimes(1);
    // A meeting runs: events every line, no download.
    await meeting(null);
    await meeting(null);
    expect(status).toHaveBeenCalledTimes(1);
    // A download runs and ends: one refresh.
    await meeting({ model: "m", done: 5, total: 10, error: null });
    await meeting({ model: "m", done: 9, total: 10, error: null });
    expect(status).toHaveBeenCalledTimes(1);
    await meeting(null);
    expect(status).toHaveBeenCalledTimes(2);
    await meeting(null);
    expect(status).toHaveBeenCalledTimes(2);
  });
});
