import { describe, it, expect, afterEach } from "vitest";
import { applyTheme, normaliseTheme, themeLabel } from "./theme";

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
});
