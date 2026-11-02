// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { RecordProjectItem } from "../../lib/ipc";

const list = vi.fn();
vi.mock("../../lib/ipc", () => ({ recordListProjects: () => list() }));
const { TakePicker, insertableTakes } = await import("./TakePicker");

const item = (dir: string, over: Partial<RecordProjectItem> = {}): RecordProjectItem => ({
  dir,
  title: dir.split("/").pop() ?? dir,
  createdAt: 0,
  durationMs: 4_000,
  status: "ready",
  failure: null,
  active: false,
  ...over,
});

afterEach(() => {
  cleanup();
  list.mockReset();
});

describe("insertableTakes", () => {
  it("offers only other finished recordings", () => {
    const all = [
      item("/r/open"),
      item("/r/other"),
      item("/r/failed", { status: "failed" }),
      item("/r/rec", { status: "recording", active: true }),
      item("/r/broken", { status: "incomplete" }),
      item("/r/empty", { durationMs: 0 }),
    ];
    expect(insertableTakes(all, "/r/open/").map((p) => p.dir)).toEqual(["/r/other"]);
  });
});

describe("TakePicker", () => {
  it("lists the other recordings and picks one", async () => {
    list.mockResolvedValue([item("/r/open"), item("/r/b-take")]);
    const onPick = vi.fn();
    render(<TakePicker currentDir="/r/open" onPick={onPick} onClose={() => undefined} onError={() => undefined} />);
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    fireEvent.click(screen.getByText("b-take"));
    expect(onPick).toHaveBeenCalledWith("/r/b-take");
    expect(screen.queryByText("open")).toBeNull();
  });

  it("Escape closes it", () => {
    list.mockResolvedValue([]);
    const onClose = vi.fn();
    render(<TakePicker currentDir="/r/open" onPick={() => undefined} onClose={onClose} onError={() => undefined} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});
