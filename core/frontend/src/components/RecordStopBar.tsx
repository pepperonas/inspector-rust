import { useEffect, useRef, useState } from "react";
import { Pause, Play, Square } from "lucide-react";
import { pauseScreenRecord, resumeScreenRecord, stopScreenRecord } from "../lib/ipc";

/**
 * Floating control bar shown while a screen recording is active (the
 * `record-stop` window — routed in `main.tsx`). A pulsing red dot (static
 * while paused), the elapsed *recording* time (frozen during pauses),
 * Pause/Resume, and Stop. Pause → `pauseScreenRecord` (finalises the current
 * segment); Resume → `resumeScreenRecord` (fresh segment); Stop →
 * `stopScreenRecord` (concats the segments into the final MP4, reveals it,
 * closes this window). The bar (except the buttons) is a drag region.
 */
export function RecordStopBar() {
  const [elapsed, setElapsed] = useState(0);
  const [paused, setPaused] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [busy, setBusy] = useState(false);
  const pausedRef = useRef(false);

  // Tick once per second; only advance while actively recording.
  useEffect(() => {
    const id = window.setInterval(() => {
      if (!pausedRef.current) setElapsed((s) => s + 1);
    }, 1000);
    return () => window.clearInterval(id);
  }, []);

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

  return (
    <div
      className="flex h-screen w-screen items-center gap-2.5 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-[var(--color-fg)] shadow-2xl"
    >
      {/* Drag handle: the left portion (dot + time + paused label) drags
          the window; buttons are intentionally outside the drag region. */}
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
