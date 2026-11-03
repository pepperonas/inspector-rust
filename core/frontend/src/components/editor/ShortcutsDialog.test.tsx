import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { DEFAULT_BINDINGS, type Bindings } from "../../lib/editor-shortcuts";

vi.mock("../../lib/ipc", () => ({
  EV_RECORD_SHORTCUTS: "record-shortcuts-changed",
  getRecordShortcuts: async () => ({}),
  setRecordShortcuts: async (s: unknown) => s,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));

import { ShortcutsDialog } from "./ShortcutsDialog";

afterEach(cleanup);

const press = (key: string, o: KeyboardEventInit = {}) => {
  const ev = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...o });
  act(() => {
    window.dispatchEvent(ev);
  });
  return ev;
};

function setup(bindings: Bindings = DEFAULT_BINDINGS) {
  const onSave = vi.fn();
  const onClose = vi.fn();
  const r = render(<ShortcutsDialog bindings={bindings} onSave={onSave} onClose={onClose} />);
  return { ...r, onSave, onClose };
}

describe("ShortcutsDialog", () => {
  it("lists every action with its key", () => {
    const { getByTestId } = setup();
    expect(getByTestId("ed-key-split").textContent).toBe("S");
    expect(getByTestId("ed-key-duplicate").textContent).toBe("⌘D");
    expect(getByTestId("ed-key-deleteSelected").textContent).toBe("⌫ / ⌦");
    expect(getByTestId("ed-key-playPause").textContent).toBe("Space");
  });

  it("a click records the next key and saves it", () => {
    const { getByTestId, onSave } = setup();
    fireEvent.click(getByTestId("ed-key-split"));
    expect(press("Shift", { shiftKey: true }).defaultPrevented).toBe(true); // a lone modifier waits
    expect(onSave).not.toHaveBeenCalled();
    const ev = press("x");
    expect(ev.defaultPrevented).toBe(true);
    expect(onSave).toHaveBeenCalledWith({ ...DEFAULT_BINDINGS, split: { key: "x" } });
  });

  it("conflict_offers_swap: another action's key asks to swap", () => {
    const { getByTestId, onSave, queryByTestId } = setup();
    fireEvent.click(getByTestId("ed-key-split"));
    press("z");
    expect(onSave).not.toHaveBeenCalled();
    expect(getByTestId("ed-key-conflict").textContent).toContain("Add zoom");
    fireEvent.click(getByTestId("ed-key-swap"));
    expect(onSave).toHaveBeenCalledWith({ ...DEFAULT_BINDINGS, split: { key: "z" }, addZoom: { key: "s" } });
    expect(queryByTestId("ed-key-conflict")).toBeNull();
  });

  it("cancel keeps both keys", () => {
    const { getByTestId, onSave } = setup();
    fireEvent.click(getByTestId("ed-key-split"));
    press("z");
    fireEvent.click(getByTestId("ed-key-cancel"));
    expect(onSave).not.toHaveBeenCalled();
  });

  it("fixed_key_refused: a fixed key is blocked with a note and no swap", () => {
    const { getByTestId, onSave, queryByTestId } = setup();
    fireEvent.click(getByTestId("ed-key-split"));
    press("z", { metaKey: true });
    expect(onSave).not.toHaveBeenCalled();
    expect(getByTestId("ed-key-conflict").textContent).toContain("⌘Z");
    expect(queryByTestId("ed-key-swap")).toBeNull();
  });

  it("Escape cancels recording first, then closes, and never leaks to other handlers", () => {
    const { getByTestId, onClose, onSave } = setup();
    const outer = vi.fn();
    window.addEventListener("keydown", outer);
    try {
      fireEvent.click(getByTestId("ed-key-split"));
      press("Escape");
      expect(onClose).not.toHaveBeenCalled();
      expect(onSave).not.toHaveBeenCalled();
      expect(getByTestId("ed-key-split").textContent).toBe("S");
      press("Escape");
      expect(onClose).toHaveBeenCalledTimes(1);
      expect(outer).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("keydown", outer);
    }
  });

  it("reset all saves the defaults", () => {
    const { getByTestId, onSave } = setup({ ...DEFAULT_BINDINGS, split: { key: "x" } });
    expect(getByTestId("ed-key-split").textContent).toBe("X");
    fireEvent.click(getByTestId("ed-keys-reset"));
    expect(onSave).toHaveBeenCalledWith(DEFAULT_BINDINGS);
  });
});
