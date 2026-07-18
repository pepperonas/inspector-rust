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

describe("detectSmartActions — boundaries + Unicode", () => {
  it("trims whitespace before detection and in the QR payload", () => {
    const a = detectSmartActions("   https://example.com   ");
    expect(a[0]).toMatchObject({ kind: "open-url", href: "https://example.com" });
    expect(a[1]).toMatchObject({ kind: "qr", href: "https://example.com" });
  });

  it("QR length boundary is exactly 512 chars", () => {
    expect(kinds("x".repeat(512))).toEqual(["qr"]);
    expect(kinds("x".repeat(513))).toEqual([]);
  });

  it("bare domain with path/query keeps the path in the https URL", () => {
    expect(detectSmartActions("example.com/path?x=1")[0]).toMatchObject({
      kind: "open-url",
      href: "https://example.com/path?x=1",
    });
  });

  it("plain http URLs open unchanged (no https upgrade)", () => {
    expect(detectSmartActions("http://example.com")[0]).toMatchObject({
      kind: "open-url",
      href: "http://example.com",
    });
  });

  it("phone digit-count boundaries: 7 and 15 call, 6 and 16 don't", () => {
    expect(kinds("1234567")).toContain("call");
    expect(kinds("123456789012345")).toContain("call");
    expect(kinds("123456")).not.toContain("call");
    expect(kinds("1234567890123456")).not.toContain("call");
  });

  it("coordinate range boundaries: ±90/±180 valid, beyond invalid", () => {
    expect(kinds("90, 180")).toContain("maps");
    expect(kinds("-90, -180")).toContain("maps");
    expect(kinds("90.0001, 0")).not.toContain("maps");
    expect(kinds("0, 180.5")).not.toContain("maps");
  });

  it("compact coordinates without a space still map", () => {
    expect(detectSmartActions("48.1,11.5")[0]).toMatchObject({ kind: "maps" });
  });

  it("integer coordinate pairs are treated as coords, never as a phone number", () => {
    // "123, 456" matches the coord shape (out of range) — the else-if chain
    // must not fall through to the phone detector.
    expect(kinds("123, 456")).toEqual(["qr"]);
  });

  it("an email with Umlauts still composes (no crash, mailto preserved)", () => {
    const a = detectSmartActions("grüße@example.de");
    expect(a[0]).toMatchObject({ kind: "email", href: "mailto:grüße@example.de" });
  });

  it("an IDN bare domain (non-ASCII) degrades to QR only", () => {
    // The domain matcher is deliberately ASCII-only.
    expect(kinds("münchen.de")).toEqual(["qr"]);
  });

  it("a URL with embedded whitespace is not a URL", () => {
    expect(kinds("https://exa mple.com")).not.toContain("open-url");
  });

  it("multi-line content gets no actions at all, even when a line is a URL", () => {
    expect(kinds("https://example.com\nsecond line")).toEqual([]);
  });

  it("the tel: href keeps the leading + and strips everything non-digit", () => {
    const a = detectSmartActions("+1 (555) 123-4567");
    expect(a[0].href).toBe("tel:+15551234567");
  });
});
