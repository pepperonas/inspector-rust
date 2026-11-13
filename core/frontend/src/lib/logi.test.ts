import { beforeAll, describe, expect, it } from "vitest";
import en from "../locales/en.json";
import de from "../locales/de.json";
import {
  actionComplete,
  assignableControls,
  batteryLevel,
  buttonLabelKey,
  buttonName,
  clampThreshold,
  configComplete,
  EMPTY_CONFIG,
  hasDirections,
  shownDpi,
  shownWheel,
  SMART_MAX,
  SMART_MIN,
  withAction,
  type LogiStatus,
} from "./logi";

let RUST = "";
beforeAll(async () => {
  const { readFileSync } = (await import("node:" + "fs")) as unknown as {
    readFileSync(path: string, encoding: "utf8"): string;
  };
  const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
  RUST = readFileSync(`${cwd}/../rust-lib/src/logi/mod.rs`, "utf8");
});

const status: LogiStatus = {
  supported: true,
  connected: true,
  permission_denied: false,
  name: "MX Master",
  battery: { percent: 90, charging: false },
  dpi: { current: 1000, default: 1000, options: [400, 600, 800, 1000, 1200, 1400, 1600] },
  wheel: { mode: "smart", threshold: 16 },
  invert: false,
  controls: [
    { cid: 0x50, divertable: false },
    { cid: 0x51, divertable: false },
    { cid: 0x52, divertable: true },
    { cid: 0x53, divertable: true },
    { cid: 0x56, divertable: true },
    { cid: 0xc3, divertable: true },
    { cid: 0xc4, divertable: true },
    { cid: 0x999, divertable: true },
  ],
  options_running: false,
};

describe("constants shared with Rust", () => {
  it("has the same SmartShift range", () => {
    expect(RUST).toContain(`pub const SMART_MIN: u8 = ${SMART_MIN};`);
    expect(RUST).toContain(`pub const SMART_MAX: u8 = ${SMART_MAX};`);
  });

  it("names every control the Rust side names, the same way", () => {
    const arms = [...RUST.matchAll(/0x([0-9A-Fa-f]{2}) => "([a-z_]+)",/g)];
    expect(arms.length).toBeGreaterThanOrEqual(8);
    for (const [, hex, name] of arms) expect(buttonName(parseInt(hex, 16))).toBe(name);
    expect(buttonName(0x1234)).toBe("other");
  });

  it("has a label for every button name in both languages", () => {
    for (const cid of [0x50, 0x51, 0x52, 0x53, 0x56, 0xc3, 0xc4, 0xd7, 0x1234]) {
      const key = buttonLabelKey(cid);
      expect((en as Record<string, string>)[key], key).toBeTruthy();
      expect((de as Record<string, string>)[key], key).toBeTruthy();
    }
  });
});

describe("assignableControls", () => {
  it("drops the fixed buttons and puts the thumb button first", () => {
    expect(assignableControls(status.controls).map((c) => c.cid)).toEqual([0xc3, 0x52, 0x53, 0x56, 0xc4, 0x999]);
  });
});

describe("withAction", () => {
  it("sets a slot without touching the input", () => {
    const next = withAction(EMPTY_CONFIG, 0xc3, "up", { type: "volume_up" });
    expect(next.buttons["195"]).toEqual({ up: { type: "volume_up" } });
    expect(EMPTY_CONFIG.buttons).toEqual({});
  });

  it("removes the button when its last slot is cleared", () => {
    const one = withAction(EMPTY_CONFIG, 0xc3, "up", { type: "volume_up" });
    const two = withAction(one, 0xc3, "press", { type: "mute_toggle" });
    const back = withAction(two, 0xc3, "up", null);
    expect(back.buttons["195"]).toEqual({ press: { type: "mute_toggle" } });
    expect(withAction(back, 0xc3, "press", null).buttons).toEqual({});
  });

  it("keeps the other buttons", () => {
    const a = withAction(EMPTY_CONFIG, 0x53, "press", { type: "prev_tab" });
    const b = withAction(a, 0x56, "press", { type: "next_tab" });
    expect(Object.keys(b.buttons).sort()).toEqual(["83", "86"]);
  });
});

describe("completeness", () => {
  it("knows which actions still miss their target", () => {
    expect(actionComplete({ type: "mute_toggle" })).toBe(true);
    expect(actionComplete({ type: "shortcut", keys: "" })).toBe(false);
    expect(actionComplete({ type: "shortcut", keys: "Ctrl+ArrowUp" })).toBe(true);
    expect(actionComplete({ type: "open", target: "  " })).toBe(false);
    expect(actionComplete({ type: "hotkey", action: "" })).toBe(false);
    expect(actionComplete({ type: "task", id: 0 })).toBe(false);
    expect(actionComplete({ type: "task", id: 3 })).toBe(true);
  });

  it("holds a config back while one action is unfinished", () => {
    const ok = withAction(EMPTY_CONFIG, 0x53, "press", { type: "prev_tab" });
    expect(configComplete(ok)).toBe(true);
    expect(configComplete(withAction(ok, 0xc3, "left", { type: "shortcut", keys: "" }))).toBe(false);
    expect(configComplete(EMPTY_CONFIG)).toBe(true);
  });
});

describe("small rules", () => {
  it("detects direction actions", () => {
    expect(hasDirections(undefined)).toBe(false);
    expect(hasDirections({ press: { type: "mute_toggle" } })).toBe(false);
    expect(hasDirections({ left: { type: "prev_tab" } })).toBe(true);
  });

  it("clamps the SmartShift threshold", () => {
    expect(clampThreshold(0)).toBe(SMART_MIN);
    expect(clampThreshold(999)).toBe(SMART_MAX);
    expect(clampThreshold(16.4)).toBe(16);
    expect(clampThreshold(Number.NaN)).toBe(SMART_MIN);
  });

  it("shows the stored wheel setting before the reported one", () => {
    expect(shownWheel(EMPTY_CONFIG, status)).toEqual({ mode: "smart", threshold: 16 });
    const cfg = { ...EMPTY_CONFIG, wheel: { mode: "free" as const, threshold: 16 } };
    expect(shownWheel(cfg, status)).toEqual({ mode: "free", threshold: 16 });
    expect(shownWheel(EMPTY_CONFIG, { ...status, wheel: null })).toBeNull();
  });

  it("shows the DPI the mouse reports, else the stored one", () => {
    expect(shownDpi({ ...EMPTY_CONFIG, dpi: 1600 }, status)).toBe(1000);
    expect(shownDpi({ ...EMPTY_CONFIG, dpi: 1600 }, { ...status, dpi: null })).toBe(1600);
  });

  it("grades the battery", () => {
    expect(batteryLevel(10)).toBe("low");
    expect(batteryLevel(15)).toBe("low");
    expect(batteryLevel(40)).toBe("mid");
    expect(batteryLevel(41)).toBe("ok");
  });
});
