import { describe, expect, it } from "vitest";
import {
  findSensitive,
  parseOcrLines,
  detectFindings,
  snapRectToWords,
  isCovered,
  summarizeFindings,
  type OcrLine,
} from "./editor-ocr";

const kinds = (s: string) => findSensitive(s).map((m) => [m.kind, s.slice(m.start, m.end)]);

describe("findSensitive", () => {
  it("finds e-mail addresses", () => {
    expect(kinds("Kontakt: ana.lima+test@northwind.example.com, danke")).toEqual([
      ["email", "ana.lima+test@northwind.example.com"],
    ]);
  });

  it("finds IBANs only when the checksum holds", () => {
    expect(kinds("IBAN DE89 3704 0044 0532 0130 00 bitte")).toEqual([["iban", "DE89 3704 0044 0532 0130 00"]]);
    expect(kinds("DE89370400440532013000")).toEqual([["iban", "DE89370400440532013000"]]);
    expect(kinds("DE88 3704 0044 0532 0130 00")).toEqual([]); // wrong check digits
  });

  it("finds card numbers only when Luhn holds", () => {
    expect(kinds("Karte 4111 1111 1111 1111 gültig")).toEqual([["card", "4111 1111 1111 1111"]]);
    expect(kinds("4111 1111 1111 1112")).toEqual([]);
  });

  it("finds API keys by known prefixes and as values of secret-looking keys", () => {
    expect(kinds("export OPENAI=sk-proj-AbCdEf0123456789xyzABC")).toEqual([["secret", "sk-proj-AbCdEf0123456789xyzABC"]]);
    expect(kinds("ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789")).toEqual([["secret", "ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"]]);
    expect(kinds("AKIAIOSFODNN7EXAMPLE")).toEqual([["secret", "AKIAIOSFODNN7EXAMPLE"]]);
    expect(kinds("password: hunter2")).toEqual([["secret", "hunter2"]]);
    expect(kinds("Passwort = Sommer2026!")).toEqual([["secret", "Sommer2026!"]]);
    expect(kinds("api_key=12345")).toEqual([["secret", "12345"]]);
    expect(kinds('token: "abc123def", next')).toEqual([["secret", "abc123def"]]);
  });

  it("treats a long random token as a secret but not a commit hash or a plain word", () => {
    expect(kinds("token Zq8vR2mXc7LpT4wY9nB1kH6sJ3dF5gA0eU")).toEqual([["secret", "Zq8vR2mXc7LpT4wY9nB1kH6sJ3dF5gA0eU"]]);
    expect(kinds("commit 3f2a9c1e8b7d6a5f4e3d2c1b0a9f8e7d6c5b4a3f")).toEqual([]);
    expect(kinds("Donaudampfschifffahrtsgesellschaftskapitän")).toEqual([]);
  });

  it("does not call a repetitive placeholder a secret", () => {
    expect(kinds("value aA1aA1aA1aA1aA1aA1aA1aA1aA1")).toEqual([]);
  });

  it("ignores digit runs too short to be a phone number", () => {
    expect(kinds("Zimmer 030 12")).toEqual([]);
    expect(kinds("Raum 030 - 12")).toEqual([]); // matches the pattern, 5 digits
  });

  it("finds JWTs and URLs that carry a token", () => {
    const jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    expect(kinds(`Bearer ${jwt}`)).toEqual([["secret", jwt]]);
    expect(kinds("https://api.example.com/v1/items?id=4&access_token=abc123 ok")).toEqual([
      ["url-token", "https://api.example.com/v1/items?id=4&access_token=abc123"],
    ]);
    expect(kinds("https://example.com/docs?page=2")).toEqual([]);
  });

  it("finds phone numbers but not dates, times, versions or prices", () => {
    expect(kinds("Tel. +49 30 1234567")).toEqual([["phone", "+49 30 1234567"]]);
    expect(kinds("Ruf an: 030 / 123 456 78")).toEqual([["phone", "030 / 123 456 78"]]);
    expect(kinds("0176-12345678")).toEqual([["phone", "0176-12345678"]]);
    expect(kinds("am 08.11.2026 um 14:30, Version 0.245.0, 1.299,00 €")).toEqual([]);
  });

  it("finds IPv4 addresses with valid octets", () => {
    expect(kinds("Server 203.0.113.42 antwortet")).toEqual([["ip", "203.0.113.42"]]);
    expect(kinds("999.1.1.1")).toEqual([]);
  });

  it("claims each character once: an e-mail is not also a secret, an IBAN not also a phone", () => {
    expect(findSensitive("DE89 3704 0044 0532 0130 00")).toHaveLength(1);
    expect(findSensitive("token=ana@northwind.example")).toHaveLength(1);
  });

  it("returns nothing for ordinary text", () => {
    expect(findSensitive("Ein ganz normaler Satz ohne Geheimnisse.")).toEqual([]);
    expect(findSensitive("")).toEqual([]);
  });
});

/** A line "Mail an ana@x.de jetzt" on a 1000×500 image; each word 100 px wide. */
function line(text: string, y = 0.1): OcrLine {
  const words: OcrLine["w"] = [];
  let i = 0;
  let col = 0;
  for (const tok of text.split(" ")) {
    words.push({ s: i, e: i + tok.length, b: [col * 0.1, y, 0.09, 0.04] });
    i += tok.length + 1;
    col++;
  }
  return { t: text, b: [0, y, col * 0.1, 0.04], w: words };
}

describe("parseOcrLines", () => {
  it("reads the helper's JSON and drops malformed entries", () => {
    const json = JSON.stringify([
      line("a b"),
      { t: 3, b: [0, 0, 1, 1], w: [] },
      { t: "x", b: [0, 0, 1], w: [] },
      { t: "y", b: [0, 0, 1, 1], w: [{ s: 0, e: 1, b: [0, 0, "1", 1] }, { s: 0, e: 1, b: [0, 0, 1, 1] }] },
    ]);
    const out = parseOcrLines(json);
    expect(out.map((l) => l.t)).toEqual(["a b", "y"]);
    expect(out[1].w).toHaveLength(1);
  });

  it("returns [] for garbage", () => {
    expect(parseOcrLines("nope")).toEqual([]);
    expect(parseOcrLines('{"t":1}')).toEqual([]);
  });
});

describe("detectFindings", () => {
  it("maps a finding to the image rectangle of its words, padded", () => {
    const f = detectFindings([line("Mail an ana@x.de jetzt")], 1000, 500);
    expect(f).toHaveLength(1);
    expect(f[0].kind).toBe("email");
    const r = f[0].rect;
    // word 3 spans x 200..290, y 50..70 — padding grows it a little on every side
    expect(r.x).toBeLessThan(200);
    expect(r.x).toBeGreaterThan(190);
    expect(r.x + r.w).toBeGreaterThan(290);
    expect(r.x + r.w).toBeLessThan(300);
    expect(r.y).toBeLessThan(50);
    expect(r.y + r.h).toBeGreaterThan(70);
  });

  it("covers every word of a multi-word finding with one rectangle", () => {
    const f = detectFindings([line("Tel. +49 30 1234567")], 1000, 500);
    expect(f).toHaveLength(1);
    expect(f[0].rect.x).toBeLessThan(100);
    expect(f[0].rect.x + f[0].rect.w).toBeGreaterThan(390);
  });

  it("covers the whole line when ONE word of a multi-word finding has no box", () => {
    const l = line("Tel. +49 30 1234567");
    l.w = l.w.filter((w) => w.s !== 9); // "30" missing
    const f = detectFindings([l], 1000, 500);
    expect(f).toHaveLength(1);
    expect(f[0].rect.x).toBeLessThan(0);
  });

  it("covers the whole line when a word of the finding has no box", () => {
    const l = line("Mail an ana@x.de jetzt");
    l.w = l.w.filter((w) => w.s !== 8);
    const f = detectFindings([l], 1000, 500);
    expect(f).toHaveLength(1);
    expect(f[0].rect.x).toBeLessThan(0); // line starts at 0, padded outwards
    expect(f[0].rect.x + f[0].rect.w).toBeGreaterThan(400);
  });
});

describe("snapRectToWords", () => {
  it("returns one tight box per line for the words whose centre is in the drag", () => {
    const lines = [line("one two three", 0.1), line("four five six", 0.3)];
    // drag over "two three" on line 1 and "five six" on line 2 (image 1000×500)
    const out = snapRectToWords(lines, { x: 120, y: 40, w: 200, h: 130 }, 1000, 500);
    expect(out).toHaveLength(2);
    expect(out[0].x).toBeGreaterThan(95);
    expect(out[0].x).toBeLessThan(100);
    expect(out[0].x + out[0].w).toBeGreaterThan(290);
    expect(out[1].y).toBeGreaterThan(140);
  });

  it("leaves out lines above or below the drag", () => {
    const lines = [line("one two three", 0.1), line("four five six", 0.3)];
    const out = snapRectToWords(lines, { x: 0, y: 40, w: 1000, h: 40 }, 1000, 500);
    expect(out).toHaveLength(1);
    expect(out[0].y).toBeLessThan(50);
  });

  it("returns [] when the drag covers no word", () => {
    expect(snapRectToWords([line("one two")], { x: 600, y: 300, w: 50, h: 50 }, 1000, 500)).toEqual([]);
  });
});

describe("isCovered", () => {
  it("is true when an existing box contains the rectangle (with a small tolerance)", () => {
    const r = { x: 10, y: 10, w: 50, h: 20 };
    expect(isCovered(r, [{ x: 11, y: 9, w: 49, h: 22 }])).toBe(true);
    expect(isCovered(r, [{ x: 30, y: 10, w: 50, h: 20 }])).toBe(false);
    expect(isCovered(r, [])).toBe(false);
  });
});

describe("summarizeFindings", () => {
  it("counts per kind, most frequent first", () => {
    const f = detectFindings(
      [line("a@b.de c@d.de"), line("+49 30 1234567", 0.3)],
      1000,
      500,
    );
    expect(summarizeFindings(f)).toEqual([
      ["email", 2],
      ["phone", 1],
    ]);
  });
});

import { autoRedact, snapDrawn } from "./editor-ocr";
import type { Annotation } from "./editor-geometry";

const ids = () => {
  let n = 0;
  return () => `t${++n}`;
};

describe("snapDrawn", () => {
  const lines = [line("one two three", 0.1), line("four five six", 0.3)];
  const hl: Annotation = { id: "a1", type: "highlight", x: 120, y: 40, w: 200, h: 130, color: "#facc15" };

  it("turns a highlight over words into one tight box per line, keeping its style", () => {
    const out = snapDrawn(hl, lines, 1000, 500, ids());
    expect(out).toHaveLength(2);
    expect(out.every((a) => a.type === "highlight" && a.color === "#facc15")).toBe(true);
    expect(out.map((a) => a.id)).toEqual(["t1", "t2"]);
  });

  it("snaps a redaction too", () => {
    const rd: Annotation = { id: "a1", type: "redact", x: 120, y: 40, w: 200, h: 40 };
    expect(snapDrawn(rd, lines, 1000, 500, ids())).toHaveLength(1);
  });

  it("keeps other shapes and drags over no word unchanged", () => {
    const rect: Annotation = { id: "a1", type: "rect", x: 120, y: 40, w: 200, h: 130, color: "#f00", width: 4, opacity: 1, shadow: false };
    expect(snapDrawn(rect, lines, 1000, 500, ids())).toEqual([rect]);
    const empty: Annotation = { ...hl, x: 600, y: 300, w: 50, h: 50 };
    expect(snapDrawn(empty, lines, 1000, 500, ids())).toEqual([empty]);
  });
});

describe("autoRedact", () => {
  const lines = [line("Mail an ana@x.de jetzt"), line("Tel. +49 30 1234567", 0.3)];

  it("adds one redaction per finding and reports what it found", () => {
    const r = autoRedact([], lines, 1000, 500, ids());
    expect(r.added.map((a) => a.type)).toEqual(["redact", "redact"]);
    expect(r.findings.map((f) => f.kind)).toEqual(["email", "phone"]);
    expect(r.alreadyCovered).toBe(0);
    expect(r.fresh).toEqual(r.findings);
  });

  it("adds nothing for findings an existing redaction already covers", () => {
    const first = autoRedact([], lines, 1000, 500, ids());
    const again = autoRedact(first.added, lines, 1000, 500, ids());
    expect(again.added).toEqual([]);
    expect(again.alreadyCovered).toBe(2);
    expect(again.fresh).toEqual([]);
  });

  it("does not count a highlight or a blur as covering", () => {
    const first = autoRedact([], lines, 1000, 500, ids());
    const asHighlights = first.added.map((a) => ({ ...a, type: "highlight", color: "#facc15" }) as Annotation);
    expect(autoRedact(asHighlights, lines, 1000, 500, ids()).added).toHaveLength(2);
  });
});
