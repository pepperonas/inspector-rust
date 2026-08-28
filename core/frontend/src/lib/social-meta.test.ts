import { describe, it, expect, vi } from "vitest";
import { formatDuration, clampDescription, createMetaLoader } from "./social-meta";
import type { SocialMeta } from "./ipc";

const meta = (url: string): SocialMeta => ({
  url,
  title: `T ${url}`,
  uploader: "U",
  duration_s: 1,
  thumbnail: null,
  description: "",
});

/** A fetcher whose promises resolve only when the test says so. */
function deferredFetcher() {
  const pending: Array<{ url: string; resolve: () => void; reject: (e: unknown) => void }> = [];
  const fn = vi.fn((url: string) =>
    new Promise<SocialMeta>((res, rej) => {
      pending.push({ url, resolve: () => res(meta(url)), reject: rej });
    }),
  );
  return { fn, pending };
}

describe("formatDuration", () => {
  it("reads as a running time", () => {
    expect(formatDuration(213)).toBe("3:33");
    expect(formatDuration(59)).toBe("0:59");
    expect(formatDuration(3661)).toBe("1:01:01");
  });
  it("says nothing rather than lying when the duration is unknown", () => {
    expect(formatDuration(null)).toBe("");
    expect(formatDuration(undefined)).toBe("");
    expect(formatDuration(-1)).toBe("");
    expect(formatDuration(NaN)).toBe("");
  });
});

describe("clampDescription", () => {
  it("leaves a short text alone and collapses whitespace", () => {
    expect(clampDescription("  a\n\n b ", 40)).toBe("a b");
  });
  it("cuts on a word boundary and marks the cut", () => {
    const out = clampDescription("alpha beta gamma delta epsilon", 20);
    expect(out.endsWith("…")).toBe(true);
    expect(out.length).toBeLessThanOrEqual(21);
    expect(out).not.toContain("epsilon");
  });
});

describe("createMetaLoader", () => {
  it("fetches each url once and caches the result", async () => {
    const { fn, pending } = deferredFetcher();
    const l = createMetaLoader(fn, 3);
    l.request(["a", "b"]);
    l.request(["a", "b"]); // same render, one keystroke later
    expect(fn).toHaveBeenCalledTimes(2);
    pending.forEach((p) => p.resolve());
    await Promise.resolve();
    await Promise.resolve();
    l.request(["a"]);
    expect(fn).toHaveBeenCalledTimes(2);
  });

  it("never runs more than the cap at once", async () => {
    // ⚠️ The point of the cap: a pasted list of thirty links must not spawn
    // thirty yt-dlp processes — each costs ~4 s and saturates the machine.
    const { fn, pending } = deferredFetcher();
    const l = createMetaLoader(fn, 2);
    l.request(["a", "b", "c", "d", "e"]);
    expect(fn).toHaveBeenCalledTimes(2);
    expect(l.inFlight()).toBe(2);
    pending[0].resolve();
    await new Promise((r) => setTimeout(r, 0));
    expect(fn).toHaveBeenCalledTimes(3);
  });

  it("records a failure instead of throwing, and does not retry it", async () => {
    const { fn, pending } = deferredFetcher();
    const l = createMetaLoader(fn, 1);
    l.request(["bad"]);
    pending[0].reject(new Error("nope"));
    await new Promise((r) => setTimeout(r, 0));
    expect(l.get("bad")).toMatchObject({ state: "failed" });
    l.request(["bad"]);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("reports a url as loading the moment it is queued", () => {
    const { fn } = deferredFetcher();
    const l = createMetaLoader(fn, 1);
    l.request(["a"]);
    expect(l.get("a")).toEqual({ state: "loading" });
  });
});
