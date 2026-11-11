import { describe, it, expect, vi, afterEach } from "vitest";
import { whenElement } from "./when-element";

const tick = () => new Promise((r) => setTimeout(r, 0));

describe("whenElement", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("calls at once for an element that is already there", () => {
    document.body.innerHTML = '<div id="settings-a"></div>';
    const found = vi.fn();
    whenElement("settings-a", document.body, 1000, found);
    expect(found).toHaveBeenCalledTimes(1);
  });

  it("waits for an element that appears later (a section still loading)", async () => {
    const found = vi.fn();
    whenElement("settings-late", document.body, 1000, found);
    expect(found).not.toHaveBeenCalled();
    const other = document.createElement("div");
    document.body.appendChild(other);
    await tick();
    expect(found).not.toHaveBeenCalled();
    const el = document.createElement("section");
    el.id = "settings-late";
    other.appendChild(el);
    await tick();
    expect(found).toHaveBeenCalledTimes(1);
    expect(found).toHaveBeenCalledWith(el);
    // Once is enough: later changes do not call again.
    document.body.appendChild(document.createElement("p"));
    await tick();
    expect(found).toHaveBeenCalledTimes(1);
  });

  it("cancelling stops the wait and runs the cleanup of a found element", async () => {
    const found = vi.fn();
    const cancel = whenElement("settings-x", document.body, 1000, found);
    cancel();
    const el = document.createElement("div");
    el.id = "settings-x";
    document.body.appendChild(el);
    await tick();
    expect(found).not.toHaveBeenCalled();

    const cleanup = vi.fn();
    const cancel2 = whenElement("settings-x", document.body, 1000, () => cleanup);
    cancel2();
    expect(cleanup).toHaveBeenCalledTimes(1);
  });

  it("gives up after the timeout", async () => {
    vi.useFakeTimers();
    const found = vi.fn();
    whenElement("settings-never", document.body, 500, found);
    vi.advanceTimersByTime(501);
    vi.useRealTimers();
    const el = document.createElement("div");
    el.id = "settings-never";
    document.body.appendChild(el);
    await tick();
    expect(found).not.toHaveBeenCalled();
  });
});
