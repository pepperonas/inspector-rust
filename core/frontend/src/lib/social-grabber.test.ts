import { describe, it, expect } from "vitest";
import { extractSocialLinks, allYouTube } from "./social";

describe("extractSocialLinks", () => {
  it("finds several links across mixed separators", () => {
    const text = `
      https://youtu.be/aaa
      https://www.tiktok.com/@x/video/1, https://vimeo.com/999
      schau mal: https://www.youtube.com/watch?v=bbb und tschüss
    `;
    expect(extractSocialLinks(text).map((t) => t.url)).toEqual([
      "https://youtu.be/aaa",
      "https://www.tiktok.com/@x/video/1",
      "https://www.youtube.com/watch?v=bbb",
    ]);
  });

  it("drops non-social links", () => {
    expect(extractSocialLinks("https://example.com/a https://vimeo.com/1")).toEqual([]);
  });

  it("deduplicates, keeping the first occurrence order", () => {
    const t = extractSocialLinks("https://youtu.be/a https://youtu.be/b https://youtu.be/a");
    expect(t.map((x) => x.url)).toEqual(["https://youtu.be/a", "https://youtu.be/b"]);
  });

  it("strips sentence punctuation a paste dragged in", () => {
    for (const [raw, want] of [
      ["Guck: https://youtu.be/a.", "https://youtu.be/a"],
      ["https://youtu.be/a, https://youtu.be/b;", "https://youtu.be/a"],
      ["(https://youtu.be/a)", "https://youtu.be/a"],
      ["<https://youtu.be/a>", "https://youtu.be/a"],
      ["https://youtu.be/a?!", "https://youtu.be/a"],
    ] as const) {
      expect(extractSocialLinks(raw)[0]?.url, raw).toBe(want);
    }
  });

  it("keeps a balanced bracket that belongs to the url", () => {
    // Wikipedia-style parentheses appear in real query strings; only an
    // UNBALANCED closer is punctuation.
    const u = "https://www.youtube.com/watch?v=a(b)";
    expect(extractSocialLinks(u)[0]?.url).toBe(u);
  });

  it("classifies each link on its own", () => {
    const t = extractSocialLinks("https://youtu.be/a https://www.instagram.com/p/x/");
    expect(t.map((x) => x.platform)).toEqual(["youtube", "instagram"]);
  });

  it("returns nothing for text without links", () => {
    expect(extractSocialLinks("kein link weit und breit")).toEqual([]);
  });
});

describe("allYouTube", () => {
  it("is false for an empty list — nothing to offer audio for", () => {
    expect(allYouTube([])).toBe(false);
  });
  it("is false as soon as one link is not YouTube", () => {
    expect(allYouTube(extractSocialLinks("https://youtu.be/a https://vm.tiktok.com/x"))).toBe(false);
  });
  it("is true when every link is YouTube", () => {
    expect(allYouTube(extractSocialLinks("https://youtu.be/a https://youtube.com/watch?v=b"))).toBe(true);
  });
});
