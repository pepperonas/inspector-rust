/**
 * Pure helpers for the `rickroll` command (v0.122.0). The video id + URL
 * builders live here so they're testable; the panel owns the iframe + fun.
 */

/** Rick Astley — "Never Gonna Give You Up" (official upload). */
export const RICKROLL_VIDEO_ID = "dQw4w9WgXcQ";

/**
 * Privacy-friendly YouTube embed URL. `autoplay` starts it (needs a user
 * gesture for sound — the Enter/click that opened the panel provides one);
 * `mute` can force muted autoplay as a fallback if a browser blocks sound.
 */
export function embedUrl(opts: { autoplay?: boolean; mute?: boolean } = {}): string {
  const p = new URLSearchParams({
    autoplay: opts.autoplay ? "1" : "0",
    mute: opts.mute ? "1" : "0",
    rel: "0",
    modestbranding: "1",
    playsinline: "1",
  });
  return `https://www.youtube-nocookie.com/embed/${RICKROLL_VIDEO_ID}?${p.toString()}`;
}

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
