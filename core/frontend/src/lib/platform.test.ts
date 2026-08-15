import { afterEach, describe, it, expect, vi } from "vitest";
import { shortcut, formatHotkey, IS_MAC } from "./platform";

// IS_MAC is computed once at module load from navigator.platform.
// happy-dom defaults to a non-Mac UA → IS_MAC === false in the test env, so the
// blocks below assert the non-Mac rendering. The macOS rendering (⌘ / ⇧ / ⌥ / ⌃
// and the empty join separator) is covered for real at the bottom of the file by
// re-importing the module under a stubbed navigator.

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

describe("formatHotkey() — Tauri spec → display (non-Mac env)", () => {
  it("maps modifiers and digit codes", () => {
    expect(formatHotkey("Ctrl+Shift+V")).toBe("Ctrl+Shift+V");
    expect(formatHotkey("Alt+Digit1")).toBe("Alt+1");
  });

  it("maps KeyX, Backquote, arrows, and F-keys", () => {
    expect(formatHotkey("Meta+KeyB")).toBe("Win+B");
    expect(formatHotkey("Alt+Backquote")).toBe("Alt+`");
    expect(formatHotkey("Ctrl+ArrowUp")).toBe("Ctrl+↑");
    expect(formatHotkey("Ctrl+F5")).toBe("Ctrl+F5");
  });

  it("returns an em-dash for an empty / blank spec", () => {
    expect(formatHotkey("")).toBe("—");
    expect(formatHotkey("   ")).toBe("—");
  });

  it("passes unknown tokens through verbatim", () => {
    expect(formatHotkey("Ctrl+Space")).toBe("Ctrl+Space");
  });
});

describe("formatHotkey() — the rest of the key-code table (non-Mac env)", () => {
  it("maps numpad digits with a Num prefix", () => {
    expect(formatHotkey("Ctrl+Numpad5")).toBe("Ctrl+Num5");
    expect(formatHotkey("Numpad0")).toBe("Num0");
  });

  it("maps the punctuation codes a hotkey capture can produce", () => {
    expect(formatHotkey("Ctrl+Minus")).toBe("Ctrl+-");
    expect(formatHotkey("Ctrl+Equal")).toBe("Ctrl+=");
    expect(formatHotkey("Alt+Backquote")).toBe("Alt+`");
  });

  it("maps all four arrow codes", () => {
    expect(formatHotkey("ArrowUp")).toBe("↑");
    expect(formatHotkey("ArrowDown")).toBe("↓");
    expect(formatHotkey("ArrowLeft")).toBe("←");
    expect(formatHotkey("ArrowRight")).toBe("→");
  });

  it("accepts every modifier alias the backend can persist", () => {
    // Tauri specs use Ctrl/Shift/Alt/Super; hand-written ones use
    // Control/Option/Cmd/Command — all must render.
    expect(formatHotkey("Control+KeyA")).toBe("Ctrl+A");
    expect(formatHotkey("Option+KeyA")).toBe("Alt+A");
    expect(formatHotkey("Cmd+KeyA")).toBe("Win+A");
    expect(formatHotkey("Command+KeyA")).toBe("Win+A");
    expect(formatHotkey("Super+KeyA")).toBe("Win+A");
  });

  it("F1–F24 are recognised (and up-cased); F25 is not an F-key", () => {
    // Lowercase input makes the boundary observable: a recognised F-key is
    // up-cased, an unrecognised token is passed through verbatim.
    expect(formatHotkey("ctrl+f1")).toBe("Ctrl+F1");
    expect(formatHotkey("ctrl+f24")).toBe("Ctrl+F24");
    expect(formatHotkey("ctrl+f25")).toBe("Ctrl+f25");
    expect(formatHotkey("ctrl+f0")).toBe("Ctrl+f0");
  });

  it("only single-digit / single-letter codes are unwrapped", () => {
    expect(formatHotkey("Digit10")).toBe("Digit10");
    expect(formatHotkey("KeyBB")).toBe("KeyBB");
    expect(formatHotkey("Numpad10")).toBe("Numpad10");
  });

  it("tolerates whitespace around the segments of a stored spec", () => {
    expect(formatHotkey(" Ctrl + Shift + KeyV ")).toBe("Ctrl+Shift+V");
  });

  it("survives a degenerate spec without throwing", () => {
    // A capture that recorded a literal `+` produces empty segments.
    expect(() => formatHotkey("Ctrl++")).not.toThrow();
    expect(() => formatHotkey("+++")).not.toThrow();
  });
});

// ── Platform-dependent rendering ────────────────────────────────────────────
// IS_MAC / IS_WINDOWS / CURRENT_PLATFORM are module-load-time constants, so the
// only way to exercise the other platforms' rendering is to re-import the
// module under a stubbed navigator. Without this the whole macOS half of the
// lookup tables (⌘ ⇧ ⌥ ⌃ + the empty join separator) — i.e. what actually ships
// to the maintainer's machine — is never executed.
async function loadPlatform(nav: { platform?: string; userAgent?: string } | undefined) {
  vi.resetModules();
  vi.stubGlobal("navigator", nav);
  return await import("./platform");
}

describe("platform detection under a stubbed navigator", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it("detects macOS from navigator.platform", async () => {
    const p = await loadPlatform({ platform: "MacIntel", userAgent: "Mozilla/5.0 (Macintosh)" });
    expect(p.IS_MAC).toBe(true);
    expect(p.IS_WINDOWS).toBe(false);
    expect(p.CURRENT_PLATFORM).toBe("mac");
  });

  it("detects macOS from the user agent alone (empty platform)", async () => {
    const p = await loadPlatform({ platform: "", userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)" });
    expect(p.IS_MAC).toBe(true);
    expect(p.CURRENT_PLATFORM).toBe("mac");
  });

  it("detects Windows and prefers it over the linux fallback", async () => {
    const p = await loadPlatform({ platform: "Win32", userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)" });
    expect(p.IS_WINDOWS).toBe(true);
    expect(p.IS_MAC).toBe(false);
    expect(p.CURRENT_PLATFORM).toBe("win");
  });

  it("detects Linux", async () => {
    const p = await loadPlatform({ platform: "Linux x86_64", userAgent: "Mozilla/5.0 (X11; Linux x86_64)" });
    expect(p.IS_LINUX).toBe(true);
    expect(p.IS_MAC).toBe(false);
    expect(p.IS_WINDOWS).toBe(false);
    expect(p.CURRENT_PLATFORM).toBe("linux");
  });

  it("falls back to linux when there is no navigator at all", async () => {
    // Command gating must still resolve to *something* rather than throwing.
    const p = await loadPlatform(undefined);
    expect(p.IS_MAC).toBe(false);
    expect(p.IS_LINUX).toBe(false);
    expect(p.IS_WINDOWS).toBe(false);
    expect(p.CURRENT_PLATFORM).toBe("linux");
  });

  it("CURRENT_PLATFORM is always exactly one of the three discriminators", async () => {
    for (const nav of [
      { platform: "MacIntel", userAgent: "" },
      { platform: "Win32", userAgent: "" },
      { platform: "Linux", userAgent: "" },
      { platform: "", userAgent: "" },
    ]) {
      const p = await loadPlatform(nav);
      expect(["mac", "win", "linux"]).toContain(p.CURRENT_PLATFORM);
    }
  });
});

describe("macOS rendering (glyphs, no separator)", () => {
  const mac = () => loadPlatform({ platform: "MacIntel", userAgent: "Mozilla/5.0 (Macintosh)" });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it("formatHotkey renders modifier glyphs and joins WITHOUT a separator", async () => {
    const { formatHotkey: f } = await mac();
    expect(f("Ctrl+Shift+KeyV")).toBe("⌃⇧V");
    expect(f("Alt+Digit1")).toBe("⌥1");
    expect(f("Meta+KeyB")).toBe("⌘B");
    expect(f("Ctrl+Shift+Alt+KeyS")).toBe("⌃⇧⌥S");
  });

  it("formatHotkey still returns the em-dash for an unset spec on macOS", async () => {
    const { formatHotkey: f } = await mac();
    expect(f("")).toBe("—");
    expect(f("   ")).toBe("—");
  });

  it("shortcut() maps mod/cmdorctrl to ⌘ and joins without a separator", async () => {
    const { shortcut: s } = await mac();
    expect(s("mod", "V")).toBe("⌘V");
    expect(s("cmdorctrl", "V")).toBe("⌘V");
    expect(s("cmd", "shift", "P")).toBe("⌘⇧P");
    expect(s("alt", "option")).toBe("⌥⌥");
  });

  it("shortcut() keeps the literal word for `ctrl` even on macOS", async () => {
    // Deliberate: IR's action hotkeys are literal Control on EVERY OS
    // (⌃⇧O, not ⌘⇧O), and spelling it out is what stops a Mac user reading
    // the chord as Command. `mod` is the token that means "⌘ here, Ctrl there".
    const { shortcut: s } = await mac();
    expect(s("ctrl", "shift", "O")).toBe("Ctrl⇧O");
    expect(s("control", "C")).toBe("CtrlC");
  });

  it("special key tokens are platform-independent", async () => {
    const { shortcut: s } = await mac();
    expect(s("enter")).toBe("⏎");
    expect(s("escape")).toBe("Esc");
    expect(s("up")).toBe("↑");
    expect(s("down")).toBe("↓");
    expect(s("`")).toBe("`");
  });
});
