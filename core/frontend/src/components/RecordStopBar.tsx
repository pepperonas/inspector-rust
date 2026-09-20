import { useEffect, useRef, useState } from "react";
import { ChevronDown, Pause, Play, Square } from "lucide-react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { pauseScreenRecord, resumeScreenRecord, stopScreenRecord } from "../lib/ipc";

/**
 * Floating control bar shown while a screen recording is active (the
 * `record-stop` window — routed in `main.tsx`). A pulsing red dot (static
 * while paused), the elapsed *recording* time (frozen during pauses),
 * Pause/Resume, and Stop. Pause → `pauseScreenRecord` (finalises the current
 * segment); Resume → `resumeScreenRecord` (fresh segment); Stop →
 * `stopScreenRecord` (concats the segments into the final MP4, reveals it,
 * closes this window). The bar (except the buttons) is a drag region.
 *
 * Collapse toggle (top-left chevron): while a recording runs in the background
 * the bar can be in the way, so a click shrinks the whole WINDOW to a tiny nub
 * (x AND y) showing only the chevron; another click restores it. It resizes the
 * Tauri window, not just CSS — the window is transparent, so a CSS-only collapse
 * would leave the full-size (invisible) window still occupying/capturing the
 * screen. The chevron rotation is the animated state switch.
 */
const EXPANDED_W = 312;
const EXPANDED_H = 54;
// "Extremely minimised": ~7% of the bar's area, still a comfortable tap target.
const NUB_W = 36;
const NUB_H = 30;

export function RecordStopBar() {
  const [elapsed, setElapsed] = useState(0);
  const [paused, setPaused] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [busy, setBusy] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const pausedRef = useRef(false);
  const resizingRef = useRef(false);

  // Tick once per second; only advance while actively recording. Keeps running
  // while collapsed (the recording continues), so the time is current on expand.
  useEffect(() => {
    const id = window.setInterval(() => {
      if (!pausedRef.current) setElapsed((s) => s + 1);
    }, 1000);
    return () => window.clearInterval(id);
  }, []);

  const toggleCollapsed = async () => {
    if (resizingRef.current) return;
    resizingRef.current = true;
    const next = !collapsed;
    setCollapsed(next); // chevron animates immediately
    try {
      await getCurrentWebviewWindow().setSize(
        next ? new LogicalSize(NUB_W, NUB_H) : new LogicalSize(EXPANDED_W, EXPANDED_H),
      );
    } catch (e) {
      console.error("resize stop bar", e);
      setCollapsed(!next); // revert the UI if the window didn't actually resize
    } finally {
      resizingRef.current = false;
    }
  };

  const togglePause = async () => {
    if (busy || stopping) return;
    setBusy(true);
    try {
      if (paused) {
        await resumeScreenRecord();
        pausedRef.current = false;
        setPaused(false);
      } else {
        await pauseScreenRecord();
        pausedRef.current = true;
        setPaused(true);
      }
    } catch (e) {
      console.error("toggle pause", e);
    } finally {
      setBusy(false);
    }
  };

  const stop = () => {
    if (stopping) return;
    setStopping(true);
    stopScreenRecord().catch(() => setStopping(false));
  };

  const mm = String(Math.floor(elapsed / 60)).padStart(2, "0");
  const ss = String(elapsed % 60).padStart(2, "0");

  // The collapse toggle — top-left in both states. Chevron points up when
  // expanded (click folds the bar away) and down when collapsed (click unfolds
  // it); the rotation is the animated switch (disabled under reduced motion).
  const chevron = (
    <button
      onClick={toggleCollapsed}
      title={collapsed ? "Ausklappen" : "Einklappen"}
      aria-label={collapsed ? "Ausklappen" : "Einklappen"}
      className="flex shrink-0 items-center justify-center rounded-md p-1 text-[var(--color-muted)] hover:bg-[var(--color-bg)] hover:text-[var(--color-fg)]"
    >
      <ChevronDown
        size={16}
        className={
          "transition-transform duration-(--duration-slow) ease-sharp motion-reduce:transition-none " +
          (collapsed ? "" : "rotate-180")
        }
      />
    </button>
  );

  if (collapsed) {
    // Tiny nub: only the chevron, centred. Not a drag region — dragging is done
    // on the expanded bar's top area; collapsed prioritises minimal footprint.
    return (
      <div className="flex h-screen w-screen items-center justify-center rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-fg)] shadow-2xl">
        {chevron}
      </div>
    );
  }

  return (
    <div className="flex h-screen w-screen items-center gap-2.5 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-[var(--color-fg)] shadow-2xl">
      {chevron}
      {/* Drag handle: the middle portion (dot + time + paused label) drags the
          window; the chevron and buttons are intentionally outside the drag
          region so their clicks work. */}
      <div data-tauri-drag-region className="flex flex-1 items-center gap-2.5">
        <span
          className={
            "h-3 w-3 shrink-0 rounded-full bg-red-600 " + (paused ? "opacity-50" : "recordPulse")
          }
        />
        <span className="tabular-nums text-[14px] font-medium">
          {mm}:{ss}
        </span>
        {paused && (
          <span className="text-[11px] font-medium uppercase tracking-wide text-[var(--color-muted)]">
            Paused
          </span>
        )}
      </div>

      <button
        onClick={togglePause}
        disabled={busy || stopping}
        title={paused ? "Resume" : "Pause"}
        className="flex items-center gap-1 rounded-lg border border-[var(--color-border)] px-2.5 py-1.5 text-[12px] font-medium hover:bg-[var(--color-bg)] disabled:opacity-60"
      >
        {paused ? <Play size={12} fill="currentColor" /> : <Pause size={12} fill="currentColor" />}
        {paused ? "Resume" : "Pause"}
      </button>
      <button
        onClick={stop}
        disabled={stopping}
        title="Stop recording"
        className="flex items-center gap-1 rounded-lg bg-red-600 px-2.5 py-1.5 text-[12px] font-medium text-white hover:bg-red-500 disabled:opacity-60"
      >
        <Square size={12} fill="currentColor" /> {stopping ? "Saving…" : "Stop"}
      </button>
    </div>
  );
}
