import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, act } from "@testing-library/react";
import type { ClipEntry, ListEntry } from "../lib/types";

const { listen, live, emit } = vi.hoisted(() => {
  const live = new Set<{ event: string; handler: () => void }>();
  const listen = vi.fn(async (event: string, handler: () => void) => {
    const sub = { event, handler };
    live.add(sub);
    return () => void live.delete(sub);
  });
  const emit = (event: string) => {
    for (const s of [...live]) if (s.event === event) s.handler();
  };
  return { listen, live, emit };
});
vi.mock("@tauri-apps/api/event", () => ({ listen }));

// The color-picker modal is always mounted (closed); keep its IPC off the wire.
vi.mock("../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/ipc")>()),
  pickScreenColor: vi.fn(async () => null),
  getAppIcon: vi.fn(async () => ""),
}));

import { HistoryList } from "./HistoryList";

function clip(id: number, over: Partial<ClipEntry> = {}): ListEntry {
  return {
    kind: "clip",
    data: {
      id,
      content_type: "text",
      content_text: `clip ${id}`,
      content_data: `clip ${id}`,
      hash: `h${id}`,
      byte_size: 6,
      created_at: 0,
      last_used_at: 0,
      pinned: false,
      note: null,
      derived_from: null,
      derived_kind: null,
      ...over,
    },
  };
}

const commandRow: ListEntry = {
  kind: "command",
  data: { commandKind: "optim", rawInput: "optim", arg: "", label: "Optimise", hint: "PNG/JPEG" },
};

function setup(over: Partial<Parameters<typeof HistoryList>[0]> = {}) {
  const props = {
    entries: [clip(1), clip(2)] as ListEntry[],
    selectedIndex: 0,
    onSelect: vi.fn(),
    onActivate: vi.fn(),
    ...over,
  };
  render(<HistoryList {...props} />);
  return props;
}

afterEach(cleanup);
beforeEach(() => {
  listen.mockClear();
  live.clear();
});

describe("HistoryList — empty state", () => {
  it("says 'No matches' when a search filtered everything away", () => {
    setup({ entries: [] });
    expect(screen.getByText("No matches")).toBeTruthy();
  });

  it("says 'No pinned clips' instead while the pinned-only filter is on", () => {
    // The generic "No matches" would read as "your search failed" when in fact
    // the user simply has not pinned anything yet.
    setup({ entries: [], pinnedOnly: true, onTogglePinnedOnly: vi.fn() });
    expect(screen.getByText("No pinned clips")).toBeTruthy();
    expect(screen.queryByText("No matches")).toBeNull();
  });

  it("keeps the toolbar available with an empty list", () => {
    setup({ entries: [] });
    expect(screen.getByTitle(/Open the color picker/)).toBeTruthy();
  });
});

describe("HistoryList — clip counter", () => {
  it("counts only real clips, ignoring virtual rows", () => {
    // Commands / calc / snippet rows are virtual — "Clear all" never deletes
    // them, so they must not inflate the count next to that button.
    setup({ entries: [clip(1), commandRow, clip(2)] });
    expect(screen.getByText("2 clips")).toBeTruthy();
  });

  it("uses the singular for exactly one clip", () => {
    setup({ entries: [clip(1)] });
    expect(screen.getByText("1 clip")).toBeTruthy();
  });

  it("shows zero clips when the list holds only virtual rows", () => {
    setup({ entries: [commandRow] });
    expect(screen.getByText("0 clips")).toBeTruthy();
  });
});

describe("HistoryList — pinned-only toggle", () => {
  it("is absent unless the caller supports it", () => {
    setup();
    expect(screen.queryByRole("button", { name: /Pinned/ })).toBeNull();
  });

  it("advertises the pinned count while off, and reports the shown count while on", () => {
    setup({ onTogglePinnedOnly: vi.fn(), pinnedCount: 3 });
    const off = screen.getByRole("button", { name: /Pinned/ });
    expect(off.textContent).toContain("Pinned (3)");
    expect(off.getAttribute("aria-pressed")).toBe("false");
    cleanup();

    setup({
      entries: [clip(1, { pinned: true }), clip(2, { pinned: true })],
      onTogglePinnedOnly: vi.fn(),
      pinnedOnly: true,
      pinnedCount: 2,
    });
    const on = screen.getByRole("button", { name: /pinned/ });
    expect(on.textContent).toContain("2 pinned");
    expect(on.getAttribute("aria-pressed")).toBe("true");
  });

  it("drops the '(n)' badge when nothing is pinned", () => {
    setup({ onTogglePinnedOnly: vi.fn(), pinnedCount: 0 });
    expect(screen.getByRole("button", { name: /Pinned/ }).textContent).not.toContain("(");
  });

  it("hides the plain clip counter while the filter is on (the toggle carries it)", () => {
    setup({ onTogglePinnedOnly: vi.fn(), pinnedOnly: true });
    expect(screen.queryByText(/^\d+ clips?$/)).toBeNull();
  });

  it("flips the filter when clicked", () => {
    const onTogglePinnedOnly = vi.fn();
    setup({ onTogglePinnedOnly });
    fireEvent.click(screen.getByRole("button", { name: /Pinned/ }));
    expect(onTogglePinnedOnly).toHaveBeenCalledTimes(1);
  });
});

describe("HistoryList — clear all is two-stage", () => {
  it("asks before deleting, naming how many clips are at stake", () => {
    const onClearAll = vi.fn();
    setup({ onClearAll });

    fireEvent.click(screen.getByTitle("Delete all clipboard history"));

    expect(screen.getByText("Delete 2 clips?")).toBeTruthy();
    expect(onClearAll).not.toHaveBeenCalled(); // the first click NEVER deletes
  });

  it("deletes only on the confirm, then returns to the idle button", () => {
    const onClearAll = vi.fn();
    setup({ onClearAll });

    fireEvent.click(screen.getByTitle("Delete all clipboard history"));
    fireEvent.click(screen.getByText("Yes"));

    expect(onClearAll).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Yes")).toBeNull();
    expect(screen.getByTitle("Delete all clipboard history")).toBeTruthy();
  });

  it("backs out on Cancel without deleting", () => {
    const onClearAll = vi.fn();
    setup({ onClearAll });

    fireEvent.click(screen.getByTitle("Delete all clipboard history"));
    fireEvent.click(screen.getByText("Cancel"));

    expect(onClearAll).not.toHaveBeenCalled();
    expect(screen.getByTitle("Delete all clipboard history")).toBeTruthy();
  });

  it("offers no Clear-all when there is nothing to clear", () => {
    setup({ entries: [commandRow], onClearAll: vi.fn() });
    expect(screen.queryByTitle("Delete all clipboard history")).toBeNull();
  });
});

describe("HistoryList — color picker modal", () => {
  // The toolbar button and the modal heading share the label "Color picker",
  // so the heading role is what distinguishes "modal is open".
  const modal = () => screen.queryByRole("heading", { name: "Color picker" });
  const openPicker = () => fireEvent.click(screen.getByTitle(/Open the color picker/));

  it("opens from the toolbar", () => {
    setup();
    expect(modal()).toBeNull();
    openPicker();
    expect(modal()).toBeTruthy();
  });

  it("closes itself when the popup is dismissed, so it never resurrects on re-open", () => {
    setup();
    openPicker();
    expect(modal()).toBeTruthy();

    act(() => emit("popup-hidden"));

    expect(modal()).toBeNull();
  });

  it("detaches its popup-hidden listener on unmount", async () => {
    const { unmount } = render(
      <HistoryList entries={[]} selectedIndex={0} onSelect={vi.fn()} onActivate={vi.fn()} />,
    );
    await act(async () => {}); // let the listen() promise settle
    expect(live.size).toBe(1);

    unmount();
    await act(async () => {});
    expect(live.size).toBe(0);
  });
});
