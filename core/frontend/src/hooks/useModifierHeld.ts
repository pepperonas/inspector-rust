import { useEffect, useState } from "react";

/**
 * Tracks whether the platform-modifier (Cmd on macOS, Ctrl on
 * Win/Linux) is currently held down. Use to gate UI overlays on the
 * same modifier the keyboard-shortcut handlers fire on — e.g. the
 * preview-pane Transform chips, where the chip list should appear
 * only while the user is holding Cmd / Ctrl waiting to press a digit.
 *
 * Resets to `false` on window blur to dodge the "stuck modifier"
 * problem: user holds Cmd, hits Cmd+Tab, switches apps → the keyup
 * never fires in our window and the overlay would stay on forever.
 */
export function useModifierHeld(): boolean {
  const [held, setHeld] = useState(false);
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      // Don't gate on `e.metaKey` alone: when the user presses *only*
      // Cmd (no other key), `keydown` fires with `key === "Meta"` and
      // `metaKey === true` — both conditions match. But for safety
      // against future key-event quirks, accept either signal.
      if (e.metaKey || e.ctrlKey || e.key === "Meta" || e.key === "Control") {
        setHeld(true);
      }
    };
    const up = (e: KeyboardEvent) => {
      // On Cmd-release the `metaKey` flag is `false` (the modifier
      // just became inactive). Track the released key directly so we
      // catch the exact transition rather than re-reading the flag.
      if (e.key === "Meta" || e.key === "Control") setHeld(false);
    };
    const blur = () => setHeld(false);
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
      window.removeEventListener("blur", blur);
    };
  }, []);
  return held;
}
