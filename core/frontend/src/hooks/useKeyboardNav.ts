import { useCallback, useEffect } from "react";

interface Args {
  length: number;
  selected: number;
  setSelected: (i: number) => void;
  /** Called on Enter or Shift+Enter. The boolean argument tells the
   *  caller whether Shift was held — the activate logic uses that to
   *  pick `paste_entry_formatted` over `paste_entry`. */
  onEnter: (shiftKey: boolean) => void;
  onEscape: () => void;
  /** Shift+ArrowUp / Shift+ArrowDown. When provided, holding Shift
   *  while pressing an arrow calls this *instead of* moving the list
   *  selection — wired to system-volume control. */
  onShiftArrow?: (direction: "up" | "down") => void;
  /** When false, the listener no-ops — used to fully hand keyboard
   *  control to a takeover surface (e.g. the `getshaky` Pong game)
   *  without unmounting the hook (hooks can't be called conditionally). */
  enabled?: boolean;
}

export function useKeyboardNav({
  length,
  selected,
  setSelected,
  onEnter,
  onEscape,
  onShiftArrow,
  enabled = true,
}: Args) {
  const handler = useCallback(
    (e: KeyboardEvent) => {
      if (!enabled) return;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        // Shift+↓ → volume down (not list navigation).
        if (e.shiftKey) {
          onShiftArrow?.("down");
          return;
        }
        if (length === 0) return;
        setSelected(Math.min(selected + 1, length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        // Shift+↑ → volume up.
        if (e.shiftKey) {
          onShiftArrow?.("up");
          return;
        }
        if (length === 0) return;
        setSelected(Math.max(selected - 1, 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        if (length > 0) onEnter(e.shiftKey);
      } else if (e.key === "Escape") {
        e.preventDefault();
        onEscape();
      }
    },
    [length, selected, setSelected, onEnter, onEscape, onShiftArrow, enabled],
  );

  useEffect(() => {
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [handler]);
}
