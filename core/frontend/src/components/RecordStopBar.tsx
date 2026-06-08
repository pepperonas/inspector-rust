import { useEffect, useState } from "react";
import { Square } from "lucide-react";
import { stopScreenRecord } from "../lib/ipc";

/**
 * Floating stop bar shown while a screen recording is active (the
 * `record-stop` window — routed in `main.tsx`). A pulsing red dot, the
 * elapsed time, and a Stop button. Stop → `stopScreenRecord` (finalises
 * the MP4, reveals it in Finder/Explorer, closes this window). The whole
 * bar except the button is a drag region so it can be repositioned.
 */
export function RecordStopBar() {
  const [elapsed, setElapsed] = useState(0);
  const [stopping, setStopping] = useState(false);

  useEffect(() => {
    const id = window.setInterval(() => setElapsed((s) => s + 1), 1000);
    return () => window.clearInterval(id);
  }, []);

  const stop = () => {
    if (stopping) return;
    setStopping(true);
    stopScreenRecord().catch(() => setStopping(false));
  };

  const mm = String(Math.floor(elapsed / 60)).padStart(2, "0");
  const ss = String(elapsed % 60).padStart(2, "0");

  return (
    <div
      data-tauri-drag-region
      className="flex h-screen w-screen items-center gap-3 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-[var(--color-fg)] shadow-2xl"
    >
      <span className="recordPulse h-3 w-3 shrink-0 rounded-full bg-red-600" />
      <span className="tabular-nums text-[14px] font-medium">
        {mm}:{ss}
      </span>
      <button
        onClick={stop}
        disabled={stopping}
        title="Stop recording"
        className="ml-auto flex items-center gap-1.5 rounded-lg bg-red-600 px-3 py-1.5 text-[12px] font-medium text-white hover:bg-red-500 disabled:opacity-60"
      >
        <Square size={12} fill="currentColor" /> {stopping ? "Saving…" : "Stop"}
      </button>
    </div>
  );
}
