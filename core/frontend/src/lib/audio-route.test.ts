import { describe, expect, it } from "vitest";
import { audioKind, audioLabel, audioRouteView } from "./audio-route";
import type { AudioRoute } from "./ipc";

const speakers = { id: "10", name: "MacBook Pro-Lautsprecher", is_default: false, transport: "builtin" };
const bt = { id: "20", name: "MX Sound", is_default: false, transport: "bluetooth" };
const boomDev = { id: "30", name: "boom Audio", is_default: false, transport: "virtual" };

function route(over: Partial<AudioRoute> & { def?: string }): AudioRoute {
  const def = over.def;
  return {
    devices: [speakers, bt, boomDev].map((d) => ({ ...d, is_default: d.id === def })),
    boom_enabled: false,
    boom_installed: true,
    boom_device: "30",
    boom_target: null,
    ...over,
  };
}

describe("audioKind", () => {
  it("draws speakers, bluetooth, headphones and displays", () => {
    expect(audioKind(speakers)).toBe("speaker");
    expect(audioKind(bt)).toBe("bluetooth");
    expect(audioKind({ name: "AirPods Pro", transport: "bluetooth" })).toBe("headphones");
    expect(audioKind({ name: "LG HDR 4K", transport: "hdmi" })).toBe("display");
    expect(audioKind({ name: "Externe Kopfhörer", transport: "builtin" })).toBe("headphones");
    expect(audioKind({ name: "Weird thing", transport: "" })).toBe("other");
  });
});

describe("audioRouteView", () => {
  it("without boom the default device plays", () => {
    const v = audioRouteView(route({ def: "20" }));
    expect(v.viaBoom).toBe(false);
    expect(v.activeName).toBe("MX Sound");
    expect(v.activeKind).toBe("bluetooth");
    expect(audioLabel(v)).toBe("MX Sound");
  });

  it("with boom bridging the TARGET plays, not boom Audio", () => {
    const v = audioRouteView(route({ def: "30", boom_enabled: true, boom_target: "10" }));
    expect(v.viaBoom).toBe(true);
    expect(v.activeName).toBe("MacBook Pro-Lautsprecher");
    expect(v.rows.find((r) => r.active)?.id).toBe("10");
    expect(audioLabel(v)).toBe("boom → MacBook Pro-Lautsprecher");
  });

  it("hides boom Audio from the device list", () => {
    const v = audioRouteView(route({ def: "10" }));
    expect(v.rows.map((r) => r.id)).toEqual(["10", "20"]);
  });

  it("boom selected but no bridge yet → says so instead of guessing", () => {
    const v = audioRouteView(route({ def: "30", boom_enabled: true }));
    expect(v.viaBoom).toBe(true);
    expect(v.activeName).toBeNull();
    expect(v.rows.some((r) => r.active)).toBe(false);
    expect(audioLabel(v)).toBe("boom → startet…");
  });
});
