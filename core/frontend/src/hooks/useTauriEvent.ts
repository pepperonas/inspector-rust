import { useEffect } from "react";
import { listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * `useEffect`-safe Tauri-event subscription. Wraps `listen()` with a
 * **mount-status flag** that guards against the standard race:
 *
 *   useEffect(() => {
 *     let unlisten: UnlistenFn | undefined;
 *     void listen("foo", handler).then(u => unlisten = u);
 *     return () => unlisten?.();
 *   }, []);
 *
 * If the component unmounts *before* the `listen()` promise resolves,
 * the cleanup runs with `unlisten === undefined` → the listener
 * becomes orphaned for the lifetime of the app. Tauri 2's `UnlistenFn`
 * stays valid until called; without a call, the closure + its captured
 * state stay reachable.
 *
 * Symptom in practice: under React's strict-mode double-mount in dev,
 * every event is delivered twice. In production it's a slow leak on
 * tab switches / window re-opens.
 *
 * This hook stashes an `isMounted` ref and, when the listen() promise
 * resolves AFTER unmount, immediately calls the returned unlisten so
 * the listener never leaks.
 *
 * @param event Event name string (e.g. `"wakelock-changed"`).
 * @param handler Callback receiving the event payload.
 * @param deps Standard `useEffect` deps — when they change, the
 *             listener is detached + reattached.
 */
export function useTauriEvent<T = unknown>(
  event: string,
  handler: EventCallback<T>,
  deps: React.DependencyList = [],
): void {
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;

    void listen<T>(event, handler).then((u) => {
      if (cancelled) {
        // Component unmounted (or deps changed) before listen
        // resolved — clean up the orphan immediately.
        u();
      } else {
        unlisten = u;
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}
