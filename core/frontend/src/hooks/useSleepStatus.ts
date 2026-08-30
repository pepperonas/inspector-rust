import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getSleepStatus, type SleepStatus } from "../lib/ipc";
import { IS_MAC } from "../lib/platform";

/** Poll cadence while the popup is visible. Each poll spawns `pmset` twice
 *  (cheap, but not free) — the footer's local 1 s ticker carries the countdown
 *  between polls, so 10 s is plenty fresh. */
const POLL_MS = 10_000;

/**
 * System sleep status for the footer (v0.114.0; made live v0.152.0). Polls
 * `get_sleep_status` every 10 s — but ONLY while the popup is visible (the `window-shown` /
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
    // ⚠️ Fetch once on mount, not only on `window-shown`. `listen()` resolves
    // asynchronously, so a `window-shown` emitted before the listener was
    // registered used to be missed entirely — no fetch AND no interval, i.e.
    // the indicator stayed blank until the popup was closed and reopened.
    fetchNow();

    // Unmount-before-resolve guard, same as useTauriEvent/useClipboardHistory.
    const unlisteners: UnlistenFn[] = [];
    const on = (event: string, fn: () => void) => {
      void listen(event, fn).then((u) => (cancelled ? u() : unlisteners.push(u)));
    };
    on("window-shown", () => {
      fetchNow();
      stopPolling();
      timerRef.current = window.setInterval(fetchNow, POLL_MS);
    });
    on("popup-hidden", () => stopPolling());
    // ⚠️ Both of these change the answer INSTANTLY, and the 10 s poll is far
    // too slow for a state the user just toggled: without them the footer
    // showed the old reading for up to ten seconds while the wakelock LED had
    // already flipped — two indicators contradicting each other, which is
    // exactly what read as broken.
    on("wakelock-changed", fetchNow);
    on("nosleep-changed", fetchNow);
    return () => {
      cancelled = true;
      stopPolling();
      for (const u of unlisteners) u();
    };
  }, []);

  return status;
}
