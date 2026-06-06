import { describe, it, expect } from "vitest";
import { matchMemes, memeScore, type MemeEntry } from "./meme";

function m(name: string, category = ""): MemeEntry {
  return { name, category, path: `/memes/${category}/${name}.gif` };
}

const lib: MemeEntry[] = [
  m("grumpy-cat", "cats"),
  m("happy-dog", "dogs"),
  m("deal-with-it", "reactions"),
  m("cat-vibing", "cats"),
  m("facepalm", "reactions"),
];

describe("memeScore", () => {
  it("scores 0 for an empty query (everything matches)", () => {
    expect(memeScore("", m("x"))).toBe(0);
  });
  it("ranks an exact name above a prefix above an infix", () => {
    const exact = memeScore("facepalm", m("facepalm"))!;
    const prefix = memeScore("face", m("facepalm"))!;
    const infix = memeScore("palm", m("facepalm"))!;
    expect(exact).toBeGreaterThan(prefix);
    expect(prefix).toBeGreaterThan(infix);
  });
  it("a name match outranks a category-only match", () => {
    const nameHit = memeScore("cat", m("cat-vibing", "dogs"))!; // name has "cat"
    const catHit = memeScore("cat", m("doge", "cats"))!; // only category
    expect(nameHit).toBeGreaterThan(catHit);
  });
  it("matches via category when the name doesn't", () => {
    expect(memeScore("dogs", m("doge", "dogs"))).not.toBeNull();
  });
  it("supports 3+ char subsequence, not 1–2 char", () => {
    expect(memeScore("gmc", m("grumpy-cat"))).not.toBeNull(); // g..m..c subsequence
    expect(memeScore("zz", m("grumpy-cat"))).toBeNull();
  });
  it("returns null when nothing matches", () => {
    expect(memeScore("zzzzz", m("grumpy-cat", "cats"))).toBeNull();
  });
});

describe("matchMemes", () => {
  it("returns the whole library (capped) for an empty query", () => {
    expect(matchMemes("", lib)).toEqual(lib);
    expect(matchMemes("   ", lib).length).toBe(lib.length);
  });
  it("filters + ranks by relevance", () => {
    const res = matchMemes("cat", lib).map((x) => x.name);
    // Both cat-named memes surface; the dog/reaction ones don't.
    expect(res).toContain("grumpy-cat");
    expect(res).toContain("cat-vibing");
    expect(res).not.toContain("happy-dog");
  });
  it("matches on category too", () => {
    const res = matchMemes("reactions", lib).map((x) => x.name);
    expect(res).toContain("deal-with-it");
    expect(res).toContain("facepalm");
  });
  it("respects the result cap", () => {
    const many = Array.from({ length: 200 }, (_, i) => m(`meme-${i}`, "x"));
    expect(matchMemes("meme", many, 60).length).toBe(60);
  });
  it("does not mutate the input", () => {
    const copy = [...lib];
    matchMemes("cat", lib);
    expect(lib).toEqual(copy);
  });
});
