import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@testing-library/react";
import { memo } from "react";
import type { ListEntry } from "../lib/types";

// Count real renders of each row. The mock keeps `memo` (default shallow
// compare) — exactly what the production HistoryItem uses.
const renders = new Map<number, number>();
vi.mock("./HistoryItem", () => ({
  HistoryItem: memo(function Row(p: { entry: ListEntry; selected: boolean; onClick: () => void }) {
    const id = p.entry.kind === "clip" ? p.entry.data.id : -1;
    renders.set(id, (renders.get(id) ?? 0) + 1);
    return (
      <div data-testid={`row-${id}`} data-selected={p.selected} onClick={p.onClick}>
        {id}
      </div>
    );
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock("./ColorPickerModal", () => ({ ColorPickerModal: () => null }));
// happy-dom has no layout, so the real virtualiser renders zero rows. This
// stand-in lays every row out at a fixed 36 px — the item objects are fresh
// per render, as TanStack's are.
vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: (o: { count: number }) => ({
    getTotalSize: () => o.count * 36,
    getVirtualItems: () =>
      Array.from({ length: o.count }, (_, i) => ({ index: i, key: i, start: i * 36, size: 36 })),
    scrollToIndex: () => undefined,
  }),
}));

import { HistoryList } from "./HistoryList";

const clip = (id: number): ListEntry => ({
  kind: "clip",
  data: {
    id,
    content_type: "text",
    content_text: `clip ${id}`,
    content_data: "",
    hash: `h${id}`,
    byte_size: 6,
    created_at: 0,
    last_used_at: 0,
    pinned: false,
    note: null,
    derived_from: null,
    derived_kind: null,
  },
});
const entries = [clip(1), clip(2), clip(3), clip(4)];

afterEach(() => {
  cleanup();
  renders.clear();
});

describe("HistoryList row memoisation", () => {
  it("a selection move re-renders only the rows whose `selected` changed", () => {
    const onSelect = vi.fn();
    const { rerender } = render(
      <HistoryList entries={entries} selectedIndex={0} onSelect={onSelect} onActivate={vi.fn()} />,
    );
    const before = new Map(renders);
    // A parent re-render hands NEW callback identities — as App does.
    rerender(
      <HistoryList entries={entries} selectedIndex={1} onSelect={vi.fn()} onActivate={vi.fn()} />,
    );
    const delta = (id: number) => (renders.get(id) ?? 0) - (before.get(id) ?? 0);
    expect(delta(1)).toBe(1);
    expect(delta(2)).toBe(1);
    expect(delta(3)).toBe(0);
    expect(delta(4)).toBe(0);
  });

  it("a click still reaches the LATEST parent callback (no stale closure)", () => {
    const first = vi.fn();
    const second = vi.fn();
    const { rerender, getByTestId } = render(
      <HistoryList entries={entries} selectedIndex={0} onSelect={first} onActivate={vi.fn()} />,
    );
    rerender(
      <HistoryList entries={entries} selectedIndex={0} onSelect={second} onActivate={vi.fn()} />,
    );
    fireEvent.click(getByTestId("row-3"));
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledWith(2);
  });
});
