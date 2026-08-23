import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getSleepStatus, type SleepStatus } from "../lib/ipc";
import { IS_MAC } from "../lib/platform";

/** Poll cadence while the popup is visible. Each poll spawns `pmset` twice
 *  (cheap, but not free) — the footer's local 1 s ticker carries the countdown
 *  between polls, so 10 s is plenty fresh. */
const POLL_MS = 10_000;

/**
 * System sleep status for the footer (v0.114.0). Polls `get_sleep_status`
 * every 10 s — but ONLY while the popup is visible (the `window-shown` /
 * `popup-hidden` visibility gate, same pattern as `useClipboardHistory`): the
 * popup spends most of its life hidden, and a subprocess spawn every 10 s for
 * an invisible indicator would be waste. Non-macOS never polls at all (the
 * backend would answer `supported: false` per call — pointless IPC).
 */
export function useSleepStatus(): SleepStatus | null {
  const [status, setStatus] = useState<SleepStatus | null>(null);
  const timerRef = useRef<number | null>(null);

  useEffect(() => {
    if (!IS_MAC) return;
    let cancelled = false;
    const fetchNow = () => {
      void getSleepStatus()
        .then((s) => {
          if (!cancelled) setStatus(s);
        })
        .catch(() => undefined); // transient IPC failure → keep the last state
    };
    const stopPolling = () => {
      if (timerRef.current != null) {
        window.clearInterval(timerRef.current);
        timerRef.current = null;
      }
    };
    // Unmount-before-resolve guard, same as useTauriEvent/useClipboardHistory.
    let unshow: UnlistenFn | undefined;
    let unhide: UnlistenFn | undefined;
    void listen("window-shown", () => {
      fetchNow();
      stopPolling();
      timerRef.current = window.setInterval(fetchNow, POLL_MS);
    }).then((u) => {
      if (cancelled) u();
      else unshow = u;
    });
    void listen("popup-hidden", () => stopPolling()).then((u) => {
      if (cancelled) u();
      else unhide = u;
    });
    return () => {
      cancelled = true;
      stopPolling();
      unshow?.();
      unhide?.();
    };
  }, []);

  return status;
}
