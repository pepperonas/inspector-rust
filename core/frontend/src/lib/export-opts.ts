// Export options of the record editor: defaults, what a platform preset changes, the stored
// last choice, and the texts of the summary line. Pure — sizes, bitrates and warnings come
// from Rust (`editor_export_info`).
import type { EditorExportOptions, RecordPlatform, RecordPlatformSpec } from "./ipc";

export const MP4_FPS = [24, 30, 60] as const;
export const GIF_FPS = [15, 20, 25, 30] as const;
export const GIF_WIDTHS = [480, 640, 854, 1080] as const;
export const TARGETS = ["p720", "p1080", "p1440", "p2160", "source"] as const;
export const QUALITIES = ["auto", "small", "standard", "high", "max", "custom"] as const;
export const PLATFORMS = ["youtube", "shorts", "reels", "tiktok"] as const;
/** Range of a custom bitrate in Mbit/s (mirrors `platform.rs` `CUSTOM_MBPS`). */
export const CUSTOM_MBPS = [2, 80] as const;
export const STORE_KEY = "ir.record.export.v1";

export const PLATFORM_NAME: Record<(typeof PLATFORMS)[number], string> = {
  youtube: "YouTube",
  shorts: "Shorts",
  reels: "Reels",
  tiktok: "TikTok",
};

export const TARGET_NAME: Record<Exclude<(typeof TARGETS)[number], "source">, string> = {
  p720: "720p",
  p1080: "1080p",
  p1440: "1440p",
  p2160: "4K",
};

export type ExportOpts = Required<EditorExportOptions>;

export const DEFAULT_OPTS: ExportOpts = {
  kind: "mp4",
  target: "p1080",
  fps: 30,
  codec: "h264",
  gifWidth: 640,
  loopForever: true,
  preset: null,
  quality: "auto",
  bitrateMbps: null,
  loudness: "-16",
  captionsSidecar: null,
};

/** `opts` set up for a platform. The aspect is the document's — the caller switches it. */
export function applyPreset(opts: ExportOpts, spec: RecordPlatformSpec): ExportOpts {
  return {
    ...opts,
    kind: "mp4",
    preset: spec.id,
    target: spec.target,
    fps: spec.fps,
    quality: spec.quality,
    bitrateMbps: null,
    loudness: spec.loudness,
  };
}

export function clampMbps(v: number): number {
  if (!Number.isFinite(v)) return 12;
  return Math.min(CUSTOM_MBPS[1], Math.max(CUSTOM_MBPS[0], Math.round(v * 10) / 10));
}

const oneOf = <T extends string | number | null>(list: readonly T[], v: unknown, fallback: T): T =>
  list.includes(v as T) ? (v as T) : fallback;

/** The stored last choice, field by field: anything unknown falls back to the default. */
export function parseStored(raw: string | null): ExportOpts {
  let o: Record<string, unknown> = {};
  try {
    const v: unknown = raw ? JSON.parse(raw) : null;
    if (v && typeof v === "object" && !Array.isArray(v)) o = v as Record<string, unknown>;
  } catch {
    // unreadable → defaults
  }
  const d = DEFAULT_OPTS;
  const kind = oneOf(["mp4", "gif"] as const, o.kind, d.kind);
  const quality = oneOf(QUALITIES, o.quality, d.quality);
  const mbps = typeof o.bitrateMbps === "number" ? clampMbps(o.bitrateMbps) : null;
  return {
    kind,
    target: oneOf(TARGETS, o.target, d.target),
    fps: kind === "gif" ? oneOf(GIF_FPS, o.fps, 15) : oneOf(MP4_FPS, o.fps, d.fps),
    codec: oneOf(["h264", "hevc"] as const, o.codec, d.codec),
    gifWidth: oneOf(GIF_WIDTHS, o.gifWidth, d.gifWidth),
    loopForever: typeof o.loopForever === "boolean" ? o.loopForever : d.loopForever,
    preset: oneOf<RecordPlatform | null>([...PLATFORMS, null], o.preset, null),
    // A custom level without its number would be refused by the export.
    quality: quality === "custom" && mbps === null ? "standard" : quality,
    bitrateMbps: quality === "custom" ? mbps : null,
    loudness: oneOf(["-16", "-14"] as const, o.loudness, d.loudness),
    captionsSidecar: oneOf<"srt" | "vtt" | null>(["srt", "vtt", null], o.captionsSidecar, null),
  };
}

/** `8.1` for 8 100 000 bit/s (one decimal below 10, none above). */
export function mbpsText(bps: number, locale: string): string {
  const m = bps / 1e6;
  return m.toLocaleString(locale, { maximumFractionDigits: m < 10 ? 1 : 0 });
}

/** `45 MB` / `1.2 GB` (decimal units, like the Finder). */
export function sizeText(bytes: number, locale: string): string {
  const mb = bytes / 1e6;
  if (mb < 1000) return `${Math.max(1, Math.round(mb)).toLocaleString(locale)} MB`;
  return `${(mb / 1000).toLocaleString(locale, { maximumFractionDigits: 1 })} GB`;
}

/** `3:00` / `1:00:00` for a limit in seconds. */
export function limitText(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = String(seconds % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}
