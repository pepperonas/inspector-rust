import { describe, it, expect } from "vitest";
import { shortcut, IS_MAC } from "./platform";

// IS_MAC is computed once at module load from navigator.platform.
// happy-dom defaults to a non-Mac UA → IS_MAC === false in the test env.
// All assertions below assume the non-Mac rendering; Mac-specific glyphs
// (⌘ / ⇧ / ⌥) are only spot-checked by inverting the join-separator test
// and trusting the same lookup tables produce the matching glyphs on macOS.

describe("platform.IS_MAC", () => {
  it("is a boolean", () => {
    expect(typeof IS_MAC).toBe("boolean");
  });

  it("is false under happy-dom (default test env)", () => {
    expect(IS_MAC).toBe(false);
  });
});

describe("shortcut() — modifier aliases (non-Mac env)", () => {
  it("maps cmdorctrl + mod to Ctrl", () => {
    expect(shortcut("cmdorctrl", "V")).toBe("Ctrl+V");
    expect(shortcut("mod", "V")).toBe("Ctrl+V");
  });

  it("maps shift to Shift", () => {
    expect(shortcut("shift", "A")).toBe("Shift+A");
  });

  it("maps alt and option to Alt", () => {
    expect(shortcut("alt", "F4")).toBe("Alt+F4");
    expect(shortcut("option", "F4")).toBe("Alt+F4");
  });

  it("maps ctrl and control to Ctrl", () => {
    expect(shortcut("ctrl", "C")).toBe("Ctrl+C");
    expect(shortcut("control", "C")).toBe("Ctrl+C");
  });

  it("maps cmd and meta to Win on non-Mac", () => {
    expect(shortcut("cmd", "X")).toBe("Win+X");
    expect(shortcut("meta", "X")).toBe("Win+X");
  });

  it("is case-insensitive for the lookup tokens", () => {
    expect(shortcut("CtrL", "V")).toBe("Ctrl+V");
    expect(shortcut("SHIFT", "ENTER")).toBe("Shift+⏎");
    expect(shortcut("Mod", "z")).toBe("Ctrl+z");
  });
});

describe("shortcut() — special key tokens", () => {
  it("maps enter to ⏎", () => {
    expect(shortcut("enter")).toBe("⏎");
  });

  it("maps esc and escape to Esc", () => {
    expect(shortcut("esc")).toBe("Esc");
    expect(shortcut("escape")).toBe("Esc");
  });

  it("maps up and down to arrow glyphs", () => {
    expect(shortcut("up")).toBe("↑");
    expect(shortcut("down")).toBe("↓");
  });

  it("maps backquote and ` to literal `", () => {
    expect(shortcut("backquote")).toBe("`");
    expect(shortcut("`")).toBe("`");
  });
});

describe("shortcut() — combinations", () => {
  it("joins multi-modifier chords with + on non-Mac", () => {
    expect(shortcut("ctrl", "shift", "V")).toBe("Ctrl+Shift+V");
    expect(shortcut("ctrl", "shift", "alt", "Delete")).toBe(
      "Ctrl+Shift+Alt+Delete",
    );
  });

  it("preserves unrecognised tokens verbatim", () => {
    expect(shortcut("ctrl", "F12")).toBe("Ctrl+F12");
    expect(shortcut("Tab")).toBe("Tab");
    expect(shortcut("Space")).toBe("Space");
  });

  it("handles empty input", () => {
    expect(shortcut()).toBe("");
  });

  it("preserves digit and letter keys", () => {
    expect(shortcut("alt", "1")).toBe("Alt+1");
    expect(shortcut("alt", "KeyO")).toBe("Alt+KeyO");
  });
});

describe("shortcut() — typography stays consistent", () => {
  it("does not append spaces around the join separator on non-Mac", () => {
    expect(shortcut("ctrl", "V")).not.toContain(" +");
    expect(shortcut("ctrl", "V")).not.toContain("+ ");
  });

  it("returns a string for every call", () => {
    const out = shortcut("ctrl", "shift", "X");
    expect(typeof out).toBe("string");
    expect(out.length).toBeGreaterThan(0);
  });
});
