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

  it("ranking survives the cap: the best matches are kept, not the first", () => {
    // A late-in-the-library prefix match must beat early subsequence matches
    // even with limit 1.
    const many = [m("xaxbxc"), m("xxabc"), m("abc-classic")]; // subsequence, infix, prefix
    expect(matchMemes("abc", many, 1).map((x) => x.name)).toEqual(["abc-classic"]);
  });

  it("breaks score ties alphabetically by name (deterministic order)", () => {
    const res = matchMemes("x", [m("xb"), m("xa")]).map((e) => e.name);
    expect(res).toEqual(["xa", "xb"]);
  });

  it("empty-query cap applies too", () => {
    const many = Array.from({ length: 100 }, (_, i) => m(`m${i}`));
    expect(matchMemes("", many, 10).length).toBe(10);
  });
});

describe("memeScore — more ranking details", () => {
  it("is case-insensitive including Umlauts", () => {
    expect(memeScore("ÜBER", m("über-cat"))).not.toBeNull();
    expect(matchMemes("über", [m("ÜBER-CAT")]).length).toBe(1);
  });

  it("trims surrounding whitespace in the query", () => {
    expect(memeScore("  facepalm  ", m("facepalm"))).toBe(memeScore("facepalm", m("facepalm")));
  });

  it("an exact name match outranks an exact category match on another meme", () => {
    const byName = memeScore("cat", m("cat", "misc"))!;
    const byCat = memeScore("cat", m("doge", "cat"))!;
    expect(byName).toBeGreaterThan(byCat);
  });

  it("when both name and category match, the score never drops below the category tier", () => {
    // Name subsequence (weak) + category exact (strong) → the category score wins.
    const both = memeScore("cat", m("c-a-t-collage", "cat"))!;
    const catOnly = memeScore("cat", m("zzz", "cat"))!;
    expect(both).toBeGreaterThanOrEqual(catOnly);
  });

  it("an earlier infix match scores above a later one", () => {
    const early = memeScore("cat", m("acat"))!; // index 1
    const late = memeScore("cat", m("aacat"))!; // index 2
    expect(early).toBeGreaterThan(late);
  });

  it("a shorter prefix-matched name outranks a longer one", () => {
    const short = memeScore("cat", m("cats"))!;
    const long = memeScore("cat", m("cat-compilation-2024"))!;
    expect(short).toBeGreaterThan(long);
  });

  it("category subsequence works for 3+ char queries", () => {
    expect(memeScore("rcn", m("zzz", "reactions"))).not.toBeNull();
    expect(memeScore("rc", m("zzz", "reactions"))).toBeNull(); // 2 chars → no subsequence tier
  });

  it("a top-level file (empty category) never matches via category", () => {
    expect(memeScore("cats", m("doge", ""))).toBeNull();
  });
});
