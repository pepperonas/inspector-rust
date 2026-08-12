import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getHistory } from "../lib/ipc";
import type { ClipEntry } from "../lib/types";

export function useClipboardHistory() {
  const [entries, setEntries] = useState<ClipEntry[]>([]);
  const [loading, setLoading] = useState(true);
  // Visibility gate (v0.105.0): "clipboard-changed" fires for EVERY copy
  // anywhere in the OS, and the popup spends most of its life hidden — a
  // 1000-row decrypt + IPC marshal + full re-render for a UI nobody can see,
  // per copy. While hidden we skip the refresh; "window-shown" refreshes
  // unconditionally, so the list is always fresh the moment it is visible.
  // Starts `true`: the popup window is created hidden at launch (the mount
  // refresh below still loads the initial data once).
  const hiddenRef = useRef(true);

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
    let unhide: UnlistenFn | undefined;
    void listen("clipboard-changed", () => {
      if (!hiddenRef.current) void refresh();
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    void listen("window-shown", () => {
      hiddenRef.current = false;
      void refresh();
    }).then((u) => {
      if (cancelled) u();
      else unshow = u;
    });
    void listen("popup-hidden", () => {
      hiddenRef.current = true;
    }).then((u) => {
      if (cancelled) u();
      else unhide = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
      unshow?.();
      unhide?.();
    };
  }, [refresh]);

  return { entries, loading, refresh };
}
