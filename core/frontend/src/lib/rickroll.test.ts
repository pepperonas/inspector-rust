import { describe, it, expect } from "vitest";
import { RICKROLL_VIDEO_ID, embedUrl, watchUrl, RICK_LINES } from "./rickroll";

describe("rickroll urls", () => {
  it("embeds the canonical video via the privacy host", () => {
    expect(RICKROLL_VIDEO_ID).toBe("dQw4w9WgXcQ");
    const u = embedUrl({ autoplay: true });
    expect(u.startsWith("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?")).toBe(true);
    expect(u).toContain("autoplay=1");
    expect(u).toContain("playsinline=1");
  });

  it("can request muted autoplay as a fallback", () => {
    expect(embedUrl({ autoplay: true, mute: true })).toContain("mute=1");
    expect(embedUrl()).toContain("autoplay=0");
    expect(embedUrl()).toContain("mute=0");
  });

  it("watch url points at the real video", () => {
    expect(watchUrl()).toBe("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
  });

  it("has lyric lines for the marquee", () => {
    expect(RICK_LINES.length).toBeGreaterThan(3);
    expect(RICK_LINES[0]).toMatch(/never gonna/i);
  });
});
