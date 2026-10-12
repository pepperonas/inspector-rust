/**
 * Pure helpers for the `adb` panel (v0.119.0): the keycode catalogue, package
 * filtering, formatting and input validation. Mirrors the ranges the Rust
 * side enforces (adb.rs validators) so the UI can reject before the IPC.
 */

import { t } from "./i18n";

export interface AdbKeyDef {
  label: string;
  code: string;
}

/** Navigation keys — the ADBOSS Input-Remote set. */
export const NAV_KEYS: AdbKeyDef[] = [
  { get label() { return t("adb.key.home"); }, code: "KEYCODE_HOME" },
  { get label() { return t("adb.key.back"); }, code: "KEYCODE_BACK" },
  { get label() { return t("adb.key.recents"); }, code: "KEYCODE_APP_SWITCH" },
  { get label() { return t("adb.key.menu"); }, code: "KEYCODE_MENU" },
  { get label() { return t("adb.key.power"); }, code: "KEYCODE_POWER" },
  { get label() { return t("adb.key.vol_down"); }, code: "KEYCODE_VOLUME_DOWN" },
  { get label() { return t("adb.key.vol_up"); }, code: "KEYCODE_VOLUME_UP" },
  { get label() { return t("adb.key.mute"); }, code: "KEYCODE_VOLUME_MUTE" },
];

/** D-pad (grid positions handled by the component). */
export const DPAD_KEYS: Record<string, AdbKeyDef> = {
  up: { label: "▲", code: "KEYCODE_DPAD_UP" },
  down: { label: "▼", code: "KEYCODE_DPAD_DOWN" },
  left: { label: "◀", code: "KEYCODE_DPAD_LEFT" },
  right: { label: "▶", code: "KEYCODE_DPAD_RIGHT" },
  center: { label: "OK", code: "KEYCODE_DPAD_CENTER" },
  enter: { label: "⏎", code: "KEYCODE_ENTER" },
};

/** Package filter: every whitespace term must appear; prefix matches rank
 *  before infix (deterministic, stable within rank). */
export function filterPackages(pkgs: readonly string[], query: string): string[] {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return [...pkgs];
  const matches = pkgs.filter((p) => {
    const lower = p.toLowerCase();
    return terms.every((t) => lower.includes(t));
  });
  const first = terms[0];
  return matches.sort((a, b) => {
    const ap = a.toLowerCase().startsWith(first) ? 0 : 1;
    const bp = b.toLowerCase().startsWith(first) ? 0 : 1;
    return ap - bp || a.localeCompare(b);
  });
}

/** kB (adb's meminfo/df unit) → human string via 1024 ladder. */
export function kbHuman(kb: number): string {
  if (!Number.isFinite(kb) || kb <= 0) return "0 B";
  const units = ["KB", "MB", "GB", "TB"];
  let v = kb;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v < 10 && i > 0 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

/** Uptime seconds → "3d 4h" / "4h 12m" / "12m". */
export function uptimeHuman(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return "—";
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

/** Device row label: model + transport + state warning. */
export function deviceLabel(d: { serial: string; model: string; state: string; wifi: boolean }): string {
  const base = d.model !== "unknown" ? d.model : d.serial;
  const transport = d.wifi ? ` · ${t("adb.view.wifi")}` : "";
  if (d.state === "unauthorized") return `${base}${transport} — ${t("adb.unauthorized")}`;
  if (d.state === "offline") return `${base}${transport} — ${t("adb.offline")}`;
  return `${base}${transport}`;
}

/** Mirrors adb.rs range checks so the form can validate before the IPC. */
export function validTap(x: number, y: number): boolean {
  const ok = (v: number) => Number.isInteger(v) && v >= 0 && v <= 9999;
  return ok(x) && ok(y);
}
export function validSwipe(x1: number, y1: number, x2: number, y2: number, durMs: number): boolean {
  return (
    validTap(x1, y1) && validTap(x2, y2) && Number.isInteger(durMs) && durMs >= 50 && durMs <= 5000
  );
}

/** `input text` is ASCII-only (the Rust side rejects too) — pre-check so the
 *  hint appears while typing, not after a failed round trip. */
export function textSendable(text: string): boolean {
  return text.length > 0 && /^[\x20-\x7e]+$/.test(text);
}
