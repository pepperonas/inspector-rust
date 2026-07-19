import { describe, it, expect } from "vitest";
import {
  computeBruno,
  formatBrunoBreakdown,
  isBrunoPrefix,
  normaliseAmount,
  parseBrunoCommand,
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
