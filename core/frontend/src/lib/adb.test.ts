import { describe, it, expect } from "vitest";
import {
  NAV_KEYS,
  DPAD_KEYS,
  filterPackages,
  kbHuman,
  uptimeHuman,
  deviceLabel,
  validTap,
  validSwipe,
  textSendable,
} from "./adb";

describe("keycode catalogue", () => {
  it("every code is a valid KEYCODE_ token (the Rust validator's grammar)", () => {
    const all = [...NAV_KEYS, ...Object.values(DPAD_KEYS)];
    for (const k of all) {
      expect(k.code).toMatch(/^[A-Z0-9_]+$/);
      expect(k.label.length).toBeGreaterThan(0);
    }
    // No duplicate codes — a duplicate would fire the wrong button's action.
    const codes = all.map((k) => k.code);
    expect(new Set(codes).size).toBe(codes.length);
  });
});

describe("filterPackages", () => {
  const pkgs = ["com.spotify.music", "com.android.chrome", "de.spiegel.android", "com.whatsapp"];

  it("all terms must match; prefix ranks before infix", () => {
    expect(filterPackages(pkgs, "spo")).toEqual(["com.spotify.music"]);
    // Multi-term AND.
    expect(filterPackages(pkgs, "com music")).toEqual(["com.spotify.music"]);
    // Prefix beats infix for the first term.
    const ranked = filterPackages(["zz.chrome.a", "chrome.b"], "chrome");
    expect(ranked[0]).toBe("chrome.b");
  });

  it("empty query returns everything, no matches returns empty", () => {
    expect(filterPackages(pkgs, "  ")).toHaveLength(4);
    expect(filterPackages(pkgs, "zzz")).toEqual([]);
  });
});

describe("formatters", () => {
  it("kbHuman climbs the 1024 ladder from kB", () => {
    expect(kbHuman(512)).toBe("512 KB");
    expect(kbHuman(7994052)).toBe("7.6 GB");
    expect(kbHuman(0)).toBe("0 B");
  });

  it("uptimeHuman is compact", () => {
    expect(uptimeHuman(90061)).toBe("1d 1h");
    expect(uptimeHuman(4500)).toBe("1h 15m");
    expect(uptimeHuman(300)).toBe("5m");
    expect(uptimeHuman(0)).toBe("—");
  });

  it("deviceLabel surfaces transport and the unauthorized warning", () => {
    expect(deviceLabel({ serial: "R5", model: "SM A546B", state: "device", wifi: false })).toBe("SM A546B");
    expect(deviceLabel({ serial: "1.2.3.4:5555", model: "SM A546B", state: "device", wifi: true })).toBe(
      "SM A546B · WLAN",
    );
    expect(deviceLabel({ serial: "R5", model: "unknown", state: "unauthorized", wifi: false })).toBe(
      "R5 — nicht autorisiert",
    );
  });
});

describe("input validation (mirrors the Rust ranges)", () => {
  it("tap and swipe ranges", () => {
    expect(validTap(0, 0)).toBe(true);
    expect(validTap(9999, 9999)).toBe(true);
    expect(validTap(-1, 5)).toBe(false);
    expect(validTap(10000, 5)).toBe(false);
    expect(validTap(1.5, 5)).toBe(false);
    expect(validSwipe(0, 0, 100, 100, 300)).toBe(true);
    expect(validSwipe(0, 0, 100, 100, 49)).toBe(false);
    expect(validSwipe(0, 0, 100, 100, 5001)).toBe(false);
  });

  it("textSendable is the ASCII gate", () => {
    expect(textSendable("hello world!")).toBe(true);
    expect(textSendable("grüße")).toBe(false); // input text can't deliver unicode
    expect(textSendable("")).toBe(false);
    expect(textSendable("tab\there")).toBe(false);
  });
});
