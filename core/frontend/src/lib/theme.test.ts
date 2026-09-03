import { describe, it, expect, afterEach, vi } from "vitest";
import { applyTheme, applyThemeAnimated, normaliseTheme, themeLabel, THEME_SWITCH_MS } from "./theme";

afterEach(() => {
  // Reset the attribute so tests don't leak into each other.
  document.documentElement.removeAttribute("data-theme");
});

describe("normaliseTheme", () => {
  it("passes through the three valid values", () => {
    expect(normaliseTheme("light")).toBe("light");
    expect(normaliseTheme("dark")).toBe("dark");
    expect(normaliseTheme("system")).toBe("system");
  });

  it("collapses unknown strings to system", () => {
    expect(normaliseTheme("midnight")).toBe("system");
    expect(normaliseTheme("")).toBe("system");
    expect(normaliseTheme("DARK")).toBe("system"); // case-sensitive on purpose
    expect(normaliseTheme("Light")).toBe("system");
  });

  it("collapses null / undefined to system", () => {
    expect(normaliseTheme(null)).toBe("system");
    expect(normaliseTheme(undefined)).toBe("system");
  });
});

describe("applyTheme", () => {
  it("writes the data-theme attribute on <html>", () => {
    applyTheme("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    applyTheme("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    applyTheme("system");
    expect(document.documentElement.getAttribute("data-theme")).toBe("system");
  });

  it("is idempotent — applying the same theme twice is stable", () => {
    applyTheme("dark");
    applyTheme("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("overwrites a previously-applied theme", () => {
    applyTheme("light");
    applyTheme("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });
});

describe("themeLabel", () => {
  it("returns the human-readable label for each theme", () => {
    expect(themeLabel("light")).toBe("Light");
    expect(themeLabel("dark")).toBe("Dark");
    expect(themeLabel("system")).toBe("System");
  });

  it("labels are display-only — feeding one back as a preference falls to system", () => {
    // Guards against persisting the UI label instead of the raw preference.
    expect(normaliseTheme(themeLabel("dark"))).toBe("system");
    expect(normaliseTheme(themeLabel("light"))).toBe("system");
  });
});

describe("normaliseTheme — strictness", () => {
  it("does not trim whitespace (strict whitelist, mirrors the Rust side)", () => {
    expect(normaliseTheme(" dark")).toBe("system");
    expect(normaliseTheme("dark ")).toBe("system");
    expect(normaliseTheme("\tlight")).toBe("system");
  });
});

describe("normaliseTheme + applyTheme — end to end", () => {
  it("any persisted value yields a valid data-theme attribute", () => {
    for (const raw of ["dark", "light", "system", "midnight", "", null, undefined]) {
      applyTheme(normaliseTheme(raw));
      const attr = document.documentElement.getAttribute("data-theme");
      expect(["dark", "light", "system"]).toContain(attr);
    }
  });

  it("overrides an attribute written by anything else", () => {
    document.documentElement.setAttribute("data-theme", "corrupted");
    applyTheme("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });
});

describe("applyThemeAnimated (Etappe 6)", () => {
  it("arms the one-shot theme-switching blanket and removes it after", () => {
    vi.useFakeTimers();
    try {
      applyThemeAnimated("light");
      const root = document.documentElement;
      // The class must be PRESENT while the data-theme flip paints (that's
      // what makes the colours glide instead of jump)…
      expect(root.classList.contains("theme-switching")).toBe(true);
      expect(root.getAttribute("data-theme")).toBe("light");
      // …and must NOT persist: a blanket transition on * that outlives the
      // switch would tax every later interaction.
      vi.advanceTimersByTime(THEME_SWITCH_MS + 1);
      expect(root.classList.contains("theme-switching")).toBe(false);
    } finally {
      vi.useRealTimers();
      document.documentElement.classList.remove("theme-switching");
    }
  });
});
