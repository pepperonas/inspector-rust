/**
 * `bruno` — German income-tax + social-contributions calculator.
 *
 * Port of the maintainer's `steuerschleuder` web app
 * (Brutto-Netto-Rechner 2025, §32a EStG simplified). Constants apply
 * to **Steuerjahr 2025**: Grundfreibetrag 12.096 €, Beitragsbemessungs-
 * grenzen KV 66.150 € / RV 96.600 €, RV 18.6 %, AV 2.6 %, PV 4.8 %
 * (kinderlos) / 3.6 % (mit Kindern, weitere -0.25 % pro Kind #2..#5).
 *
 * Lives in TS (not Rust) so the compute is **instant** as the user
 * types `bruno 60000` — no IPC round-trip per keystroke. The Rust
 * side owns only the persisted per-user defaults (see `bruno.rs`).
 *
 * ⚠️ Vereinfachte Tarif-Formeln + durchschnittliche Beitragssätze.
 * Faktorverfahren, individuelle Freibeträge, Kinderbonus-Verrechnung
 * und Lohnsteuer-Ermäßigungen werden nicht abgebildet. Keine
 * Steuerberatung.
 */

// ── Steuerkonstanten 2025 ────────────────────────────────────────

export const TC = {
  grundfreibetrag: 12096,
  childAllowance: 6672,
  werbungskosten: 1230,
  sonderausgaben: 36,
  entlastungAlleinerziehend: 4260,

  pensionRate: 18.6 / 100,
  unemploymentRate: 2.6 / 100,
  careRateWithKids: 3.6 / 100,
  careRateChildless: 4.8 / 100,
  careReductionPerKid: 0.5 / 100,

  pensionCap: 96600,
  healthCap: 66150,

  /** Healthcare base rate (`§ 241 SGB V`) — split 50/50 with employer. */
  healthRateBase: 14.6,

  /** Kirchensteuersätze pro Bundesland (8 % in BW/BY, sonst 9 %). */
  churchRates: {
    bw: 0.08, by: 0.08,
    be: 0.09, bb: 0.09, hb: 0.09, hh: 0.09, he: 0.09, mv: 0.09,
    ni: 0.09, nw: 0.09, rp: 0.09, sl: 0.09, sn: 0.09, st: 0.09,
    sh: 0.09, th: 0.09,
  } as Record<string, number>,

  soliFreeLimit: 19950,
  soliRate: 0.055,
} as const;

export type GermanState =
  | "bw" | "by" | "be" | "bb" | "hb" | "hh" | "he" | "mv"
  | "ni" | "nw" | "rp" | "sl" | "sn" | "st" | "sh" | "th";

export interface BrunoInput {
  /** Bruttogehalt pro Jahr in Euro. */
  yearlyGross: number;
  /** Lohnsteuerklasse 1..=6. */
  taxClass: 1 | 2 | 3 | 4 | 5 | 6;
  /** Bundesland-ISO. */
  state: GermanState;
  /** Anzahl Kinder (für PV-Ermäßigung + Kinderfreibetrag). */
  children: number;
  /** Mitglied einer kirchensteuerpflichtigen Religionsgemeinschaft. */
  isChurchMember: boolean;
  /** KV-Zusatzbeitrag der Kasse, in Prozent (z.B. 2.45 für TK 2025). */
  healthAdd: number;
}

export interface BrunoResult {
  yearlyGross: number;
  netYear: number;
  netMonth: number;
  /** Sozialversicherung — Arbeitnehmer-Anteil pro Jahr. */
  social: {
    health: number;
    care: number;
    pension: number;
    unemployment: number;
    total: number;
  };
  incomeTax: number;
  soli: number;
  churchTax: number;
  /** Summe aller Abzüge pro Jahr. */
  totalDeductions: number;
  /** Effektive Abgabenquote (Abzüge / Brutto). */
  deductionRate: number;
  /** Grenzsteuersatz (marginal income-tax rate at this zvE). */
  marginalRate: number;
}

// ── Einkommensteuer-Tarif §32a EStG (vereinfacht 2025) ─────────────

function grundtarif(zvE: number): number {
  const z = Math.floor(Math.max(0, zvE));
  if (z <= TC.grundfreibetrag) return 0;
  if (z <= 17430) {
    const y = (z - TC.grundfreibetrag) / 10000;
    return (922.98 * y + 1400) * y;
  }
  if (z <= 68480) {
    const w = (z - 17431) / 10000;
    return (181.19 * w + 2397) * w + 1025.38;
  }
  if (z <= 277825) return 0.42 * z - 11294.68;
  return 0.45 * z - 19619.93;
}

function grundtarif6(income: number): number {
  const z = Math.floor(Math.max(0, income));
  if (z <= 1000) return 0;
  if (z <= 17430) {
    const y = (z - 1000) / 10000;
    return (922.98 * y + 1400) * y;
  }
  if (z <= 68480) {
    const w = (z - 17431) / 10000;
    return (181.19 * w + 2397) * w + 1025.38;
  }
  if (z <= 277825) return 0.42 * z - 11294.68;
  return 0.45 * z - 19619.93;
}

function incomeTaxFor(zvE: number, taxClass: BrunoInput["taxClass"]): number {
  if (taxClass === 6) return grundtarif6(zvE);
  if (taxClass === 3) return 2 * grundtarif(zvE / 2);
  let tax = grundtarif(zvE);
  if (taxClass === 5) tax *= 1.15;
  return Math.max(0, tax);
}

function solidarityTax(incomeTax: number): number {
  if (incomeTax <= TC.soliFreeLimit) return 0;
  const glideEnd = TC.soliFreeLimit * 1.734;
  if (incomeTax <= glideEnd) {
    const excess = incomeTax - TC.soliFreeLimit;
    const rate = Math.min(0.055, (excess * 0.119) / (glideEnd - TC.soliFreeLimit));
    return excess * rate;
  }
  return incomeTax * TC.soliRate;
}

function churchTax(incomeTax: number, state: string, isMember: boolean): number {
  return isMember ? incomeTax * (TC.churchRates[state] ?? 0.09) : 0;
}

function socialContributions(yearlyGross: number, healthAdd: number, children: number) {
  const healthBase = Math.min(yearlyGross, TC.healthCap);
  const pensionBase = Math.min(yearlyGross, TC.pensionCap);

  const health = (healthBase * ((TC.healthRateBase + healthAdd) / 100)) / 2;

  let careRate = TC.careRateChildless;
  if (children > 0) {
    careRate = TC.careRateWithKids;
    if (children >= 2) {
      const reduction = Math.min((children - 1) * TC.careReductionPerKid, 0.02);
      careRate = Math.max(careRate - reduction, 0.036);
    }
  }
  const care = (healthBase * careRate) / 2;
  const pension = (pensionBase * TC.pensionRate) / 2;
  const unemployment = (pensionBase * TC.unemploymentRate) / 2;

  return {
    health,
    care,
    pension,
    unemployment,
    total: health + care + pension + unemployment,
  };
}

/** Hauptberechnung. Reine Funktion, keine Side-Effects. */
export function computeBruno(input: BrunoInput): BrunoResult {
  const { yearlyGross, taxClass, state, children, isChurchMember, healthAdd } = input;

  const social = socialContributions(yearlyGross, healthAdd, children);

  let deductions = TC.werbungskosten + TC.sonderausgaben + children * TC.childAllowance;
  if (taxClass === 2) deductions += TC.entlastungAlleinerziehend;

  const zvE =
    taxClass === 6
      ? Math.max(0, yearlyGross)
      : Math.max(0, yearlyGross - social.total - deductions);

  const incomeTax = incomeTaxFor(zvE, taxClass);
  const soli = solidarityTax(incomeTax);
  const church = churchTax(incomeTax, state, isChurchMember);

  const totalDeductions = social.total + incomeTax + soli + church;
  const netYear = yearlyGross - totalDeductions;
  const netMonth = netYear / 12;

  const deductionRate = yearlyGross > 0 ? totalDeductions / yearlyGross : 0;
  const marginalRate = Math.min(
    0.45,
    (incomeTaxFor(zvE + 100, taxClass) - incomeTax) / 100,
  );

  return {
    yearlyGross,
    netYear,
    netMonth,
    social,
    incomeTax,
    soli,
    churchTax: church,
    totalDeductions,
    deductionRate,
    marginalRate,
  };
}

// ── Parser ─────────────────────────────────────────────────────────

export interface BrunoCommand {
  /** Yearly gross in EUR. Monthly input is normalised here. */
  yearlyGross: number;
  /** What the user typed: `m` = monatlich, `j`/`y` = jährlich,
   *  `null` = no suffix → defaults to yearly. */
  period: "monthly" | "yearly";
}

/**
 * Parse a `bruno`-family command. Accepts:
 *   `bruno 60000`         → 60.000 € yearly
 *   `bruno 5000m`         → 5.000 € monthly → 60.000 € yearly
 *   `bruno 60000j`        → explicit yearly
 *   `bruno 60.000`        → German thousands separator
 *   `bruno 60,000.50`     → US format
 *   `bruno 4.500,75 m`    → German with monthly suffix
 *
 * Returns `null` when the input isn't a complete bruno command
 * (e.g. `bruno`, `bruno abc`, empty arg).
 */
export function parseBrunoCommand(query: string): BrunoCommand | null {
  const trimmed = query.trimStart();
  // Accept `bruno`, optional space, then the amount + optional period suffix.
  // The amount is captured loosely; we re-parse with normaliseAmount below.
  const m = trimmed.match(/^bruno\b\s*([\d.,]+)\s*([mjy])?\s*$/i);
  if (!m) return null;
  const amount = normaliseAmount(m[1]);
  if (amount === null || amount <= 0) return null;
  const periodChar = (m[2] ?? "j").toLowerCase();
  const period = periodChar === "m" ? "monthly" : "yearly";
  const yearlyGross = period === "monthly" ? amount * 12 : amount;
  return { yearlyGross, period };
}

/**
 * Detect whether the user is *partially* typing a bruno command —
 * i.e. has typed `b`, `br`, `bru`, `brun`, `bruno`, or `bruno ` but
 * not yet a parseable amount. Used to surface the autocomplete row
 * before the command is runnable, mirroring the `rz` / `optim` UX.
 */
export function isBrunoPrefix(query: string): boolean {
  const t = query.trimStart().toLowerCase();
  if (t.length === 0) return false;
  // `bruno` is the longest keyword — match any strict prefix of it
  // (including the empty suffix `bruno `).
  for (let n = 1; n <= "bruno".length; n++) {
    if (t === "bruno".slice(0, n)) return true;
  }
  return t === "bruno " || /^bruno\s/.test(t);
}

/**
 * Normalise mixed German / US number formats into a JS float.
 * - `60000` → 60000
 * - `60.000` → 60000 (German thousands sep)
 * - `60,000` → 60000 (US thousands sep, ambiguous → treat as thousands)
 * - `60.000,50` → 60000.50 (German full)
 * - `60,000.50` → 60000.50 (US full)
 * - `4500,75` → 4500.75 (German decimal)
 * - `4500.75` → 4500.75 (US decimal)
 * - returns `null` if it can't make sense of the input.
 *
 * Heuristic: if BOTH `.` and `,` appear, the rightmost one is the
 * decimal separator. If only one separator appears AND it's the only
 * one AND it has 1-2 digits after it, treat as decimal; otherwise
 * thousands.
 */
export function normaliseAmount(raw: string): number | null {
  const s = raw.trim();
  if (s.length === 0) return null;
  if (!/^[\d.,]+$/.test(s)) return null;

  const hasDot = s.includes(".");
  const hasComma = s.includes(",");

  if (hasDot && hasComma) {
    // Rightmost wins as decimal sep; the other is the thousands sep.
    const lastDot = s.lastIndexOf(".");
    const lastComma = s.lastIndexOf(",");
    const decimalIsDot = lastDot > lastComma;
    const cleaned = decimalIsDot
      ? s.replace(/,/g, "").replace(/\.(?=\d)/g, (_m, _i) => ".")
      : s.replace(/\./g, "").replace(",", ".");
    const n = parseFloat(cleaned);
    return Number.isFinite(n) ? n : null;
  }

  const only = hasDot ? "." : hasComma ? "," : null;
  if (only === null) {
    // Pure digits.
    const n = parseInt(s, 10);
    return Number.isFinite(n) ? n : null;
  }

  // Single separator type. Last group decides:
  // 1-2 digits after sep → decimal; otherwise thousands.
  const parts = s.split(only);
  const last = parts[parts.length - 1];
  if (parts.length === 2 && (last.length === 1 || last.length === 2)) {
    const n = parseFloat(only === "," ? s.replace(",", ".") : s);
    return Number.isFinite(n) ? n : null;
  }
  // Otherwise it's all thousands separators — strip them all.
  const n = parseInt(parts.join(""), 10);
  return Number.isFinite(n) ? n : null;
}
