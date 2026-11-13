// Devices in the recording HUD: lists that refresh while it is open, the stored choice
// against what is plugged in, the camera preview's frame format and the level meter's peak.
// Pure — the capture itself lives in Rust.

export interface NamedDevice { id: string; name: string }

/** How often the HUD re-reads its device lists. */
export const DEVICE_POLL_MS = 2000;
/** How often the camera picture and the level are fetched. */
export const PREVIEW_POLL_MS = 90;
export const LEVEL_POLL_MS = 80;
/** The peak marker holds this long, then falls by this much per second (0..1 of the bar). */
export const PEAK_HOLD_MS = 900;
export const PEAK_FALL_PER_S = 0.6;
/** From here the bar is drawn as "too loud". */
export const HOT_LEVEL = 0.9;

/** Same devices in the same order (ids and names) — a poll that changed nothing keeps the list. */
export function sameDevices(a: readonly NamedDevice[], b: readonly NamedDevice[]): boolean {
  return a.length === b.length && a.every((d, i) => d.id === b[i].id && d.name === b[i].name);
}

/** The select's value: the stored device while it is there, else "" (the default entry). */
export function shownDevice(devices: readonly NamedDevice[], stored: string | undefined): string {
  return stored && devices.some((d) => d.id === stored) ? stored : "";
}

/** A device was chosen but is not connected (the take would use the default one). */
export function deviceMissing(devices: readonly NamedDevice[], stored: string | undefined, loaded: boolean): boolean {
  return loaded && !!stored && !devices.some((d) => d.id === stored);
}

/** Display name of a stored id that is not in the list (`USB Audio#2` → `USB Audio`). */
export function storedName(stored: string): string {
  return stored.replace(/#\d+$/, "");
}

export interface PreviewFrame { gen: number; jpeg: Uint8Array }

/** Splits a preview answer; `null` for the empty "nothing newer" answer or a broken one. */
export function parsePreviewFrame(buf: ArrayBuffer): PreviewFrame | null {
  if (buf.byteLength <= 8) return null;
  const view = new DataView(buf);
  const gen = Number(view.getBigUint64(0, true));
  const jpeg = new Uint8Array(buf, 8);
  // A JPEG starts with FF D8; anything else is not shown.
  if (jpeg[0] !== 0xff || jpeg[1] !== 0xd8) return null;
  return { gen, jpeg };
}

export interface Peak { level: number; heldMs: number }

/** The peak marker after `dtMs` with the bar at `level`: jumps up at once, holds, then falls. */
export function peakStep(prev: Peak, level: number, dtMs: number): Peak {
  const l = Math.min(1, Math.max(0, Number.isFinite(level) ? level : 0));
  if (l >= prev.level) return { level: l, heldMs: 0 };
  const held = prev.heldMs + Math.max(0, dtMs);
  if (held <= PEAK_HOLD_MS) return { level: prev.level, heldMs: held };
  const fallen = prev.level - (PEAK_FALL_PER_S * Math.max(0, dtMs)) / 1000;
  return { level: Math.max(l, fallen), heldMs: held };
}
