import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";
import { useState } from "react";
import { CellCountField } from "./CellCountField";
import { CELL_DRAFT_DEBOUNCE_MS } from "../lib/palette-cells";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

/** Behaves like the settings panel: a commit becomes the new value. */
function Harness({ start, onCommit }: { start: number; onCommit: (n: number) => void }) {
  const [v, setV] = useState(start);
  return (
    <CellCountField
      value={v}
      label="cols"
      onCommit={(n) => {
        onCommit(n);
        setV(n);
      }}
    />
  );
}

const field = () => screen.getByLabelText("cols") as HTMLInputElement;

/** Type character by character, the way the field really receives input. */
function type(text: string) {
  let acc = "";
  for (const ch of text) {
    acc += ch;
    fireEvent.change(field(), { target: { value: acc } });
  }
}

describe("CellCountField", () => {
  it("typing a two-digit value saves it once, not its first digit", () => {
    vi.useFakeTimers();
    const onCommit = vi.fn();
    render(<Harness start={16} onCommit={onCommit} />);
    fireEvent.focus(field());
    fireEvent.change(field(), { target: { value: "" } });
    type("12");
    expect(field().value).toBe("12");
    act(() => vi.advanceTimersByTime(CELL_DRAFT_DEBOUNCE_MS));
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith(12);
    expect(field().value).toBe("12");
  });

  it("a pause on an incomplete digit saves nothing and leaves the digit alone", () => {
    vi.useFakeTimers();
    const onCommit = vi.fn();
    render(<Harness start={16} onCommit={onCommit} />);
    fireEvent.focus(field());
    fireEvent.change(field(), { target: { value: "1" } });
    act(() => vi.advanceTimersByTime(CELL_DRAFT_DEBOUNCE_MS * 3));
    // The old field saved "1", got "2" back and showed it under the cursor.
    expect(onCommit).not.toHaveBeenCalled();
    expect(field().value).toBe("1");
  });

  it("an empty field is not saved while typing", () => {
    vi.useFakeTimers();
    const onCommit = vi.fn();
    render(<Harness start={16} onCommit={onCommit} />);
    fireEvent.focus(field());
    fireEvent.change(field(), { target: { value: "" } });
    act(() => vi.advanceTimersByTime(CELL_DRAFT_DEBOUNCE_MS * 3));
    expect(onCommit).not.toHaveBeenCalled();
    expect(field().value).toBe("");
  });

  it("Enter saves at once and clamps an out-of-range number", () => {
    const onCommit = vi.fn();
    render(<Harness start={10} onCommit={onCommit} />);
    fireEvent.focus(field());
    fireEvent.change(field(), { target: { value: "40" } });
    fireEvent.keyDown(field(), { key: "Enter" });
    expect(onCommit).toHaveBeenCalledWith(24);
    expect(field().value).toBe("24");
  });

  it("leaving an emptied field restores the saved value", () => {
    const onCommit = vi.fn();
    render(<Harness start={10} onCommit={onCommit} />);
    fireEvent.focus(field());
    fireEvent.change(field(), { target: { value: "" } });
    fireEvent.blur(field());
    expect(onCommit).not.toHaveBeenCalled();
    expect(field().value).toBe("10");
  });

  it("Esc throws the draft away", () => {
    vi.useFakeTimers();
    const onCommit = vi.fn();
    render(<Harness start={10} onCommit={onCommit} />);
    fireEvent.focus(field());
    fireEvent.change(field(), { target: { value: "20" } });
    fireEvent.keyDown(field(), { key: "Escape" });
    act(() => vi.advanceTimersByTime(CELL_DRAFT_DEBOUNCE_MS * 2));
    expect(onCommit).not.toHaveBeenCalled();
    expect(field().value).toBe("10");
  });

  it("is never disabled, so a save can't steal focus mid-typing", () => {
    render(<Harness start={10} onCommit={() => {}} />);
    expect(field().disabled).toBe(false);
  });
});
