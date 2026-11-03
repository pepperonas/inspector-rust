import { describe, expect, it, vi } from "vitest";
import { KEY_LAYER, onEditorKey } from "./editor-keys";

describe("editor key layers", () => {
  it("calls the timeline layer before the editor layer, whatever the registration order", () => {
    const order: string[] = [];
    const offEditor = onEditorKey(KEY_LAYER.editor, () => order.push("editor"));
    const offTimeline = onEditorKey(KEY_LAYER.timeline, () => order.push("timeline"));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "x" }));
    expect(order).toEqual(["timeline", "editor"]);
    offEditor();
    offTimeline();
  });

  it("removes its window listener when the last handler leaves", () => {
    const remove = vi.spyOn(window, "removeEventListener");
    const h = vi.fn();
    const off = onEditorKey(KEY_LAYER.editor, h);
    off();
    expect(remove).toHaveBeenCalledWith("keydown", expect.any(Function));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "x" }));
    expect(h).not.toHaveBeenCalled();
    remove.mockRestore();
  });
});
