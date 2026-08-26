/**
 * Pure helpers for the `rickroll` command (v0.122.0; local clip since
 * v0.128.2). The video is a BUNDLED asset now — YouTube embeds die in the
 * Tauri WKWebView with "Error 153: Video player configuration error" (embed/
 * referer restrictions), so the panel plays a heavily compressed local MP4
 * (480p, ~5 MB) with zero network dependency. Only the "open in browser"
 * fallback still points at YouTube.
 */

/** Rick Astley — "Never Gonna Give You Up" (official upload). */
export const RICKROLL_VIDEO_ID = "dQw4w9WgXcQ";

/** Plain watch URL for the "open in browser" fallback. */
export function watchUrl(): string {
  return `https://www.youtube.com/watch?v=${RICKROLL_VIDEO_ID}`;
}

/** A few of the iconic lyrics, for the marquee under the video. */
export const RICK_LINES: readonly string[] = [
  "Never gonna give you up",
  "Never gonna let you down",
  "Never gonna run around and desert you",
  "Never gonna make you cry",
  "Never gonna say goodbye",
  "Never gonna tell a lie and hurt you",
];
