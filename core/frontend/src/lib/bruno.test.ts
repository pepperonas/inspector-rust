import { describe, it, expect } from "vitest";
import {
  computeBruno,
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
});
