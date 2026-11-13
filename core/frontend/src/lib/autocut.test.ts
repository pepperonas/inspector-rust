import { beforeAll, describe, expect, it } from "vitest";
import type { RecordEditDoc, RecordEditorView } from "./ipc";
import {
  DEFAULT_GAP_MS,
  DENOISE_LEVELS,
  GAP_CHOICES,
  cutMany,
  cutTotalMs,
  denoisePending,
  denoisedName,
  formatSeconds,
  micFileFor,
  normalizeDenoise,
  setMicDenoise,
} from "./autocut";
import { cutSourceRange } from "./editor";

let DENOISE_RS = "";
let AUTOCUT_RS = "";
beforeAll(async () => {
  const { readFileSync } = (await import("node:" + "fs")) as unknown as {
    readFileSync(path: string, encoding: "utf8"): string;
  };
  const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
  DENOISE_RS = readFileSync(`${cwd}/../rust-lib/src/record/denoise.rs`, "utf8");
  AUTOCUT_RS = readFileSync(`${cwd}/../rust-lib/src/record/autocut.rs`, "utf8");
});

const doc = (clips: [number, number][], micDenoise?: number): RecordEditDoc =>
  ({
    version: 7,
    clips: clips.map(([startMs, endMs]) => ({ startMs, endMs, source: null })),
    speed: [],
    crop: clips.map(() => null),
    zooms: [],
    annotations: [],
    audioTracks: [],
    layoutRegions: [],
    effects: [],
    transcripts: [],
    micDenoise,
  }) as unknown as RecordEditDoc;

const view = (d: RecordEditDoc, micSources: string[], denoised: string[]) => ({ doc: d, micSources, denoised }) as unknown as RecordEditorView;

describe("shared with Rust", () => {
  it("offers the same strengths and file name", () => {
    expect(DENOISE_RS).toContain(`pub const LEVELS: [u8; 3] = [${DENOISE_LEVELS.join(", ")}];`);
    expect(DENOISE_RS).toContain('const CACHE_PREFIX: &str = "mic.denoised-";');
    expect(denoisedName(75)).toBe("mic.denoised-75.wav");
  });

  it("offers pause lengths Rust accepts, with Rust's default", () => {
    const m = AUTOCUT_RS.match(/MIN_GAP_RANGE_MS: \(u64, u64\) = \(([\d_]+), ([\d_]+)\)/);
    expect(m).toBeTruthy();
    const [lo, hi] = [m![1], m![2]].map((x) => Number(x.replace(/_/g, "")));
    for (const g of GAP_CHOICES) expect(g >= lo && g <= hi, String(g)).toBe(true);
    expect(AUTOCUT_RS).toContain(`pub const DEFAULT_MIN_GAP_MS: u64 = ${DEFAULT_GAP_MS};`);
    expect(GAP_CHOICES).toContain(DEFAULT_GAP_MS);
  });

  it("snaps a strength like Rust", () => {
    expect([0, 10, 24, 25, 60, 63, 80, 100, 255].map(normalizeDenoise)).toEqual([0, 0, 0, 50, 50, 75, 75, 100, 100]);
    expect(normalizeDenoise(undefined)).toBe(0);
    expect(normalizeDenoise(Number.NaN)).toBe(0);
  });
});

describe("noise suppression", () => {
  it("keeps the document when nothing changes", () => {
    const d = doc([[0, 5000]]);
    expect(setMicDenoise(d, 0)).toBe(d);
    expect(setMicDenoise(d, 75).micDenoise).toBe(75);
    const on = doc([[0, 5000]], 75);
    expect(setMicDenoise(on, 75)).toBe(on);
    expect(setMicDenoise(on, 0).micDenoise).toBe(0);
  });

  it("plays the cleaned file only where Rust says it exists", () => {
    const on = doc([[0, 5000]], 100);
    expect(micFileFor(view(on, [""], [""]), "")).toBe("mic.denoised-100.wav");
    expect(micFileFor(view(on, ["", "takes/a"], [""]), "takes/a")).toBe("mic.wav");
    expect(micFileFor(view(doc([[0, 5000]]), [""], [""]), "")).toBe("mic.wav");
  });

  it("knows when a cleaned file is still missing", () => {
    const on = doc([[0, 5000]], 50);
    expect(denoisePending(view(on, ["", "takes/a"], [""]))).toBe(true);
    expect(denoisePending(view(on, ["", "takes/a"], ["takes/a", ""]))).toBe(false);
    expect(denoisePending(view(on, [], []))).toBe(false);
    expect(denoisePending(view(doc([[0, 5000]]), [""], []))).toBe(false);
  });
});

describe("cutMany", () => {
  it("equals the single cuts applied from the back", () => {
    const d = doc([
      [0, 10_000],
      [20_000, 30_000],
    ]);
    const cuts = [
      { clip: 0, startMs: 2_000, endMs: 3_000 },
      { clip: 1, startMs: 24_000, endMs: 25_000 },
      { clip: 0, startMs: 6_000, endMs: 7_000 },
    ];
    const got = cutMany(d, cuts);
    expect(got.clips.map((c) => [c.startMs, c.endMs])).toEqual([
      [0, 2_000],
      [3_000, 6_000],
      [7_000, 10_000],
      [20_000, 24_000],
      [25_000, 30_000],
    ]);
    // The order the caller hands them in does not matter.
    expect(cutMany(d, [...cuts].reverse())).toEqual(got);
    // Applied front to back with the ORIGINAL indices the result would be wrong:
    const naive = cuts.reduce((x, c) => cutSourceRange(x, c.clip, c.startMs, c.endMs), d);
    expect(naive).not.toEqual(got);
  });

  it("handles a cut at a clip's edge and a clip cut away whole", () => {
    const d = doc([
      [0, 1_000],
      [5_000, 9_000],
      [10_000, 12_000],
    ]);
    const got = cutMany(d, [
      { clip: 0, startMs: 0, endMs: 1_000 },
      { clip: 1, startMs: 5_000, endMs: 6_000 },
      { clip: 2, startMs: 11_000, endMs: 11_500 },
    ]);
    expect(got.clips.map((c) => [c.startMs, c.endMs])).toEqual([
      [6_000, 9_000],
      [10_000, 11_000],
      [11_500, 12_000],
    ]);
    expect(cutMany(d, [])).toBe(d);
  });

  it("adds up what the cuts remove, overlaps once", () => {
    expect(
      cutTotalMs([
        { clip: 0, startMs: 1_000, endMs: 2_000 },
        { clip: 0, startMs: 1_500, endMs: 2_500 },
        { clip: 1, startMs: 1_000, endMs: 1_300 },
      ]),
    ).toBe(1_800);
    expect(cutTotalMs([])).toBe(0);
  });

  it("formats seconds in the app language", () => {
    expect(formatSeconds(1_740, "de-DE")).toBe("1,7 s");
    expect(formatSeconds(1_740, "en-US")).toBe("1.7 s");
  });
});
