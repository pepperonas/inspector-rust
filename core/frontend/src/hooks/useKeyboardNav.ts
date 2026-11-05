import { useCallback, useEffect } from "react";

interface Args {
  length: number;
  selected: number;
  setSelected: (i: number) => void;
  /** Called on Enter / Shift+Enter / Alt+Enter / Cmd+Shift+Enter. The
   *  booleans tell the caller which modifiers were held — the activate
   *  logic uses Shift to pick `paste_entry_formatted` over `paste_entry`,
   *  Alt to switch the pwgen mode to alphanumeric-only on copy, and
   *  Cmd/Ctrl+Shift for figlet's save-PNG-to-Downloads path. */
  onEnter: (shiftKey: boolean, altKey: boolean, metaKey: boolean) => void;
  onEscape: () => void;
  /** Shift+ArrowUp / Shift+ArrowDown. When provided, holding Shift
   *  while pressing an arrow calls this *instead of* moving the list
   *  selection — wired to system-volume control. */
  onShiftArrow?: (direction: "up" | "down") => void;
  /** When false, the listener no-ops — used to fully hand keyboard
   *  control to a takeover surface (e.g. the `getshaky` Pong game)
   *  without unmounting the hook (hooks can't be called conditionally). */
  enabled?: boolean;
  /** Key-repeat guard (animation layer): called with `e.repeat` on every
   *  ArrowUp/ArrowDown navigation — the caller switches the selection
   *  indicator (and the preview crossfade) to instant while a held arrow
   *  auto-repeats, so neither lags behind fast navigation. */
  onNavRepeat?: (repeating: boolean) => void;
}

/** Marks the popup's main search field: the only text field the list keys may drive. */
export const MAIN_SEARCH_ATTR = "data-main-search";

/** A key typed into some OTHER text field (clip note, hex value, link box, time
 *  field): it belongs to that field — Enter must not paste the selected row, the
 *  arrows must move the caret, Esc must reach the field's own handler. */
function typedElsewhere(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement) || t.hasAttribute(MAIN_SEARCH_ATTR)) return false;
  if (t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable) return true;
  if (t.tagName !== "INPUT") return false;
  const type = (t as HTMLInputElement).type;
  return !["range", "checkbox", "radio", "button"].includes(type);
}

export function useKeyboardNav({
  length,
  selected,
  setSelected,
  onEnter,
  onEscape,
  onShiftArrow,
  enabled = true,
  onNavRepeat,
}: Args) {
  const handler = useCallback(
    (e: KeyboardEvent) => {
      if (!enabled) return;
      // Already handled, mid-IME-composition, or typed into another field.
      if (e.defaultPrevented || e.isComposing || typedElsewhere(e.target)) return;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        // Shift+↓ → volume down (not list navigation).
        if (e.shiftKey) {
          onShiftArrow?.("down");
          return;
        }
        if (length === 0) return;
        onNavRepeat?.(e.repeat);
        setSelected(Math.min(selected + 1, length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        // Shift+↑ → volume up.
        if (e.shiftKey) {
          onShiftArrow?.("up");
          return;
        }
        if (length === 0) return;
        onNavRepeat?.(e.repeat);
        setSelected(Math.max(selected - 1, 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        // A held Enter must not activate again while the first one is closing the popup.
        if (length > 0 && !e.repeat) onEnter(e.shiftKey, e.altKey, e.metaKey || e.ctrlKey);
      } else if (e.key === "Escape") {
        e.preventDefault();
        if (!e.repeat) onEscape(); // key-repeat must not queue extra hides
      }
    },
    [length, selected, setSelected, onEnter, onEscape, onShiftArrow, enabled, onNavRepeat],
  );

  useEffect(() => {
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [handler]);
}
