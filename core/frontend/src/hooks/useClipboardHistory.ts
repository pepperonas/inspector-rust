import { useCallback, useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getHistory } from "../lib/ipc";
import type { ClipEntry } from "../lib/types";

export function useClipboardHistory() {
  const [entries, setEntries] = useState<ClipEntry[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    const rows = await getHistory(1000, 0);
    setEntries(rows);
    setLoading(false);
  }, []);

  useEffect(() => {
    void refresh();
    // Unmount-before-resolve guard: if the effect tears down before listen()
    // resolves, the resolved unlisten must still be called or the listener
    // leaks for the process lifetime (same pattern as useTauriEvent).
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    let unshow: UnlistenFn | undefined;
    void listen("clipboard-changed", () => {
      void refresh();
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    void listen("window-shown", () => {
      void refresh();
    }).then((u) => {
      if (cancelled) u();
      else unshow = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
      unshow?.();
    };
  }, [refresh]);

  return { entries, loading, refresh };
}
