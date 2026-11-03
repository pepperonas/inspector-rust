import { describe, expect, it } from "vitest";
import {
  ACTIONS,
  DEFAULT_BINDINGS,
  bindingFromEvent,
  findConflict,
  formatBinding,
  matchAction,
  mergeWithDefaults,
  sameBinding,
  storedDeviations,
  swapBinding,
  type Binding,
} from "./editor-shortcuts";

const key = (k: string, o: KeyboardEventInit = {}, target?: EventTarget) => {
  const e = new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...o });
  if (target) Object.defineProperty(e, "target", { value: target });
  return e;
};

describe("editor shortcuts", () => {
  it("pins the default table", () => {
    expect(DEFAULT_BINDINGS).toEqual({
      playPause: { key: "Space" },
      split: { key: "s" },
      crop: { key: "c" },
      addZoom: { key: "z" },
      addAnnotation: { key: "a" },
      addAudio: { key: "m" },
      addVoiceover: { key: "v" },
      duplicate: { key: "d", ctrl: true },
      deleteSelected: { key: "Backspace" },
      copy: { key: "c", ctrl: true },
      cut: { key: "x", ctrl: true },
      paste: { key: "v", ctrl: true },
      openProject: { key: "o", ctrl: true },
      showShortcuts: { key: "?" },
    });
    expect(ACTIONS).toHaveLength(14);
  });

  it("matches the defaults, with ⌘ or Ctrl, Delete equal to Backspace", () => {
    const b = DEFAULT_BINDINGS;
    expect(matchAction(key(" "), b)).toBe("playPause");
    expect(matchAction(key("S", { shiftKey: true }), b)).toBe(null); // shift held: a different chord
    expect(matchAction(key("S"), b)).toBe("split"); // caps lock: still the same key
    expect(matchAction(key("s"), b)).toBe("split");
    expect(matchAction(key("d", { metaKey: true }), b)).toBe("duplicate");
    expect(matchAction(key("d", { ctrlKey: true }), b)).toBe("duplicate");
    expect(matchAction(key("Delete"), b)).toBe("deleteSelected");
    expect(matchAction(key("Backspace"), b)).toBe("deleteSelected");
    expect(matchAction(key("c", { metaKey: true }), b)).toBe("copy");
    expect(matchAction(key("c"), b)).toBe("crop");
    expect(matchAction(key("c", { altKey: true }), b)).toBe(null);
    // "?" carries its own shift on every layout.
    expect(matchAction(key("?", { shiftKey: true }), b)).toBe("showShortcuts");
  });

  it("typing_in_field_triggers_nothing", () => {
    const input = document.createElement("input");
    const area = document.createElement("textarea");
    const editable = document.createElement("div");
    editable.contentEditable = "true";
    Object.defineProperty(editable, "isContentEditable", { value: true });
    for (const el of [input, area, editable]) {
      expect(matchAction(key("s", {}, el), DEFAULT_BINDINGS)).toBe(null);
      expect(matchAction(key("c", { metaKey: true }, el), DEFAULT_BINDINGS)).toBe(null);
    }
    const slider = document.createElement("input");
    slider.type = "range";
    expect(matchAction(key("s", {}, slider), DEFAULT_BINDINGS)).toBe("split");
  });

  it("a rebound split: S does nothing any more, X splits", () => {
    const b = { ...DEFAULT_BINDINGS, split: { key: "x" } };
    expect(matchAction(key("s"), b)).toBe(null);
    expect(matchAction(key("x"), b)).toBe("split");
  });

  it("conflict_offers_swap", () => {
    const c = findConflict(DEFAULT_BINDINGS, "split", { key: "z" });
    expect(c).toEqual({ kind: "action", action: "addZoom" });
    const swapped = swapBinding(DEFAULT_BINDINGS, "split", { key: "z" });
    expect(swapped.split).toEqual({ key: "z" });
    expect(swapped.addZoom).toEqual({ key: "s" });
    // Its own key is no conflict.
    expect(findConflict(DEFAULT_BINDINGS, "split", { key: "s" })).toBe(null);
    expect(findConflict(DEFAULT_BINDINGS, "split", { key: "q" })).toBe(null);
  });

  it("fixed_key_refused", () => {
    const fixed: Binding[] = [
      { key: "z", ctrl: true },
      { key: "z", ctrl: true, shift: true },
      { key: "ArrowLeft" },
      { key: "ArrowRight", shift: true },
      { key: "Tab" },
      { key: "Tab", shift: true },
      { key: "Escape" },
    ];
    for (const f of fixed) expect(findConflict(DEFAULT_BINDINGS, "split", f)).toEqual({ kind: "fixed" });
  });

  it("merge_keeps_defaults_for_missing", () => {
    const m = mergeWithDefaults({ split: { key: "x" }, crop: "junk", bogus: { key: "q" }, addZoom: { key: "Escape" } });
    expect(m.split).toEqual({ key: "x" });
    expect(m.crop).toEqual(DEFAULT_BINDINGS.crop);
    expect(m.addZoom).toEqual(DEFAULT_BINDINGS.addZoom); // a fixed key never sticks
    expect(m.paste).toEqual(DEFAULT_BINDINGS.paste);
    expect(mergeWithDefaults(null)).toEqual(DEFAULT_BINDINGS);
    expect(mergeWithDefaults([1, 2])).toEqual(DEFAULT_BINDINGS);
  });

  it("stores only the deviations", () => {
    expect(storedDeviations(DEFAULT_BINDINGS)).toEqual({});
    expect(storedDeviations({ ...DEFAULT_BINDINGS, split: { key: "x" } })).toEqual({ split: { key: "x" } });
    expect(mergeWithDefaults(storedDeviations({ ...DEFAULT_BINDINGS, split: { key: "x" } })).split).toEqual({ key: "x" });
  });

  it("records a key from an event", () => {
    expect(bindingFromEvent(key("X", { shiftKey: true }))).toEqual({ key: "x", shift: true });
    expect(bindingFromEvent(key("k", { metaKey: true }))).toEqual({ key: "k", ctrl: true });
    expect(bindingFromEvent(key("?", { shiftKey: true }))).toEqual({ key: "?" });
    expect(bindingFromEvent(key("Delete"))).toEqual({ key: "Backspace" });
    expect(bindingFromEvent(key("Shift", { shiftKey: true }))).toBe(null);
    expect(bindingFromEvent(key("Meta", { metaKey: true }))).toBe(null);
    expect(sameBinding({ key: "x", shift: false }, { key: "x" })).toBe(true);
  });

  it("formats bindings with Mac symbols", () => {
    expect(formatBinding({ key: "d", ctrl: true })).toBe("⌘D");
    expect(formatBinding({ key: "x", ctrl: true, shift: true, alt: true })).toBe("⌥⇧⌘X");
    expect(formatBinding({ key: "Backspace" })).toBe("⌫");
    expect(formatBinding({ key: "Space" }, "Space")).toBe("Space");
    expect(formatBinding({ key: "?" })).toBe("?");
  });
});
