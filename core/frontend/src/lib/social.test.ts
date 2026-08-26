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
  it("classifies real-world URL shapes", () => {
    expect(detectSocial("https://www.youtube.com/shorts/abc")?.platform).toBe("youtube");
    expect(detectSocial("https://m.youtube.com/watch?v=x")?.platform).toBe("youtube");
    expect(detectSocial("https://www.instagram.com/tv/x/")?.platform).toBe("instagram");
    expect(detectSocial("https://vm.tiktok.com/ZMabc/")?.platform).toBe("tiktok");
    expect(detectSocial("https://www.facebook.com/reel/1")?.platform).toBe("facebook");
  });
  it("labels every platform", () => {
    expect(platformLabel("youtube")).toBe("YouTube");
    expect(platformLabel("instagram")).toBe("Instagram");
    expect(platformLabel("tiktok")).toBe("TikTok");
    expect(platformLabel("facebook")).toBe("Facebook");
  });

  it("classifies the FIRST URL in the text (documented contract)", () => {
    // A non-social URL first means no download suggestion, even if a social
    // link follows — the detector inspects only the first match.
    expect(detectSocial("see https://example.com and https://youtu.be/x")).toBeNull();
    const t = detectSocial("https://youtu.be/a then https://www.tiktok.com/@u/video/1");
    expect(t?.platform).toBe("youtube");
    expect(t?.url).toBe("https://youtu.be/a");
  });

  it("terminates the URL at whitespace, quotes and angle brackets", () => {
    expect(detectSocial('<https://youtu.be/abc>')?.url).toBe("https://youtu.be/abc");
    expect(detectSocial('link: "https://youtu.be/abc" ok')?.url).toBe("https://youtu.be/abc");
    expect(detectSocial("'https://www.tiktok.com/@u/video/1'")?.url).toBe(
      "https://www.tiktok.com/@u/video/1",
    );
    expect(detectSocial("https://youtu.be/abc\nnext line")?.url).toBe("https://youtu.be/abc");
  });

  it("is case-insensitive on scheme + host but preserves the original URL", () => {
    const t = detectSocial("HTTPS://WWW.YOUTUBE.COM/watch?v=AbC");
    expect(t?.platform).toBe("youtube");
    expect(t?.url).toBe("HTTPS://WWW.YOUTUBE.COM/watch?v=AbC");
  });

  it("accepts plain http and the fb.com / facebook.com hosts", () => {
    expect(detectSocial("http://youtube.com/watch?v=x")?.platform).toBe("youtube");
    expect(detectSocial("https://fb.com/watch/123")?.platform).toBe("facebook");
    expect(detectSocial("https://www.facebook.com/watch?v=1")?.platform).toBe("facebook");
  });

  it("returns null for empty / scheme-less input", () => {
    expect(detectSocial("")).toBeNull();
    expect(detectSocial("ftp://youtube.com/x")).toBeNull();
    expect(detectSocial("www.youtube.com/watch?v=x")).toBeNull();
  });

  it("finds the URL amid Umlaut text", () => {
    const t = detectSocial("Schau das an: https://youtu.be/xyz — großartig!");
    expect(t?.platform).toBe("youtube");
    expect(t?.url).toBe("https://youtu.be/xyz");
  });
});

describe("dailymotion", () => {
  it("detects the long host, the dai.ly short form and the embed host", () => {
    for (const url of [
      "https://www.dailymotion.com/video/x7xd3st",
      "https://dai.ly/x7xd3st",
      "https://geo.dailymotion.com/player.html?video=x7xd3st",
    ]) {
      expect(detectSocial(url), url).toEqual({ platform: "dailymotion", url });
    }
    expect(platformLabel("dailymotion")).toBe("Dailymotion");
  });

  it("every platform has a label — a new one must not fall through", () => {
    // The list row used to hard-code its label chain, so an unlisted platform
    // was silently announced as "Facebook".
    for (const p of ["youtube", "instagram", "tiktok", "facebook", "dailymotion"] as const) {
      expect(platformLabel(p).length, p).toBeGreaterThan(0);
    }
  });
});
