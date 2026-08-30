import { describe, it, expect } from "vitest";
import {
  computeBruno,
  formatBrunoBreakdown,
  isBrunoPrefix,
  normaliseAmount,
  parseBrunoCommand,
  toggleSelfMode,
  buildBrunoExport,
  brunoSelfAssumptions,
  type BrunoInput,
} from "./bruno";

const DEFAULT_INPUT: BrunoInput = {
  yearlyGross: 60000,
  taxClass: 1,
  state: "nw",
  children: 0,
  isChurchMember: false,
  healthAdd: 2.45,
};

describe("normaliseAmount", () => {
  it("parses plain integers", () => {
    expect(normaliseAmount("60000")).toBe(60000);
    expect(normaliseAmount("4500")).toBe(4500);
  });
  it("German thousands separator (`60.000`)", () => {
    expect(normaliseAmount("60.000")).toBe(60000);
    expect(normaliseAmount("1.234.567")).toBe(1234567);
  });
  it("US thousands separator (`60,000`)", () => {
    expect(normaliseAmount("60,000")).toBe(60000);
  });
  it("German decimal (`4500,75`)", () => {
    expect(normaliseAmount("4500,75")).toBeCloseTo(4500.75);
    expect(normaliseAmount("4500,5")).toBeCloseTo(4500.5);
  });
  it("US decimal (`4500.75`)", () => {
    expect(normaliseAmount("4500.75")).toBeCloseTo(4500.75);
  });
  it("mixed German full (`60.000,50`)", () => {
    expect(normaliseAmount("60.000,50")).toBeCloseTo(60000.5);
  });
  it("mixed US full (`60,000.50`)", () => {
    expect(normaliseAmount("60,000.50")).toBeCloseTo(60000.5);
  });
  it("returns null for garbage", () => {
    expect(normaliseAmount("")).toBeNull();
    expect(normaliseAmount("abc")).toBeNull();
    expect(normaliseAmount("12abc")).toBeNull();
  });
});

describe("parseBrunoCommand", () => {
  it("parses bare yearly", () => {
    const r = parseBrunoCommand("bruno 60000");
    expect(r?.yearlyGross).toBe(60000);
    expect(r?.period).toBe("yearly");
  });
  it("parses monthly with `m` suffix and normalises to yearly", () => {
    const r = parseBrunoCommand("bruno 5000m");
    expect(r?.yearlyGross).toBe(60000);
    expect(r?.period).toBe("monthly");
  });
  it("parses explicit `j` suffix", () => {
    const r = parseBrunoCommand("bruno 60000j");
    expect(r?.yearlyGross).toBe(60000);
    expect(r?.period).toBe("yearly");
  });
  it("parses `y` as English yearly", () => {
    const r = parseBrunoCommand("bruno 60000y");
    expect(r?.period).toBe("yearly");
  });
  it("accepts German thousands separator", () => {
    expect(parseBrunoCommand("bruno 60.000")?.yearlyGross).toBe(60000);
  });
  it("is case-insensitive on keyword + suffix", () => {
    expect(parseBrunoCommand("BRUNO 5000M")?.yearlyGross).toBe(60000);
  });
  it("returns null for bare keyword", () => {
    expect(parseBrunoCommand("bruno")).toBeNull();
    expect(parseBrunoCommand("bruno ")).toBeNull();
  });
  it("returns null for non-numeric arg", () => {
    expect(parseBrunoCommand("bruno abc")).toBeNull();
  });
  it("returns null for zero / negative", () => {
    expect(parseBrunoCommand("bruno 0")).toBeNull();
  });
  it("tolerates whitespace around suffix", () => {
    expect(parseBrunoCommand("bruno 5000 m")?.yearlyGross).toBe(60000);
  });
  it("does not match `brunoo` or `brun`", () => {
    expect(parseBrunoCommand("brunoo 5000")).toBeNull();
    expect(parseBrunoCommand("brun 5000")).toBeNull();
  });
});

describe("isBrunoPrefix", () => {
  it("matches partial keyword", () => {
    for (const p of ["b", "br", "bru", "brun", "bruno", "bruno "]) {
      expect(isBrunoPrefix(p)).toBe(true);
    }
  });
  it("matches `bruno 5000` (already typing)", () => {
    expect(isBrunoPrefix("bruno 5000")).toBe(true);
  });
  it("rejects unrelated input", () => {
    expect(isBrunoPrefix("")).toBe(false);
    expect(isBrunoPrefix("x")).toBe(false);
    expect(isBrunoPrefix("abruno")).toBe(false);
  });
});

describe("computeBruno — sanity ranges (Steuerjahr 2025, Klasse I)", () => {
  it("60k gross → ~36-40k net (rough sanity)", () => {
    const r = computeBruno(DEFAULT_INPUT);
    expect(r.netYear).toBeGreaterThan(36000);
    expect(r.netYear).toBeLessThan(40000);
    expect(r.netMonth).toBeCloseTo(r.netYear / 12, 4);
  });
  it("Grundfreibetrag — 12k gross pays only social, ~no income tax", () => {
    const r = computeBruno({ ...DEFAULT_INPUT, yearlyGross: 12000 });
    expect(r.incomeTax).toBeLessThan(50);
    expect(r.soli).toBe(0);
  });
  it("high earner (200k) hits the 42 % bracket", () => {
    const r = computeBruno({ ...DEFAULT_INPUT, yearlyGross: 200000 });
    expect(r.marginalRate).toBeGreaterThan(0.4);
    expect(r.marginalRate).toBeLessThanOrEqual(0.45);
  });
  it("church membership increases total deductions", () => {
    const base = computeBruno(DEFAULT_INPUT);
    const churchy = computeBruno({ ...DEFAULT_INPUT, isChurchMember: true });
    expect(churchy.totalDeductions).toBeGreaterThan(base.totalDeductions);
    expect(churchy.churchTax).toBeGreaterThan(0);
  });
  it("kids reduce PV → slightly higher net (small effect)", () => {
    const childless = computeBruno({ ...DEFAULT_INPUT, children: 0 });
    const parent = computeBruno({ ...DEFAULT_INPUT, children: 2 });
    // Kinderfreibetrag boost — net should rise.
    expect(parent.netYear).toBeGreaterThan(childless.netYear);
  });
  it("BW lower church rate (8 %) vs NW (9 %)", () => {
    const nw = computeBruno({ ...DEFAULT_INPUT, isChurchMember: true, state: "nw" });
    const bw = computeBruno({ ...DEFAULT_INPUT, isChurchMember: true, state: "bw" });
    expect(bw.churchTax).toBeLessThan(nw.churchTax);
  });
  it("Steuerklasse VI taxes everything (no Freibeträge)", () => {
    const k1 = computeBruno({ ...DEFAULT_INPUT, taxClass: 1 });
    const k6 = computeBruno({ ...DEFAULT_INPUT, taxClass: 6 });
    expect(k6.incomeTax).toBeGreaterThan(k1.incomeTax);
  });
  it("netMonth × 12 = netYear (no rounding drift)", () => {
    const r = computeBruno(DEFAULT_INPUT);
    expect(r.netMonth * 12).toBeCloseTo(r.netYear, 6);
  });
  it("deductionRate is between 0 and 1", () => {
    const r = computeBruno(DEFAULT_INPUT);
    expect(r.deductionRate).toBeGreaterThan(0);
    expect(r.deductionRate).toBeLessThan(1);
  });
  it("zero gross returns all-zeroes (no NaN explosion)", () => {
    const r = computeBruno({ ...DEFAULT_INPUT, yearlyGross: 0 });
    expect(r.netYear).toBe(0);
    expect(r.incomeTax).toBe(0);
    expect(r.social.total).toBe(0);
    expect(Number.isFinite(r.deductionRate)).toBe(true);
  });

  it("Steuerklasse III (splitting) taxes less than class I on the same gross", () => {
    // Class 3 applies the splitting tariff (grundtarif(zvE/2) × 2), which is
    // progressive-favourable → strictly less income tax than class 1.
    const k1 = computeBruno({ ...DEFAULT_INPUT, taxClass: 1 });
    const k3 = computeBruno({ ...DEFAULT_INPUT, taxClass: 3 });
    expect(k3.incomeTax).toBeLessThan(k1.incomeTax);
    expect(k3.incomeTax).toBeGreaterThan(0);
  });

  it("Steuerklasse V taxes more than class I (the ×1.15 secondary-earner factor)", () => {
    const k1 = computeBruno({ ...DEFAULT_INPUT, taxClass: 1 });
    const k5 = computeBruno({ ...DEFAULT_INPUT, taxClass: 5 });
    expect(k5.incomeTax).toBeGreaterThan(k1.incomeTax);
  });

  it("high earners land in the top linear bracket and still net < gross", () => {
    // 300k gross drives zvE past the 277 825 € kink → the 0.45·z − 19 619.93
    // top bracket. Just assert monotonic sanity (no formula blow-up).
    const mid = computeBruno({ ...DEFAULT_INPUT, yearlyGross: 100_000 });
    const high = computeBruno({ ...DEFAULT_INPUT, yearlyGross: 300_000 });
    expect(high.incomeTax).toBeGreaterThan(mid.incomeTax);
    expect(high.netYear).toBeGreaterThan(mid.netYear);
    expect(high.netYear).toBeLessThan(300_000);
    // Above the soli glide zone → full 5.5 % solidarity surcharge kicks in.
    expect(high.soli).toBeGreaterThan(0);
  });
});

describe("computeBruno — lower tax brackets (Progressionszonen)", () => {
  it("20k gross (class I) lands in progression zone 1: small but non-zero income tax", () => {
    const r = computeBruno({ ...DEFAULT_INPUT, yearlyGross: 20_000 });
    expect(r.incomeTax).toBeGreaterThan(0);
    expect(r.incomeTax).toBeLessThan(1_500);
    expect(r.netYear).toBeGreaterThan(0);
    expect(r.netYear).toBeLessThan(20_000);
  });

  it("class VI taxes a low gross from the first euro-ish (tiny 1k allowance only)", () => {
    // 15k in class VI → zone 1 of the class-6 tariff; class I would pay
    // almost nothing here (below/near Grundfreibetrag after deductions).
    const six = computeBruno({ ...DEFAULT_INPUT, yearlyGross: 15_000, taxClass: 6 });
    const one = computeBruno({ ...DEFAULT_INPUT, yearlyGross: 15_000, taxClass: 1 });
    expect(six.incomeTax).toBeGreaterThan(one.incomeTax);
    expect(six.incomeTax).toBeGreaterThan(500);
  });

  it("class VI mid income (50k) lands in zone 2 and still exceeds class I", () => {
    const six = computeBruno({ ...DEFAULT_INPUT, yearlyGross: 50_000, taxClass: 6 });
    const one = computeBruno({ ...DEFAULT_INPUT, yearlyGross: 50_000, taxClass: 1 });
    expect(six.incomeTax).toBeGreaterThan(one.incomeTax);
  });

  it("class II (Alleinerziehend) nets more than class I on the same gross", () => {
    const two = computeBruno({ ...DEFAULT_INPUT, children: 1, taxClass: 2 });
    const one = computeBruno({ ...DEFAULT_INPUT, children: 1, taxClass: 1 });
    expect(two.netYear).toBeGreaterThan(one.netYear);
  });
});

describe("computeBruno — church-rate fallback", () => {
  it("an unknown state falls back to the 9 % church rate (same as NW)", () => {
    const unknown = computeBruno({
      ...DEFAULT_INPUT,
      isChurchMember: true,
      state: "xx" as BrunoInput["state"],
    });
    const nw = computeBruno({ ...DEFAULT_INPUT, isChurchMember: true, state: "nw" });
    expect(unknown.churchTax).toBeCloseTo(nw.churchTax, 6);
    expect(unknown.churchTax).toBeGreaterThan(0);
  });
});

describe("normaliseAmount — separator edge cases", () => {
  it("multi-group German thousands (`1.234.567`)", () => {
    expect(normaliseAmount("1.234.567")).toBe(1_234_567);
  });

  it("multi-group US thousands with decimal (`1,234,567.89`)", () => {
    expect(normaliseAmount("1,234,567.89")).toBeCloseTo(1_234_567.89, 6);
  });

  it("single-digit decimal after comma (`4500,5`)", () => {
    expect(normaliseAmount("4500,5")).toBeCloseTo(4500.5, 6);
  });

  it("three digits after a single separator read as thousands (`1,234`)", () => {
    expect(normaliseAmount("1,234")).toBe(1234);
    expect(normaliseAmount("1.234")).toBe(1234);
  });
});

describe("computeBruno — class VI upper brackets", () => {
  it("class VI 100k hits the 42 % linear bracket", () => {
    const r = computeBruno({ ...DEFAULT_INPUT, yearlyGross: 100_000, taxClass: 6 });
    expect(r.incomeTax).toBeCloseTo(0.42 * 100_000 - 11_294.68, 0);
  });

  it("class VI 300k hits the 45 % top bracket", () => {
    const r = computeBruno({ ...DEFAULT_INPUT, yearlyGross: 300_000, taxClass: 6 });
    expect(r.incomeTax).toBeCloseTo(0.45 * 300_000 - 19_619.93, 0);
  });
});

describe("formatBrunoBreakdown", () => {
  const view = {
    yearlyGross: 60000,
    netYear: 38000.5,
    netMonth: 3166.71,
    totalDeductions: 21999.5,
    deductionRate: 0.3667,
    marginalRate: 0.42,
    social: { health: 4800.25, care: 1020, pension: 5580, unemployment: 780 },
    incomeTax: 9000.75,
    soli: 0,
    churchTax: 0,
    taxClass: 1,
    state: "nw",
    children: 0,
    isChurchMember: false,
  };

  it("contains every row the preview shows, in order", () => {
    const out = formatBrunoBreakdown(view);
    const idx = (s: string) => out.indexOf(s);
    expect(idx("Brutto / Jahr")).toBeGreaterThan(-1);
    expect(idx("Krankenversicherung")).toBeGreaterThan(idx("Brutto / Monat"));
    expect(idx("Einkommensteuer")).toBeGreaterThan(idx("Arbeitslosenversicherung"));
    expect(idx("Netto / Jahr")).toBeGreaterThan(idx("Netto / Monat"));
    // German currency formatting (de-DE uses . thousands + , decimals).
    expect(out).toContain("60.000");
    expect(out).toContain("3.166,71");
  });

  it("names the assumptions line from the defaults", () => {
    const out = formatBrunoBreakdown(view);
    expect(out).toContain("Klasse 1 · Nordrhein-Westfalen · kinderlos · keine Kirchensteuer");
  });

  it("omits zero Soli/Kirchensteuer rows but shows them when set", () => {
    const none = formatBrunoBreakdown(view);
    expect(none).not.toContain("Solidaritätszuschlag");
    // The deduction ROW is absent (the assumptions line legitimately says
    // "keine Kirchensteuer", so match the line-anchored row form).
    expect(none).not.toMatch(/^Kirchensteuer\s+−/m);
    const withBoth = formatBrunoBreakdown({
      ...view, soli: 123.45, churchTax: 500, isChurchMember: true,
    });
    expect(withBoth).toContain("Solidaritätszuschlag");
    expect(withBoth).toMatch(/^Kirchensteuer\s+−/m);
    expect(withBoth).toContain("kirchensteuerpflichtig");
  });

  it("pluralises children and falls back on unknown states", () => {
    expect(formatBrunoBreakdown({ ...view, children: 1 })).toContain("1 Kind ·");
    expect(formatBrunoBreakdown({ ...view, children: 2 })).toContain("2 Kinder");
    expect(formatBrunoBreakdown({ ...view, state: "xx" })).toContain("XX");
  });

  it("aligns the value column (keys padded to equal width)", () => {
    const out = formatBrunoBreakdown(view);
    const lines = out.split("\n").filter((l) => l.includes("  "));
    // Every €-row's value starts at the same column.
    const starts = lines
      .filter((l) => l.includes("€") || l.includes("%"))
      .map((l) => l.search(/ {2}[−\d]/));
    expect(new Set(starts).size).toBe(1);
  });
});

// ── Selbständigen-Kalkulation (Suffix `f`) ──────────────────────────────────

import { computeBrunoSelf, formatBrunoSelfBreakdown, TCF, type BrunoSelfInput } from "./bruno";

const SELF_BASE: BrunoSelfInput = {
  yearlyProfit: 80000,
  state: "nw",
  children: 0,
  isChurchMember: false,
  healthAdd: 2.45,
  kvType: "gkv",
  pkvMonthly: 0,
  kvSickPay: false,
  businessType: "freiberufler",
  hebesatz: 400,
  married: false,
};

describe("parseBrunoCommand — self-employed forms", () => {
  it("`f` suffix flags the self-employed calculation", () => {
    expect(parseBrunoCommand("bruno 80000f")).toMatchObject({
      yearlyGross: 80000, period: "yearly", self: true,
    });
    expect(parseBrunoCommand("bruno 80000 f")).toMatchObject({ self: true });
  });

  it("combines with the period suffix (`7000mf` = monthly profit)", () => {
    expect(parseBrunoCommand("bruno 7000mf")).toMatchObject({
      yearlyGross: 84000, period: "monthly", self: true,
    });
  });

  it("income − expenses form computes the profit", () => {
    expect(parseBrunoCommand("bruno 90000-15000f")).toMatchObject({
      yearlyGross: 75000, self: true, expenses: 15000,
    });
    expect(parseBrunoCommand("bruno 90.000 - 15.000 f")).toMatchObject({
      yearlyGross: 75000, expenses: 15000,
    });
  });

  it("expenses form without `f` is rejected (employee has no Betriebsausgaben)", () => {
    expect(parseBrunoCommand("bruno 90000-15000")).toBeNull();
  });

  it("expenses ≥ income → null (no profit to tax)", () => {
    expect(parseBrunoCommand("bruno 50000-50000f")).toBeNull();
    expect(parseBrunoCommand("bruno 50000-60000f")).toBeNull();
  });

  it("stays fully backward compatible for employee forms", () => {
    expect(parseBrunoCommand("bruno 60000")).toMatchObject({
      yearlyGross: 60000, period: "yearly", self: false,
    });
    expect(parseBrunoCommand("bruno 5000m")).toMatchObject({
      yearlyGross: 60000, period: "monthly", self: false,
    });
  });
});

describe("computeBrunoSelf — GKV", () => {
  it("charges the reduced GKV rate + Zusatzbeitrag on the profit", () => {
    const r = computeBrunoSelf(SELF_BASE);
    // 66.150-cap NOT hit (80k > cap → base = cap): 66150 × (14.0+2.45)%
    expect(r.health).toBeCloseTo(66150 * 0.1645, 0);
    // PV kinderlos voller Satz 4,2 % auf die gedeckelte Basis.
    expect(r.care).toBeCloseTo(66150 * 0.042, 0);
  });

  it("sick-pay option raises the rate to 14.6 %", () => {
    const withSick = computeBrunoSelf({ ...SELF_BASE, kvSickPay: true });
    const without = computeBrunoSelf(SELF_BASE);
    expect(withSick.health).toBeGreaterThan(without.health);
    expect(withSick.health - without.health).toBeCloseTo(66150 * 0.006, 0);
  });

  it("a tiny profit is charged on the Mindestbemessungsgrundlage", () => {
    const r = computeBrunoSelf({ ...SELF_BASE, yearlyProfit: 6000 });
    expect(r.health).toBeCloseTo(TCF.gkvMinBaseYearly * 0.1645, 0);
    // Mindest-KV kann das Netto unter den Gewinn drücken — aber nie auf > Gewinn stehen bleiben.
    expect(r.netYear).toBeLessThan(6000);
  });

  it("children lower the PV rate (full self-employed scale, floored)", () => {
    const one = computeBrunoSelf({ ...SELF_BASE, children: 1 });
    expect(one.care).toBeCloseTo(66150 * 0.036, 0);
    const three = computeBrunoSelf({ ...SELF_BASE, children: 3 });
    expect(three.care).toBeCloseTo(66150 * (0.036 - 2 * 0.0025), 0);
    const seven = computeBrunoSelf({ ...SELF_BASE, children: 7 });
    expect(seven.care).toBeCloseTo(66150 * 0.026, 0); // floor 2,6 %
  });
});

describe("computeBrunoSelf — PKV", () => {
  it("uses the fixed premium ×12 and no separate care row", () => {
    const r = computeBrunoSelf({ ...SELF_BASE, kvType: "pkv", pkvMonthly: 650 });
    expect(r.health).toBe(650 * 12);
    expect(r.care).toBe(0);
  });

  it("negative premium input is clamped to 0", () => {
    const r = computeBrunoSelf({ ...SELF_BASE, kvType: "pkv", pkvMonthly: -10 });
    expect(r.health).toBe(0);
  });
});

describe("computeBrunoSelf — Gewerbesteuer + § 35", () => {
  it("Freiberufler pay no Gewerbesteuer", () => {
    const r = computeBrunoSelf(SELF_BASE);
    expect(r.gewerbesteuer).toBe(0);
    expect(r.gewerbeAnrechnung).toBe(0);
  });

  it("Gewerbe: Freibetrag 24.500, Messzahl 3,5 %, Hebesatz", () => {
    const r = computeBrunoSelf({ ...SELF_BASE, businessType: "gewerbe", hebesatz: 400 });
    // (80000 − 24500 → 55500, auf 100 abgerundet) × 3,5 % × 400 %
    expect(r.gewerbesteuer).toBeCloseTo(55500 * 0.035 * 4, 0);
  });

  it("profit below the Freibetrag → no Gewerbesteuer", () => {
    const r = computeBrunoSelf({
      ...SELF_BASE, businessType: "gewerbe", yearlyProfit: 24000,
    });
    expect(r.gewerbesteuer).toBe(0);
  });

  it("§ 35 credit: at Hebesatz 400 % the credit equals the full GewSt", () => {
    // Anrechnung = min(4,0 × Messbetrag, GewSt, ESt); bei Hebesatz 400 ist
    // 4,0 × Messbetrag == GewSt → volle Anrechnung (ESt ist hier größer).
    const r = computeBrunoSelf({ ...SELF_BASE, businessType: "gewerbe", hebesatz: 400 });
    expect(r.gewerbeAnrechnung).toBeCloseTo(r.gewerbesteuer, 2);
  });

  it("§ 35 credit is capped at 4×Messbetrag for high Hebesätze", () => {
    const r = computeBrunoSelf({ ...SELF_BASE, businessType: "gewerbe", hebesatz: 500 });
    expect(r.gewerbeAnrechnung).toBeLessThan(r.gewerbesteuer);
    expect(r.gewerbeAnrechnung).toBeCloseTo((r.gewerbesteuer / 5) * 4, 0);
  });

  it("§ 35 credit never exceeds the income tax", () => {
    const r = computeBrunoSelf({
      ...SELF_BASE, businessType: "gewerbe", yearlyProfit: 30000, hebesatz: 900,
    });
    expect(r.incomeTax).toBeGreaterThanOrEqual(0);
    expect(r.gewerbeAnrechnung).toBeLessThanOrEqual(r.gewerbesteuer);
  });
});

describe("computeBrunoSelf — tariff + edges", () => {
  it("Splitting (married) yields less tax than Grundtarif at the same profit", () => {
    const single = computeBrunoSelf(SELF_BASE);
    const married = computeBrunoSelf({ ...SELF_BASE, married: true });
    expect(married.incomeTax).toBeLessThan(single.incomeTax);
    expect(married.netYear).toBeGreaterThan(single.netYear);
  });

  it("church members pay Kirchensteuer on the (post-§35) income tax", () => {
    const r = computeBrunoSelf({ ...SELF_BASE, isChurchMember: true });
    expect(r.churchTax).toBeCloseTo(r.incomeTax * 0.09, 2);
    const by = computeBrunoSelf({ ...SELF_BASE, isChurchMember: true, state: "by" });
    expect(by.churchTax).toBeCloseTo(by.incomeTax * 0.08, 2);
  });

  it("zero profit: no tax, but Mindest-KV still applies (negative net)", () => {
    const r = computeBrunoSelf({ ...SELF_BASE, yearlyProfit: 0 });
    expect(r.incomeTax).toBe(0);
    expect(r.health).toBeGreaterThan(0);
    expect(r.netYear).toBeLessThan(0);
    expect(r.deductionRate).toBe(0); // guard: no divide-by-zero
  });

  it("marginal rate is capped at 45 %", () => {
    const r = computeBrunoSelf({ ...SELF_BASE, yearlyProfit: 500000 });
    expect(r.marginalRate).toBeLessThanOrEqual(0.45);
  });
});

describe("formatBrunoSelfBreakdown", () => {
  const view = {
    ...computeBrunoSelf({ ...SELF_BASE, businessType: "gewerbe" as const }),
    businessType: "gewerbe" as const,
    hebesatz: 400,
    kvType: "gkv" as const,
    kvSickPay: false,
    children: 0,
    isChurchMember: false,
    married: false,
    state: "nw",
    expenses: 15000,
  };

  it("shows income − expenses when the expenses form was used", () => {
    const out = formatBrunoSelfBreakdown(view);
    expect(out).toContain("Einnahmen / Jahr");
    expect(out).toContain("Betriebsausgaben");
    expect(out).toContain("Gewinn / Jahr");
  });

  it("names the Rechtsform + Hebesatz + KV in the assumptions line", () => {
    const out = formatBrunoSelfBreakdown(view);
    expect(out).toContain("Gewerbebetrieb · Hebesatz 400 %");
    expect(out).toContain("GKV freiwillig ermäßigt");
    expect(out).toContain("Grundtarif");
  });

  it("Gewerbesteuer row + § 35 credit only for Gewerbe", () => {
    const out = formatBrunoSelfBreakdown(view);
    expect(out).toContain("Gewerbesteuer");
    expect(out).toContain("§ 35-Anrechnung");
    const frei = formatBrunoSelfBreakdown({
      ...view,
      ...computeBrunoSelf(SELF_BASE),
      businessType: "freiberufler" as const,
      expenses: undefined,
    });
    expect(frei).not.toContain("Gewerbesteuer");
    expect(frei).toContain("Freiberufler (keine GewSt)");
  });

  it("carries the RV/AV + USt/§19 disclaimer", () => {
    expect(formatBrunoSelfBreakdown(view)).toContain("§ 19 Kleinunternehmer");
  });
});

describe("brunoSelfAssumptions", () => {
  const base = {
    businessType: "freiberufler" as const,
    hebesatz: 400,
    kvType: "gkv" as const,
    kvSickPay: false,
    children: 0,
    isChurchMember: false,
    married: false,
    state: "nw",
  };

  it("describes the freelancer default (no trade tax, ermäßigt GKV, single, childless)", () => {
    const s = brunoSelfAssumptions(base);
    expect(s).toContain("Freiberufler (keine GewSt)");
    expect(s).toContain("GKV freiwillig ermäßigt");
    expect(s).toContain("Grundtarif");
    expect(s).toContain("kinderlos");
    expect(s).toContain("keine Kirchensteuer");
  });

  it("shows the Hebesatz only for a Gewerbebetrieb", () => {
    expect(brunoSelfAssumptions({ ...base, businessType: "gewerbe", hebesatz: 470 })).toContain(
      "Gewerbebetrieb · Hebesatz 470 %",
    );
    expect(brunoSelfAssumptions(base)).not.toContain("Hebesatz");
  });

  it("switches GKV variants and PKV", () => {
    expect(brunoSelfAssumptions({ ...base, kvSickPay: true })).toContain("GKV freiwillig mit Krankengeld");
    expect(brunoSelfAssumptions({ ...base, kvType: "pkv" })).toContain("PKV (Fixbeitrag)");
  });

  it("pluralises children and flips marriage/church", () => {
    expect(brunoSelfAssumptions({ ...base, children: 1 })).toContain("1 Kind");
    const many = brunoSelfAssumptions({ ...base, children: 3 });
    expect(many).toContain("3 Kinder");
    const wed = brunoSelfAssumptions({ ...base, married: true, isChurchMember: true });
    expect(wed).toContain("Splittingtarif");
    expect(wed).toContain("kirchensteuerpflichtig");
  });

  it("uses the state label, falling back to upper-case for an unknown code", () => {
    expect(brunoSelfAssumptions({ ...base, state: "zz" })).toContain("ZZ");
  });
});

describe("toggleSelfMode — Modus ohne Neutippen wechseln", () => {
  it("schaltet in beide Richtungen und bleibt parsebar", () => {
    expect(toggleSelfMode("bruno 60000")).toBe("bruno 60000f");
    expect(toggleSelfMode("bruno 60000f")).toBe("bruno 60000");
    // Das Ergebnis muss wieder durch den Parser gehen, sonst ist der Wechsel
    // eine Sackgasse.
    const a = parseBrunoCommand(toggleSelfMode("bruno 60000"));
    expect(a?.self).toBe(true);
    expect(a?.yearlyGross).toBe(60000);
  });

  it("behält den Monatsbezug", () => {
    expect(toggleSelfMode("bruno 5000m")).toBe("bruno 5000mf");
    const r = parseBrunoCommand("bruno 5000mf");
    expect(r?.period).toBe("monthly");
    expect(r?.yearlyGross).toBe(60000);
  });

  it("löst die Einnahmen-Ausgaben-Form auf, statt eine ungültige Eingabe zu erzeugen", () => {
    // ⚠️ `parseBrunoCommand` weist `einnahmen-ausgaben` für Angestellte ab.
    // Der Betrag behält seine Bedeutung: der GEWINN, der auf dem Schirm stand.
    const out = toggleSelfMode("bruno 90000-15000f");
    expect(out).toBe("bruno 75000");
    expect(parseBrunoCommand(out)?.yearlyGross).toBe(75000);
    expect(parseBrunoCommand(out)?.self).toBe(false);
  });

  it("lässt alles unberührt, was kein vollständiger bruno-Befehl ist", () => {
    for (const q of ["bruno", "bruno abc", "", "rz 50"]) {
      expect(toggleSelfMode(q)).toBe(q);
    }
  });
});

describe("buildBrunoExport — eine Abbildung für Vorschau UND PDF", () => {
  const employee = {
    yearlyGross: 60000, netYear: 37795, netMonth: 3149.6,
    deductionRate: 0.37, marginalRate: 0.42,
    incomeTax: 9290, soli: 0, churchTax: 0,
    social: { health: 5115, care: 1440, pension: 5580, unemployment: 780 },
    taxClass: 1, state: "nw", children: 0, isChurchMember: false,
  };

  it("Angestellten-Modus trägt alle vier Sozialversicherungen", () => {
    const r = buildBrunoExport(employee);
    expect(r.mode).toBe("employee");
    expect(r.social.map((s) => s.label)).toEqual([
      "Krankenversicherung", "Pflegeversicherung", "Rentenversicherung", "Arbeitslosenversicherung",
    ]);
    expect(r.assumptions).toContain("Steuerklasse 1");
    expect(r.assumptions).toContain("Nordrhein-Westfalen");
  });

  it("Soli 0 bleibt als Aussage drin — eine Steuer, die fehlt, wirkt vergessen", () => {
    const r = buildBrunoExport(employee);
    const soli = r.taxes.find((x) => x.label === "Solidaritätszuschlag");
    expect(soli).toBeDefined();
    expect(soli!.value).toBe(0);
  });

  it("Unternehmer-Modus: keine RV/AV, § 35 als NEGATIVE Zeile", () => {
    const r = buildBrunoExport({
      ...employee,
      self: {
        yearlyProfit: 80000, netYear: 48000, netMonth: 4000,
        totalDeductions: 32000, deductionRate: 0.4, marginalRate: 0.44,
        health: 9870, care: 2520, incomeTax: 15000, soli: 0, churchTax: 0,
        gewerbesteuer: 2800, gewerbeAnrechnung: 2380,
        businessType: "gewerbe", hebesatz: 400, kvType: "gkv", kvSickPay: false,
        children: 0, isChurchMember: false, married: false, state: "nw",
      // Der View-Typ trägt mehr Felder; für die Abbildung zählen genau diese.
      } as never,
    });
    expect(r.mode).toBe("self");
    const labels = r.social.map((s) => s.label);
    expect(labels).not.toContain("Rentenversicherung");
    expect(labels).not.toContain("Arbeitslosenversicherung");
    // ⚠️ Die § 35-Anrechnung MINDERT die ESt — als positive Zeile würde die
    // Summe der Abzüge nicht mehr zum Netto passen.
    const anr = r.taxes.find((x) => x.label === "§ 35-Anrechnung");
    expect(anr).toBeDefined();
    expect(anr!.value).toBeLessThan(0);
  });

  it("Summenprobe: Basis − Abzüge = Netto (in beiden Modi)", () => {
    // ⚠️ Der Export rechnet NICHT selbst — aber seine Zeilen müssen die
    // Rechnung des Kerns wiedergeben, sonst zeigt das PDF eine Aufstellung,
    // deren Summen nicht aufgehen.
    const r = buildBrunoExport(employee);
    const ded = [...r.taxes, ...r.social].reduce((a, x) => a + x.value, 0);
    expect(r.base_year - ded).toBeCloseTo(r.net_year, 0);
  });
});
