/**
 * Argument parser + suggestion logic for the `convert` / `cv` command.
 * Pure + unit-tested; the unit maths lives in `units.ts`.
 *
 *   cv                    → browse (category picker in the preview)
 *   cv 165 mph            → every speed unit, 165 mph highlighted
 *   cv 165mph in km/h     → same, km/h is the target (Enter pastes it)
 *   cv 165 m              → "m" is a complete unit (metre) — but `cv 165 mp`
 *                           is partial → autocomplete rows
 *   cv 2*60 km · cv 20€ · cv $5 · cv 1.234,5 eur · cv mph (value 1)
 *
 * The target separator (`in`, `to`, `nach`, `->`, `→`, `=`) is only taken when
 * the text before it is already a complete value + unit — so `cv 5 in` stays
 * "5 inches" while `cv 5 in in cm` converts inches to centimetres.
 */

import { tryEvaluate } from "./calc";
import {
  categoryById,
  convertBetween,
  formatValue,
  lookupUnit,
  suggestUnits,
  unitById,
  type Rates,
  type Unit,
} from "./units";

export type ConvertParse =
  | { kind: "browse" }
  | { kind: "invalid" }
  /** A number without a unit yet (`cv 165`). */
  | { kind: "value"; value: number; valueText: string }
  /** A number and an unfinished / unknown unit token (`cv 165 mp`). */
  | { kind: "partial"; value: number; valueText: string; unitText: string }
  | {
      kind: "ready";
      value: number;
      valueText: string;
      unit: Unit;
      /** The unit token as typed (for "did you mean a longer unit?" hints). */
      unitText: string;
      /** A separator was typed (`… in`), with or without a target yet. */
      targetOpen: boolean;
      /** Raw text after the separator. */
      targetText: string;
      /** Resolved target, when it is a unit of the SAME category. */
      target: Unit | null;
    };

const SEP_RE = /\s(in|to|nach|->|→|=)(?=\s|$)/gi;

export function parseConvertArg(arg: string): ConvertParse {
  const s = arg.replace(/(->|→|=)/g, " $1 ").replace(/\s+/g, " ").trim();
  if (!s) return { kind: "browse" };

  // Last separator whose left side is already a complete source wins.
  const seps = [...s.matchAll(SEP_RE)];
  for (let i = seps.length - 1; i >= 0; i--) {
    const m = seps[i];
    const left = parseSource(s.slice(0, m.index));
    if (left.kind !== "ready") continue;
    const targetText = s.slice((m.index ?? 0) + m[0].length).trim();
    const t = targetText ? lookupUnit(targetText) : null;
    return {
      ...left,
      targetOpen: true,
      targetText,
      target: t && t.cat === left.unit.cat ? t : null,
    };
  }
  return parseSource(s);
}

function parseSource(text: string): ConvertParse {
  const t = text.trim();
  if (!t) return { kind: "invalid" };

  let valueText: string;
  let unitText: string;
  const sym = t.match(/^([€$£¥₿])\s*(.*)$/);
  const num = t.match(/^([-+]?[0-9.,'\s+\-*/^()]*[0-9)])\s*(.*)$/);
  if (sym) {
    unitText = sym[1];
    valueText = sym[2].trim();
  } else if (num) {
    valueText = num[1].trim();
    unitText = num[2].trim();
  } else {
    // Unit only (`cv mph`) → one of it.
    valueText = "";
    unitText = t;
  }

  const value = valueText ? parseValue(valueText) : 1;
  if (value === null) return { kind: "invalid" };
  if (!unitText) return { kind: "value", value, valueText };
  const unit = lookupUnit(unitText);
  if (!unit) return { kind: "partial", value, valueText, unitText };
  return {
    kind: "ready", value, valueText, unit, unitText,
    targetOpen: false, targetText: "", target: null,
  };
}

/** A plain number in either convention (`1.234,5`, `1,234.5`, `2,5`, `1'000`)
 *  or an arithmetic expression (`2*60`, `(3+4)/2`). */
export function parseValue(text: string): number | null {
  const t = text.replace(/\s+/g, "").replace(/'/g, "");
  if (!t) return null;
  if (/^[-+]?[\d.,]+$/.test(t)) return parsePlainNumber(t);
  // Expression: a comma is a decimal comma here (thousands separators and
  // arithmetic don't mix in practice).
  const r = tryEvaluate("=" + t.replace(/,/g, "."));
  return r && Number.isFinite(r.value) ? r.value : null;
}

function parsePlainNumber(t: string): number | null {
  let s = t;
  const hasDot = s.includes(".");
  const hasComma = s.includes(",");
  if (hasDot && hasComma) {
    // The later one is the decimal mark.
    const dec = s.lastIndexOf(".") > s.lastIndexOf(",") ? "." : ",";
    const thou = dec === "." ? "," : ".";
    s = s.split(thou).join("").replace(dec, ".");
  } else if (hasComma) {
    if ((s.match(/,/g)?.length ?? 0) > 1) {
      if (!/^[-+]?\d{1,3}(,\d{3})+$/.test(s)) return null; // 1,234,567 only
      s = s.replace(/,/g, "");
    } else s = s.replace(",", ".");
  } else if (hasDot && (s.match(/\./g)?.length ?? 0) > 1) {
    if (!/^[-+]?\d{1,3}(\.\d{3})+$/.test(s)) return null; // 1.234.567 only
    s = s.replace(/\./g, "");
  }
  if (!/^[-+]?(\d+\.?\d*|\.\d+)$/.test(s)) return null;
  const n = Number(s);
  return Number.isFinite(n) ? n : null;
}

// ── counterpart: the "headline" unit when no target is typed ───────────────

const PAIR: Record<string, string> = {
  mph: "kmh", kmh: "mph", kn: "kmh", mps: "kmh", fps: "mps",
  mi: "km", km: "mi", in: "cm", cm: "in", ft: "m", m: "ft", yd: "m", mm: "in", nmi: "km",
  f: "c", c: "f", k: "c",
  lb: "kg", kg: "lb", oz: "g", g: "oz", st: "kg", t: "lb",
  gal: "l", l: "gal", ukgal: "l", floz: "ml", ml: "floz", cup: "ml", pt: "l", qt: "l",
  ft2: "m2", m2: "ft2", ac: "ha", ha: "ac", mi2: "km2", km2: "mi2",
  mpg: "l100km", mpguk: "l100km", l100km: "mpg", kml: "l100km",
  ps: "kw", kw: "ps", hp: "kw",
  kcal: "kj", kj: "kcal", kwh: "j", btu: "kwh",
  psi: "bar", bar: "psi", atm: "bar", hpa: "mmhg", mmhg: "hpa",
  rad: "deg", deg: "rad",
  gb: "gib", gib: "gb", mb: "mib", tb: "tib",
  h: "min", min: "s", d: "h", wk: "d", yr: "d",
  usd: "eur", eur: "usd", btc: "eur", eth: "eur", gbp: "eur", chf: "eur", jpy: "eur",
};

/** The unit a result line compares against when no target was typed. For a
 *  currency without a rate for the partner, falls back to EUR/USD. */
export function counterpart(unit: Unit, rates?: Rates): Unit {
  const want = unitById(PAIR[unit.id] ?? "") ?? unitById(categoryById(unit.cat).defaultUnit);
  const pick = want && want.id !== unit.id ? want : null;
  if (unit.cat === "currency") {
    const ok = (x: Unit | null) => !!x && x.id !== unit.id && !!rates?.[x.id.toUpperCase()];
    for (const c of [pick, unitById("eur"), unitById("usd")]) if (ok(c)) return c!;
  }
  if (pick) return pick;
  // Category default IS the source and has no pair → any other unit.
  return unitById(categoryById(unit.cat).defaultUnit === "m" ? "ft" : "m")!;
}

/** "165 mph → 265,5427 km/h" — or null when it can't be computed (a
 *  currency without rates). */
export function headline(
  p: Extract<ConvertParse, { kind: "ready" }>,
  rates?: Rates,
): { from: string; to: string; toUnit: Unit; value: number } | null {
  const toUnit = p.target ?? counterpart(p.unit, rates);
  const v = convertBetween(p.value, p.unit, toUnit, rates);
  if (v === null) return null;
  return {
    from: `${formatValue(p.value, p.unit)} ${p.unit.symbol}`,
    to: `${formatValue(v, toUnit)} ${toUnit.symbol}`,
    toUnit,
    value: v,
  };
}

export interface UnitSuggestion {
  unit: Unit;
  /** Argument text (without the keyword) Tab/Enter completes to. */
  arg: string;
  /** Live example, e.g. "Geschwindigkeit · 265,5 km/h". */
  sample: string;
}

/** Autocomplete rows for an unfinished source unit (`cv 165 mp`), an open
 *  target (`cv 165 mph in k`) — and, while the unit token may still grow, the
 *  LONGER units it is a prefix of (`cv 165 m` is metres, but `mph`/`mi`/`min`
 *  are one keystroke away). Empty when the argument is settled. */
export function convertSuggestions(p: ConvertParse, rates?: Rates, limit = 6): UnitSuggestion[] {
  const sourceRows = (unitText: string, value: number, valueText: string, exclude?: string) =>
    suggestUnits(unitText, { rates, limit: exclude ? 40 : limit, exclude }).map((unit) => {
      const other = counterpart(unit, rates);
      const v = convertBetween(value, unit, other, rates);
      const cat = categoryById(unit.cat).label;
      return {
        unit,
        arg: `${valueText ? valueText + " " : ""}${unit.id} `,
        sample: v === null ? cat : `${cat} · ${formatValue(v, other)} ${other.symbol}`,
      };
    });
  if (p.kind === "partial") return sourceRows(p.unitText, p.value, p.valueText);
  if (p.kind === "ready" && !p.targetOpen) {
    // Only LONGER spellings that start with the typed token — a settled unit
    // must not drown in fuzzy noise.
    const needle = p.unitText.toLowerCase();
    return sourceRows(p.unitText, p.value, p.valueText, p.unit.id)
      .filter((s) => [s.unit.id, ...s.unit.aliases].some((k) => k.toLowerCase().startsWith(needle)))
      .slice(0, limit);
  }
  if (p.kind === "ready" && p.targetOpen && !p.target) {
    return suggestUnits(p.targetText, { cat: p.unit.cat, rates, limit, exclude: p.unit.id }).map(
      (unit) => {
        const v = convertBetween(p.value, p.unit, unit, rates);
        return {
          unit,
          arg: `${sourceText(p)} in ${unit.id}`,
          sample: v === null ? unit.name : `${formatValue(v, unit)} ${unit.symbol}`,
        };
      },
    );
  }
  return [];
}

function sourceText(p: { valueText: string; unit: Unit }): string {
  return `${p.valueText ? p.valueText + " " : ""}${p.unit.id}`;
}

/** Canonical argument text for the panel's own controls writing back into
 *  the search bar (the query stays the single source of truth). */
export function buildArg(valueText: string, unitId: string, targetId?: string | null): string {
  const v = valueText.trim();
  return `${v ? v + " " : ""}${unitId}${targetId ? ` in ${targetId}` : ""}`;
}

/** Plain number for pasting (no thousands separators, dot decimal) — what
 *  other apps and spreadsheets parse reliably. */
export function pasteNumber(v: number, unit: Unit): string {
  if (!Number.isFinite(v)) return "";
  const digits = unit.cat === "currency" ? (unit.crypto ? 8 : 2) : 6;
  const r = Number(v.toFixed(digits));
  return String(Object.is(r, -0) ? 0 : r);
}
