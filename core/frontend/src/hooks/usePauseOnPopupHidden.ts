import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

/**
 * Re-engage a game's resume-gate when the popup is hidden mid-play (e.g. the
 * user clicked outside the overlay, which hides the window on focus loss).
 * On the next open the game stays frozen behind the "press a key to continue"
 * gate — the same behaviour as pausing with Esc.
 *
 * @param playing     whether the game is currently in its playing phase
 * @param engageGate  re-arms the resume gate (sets the gate ref + state true)
 */
export function usePauseOnPopupHidden(playing: boolean, engageGate: () => void) {
  // Mirror the latest values into refs so the (once-installed) listener always
  // sees current state without re-subscribing.
  const playingRef = useRef(playing);
  const engageRef = useRef(engageGate);
  useEffect(() => {
    playingRef.current = playing;
    engageRef.current = engageGate;
  }, [playing, engageGate]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen("popup-hidden", () => {
      if (playingRef.current) engageRef.current();
    }).then((u) => {
      // If the component already unmounted before the listener resolved, undo
      // it immediately so we never leak an orphaned listener.
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
