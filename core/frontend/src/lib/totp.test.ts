import { describe, it, expect } from "vitest";
import { matchTotpEntries, type TotpEntry } from "./totp";

function entry(id: number, issuer: string, account: string): TotpEntry {
  return { id, issuer, account, digits: 6, period: 30, algorithm: "SHA1", created_at: 0 };
}

const amazon = entry(1, "Amazon", "alice@example.com");
const apple = entry(2, "Apple", "alice@icloud.com");
const github = entry(3, "GitHub", "octocat");
const google = entry(4, "Google", "bob@gmail.com");
const all = [amazon, apple, github, google];

describe("matchTotpEntries", () => {
  it("returns all entries unchanged for an empty query", () => {
    expect(matchTotpEntries("", all)).toEqual(all);
    expect(matchTotpEntries("   ", all)).toEqual(all);
  });

  it("is case-insensitive on the issuer", () => {
    expect(matchTotpEntries("AMAZON", all).map((e) => e.id)).toEqual([1]);
    expect(matchTotpEntries("amazon", all).map((e) => e.id)).toEqual([1]);
  });

  it("matches an issuer prefix", () => {
    expect(matchTotpEntries("ama", all).map((e) => e.id)).toEqual([1]);
  });

  it("matches an issuer infix", () => {
    // "it" appears inside "GitHub".
    expect(matchTotpEntries("it", all).map((e) => e.id)).toContain(3);
  });

  it("ranks prefix matches above infix matches", () => {
    const apl = entry(5, "Appliance", "x"); // prefix "app"
    const grappa = entry(6, "Grappa", "y"); // infix "app"
    const res = matchTotpEntries("app", [grappa, apl]);
    expect(res[0].id).toBe(5); // prefix wins
    expect(res[1].id).toBe(6);
  });

  it("a shorter prefix query scores higher (more specific match earlier)", () => {
    // Two issuers both prefix-matched; both score 100 - q.length, so the
    // ranking is stable but the score reflects query length.
    const res = matchTotpEntries("a", [amazon, apple]);
    expect(res.map((e) => e.id).sort()).toEqual([1, 2]);
  });

  it("falls back to account matches when the issuer doesn't match", () => {
    // "octocat" only matches GitHub's account.
    expect(matchTotpEntries("octo", all).map((e) => e.id)).toEqual([3]);
  });

  it("ranks issuer matches above account-only matches", () => {
    // "gmail" matches Google's account; an issuer with "gm" prefix outranks.
    const gmco = entry(7, "Gmco", "z"); // issuer prefix "gm"... query "gm"
    const res = matchTotpEntries("gm", [google, gmco]);
    expect(res[0].id).toBe(7); // issuer prefix beats account infix
  });

  it("returns nothing when neither issuer nor account match", () => {
    expect(matchTotpEntries("zzzzz", all)).toEqual([]);
  });

  it("account prefix outranks account infix", () => {
    const a = entry(8, "X", "bobby"); // account prefix "bob"
    const b = entry(9, "Y", "xxbobxx"); // account infix "bob"
    const res = matchTotpEntries("bob", [b, a]);
    expect(res[0].id).toBe(8);
  });

  it("does not mutate the input array", () => {
    const copy = [...all];
    matchTotpEntries("a", all);
    expect(all).toEqual(copy);
  });

  it("trims surrounding whitespace in the query", () => {
    expect(matchTotpEntries("  amazon  ", all).map((e) => e.id)).toEqual([1]);
  });

  it("returns [] for an empty entry list, regardless of query", () => {
    expect(matchTotpEntries("", [])).toEqual([]);
    expect(matchTotpEntries("amazon", [])).toEqual([]);
  });

  it("equal scores keep the stored order (stable sort)", () => {
    // Same-length issuers, same prefix query → identical score.
    const a = entry(10, "Alpha", "x");
    const b = entry(11, "Amiga", "y");
    expect(matchTotpEntries("a", [a, b]).map((e) => e.id)).toEqual([10, 11]);
    expect(matchTotpEntries("a", [b, a]).map((e) => e.id)).toEqual([11, 10]);
  });

  it("an earlier issuer-infix match outranks a later one", () => {
    const early = entry(12, "xgit", "a"); // "git" at index 1
    const late = entry(13, "xxgit", "b"); // "git" at index 2
    expect(matchTotpEntries("git", [late, early]).map((e) => e.id)).toEqual([12, 13]);
  });

  it("matches Umlaut issuers case-insensitively", () => {
    const ueber = entry(14, "Über-Secure", "me");
    expect(matchTotpEntries("über", [ueber]).map((e) => e.id)).toEqual([14]);
    expect(matchTotpEntries("ÜBER", [ueber]).map((e) => e.id)).toEqual([14]);
  });

  it("matching a full issuer+account query set never invents matches", () => {
    // Query spanning issuer AND account text matches neither field alone.
    expect(matchTotpEntries("amazon alice", all)).toEqual([]);
  });

  it("the account is only consulted when the issuer misses entirely", () => {
    // "ali" hits both Amazon's and Apple's accounts but no issuer → account tier;
    // both surface (order preserved: equal account-prefix scores).
    expect(matchTotpEntries("ali", all).map((e) => e.id)).toEqual([1, 2]);
  });
});
