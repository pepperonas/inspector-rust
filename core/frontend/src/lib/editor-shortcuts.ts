// The record editor's key map: one table of actions → keys, freely rebindable.
//
// Every editor key handler asks `matchAction`; nothing compares `e.key` against a letter of its
// own. The few keys that are not rebindable (undo/redo, frame stepping, annotation walking,
// Escape) live in `isFixed` and are refused as a binding.
import { isTypingTarget } from "./editor";
import type { MessageKey } from "./i18n";

/** A key with its modifiers. `ctrl` means ⌘ on the Mac (Ctrl elsewhere). */
export type Binding = { key: string; ctrl?: boolean; shift?: boolean; alt?: boolean };

export type ActionId =
  | "playPause"
  | "split"
  | "crop"
  | "addZoom"
  | "addAnnotation"
  | "addAudio"
  | "addVoiceover"
  | "duplicate"
  | "deleteSelected"
  | "copy"
  | "cut"
  | "paste"
  | "openProject"
  | "showShortcuts";

/** Every action's key; `null` = not assigned (it lost its key to another action). */
export type Bindings = Record<ActionId, Binding | null>;

/** Display order (also the order `matchAction` tries them in). */
export const ACTIONS: readonly ActionId[] = [
  "playPause",
  "split",
  "crop",
  "addZoom",
  "addAnnotation",
  "addAudio",
  "addVoiceover",
  "duplicate",
  "deleteSelected",
  "copy",
  "cut",
  "paste",
  "openProject",
  "showShortcuts",
];

export const DEFAULT_BINDINGS: Record<ActionId, Binding> = {
  playPause: { key: "Space" },
  split: { key: "s" },
  crop: { key: "c" },
  addZoom: { key: "z" },
  addAnnotation: { key: "a" },
  addAudio: { key: "m" },
  addVoiceover: { key: "v" },
  duplicate: { key: "d", ctrl: true },
  deleteSelected: { key: "Backspace" },
  copy: { key: "c", ctrl: true },
  cut: { key: "x", ctrl: true },
  paste: { key: "v", ctrl: true },
  openProject: { key: "o", ctrl: true },
  showShortcuts: { key: "?" },
};

export const ACTION_LABEL: Record<ActionId, MessageKey> = {
  playPause: "rec4.keys.playPause",
  split: "rec4.keys.split",
  crop: "rec4.keys.crop",
  addZoom: "rec4.keys.addZoom",
  addAnnotation: "rec4.keys.addAnnotation",
  addAudio: "rec4.keys.addAudio",
  addVoiceover: "rec4.keys.addVoiceover",
  duplicate: "rec4.keys.duplicate",
  deleteSelected: "rec4.keys.deleteSelected",
  copy: "rec4.keys.copy",
  cut: "rec4.keys.cut",
  paste: "rec4.keys.paste",
  openProject: "rec4.keys.openProject",
  showShortcuts: "rec4.keys.showShortcuts",
};

/** The keys that are not rebindable, for the dialog's list. */
export const FIXED_KEYS: readonly { keys: readonly Binding[]; label: MessageKey }[] = [
  { keys: [{ key: "z", ctrl: true }], label: "rec4.keys.fixed_undo" },
  { keys: [{ key: "z", ctrl: true, shift: true }], label: "rec4.keys.fixed_redo" },
  { keys: [{ key: "ArrowLeft" }, { key: "ArrowRight" }], label: "rec4.keys.fixed_step" },
  { keys: [{ key: "ArrowLeft", shift: true }, { key: "ArrowRight", shift: true }], label: "rec4.keys.fixed_step_second" },
  { keys: [{ key: "Tab" }, { key: "Tab", shift: true }], label: "rec4.keys.fixed_tab" },
  { keys: [{ key: "Escape" }], label: "rec4.keys.fixed_escape" },
];

const MODIFIER_KEYS = new Set(["Shift", "Meta", "Control", "Alt", "AltGraph", "CapsLock", "Fn", "OS", "Hyper", "Super"]);

/** A single printable character that is not a letter or digit carries its own shift. */
const ownsShift = (k: string) => [...k].length === 1 && !/[\p{L}\p{N}]/u.test(k);

/** The canonical key name: letters lowercase, Space spelled out, Delete = Backspace. */
function canonicalKey(k: string): string {
  if (k === " " || k === "Spacebar") return "Space";
  if (k === "Delete") return "Backspace";
  return [...k].length === 1 ? k.toLowerCase() : k;
}

function normalize(b: Binding): Binding {
  const key = canonicalKey(b.key);
  const out: Binding = { key };
  if (b.ctrl) out.ctrl = true;
  if (b.shift && !ownsShift(key)) out.shift = true;
  if (b.alt) out.alt = true;
  return out;
}

export function sameBinding(a: Binding | null, b: Binding | null): boolean {
  if (!a || !b) return false;
  const x = normalize(a);
  const y = normalize(b);
  return x.key === y.key && !!x.ctrl === !!y.ctrl && !!x.shift === !!y.shift && !!x.alt === !!y.alt;
}

/** The binding a key press spells, or `null` for a lone modifier. */
export function bindingFromEvent(e: KeyboardEvent): Binding | null {
  if (!e.key || MODIFIER_KEYS.has(e.key) || e.key === "Dead" || e.key === "Unidentified") return null;
  return normalize({ key: e.key, ctrl: e.metaKey || e.ctrlKey, shift: e.shiftKey, alt: e.altKey });
}

/** Undo/redo, frame stepping, annotation walking and Escape are not rebindable. */
export function isFixed(b: Binding): boolean {
  const n = normalize(b);
  if (n.key === "ArrowLeft" || n.key === "ArrowRight" || n.key === "Tab" || n.key === "Escape") return true;
  return n.key === "z" && !!n.ctrl && !n.alt;
}

/** Controls that Enter / Space press by themselves. */
function pressesItself(t: EventTarget | null): boolean {
  if (!(t instanceof Element)) return false;
  if (t.closest("button, a[href], summary, [role=button], [role=switch], [role=checkbox], [role=tab]")) return true;
  if (t instanceof HTMLInputElement) return ["button", "submit", "reset", "checkbox", "radio"].includes(t.type);
  return false;
}

/**
 * The action a key press triggers. Nothing while typing in a text field, and never Enter or
 * Space on a focused button: those press the button, whatever they are bound to.
 */
export function matchAction(e: KeyboardEvent, bindings: Bindings): ActionId | null {
  if (isTypingTarget(e.target)) return null;
  if ((e.key === "Enter" || e.key === " ") && pressesItself(e.target)) return null;
  const b = bindingFromEvent(e);
  if (!b) return null;
  return ACTIONS.find((a) => sameBinding(bindings[a], b)) ?? null;
}

export type Conflict = { kind: "fixed" } | { kind: "action"; action: ActionId };

/** What giving `action` the key `binding` would collide with. */
export function findConflict(bindings: Bindings, action: ActionId, binding: Binding): Conflict | null {
  if (isFixed(binding)) return { kind: "fixed" };
  const other = ACTIONS.find((a) => a !== action && sameBinding(bindings[a], binding));
  return other ? { kind: "action", action: other } : null;
}

/** Give `action` the key; whoever held it gets `action`'s old key (the swap). */
export function swapBinding(bindings: Bindings, action: ActionId, binding: Binding): Bindings {
  const next = { ...bindings, [action]: normalize(binding) };
  const c = findConflict(bindings, action, binding);
  if (c?.kind === "action") next[c.action] = bindings[action];
  return next;
}

function asBinding(v: unknown): Binding | null {
  if (!v || typeof v !== "object" || Array.isArray(v)) return null;
  const o = v as Record<string, unknown>;
  if (typeof o.key !== "string" || !o.key || MODIFIER_KEYS.has(o.key)) return null;
  for (const f of ["ctrl", "shift", "alt"]) if (o[f] !== undefined && typeof o[f] !== "boolean") return null;
  return normalize({ key: o.key, ctrl: o.ctrl as boolean, shift: o.shift as boolean, alt: o.alt as boolean });
}

/**
 * The stored deviations laid over the defaults. Anything unusable keeps its default. A stored key
 * wins over another action's default: that action is left unassigned (`null`), never silently
 * unreachable behind a duplicate. `null` stored = deliberately unassigned.
 */
export function mergeWithDefaults(stored: unknown): Bindings {
  const out: Bindings = { ...DEFAULT_BINDINGS };
  if (!stored || typeof stored !== "object" || Array.isArray(stored)) return out;
  const s = stored as Record<string, unknown>;
  const fromStore = new Set<ActionId>();
  for (const a of ACTIONS) {
    if (s[a] === null) {
      out[a] = null;
      fromStore.add(a);
      continue;
    }
    const b = asBinding(s[a]);
    if (!b || isFixed(b)) continue;
    // Two stored keys on one chord: the first (display order) keeps it.
    if (ACTIONS.some((o) => fromStore.has(o) && sameBinding(out[o], b))) continue;
    out[a] = b;
    fromStore.add(a);
  }
  for (const a of ACTIONS) {
    if (fromStore.has(a)) continue;
    if (ACTIONS.some((o) => fromStore.has(o) && sameBinding(out[o], out[a]))) out[a] = null;
  }
  return out;
}

/** Only what differs from the defaults is stored, so a later default change still arrives. */
export function storedDeviations(bindings: Bindings): Partial<Bindings> {
  const out: Partial<Bindings> = {};
  for (const a of ACTIONS) {
    const b = bindings[a];
    if (!b) out[a] = null;
    else if (!sameBinding(b, DEFAULT_BINDINGS[a])) out[a] = normalize(b);
  }
  return out;
}

const KEY_GLYPH: Record<string, string> = {
  Backspace: "⌫",
  Enter: "↩",
  Escape: "Esc",
  Tab: "⇥",
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
};

/** "⌥⇧⌘X" — the Mac's modifier order. `space` is the localized name of the space bar. */
export function formatBinding(b: Binding, space = "Space"): string {
  const n = normalize(b);
  const key = n.key === "Space" ? space : (KEY_GLYPH[n.key] ?? ([...n.key].length === 1 ? n.key.toUpperCase() : n.key));
  return `${n.alt ? "⌥" : ""}${n.shift ? "⇧" : ""}${n.ctrl ? "⌘" : ""}${key}`;
}
