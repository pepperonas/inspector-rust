import { describe, it, expect } from "vitest";
import { detectSmartActions } from "./smart-actions";

const kinds = (s: string) => detectSmartActions(s).map((a) => a.kind);

describe("detectSmartActions", () => {
  it("detects an http(s) URL", () => {
    const a = detectSmartActions("https://example.com/path?x=1");
    expect(a[0]).toMatchObject({ kind: "open-url", href: "https://example.com/path?x=1" });
  });

  it("detects a bare domain and prefixes https", () => {
    expect(detectSmartActions("example.com")[0]).toMatchObject({
      kind: "open-url",
      href: "https://example.com",
    });
    expect(detectSmartActions("www.rust-lang.org")[0].href).toBe("https://www.rust-lang.org");
  });

  it("detects an email → mailto", () => {
    expect(detectSmartActions("me@example.com")[0]).toMatchObject({
      kind: "email",
      href: "mailto:me@example.com",
    });
  });

  it("does not treat an email as a bare domain", () => {
    expect(kinds("me@example.com")).toContain("email");
    expect(kinds("me@example.com")).not.toContain("open-url");
  });

  it("detects a phone number → tel (digits stripped of formatting)", () => {
    const a = detectSmartActions("+49 (170) 123-4567");
    expect(a[0]).toMatchObject({ kind: "call", href: "tel:+491701234567" });
  });

  it("detects lat,lng coordinates → maps", () => {
    const a = detectSmartActions("48.137154, 11.576124");
    expect(a[0]).toMatchObject({ kind: "maps" });
    expect(a[0].href).toContain("query=48.137154,11.576124");
  });

  it("rejects out-of-range coordinates", () => {
    expect(kinds("200.0, 999.0")).not.toContain("maps");
  });

  it("appends a QR action for any short single-line value", () => {
    expect(kinds("https://example.com")).toEqual(["open-url", "qr"]);
    expect(kinds("just some text")).toEqual(["qr"]);
  });

  it("offers no QR for multi-line or very long text", () => {
    expect(kinds("line1\nline2")).toEqual([]);
    expect(kinds("x".repeat(600))).toEqual([]);
  });

  it("returns nothing for empty / whitespace", () => {
    expect(detectSmartActions("")).toEqual([]);
    expect(detectSmartActions("   ")).toEqual([]);
  });

  it("does not misfire on a number that's too short to be a phone", () => {
    expect(kinds("12345")).toEqual(["qr"]); // 5 digits → QR only, no call
  });
});

describe("detectSmartActions — phone-shaped strings without digits", () => {
  it("does not offer Call for punctuation that merely matches the phone shape", () => {
    // Matches PHONE_RE's character class + length but contains zero digits.
    const actions = detectSmartActions("().-()().-");
    expect(actions.some((a) => a.kind === "call")).toBe(false);
  });
});
