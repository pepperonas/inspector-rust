import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@testing-library/react";
import type { DiskScan, DiskNode } from "../lib/ipc";

// The panel scans on mount and listens for progress events — both stubbed so
// it renders standalone. `diskScan` is the interesting one: how OFTEN and with
// WHICH path it is called is exactly what these tests are about.
const diskScan = vi.fn<(path: string | null) => Promise<DiskScan>>();
const diskTrash = vi.fn(async () => undefined);
vi.mock("../lib/ipc", () => ({
  diskScan: (p: string | null) => diskScan(p),
  diskTrash: () => diskTrash(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async () => () => undefined,
}));
vi.mock("../lib/confirm", () => ({ confirmDialog: async () => true }));
// Reduced motion keeps the arcs free of entrance classes; irrelevant here.
vi.mock("../lib/md3-motion", () => ({ prefersReducedMotion: () => true }));

import { DiskPanel } from "./DiskPanel";

const dir = (name: string, size: number, children?: DiskNode[]): DiskNode => ({
  name,
  size,
  is_dir: true,
  child_count: children?.length ?? 0,
  ...(children ? { children } : {}),
});

/** A tree whose "leafdir" has NO children — the walk's pruning boundary. */
function scanOf(rootPath: string): DiskScan {
  return {
    root_path: rootPath,
    root_name: rootPath.split("/").filter(Boolean).pop() ?? "/",
    total: 300,
    volume_mount: "/",
    volume_total: 1000,
    volume_free: 700,
    is_volume_root: false,
    tree: dir("root", 300, [dir("deep", 200, [dir("inner", 200)]), dir("leafdir", 100)]),
    top_files: [],
    items: 42,
  };
}

/** Find a rendered arc by hovering until the hub names it — black-box, no
 *  test-only attributes in the component. */
function arcNamed(container: HTMLElement, name: string): Element {
  for (const p of Array.from(container.querySelectorAll("path"))) {
    fireEvent.mouseEnter(p);
    if (container.textContent?.includes(name)) return p;
  }
  throw new Error(`no arc named ${name}`);
}

const settled = (c: HTMLElement, path: string) =>
  waitFor(() => expect(c.textContent).toContain(path.split("/").filter(Boolean).pop()!));

beforeEach(() => {
  diskScan.mockReset();
  diskScan.mockImplementation(async (p) => scanOf(p ?? "/Users/martin"));
});
afterEach(cleanup);

describe("scanning", () => {
  it("scans exactly ONCE on mount", async () => {
    const { container } = render(<DiskPanel arg="" focused onExit={() => {}} />);
    await settled(container, "/Users/martin");
    // The target is seeded from the argument, so the argument effect must not
    // fire a second walk — a full home scan is expensive and visible.
    expect(diskScan).toHaveBeenCalledTimes(1);
    expect(diskScan).toHaveBeenCalledWith(null);
  });

  it("passes a typed path through, and blank means 'let the backend decide'", async () => {
    const { container } = render(<DiskPanel arg="  /tmp  " focused onExit={() => {}} />);
    await settled(container, "/tmp");
    expect(diskScan).toHaveBeenCalledTimes(1);
    expect(diskScan).toHaveBeenCalledWith("/tmp");
  });

  it("re-targets when the typed argument changes", async () => {
    const { container, rerender } = render(<DiskPanel arg="/tmp" focused onExit={() => {}} />);
    await settled(container, "/tmp");
    rerender(<DiskPanel arg="/var" focused onExit={() => {}} />);
    await waitFor(() => expect(diskScan).toHaveBeenLastCalledWith("/var"));
    expect(diskScan).toHaveBeenCalledTimes(2);
  });
});

describe("the path bar", () => {
  it("always spells out the absolute path of what is shown", async () => {
    const { container } = render(<DiskPanel arg="/Users/martin/claude" focused onExit={() => {}} />);
    await settled(container, "/Users/martin/claude");
    for (const seg of ["Users", "martin", "claude"]) {
      expect(container.textContent).toContain(seg);
    }
  });

  it("a crumb above the scan root re-scans there", async () => {
    const { container, getByTitle } = render(
      <DiskPanel arg="/Users/martin/claude" focused onExit={() => {}} />,
    );
    await settled(container, "/Users/martin/claude");
    fireEvent.click(getByTitle("/Users"));
    await waitFor(() => expect(diskScan).toHaveBeenLastCalledWith("/Users"));
  });
});

describe("navigating", () => {
  it("drilling into a folder that HAS children costs no scan", async () => {
    const { container } = render(<DiskPanel arg="/Users/martin" focused onExit={() => {}} />);
    await settled(container, "/Users/martin");
    fireEvent.click(arcNamed(container, "deep"));
    // The sizes are already computed — re-walking would be pure waste.
    await waitFor(() => expect(container.textContent).toContain("deep"));
    expect(diskScan).toHaveBeenCalledTimes(1);
  });

  it("clicking at the pruning boundary scans that folder afresh", async () => {
    const { container } = render(<DiskPanel arg="/Users/martin" focused onExit={() => {}} />);
    await settled(container, "/Users/martin");
    // "leafdir" carries no children in the tree, so an in-tree drill would
    // show nothing; this is what makes the depth effectively unlimited.
    fireEvent.click(arcNamed(container, "leafdir"));
    await waitFor(() => expect(diskScan).toHaveBeenLastCalledWith("/Users/martin/leafdir"));
  });

  it("Backspace at the scan root climbs into the parent folder", async () => {
    const { container } = render(<DiskPanel arg="/Users/martin/claude" focused onExit={() => {}} />);
    await settled(container, "/Users/martin/claude");
    fireEvent.keyDown(window, { key: "Backspace" });
    await waitFor(() => expect(diskScan).toHaveBeenLastCalledWith("/Users/martin"));
  });

  it("Backspace inside the tree is instant — no scan", async () => {
    const { container } = render(<DiskPanel arg="/Users/martin" focused onExit={() => {}} />);
    await settled(container, "/Users/martin");
    fireEvent.click(arcNamed(container, "deep"));
    await waitFor(() => expect(container.textContent).toContain("deep"));
    fireEvent.keyDown(window, { key: "Backspace" });
    expect(diskScan).toHaveBeenCalledTimes(1);
  });

  it("R re-scans the CURRENT folder, not the typed argument", async () => {
    const { container } = render(<DiskPanel arg="/Users/martin/claude" focused onExit={() => {}} />);
    await settled(container, "/Users/martin/claude");
    fireEvent.keyDown(window, { key: "Backspace" }); // → /Users/martin
    await waitFor(() => expect(diskScan).toHaveBeenLastCalledWith("/Users/martin"));
    fireEvent.keyDown(window, { key: "r" });
    // Re-scanning the argument would silently teleport the user back.
    await waitFor(() => expect(diskScan).toHaveBeenCalledTimes(3));
    expect(diskScan).toHaveBeenLastCalledWith("/Users/martin");
  });

  it("shortcuts never eat keystrokes meant for the search field", async () => {
    const { container } = render(<DiskPanel arg="/Users/martin" focused onExit={() => {}} />);
    await settled(container, "/Users/martin");
    const input = document.createElement("input");
    document.body.appendChild(input);
    fireEvent.keyDown(input, { key: "r" });
    fireEvent.keyDown(input, { key: "Backspace" });
    expect(diskScan).toHaveBeenCalledTimes(1);
    input.remove();
  });

  it("Esc backs out of the drill first and only then exits", async () => {
    const onExit = vi.fn();
    const { container } = render(<DiskPanel arg="/Users/martin" focused onExit={onExit} />);
    await settled(container, "/Users/martin");
    fireEvent.click(arcNamed(container, "deep"));
    await waitFor(() => expect(container.textContent).toContain("deep"));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onExit).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onExit).toHaveBeenCalledTimes(1);
  });

  it("does not react to keys while unfocused", async () => {
    const onExit = vi.fn();
    const { container } = render(<DiskPanel arg="/Users/martin" focused={false} onExit={onExit} />);
    await settled(container, "/Users/martin");
    fireEvent.keyDown(window, { key: "Backspace" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(diskScan).toHaveBeenCalledTimes(1);
    expect(onExit).not.toHaveBeenCalled();
  });
});
