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

  it("an unassigned action reads 'not assigned' and can be given a key", () => {
    const { getByTestId, onSave } = setup({ ...DEFAULT_BINDINGS, addZoom: { key: "s" }, split: null });
    expect(getByTestId("ed-key-split").textContent).toBe("not assigned");
    fireEvent.click(getByTestId("ed-key-split"));
    press("y");
    expect(onSave).toHaveBeenCalledWith({ ...DEFAULT_BINDINGS, addZoom: { key: "s" }, split: { key: "y" } });
  });

  it("focus starts on the first key and Tab stays inside the dialog", () => {
    const outside = document.createElement("button");
    document.body.appendChild(outside);
    try {
      outside.focus();
      const { getByTestId } = setup();
      expect(document.activeElement).toBe(getByTestId("ed-key-playPause"));
      const reset = getByTestId("ed-keys-reset");
      reset.focus();
      press("Tab");
      // After the last control, Tab wraps to the first (the close button).
      expect(document.activeElement?.getAttribute("aria-label")).toBe("Close");
      press("Tab", { shiftKey: true });
      expect(document.activeElement).toBe(reset);
      cleanup();
      expect(document.activeElement, "focus goes back where it was").toBe(outside);
    } finally {
      outside.remove();
    }
  });

  it("no key leaves the dialog: other window and document handlers never see it", () => {
    setup();
    const outer = vi.fn();
    window.addEventListener("keydown", outer);
    document.addEventListener("keydown", outer, true);
    try {
      for (const k of ["s", "Escape", "Backspace", "?"]) {
        const ev = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true });
        act(() => {
          document.body.dispatchEvent(ev);
        });
        expect(ev.defaultPrevented, k).toBe(true);
      }
      expect(outer).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("keydown", outer);
      document.removeEventListener("keydown", outer, true);
    }
  });

  it("reset all saves the defaults", () => {
    const { getByTestId, onSave } = setup({ ...DEFAULT_BINDINGS, split: { key: "x" } });
    expect(getByTestId("ed-key-split").textContent).toBe("X");
    fireEvent.click(getByTestId("ed-keys-reset"));
    expect(onSave).toHaveBeenCalledWith(DEFAULT_BINDINGS);
  });
});
