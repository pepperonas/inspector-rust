/**
 * `logi` — Logitech mouse settings (pure helpers; the device lives in Rust,
 * `core/rust-lib/src/logi/`). The constants below mirror the Rust side and
 * are pinned against it in `logi.test.ts`.
 */
import type { BindingAction } from "./gesture-bindings";
import type { MessageKey } from "./i18n";

export const SMART_MIN = 1;
export const SMART_MAX = 50;

export type WheelMode = "free" | "ratchet" | "smart";

export interface LogiWheel {
  mode: WheelMode;
  threshold: number;
}

export type ButtonSlot = "press" | "up" | "down" | "left" | "right";
export const DIRECTION_SLOTS: ButtonSlot[] = ["up", "down", "left", "right"];

export type ButtonConfig = Partial<Record<ButtonSlot, BindingAction>>;

export interface LogiConfig {
  dpi: number | null;
  wheel: LogiWheel | null;
  invert: boolean | null;
  /** Control id (decimal) → actions. */
  buttons: Record<string, ButtonConfig>;
}

export interface LogiControl {
  cid: number;
  divertable: boolean;
}

export interface LogiStatus {
  supported: boolean;
  connected: boolean;
  permission_denied: boolean;
  name: string;
  battery: { percent: number; charging: boolean } | null;
  dpi: { current: number; default: number; options: number[] } | null;
  wheel: LogiWheel | null;
  invert: boolean | null;
  controls: LogiControl[];
  options_running: boolean;
  /** A mouse is there, but macOS refuses requests to it right now (retried by Rust). */
  refused?: boolean;
  /** While `refused`: Secure Input is on, which locks keyboard-like devices. */
  secure_input?: boolean;
}

export interface LogiView {
  status: LogiStatus;
  config: LogiConfig;
}

/** Control id → catalogue key. Same names as Rust's `control_name`. */
const BUTTON_NAMES: Record<number, string> = {
  0x50: "left",
  0x51: "right",
  0x52: "middle",
  0x53: "back",
  0x56: "forward",
  0xc3: "gesture",
  0xc4: "wheel_mode",
  0xd7: "host",
};

export function buttonName(cid: number): string {
  return BUTTON_NAMES[cid] ?? "other";
}

export function buttonLabelKey(cid: number): MessageKey {
  return `logi.btn.${buttonName(cid)}` as MessageKey;
}

/** The order the panel lists buttons in; unknown ones follow by id. */
const ORDER = [0xc3, 0x52, 0x53, 0x56, 0xc4];

/** The buttons that can be given an action, thumb button first. */
export function assignableControls(controls: LogiControl[]): LogiControl[] {
  const rank = (c: LogiControl) => {
    const i = ORDER.indexOf(c.cid);
    return i === -1 ? ORDER.length : i;
  };
  return controls.filter((c) => c.divertable).sort((a, b) => rank(a) - rank(b) || a.cid - b.cid);
}

/** `cfg` with one slot of one button set (or cleared with `null`). A button
 *  left with nothing is removed, which hands it back to the mouse. */
export function withAction(cfg: LogiConfig, cid: number, slot: ButtonSlot, action: BindingAction | null): LogiConfig {
  const key = String(cid);
  const button: ButtonConfig = { ...(cfg.buttons[key] ?? {}) };
  if (action) button[slot] = action;
  else delete button[slot];
  const buttons = { ...cfg.buttons };
  if (Object.keys(button).length) buttons[key] = button;
  else delete buttons[key];
  return { ...cfg, buttons };
}

export function hasDirections(b: ButtonConfig | undefined): boolean {
  return !!b && DIRECTION_SLOTS.some((s) => b[s]);
}

/** An action the backend would refuse (a target still to be filled in). */
export function actionComplete(a: BindingAction): boolean {
  switch (a.type) {
    case "hotkey":
      return a.action !== "";
    case "shortcut":
      return a.keys !== "";
    case "open":
      return a.target.trim() !== "";
    case "task":
      return a.id > 0;
    default:
      return true;
  }
}

/** Whether `cfg` can be saved as it stands. */
export function configComplete(cfg: LogiConfig): boolean {
  return Object.values(cfg.buttons).every((b) => Object.values(b).every((a) => !a || actionComplete(a)));
}

export function clampThreshold(n: number): number {
  if (!Number.isFinite(n)) return SMART_MIN;
  return Math.max(SMART_MIN, Math.min(SMART_MAX, Math.round(n)));
}

/** The wheel setting the panel shows: the stored one, else what the mouse
 *  reports. */
export function shownWheel(cfg: LogiConfig, status: LogiStatus): LogiWheel | null {
  return cfg.wheel ?? status.wheel;
}

export function shownDpi(cfg: LogiConfig, status: LogiStatus): number | null {
  return status.dpi?.current ?? cfg.dpi;
}

export type BatteryLevel = "low" | "mid" | "ok";

export function batteryLevel(percent: number): BatteryLevel {
  if (percent <= 15) return "low";
  if (percent <= 40) return "mid";
  return "ok";
}

export const EMPTY_CONFIG: LogiConfig = { dpi: null, wheel: null, invert: null, buttons: {} };
