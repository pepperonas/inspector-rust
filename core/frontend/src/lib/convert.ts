/**
 * Inline converters surfaced as `calc` rows in the search bar — unit, number
 * base, and epoch↔date conversions. `tryConvert(input)` returns a `CalcEntry`
 * (so it renders + pastes exactly like the calculator) or `null` when the
 * input isn't a conversion. Pure + unit-tested.
 *
 * Grammar (case-insensitive, `in`/`to`/`as` interchangeable):
 *   <number> <unit> in <unit>      e.g. `5 km in mi`, `72 f to c`, `2 gb in mb`
 *   <value>  in <base>             e.g. `0xff in dec`, `255 to hex`, `0b1010 in dec`
 *   <epoch>  as date               e.g. `1717000000 as date`
 */

import type { CalcEntry } from "./types";

// ── Unit tables (factor = how many base units in one of this unit) ──────────

interface Unit {
  cat: string;
  factor: number;
}

// Each category has a base unit (factor 1). Temperature is special-cased below.
const UNITS: Record<string, Unit> = {
  // length — base: metre
  mm: { cat: "len", factor: 0.001 },
  cm: { cat: "len", factor: 0.01 },
  m: { cat: "len", factor: 1 },
  km: { cat: "len", factor: 1000 },
  in: { cat: "len", factor: 0.0254 },
  inch: { cat: "len", factor: 0.0254 },
  ft: { cat: "len", factor: 0.3048 },
  yd: { cat: "len", factor: 0.9144 },
  mi: { cat: "len", factor: 1609.344 },
  // mass — base: gram
  mg: { cat: "mass", factor: 0.001 },
  g: { cat: "mass", factor: 1 },
  kg: { cat: "mass", factor: 1000 },
  t: { cat: "mass", factor: 1_000_000 },
  oz: { cat: "mass", factor: 28.349523125 },
  lb: { cat: "mass", factor: 453.59237 },
  st: { cat: "mass", factor: 6350.29318 },
  // data — base: byte (binary, 1024-based)
  b: { cat: "data", factor: 1 },
  kb: { cat: "data", factor: 1024 },
  mb: { cat: "data", factor: 1024 ** 2 },
  gb: { cat: "data", factor: 1024 ** 3 },
  tb: { cat: "data", factor: 1024 ** 4 },
  // time — base: second
  s: { cat: "time", factor: 1 },
  sec: { cat: "time", factor: 1 },
  min: { cat: "time", factor: 60 },
  h: { cat: "time", factor: 3600 },
  hr: { cat: "time", factor: 3600 },
  d: { cat: "time", factor: 86400 },
  day: { cat: "time", factor: 86400 },
  wk: { cat: "time", factor: 604800 },
  // speed — base: m/s
  mps: { cat: "speed", factor: 1 },
  kmh: { cat: "speed", factor: 1000 / 3600 },
  mph: { cat: "speed", factor: 1609.344 / 3600 },
  kn: { cat: "speed", factor: 1852 / 3600 },
};

/** Round to at most 6 significant-ish decimals, dropping trailing zeros. */
function fmtNum(n: number): string {
  if (!Number.isFinite(n)) return String(n);
  const rounded = Math.round(n * 1e6) / 1e6;
  return String(rounded);
}

function toCelsius(n: number, unit: string): number | null {
  switch (unit) {
    case "c":
      return n;
    case "f":
      return ((n - 32) * 5) / 9;
    case "k":
      return n - 273.15;
    default:
      return null;
  }
}

function fromCelsius(c: number, unit: string): number | null {
  switch (unit) {
    case "c":
      return c;
    case "f":
      return (c * 9) / 5 + 32;
    case "k":
      return c + 273.15;
    default:
      return null;
  }
}

function convertUnit(value: number, from: string, to: string): number | null {
  const TEMP = new Set(["c", "f", "k"]);
  if (TEMP.has(from) && TEMP.has(to)) {
    const c = toCelsius(value, from);
    return c === null ? null : fromCelsius(c, to);
  }
  const a = UNITS[from];
  const b = UNITS[to];
  if (!a || !b || a.cat !== b.cat) return null;
  return (value * a.factor) / b.factor;
}

// ── Number base ─────────────────────────────────────────────────────────────

function parseIntLike(token: string): number | null {
  const t = token.toLowerCase();
  let v: number;
  if (t.startsWith("0x")) v = parseInt(t.slice(2), 16);
  else if (t.startsWith("0b")) v = parseInt(t.slice(2), 2);
  else if (t.startsWith("0o")) v = parseInt(t.slice(2), 8);
  else if (/^-?\d+$/.test(t)) v = parseInt(t, 10);
  else return null;
  return Number.isFinite(v) ? v : null;
}

const BASE_ALIASES: Record<string, number> = {
  dec: 10,
  decimal: 10,
  hex: 16,
  hexadecimal: 16,
  bin: 2,
  binary: 2,
  oct: 8,
  octal: 8,
};

function formatBase(value: number, base: number): string {
  if (base === 10) return String(value);
  const prefix = base === 16 ? "0x" : base === 2 ? "0b" : base === 8 ? "0o" : "";
  const neg = value < 0 ? "-" : "";
  return neg + prefix + Math.abs(value).toString(base);
}

// ── Entry point ─────────────────────────────────────────────────────────────

export function tryConvert(input: string): CalcEntry | null {
  const q = input.trim();
  if (!q) return null;

  // <value> in <base>
  const baseM = q.match(/^(\S+)\s+(?:in|to|as)\s+([a-z]+)$/i);
  if (baseM && BASE_ALIASES[baseM[2].toLowerCase()] !== undefined) {
    const value = parseIntLike(baseM[1]);
    if (value !== null) {
      const base = BASE_ALIASES[baseM[2].toLowerCase()];
      const display = formatBase(value, base);
      return { expression: q, value, display };
    }
  }

  // <epoch seconds> as date   (10-digit unix seconds, optionally 13-digit ms)
  const epochM = q.match(/^(\d{9,13})\s+(?:as|to|in)\s+(date|iso|utc)$/i);
  if (epochM) {
    const raw = epochM[1];
    const ms = raw.length >= 13 ? Number(raw) : Number(raw) * 1000;
    const date = new Date(ms);
    if (!Number.isNaN(date.getTime())) {
      const display = date.toISOString();
      return { expression: q, value: Math.floor(ms / 1000), display };
    }
  }

  // <number> <unit> in <unit>
  const unitM = q.match(/^(-?\d+(?:\.\d+)?)\s*([a-z°]+)\s+(?:in|to)\s+([a-z°]+)$/i);
  if (unitM) {
    const value = Number(unitM[1]);
    const from = unitM[2].toLowerCase().replace("°", "");
    const to = unitM[3].toLowerCase().replace("°", "");
    const out = convertUnit(value, from, to);
    if (out !== null) {
      const display = `${fmtNum(out)} ${to}`;
      return { expression: q, value: out, display };
    }
  }

  return null;
}
