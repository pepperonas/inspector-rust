/**
 * Unit registry for the `convert` / `cv` command — the ONE source of truth for
 * categories, units, aliases and conversion maths. Pure + unit-tested.
 *
 * Every unit converts through its category's BASE unit:
 *   - linear units carry a `factor` (how many base units one of this unit is),
 *   - the few non-linear ones carry `toBase`/`fromBase` (temperature is affine,
 *     fuel consumption is reciprocal between L/100 km and km/L or mpg).
 * Currencies are linear too, but their factor (EUR value of one unit) only
 * exists at runtime — it comes from `fx_rates` and is passed in as `rates`.
 * A currency without a rate is simply not shown; it is never treated as 0.
 */

import { locale, t, type MessageKey } from "./i18n";

export type CategoryId =
  | "length"
  | "area"
  | "volume"
  | "mass"
  | "temperature"
  | "speed"
  | "time"
  | "data"
  | "energy"
  | "power"
  | "pressure"
  | "fuel"
  | "angle"
  | "currency";

export interface Category {
  id: CategoryId;
  /** Label on the category chip, in the app language (getter). */
  readonly label: string;
  /** Unit id the preview uses when a category is picked without a unit. */
  defaultUnit: string;
}

export interface Unit {
  /** Stable id; also the canonical token written back into the search bar. */
  id: string;
  cat: CategoryId;
  /** Display symbol, e.g. `km/h`, `°C`, `m²`. */
  symbol: string;
  /** Display name in the app language (getter, catalogue key `unit.<id>`). */
  readonly name: string;
  /** Extra typable spellings (lower-case, spaces/° stripped by `normKey`). */
  aliases: string[];
  /** Linear: base units per one of this unit. */
  factor?: number;
  toBase?: (v: number) => number;
  fromBase?: (v: number) => number;
  /** Currency only: a crypto asset (8 fraction digits, CoinGecko source). */
  crypto?: boolean;
}

export const CATEGORIES: Category[] = [
  { id: "length", get label() { return t("unitcat.length"); }, defaultUnit: "m" },
  { id: "area", get label() { return t("unitcat.area"); }, defaultUnit: "m2" },
  { id: "volume", get label() { return t("unitcat.volume"); }, defaultUnit: "l" },
  { id: "mass", get label() { return t("unitcat.mass"); }, defaultUnit: "kg" },
  { id: "temperature", get label() { return t("unitcat.temperature"); }, defaultUnit: "c" },
  { id: "speed", get label() { return t("unitcat.speed"); }, defaultUnit: "kmh" },
  { id: "time", get label() { return t("unitcat.time"); }, defaultUnit: "h" },
  { id: "data", get label() { return t("unitcat.data"); }, defaultUnit: "gb" },
  { id: "energy", get label() { return t("unitcat.energy"); }, defaultUnit: "kwh" },
  { id: "power", get label() { return t("unitcat.power"); }, defaultUnit: "kw" },
  { id: "pressure", get label() { return t("unitcat.pressure"); }, defaultUnit: "bar" },
  { id: "fuel", get label() { return t("unitcat.fuel"); }, defaultUnit: "l100km" },
  { id: "angle", get label() { return t("unitcat.angle"); }, defaultUnit: "deg" },
  { id: "currency", get label() { return t("unitcat.currency"); }, defaultUnit: "eur" },
];

// Reciprocal fuel scales: value ↔ L/100 km (base). An involution — the same
// function converts both ways. 0 has no reciprocal → Infinity (formatted "—").
const recip = (k: number) => (v: number) => k / v;
const MPG_US = (100 * 3.785411784) / 1.609344; // 235.2145833…
const MPG_UK = (100 * 4.54609) / 1.609344; // 282.4809363…

/** A unit's display name in the app language (catalogue key `unit.<id>`). */
function unitName(id: string): string {
  return t(`unit.${id}` as MessageKey);
}

const u = (
  id: string,
  cat: CategoryId,
  symbol: string,
  factor: number,
  aliases: string[] = [],
): Unit => ({ id, cat, symbol, get name() { return unitName(id); }, factor, aliases });

export const UNITS: Unit[] = [
  // ── length (base: metre) ─────────────────────────────────────────────────
  u("mm", "length", "mm", 0.001, ["millimeter", "millimetre"]),
  u("cm", "length", "cm", 0.01, ["zentimeter", "centimeter", "centimetre"]),
  u("m", "length", "m", 1, ["meter", "metre", "metres", "meters"]),
  u("km", "length", "km", 1000, ["kilometer", "kilometre", "kilometres"]),
  u("in", "length", "in", 0.0254, ["inch", "inches", "zoll", '"']),
  u("ft", "length", "ft", 0.3048, ["foot", "feet", "fuss", "fuß", "'"]),
  u("yd", "length", "yd", 0.9144, ["yard", "yards"]),
  u("mi", "length", "mi", 1609.344, ["mile", "miles", "meile", "meilen"]),
  u("nmi", "length", "sm", 1852, ["seemeile", "seemeilen", "nauticalmile", "sm"]),

  // ── area (base: m²) ──────────────────────────────────────────────────────
  u("cm2", "area", "cm²", 1e-4, ["qcm", "sqcm"]),
  u("m2", "area", "m²", 1, ["qm", "sqm", "quadratmeter"]),
  u("ha", "area", "ha", 1e4, ["hektar", "hectare", "hectares"]),
  u("km2", "area", "km²", 1e6, ["qkm", "sqkm"]),
  u("in2", "area", "in²", 0.00064516, ["sqin"]),
  u("ft2", "area", "ft²", 0.09290304, ["sqft"]),
  u("yd2", "area", "yd²", 0.83612736, ["sqyd"]),
  u("ac", "area", "ac", 4046.8564224, ["acre", "acres"]),
  u("mi2", "area", "mi²", 2589988.110336, ["sqmi"]),

  // ── volume (base: litre) ─────────────────────────────────────────────────
  u("ml", "volume", "ml", 0.001, ["milliliter", "millilitre"]),
  u("cl", "volume", "cl", 0.01, ["zentiliter", "centiliter"]),
  u("dl", "volume", "dl", 0.1, ["deziliter", "deciliter"]),
  u("l", "volume", "l", 1, ["liter", "litre", "liters", "litres", "ltr"]),
  u("m3", "volume", "m³", 1000, ["cbm", "kubikmeter"]),
  u("tsp", "volume", "TL", 0.00492892159375, ["tl", "teelöffel", "teaspoon"]),
  u("tbsp", "volume", "EL", 0.01478676478125, ["el", "esslöffel", "tablespoon"]),
  u("floz", "volume", "fl oz", 0.0295735295625, ["fluidounce", "usfloz"]),
  u("cup", "volume", "cup", 0.2365882365, ["cups", "tasse", "tassen"]),
  u("pt", "volume", "pt", 0.473176473, ["pint", "pints", "uspt"]),
  u("ukpt", "volume", "pt (UK)", 0.56826125, ["impt", "imperialpint"]),
  u("qt", "volume", "qt", 0.946352946, ["quart", "quarts"]),
  u("gal", "volume", "gal", 3.785411784, ["gallon", "gallons", "gallone", "usgal"]),
  u("ukgal", "volume", "gal (UK)", 4.54609, ["impgal", "imperialgallon"]),

  // ── mass (base: gram) ────────────────────────────────────────────────────
  u("mg", "mass", "mg", 0.001, ["milligramm", "milligram"]),
  u("g", "mass", "g", 1, ["gramm", "gram", "grams"]),
  u("kg", "mass", "kg", 1000, ["kilo", "kilogramm", "kilogram", "kilos"]),
  u("t", "mass", "t", 1e6, ["tonne", "tonnen", "ton"]),
  u("oz", "mass", "oz", 28.349523125, ["ounce", "ounces", "unze", "unzen"]),
  u("lb", "mass", "lb", 453.59237, ["lbs", "pound", "pounds"]),
  u("st", "mass", "st", 6350.29318, ["stone"]),
  u("ct", "mass", "ct", 0.2, ["karat", "carat"]),

  // ── temperature (base: °C, affine) ───────────────────────────────────────
  {
    id: "c", cat: "temperature", symbol: "°C", get name() { return unitName("c"); },
    aliases: ["celsius"], toBase: (v) => v, fromBase: (v) => v,
  },
  {
    id: "f", cat: "temperature", symbol: "°F", get name() { return unitName("f"); },
    aliases: ["fahrenheit"], toBase: (v) => ((v - 32) * 5) / 9, fromBase: (v) => (v * 9) / 5 + 32,
  },
  {
    id: "k", cat: "temperature", symbol: "K", get name() { return unitName("k"); },
    aliases: ["kelvin"], toBase: (v) => v - 273.15, fromBase: (v) => v + 273.15,
  },

  // ── speed (base: m/s) ────────────────────────────────────────────────────
  u("kmh", "speed", "km/h", 1000 / 3600, ["kph", "kmph", "kmproh", "kmpro"]),
  u("mph", "speed", "mph", 1609.344 / 3600, ["mi/h", "milesperhour"]),
  u("mps", "speed", "m/s", 1, ["ms-1", "meterprosekunde"]),
  u("kn", "speed", "kn", 1852 / 3600, ["knot", "knots", "knoten", "kt"]),
  u("fps", "speed", "ft/s", 0.3048, ["ft/s", "feetpersecond"]),

  // ── time (base: second) ──────────────────────────────────────────────────
  u("ms", "time", "ms", 0.001, ["millisekunde", "millisekunden", "millisecond", "milliseconds"]),
  u("s", "time", "s", 1, ["sec", "sekunde", "sekunden", "second", "seconds"]),
  u("min", "time", "min", 60, ["minute", "minuten", "minutes", "mins"]),
  u("h", "time", "h", 3600, ["hr", "hrs", "std", "stunde", "stunden", "hour", "hours"]),
  u("d", "time", "d", 86400, ["day", "days", "tag", "tage"]),
  u("wk", "time", "Wo", 604800, ["week", "weeks", "woche", "wochen"]),
  u("mo", "time", "Mon", 2629746, ["month", "months", "monat", "monate"]),
  u("yr", "time", "a", 31556952, ["year", "years", "jahr", "jahre", "y"]),

  // ── data (base: byte; SI decimal AND IEC binary) ─────────────────────────
  u("bit", "data", "bit", 0.125, ["bits"]),
  u("b", "data", "B", 1, ["byte", "bytes"]),
  u("kb", "data", "kB", 1e3, ["kilobyte"]),
  u("mb", "data", "MB", 1e6, ["megabyte"]),
  u("gb", "data", "GB", 1e9, ["gigabyte", "gig"]),
  u("tb", "data", "TB", 1e12, ["terabyte"]),
  u("pb", "data", "PB", 1e15, ["petabyte"]),
  u("kib", "data", "KiB", 1024, ["kibibyte"]),
  u("mib", "data", "MiB", 1024 ** 2, ["mebibyte"]),
  u("gib", "data", "GiB", 1024 ** 3, ["gibibyte"]),
  u("tib", "data", "TiB", 1024 ** 4, ["tebibyte"]),

  // ── energy (base: joule) ─────────────────────────────────────────────────
  u("j", "energy", "J", 1, ["joule"]),
  u("kj", "energy", "kJ", 1e3, ["kilojoule"]),
  u("cal", "energy", "cal", 4.184, ["kalorie", "calorie"]),
  u("kcal", "energy", "kcal", 4184, ["kilokalorie", "kalorien", "calories"]),
  u("wh", "energy", "Wh", 3600, ["wattstunde"]),
  u("kwh", "energy", "kWh", 3.6e6, ["kilowattstunde", "kilowatthour"]),
  u("btu", "energy", "BTU", 1055.05585262, []),

  // ── power (base: watt) ───────────────────────────────────────────────────
  u("w", "power", "W", 1, ["watt"]),
  u("kw", "power", "kW", 1e3, ["kilowatt"]),
  u("mw", "power", "MW", 1e6, ["megawatt"]),
  u("ps", "power", "PS", 735.49875, ["pferdestärke", "pferdestaerke"]),
  u("hp", "power", "hp", 550 * 0.3048 * 4.4482216152605, ["horsepower", "bhp"]), // 550 ft·lbf/s

  // ── pressure (base: pascal) ──────────────────────────────────────────────
  u("pa", "pressure", "Pa", 1, ["pascal"]),
  u("hpa", "pressure", "hPa", 100, ["hektopascal"]),
  u("kpa", "pressure", "kPa", 1e3, ["kilopascal"]),
  u("mbar", "pressure", "mbar", 100, ["millibar"]),
  u("bar", "pressure", "bar", 1e5, []),
  u("psi", "pressure", "psi", 6894.757293168, []),
  u("atm", "pressure", "atm", 101325, ["atmosphäre", "atmosphere"]),
  u("mmhg", "pressure", "mmHg", 133.322387415, ["torr"]),

  // ── fuel consumption (base: L/100 km, reciprocal scales) ─────────────────
  {
    id: "l100km", cat: "fuel", symbol: "L/100 km", get name() { return unitName("l100km"); },
    aliases: ["l/100km", "lpro100km", "l100"], toBase: (v) => v, fromBase: (v) => v,
  },
  {
    id: "kml", cat: "fuel", symbol: "km/L", get name() { return unitName("kml"); },
    aliases: ["km/l", "kmpl"], toBase: recip(100), fromBase: recip(100),
  },
  {
    id: "mpg", cat: "fuel", symbol: "mpg (US)", get name() { return unitName("mpg"); },
    aliases: ["mpgus", "usmpg"], toBase: recip(MPG_US), fromBase: recip(MPG_US),
  },
  {
    id: "mpguk", cat: "fuel", symbol: "mpg (UK)", get name() { return unitName("mpguk"); },
    aliases: ["ukmpg", "mpgimp"], toBase: recip(MPG_UK), fromBase: recip(MPG_UK),
  },

  // ── angle (base: degree) ─────────────────────────────────────────────────
  u("deg", "angle", "°", 1, ["°", "degree", "degrees"]),
  u("rad", "angle", "rad", 180 / Math.PI, ["radiant", "radian", "radians"]),
  u("grad", "angle", "gon", 0.9, ["gon", "gradian"]),
  u("turn", "angle", "U", 360, ["umdrehung", "umdrehungen", "turns", "rev"]),
  u("arcmin", "angle", "′", 1 / 60, ["bogenminute"]),
  u("arcsec", "angle", "″", 1 / 3600, ["bogensekunde"]),
];

// ── currencies (factor = EUR value of one unit, supplied at runtime) ────────

interface CurrencyDef {
  code: string;
  symbol?: string;
  aliases?: string[];
  crypto?: boolean;
}

/** Order = preview order: the everyday ones first. Codes match the ECB set
 *  frankfurter serves; BTC/ETH come from CoinGecko. */
const CURRENCIES: CurrencyDef[] = [
  { code: "EUR", symbol: "€", aliases: ["euro", "euros"] },
  { code: "USD", symbol: "$", aliases: ["dollar", "dollars", "usdollar"] },
  { code: "GBP", symbol: "£", aliases: ["pfundsterling", "sterling", "quid"] },
  { code: "CHF", aliases: ["franken", "franc", "francs"] },
  { code: "JPY", symbol: "¥", aliases: ["yen"] },
  { code: "BTC", symbol: "₿", aliases: ["bitcoin", "xbt"], crypto: true },
  { code: "ETH", aliases: ["ether", "ethereum"], crypto: true },
  { code: "CNY", aliases: ["yuan", "renminbi", "rmb"] },
  { code: "CAD" },
  { code: "AUD" },
  { code: "NZD" },
  { code: "SEK" },
  { code: "NOK" },
  { code: "DKK" },
  { code: "PLN", aliases: ["zloty", "złoty"] },
  { code: "CZK" },
  { code: "HUF", aliases: ["forint"] },
  { code: "RON" },
  { code: "ISK" },
  { code: "TRY", aliases: ["lira"] },
  { code: "HKD" },
  { code: "SGD" },
  { code: "KRW", aliases: ["won"] },
  { code: "INR", aliases: ["rupie", "rupee", "rupees"] },
  { code: "IDR", aliases: ["rupiah"] },
  { code: "MYR", aliases: ["ringgit"] },
  { code: "PHP" },
  { code: "THB", aliases: ["baht"] },
  { code: "BRL", aliases: ["real", "reais"] },
  { code: "MXN" },
  { code: "ZAR", aliases: ["rand"] },
  { code: "ILS", aliases: ["schekel", "shekel"] },
];

for (const c of CURRENCIES) {
  UNITS.push({
    id: c.code.toLowerCase(),
    cat: "currency",
    symbol: c.code,
    get name() { return unitName(c.code.toLowerCase()); },
    aliases: [...(c.symbol ? [c.symbol] : []), ...(c.aliases ?? [])],
    crypto: c.crypto,
  });
}

/** EUR value of ONE unit of each currency, keyed by upper-case code
 *  (`{ EUR: 1, USD: 0.92, BTC: 58000 }`). Built from `fx_rates`. */
export type Rates = Record<string, number>;

// ── lookup ──────────────────────────────────────────────────────────────────

/** Normalise a typed unit token: lower-case, no spaces, superscripts → digits,
 *  a leading `°` dropped (`°C` → `c`) — but a lone `°` stays (degrees). */
export function normKey(token: string): string {
  let k = token.trim().toLowerCase().replace(/\s+/g, "").replace(/²/g, "2").replace(/³/g, "3");
  if (k.length > 1 && k.startsWith("°")) k = k.slice(1);
  return k;
}

const BY_KEY = new Map<string, Unit>();
for (const unit of UNITS) {
  for (const key of [unit.id, normKey(unit.symbol), ...unit.aliases.map(normKey)]) {
    // First registration wins — `unitKeyCollisions()` (tested) keeps the
    // registry free of ambiguous spellings, so this order never decides.
    if (!BY_KEY.has(key)) BY_KEY.set(key, unit);
  }
}

/** Every spelling that maps to more than one unit (the test demands none). */
export function unitKeyCollisions(): string[] {
  const owner = new Map<string, string>();
  const out = new Set<string>();
  for (const unit of UNITS) {
    const keys = new Set([unit.id, normKey(unit.symbol), ...unit.aliases.map(normKey)]);
    for (const key of keys) {
      const prev = owner.get(key);
      if (prev && prev !== unit.id) out.add(key);
      else owner.set(key, unit.id);
    }
  }
  return [...out];
}

export function lookupUnit(token: string): Unit | null {
  return BY_KEY.get(normKey(token)) ?? null;
}

export function unitById(id: string): Unit | null {
  return UNITS.find((x) => x.id === id) ?? null;
}

export function categoryById(id: CategoryId): Category {
  return CATEGORIES.find((c) => c.id === id)!;
}

export function unitsOf(cat: CategoryId, rates?: Rates): Unit[] {
  return UNITS.filter((x) => x.cat === cat && (x.cat !== "currency" || hasRate(x, rates)));
}

function hasRate(unit: Unit, rates?: Rates): boolean {
  const r = rates?.[unit.id.toUpperCase()];
  return typeof r === "number" && Number.isFinite(r) && r > 0;
}

/** Score one spelling against a partial token: prefix beats a word-start hit
 *  beats an anchored subsequence (3+ chars). Lower is better; null = no match. */
function keyScore(key: string, needle: string): number | null {
  if (key === needle) return -10000;
  if (key.startsWith(needle)) return -5000 + key.length;
  if (needle.length < 3 || key[0] !== needle[0]) return null;
  let k = 1;
  let prev = 0;
  let gaps = 0;
  for (let n = 1; n < needle.length; n++) {
    let found = -1;
    while (k < key.length) {
      if (key[k] === needle[n]) {
        found = k;
        k++;
        break;
      }
      k++;
    }
    if (found === -1) return null;
    gaps += found - prev - 1;
    prev = found;
  }
  return gaps * 3 + key.length;
}

/** Autocomplete candidates for a partial unit token, best first. `cat`
 *  restricts to one category (the target of `… in <partial>`). Currencies
 *  without a rate are left out when `rates` is given. */
export function suggestUnits(
  partial: string,
  opts: { cat?: CategoryId; rates?: Rates; limit?: number; exclude?: string } = {},
): Unit[] {
  const needle = normKey(partial);
  const limit = opts.limit ?? 8;
  const scored: { unit: Unit; score: number; order: number }[] = [];
  UNITS.forEach((unit, order) => {
    if (opts.cat && unit.cat !== opts.cat) return;
    if (opts.exclude && unit.id === opts.exclude) return;
    if (unit.cat === "currency" && opts.rates && !hasRate(unit, opts.rates)) return;
    if (!needle) {
      scored.push({ unit, score: 0, order });
      return;
    }
    let best: number | null = null;
    for (const key of [unit.id, normKey(unit.symbol), ...unit.aliases.map(normKey)]) {
      const s = keyScore(key, needle);
      if (s !== null && (best === null || s < best)) best = s;
    }
    if (best !== null) scored.push({ unit, score: best, order });
  });
  scored.sort((a, b) => a.score - b.score || a.order - b.order);
  return scored.slice(0, limit).map((s) => s.unit);
}

// ── conversion ──────────────────────────────────────────────────────────────

function toBaseValue(value: number, unit: Unit, rates?: Rates): number | null {
  if (unit.cat === "currency") {
    return hasRate(unit, rates) ? value * rates![unit.id.toUpperCase()] : null;
  }
  if (unit.toBase) return unit.toBase(value);
  return value * (unit.factor as number);
}

function fromBaseValue(base: number, unit: Unit, rates?: Rates): number | null {
  if (unit.cat === "currency") {
    return hasRate(unit, rates) ? base / rates![unit.id.toUpperCase()] : null;
  }
  if (unit.fromBase) return unit.fromBase(base);
  return base / (unit.factor as number);
}

/** Convert between two units of the same category; null across categories
 *  or when a currency has no rate. */
export function convertBetween(value: number, from: Unit, to: Unit, rates?: Rates): number | null {
  if (from.cat !== to.cat) return null;
  if (from.id === to.id) return value;
  const base = toBaseValue(value, from, rates);
  if (base === null) return null;
  return fromBaseValue(base, to, rates);
}

export interface ConvertRow {
  unit: Unit;
  value: number;
  /** German-formatted number, without the symbol. */
  text: string;
}

/** Every unit of `from`'s category, in registry order (the everyday units
 *  first). Units that can't be computed (currency without a rate) drop out. */
export function convertAll(value: number, from: Unit, rates?: Rates): ConvertRow[] {
  const rows: ConvertRow[] = [];
  for (const unit of unitsOf(from.cat, from.cat === "currency" ? rates : undefined)) {
    const v = convertBetween(value, from, unit, rates);
    if (v === null) continue;
    rows.push({ unit, value: v, text: formatValue(v, unit) });
  }
  return rows;
}

// ── formatting ──────────────────────────────────────────────────────────────

// Formatters per app language, built once each (Intl constructors are slow).
const FORMATTERS = new Map<string, { sig: Intl.NumberFormat; money: Intl.NumberFormat; crypto: Intl.NumberFormat }>();
function formatters() {
  const loc = locale();
  let f = FORMATTERS.get(loc);
  if (!f) {
    f = {
      sig: new Intl.NumberFormat(loc, { maximumSignificantDigits: 7 }),
      money: new Intl.NumberFormat(loc, { minimumFractionDigits: 2, maximumFractionDigits: 2 }),
      crypto: new Intl.NumberFormat(loc, { maximumSignificantDigits: 8 }),
    };
    FORMATTERS.set(loc, f);
  }
  return f;
}

/** Number for a unit, formatted in the app language: money with two decimals, crypto with
 *  up to eight significant digits, everything else with seven significant
 *  digits; scientific notation only at the extremes; never "-0" or "NaN". */
export function formatValue(v: number, unit?: Unit): string {
  if (!Number.isFinite(v)) return "—";
  if (Object.is(v, -0) || v === 0) v = 0;
  const abs = Math.abs(v);
  const { sig, money, crypto } = formatters();
  if (unit?.cat === "currency") {
    if (unit.crypto) return abs !== 0 && abs < 1e-8 ? sci(v) : crypto.format(v);
    return abs !== 0 && abs < 0.005 ? crypto.format(v) : money.format(v);
  }
  if (abs !== 0 && (abs >= 1e15 || abs < 1e-6)) return sci(v);
  const out = sig.format(v);
  return out === "-0" ? "0" : out;
}

const SUP: Record<string, string> = {
  "-": "⁻", "0": "⁰", "1": "¹", "2": "²", "3": "³", "4": "⁴",
  "5": "⁵", "6": "⁶", "7": "⁷", "8": "⁸", "9": "⁹",
};

function sci(v: number): string {
  const [mant, exp] = v.toExponential(4).split("e");
  const m = new Intl.NumberFormat(locale(), { maximumFractionDigits: 4 }).format(Number(mant));
  const e = String(Number(exp))
    .split("")
    .map((ch) => SUP[ch] ?? ch)
    .join("");
  return `${m}·10${e}`;
}
