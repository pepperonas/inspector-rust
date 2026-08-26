import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup } from "@testing-library/react";
import { InlineMd } from "./InlineMd";
import { COMMAND_DOCS } from "../lib/commandDocs";

afterEach(cleanup);

describe("InlineMd", () => {
  it("renders code spans as <code> and bold as <strong>", () => {
    const { container } = render(<InlineMd text="**Info** — run `adb wifi` now" />);
    expect(container.querySelector("strong")?.textContent).toBe("Info");
    expect(container.querySelector("code")?.textContent).toBe("adb wifi");
    // The visible text keeps every word, minus the markers.
    expect(container.textContent).toBe("Info — run adb wifi now");
  });

  it("plain text renders without extra elements", () => {
    const { container } = render(<InlineMd text="nothing special here" />);
    expect(container.querySelectorAll("code, strong")).toHaveLength(0);
    expect(container.textContent).toBe("nothing special here");
  });

  it("never hard-codes a colour — the chip inherits via currentColor", () => {
    // These strings also render on the SELECTED command row (white on rose);
    // a fixed accent/fg colour would be illegible there.
    const { container } = render(<InlineMd text="run `x`" />);
    const cls = container.querySelector("code")?.className ?? "";
    expect(cls).toContain("currentColor");
    expect(cls).not.toMatch(/--color-(accent|fg)\b/);
  });

  it("keeps unmatched markers literal instead of eating text", () => {
    const { container } = render(<InlineMd text="an unclosed `code span" />);
    expect(container.textContent).toBe("an unclosed `code span");
  });
});

describe("the doc registry renders without raw markers", () => {
  // The v0.131.0 field report: the `?` panel showed literal `**Info**` and
  // backticks. Guard EVERY doc string, not just the reported one.
  it("no doc field leaks `**` or a stray backtick into the rendered text", () => {
    const offenders: string[] = [];
    const check = (raw: string, where: string) => {
      const { container } = render(<InlineMd text={raw} />);
      const shown = container.textContent ?? "";
      // A marker may legitimately survive when it was unmatched in the source
      // (kept literal on purpose) — so compare against the source itself.
      const sourceHasPair = /\*\*[^*\s][^*]*\*\*/.test(raw.replace(/`[^`]+`/g, ""));
      const sourceHasCode = /`[^`\n]+`/.test(raw);
      if (sourceHasPair && shown.includes("**")) offenders.push(`${where}: bold marker visible`);
      if (sourceHasCode && shown.includes("`")) offenders.push(`${where}: backtick visible`);
      cleanup();
    };
    for (const d of COMMAND_DOCS) {
      check(d.description, `${d.command}.description`);
      check(d.tagline, `${d.command}.tagline`);
      d.arguments.forEach((a) => check(a.description, `${d.command}.arg(${a.name})`));
      d.flags.forEach((f) => check(f.description, `${d.command}.flag(${f.flag})`));
      d.examples.forEach((e, i) => check(e.result, `${d.command}.example[${i}]`));
      d.tips.forEach((t, i) => check(t, `${d.command}.tip[${i}]`));
      d.caveats.forEach((c, i) => check(c, `${d.command}.caveat[${i}]`));
    }
    expect(offenders).toEqual([]);
  });

  it("the reported adb description formats its sections", () => {
    const adb = COMMAND_DOCS.find((d) => d.command === "adb");
    expect(adb, "adb doc exists").toBeTruthy();
    const { container } = render(<InlineMd text={adb!.description} />);
    const bold = [...container.querySelectorAll("strong")].map((e) => e.textContent);
    expect(bold).toContain("Info");
    expect(container.textContent).not.toContain("**");
  });
});
