import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { editorToSource, type RecordAudioTrack, type RecordEditDoc, type RecordEditorView, type RecordWaveSpan } from "../../lib/ipc";
import { updateTrack } from "../../lib/editor";
import { useT } from "../../lib/i18n";
import { snapGuide, snapShift, type Snapping } from "../../lib/timeline-snap";

/** Shortest track a drag leaves, programme ms. */
export const MIN_TRACK_MS = 100;
/** A pointer moved less than this many px: a click (select), not a drag. */
const CLICK_SLOP_PX = 3;
/** Height of one track row, px. */
export const LANE_ROW_PX = 26;

/** A track as the lane draws it: the extent of its entries on the programme (Rust's spans). */
export interface TrackBar {
  index: number;
  progStartMs: number;
  progEndMs: number;
  /** Programme ms where a loop repeat after the first begins. */
  repeats: number[];
  muted: boolean;
}

/** One bar per track that has entries, from `EditorView.tracks` (never re-projected here). */
export function trackBars(view: RecordEditorView): TrackBar[] {
  const by = new Map<number, TrackBar>();
  for (const s of view.tracks ?? []) {
    const b = by.get(s.index);
    if (!b) {
      by.set(s.index, { index: s.index, progStartMs: s.progStartMs, progEndMs: s.progEndMs, repeats: s.repeat > 0 ? [s.progStartMs] : [], muted: s.muted });
    } else {
      b.progStartMs = Math.min(b.progStartMs, s.progStartMs);
      b.progEndMs = Math.max(b.progEndMs, s.progEndMs);
      if (s.repeat > 0) b.repeats.push(s.progStartMs);
    }
  }
  return [...by.values()].sort((a, b) => a.index - b.index);
}

export type TrackDragMode = "move" | "start" | "end";

/** What a drag shows on the programme (the file window is Rust's: `editor_track_edge`). */
export interface TrackDragResult {
  progStartMs: number;
  progEndMs: number;
}

/**
 * A drag of `deltaMs` on a bar, in programme ms and clamped to the programme: moving keeps
 * the length (a loop may move later, Rust refills it to the end); an edge moves alone and
 * leaves at least 100 ms. Only positions — what an edge does to the file (cuts, the file's
 * ends) Rust resolves (R12).
 */
export function dragTrackBar(
  bar: Pick<TrackBar, "progStartMs" | "progEndMs">,
  track: Pick<RecordAudioTrack, "looped">,
  mode: TrackDragMode,
  deltaMs: number,
  programmeMs: number,
  snapping?: Snapping,
): TrackDragResult {
  const len = bar.progEndMs - bar.progStartMs;
  const at = (d: number): TrackDragResult => {
    if (mode === "move") {
      const hi = Math.max(0, track.looped ? programmeMs - MIN_TRACK_MS : programmeMs - len);
      const start = Math.round(Math.min(hi, Math.max(0, bar.progStartMs + d)));
      return { progStartMs: start, progEndMs: Math.min(track.looped ? programmeMs : Infinity, start + len) };
    }
    if (mode === "start") {
      const start = Math.round(Math.min(bar.progEndMs - MIN_TRACK_MS, Math.max(0, bar.progStartMs + d)));
      return { progStartMs: start, progEndMs: bar.progEndMs };
    }
    const end = Math.round(Math.min(programmeMs, Math.max(bar.progStartMs + MIN_TRACK_MS, bar.progEndMs + d)));
    return { progStartMs: bar.progStartMs, progEndMs: end };
  };
  const d = Number.isFinite(deltaMs) ? deltaMs : 0;
  const raw = at(d);
  if (!snapping) return raw;
  const { shiftMs } = snapShift(trackSnapEdges(raw, track, mode), snapping.targets, snapping.pxPerMs);
  return shiftMs ? at(d + shiftMs) : raw;
}

/**
 * The edges a track drag snaps with: an edge drag its own edge; a move both — except a looping
 * bar, whose end is the programme end Rust refills to, so only its start snaps.
 */
export function trackSnapEdges(r: TrackDragResult, track: Pick<RecordAudioTrack, "looped">, mode: TrackDragMode): number[] {
  if (mode === "start") return [r.progStartMs];
  if (mode === "end") return [r.progEndMs];
  return track.looped ? [r.progStartMs] : [r.progStartMs, r.progEndMs];
}

/** Resolves an edge drag in Rust: the track's new anchor and file window (`null`: nothing to do). */
export type ResolveTrackEdge = (
  index: number,
  id: string,
  edge: "start" | "end",
  programmeMs: number,
) => Promise<Pick<RecordAudioTrack, "clip" | "startMs" | "offsetMs" | "spanMs"> | null>;

/** A track's name: its label, else its file name without the extension. */
export function trackName(t: RecordAudioTrack): string {
  if (t.label.trim()) return t.label.trim();
  return t.file.replace(/\.[^.]+$/, "");
}

/**
 * A waveform: `[min, max]` pairs drawn as one filled shape over its box (`viewBox` stretched
 * to the element, so it scales with the timeline).
 */
export function Waveform({ peaks, className }: { peaks: readonly number[]; className?: string }) {
  const n = Math.floor(peaks.length / 2);
  if (n < 2) return null;
  const v = (i: number) => Math.max(-1, Math.min(1, Number.isFinite(peaks[i]) ? peaks[i] : 0));
  const top: string[] = [];
  const lows: string[] = [];
  for (let i = 0; i < n; i += 1) top.push(`${i} ${(-v(2 * i + 1)).toFixed(3)}`);
  for (let i = n - 1; i >= 0; i -= 1) lows.push(`${i} ${(-v(2 * i)).toFixed(3)}`);
  const d = `M${top.join(" L")} L${lows.join(" L")} Z`;
  return (
    <svg className={className} viewBox={`0 -1 ${n - 1} 2`} preserveAspectRatio="none" aria-hidden data-testid="ed-wave">
      <path d={d} fill="currentColor" />
    </svg>
  );
}

/** The waveforms inside a box spanning `startMs..endMs` (programme), positioned by their own spans. */
export function WaveLayer({
  waves,
  startMs,
  endMs,
  className,
}: {
  waves: readonly RecordWaveSpan[];
  startMs: number;
  endMs: number;
  className?: string;
}) {
  const len = endMs - startMs;
  if (!(len > 0) || waves.length === 0) return null;
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden>
      {waves.map((w, i) => (
        <div
          key={i}
          className="absolute inset-y-0"
          style={{ left: `${((w.progStartMs - startMs) / len) * 100}%`, width: `${((w.progEndMs - w.progStartMs) / len) * 100}%` }}
        >
          <Waveform peaks={w.peaks} className={"h-full w-full " + (className ?? "")} />
        </div>
      ))}
    </div>
  );
}

type Drag = { index: number; id: string; mode: TrackDragMode; x0: number; dx: number };

/**
 * The audio lane under the clips: one bar per music/voiceover track, from Rust's programme
 * entries, with its waveform and the loop repeats marked. Dragging the bar moves the track
 * (Rust maps the new programme start to an anchor); its edges trim the start and the length,
 * resolved in Rust (`resolveEdge`) because a voiceover's file skips what a cut removed.
 * Every drag is clamped to the programme and is one step.
 */
export function AudioLane({
  view,
  scale,
  getDoc,
  onChange,
  onError,
  selected,
  onSelect,
  resolveEdge,
  snapping,
  onGuide,
  emptyHint,
}: {
  view: RecordEditorView;
  /** px per programme ms. */
  scale: number;
  getDoc: () => RecordEditDoc;
  onChange: (doc: RecordEditDoc) => void;
  onError: (e: unknown) => void;
  selected: string | null;
  onSelect: (id: string | null) => void;
  /** Rust's resolution of an edge drag (R12). Without it the edges are not offered. */
  resolveEdge?: ResolveTrackEdge;
  /** Snap targets and scale: a dragged edge snaps within 8 px. */
  snapping?: Snapping;
  /** The target a drag sits on (`null`: none) — the timeline draws its guide line. */
  onGuide?: (ms: number | null) => void;
  /** Without tracks: a one-row note naming the keys that add one (none: the lane is hidden). */
  emptyHint?: string;
}) {
  const t = useT();
  const [drag, setDrag] = useState<Drag | null>(null);
  // Unmounted mid-drag: the timeline's guide must not outlive the drag.
  const guideRef = useRef(onGuide);
  useEffect(() => {
    guideRef.current = onGuide;
  });
  useEffect(() => () => guideRef.current?.(null), []);
  const bars = trackBars(view);
  const doc = view.doc;
  if (bars.length === 0) {
    if (!emptyHint) return null;
    return (
      <div className="relative mt-1 h-5" data-testid="ed-audio-lane-empty">
        <span className="pointer-events-none absolute inset-y-0 left-1 flex items-center text-[10px] text-[var(--color-muted)]" data-testid="ed-lane-hint">
          {emptyHint}
        </span>
      </div>
    );
  }
  const total = Math.max(1, view.programmeMs);

  const begin = (b: TrackBar, mode: TrackDragMode) => (e: ReactPointerEvent) => {
    e.stopPropagation();
    const tr = doc.audioTracks[b.index];
    if (!tr) return;
    onSelect(tr.id);
    try {
      (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    } catch {
      /* synthetic events */
    }
    setDrag({ index: b.index, id: tr.id, mode, x0: e.clientX, dx: 0 });
  };

  /** Where a drag sits on a snap target (only the edges it moves; `null` while still a click). */
  const guideOf = (d: Drag): number | null => {
    const bar = bars.find((b) => b.index === d.index);
    const tr = doc.audioTracks[d.index];
    if (!snapping || !bar || !tr || scale <= 0 || Math.abs(d.dx) < CLICK_SLOP_PX) return null;
    const r = dragTrackBar(bar, tr, d.mode, d.dx / scale, total, snapping);
    return snapGuide(trackSnapEdges(r, tr, d.mode), snapping.targets);
  };

  const finish = async (d: Drag) => {
    const bar = bars.find((b) => b.index === d.index);
    const tr = getDoc().audioTracks.find((x) => x.id === d.id);
    if (!bar || !tr || scale <= 0) return;
    const r = dragTrackBar(bar, tr, d.mode, d.dx / scale, total, snapping);
    if (d.mode === "move") {
      if (r.progStartMs === bar.progStartMs) return;
      // R12: Rust maps the programme start to the anchor (clip + source ms).
      const at = await editorToSource(Math.min(r.progStartMs, Math.max(0, total - 1)));
      if (!at) return;
      onChange(updateTrack(getDoc(), d.id, { clip: at.clip, startMs: Math.round(at.sourceMs) }));
      return;
    }
    const target = d.mode === "start" ? r.progStartMs : r.progEndMs;
    if (target === (d.mode === "start" ? bar.progStartMs : bar.progEndMs) || !resolveEdge) return;
    const patch = await resolveEdge(d.index, d.id, d.mode, target);
    if (patch) onChange(updateTrack(getDoc(), d.id, patch));
  };

  return (
    <div
      className="relative mt-1"
      style={{ height: LANE_ROW_PX * bars.length }}
      data-testid="ed-audio-lane"
      onPointerMove={(e) => {
        if (!drag) return;
        const next = { ...drag, dx: e.clientX - drag.x0 };
        setDrag(next);
        onGuide?.(guideOf(next));
      }}
      onPointerUp={() => {
        const d = drag;
        setDrag(null);
        onGuide?.(null);
        if (d && Math.abs(d.dx) >= CLICK_SLOP_PX) void finish(d).catch(onError);
      }}
      onPointerCancel={() => {
        setDrag(null);
        onGuide?.(null);
      }}
    >
      {bars.map((b, row) => {
        const tr = doc.audioTracks[b.index];
        if (!tr) return null;
        const sel = selected === tr.id;
        const d = drag?.index === b.index ? drag : null;
        const shown = d && scale > 0 ? dragTrackBar(b, tr, d.mode, d.dx / scale, total, snapping) : b;
        const start = shown.progStartMs;
        const end = shown.progEndMs;
        const waves = (view.waveforms ?? []).filter((w) => w.kind === "track" && w.track === b.index);
        const music = tr.kind === "music";
        const kindLabel = music ? t("rec4.audio.kind_music") : t("rec4.audio.kind_voiceover");
        return (
          <div
            key={tr.id}
            data-testid="ed-audio-bar"
            data-kind={tr.kind}
            data-muted={tr.muted ? "" : undefined}
            aria-selected={sel}
            title={`${kindLabel} · ${trackName(tr)}`}
            onPointerDown={begin(b, "move")}
            className={
              "absolute flex items-center overflow-hidden rounded-sm border px-1.5 text-[10px] font-medium " +
              "cursor-grab " +
              (music ? "border-emerald-400 bg-emerald-500/70 text-white " : "border-orange-400 bg-orange-500/70 text-white ") +
              (tr.muted ? "opacity-45 " : "") +
              (sel ? "ring-2 ring-[var(--color-accent)]" : "")
            }
            style={{ left: start * scale, top: LANE_ROW_PX * row, height: LANE_ROW_PX - 4, width: Math.max(10, (end - start) * scale) }}
          >
            <WaveLayer waves={waves} startMs={b.progStartMs} endMs={b.progEndMs} className="text-white/60" />
            {b.repeats.map((ms) => (
              <span
                key={ms}
                data-testid="ed-audio-repeat"
                aria-hidden
                className="pointer-events-none absolute inset-y-0 w-px bg-white/60"
                style={{ left: (ms - b.progStartMs) * scale }}
              />
            ))}
            <span className="relative truncate">{trackName(tr)}</span>
            {resolveEdge &&
              (["start", "end"] as const).map((edge) => (
                <span
                  key={edge}
                  aria-label={edge === "start" ? t("rec4.audio.trim_start") : t("rec4.audio.trim_end")}
                  data-testid={`ed-audio-${edge}`}
                  onPointerDown={begin(b, edge)}
                  className={
                    "absolute top-0 h-full w-1.5 cursor-ew-resize bg-white/60 opacity-0 transition-opacity duration-(--duration-fast) ease-sharp hover:opacity-100 " +
                    (edge === "start" ? "left-0" : "right-0")
                  }
                />
              ))}
          </div>
        );
      })}
    </div>
  );
}
