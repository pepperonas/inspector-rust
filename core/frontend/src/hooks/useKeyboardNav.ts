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
  enabled = true,
}: Args) {
  const handler = useCallback(
    (e: KeyboardEvent) => {
      if (!enabled) return;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        if (length === 0) return;
        setSelected(Math.min(selected + 1, length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
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
    [length, selected, setSelected, onEnter, onEscape, enabled],
  );

  useEffect(() => {
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [handler]);
}
