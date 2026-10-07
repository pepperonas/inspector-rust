import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { audioRoute, type AudioRoute } from "../lib/ipc";

/** Outputs change from elsewhere too (menu bar, a BT box connecting) — a
 *  modest poll while the popup is visible keeps the footer honest. */
const POLL_MS = 5_000;

/**
 * Footer audio route (v0.204.0). Same visibility gate as `useFreeSpace`:
 * fetch on mount and every `window-shown`, poll only while visible, stop on
 * `popup-hidden`. `refresh` is for after a switch (boom's bridge comes up a
 * beat later, so callers refresh again shortly after).
 */
export function useAudioRoute(): { route: AudioRoute | null; refresh: () => void } {
  const [route, setRoute] = useState<AudioRoute | null>(null);
  const timerRef = useRef<number | null>(null);
  const aliveRef = useRef(true);

  const refresh = useCallback(() => {
    void audioRoute()
      .then((r) => {
        if (aliveRef.current && r) setRoute(r);
      })
      .catch(() => undefined); // keep the last reading
  }, []);

  useEffect(() => {
    aliveRef.current = true;
    let cancelled = false;
    const stopPolling = () => {
      if (timerRef.current != null) {
        window.clearInterval(timerRef.current);
        timerRef.current = null;
      }
    };
    const startPolling = () => {
      stopPolling();
      timerRef.current = window.setInterval(refresh, POLL_MS);
    };
    refresh();
    startPolling();
    const unlisteners: UnlistenFn[] = [];
    const on = (event: string, fn: () => void) => {
      void listen(event, fn).then((u) => (cancelled ? u() : unlisteners.push(u)));
    };
    on("window-shown", () => {
      refresh();
      startPolling();
    });
    on("popup-hidden", stopPolling);
    return () => {
      cancelled = true;
      aliveRef.current = false;
      stopPolling();
      for (const u of unlisteners) u();
    };
  }, [refresh]);

  return { route, refresh };
}
