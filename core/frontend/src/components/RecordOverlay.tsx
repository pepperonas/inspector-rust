import { useCallback, useEffect, useRef, useState } from "react";
import { Mic, Monitor, Volume2, X } from "lucide-react";
import {
  cancelRecordOverlay,
  ERR_NO_FFMPEG,
  startScreenRecord,
  type RecordRegion,
} from "../lib/ipc";

/**
 * Fullscreen region-select + audio-config + countdown overlay (the
 * `record-overlay` window — routed in `main.tsx`). The start of the
 * Ctrl+Shift+R screen-recording flow:
 *
 *   1. **select**    — drag a marquee over the area to record.
 *   2. **configure** — toggle System / Microphone audio, hit Record.
 *   3. **countdown** — 3-2-1, then the overlay goes fully transparent
 *      (so the first recorded frames are clean) and calls
 *      `startScreenRecord` with the region in **physical** pixels.
 *
 * The backend (`start_screen_record`) closes this window once ffmpeg is
 * spawned and pops the floating stop bar. Esc cancels at any phase.
 */
type Phase = "select" | "configure" | "countdown" | "starting";

interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

const MIN_SIZE = 12; // px — ignore accidental click-without-drag

export function RecordOverlay() {
  const [phase, setPhase] = useState<Phase>("select");
  const [rect, setRect] = useState<Rect | null>(null);
  const [system, setSystem] = useState(false);
  const [mic, setMic] = useState(false);
  const [count, setCount] = useState(3);
  const [error, setError] = useState<string | null>(null);
  const dragStart = useRef<{ x: number; y: number } | null>(null);

  const cancel = useCallback(() => {
    void cancelRecordOverlay();
  }, []);

  // Esc cancels everywhere.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") cancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [cancel]);

  // ── Phase: select ──────────────────────────────────────────────
  const onMouseDown = (e: React.MouseEvent) => {
    if (phase !== "select" || e.button !== 0) return;
    dragStart.current = { x: e.clientX, y: e.clientY };
    setRect({ left: e.clientX, top: e.clientY, width: 0, height: 0 });
  };
  const onMouseMove = (e: React.MouseEvent) => {
    if (phase !== "select" || !dragStart.current) return;
    const s = dragStart.current;
    setRect({
      left: Math.min(s.x, e.clientX),
      top: Math.min(s.y, e.clientY),
      width: Math.abs(e.clientX - s.x),
      height: Math.abs(e.clientY - s.y),
    });
  };
  const onMouseUp = () => {
    if (phase !== "select" || !dragStart.current) return;
    dragStart.current = null;
    if (rect && rect.width >= MIN_SIZE && rect.height >= MIN_SIZE) {
      setPhase("configure");
    } else {
      setRect(null); // too small — reset
    }
  };

  // ── Phase: countdown → start ───────────────────────────────────
  const beginRecording = useCallback(() => {
    if (!rect) return;
    const dpr = window.devicePixelRatio || 1;
    // libx264 + yuv420p needs even dimensions.
    const even = (n: number) => Math.max(2, Math.floor(n / 2) * 2);
    const region: RecordRegion = {
      x: Math.round(rect.left * dpr),
      y: Math.round(rect.top * dpr),
      w: even(rect.width * dpr),
      h: even(rect.height * dpr),
    };
    startScreenRecord(region, { system, mic }).catch((e) => {
      const msg = String(e);
      setPhase("configure");
      setError(
        msg.includes(ERR_NO_FFMPEG)
          ? "ffmpeg is not installed. Install it (Windows: winget install ffmpeg; macOS: brew install ffmpeg) and restart the app."
          : `Could not start recording: ${msg}`,
      );
    });
  }, [rect, system, mic]);

  // Tick the countdown down to 0, then switch to the transparent "starting"
  // phase. IMPORTANT: do NOT schedule the start timer in this same effect — its
  // deps include `phase`, so `setPhase` would tear the effect down (running the
  // cleanup) and cancel the timer before it fires. The dedicated effect below
  // owns the start.
  useEffect(() => {
    if (phase !== "countdown") return;
    if (count <= 0) {
      setPhase("starting");
      return;
    }
    const t = window.setTimeout(() => setCount((c) => c - 1), 1000);
    return () => window.clearTimeout(t);
  }, [phase, count]);

  // Once transparent (so our chrome isn't in the first frames), kick off the
  // recording — exactly once (a ref guards against effect re-runs).
  const startedRef = useRef(false);
  useEffect(() => {
    if (phase !== "starting" || startedRef.current) return;
    startedRef.current = true;
    const t = window.setTimeout(beginRecording, 150);
    return () => window.clearTimeout(t);
  }, [phase, beginRecording]);

  const startCountdown = () => {
    setError(null);
    setCount(3);
    setPhase("countdown");
  };

  // ── Render ─────────────────────────────────────────────────────
  if (phase === "starting") {
    // Fully transparent — nothing must appear in the recording.
    return <div className="h-screen w-screen" />;
  }

  const hasRect = rect && rect.width > 0 && rect.height > 0;

  return (
    <div
      className="relative h-screen w-screen cursor-crosshair select-none overflow-hidden"
      onMouseDown={onMouseDown}
      onMouseMove={onMouseMove}
      onMouseUp={onMouseUp}
      style={{ cursor: phase === "select" ? "crosshair" : "default" }}
    >
      {/* Dim the whole screen; the selection rect punches a clear hole via
          a huge spread box-shadow. */}
      {hasRect ? (
        <div
          className="absolute border-2 border-[var(--color-accent)]"
          style={{
            left: rect!.left,
            top: rect!.top,
            width: rect!.width,
            height: rect!.height,
            boxShadow: "0 0 0 9999px rgba(0,0,0,0.45)",
          }}
        />
      ) : (
        <div className="absolute inset-0" style={{ background: "rgba(0,0,0,0.30)" }} />
      )}

      {/* Hint while selecting. */}
      {phase === "select" && (
        <div className="pointer-events-none absolute left-1/2 top-8 -translate-x-1/2 rounded-lg bg-black/70 px-4 py-2 text-[13px] text-white shadow-lg">
          Drag to select the recording area · Esc to cancel
        </div>
      )}

      {/* Countdown big number, centred in the selection. */}
      {phase === "countdown" && hasRect && (
        <div
          className="pointer-events-none absolute flex items-center justify-center"
          style={{ left: rect!.left, top: rect!.top, width: rect!.width, height: rect!.height }}
        >
          <span
            className="font-bold text-white drop-shadow-lg"
            style={{ fontSize: Math.min(160, Math.max(48, rect!.height * 0.5)) }}
          >
            {count}
          </span>
        </div>
      )}

      {/* Configure panel — centred inside the selection rectangle so it
          is always visible on the same monitor as the selection. */}
      {phase === "configure" && hasRect && (
        <div
          className="absolute flex flex-col gap-3 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 text-[var(--color-fg)] shadow-2xl"
          style={{
            left: rect!.left + Math.max(0, (rect!.width - 264) / 2),
            top: rect!.top + Math.max(0, (rect!.height - 200) / 2),
            width: 264,
          }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          <div className="flex items-center justify-between">
            <span className="flex items-center gap-2 text-[13px] font-medium">
              <Monitor size={15} className="text-[var(--color-accent)]" /> Record audio
            </span>
            <button
              onClick={cancel}
              title="Cancel (Esc)"
              className="flex h-6 w-6 items-center justify-center rounded-full text-[var(--color-muted)] hover:bg-[var(--color-bg)] hover:text-[var(--color-fg)]"
            >
              <X size={14} />
            </button>
          </div>

          <Toggle
            on={system}
            onToggle={() => setSystem((v) => !v)}
            icon={<Volume2 size={15} />}
            label="System audio"
          />
          <Toggle
            on={mic}
            onToggle={() => setMic((v) => !v)}
            icon={<Mic size={15} />}
            label="Microphone"
          />

          {error && (
            <p className="text-[11px] leading-snug text-red-400">{error}</p>
          )}

          <div className="mt-1 flex items-center gap-2">
            <button
              onClick={startCountdown}
              className="flex flex-1 items-center justify-center gap-2 rounded-lg bg-red-600 px-3 py-2 text-[13px] font-medium text-white hover:bg-red-500"
            >
              <span className="h-2.5 w-2.5 rounded-full bg-white" /> Record
            </button>
            <button
              onClick={cancel}
              className="rounded-lg border border-[var(--color-border)] px-3 py-2 text-[13px] text-[var(--color-muted)] hover:bg-[var(--color-bg)]"
            >
              Cancel
            </button>
          </div>
          <p className="text-[10px] leading-snug text-[var(--color-muted)]">
            Mic records directly. <b>System audio</b> is captured through a
            loopback device (macOS: BlackHole) — your output must actually be
            routed through it (e.g. a Multi-Output Device), otherwise it records
            silence. Starts after a 3&nbsp;s countdown.
          </p>
        </div>
      )}
    </div>
  );
}

function Toggle({
  on,
  onToggle,
  icon,
  label,
}: {
  on: boolean;
  onToggle: () => void;
  icon: React.ReactNode;
  label: string;
}) {
  return (
    <button
      onClick={onToggle}
      className={
        "flex items-center justify-between rounded-lg border px-3 py-2 text-[13px] " +
        (on
          ? "border-[var(--color-accent)] bg-[var(--color-accent)]/10 text-[var(--color-fg)]"
          : "border-[var(--color-border)] text-[var(--color-muted)] hover:bg-[var(--color-bg)]")
      }
    >
      <span className="flex items-center gap-2">
        {icon}
        {label}
      </span>
      <span
        className={
          "flex h-4 w-4 items-center justify-center rounded-full border " +
          (on
            ? "border-[var(--color-accent)] bg-[var(--color-accent)]"
            : "border-[var(--color-muted)]")
        }
      >
        {on && <span className="h-1.5 w-1.5 rounded-full bg-white" />}
      </span>
    </button>
  );
}
