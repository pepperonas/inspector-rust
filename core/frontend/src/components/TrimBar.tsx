import { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Play, Pause, Scissors, SkipBack, SkipForward } from "lucide-react";
import { socialAudioProxy } from "../lib/ipc";
import {
  fmtClock,
  fullRange,
  moveHandle,
  moveRange,
  nudgeStep,
  parseClock,
  pctAt,
  timeAtX,
  type Range,
} from "../lib/trim-range";

/**
 * QuickTime-style trim bar under the download buttons.
 *
 * The model is Apple's: two handles over a track, everything OUTSIDE them is
 * discarded, plus YouTube Studio's numeric time entry. The handles PUSH each
 * other rather than swapping (see `moveHandle`), and all the maths lives in the
 * pure `lib/trim-range.ts` so this component only paints and wires events.
 *
 * ⚠️ It scrubs an AUDIO PROXY, not the source. Measured on the reference video:
 * every YouTube format is video-only or audio-only, so a media element cannot
 * play the source at all. The proxy is a 48.8 kbit/s m4a (30 MB for 83 min,
 * ~5 s to fetch) in the app cache, which is already inside the asset-protocol
 * scope. For a DJ set — the case this was built for — finding the cut by ear is
 * the right tool anyway; for video downloads the picture stays absent, and that
 * is the honest cost of not downloading 300 MB first.
 */
export function TrimBar({
  url,
  duration,
  range,
  onRange,
  disabled,
}: {
  url: string;
  /** Media duration in seconds, from the already-fetched metadata. */
  duration: number;
  range: Range;
  onRange: (r: Range) => void;
  disabled?: boolean;
}) {
  const [proxy, setProxy] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);
  const [head, setHead] = useState(0);

  const audioRef = useRef<HTMLAudioElement | null>(null);
  const trackRef = useRef<HTMLDivElement | null>(null);
  const stopAtRef = useRef<number | null>(null);
  const dragRef = useRef<null | { kind: "start" | "end" | "range" | "head"; lastT: number }>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void socialAudioProxy(url)
      .then((p) => {
        if (!cancelled) setProxy(convertFileSrc(p));
      })
      .catch((e) => {
        if (!cancelled) setError(String(e).includes("no_ytdlp") ? "yt-dlp fehlt" : "Vorschau nicht ladbar");
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [url]);

  // Stop exactly at the range end when previewing a section.
  const onTime = useCallback(() => {
    const a = audioRef.current;
    if (!a) return;
    setHead(a.currentTime);
    const stop = stopAtRef.current;
    if (stop !== null && a.currentTime >= stop) {
      stopAtRef.current = null;
      a.pause();
    }
  }, []);

  const seek = useCallback((t: number) => {
    const a = audioRef.current;
    if (!a) return;
    a.currentTime = t;
    setHead(t);
  }, []);

  const playFrom = useCallback(
    (from: number, until: number | null) => {
      const a = audioRef.current;
      if (!a) return;
      stopAtRef.current = until;
      a.currentTime = from;
      setHead(from);
      void a.play().catch(() => undefined);
    },
    [],
  );

  const toggle = useCallback(() => {
    const a = audioRef.current;
    if (!a) return;
    if (a.paused) {
      stopAtRef.current = null;
      void a.play().catch(() => undefined);
    } else {
      a.pause();
    }
  }, []);

  // ── Pointer handling on the track ────────────────────────────────────────
  const timeFromEvent = (clientX: number) => {
    const el = trackRef.current;
    if (!el) return 0;
    const r = el.getBoundingClientRect();
    return timeAtX(clientX, r.left, r.width, duration);
  };

  const startDrag = (kind: "start" | "end" | "range" | "head", e: React.PointerEvent) => {
    if (disabled) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    const at = timeFromEvent(e.clientX);
    dragRef.current = { kind, lastT: at };
    if (kind === "head") seek(at);
  };

  const onMove = (e: React.PointerEvent) => {
    const d = dragRef.current;
    if (!d) return;
    const t = timeFromEvent(e.clientX);
    if (d.kind === "range") {
      onRange(moveRange(range, t - d.lastT, duration));
    } else if (d.kind === "head") {
      seek(t);
    } else {
      onRange(moveHandle(range, d.kind, t, duration));
    }
    d.lastT = t;
  };

  const endDrag = (e: React.PointerEvent) => {
    (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
    dragRef.current = null;
  };

  const nudge = (which: "start" | "end", e: React.KeyboardEvent) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const step = nudgeStep(duration, e.shiftKey) * (e.key === "ArrowLeft" ? -1 : 1);
    const cur = which === "start" ? range.start : range.end;
    onRange(moveHandle(range, which, cur + step, duration));
  };

  const withHours = duration >= 3600;
  const len = Math.max(0, range.end - range.start);
  const startPct = pctAt(range.start, duration);
  const endPct = pctAt(range.end, duration);
  const headPct = pctAt(head, duration);

  const chip =
    "md3-press rounded border border-[var(--color-border)] px-2 py-1 text-[11px] hover:border-[var(--color-accent)] disabled:opacity-40";

  return (
    <div className="flex flex-col gap-2">
      {proxy && (
        <audio
          ref={audioRef}
          src={proxy}
          preload="metadata"
          onTimeUpdate={onTime}
          onPlay={() => setPlaying(true)}
          onPause={() => setPlaying(false)}
        />
      )}

      {/* Track */}
      <div
        ref={trackRef}
        className="relative h-11 w-full select-none rounded border border-[var(--color-border)] bg-[var(--color-bg)]"
        onPointerMove={onMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        {/* Discarded areas — QuickTime dims what falls outside the handles. */}
        <div className="absolute inset-y-0 left-0 rounded-l bg-black/45" style={{ width: `${startPct}%` }} />
        <div className="absolute inset-y-0 right-0 rounded-r bg-black/45" style={{ width: `${100 - endPct}%` }} />

        {/* Kept region — draggable as a whole. */}
        <div
          className="absolute inset-y-0 cursor-grab active:cursor-grabbing"
          style={{
            left: `${startPct}%`,
            width: `${Math.max(0, endPct - startPct)}%`,
            background: "color-mix(in srgb, var(--color-accent) 14%, transparent)",
            borderTop: "2px solid #e0b100",
            borderBottom: "2px solid #e0b100",
          }}
          onPointerDown={(e) => startDrag("range", e)}
          title="Bereich verschieben"
        />

        {/* Playhead */}
        <div
          className="pointer-events-none absolute inset-y-0 w-[2px] bg-[var(--color-fg)]"
          style={{ left: `${headPct}%` }}
        />

        {/* Handles — QuickTime's yellow grips. */}
        {(["start", "end"] as const).map((which) => (
          <button
            key={which}
            type="button"
            aria-label={which === "start" ? "Startpunkt" : "Endpunkt"}
            onPointerDown={(e) => startDrag(which, e)}
            onKeyDown={(e) => nudge(which, e)}
            className="absolute inset-y-0 w-[11px] cursor-ew-resize rounded-sm focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--color-accent)]"
            style={{
              left: `calc(${which === "start" ? startPct : endPct}% - 5.5px)`,
              background: "#e0b100",
              boxShadow: "0 0 0 1px rgba(0,0,0,.35)",
            }}
          />
        ))}

        {/* Scrub layer — click anywhere unclaimed to move the playhead. */}
        <div className="absolute inset-0 -z-10" onPointerDown={(e) => startDrag("head", e)} />
      </div>

      {/* Times */}
      <div className="flex flex-wrap items-center gap-2 font-[var(--font-mono)] text-[11px]">
        <TimeField label="Start" value={range.start} withHours={withHours}
          onCommit={(t) => onRange(moveHandle(range, "start", t, duration))} />
        <TimeField label="Ende" value={range.end} withHours={withHours}
          onCommit={(t) => onRange(moveHandle(range, "end", t, duration))} />
        <span className="text-[var(--color-muted)]">
          Länge {fmtClock(len, withHours)} · Position {fmtClock(head, withHours)}
        </span>
      </div>

      {/* Transport */}
      <div className="flex flex-wrap items-center gap-1.5">
        <button type="button" className={chip} disabled={!proxy}
          onClick={() => playFrom(range.start, Math.min(range.start + 3, range.end))}>
          <SkipBack size={11} className="mr-1 inline" />Anfang
        </button>
        <button type="button" className={chip} disabled={!proxy}
          onClick={() => (playing ? toggle() : playFrom(range.start, range.end))}>
          {playing ? <Pause size={11} className="mr-1 inline" /> : <Play size={11} className="mr-1 inline" />}
          Bereich
        </button>
        <button type="button" className={chip} disabled={!proxy}
          onClick={() => playFrom(Math.max(range.start, range.end - 3), range.end)}>
          <SkipForward size={11} className="mr-1 inline" />Ende
        </button>
        <button type="button" className={chip} onClick={() => onRange(fullRange(duration))}>
          Ganzes Video
        </button>
        <span className="ml-auto flex items-center gap-1 text-[11px] text-[var(--color-muted)]">
          <Scissors size={11} />
          {loading ? "Vorschau lädt…" : error ? error : "Anhören zum Prüfen"}
        </span>
      </div>
    </div>
  );
}

/**
 * Editable `m:ss` field. Commits on blur/Enter only — while typing, an
 * unreadable value is left alone rather than snapping the handle to 0.
 */
function TimeField({
  label,
  value,
  withHours,
  onCommit,
}: {
  label: string;
  value: number;
  withHours: boolean;
  onCommit: (t: number) => void;
}) {
  const [text, setText] = useState(() => fmtClock(value, withHours));
  const [editing, setEditing] = useState(false);
  useEffect(() => {
    if (!editing) setText(fmtClock(value, withHours));
  }, [value, withHours, editing]);

  const commit = () => {
    setEditing(false);
    const t = parseClock(text);
    if (t === null) setText(fmtClock(value, withHours));
    else onCommit(t);
  };

  return (
    <label className="flex items-center gap-1">
      <span className="text-[var(--color-muted)]">{label}</span>
      <input
        value={text}
        onChange={(e) => setText(e.target.value)}
        onFocus={() => setEditing(true)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            (e.currentTarget as HTMLInputElement).blur();
          }
        }}
        className="w-[68px] rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1 py-0.5 text-center text-[var(--color-fg)] tabular-nums"
        aria-label={label}
      />
    </label>
  );
}
