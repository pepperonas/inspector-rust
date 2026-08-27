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

/** Find a rendered arc by hovering until the hub + detail row name it.
 *
 * ⚠️ Counts occurrences rather than testing `includes`: since v0.140.0 the
 * child LIST also spells out every name, so a plain substring check matched
 * the very first arc hovered. Hovering the right arc adds the name again (hub
 * + detail row), so the count is what identifies it. */
function arcNamed(container: HTMLElement, name: string): Element {
  const count = () => (container.textContent?.split(name).length ?? 1) - 1;
  const base = count();
  for (const p of Array.from(container.querySelectorAll("path"))) {
    fireEvent.mouseEnter(p);
    if (count() > base) return p;
  }
  throw new Error(`no arc named ${name}`);
}

/** Click the child-list row for `name` — the path a user takes into a folder
 *  whose arc is too thin to hit. */
function rowNamed(container: HTMLElement, name: string): Element {
  const btn = Array.from(container.querySelectorAll("button")).find(
    (b) => b.textContent?.trim().startsWith(name),
  );
  if (!btn) throw new Error(`no list row named ${name}`);
  return btn;
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

describe("the child list — reaching what the chart cannot show", () => {
  /** The real software-project shape: source dwarfed by build output. */
  function lopsided(): DiskScan {
    return {
      ...scanOf("/Users/martin/projekt"),
      tree: dir("projekt", 20_002_000_000, [
        dir("target", 20_000_000_000, [dir("debug", 20_000_000_000)]),
        dir("src", 2_000_000, [dir("lib", 2_000_000)]),
      ]),
    };
  }

  it("lists a folder whose arc is too thin to be drawn at all", async () => {
    diskScan.mockImplementation(async () => lopsided());
    const { container } = render(<DiskPanel arg="/Users/martin/projekt" focused onExit={() => {}} />);
    await settled(container, "projekt");

    // `src` is 0.01 % of the circle — below minAngle, so it has NO arc.
    expect(() => arcNamed(container, "src")).toThrow();
    // …but it is a row, and clicking it opens the folder.
    fireEvent.click(rowNamed(container, "src"));
    await waitFor(() => expect(container.textContent).toContain("lib"));
    expect(diskScan).toHaveBeenCalledTimes(1); // in-tree, no re-walk
  });

  it("opens a folder with the keyboard: arrows select, Enter enters", async () => {
    diskScan.mockImplementation(async () => lopsided());
    const { container } = render(<DiskPanel arg="/Users/martin/projekt" focused onExit={() => {}} />);
    await settled(container, "projekt");

    // Row 0 is `target` (largest first); one step down selects `src`.
    fireEvent.keyDown(window, { key: "ArrowDown" });
    fireEvent.keyDown(window, { key: "Enter" });
    await waitFor(() => expect(container.textContent).toContain("lib"));
  });

  it("the arrows wrap rather than sticking at the ends", async () => {
    diskScan.mockImplementation(async () => lopsided());
    const { container } = render(<DiskPanel arg="/Users/martin/projekt" focused onExit={() => {}} />);
    await settled(container, "projekt");
    // Up from the first row lands on the last one — `src`.
    fireEvent.keyDown(window, { key: "ArrowUp" });
    fireEvent.keyDown(window, { key: "Enter" });
    await waitFor(() => expect(container.textContent).toContain("lib"));
  });
});

describe("the child list must not scroll the panel on its own", () => {
  it("reveals itself only once the keyboard drives it", async () => {
    // Live-observed: revealing the selected row on MOUNT scrolled the whole
    // preview column and pushed the title + path bar off-screen. But the list
    // sits below the chart, so keyboard navigation DOES have to bring it in.
    const spy = vi.fn();
    const orig = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = spy;
    try {
      const { container } = render(<DiskPanel arg="/Users/martin" focused onExit={() => {}} />);
      await settled(container, "/Users/martin");
      expect(spy, "kein Scrollen beim Öffnen").not.toHaveBeenCalled();

      fireEvent.keyDown(window, { key: "ArrowDown" });
      await waitFor(() => expect(spy).toHaveBeenCalled());
    } finally {
      Element.prototype.scrollIntoView = orig;
    }
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
