import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Check, FolderOpen, Loader2, Scissors, X } from "lucide-react";
import { trimApply, trimCancelOverlay, trimFileInfo } from "../lib/ipc";

/**
 * Trim overlay (`trim` command). Pick a local audio/video file, set start/end,
 * and cut it — lossless & fast (`-c copy`, keyframe-snapped) or frame-accurate
 * (re-encode). Writes a sibling `<name>-trim.<ext>` (non-destructive).
 */

const MEDIA_EXTS = [
  "mp4", "mov", "m4v", "mkv", "avi", "webm", // video
  "mp3", "m4a", "aac", "wav", "flac", "ogg", "opus", "aiff", // audio
];

function baseName(p: string): string {
  return p.split(/[/\\]/).pop() || p;
}
function fmtTime(s: number): string {
  if (!Number.isFinite(s) || s < 0) s = 0;
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${sec.toString().padStart(2, "0")}`;
}

export function TrimOverlay() {
  const [path, setPath] = useState<string | null>(null);
  const [duration, setDuration] = useState(0);
  const [isVideo, setIsVideo] = useState(false);
  const [start, setStart] = useState(0);
  const [end, setEnd] = useState(0);
  const [lossless, setLossless] = useState(true);

  const [applying, setApplying] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const close = useCallback(() => void trimCancelOverlay(), []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [close]);

  const load = useCallback(async (p: string) => {
    const info = await trimFileInfo(p).catch(() => null);
    setPath(p);
    const dur = info?.duration ?? 0;
    setDuration(dur);
    setIsVideo(info?.is_video ?? false);
    setStart(0);
    setEnd(dur);
    setDone(null);
    setError(null);
  }, []);

  const pick = useCallback(async () => {
    const sel = await openDialog({ multiple: false, filters: [{ name: "Media", extensions: MEDIA_EXTS }] });
    if (typeof sel === "string") await load(sel);
  }, [load]);

  // Auto-open the file picker once on mount (the command's intent: pick a file).
  const pickedRef = useRef(false);
  useEffect(() => {
    if (pickedRef.current) return;
    pickedRef.current = true;
    void pick();
  }, [pick]);

  const apply = async () => {
    if (!path || applying || end <= start) return;
    setApplying(true);
    setError(null);
    setDone(null);
    try {
      const out = await trimApply(path, start, end, lossless);
      setDone(baseName(out));
    } catch (e) {
      setError(`Failed: ${String(e)}`);
    } finally {
      setApplying(false);
    }
  };

  return (
    <div className="md3-pop-in flex h-screen w-screen flex-col overflow-y-auto bg-[var(--color-bg)] text-[var(--color-fg)]">
      <div className="sticky top-0 z-10 flex items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-bg)] px-4 py-2.5">
        <div className="flex items-center gap-2 text-[13px] font-semibold">
          <Scissors size={14} className="text-[var(--color-accent)]" />
          Trim audio / video
        </div>
        <button onClick={close} className="md3-press rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]" title="Close (Esc)">
          <X size={15} />
        </button>
      </div>

      <div className="flex flex-1 flex-col gap-4 p-4">
        {/* File */}
        {path ? (
          <div className="flex items-center justify-between gap-2 rounded-lg border border-[var(--color-border)] p-3">
            <div className="min-w-0">
              <div className="truncate text-[13px] font-medium">{baseName(path)}</div>
              <div className="text-[11px] text-[var(--color-muted)]">
                {fmtTime(duration)} · {isVideo ? "video" : "audio"}
              </div>
            </div>
            <button onClick={pick} className="md3-press shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[11px] hover:border-[var(--color-accent)]">
              Change
            </button>
          </div>
        ) : (
          <button onClick={pick} className="md3-press flex w-full items-center justify-center gap-2 rounded border border-dashed border-[var(--color-border)] px-3 py-4 text-[12px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]">
            <FolderOpen size={14} /> Choose audio / video file…
          </button>
        )}

        {path && duration > 0 && (
          <>
            <Slider label="Start" value={start} min={0} max={Math.max(0, end - 0.1)} onChange={setStart} display={fmtTime(start)} />
            <Slider label="End" value={end} min={start + 0.1} max={duration} onChange={setEnd} display={fmtTime(end)} />
            <div className="text-[11px] text-[var(--color-muted)]">Result length: {fmtTime(end - start)}</div>

            <div className="flex gap-1">
              <ModeBtn active={lossless} onClick={() => setLossless(true)} label="Lossless & fast" hint="−c copy · keyframe-snapped" />
              <ModeBtn active={!lossless} onClick={() => setLossless(false)} label="Frame-accurate" hint="re-encode · exact cut" />
            </div>
          </>
        )}

        {error && <div className="rounded border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-[12px] text-rose-400">{error}</div>}
        {done && (
          <div className="md3-banner-in flex items-center gap-2 rounded border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-[12px] text-emerald-400">
            <Check size={14} /> Saved <b>{done}</b> next to the source (revealed in Finder).
          </div>
        )}
      </div>

      <div className="sticky bottom-0 border-t border-[var(--color-border)] bg-[var(--color-bg)] p-3">
        <button
          onClick={apply}
          disabled={!path || duration <= 0 || applying || end <= start}
          className="md3-press flex w-full items-center justify-center gap-2 rounded bg-[var(--color-accent)] px-4 py-2.5 text-[13px] font-semibold text-[var(--color-accent-fg)] disabled:opacity-40"
        >
          {applying ? <Loader2 size={15} className="animate-spin" /> : <Scissors size={15} />}
          {applying ? "Trimming…" : "Trim & save"}
        </button>
      </div>
    </div>
  );
}

function ModeBtn({ active, onClick, label, hint }: { active: boolean; onClick: () => void; label: string; hint: string }) {
  return (
    <button
      onClick={onClick}
      className={
        "md3-press flex-1 rounded border px-2 py-1.5 text-[12px] " +
        (active
          ? "border-[var(--color-accent)] bg-[var(--color-accent)]/15 text-[var(--color-fg)]"
          : "border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-accent)]")
      }
    >
      <div className="font-medium">{label}</div>
      <div className="text-[10px] opacity-70">{hint}</div>
    </button>
  );
}

function Slider({
  label, value, min, max, onChange, display,
}: {
  label: string; value: number; min: number; max: number; onChange: (v: number) => void; display: string;
}) {
  return (
    <div>
      <div className="mb-1 flex justify-between text-[11px]">
        <span className="text-[var(--color-muted)]">{label}</span>
        <span className="font-[var(--font-mono)] font-medium">{display}</span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={0.1}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        className="h-1.5 w-full cursor-pointer appearance-none rounded-full bg-[var(--color-surface)] accent-[var(--color-accent)]"
      />
    </div>
  );
}
