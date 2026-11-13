import { describe, expect, it } from "vitest";
import {
  applyPreset, clampMbps, CUSTOM_MBPS, DEFAULT_OPTS, limitText, mbpsText, MP4_FPS, parseStored, PLATFORMS, QUALITIES,
  sizeText, TARGETS,
} from "./export-opts";
import type { RecordPlatformSpec } from "./ipc";

const shorts: RecordPlatformSpec = {
  id: "shorts", aspect: "9:16", target: "p1080", fps: 30, quality: "high", loudness: "-14",
  maxDurationS: 180, softDurationS: null, suffix: "shorts",
};

describe("export options", () => {
  it("a preset sets the platform's values and keeps the rest", () => {
    const o = applyPreset({ ...DEFAULT_OPTS, kind: "gif", fps: 15, codec: "hevc", captionsSidecar: "srt", quality: "custom", bitrateMbps: 40 }, shorts);
    expect(o).toMatchObject({ kind: "mp4", preset: "shorts", target: "p1080", fps: 30, quality: "high", loudness: "-14", bitrateMbps: null });
    expect(o.codec).toBe("hevc");
    expect(o.captionsSidecar).toBe("srt");
  });

  it("the stored choice is read field by field", () => {
    expect(parseStored(null)).toEqual(DEFAULT_OPTS);
    expect(parseStored("{broken")).toEqual(DEFAULT_OPTS);
    expect(parseStored("[1,2]")).toEqual(DEFAULT_OPTS);
    const o = parseStored(JSON.stringify({ target: "p2160", fps: 60, quality: "max", loudness: "-14", preset: "tiktok", codec: "x265", gifWidth: 500, captionsSidecar: "ass" }));
    expect(o).toMatchObject({ target: "p2160", fps: 60, quality: "max", loudness: "-14", preset: "tiktok" });
    expect(o.codec).toBe("h264");
    expect(o.gifWidth).toBe(640);
    expect(o.captionsSidecar).toBeNull();
    // A frame rate that only a GIF offers is not kept for an MP4.
    expect(parseStored(JSON.stringify({ fps: 20 })).fps).toBe(30);
    expect(parseStored(JSON.stringify({ kind: "gif", fps: 20 })).fps).toBe(20);
  });

  it("a custom level without a number becomes standard", () => {
    expect(parseStored(JSON.stringify({ quality: "custom" })).quality).toBe("standard");
    const o = parseStored(JSON.stringify({ quality: "custom", bitrateMbps: 500 }));
    expect([o.quality, o.bitrateMbps]).toEqual(["custom", CUSTOM_MBPS[1]]);
    // A number left over from an earlier custom choice is dropped with the level.
    expect(parseStored(JSON.stringify({ quality: "high", bitrateMbps: 30 })).bitrateMbps).toBeNull();
  });

  it("clampMbps keeps a custom bitrate in range", () => {
    expect(clampMbps(0.2)).toBe(2);
    expect(clampMbps(12.34)).toBe(12.3);
    expect(clampMbps(999)).toBe(80);
    expect(clampMbps(Number.NaN)).toBe(12);
  });

  it("summary texts", () => {
    expect(mbpsText(8_100_000, "en-US")).toBe("8.1");
    expect(mbpsText(8_100_000, "de-DE")).toBe("8,1");
    expect(mbpsText(32_300_000, "en-US")).toBe("32");
    expect(sizeText(45_400_000, "en-US")).toBe("45 MB");
    expect(sizeText(200_000, "en-US")).toBe("1 MB");
    expect(sizeText(1_240_000_000, "de-DE")).toBe("1,2 GB");
    expect(limitText(180)).toBe("3:00");
    expect(limitText(3600)).toBe("1:00:00");
    expect(limitText(75)).toBe("1:15");
  });

  it("offers what Rust accepts", async () => {
    const { readFileSync } = (await import("node:" + "fs")) as unknown as {
      readFileSync(path: string, encoding: "utf8"): string;
    };
    const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
    const read = (f: string) => readFileSync(`${cwd}/../rust-lib/src/record/${f}`, "utf8");
    const platform = read("platform.rs");
    const editor = read("editor.rs");
    expect(platform).toContain(`pub const MP4_FPS: [u32; 3] = [${MP4_FPS.join(", ")}];`);
    expect(platform).toContain(`pub const CUSTOM_MBPS: (f64, f64) = (${CUSTOM_MBPS[0]}.0, ${CUSTOM_MBPS[1]}.0);`);
    const variants = (src: string, name: string) =>
      [...(src.match(new RegExp(`pub enum ${name} \\{([^}]*)\\}`))?.[1] ?? "").matchAll(/^\s*([A-Z]\w*),/gm)].map((m) => m[1].toLowerCase());
    expect(variants(platform, "Quality")).toEqual([...QUALITIES]);
    expect(variants(platform, "Platform")).toEqual([...PLATFORMS]);
    expect(variants(editor, "OutputTarget")).toEqual([...TARGETS]);
  });
});
