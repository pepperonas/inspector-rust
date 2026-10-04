/**
 * Gesture → action bindings (the bindings editor in Settings → Touchpad
 * gestures). Mirrors `core/rust-lib/src/gestures/bindings.rs`; Rust validates
 * on save, these helpers only label and pre-check so the editor can explain
 * a problem before the round trip.
 */
import type { GestureKindName, GestureLogEntry } from "./ipc";
import { formatHotkey } from "./platform";

/** Swipes and taps take 3–5 fingers (mirrors `MIN_FINGERS`/`MAX_FINGERS`). */
export const MIN_FINGERS = 3;
export const MAX_FINGERS = 5;
/** Tip-taps have a fixed posture; their trigger stores this count. */
export const TIPTAP_FINGERS = 2;

export interface Trigger {
  kind: GestureKindName;
  fingers: number;
}

export type BindingAction =
  | { type: "volume_up" }
  | { type: "volume_down" }
  | { type: "mute_toggle" }
  | { type: "next_tab" }
  | { type: "prev_tab" }
  | { type: "hotkey"; action: string }
  | { type: "shortcut"; keys: string }
  | { type: "open"; target: string }
  | { type: "task"; id: number };

export type BindingActionType = BindingAction["type"];

export interface GestureBinding {
  id: string;
  trigger: Trigger;
  action: BindingAction;
  /** Only while this app is in front (macOS bundle id). */
  app: string | null;
  /** Display name for `app`. */
  app_name: string | null;
  enabled: boolean;
  typing_guard: boolean;
}

export interface GestureBindingsView {
  bindings: GestureBinding[];
  /** The user saved a list (otherwise: the built-in set). */
  customised: boolean;
  min_fingers: number;
  max_fingers: number;
}

export const TRIGGER_KINDS: { kind: GestureKindName; label: string }[] = [
  { kind: "swipe_up", label: "Wischen nach oben" },
  { kind: "swipe_down", label: "Wischen nach unten" },
  { kind: "swipe_left", label: "Wischen nach links" },
  { kind: "swipe_right", label: "Wischen nach rechts" },
  { kind: "tap", label: "Tippen" },
  { kind: "tip_tap_left", label: "Tip-Tap links" },
  { kind: "tip_tap_right", label: "Tip-Tap rechts" },
];

export const ACTION_TYPES: { type: BindingActionType; label: string }[] = [
  { type: "volume_up", label: "Lauter" },
  { type: "volume_down", label: "Leiser" },
  { type: "mute_toggle", label: "Stumm an/aus" },
  { type: "next_tab", label: "Nächster Tab" },
  { type: "prev_tab", label: "Vorheriger Tab" },
  { type: "shortcut", label: "Tastenkürzel senden" },
  { type: "hotkey", label: "Inspector-Rust-Aktion" },
  { type: "open", label: "Adresse oder Datei öffnen" },
  { type: "task", label: "KI-Task ausführen" },
];

export function isTipTap(kind: GestureKindName): boolean {
  return kind === "tip_tap_left" || kind === "tip_tap_right";
}

export function triggerLabel(t: Trigger): string {
  const name = TRIGGER_KINDS.find((k) => k.kind === t.kind)?.label ?? t.kind;
  if (isTipTap(t.kind)) return name;
  return `${t.fingers} Finger · ${name}`;
}

/** What a tip-tap is, in one line — the editor shows it under the picker. */
export function tipTapHint(kind: GestureKindName): string | null {
  if (kind === "tip_tap_left") return "Ein Finger ruht, ein zweiter tippt links daneben.";
  if (kind === "tip_tap_right") return "Ein Finger ruht, ein zweiter tippt rechts daneben.";
  return null;
}

export interface ActionContext {
  /** Global actions: id → label. */
  hotkeys?: Record<string, string>;
  /** AI tasks: id → name. */
  tasks?: Record<number, string>;
}

export function actionLabel(a: BindingAction, ctx: ActionContext = {}): string {
  switch (a.type) {
    case "hotkey":
      return ctx.hotkeys?.[a.action] ?? a.action;
    case "shortcut":
      return a.keys ? `Kürzel ${formatHotkey(a.keys)}` : "Kürzel (leer)";
    case "open":
      return a.target ? `Öffnen: ${shortTarget(a.target)}` : "Öffnen (leer)";
    case "task":
      return `KI-Task: ${ctx.tasks?.[a.id] ?? `#${a.id}`}`;
    default:
      return ACTION_TYPES.find((t) => t.type === a.type)?.label ?? a.type;
  }
}

/** `https://www.example.com/a` → `example.com/a`; a path keeps its last part. */
export function shortTarget(target: string): string {
  const t = target.trim();
  const url = t.replace(/^https?:\/\/(www\.)?/i, "");
  if (url !== t) return url.replace(/\/$/, "");
  const parts = t.split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : t;
}

/** A fresh action of `type`, carrying over nothing from the old one. */
export function emptyAction(type: BindingActionType): BindingAction {
  switch (type) {
    case "hotkey":
      return { type, action: "" };
    case "shortcut":
      return { type, keys: "" };
    case "open":
      return { type, target: "" };
    case "task":
      return { type, id: 0 };
    default:
      return { type };
  }
}

/** Tabs send keystrokes themselves and run in bursts — exempt by default. */
export function defaultTypingGuard(a: BindingAction): boolean {
  return a.type !== "next_tab" && a.type !== "prev_tab";
}

export function newBinding(): GestureBinding {
  const action: BindingAction = { type: "next_tab" };
  return {
    id: "",
    trigger: { kind: "swipe_left", fingers: 3 },
    action,
    app: null,
    app_name: null,
    enabled: true,
    typing_guard: defaultTypingGuard(action),
  };
}

/** Two triggers fire on the same physical gesture (mirrors `Trigger::same_as`). */
export function sameTrigger(a: Trigger, b: Trigger): boolean {
  return a.kind === b.kind && (isTipTap(a.kind) || a.fingers === b.fingers);
}

function sameScope(a: GestureBinding, b: GestureBinding): boolean {
  if (!a.app && !b.app) return true;
  if (!a.app || !b.app) return false;
  return a.app.trim().toLowerCase() === b.app.trim().toLowerCase();
}

/** Ids of active bindings that share a gesture in the same scope — what the
 *  backend refuses on save (mirrors `validate`). */
export function conflictingIds(list: GestureBinding[]): Set<string> {
  const out = new Set<string>();
  list.forEach((a, i) => {
    list.slice(i + 1).forEach((b) => {
      if (a.enabled && b.enabled && sameScope(a, b) && sameTrigger(a.trigger, b.trigger)) {
        out.add(a.id);
        out.add(b.id);
      }
    });
  });
  return out;
}

/** What still has to be filled in before a binding can be saved, or null. */
export function missingField(b: GestureBinding): string | null {
  const a = b.action;
  if (b.app !== null && !b.app.trim()) return "App auswählen";
  if (a.type === "hotkey" && !a.action) return "Aktion auswählen";
  if (a.type === "shortcut" && !a.keys) return "Tastenkürzel aufnehmen";
  if (a.type === "open" && !a.target.trim()) return "Adresse oder Pfad eingeben";
  if (a.type === "task" && a.id <= 0) return "KI-Task auswählen";
  if (!isTipTap(b.trigger.kind) && (b.trigger.fingers < MIN_FINGERS || b.trigger.fingers > MAX_FINGERS))
    return `${MIN_FINGERS}–${MAX_FINGERS} Finger`;
  return null;
}

/** Swipes can also be bound by macOS itself (System Settings → Trackpad →
 *  More Gestures); the editor says so instead of guessing which ones. */
export function mayCollideWithSystem(t: Trigger): boolean {
  return t.kind.startsWith("swipe_") || (t.kind === "tap" && t.fingers >= 4);
}

export type CaptureResult = { trigger: Trigger } | { error: string };

/** The gesture the user just demonstrated: the newest log entry with a
 *  gesture after `sinceSeq` (newest first, as the live snapshot delivers). */
export function captureFromLog(log: GestureLogEntry[], sinceSeq: number): CaptureResult | null {
  const hit = log.find((e) => e.seq > sinceSeq && e.kind !== null);
  if (!hit || !hit.kind) return null;
  if (isTipTap(hit.kind)) return { trigger: { kind: hit.kind, fingers: TIPTAP_FINGERS } };
  const fingers = hit.fingers ?? 0;
  if (fingers < MIN_FINGERS)
    return { error: `Erkannt: ${fingers} Finger — zwei Finger sind Scrollen und Rechtsklick, nimm 3 oder mehr.` };
  if (fingers > MAX_FINGERS) return { error: `Erkannt: ${fingers} Finger — höchstens ${MAX_FINGERS}.` };
  return { trigger: { kind: hit.kind, fingers } };
}

/** The newest sequence number in a log (0 when empty). */
export function latestSeq(log: GestureLogEntry[]): number {
  return log.reduce((m, e) => Math.max(m, e.seq), 0);
}
