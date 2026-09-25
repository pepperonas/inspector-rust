import { describe, it, expect } from "vitest";
import cases from "./repo-url-cases.json";
import { parseRepoUrl, findRepoUrl } from "./repo-url";

describe("parseRepoUrl (shared fixture with repo_url.rs)", () => {
  for (const c of cases as { input: string; expect: [string, string] | null }[]) {
    it(`${JSON.stringify(c.input)} → ${JSON.stringify(c.expect)}`, () => {
      const r = parseRepoUrl(c.input);
      if (c.expect === null) expect(r).toBeNull();
      else {
        expect(r).not.toBeNull();
        expect([r!.owner, r!.repo]).toEqual(c.expect);
        expect(r!.web_url).toBe(`https://github.com/${c.expect[0]}/${c.expect[1]}`);
        expect(r!.clone_url).toBe(`https://github.com/${c.expect[0]}/${c.expect[1]}.git`);
      }
    });
  }
});

describe("findRepoUrl (inside prose / a clip)", () => {
  it("finds a URL in the middle of text and trims trailing punctuation", () => {
    expect(findRepoUrl("schau mal: https://github.com/o/r.")).toMatchObject({ owner: "o", repo: "r" });
    expect(findRepoUrl("(siehe https://github.com/o/r)")).toMatchObject({ owner: "o", repo: "r" });
  });
  it("skips non-repo github links and returns the first repo link", () => {
    expect(findRepoUrl("https://github.com/o/r/issues/1 und https://github.com/a/b")).toMatchObject({ owner: "a", repo: "b" });
  });
  it("returns null when no repo link exists", () => {
    expect(findRepoUrl("nur text ohne link")).toBeNull();
  });
});
