// Faker search-bar command — pure parser + output formatters (v0.84.270).
//
// Parsing is 100% frontend (no IPC per keystroke, like calc.ts); generation is
// one IPC call. The Rust registry is the single source of truth for generators
// — this file only classifies the command grammar and formats the raw values
// Rust returns. Grammar (argument order is irrelevant):
//
//   faker                          → catalog
//   faker <gen>                    → default-count values
//   faker <gen> <n>                → n values
//   faker <gen> <n> --json|csv|sql[=t]|ts
//   faker <gen> @de                → locale override
//   faker <gen> --seed=42          → reproducible
//   faker int 1..100               → range arg
//   faker tpl "<template>" [n]     → free template

export type FakerFormat = "plain" | "json" | "csv" | "sql" | "ts";

export interface CatalogEntry {
  name: string;
  aliases: string[];
  category: string;
  description: string;
  supported_locales: string[];
  sample: string;
  composite: boolean;
  numeric: boolean;
  fields: string[];
}

export interface FakerDefaults {
  locale: string;
  count: number;
  format: FakerFormat;
  pinned: string[];
  save_history: boolean;
}

export interface FakerSpec {
  mode: "generate" | "template";
  generator: string; // canonical name, or "tpl"
  n: number;
  format: FakerFormat;
  locale?: string; // code, only when @flag given
  seed?: number;
  args?: string; // range / date-fmt
  template?: string;
  sqlTable?: string;
}

export interface FakerGenResult {
  values: (string | number | boolean | Record<string, unknown>)[];
  seed: number;
  locale_used: string;
  fell_back: boolean;
  generator: string;
  fields?: string[];
}

export type FakerParse =
  | { kind: "catalog" }
  | { kind: "spec"; spec: FakerSpec }
  | { kind: "suggestion"; message: string; didYouMean?: string };

const MAX_N = 10_000;

// ── Grammar parsing ──────────────────────────────────────────────────────────

/** Tolerant integer: `1.000` / `1_000` → 1000; `""`/NaN → null. */
export function parseCount(tok: string): number | null {
  const cleaned = tok.replace(/[._](?=\d)/g, "");
  if (!/^\d+$/.test(cleaned)) return null;
  const n = parseInt(cleaned, 10);
  return Number.isFinite(n) ? n : null;
}

function isRange(tok: string): boolean {
  return /^-?\d+(\.\d+)?\.\.=?-?\d+(\.\d+)?$/.test(tok);
}

/**
 * Parse the argument string that follows `faker`/`fake` (the keyword is matched
 * upstream by commands.ts). `arg` is everything after the keyword.
 */
export function parseFakerCommand(
  arg: string,
  catalog: CatalogEntry[],
  defaults: FakerDefaults,
): FakerParse {
  const trimmed = arg.trim();
  if (trimmed === "") return { kind: "catalog" };

  // Template mode: `tpl "<template>" [n]`.
  const tplMatch = /^tpl\b\s*(.*)$/is.exec(trimmed);
  if (tplMatch) return parseTemplate(tplMatch[1], defaults);

  const tokens = trimmed.split(/\s+/).filter(Boolean);
  let generator: string | undefined;
  let n: number | undefined;
  let format: FakerFormat = defaults.format;
  let locale: string | undefined;
  let seed: number | undefined;
  let args: string | undefined;
  let sqlTable: string | undefined;

  for (const tok of tokens) {
    const lower = tok.toLowerCase();
    if (tok.startsWith("@")) {
      locale = tok.slice(1).toUpperCase();
    } else if (lower === "--json" || lower === "--csv" || lower === "--ts" || lower === "--plain") {
      format = lower.slice(2) as FakerFormat;
    } else if (lower === "--sql" || lower.startsWith("--sql=")) {
      format = "sql";
      const eq = tok.indexOf("=");
      if (eq >= 0) sqlTable = tok.slice(eq + 1);
    } else if (lower.startsWith("--seed=")) {
      const v = parseCount(tok.slice(7));
      if (v != null) seed = v;
    } else if (isRange(tok) || tok.startsWith("%")) {
      args = tok;
    } else if (n === undefined && parseCount(tok) != null) {
      n = parseCount(tok)!;
    } else if (generator === undefined) {
      generator = lower;
    } else if (args === undefined) {
      args = tok; // an extra bare token becomes the generator arg
    }
  }

  if (generator === undefined) {
    return { kind: "suggestion", message: "Type a generator name (e.g. email, person, uuid)" };
  }

  const entry = resolveGenerator(generator, catalog);
  if (!entry) {
    const dym = fuzzyBest(generator, catalog);
    return {
      kind: "suggestion",
      message: `Unknown generator '${generator}'`,
      didYouMean: dym?.name,
    };
  }

  const count = clampN(n ?? defaults.count);
  return {
    kind: "spec",
    spec: {
      mode: "generate",
      generator: entry.name,
      n: count,
      format,
      locale,
      seed,
      args,
      sqlTable: format === "sql" ? (sqlTable ?? entry.name) : undefined,
    },
  };
}

function parseTemplate(rest: string, defaults: FakerDefaults): FakerParse {
  // `"<template>" [n]` — the template is a quoted string, optional trailing n.
  const q = /^["'](.*)["']\s*(\d[\d._]*)?\s*$/s.exec(rest.trim());
  let template: string;
  let n: number | undefined;
  if (q) {
    template = q[1];
    if (q[2]) n = parseCount(q[2]) ?? undefined;
  } else {
    // Unquoted fallback: treat the whole rest as the template.
    template = rest.trim();
  }
  if (template === "") {
    return { kind: "suggestion", message: 'Template: faker tpl "{name} <{email}>" 10' };
  }
  return {
    kind: "spec",
    spec: { mode: "template", generator: "tpl", n: clampN(n ?? defaults.count), format: "plain", template },
  };
}

function clampN(n: number): number {
  return Math.max(1, Math.min(MAX_N, Math.floor(n)));
}

// ── Catalogue lookup + fuzzy suggestion ──────────────────────────────────────

export function resolveGenerator(name: string, catalog: CatalogEntry[]): CatalogEntry | undefined {
  const n = name.toLowerCase();
  return (
    catalog.find((g) => g.name === n) ??
    catalog.find((g) => g.aliases.some((a) => a.toLowerCase() === n))
  );
}

/** Score a catalogue entry against a needle (lower = better; null = no match). */
export function fuzzyScore(entry: CatalogEntry, needle: string): number | null {
  const q = needle.toLowerCase();
  const targets = [entry.name, ...entry.aliases];
  let best: number | null = null;
  for (const t of targets) {
    const s = scoreString(t, q);
    if (s != null && (best == null || s < best)) best = s;
  }
  // Description contains → weak match.
  if (best == null && entry.description.toLowerCase().includes(q)) best = 500;
  return best;
}

function scoreString(target: string, q: string): number | null {
  if (target === q) return -1000;
  if (target.startsWith(q)) return -500 + target.length;
  if (target.includes(q)) return -100 + target.length;
  // Typo with an extra trailing char (`plzz` → `plz`): target is a prefix of
  // the query and they're close in length.
  if (q.length >= 3 && q.startsWith(target) && q.length - target.length <= 2) {
    return -50 + q.length;
  }
  // first-char-anchored subsequence (3+ chars)
  if (q.length >= 3 && target[0] === q[0] && isSubsequence(q, target)) {
    return target.length;
  }
  return null;
}

function isSubsequence(needle: string, hay: string): boolean {
  let i = 0;
  for (const c of hay) {
    if (c === needle[i]) i++;
    if (i === needle.length) return true;
  }
  return i === needle.length;
}

export function fuzzyBest(needle: string, catalog: CatalogEntry[]): CatalogEntry | undefined {
  let best: { e: CatalogEntry; s: number } | undefined;
  for (const e of catalog) {
    const s = fuzzyScore(e, needle);
    if (s != null && (!best || s < best.s)) best = { e, s };
  }
  return best?.e;
}

/** Catalogue rows matching a partial query, best-first (for the `faker` list). */
export function matchCatalog(query: string, catalog: CatalogEntry[]): CatalogEntry[] {
  const q = query.trim().toLowerCase();
  if (q === "") return [...catalog];
  return catalog
    .map((e) => ({ e, s: fuzzyScore(e, q) }))
    .filter((x): x is { e: CatalogEntry; s: number } => x.s != null)
    .sort((a, b) => a.s - b.s)
    .map((x) => x.e);
}

// ── Output formatters (raw values → clipboard text) ──────────────────────────

export function formatValues(result: FakerGenResult, format: FakerFormat): string {
  switch (format) {
    case "json":
      return JSON.stringify(result.values, null, 2);
    case "csv":
      return toCsv(result);
    case "sql":
      return toSql(result);
    case "ts":
      return toTs(result);
    default:
      return toPlain(result);
  }
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function scalarString(v: unknown): string {
  if (v == null) return "";
  if (typeof v === "string") return v;
  return String(v);
}

function toPlain(r: FakerGenResult): string {
  if (r.fields && r.values.every(isRecord)) {
    // key: value block per record, blank line between records.
    return (r.values as Record<string, unknown>[])
      .map((rec) => r.fields!.map((f) => `${f}: ${scalarString(rec[f])}`).join("\n"))
      .join("\n\n");
  }
  return r.values.map(scalarString).join("\n");
}

function csvCell(v: unknown): string {
  const s = scalarString(v);
  if (/[",\n\r]/.test(s)) return `"${s.replace(/"/g, '""')}"`;
  return s;
}

function toCsv(r: FakerGenResult): string {
  if (r.fields && r.values.every(isRecord)) {
    const header = r.fields.map(csvCell).join(",");
    const rows = (r.values as Record<string, unknown>[]).map((rec) =>
      r.fields!.map((f) => csvCell(rec[f])).join(","),
    );
    return [header, ...rows].join("\n");
  }
  const header = csvCell(r.generator);
  const rows = r.values.map((v) => csvCell(v));
  return [header, ...rows].join("\n");
}

function sqlLiteral(v: unknown): string {
  if (typeof v === "number") return String(v);
  if (typeof v === "boolean") return v ? "TRUE" : "FALSE";
  return `'${scalarString(v).replace(/'/g, "''")}'`;
}

function toSql(r: FakerGenResult): string {
  const table = sqlIdent(r.generator);
  if (r.fields && r.values.every(isRecord)) {
    const cols = r.fields.map(sqlIdent).join(", ");
    return (r.values as Record<string, unknown>[])
      .map(
        (rec) =>
          `INSERT INTO ${table} (${cols}) VALUES (${r.fields!.map((f) => sqlLiteral(rec[f])).join(", ")});`,
      )
      .join("\n");
  }
  const col = sqlIdent(r.generator);
  return r.values
    .map((v) => `INSERT INTO ${table} (${col}) VALUES (${sqlLiteral(v)});`)
    .join("\n");
}

function sqlIdent(name: string): string {
  // Safe identifier: keep alnum/underscore, else fall back to a generic name.
  const safe = name.replace(/[^A-Za-z0-9_]/g, "_");
  return /^[A-Za-z_]/.test(safe) ? safe : `t_${safe}`;
}

function tsLiteral(v: unknown): string {
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  return JSON.stringify(scalarString(v));
}

function toTs(r: FakerGenResult): string {
  if (r.fields && r.values.every(isRecord)) {
    const objs = (r.values as Record<string, unknown>[]).map((rec) => {
      const body = r.fields!.map((f) => `  ${tsKey(f)}: ${tsLiteral(rec[f])}`).join(",\n");
      return `{\n${body},\n}`;
    });
    return `const data = [\n${objs.join(",\n")},\n];`;
  }
  const items = r.values.map((v) => tsLiteral(v)).join(", ");
  return `const data = [${items}];`;
}

function tsKey(k: string): string {
  return /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(k) ? k : JSON.stringify(k);
}
