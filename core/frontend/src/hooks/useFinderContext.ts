import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { finderContext, type FinderContext } from "../lib/ipc";
import { IS_LINUX } from "../lib/platform";

/** Fired after touch/echo/mkdir so the footer shows the new state. */
export const FINDER_CONTEXT_REFRESH = "finder-context-refresh";

/**
 * The working folder of the front Finder/Explorer window for the footer.
 * One osascript round trip (~0.1–0.4 s), so it is asked on popup open and
 * after a file command — never polled. On error (Automation not granted,
 * Finder busy) the last reading stays; with none, the chip is hidden.
 */
export function useFinderContext(): FinderContext | null {
  const [ctx, setCtx] = useState<FinderContext | null>(null);
  useEffect(() => {
    if (IS_LINUX) return;
    let cancelled = false;
    const fetchNow = () => {
      void finderContext()
        .then((c) => {
          if (!cancelled) setCtx(c);
        })
        .catch(() => undefined);
    };
    fetchNow();
    const unlisteners: UnlistenFn[] = [];
    void listen("window-shown", fetchNow).then((u) => (cancelled ? u() : unlisteners.push(u)));
    window.addEventListener(FINDER_CONTEXT_REFRESH, fetchNow);
    return () => {
      cancelled = true;
      window.removeEventListener(FINDER_CONTEXT_REFRESH, fetchNow);
      for (const u of unlisteners) u();
    };
  }, []);
  return ctx;
}
