import { describe, it, expect } from "vitest";
import { RICKROLL_VIDEO_ID, watchUrl, RICK_LINES } from "./rickroll";

describe("rickroll", () => {
  it("watch url points at the real video (the browser fallback)", () => {
    expect(RICKROLL_VIDEO_ID).toBe("dQw4w9WgXcQ");
    expect(watchUrl()).toBe("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
  });

  it("has lyric lines for the marquee", () => {
    expect(RICK_LINES.length).toBeGreaterThan(3);
    expect(RICK_LINES[0]).toMatch(/never gonna/i);
  });
});
