import { describe, it, expect } from "vitest";
import { detectSocial, platformLabel } from "./social";

describe("detectSocial", () => {
  it("classifies each platform", () => {
    expect(detectSocial("https://www.youtube.com/watch?v=x")?.platform).toBe("youtube");
    expect(detectSocial("https://youtu.be/x")?.platform).toBe("youtube");
    expect(detectSocial("https://www.instagram.com/reel/x/")?.platform).toBe("instagram");
    expect(detectSocial("https://www.tiktok.com/@u/video/1")?.platform).toBe("tiktok");
    expect(detectSocial("https://fb.watch/abc/")?.platform).toBe("facebook");
  });
  it("keeps query params (playlist/index) in the URL", () => {
    const t = detectSocial("https://www.youtube.com/watch?v=lA_gqFach1I&list=RDlA_gqFach1I&index=1");
    expect(t?.platform).toBe("youtube");
    expect(t?.url).toBe("https://www.youtube.com/watch?v=lA_gqFach1I&list=RDlA_gqFach1I&index=1");
  });
  it("extracts the URL out of surrounding text", () => {
    const t = detectSocial("watch this https://youtu.be/abc it's great");
    expect(t?.url).toBe("https://youtu.be/abc");
  });
  it("returns null for non-social / non-URL text", () => {
    expect(detectSocial("https://example.com/x")).toBeNull();
    expect(detectSocial("just some text")).toBeNull();
    expect(detectSocial("youtube.com/x")).toBeNull(); // no scheme
  });
  it("labels platforms", () => {
    expect(platformLabel("youtube")).toBe("YouTube");
    expect(platformLabel("tiktok")).toBe("TikTok");
  });
});
