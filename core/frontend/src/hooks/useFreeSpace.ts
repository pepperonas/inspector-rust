import { useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getFreeSpace, type FreeSpace } from "../lib/ipc";

/** Poll cadence while the popup is visible — free space changes slowly, and
 *  every open fetches immediately anyway. */
const POLL_MS = 30_000;

/**
 * Free space on the home disk for the footer (v0.195.0). Same visibility gate
 * as `useSleepStatus`: fetch on mount and on every `window-shown`, poll only
 * while the popup is visible, stop on `popup-hidden`.
 */
export function useFreeSpace(): FreeSpace | null {
  const [space, setSpace] = useState<FreeSpace | null>(null);
  const timerRef = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    const fetchNow = () => {
      void getFreeSpace()
        .then((s) => {
          if (!cancelled && s) setSpace(s);
        })
        .catch(() => undefined); // keep the last reading
    };
    const stopPolling = () => {
      if (timerRef.current != null) {
        window.clearInterval(timerRef.current);
        timerRef.current = null;
      }
    };
    fetchNow();
    const unlisteners: UnlistenFn[] = [];
    const on = (event: string, fn: () => void) => {
      void listen(event, fn).then((u) => (cancelled ? u() : unlisteners.push(u)));
    };
    on("window-shown", () => {
      fetchNow();
      stopPolling();
      timerRef.current = window.setInterval(fetchNow, POLL_MS);
    });
    on("popup-hidden", stopPolling);
    return () => {
      cancelled = true;
      stopPolling();
      for (const u of unlisteners) u();
    };
  }, []);

  return space;
}
