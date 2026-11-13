/**
 * One key rule for every preview panel.
 *
 * The search field keeps the DOM focus while a panel owns the keyboard, so a
 * panel can't simply ignore keys that come from an input — then none of its
 * keys would ever arrive. And it can't take everything either, or it would
 * swallow letters the user is typing into the search field (`weather
 * darmstadt` losing its "r").
 *
 * The rule: text stays text. From the search field, a printable key and
 * ⌫/Delete without ⌘/Ctrl are typing; arrows, Enter, Home/End, Page keys and
 * every ⌘/Ctrl chord belong to the panel. Any other text field keeps all of
 * its keys.
 */
import { MAIN_SEARCH_ATTR } from "../hooks/useKeyboardNav";

export type KeyOrigin = "search" | "field" | "page";

interface KeyLike {
  key: string;
  target: EventTarget | null;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
}

export function keyOrigin(target: EventTarget | null): KeyOrigin {
  const el = target as HTMLElement | null;
  if (!el || typeof el.tagName !== "string") return "page";
  if (el.tagName === "INPUT" && el.hasAttribute(MAIN_SEARCH_ATTR)) return "search";
  if (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable) {
    return "field";
  }
  return "page";
}

/** True when the key is text entry (or editing) for the field it came from —
 *  a panel must leave it alone. */
export function isTyping(e: KeyLike): boolean {
  const origin = keyOrigin(e.target);
  if (origin === "field") return true;
  if (origin === "page") return false;
  if (e.metaKey || e.ctrlKey) return false;
  return e.key.length === 1 || e.key === "Backspace" || e.key === "Delete";
}

export function fromSearch(e: KeyLike): boolean {
  return keyOrigin(e.target) === "search";
}

/** A letter shortcut: the bare key (only reachable when `isTyping` let it
 *  through, i.e. not from a text field) or ⌘/Ctrl + the key. */
export function hitKey(e: KeyLike, key: string): boolean {
  return !e.altKey && e.key.toLowerCase() === key;
}

/** ⌘ (macOS) or Ctrl held, without Alt. */
export function withMod(e: KeyLike): boolean {
  return (e.metaKey || e.ctrlKey) && !e.altKey;
}
