import { describe, it, expect } from "vitest";
import {
  parseFakerCommand,
  parseCount,
  formatValues,
  matchCatalog,
  resolveGenerator,
  fuzzyBest,
  fuzzyScore,
  type CatalogEntry,
  type FakerDefaults,
  type FakerGenResult,
} from "./faker";

const CAT: CatalogEntry[] = [
  { name: "email", aliases: ["mail"], category: "Internet", description: "Email address", supported_locales: ["EN"], sample: "a@b.com", composite: false, numeric: false, fields: [] },
  { name: "zip", aliases: ["plz", "postcode"], category: "Address", description: "Postal code", supported_locales: ["EN", "DE_DE"], sample: "12345", composite: false, numeric: false, fields: [] },
  { name: "int", aliases: ["number"], category: "Numbers", description: "Integer", supported_locales: ["EN"], sample: "42", composite: false, numeric: true, fields: [] },
  { name: "person", aliases: ["persona"], category: "Composite", description: "Person record", supported_locales: ["EN", "DE_DE"], sample: "{…}", composite: true, numeric: false, fields: ["first_name", "email"] },
];

const DEF: FakerDefaults = { locale: "DE_DE", count: 1, format: "plain", pinned: [], save_history: true };

describe("parseCount", () => {
  it("tolerates thousands separators", () => {
    expect(parseCount("1000")).toBe(1000);
    expect(parseCount("1.000")).toBe(1000);
    expect(parseCount("1_000")).toBe(1000);
    expect(parseCount("25")).toBe(25);
  });
  it("rejects non-numbers", () => {
    expect(parseCount("abc")).toBeNull();
    expect(parseCount("")).toBeNull();
    expect(parseCount("1..5")).toBeNull();
  });
});

describe("parseFakerCommand", () => {
  it("bare faker → catalog", () => {
    expect(parseFakerCommand("", CAT, DEF)).toEqual({ kind: "catalog" });
    expect(parseFakerCommand("   ", CAT, DEF)).toEqual({ kind: "catalog" });
  });

  it("single generator uses the default count", () => {
    const r = parseFakerCommand("email", CAT, DEF);
    expect(r.kind).toBe("spec");
    if (r.kind === "spec") {
      expect(r.spec.generator).toBe("email");
      expect(r.spec.n).toBe(1);
      expect(r.spec.format).toBe("plain");
    }
  });

  it("argument order is irrelevant", () => {
    const a = parseFakerCommand("20 email @de --csv", CAT, DEF);
    const b = parseFakerCommand("email @de 20 --csv", CAT, DEF);
    const c = parseFakerCommand("--csv email 20 @de", CAT, DEF);
    expect(a).toEqual(b);
    expect(b).toEqual(c);
    if (a.kind === "spec") {
      expect(a.spec.generator).toBe("email");
      expect(a.spec.n).toBe(20);
      expect(a.spec.format).toBe("csv");
      expect(a.spec.locale).toBe("DE");
    }
  });

  it("resolves aliases", () => {
    const r = parseFakerCommand("mail", CAT, DEF);
    expect(r.kind === "spec" && r.spec.generator).toBe("email");
    const r2 = parseFakerCommand("plz 5", CAT, DEF);
    expect(r2.kind === "spec" && r2.spec.generator).toBe("zip");
  });

  it("parses --sql with and without a table", () => {
    const bare = parseFakerCommand("person 3 --sql", CAT, DEF);
    expect(bare.kind === "spec" && bare.spec.sqlTable).toBe("person");
    const named = parseFakerCommand("person 3 --sql=users", CAT, DEF);
    expect(named.kind === "spec" && named.spec.sqlTable).toBe("users");
  });

  it("parses --seed and int ranges", () => {
    const r = parseFakerCommand("int 100 1..6 --seed=42", CAT, DEF);
    if (r.kind === "spec") {
      expect(r.spec.seed).toBe(42);
      expect(r.spec.args).toBe("1..6");
      expect(r.spec.n).toBe(100);
    } else {
      throw new Error("expected spec");
    }
  });

  it("tolerates thousands separators in n", () => {
    const r = parseFakerCommand("email 1.000", CAT, DEF);
    expect(r.kind === "spec" && r.spec.n).toBe(1000);
  });

  it("unknown generator → suggestion with did-you-mean", () => {
    const r = parseFakerCommand("mai", CAT, DEF);
    expect(r.kind).toBe("suggestion");
    if (r.kind === "suggestion") expect(r.didYouMean).toBe("email");
    const r2 = parseFakerCommand("plzz", CAT, DEF);
    expect(r2.kind === "suggestion" && r2.didYouMean).toBe("zip");
  });

  it("template mode extracts the quoted template + count", () => {
    const r = parseFakerCommand('tpl "{name} <{email}>" 10', CAT, DEF);
    if (r.kind === "spec") {
      expect(r.spec.mode).toBe("template");
      expect(r.spec.template).toBe("{name} <{email}>");
      expect(r.spec.n).toBe(10);
    } else {
      throw new Error("expected spec");
    }
  });

  it("tpl without quotes treats the whole rest as the template", () => {
    // Nobody should NEED quotes for a quick one-liner — the unquoted rest
    // is the template verbatim (a trailing count only works with quotes).
    const r = parseFakerCommand("tpl {name} <{email}>", CAT, DEF);
    expect(r.kind).toBe("spec");
    if (r.kind === "spec") {
      expect(r.spec.mode).toBe("template");
      expect(r.spec.template).toBe("{name} <{email}>");
    }
  });

  it("bare tpl shows the usage hint instead of an empty template", () => {
    const r = parseFakerCommand("tpl", CAT, DEF);
    expect(r.kind).toBe("suggestion");
    if (r.kind === "suggestion") expect(r.message).toContain("faker tpl");
  });

  it("clamps n to [1, 10000]", () => {
    const big = parseFakerCommand("email 99999", CAT, DEF);
    expect(big.kind === "spec" && big.spec.n).toBe(10000);
    const zero = parseFakerCommand("email 0", CAT, DEF);
    expect(zero.kind === "spec" && zero.spec.n).toBe(1);
  });

  it("does not fire on 'fake news' — surfaces a suggestion, not a spec", () => {
    // "news" is not a generator → suggestion (never a silent generate/paste).
    const r = parseFakerCommand("news", CAT, DEF);
    expect(r.kind).toBe("suggestion");
  });
});

describe("catalogue matching", () => {
  it("matchCatalog fuzzes name/alias/description", () => {
    expect(matchCatalog("mai", CAT).map((e) => e.name)).toContain("email");
    expect(matchCatalog("plz", CAT).map((e) => e.name)).toContain("zip");
    expect(matchCatalog("", CAT).length).toBe(CAT.length);
  });
  it("resolveGenerator prefers exact name over alias", () => {
    expect(resolveGenerator("email", CAT)?.name).toBe("email");
    expect(resolveGenerator("plz", CAT)?.name).toBe("zip");
    expect(resolveGenerator("nope", CAT)).toBeUndefined();
  });
  it("fuzzyBest returns the closest generator", () => {
    expect(fuzzyBest("emai", CAT)?.name).toBe("email");
  });
});

// ── Formatters (escaping is a correctness concern) ───────────────────────────

const scalarResult = (values: (string | number | boolean)[]): FakerGenResult => ({
  values,
  seed: 1,
  locale_used: "EN",
  fell_back: false,
  generator: "name",
});

const compositeResult = (values: Record<string, unknown>[], fields: string[]): FakerGenResult => ({
  values,
  seed: 1,
  locale_used: "EN",
  fell_back: false,
  generator: "person",
  fields,
});

describe("formatters", () => {
  it("plain joins scalars by newline; composites as key: value blocks", () => {
    expect(formatValues(scalarResult(["a", "b"]), "plain")).toBe("a\nb");
    const c = compositeResult([{ first_name: "Anna", email: "a@x.com" }], ["first_name", "email"]);
    expect(formatValues(c, "plain")).toBe("first_name: Anna\nemail: a@x.com");
  });

  it("json is valid and round-trips", () => {
    const out = formatValues(scalarResult(["a", "b"]), "json");
    expect(JSON.parse(out)).toEqual(["a", "b"]);
    const c = compositeResult([{ first_name: "Müller, Anna", email: 'x"y' }], ["first_name", "email"]);
    expect(JSON.parse(formatValues(c, "json"))).toEqual([{ first_name: "Müller, Anna", email: 'x"y' }]);
  });

  it("csv escapes commas, quotes and newlines", () => {
    const c = compositeResult(
      [{ name: "Müller, Anna", note: 'say "hi"\nbye' }],
      ["name", "note"],
    );
    const out = formatValues(c, "csv");
    expect(out.split("\n")[0]).toBe("name,note");
    expect(out).toContain('"Müller, Anna"');
    expect(out).toContain('"say ""hi""\nbye"');
  });

  it("csv scalar uses the generator name as the header", () => {
    const out = formatValues(scalarResult(["a", "b,c"]), "csv");
    expect(out).toBe('name\na\n"b,c"');
  });

  it("sql quotes strings (doubling '), leaves numbers/bools bare", () => {
    const c = compositeResult(
      [{ name: "O'Brien", age: 42, active: true }],
      ["name", "age", "active"],
    );
    const out = formatValues(c, "sql");
    expect(out).toBe(
      "INSERT INTO person (name, age, active) VALUES ('O''Brien', 42, TRUE);",
    );
  });

  it("sql scalar single-column insert", () => {
    const out = formatValues(scalarResult(["O'Brien"]), "sql");
    expect(out).toBe("INSERT INTO name (name) VALUES ('O''Brien');");
  });

  it("ts emits a valid object-literal array", () => {
    const c = compositeResult([{ first_name: "Anna", n: 5 }], ["first_name", "n"]);
    const out = formatValues(c, "ts");
    expect(out).toContain("const data = [");
    expect(out).toContain('first_name: "Anna"');
    expect(out).toContain("n: 5");
  });

  it("ts scalar emits an array of literals with escaped quotes", () => {
    const out = formatValues(scalarResult(['a"b', "c"]), "ts");
    expect(out).toBe('const data = ["a\\"b", "c"];');
  });
});

describe("fuzzyScore", () => {
  const email = CAT[0]; // name "email", alias "mail"
  const zip = CAT[1]; // name "zip", aliases "plz"/"postcode"

  it("ranks an exact match best, then prefix, then infix (lower = better)", () => {
    const exact = fuzzyScore(email, "email")!;
    const prefix = fuzzyScore(email, "ema")!;
    const infix = fuzzyScore(email, "mai")!; // "mail" alias contains "mai"? prefix actually
    expect(exact).toBeLessThan(prefix);
    expect(prefix).toBeLessThan(0);
    expect(infix).toBeLessThan(0);
  });

  it("scores against the best of name + aliases", () => {
    // "plz" is an exact alias → beats a mere prefix of the name.
    expect(fuzzyScore(zip, "plz")).toBe(-1000);
    expect(fuzzyScore(zip, "postcode")).toBe(-1000);
  });

  it("matches a trailing-typo query (plzz → plz)", () => {
    // The query is one char longer than the alias but within the fuzz window.
    expect(fuzzyScore(zip, "plzz")).not.toBeNull();
    // Too far off → no match.
    expect(fuzzyScore(zip, "plzzzz")).toBeNull();
  });

  it("matches a first-char-anchored subsequence of 3+ chars", () => {
    // "pstcd" is a subsequence of "postcode" anchored on 'p'.
    expect(fuzzyScore(zip, "pstcd")).not.toBeNull();
    // "otd" is a subsequence of "postcode" but NOT anchored on 'p' and not an
    // infix → no match (the anchor guard is what stops noisy hits).
    expect(fuzzyScore(zip, "otd")).toBeNull();
  });

  it("falls back to a weak description-contains match", () => {
    // "postal" is only in the description, not the name/aliases → weak (500).
    expect(fuzzyScore(zip, "postal")).toBe(500);
  });

  it("returns null when nothing matches", () => {
    expect(fuzzyScore(email, "xyz")).toBeNull();
  });
});

describe("parseFakerCommand — arguments without a generator", () => {
  // The token loop assigns count/format/locale/seed first, so it is easy to
  // type something complete-looking that still names no generator. The row must
  // then coach rather than silently produce nothing.
  it("a bare count is not enough", () => {
    const r = parseFakerCommand("20", CAT, DEF);
    expect(r.kind).toBe("suggestion");
    if (r.kind !== "suggestion") return;
    expect(r.message).toMatch(/generator name/i);
    expect(r.didYouMean).toBeUndefined(); // nothing to guess at yet
  });

  it("a bare format flag is not enough", () => {
    expect(parseFakerCommand("--json", CAT, DEF).kind).toBe("suggestion");
    expect(parseFakerCommand("--sql=users", CAT, DEF).kind).toBe("suggestion");
  });

  it("a bare locale or seed is not enough", () => {
    expect(parseFakerCommand("@de", CAT, DEF).kind).toBe("suggestion");
    expect(parseFakerCommand("--seed=42", CAT, DEF).kind).toBe("suggestion");
  });

  it("adding the generator to the same flags produces a real spec", () => {
    const r = parseFakerCommand("20 --json email", CAT, DEF);
    expect(r.kind).toBe("spec");
  });
});

describe("formatters — non-string field values", () => {
  it("plain renders numbers, booleans, null and absent keys safely", () => {
    // Generator records come from Rust as arbitrary JSON scalars; a number must
    // print as a number and a null/missing key must be blank, never "null" or
    // "undefined".
    const c = compositeResult(
      [{ age: 42, active: true, note: null, ratio: 0 }],
      ["age", "active", "note", "ratio", "absent"],
    );
    expect(formatValues(c, "plain")).toBe("age: 42\nactive: true\nnote: \nratio: 0\nabsent: ");
  });

  it("plain keeps a blank line between records", () => {
    const c = compositeResult([{ n: 1 }, { n: 2 }], ["n"]);
    expect(formatValues(c, "plain")).toBe("n: 1\n\nn: 2");
  });
});

describe("parseFakerCommand — an extra bare token becomes the generator arg", () => {
  it("a trailing word after the generator is carried as `args`", () => {
    const r = parseFakerCommand("zip 5 extra", CAT, DEF);
    expect(r.kind).toBe("spec");
    if (r.kind !== "spec") return;
    expect(r.spec.generator).toBe("zip");
    expect(r.spec.n).toBe(5);
    expect(r.spec.args).toBe("extra");
  });

  it("only the FIRST extra token is taken; further ones are ignored", () => {
    const r = parseFakerCommand("zip first second", CAT, DEF);
    expect(r.kind).toBe("spec");
    if (r.kind !== "spec") return;
    expect(r.spec.args).toBe("first");
  });

  it("an explicit range (Rust `a..b` syntax) overrides an earlier bare token", () => {
    const r = parseFakerCommand("int extra 1..100", CAT, DEF);
    expect(r.kind).toBe("spec");
    if (r.kind !== "spec") return;
    expect(r.spec.args).toBe("1..100");
  });

  it("a hyphen range is NOT the range syntax — it is just a bare arg", () => {
    // Ranges are Rust-shaped (`1..100` / `1..=100`); `1-100` stays a plain
    // token so it can't silently be read as a range by the backend.
    const r = parseFakerCommand("int 1-100", CAT, DEF);
    expect(r.kind).toBe("spec");
    if (r.kind !== "spec") return;
    expect(r.spec.args).toBe("1-100");
  });
});
