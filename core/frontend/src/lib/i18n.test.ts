import { describe, it, expect, afterEach } from "vitest";
import en from "../locales/en.json";
import de from "../locales/de.json";
import { setCatalog, t, tn, fill, locale } from "./i18n";

// Every source file of the app (not tests) — scanned for the keys it uses.
const sources = import.meta.glob(["../**/*.{ts,tsx}", "!../**/*.test.{ts,tsx}"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

afterEach(() => setCatalog("en", en));

describe("locale catalogues", () => {
  it("English and German have exactly the same keys", () => {
    const enKeys = Object.keys(en).sort();
    const deKeys = Object.keys(de).sort();
    expect(deKeys.filter((k) => !(k in en)), "keys only in de.json").toEqual([]);
    expect(enKeys.filter((k) => !(k in de)), "keys missing in de.json").toEqual([]);
  });

  it("no value is empty and placeholders match", () => {
    for (const [k, v] of Object.entries(en)) {
      expect(v.trim(), `${k} empty in en`).not.toBe("");
      const d = (de as Record<string, string>)[k];
      expect(d?.trim(), `${k} empty in de`).not.toBe("");
      expect(placeholders(d ?? ""), `placeholders of ${k}`).toEqual(placeholders(v));
    }
  });

  it("the scan finds the app sources", () => {
    // Without this an empty glob would make the key check true-by-emptiness.
    expect(Object.keys(sources).length).toBeGreaterThan(150);
  });

  it("every key the code uses exists", () => {
    const missing: string[] = [];
    for (const [file, src] of Object.entries(sources)) {
      for (const m of src.matchAll(/(?:\b(?:t|tn|tSplit)\(\s*|\b(?:hint|labelKey|i18nKey):\s*)"([a-z0-9_]+(?:\.[a-z0-9_]+)+)"/g)) {
        const key = m[1];
        const known = key in en || `${key}.one` in en;
        if (!known) missing.push(`${file}: ${key}`);
      }
    }
    expect(missing).toEqual([]);
  });
});

describe("t()", () => {
  it("translates, fills placeholders and follows the loaded language", () => {
    expect(t("tray.quit")).toBe("Quit Inspector Rust");
    expect(t("tray.open", { hotkey: "⌃Space" })).toBe("Open (⌃Space)");
    expect(locale()).toBe("en-US");
    setCatalog("de", de);
    expect(t("tray.quit")).toBe("Inspector Rust beenden");
    expect(locale()).toBe("de-DE");
  });

  it("a missing key shows the key, never crashes", () => {
    expect(t("no.such.key" as never)).toBe("no.such.key");
  });

  it("fill leaves unknown placeholders visible", () => {
    expect(fill("{a} {b}", { a: 1 })).toBe("1 {b}");
  });

  it("tn picks one/other", () => {
    setCatalog("en", { "x.items.one": "{n} item", "x.items.other": "{n} items" });
    expect(tn("x.items", 1)).toBe("1 item");
    expect(tn("x.items", 3)).toBe("3 items");
  });
});
