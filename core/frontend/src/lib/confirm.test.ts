import { afterEach, describe, expect, it, vi } from "vitest";

// The two impure edges are mocked so the CONTRACT is testable: destructive
// confirmation must (1) return the native dialog's verdict, (2) fail CLOSED
// on any plugin error, (3) always suppress the popup's hide-on-focus-loss for
// the dialog's lifetime and always release it afterwards — even on failure.
const askMock = vi.fn();
const suppressMock = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: (...a: unknown[]) => askMock(...a) }));
vi.mock("./ipc", () => ({ setSuppressHide: (...a: unknown[]) => suppressMock(...a) }));

import { confirmDialog } from "./confirm";

afterEach(() => {
  askMock.mockReset();
  suppressMock.mockReset();
});

describe("confirmDialog", () => {
  it("returns the native dialog's verdict (yes and no)", async () => {
    suppressMock.mockResolvedValue(undefined);
    askMock.mockResolvedValue(true);
    await expect(confirmDialog("Delete everything?")).resolves.toBe(true);
    askMock.mockResolvedValue(false);
    await expect(confirmDialog("Delete everything?")).resolves.toBe(false);
  });

  it("FAILS CLOSED when the dialog plugin throws — never 'assumed yes'", async () => {
    suppressMock.mockResolvedValue(undefined);
    askMock.mockRejectedValue(new Error("plugin unavailable"));
    const spy = vi.spyOn(console, "error").mockImplementation(() => undefined);
    await expect(confirmDialog("Delete everything?")).resolves.toBe(false);
    spy.mockRestore();
  });

  it("suppresses hide before the dialog and releases it after — also on failure", async () => {
    const calls: Array<boolean> = [];
    suppressMock.mockImplementation((v: boolean) => {
      calls.push(v);
      return Promise.resolve();
    });
    askMock.mockResolvedValue(true);
    await confirmDialog("x");
    expect(calls).toEqual([true, false]);

    calls.length = 0;
    askMock.mockRejectedValue(new Error("boom"));
    const spy = vi.spyOn(console, "error").mockImplementation(() => undefined);
    await confirmDialog("x");
    spy.mockRestore();
    expect(calls).toEqual([true, false]); // released even when ask() threw
  });

  it("a failing suppress-hide IPC never blocks the dialog itself", async () => {
    suppressMock.mockRejectedValue(new Error("ipc down"));
    askMock.mockResolvedValue(true);
    await expect(confirmDialog("x")).resolves.toBe(true);
  });

  it("passes message and warning-kind title through to the native dialog", async () => {
    suppressMock.mockResolvedValue(undefined);
    askMock.mockResolvedValue(true);
    await confirmDialog("Wipe it?", "Really?");
    expect(askMock).toHaveBeenCalledWith("Wipe it?", { title: "Really?", kind: "warning" });
    await confirmDialog("Wipe it?");
    expect(askMock).toHaveBeenLastCalledWith("Wipe it?", {
      title: "Are you sure?",
      kind: "warning",
    });
  });
});
