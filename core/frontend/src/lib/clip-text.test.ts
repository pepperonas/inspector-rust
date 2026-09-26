import { describe, it, expect } from "vitest";
import { needsFullText } from "./clip-text";

describe("needsFullText", () => {
  it("uses content_text when it IS the whole payload", () => {
    expect(needsFullText("", "hello", 5)).toBe(false);
  });
  it("counts UTF-8 bytes, not UTF-16 units (Umlaute are 2 bytes)", () => {
    expect(needsFullText("", "grün", 5)).toBe(false);
  });
  it("fetches when content_text is only a truncated preview", () => {
    expect(needsFullText("", "a".repeat(200), 5000)).toBe(true);
  });
  it("never fetches when the row already carries content_data", () => {
    expect(needsFullText("full text", "full", 9)).toBe(false);
  });
});
