import { beforeAll, describe, it, expect } from "vitest";
import en from "../locales/en.json";
import de from "../locales/de.json";

// The settings rows build their labels from the style names Rust sends
// (`set.sounds.<cue>.<style>`), which the generic key check cannot see.
let rust = "";
beforeAll(async () => {
  const { readFileSync } = (await import("node:" + "fs")) as unknown as {
    readFileSync(p: string, enc: string): string;
  };
  const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
  rust = readFileSync(`${cwd}/../rust-lib/src/sound.rs`, "utf8");
});

function stylesOf(table: string): string[] {
  const start = rust.indexOf(`pub static ${table}: CueStyles`);
  expect(start).toBeGreaterThan(-1);
  const block = rust.slice(start, rust.indexOf("};", start));
  return [...block.matchAll(/^\s*\("([a-z]+)", (?:Some|None)/gm)].map((m) => m[1]);
}

describe("selectable cue sounds", () => {
  for (const [cue, table] of [["ocr", "OCR_STYLES"], ["expand", "EXPAND_STYLES"]] as const) {
    it(`${cue}: every style Rust offers has a label in both languages`, () => {
      const styles = stylesOf(table);
      expect(styles.length).toBeGreaterThanOrEqual(3);
      expect(styles[styles.length - 1]).toBe("off");
      for (const key of [`set.sounds.${cue}`, ...styles.map((s) => `set.sounds.${cue}.${s}`)]) {
        expect(key in en, `${key} missing in en`).toBe(true);
        expect(key in de, `${key} missing in de`).toBe(true);
      }
    });
  }
});
