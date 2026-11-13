import { describe, expect, it } from "vitest";
import { fromSearch, hitKey, isTyping, keyOrigin, withMod } from "./panel-keys";

function el(tag: string, attrs: Record<string, string> = {}): HTMLElement {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  return e;
}
const search = () => el("input", { "data-main-search": "" });
const key = (k: string, target: EventTarget | null, mods: Partial<{ metaKey: boolean; ctrlKey: boolean; altKey: boolean }> = {}) => ({
  key: k,
  target,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  ...mods,
});

describe("panel key rule", () => {
  it("tells the search field from other fields and from the page", () => {
    expect(keyOrigin(search())).toBe("search");
    expect(keyOrigin(el("input"))).toBe("field");
    expect(keyOrigin(el("textarea"))).toBe("field");
    expect(keyOrigin(el("select"))).toBe("field");
    expect(keyOrigin(el("div"))).toBe("page");
    expect(keyOrigin(null)).toBe("page");
    expect(keyOrigin(window)).toBe("page");
  });

  it("from the search field, letters, digits, space and ⌫ are typing", () => {
    for (const k of ["r", "R", "t", "1", " ", "ä", "Backspace", "Delete"]) {
      expect(isTyping(key(k, search())), k).toBe(true);
    }
  });

  it("from the search field, arrows, Enter, Page keys and Home reach the panel", () => {
    for (const k of ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Enter", "PageDown", "Home", "Escape", "Tab"]) {
      expect(isTyping(key(k, search())), k).toBe(false);
    }
  });

  it("a ⌘ or Ctrl chord from the search field reaches the panel", () => {
    expect(isTyping(key("r", search(), { metaKey: true }))).toBe(false);
    expect(isTyping(key("Backspace", search(), { metaKey: true }))).toBe(false);
    expect(isTyping(key("1", search(), { ctrlKey: true }))).toBe(false);
    // ⌥ + letter prints a character on macOS: still typing.
    expect(isTyping(key("®", search(), { altKey: true }))).toBe(true);
  });

  it("another text field keeps every key, chords and arrows included", () => {
    for (const t of [el("input"), el("textarea"), el("select")]) {
      expect(isTyping(key("ArrowDown", t))).toBe(true);
      expect(isTyping(key("Enter", t))).toBe(true);
      expect(isTyping(key("r", t, { metaKey: true }))).toBe(true);
    }
  });

  it("outside any field nothing is typing", () => {
    expect(isTyping(key("r", el("div")))).toBe(false);
    expect(isTyping(key(" ", null))).toBe(false);
  });

  it("hitKey ignores case and refuses ⌥", () => {
    expect(hitKey(key("R", null), "r")).toBe(true);
    expect(hitKey(key("r", null, { metaKey: true }), "r")).toBe(true);
    expect(hitKey(key("r", null, { altKey: true }), "r")).toBe(false);
    expect(hitKey(key("t", null), "r")).toBe(false);
  });

  it("withMod wants ⌘ or Ctrl without ⌥; fromSearch names the origin", () => {
    expect(withMod(key("r", null, { metaKey: true }))).toBe(true);
    expect(withMod(key("r", null, { ctrlKey: true }))).toBe(true);
    expect(withMod(key("r", null, { metaKey: true, altKey: true }))).toBe(false);
    expect(withMod(key("r", null))).toBe(false);
    expect(fromSearch(key("a", search()))).toBe(true);
    expect(fromSearch(key("a", el("input")))).toBe(false);
  });
});
