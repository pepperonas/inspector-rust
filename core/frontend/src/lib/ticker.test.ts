import { describe, expect, it } from "vitest";
import { advance, applyDropped, applyFinal, applyHypothesis, MAX_PIECES, prune, showsLabel, TICKER_CHOICES, tickerChoice, tickerPatchFor, type Piece } from "./ticker";

const hyp = (source: "mic" | "sys", stable: string, pending = "") => ({ source, seq: 1, stable, pending });

describe("ticker model", () => {
  it("final replaces the pending piece of the same source only", () => {
    let p: Piece[] = [];
    p = applyHypothesis(p, hyp("mic", "ich gehe", "heute"));
    p = applyHypothesis(p, hyp("sys", "hallo"));
    p = applyFinal(p, { source: "mic", seq: 1, text: "Ich gehe heute." });
    expect(p.map((x) => [x.source, x.final, x.text])).toEqual([
      ["mic", true, "Ich gehe heute."],
      ["sys", false, "hallo"],
    ]);
  });

  it("keeps one pending piece per source, updated in place", () => {
    let p: Piece[] = [];
    p = applyHypothesis(p, hyp("mic", "a"));
    p = applyHypothesis(p, hyp("sys", "b"));
    p = applyHypothesis(p, hyp("mic", "a", "c"));
    expect(p.map((x) => x.text)).toEqual(["a c", "b"]);
    expect(p.filter((x) => !x.final)).toHaveLength(2);
  });

  it("finals stay in arrival order before the previews", () => {
    let p: Piece[] = [];
    p = applyHypothesis(p, hyp("sys", "läuft"));
    p = applyFinal(p, { source: "mic", seq: 1, text: "eins" });
    p = applyFinal(p, { source: "mic", seq: 2, text: "zwei" });
    expect(p.map((x) => x.text)).toEqual(["eins", "zwei", "läuft"]);
  });

  it("a dropped segment or an empty preview removes the preview", () => {
    let p = applyHypothesis([], hyp("mic", "x"));
    expect(applyDropped(p, "mic")).toEqual([]);
    p = applyHypothesis(p, hyp("mic", "", ""));
    expect(p).toEqual([]);
  });

  it("caps at the maximum without losing previews", () => {
    let p = applyHypothesis([], hyp("sys", "vorschau"));
    for (let i = 0; i < 60; i++) p = applyFinal(p, { source: "mic", seq: i, text: `s${i}` });
    expect(p).toHaveLength(MAX_PIECES);
    expect(p[p.length - 1].text).toBe("vorschau");
    expect(p[p.length - 2].text).toBe("s59");
  });

  it("stops when the newest text is at rest and speeds up to 3x with a backlog", () => {
    expect(advance(500, 600, 16, 60, 1000)).toBe(0);
    const slow = advance(610, 600, 100, 600, 1000);
    expect(slow).toBeCloseTo(10, 5); // never past the anchor
    const base = advance(700, 600, 100, 60, 1000);
    const piled = advance(600 + 1500 * 2, 600, 100, 60, 1000);
    expect(base).toBeGreaterThan(5.9);
    expect(piled / 6).toBeCloseTo(3, 5);
    expect(advance(5000, 600, 100, 60, 1000) / 6).toBeLessThanOrEqual(3);
  });

  it("prune keeps the visual position", () => {
    const p: Piece[] = ["a", "b", "c"].map((id) => ({ id, source: "mic", text: id, final: true }));
    const widths = { a: 100, b: 50, c: 70 };
    const r = prune(p, widths, -160);
    expect(r.pieces.map((x) => x.id)).toEqual(["c"]);
    // c started at -160 + 150 = -10 before; it still starts there.
    expect(r.offset).toBe(-10);
    expect(prune(p, widths, -50).pieces).toHaveLength(3);
  });

  it("labels the speaker where it changes", () => {
    const p: Piece[] = [
      { id: "1", source: "mic", text: "a", final: true },
      { id: "2", source: "mic", text: "b", final: true },
      { id: "3", source: "sys", text: "c", final: true },
    ];
    expect([0, 1, 2].map((i) => showsLabel(p, i))).toEqual([true, false, true]);
  });
});

describe("ticker modes", () => {
  it("a size button toggles between its mode and the normal bar", async () => {
    const { nextMode, lastLines } = await import("./ticker");
    expect(nextMode("bar", "max")).toBe("max");
    expect(nextMode("max", "max")).toBe("bar");
    expect(nextMode("max", "mini")).toBe("mini");
    expect(nextMode("mini", "mini")).toBe("bar");
    const p = ["a", "b", "c"].map((id) => ({ id, source: "mic" as const, text: id, final: true }));
    expect(lastLines(p, 2).map((x) => x.id)).toEqual(["b", "c"]);
    expect(lastLines(p, 0)).toEqual([]);
  });
});

describe("ticker choice (meeting panel control)", () => {
  it("a hidden ticker reads as off, whatever its size", () => {
    expect(tickerChoice(false, "max")).toBe("off");
    expect(tickerChoice(true, "mini")).toBe("mini");
    expect(tickerChoice(true, "bar")).toBe("bar");
  });

  it("off hides it and keeps its size; a size shows it again", () => {
    expect(tickerPatchFor("off")).toEqual({ visible: false });
    expect(tickerPatchFor("bar")).toEqual({ visible: true, mode: "bar" });
    expect(tickerPatchFor("max")).toEqual({ visible: true, mode: "max" });
  });

  it("offers every choice once, off first", () => {
    expect(TICKER_CHOICES).toEqual(["off", "bar", "mini", "max"]);
  });
});
