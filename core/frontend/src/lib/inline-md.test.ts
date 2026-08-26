import { describe, it, expect } from "vitest";
import {
  parseInline,
  hasInlineMarkup,
  reserialize,
  stripInlineMarkup,
  type InlineToken,
} from "./inline-md";

const kinds = (t: InlineToken[]) => t.map((x) => `${x.kind}:${x.text}`);

describe("parseInline", () => {
  it("plain text is one token", () => {
    expect(parseInline("just words")).toEqual([{ kind: "text", text: "just words" }]);
    expect(parseInline("")).toEqual([]);
  });

  it("extracts code spans", () => {
    expect(kinds(parseInline("bare `adb` starts on the dashboard."))).toEqual([
      "text:bare ",
      "code:adb",
      "text: starts on the dashboard.",
    ]);
  });

  it("extracts bold", () => {
    expect(kinds(parseInline("**Info** — live dashboard"))).toEqual([
      "bold:Info",
      "text: — live dashboard",
    ]);
  });

  it("handles the real adb description shape (bold + code mixed)", () => {
    const s = "**Info** — dashboard. **WLAN** — switch a device, or run `adb wifi` directly.";
    expect(kinds(parseInline(s))).toEqual([
      "bold:Info",
      "text: — dashboard. ",
      "bold:WLAN",
      "text: — switch a device, or run ",
      "code:adb wifi",
      "text: directly.",
    ]);
  });

  it("code wins over bold — markers inside backticks stay literal", () => {
    // This is what protects glob patterns and any `**` inside code.
    expect(kinds(parseInline("see `src/**/*.ts` for it"))).toEqual([
      "text:see ",
      "code:src/**/*.ts",
      "text: for it",
    ]);
    expect(kinds(parseInline("`**not bold**`"))).toEqual(["code:**not bold**"]);
  });

  it("does NOT support *italic* — a lone star stays literal", () => {
    // Real caveat text from the `alias` doc: a lone-star rule would silently
    // italicise "function".
    const s = "Windows creates a PowerShell *function* (Set-Alias can't carry arguments)";
    expect(parseInline(s)).toEqual([{ kind: "text", text: s }]);
    expect(hasInlineMarkup(s)).toBe(false);
  });

  it("unmatched or degenerate markers stay literal (no text is ever lost)", () => {
    for (const s of [
      "an unclosed `code span",
      "a lone ** pair opener",
      "empty `` backticks",
      "a ** b ** c", // whitespace-flanked → not emphasis
      "**", // just the marker
      "***", // odd run
      "trailing backtick `",
      "glob src/**/* without a closer",
    ]) {
      const tokens = parseInline(s);
      expect(reserialize(tokens), s).toBe(s);
    }
  });

  it("round-trips every shape — the no-text-loss invariant", () => {
    for (const s of [
      "**bold** and `code` together",
      "`code` first then **bold**",
      "nested-looking `**x**` stays code",
      "back-to-back **a****b**",
      "unicode — ✓ `ü` **ö**",
      "",
      "*",
      "`",
    ]) {
      expect(reserialize(parseInline(s)), s).toBe(s);
    }
  });

  it("bold body containing a star is not emphasis (glob safety)", () => {
    expect(kinds(parseInline("**a*b**"))).toEqual(["text:**a*b**"]);
  });

  it("adjacent plain runs merge — no empty or split text tokens", () => {
    const t = parseInline("a ** b `c` d");
    expect(t.every((x) => x.text.length > 0)).toBe(true);
    // The literal `**` merged into its neighbouring text rather than splitting.
    expect(t.filter((x) => x.kind === "text")).toHaveLength(2);
  });
});

describe("hasInlineMarkup / stripInlineMarkup", () => {
  it("detects formatting only when it would actually render", () => {
    expect(hasInlineMarkup("plain")).toBe(false);
    expect(hasInlineMarkup("has `code`")).toBe(true);
    expect(hasInlineMarkup("has **bold**")).toBe(true);
    expect(hasInlineMarkup("unclosed `code")).toBe(false);
  });

  it("strips markers for tooltip strings", () => {
    expect(stripInlineMarkup("**Info** — run `adb wifi`")).toBe("Info — run adb wifi");
    expect(stripInlineMarkup("plain")).toBe("plain");
  });
});
