import type { AudioDevice, AudioRoute } from "./ipc";

/** Which glyph the footer draws for an output. */
export type AudioKind = "speaker" | "headphones" | "bluetooth" | "display" | "other";

/** One output row in the footer's audio menu. */
export interface AudioRow {
  id: string;
  name: string;
  kind: AudioKind;
  /** The device the sound actually comes out of right now. */
  active: boolean;
}

export interface AudioRouteView {
  /** Real outputs — "boom Audio" itself is hidden (it is a switch, not a place). */
  rows: AudioRow[];
  /** Name of the device that plays, or `null` when it can't be told. */
  activeName: string | null;
  activeKind: AudioKind;
  /** boom sits in front of the output (bridge running, or selected + starting). */
  viaBoom: boolean;
  boomEnabled: boolean;
  boomInstalled: boolean;
}

const HEADPHONES = /headphone|kopfh(ö|oe)rer|airpods|earbuds|buds|ohrh(ö|oe)rer/i;

/** Transport + name → glyph. A Bluetooth headphone is headphones (the more
 *  telling picture); wired headphones on the built-in jack are too. */
export function audioKind(dev: Pick<AudioDevice, "name" | "transport">): AudioKind {
  if (HEADPHONES.test(dev.name)) return "headphones";
  switch (dev.transport) {
    case "bluetooth":
      return "bluetooth";
    case "builtin":
      return "speaker";
    case "hdmi":
    case "airplay":
      return "display";
    default:
      return /speaker|lautsprecher/i.test(dev.name) ? "speaker" : "other";
  }
}

/**
 * Where does the sound go? With boom bridging, the SYSTEM default is "boom
 * Audio" — which says nothing; the answer is the bridge target. Without a
 * bridge it is the default device. boom selected but its bridge not (yet)
 * up → `viaBoom` with no known target (the menu says so instead of guessing).
 */
export function audioRouteView(route: AudioRoute): AudioRouteView {
  const boomId = route.boom_device;
  const real = route.devices.filter((d) => d.id !== boomId && !/^boom audio$/i.test(d.name));
  const def = route.devices.find((d) => d.is_default);
  const defIsBoom = !!def && (def.id === boomId || /^boom audio$/i.test(def.name));
  const activeId = route.boom_target ?? (defIsBoom ? null : def?.id ?? null);
  const viaBoom = route.boom_target != null || defIsBoom;
  const active = real.find((d) => d.id === activeId) ?? null;
  return {
    rows: real.map((d) => ({ id: d.id, name: d.name, kind: audioKind(d), active: d.id === activeId })),
    activeName: active?.name ?? null,
    activeKind: active ? audioKind(active) : "other",
    viaBoom,
    boomEnabled: route.boom_enabled,
    boomInstalled: route.boom_installed,
  };
}

/** Footer label: "boom → Name", the name alone, or a fallback. */
export function audioLabel(v: AudioRouteView): string {
  const name = v.activeName ?? (v.viaBoom ? "startet…" : "kein Ausgang");
  return v.viaBoom ? `boom → ${name}` : name;
}
