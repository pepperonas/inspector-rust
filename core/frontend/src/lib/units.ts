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
  /** German label shown on the category chip. */
  label: string;
  /** Unit id the preview uses when a category is picked without a unit. */
  defaultUnit: string;
}

export interface Unit {
  /** Stable id; also the canonical token written back into the search bar. */
  id: string;
  cat: CategoryId;
  /** Display symbol, e.g. `km/h`, `°C`, `m²`. */
  symbol: string;
  /** German display name. */
  name: string;
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
  { id: "length", label: "Länge", defaultUnit: "m" },
  { id: "area", label: "Fläche", defaultUnit: "m2" },
  { id: "volume", label: "Volumen", defaultUnit: "l" },
  { id: "mass", label: "Masse", defaultUnit: "kg" },
  { id: "temperature", label: "Temperatur", defaultUnit: "c" },
  { id: "speed", label: "Geschwindigkeit", defaultUnit: "kmh" },
  { id: "time", label: "Zeit", defaultUnit: "h" },
  { id: "data", label: "Daten", defaultUnit: "gb" },
  { id: "energy", label: "Energie", defaultUnit: "kwh" },
  { id: "power", label: "Leistung", defaultUnit: "kw" },
  { id: "pressure", label: "Druck", defaultUnit: "bar" },
  { id: "fuel", label: "Verbrauch", defaultUnit: "l100km" },
  { id: "angle", label: "Winkel", defaultUnit: "deg" },
  { id: "currency", label: "Währung", defaultUnit: "eur" },
];

// Reciprocal fuel scales: value ↔ L/100 km (base). An involution — the same
// function converts both ways. 0 has no reciprocal → Infinity (formatted "—").
const recip = (k: number) => (v: number) => k / v;
const MPG_US = (100 * 3.785411784) / 1.609344; // 235.2145833…
const MPG_UK = (100 * 4.54609) / 1.609344; // 282.4809363…

const u = (
  id: string,
  cat: CategoryId,
  symbol: string,
  name: string,
  factor: number,
  aliases: string[] = [],
): Unit => ({ id, cat, symbol, name, factor, aliases });

export const UNITS: Unit[] = [
  // ── length (base: metre) ─────────────────────────────────────────────────
  u("mm", "length", "mm", "Millimeter", 0.001, ["millimeter", "millimetre"]),
  u("cm", "length", "cm", "Zentimeter", 0.01, ["zentimeter", "centimeter", "centimetre"]),
  u("m", "length", "m", "Meter", 1, ["meter", "metre", "metres", "meters"]),
  u("km", "length", "km", "Kilometer", 1000, ["kilometer", "kilometre", "kilometres"]),
  u("in", "length", "in", "Zoll", 0.0254, ["inch", "inches", "zoll", '"']),
  u("ft", "length", "ft", "Fuß", 0.3048, ["foot", "feet", "fuss", "fuß", "'"]),
  u("yd", "length", "yd", "Yard", 0.9144, ["yard", "yards"]),
  u("mi", "length", "mi", "Meile", 1609.344, ["mile", "miles", "meile", "meilen"]),
  u("nmi", "length", "sm", "Seemeile", 1852, ["seemeile", "seemeilen", "nauticalmile", "sm"]),

  // ── area (base: m²) ──────────────────────────────────────────────────────
  u("cm2", "area", "cm²", "Quadratzentimeter", 1e-4, ["qcm", "sqcm"]),
  u("m2", "area", "m²", "Quadratmeter", 1, ["qm", "sqm", "quadratmeter"]),
  u("ha", "area", "ha", "Hektar", 1e4, ["hektar", "hectare", "hectares"]),
  u("km2", "area", "km²", "Quadratkilometer", 1e6, ["qkm", "sqkm"]),
  u("in2", "area", "in²", "Quadratzoll", 0.00064516, ["sqin"]),
  u("ft2", "area", "ft²", "Quadratfuß", 0.09290304, ["sqft"]),
  u("yd2", "area", "yd²", "Quadratyard", 0.83612736, ["sqyd"]),
  u("ac", "area", "ac", "Acre", 4046.8564224, ["acre", "acres"]),
  u("mi2", "area", "mi²", "Quadratmeile", 2589988.110336, ["sqmi"]),

  // ── volume (base: litre) ─────────────────────────────────────────────────
  u("ml", "volume", "ml", "Milliliter", 0.001, ["milliliter", "millilitre"]),
  u("cl", "volume", "cl", "Zentiliter", 0.01, ["zentiliter", "centiliter"]),
  u("dl", "volume", "dl", "Deziliter", 0.1, ["deziliter", "deciliter"]),
  u("l", "volume", "l", "Liter", 1, ["liter", "litre", "liters", "litres", "ltr"]),
  u("m3", "volume", "m³", "Kubikmeter", 1000, ["cbm", "kubikmeter"]),
  u("tsp", "volume", "TL", "Teelöffel (US)", 0.00492892159375, ["tl", "teelöffel", "teaspoon"]),
  u("tbsp", "volume", "EL", "Esslöffel (US)", 0.01478676478125, ["el", "esslöffel", "tablespoon"]),
  u("floz", "volume", "fl oz", "Flüssigunze (US)", 0.0295735295625, ["fluidounce", "usfloz"]),
  u("cup", "volume", "cup", "Tasse (US)", 0.2365882365, ["cups", "tasse", "tassen"]),
  u("pt", "volume", "pt", "Pint (US)", 0.473176473, ["pint", "pints", "uspt"]),
  u("ukpt", "volume", "pt (UK)", "Pint (UK)", 0.56826125, ["impt", "imperialpint"]),
  u("qt", "volume", "qt", "Quart (US)", 0.946352946, ["quart", "quarts"]),
  u("gal", "volume", "gal", "Gallone (US)", 3.785411784, ["gallon", "gallons", "gallone", "usgal"]),
  u("ukgal", "volume", "gal (UK)", "Gallone (UK)", 4.54609, ["impgal", "imperialgallon"]),

  // ── mass (base: gram) ────────────────────────────────────────────────────
  u("mg", "mass", "mg", "Milligramm", 0.001, ["milligramm", "milligram"]),
  u("g", "mass", "g", "Gramm", 1, ["gramm", "gram", "grams"]),
  u("kg", "mass", "kg", "Kilogramm", 1000, ["kilo", "kilogramm", "kilogram", "kilos"]),
  u("t", "mass", "t", "Tonne", 1e6, ["tonne", "tonnen", "ton"]),
  u("oz", "mass", "oz", "Unze", 28.349523125, ["ounce", "ounces", "unze", "unzen"]),
  u("lb", "mass", "lb", "Pfund (lb)", 453.59237, ["lbs", "pound", "pounds"]),
  u("st", "mass", "st", "Stone", 6350.29318, ["stone"]),
  u("ct", "mass", "ct", "Karat", 0.2, ["karat", "carat"]),

  // ── temperature (base: °C, affine) ───────────────────────────────────────
  {
    id: "c", cat: "temperature", symbol: "°C", name: "Celsius",
    aliases: ["celsius"], toBase: (v) => v, fromBase: (v) => v,
  },
  {
    id: "f", cat: "temperature", symbol: "°F", name: "Fahrenheit",
    aliases: ["fahrenheit"], toBase: (v) => ((v - 32) * 5) / 9, fromBase: (v) => (v * 9) / 5 + 32,
  },
  {
    id: "k", cat: "temperature", symbol: "K", name: "Kelvin",
    aliases: ["kelvin"], toBase: (v) => v - 273.15, fromBase: (v) => v + 273.15,
  },

  // ── speed (base: m/s) ────────────────────────────────────────────────────
  u("kmh", "speed", "km/h", "Kilometer pro Stunde", 1000 / 3600, ["kph", "kmph", "kmproh", "kmpro"]),
  u("mph", "speed", "mph", "Meilen pro Stunde", 1609.344 / 3600, ["mi/h", "milesperhour"]),
  u("mps", "speed", "m/s", "Meter pro Sekunde", 1, ["ms-1", "meterprosekunde"]),
  u("kn", "speed", "kn", "Knoten", 1852 / 3600, ["knot", "knots", "knoten", "kt"]),
  u("fps", "speed", "ft/s", "Fuß pro Sekunde", 0.3048, ["ft/s", "feetpersecond"]),

  // ── time (base: second) ──────────────────────────────────────────────────
  u("ms", "time", "ms", "Millisekunde", 0.001, ["millisekunde", "millisekunden", "millisecond", "milliseconds"]),
  u("s", "time", "s", "Sekunde", 1, ["sec", "sekunde", "sekunden", "second", "seconds"]),
  u("min", "time", "min", "Minute", 60, ["minute", "minuten", "minutes", "mins"]),
  u("h", "time", "h", "Stunde", 3600, ["hr", "hrs", "std", "stunde", "stunden", "hour", "hours"]),
  u("d", "time", "d", "Tag", 86400, ["day", "days", "tag", "tage"]),
  u("wk", "time", "Wo", "Woche", 604800, ["week", "weeks", "woche", "wochen"]),
  u("mo", "time", "Mon", "Monat", 2629746, ["month", "months", "monat", "monate"]),
  u("yr", "time", "a", "Jahr", 31556952, ["year", "years", "jahr", "jahre", "y"]),

  // ── data (base: byte; SI decimal AND IEC binary) ─────────────────────────
  u("bit", "data", "bit", "Bit", 0.125, ["bits"]),
  u("b", "data", "B", "Byte", 1, ["byte", "bytes"]),
  u("kb", "data", "kB", "Kilobyte", 1e3, ["kilobyte"]),
  u("mb", "data", "MB", "Megabyte", 1e6, ["megabyte"]),
  u("gb", "data", "GB", "Gigabyte", 1e9, ["gigabyte", "gig"]),
  u("tb", "data", "TB", "Terabyte", 1e12, ["terabyte"]),
  u("pb", "data", "PB", "Petabyte", 1e15, ["petabyte"]),
  u("kib", "data", "KiB", "Kibibyte", 1024, ["kibibyte"]),
  u("mib", "data", "MiB", "Mebibyte", 1024 ** 2, ["mebibyte"]),
  u("gib", "data", "GiB", "Gibibyte", 1024 ** 3, ["gibibyte"]),
  u("tib", "data", "TiB", "Tebibyte", 1024 ** 4, ["tebibyte"]),

  // ── energy (base: joule) ─────────────────────────────────────────────────
  u("j", "energy", "J", "Joule", 1, ["joule"]),
  u("kj", "energy", "kJ", "Kilojoule", 1e3, ["kilojoule"]),
  u("cal", "energy", "cal", "Kalorie", 4.184, ["kalorie", "calorie"]),
  u("kcal", "energy", "kcal", "Kilokalorie", 4184, ["kilokalorie", "kalorien", "calories"]),
  u("wh", "energy", "Wh", "Wattstunde", 3600, ["wattstunde"]),
  u("kwh", "energy", "kWh", "Kilowattstunde", 3.6e6, ["kilowattstunde", "kilowatthour"]),
  u("btu", "energy", "BTU", "British Thermal Unit", 1055.05585262, []),

  // ── power (base: watt) ───────────────────────────────────────────────────
  u("w", "power", "W", "Watt", 1, ["watt"]),
  u("kw", "power", "kW", "Kilowatt", 1e3, ["kilowatt"]),
  u("mw", "power", "MW", "Megawatt", 1e6, ["megawatt"]),
  u("ps", "power", "PS", "Pferdestärke", 735.49875, ["pferdestärke", "pferdestaerke"]),
  u("hp", "power", "hp", "Horsepower (mech.)", 550 * 0.3048 * 4.4482216152605, ["horsepower", "bhp"]), // 550 ft·lbf/s

  // ── pressure (base: pascal) ──────────────────────────────────────────────
  u("pa", "pressure", "Pa", "Pascal", 1, ["pascal"]),
  u("hpa", "pressure", "hPa", "Hektopascal", 100, ["hektopascal"]),
  u("kpa", "pressure", "kPa", "Kilopascal", 1e3, ["kilopascal"]),
  u("mbar", "pressure", "mbar", "Millibar", 100, ["millibar"]),
  u("bar", "pressure", "bar", "Bar", 1e5, []),
  u("psi", "pressure", "psi", "Pfund pro Quadratzoll", 6894.757293168, []),
  u("atm", "pressure", "atm", "Atmosphäre", 101325, ["atmosphäre", "atmosphere"]),
  u("mmhg", "pressure", "mmHg", "Millimeter Quecksilber", 133.322387415, ["torr"]),

  // ── fuel consumption (base: L/100 km, reciprocal scales) ─────────────────
  {
    id: "l100km", cat: "fuel", symbol: "L/100 km", name: "Liter pro 100 km",
    aliases: ["l/100km", "lpro100km", "l100"], toBase: (v) => v, fromBase: (v) => v,
  },
  {
    id: "kml", cat: "fuel", symbol: "km/L", name: "Kilometer pro Liter",
    aliases: ["km/l", "kmpl"], toBase: recip(100), fromBase: recip(100),
  },
  {
    id: "mpg", cat: "fuel", symbol: "mpg (US)", name: "Meilen pro Gallone (US)",
    aliases: ["mpgus", "usmpg"], toBase: recip(MPG_US), fromBase: recip(MPG_US),
  },
  {
    id: "mpguk", cat: "fuel", symbol: "mpg (UK)", name: "Meilen pro Gallone (UK)",
    aliases: ["ukmpg", "mpgimp"], toBase: recip(MPG_UK), fromBase: recip(MPG_UK),
  },

  // ── angle (base: degree) ─────────────────────────────────────────────────
  u("deg", "angle", "°", "Grad", 1, ["°", "degree", "degrees"]),
  u("rad", "angle", "rad", "Radiant", 180 / Math.PI, ["radiant", "radian", "radians"]),
  u("grad", "angle", "gon", "Gon", 0.9, ["gon", "gradian"]),
  u("turn", "angle", "U", "Umdrehung", 360, ["umdrehung", "umdrehungen", "turns", "rev"]),
  u("arcmin", "angle", "′", "Bogenminute", 1 / 60, ["bogenminute"]),
  u("arcsec", "angle", "″", "Bogensekunde", 1 / 3600, ["bogensekunde"]),
];

// ── currencies (factor = EUR value of one unit, supplied at runtime) ────────

interface CurrencyDef {
  code: string;
  name: string;
  symbol?: string;
  aliases?: string[];
  crypto?: boolean;
}

/** Order = preview order: the everyday ones first. Codes match the ECB set
 *  frankfurter serves; BTC/ETH come from CoinGecko. */
const CURRENCIES: CurrencyDef[] = [
  { code: "EUR", name: "Euro", symbol: "€", aliases: ["euro", "euros"] },
  { code: "USD", name: "US-Dollar", symbol: "$", aliases: ["dollar", "dollars", "usdollar"] },
  { code: "GBP", name: "Britisches Pfund", symbol: "£", aliases: ["pfundsterling", "sterling", "quid"] },
  { code: "CHF", name: "Schweizer Franken", aliases: ["franken", "franc", "francs"] },
  { code: "JPY", name: "Japanischer Yen", symbol: "¥", aliases: ["yen"] },
  { code: "BTC", name: "Bitcoin", symbol: "₿", aliases: ["bitcoin", "xbt"], crypto: true },
  { code: "ETH", name: "Ether", aliases: ["ether", "ethereum"], crypto: true },
  { code: "CNY", name: "Chinesischer Yuan", aliases: ["yuan", "renminbi", "rmb"] },
  { code: "CAD", name: "Kanadischer Dollar" },
  { code: "AUD", name: "Australischer Dollar" },
  { code: "NZD", name: "Neuseeland-Dollar" },
  { code: "SEK", name: "Schwedische Krone" },
  { code: "NOK", name: "Norwegische Krone" },
  { code: "DKK", name: "Dänische Krone" },
  { code: "PLN", name: "Polnischer Złoty", aliases: ["zloty", "złoty"] },
  { code: "CZK", name: "Tschechische Krone" },
  { code: "HUF", name: "Ungarischer Forint", aliases: ["forint"] },
  { code: "RON", name: "Rumänischer Leu" },
  { code: "ISK", name: "Isländische Krone" },
  { code: "TRY", name: "Türkische Lira", aliases: ["lira"] },
  { code: "HKD", name: "Hongkong-Dollar" },
  { code: "SGD", name: "Singapur-Dollar" },
  { code: "KRW", name: "Südkoreanischer Won", aliases: ["won"] },
  { code: "INR", name: "Indische Rupie", aliases: ["rupie", "rupee", "rupees"] },
  { code: "IDR", name: "Indonesische Rupiah", aliases: ["rupiah"] },
  { code: "MYR", name: "Malaysischer Ringgit", aliases: ["ringgit"] },
  { code: "PHP", name: "Philippinischer Peso" },
  { code: "THB", name: "Thailändischer Baht", aliases: ["baht"] },
  { code: "BRL", name: "Brasilianischer Real", aliases: ["real", "reais"] },
  { code: "MXN", name: "Mexikanischer Peso" },
  { code: "ZAR", name: "Südafrikanischer Rand", aliases: ["rand"] },
  { code: "ILS", name: "Israelischer Schekel", aliases: ["schekel", "shekel"] },
];

for (const c of CURRENCIES) {
  UNITS.push({
    id: c.code.toLowerCase(),
    cat: "currency",
    symbol: c.code,
    name: c.name,
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

const SIG = new Intl.NumberFormat("de-DE", { maximumSignificantDigits: 7 });
const MONEY = new Intl.NumberFormat("de-DE", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
const CRYPTO = new Intl.NumberFormat("de-DE", { maximumSignificantDigits: 8 });

/** German-formatted number for a unit: money with two decimals, crypto with
 *  up to eight significant digits, everything else with seven significant
 *  digits; scientific notation only at the extremes; never "-0" or "NaN". */
export function formatValue(v: number, unit?: Unit): string {
  if (!Number.isFinite(v)) return "—";
  if (Object.is(v, -0) || v === 0) v = 0;
  const abs = Math.abs(v);
  if (unit?.cat === "currency") {
    if (unit.crypto) return abs !== 0 && abs < 1e-8 ? sci(v) : CRYPTO.format(v);
    return abs !== 0 && abs < 0.005 ? CRYPTO.format(v) : MONEY.format(v);
  }
  if (abs !== 0 && (abs >= 1e15 || abs < 1e-6)) return sci(v);
  const out = SIG.format(v);
  return out === "-0" ? "0" : out;
}

const SUP: Record<string, string> = {
  "-": "⁻", "0": "⁰", "1": "¹", "2": "²", "3": "³", "4": "⁴",
  "5": "⁵", "6": "⁶", "7": "⁷", "8": "⁸", "9": "⁹",
};

function sci(v: number): string {
  const [mant, exp] = v.toExponential(4).split("e");
  const m = String(Number(mant)).replace(".", ",");
  const e = String(Number(exp))
    .split("")
    .map((ch) => SUP[ch] ?? ch)
    .join("");
  return `${m}·10${e}`;
}
