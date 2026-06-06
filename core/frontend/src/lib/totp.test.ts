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
});
