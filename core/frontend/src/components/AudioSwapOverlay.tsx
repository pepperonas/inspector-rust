import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Check,
  Download,
  Film,
  FolderOpen,
  Loader2,
  Music,
  Wand2,
  X,
  Link2,
} from "lucide-react";
import {
  audioSwapApply,
  audioSwapCancelOverlay,
  audioSwapDownloadYoutube,
  audioSwapGetSelectedVideo,
  audioSwapProbe,
  audioSwapYtdlpAvailable,
  ERR_NO_YTDLP,
  type SwapMode,
} from "../lib/ipc";

/**
 * Audio-swap overlay (`Ctrl+Shift+Alt+M`). Replace or overlay a video's audio
 * with a local file or a yt-dlp'd YouTube track, placed at a chosen start
 * position. Writes a sibling `<name>-audioswap.mp4` (non-destructive).
 */

interface Media {
  path: string;
  name: string;
  duration: number; // seconds (0 if unknown)
}

const VIDEO_EXTS = ["mp4", "mov", "m4v", "mkv", "avi", "webm"];
const AUDIO_EXTS = ["mp3", "m4a", "aac", "wav", "flac", "ogg", "opus", "aiff"];

function baseName(p: string): string {
  return p.split(/[/\\]/).pop() || p;
}
function fmtTime(s: number): string {
  if (!Number.isFinite(s) || s < 0) s = 0;
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${sec.toString().padStart(2, "0")}`;
}

export function AudioSwapOverlay() {
  const [video, setVideo] = useState<Media | null>(null);
  const [audio, setAudio] = useState<Media | null>(null);

  const [mode, setMode] = useState<SwapMode>("replace");
  const [startSeconds, setStartSeconds] = useState(0);
  const [audioIn, setAudioIn] = useState(0);
  const [audioOut, setAudioOut] = useState<number | null>(null);
  const [overlayVolume, setOverlayVolume] = useState(1);
  const [originalVolume, setOriginalVolume] = useState(0.6);

  const [ytUrl, setYtUrl] = useState("");
  const [ytAvailable, setYtAvailable] = useState(true);
  const [downloading, setDownloading] = useState(false);

  const [applying, setApplying] = useState(false);
  const [doneMsg, setDoneMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const close = useCallback(() => {
    void audioSwapCancelOverlay();
  }, []);

  // Esc closes.
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

  // Preload the Finder-selected video + probe yt-dlp availability.
  const loadedRef = useRef(false);
  useEffect(() => {
    if (loadedRef.current) return;
    loadedRef.current = true;
    void audioSwapYtdlpAvailable().then(setYtAvailable).catch(() => setYtAvailable(false));
    void audioSwapGetSelectedVideo().then(async (p) => {
      if (p) await loadVideo(p);
    });
  }, []);

  async function loadVideo(path: string) {
    const duration = (await audioSwapProbe(path).catch(() => null)) ?? 0;
    setVideo({ path, name: baseName(path), duration });
  }
  async function loadAudio(path: string) {
    const duration = (await audioSwapProbe(path).catch(() => null)) ?? 0;
    setAudio({ path, name: baseName(path), duration });
    setAudioIn(0);
    setAudioOut(null); // to end
    setStartSeconds((s) => (video ? Math.min(s, video.duration) : s));
    setDoneMsg(null);
  }

  async function pickVideo() {
    const sel = await openDialog({ multiple: false, filters: [{ name: "Video", extensions: VIDEO_EXTS }] });
    if (typeof sel === "string") await loadVideo(sel);
  }
  async function pickAudio() {
    const sel = await openDialog({ multiple: false, filters: [{ name: "Audio", extensions: AUDIO_EXTS }] });
    if (typeof sel === "string") await loadAudio(sel);
  }

  async function downloadYt() {
    if (!ytUrl.trim() || downloading) return;
    setDownloading(true);
    setError(null);
    try {
      const path = await audioSwapDownloadYoutube(ytUrl.trim());
      await loadAudio(path);
    } catch (e) {
      const msg = String(e);
      setError(msg === ERR_NO_YTDLP ? "yt-dlp is not installed — run: brew install yt-dlp" : `Download failed: ${msg}`);
    } finally {
      setDownloading(false);
    }
  }

  async function apply() {
    if (!video || !audio || applying) return;
    setApplying(true);
    setError(null);
    setDoneMsg(null);
    try {
      const out = await audioSwapApply(video.path, audio.path, {
        mode,
        startSeconds,
        audioIn,
        audioOut,
        overlayVolume,
        originalVolume,
        videoSeconds: video.duration,
      });
      setDoneMsg(baseName(out));
    } catch (e) {
      setError(`Failed: ${String(e)}`);
    } finally {
      setApplying(false);
    }
  }

  const audioLen = audio ? (audioOut ?? audio.duration) - audioIn : 0;
  const ready = !!video && !!audio;

  return (
    <div className="md3-pop-in flex h-screen w-screen flex-col overflow-y-auto bg-[var(--color-bg)] text-[var(--color-fg)]">
      {/* Header */}
      <div className="sticky top-0 z-10 flex items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-bg)] px-4 py-2.5">
        <div className="flex items-center gap-2 text-[13px] font-semibold">
          <Wand2 size={14} className="text-[var(--color-accent)]" />
          Replace / overlay audio
        </div>
        <button onClick={close} className="md3-press rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]" title="Close (Esc)">
          <X size={15} />
        </button>
      </div>

      <div className="flex flex-1 flex-col gap-4 p-4">
        {/* 1) Video */}
        <Section icon={<Film size={13} />} title="Video">
          {video ? (
            <div className="flex items-center justify-between gap-2">
              <div className="min-w-0">
                <div className="truncate text-[13px] font-medium">{video.name}</div>
                <div className="text-[11px] text-[var(--color-muted)]">{fmtTime(video.duration)} long</div>
              </div>
              <button onClick={pickVideo} className="md3-press shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[11px] hover:border-[var(--color-accent)]">
                Change
              </button>
            </div>
          ) : (
            <button onClick={pickVideo} className="md3-press flex w-full items-center justify-center gap-2 rounded border border-dashed border-[var(--color-border)] px-3 py-3 text-[12px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]">
              <FolderOpen size={14} /> No video selected in Finder — choose one
            </button>
          )}
        </Section>

        {/* 2) Audio source */}
        <Section icon={<Music size={13} />} title="Audio to add">
          {audio && (
            <div className="mb-2 flex items-center justify-between gap-2 rounded bg-[var(--color-surface)] px-2 py-1.5">
              <div className="min-w-0">
                <div className="truncate text-[13px] font-medium">{audio.name}</div>
                <div className="text-[11px] text-[var(--color-muted)]">{fmtTime(audio.duration)}</div>
              </div>
              <Check size={14} className="shrink-0 text-emerald-500" />
            </div>
          )}
          <button onClick={pickAudio} className="md3-press mb-2 flex w-full items-center justify-center gap-2 rounded border border-[var(--color-border)] px-3 py-2 text-[12px] hover:border-[var(--color-accent)]">
            <FolderOpen size={14} /> Choose audio file…
          </button>
          <div className="flex items-center gap-1.5">
            <Link2 size={15} className={ytAvailable ? "text-rose-500" : "text-[var(--color-muted)]"} />
            <input
              value={ytUrl}
              onChange={(e) => setYtUrl(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && downloadYt()}
              disabled={!ytAvailable || downloading}
              placeholder={ytAvailable ? "Paste a YouTube URL…" : "yt-dlp not installed"}
              className="min-w-0 flex-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[12px] outline-none focus:border-[var(--color-accent)] disabled:opacity-50"
            />
            <button
              onClick={downloadYt}
              disabled={!ytAvailable || downloading || !ytUrl.trim()}
              className="md3-press flex shrink-0 items-center gap-1 rounded bg-[var(--color-accent)] px-2.5 py-1.5 text-[12px] text-[var(--color-accent-fg)] disabled:opacity-40"
            >
              {downloading ? <Loader2 size={13} className="animate-spin" /> : <Download size={13} />}
              {downloading ? "Downloading…" : "Get"}
            </button>
          </div>
          {!ytAvailable && (
            <div className="mt-1 text-[11px] text-[var(--color-muted)]">
              Install with <code className="rounded bg-[var(--color-surface)] px-1">brew install yt-dlp</code> to download from YouTube.
            </div>
          )}
        </Section>

        {/* 3) Placement / trim / mode */}
        {ready && (
          <Section icon={<Wand2 size={13} />} title="Placement">
            <Slider
              label="Insert at (video position)"
              value={startSeconds}
              min={0}
              max={video.duration}
              onChange={setStartSeconds}
              display={fmtTime(startSeconds)}
            />
            {audio.duration > 0 && (
              <>
                <Slider
                  label="Audio start (trim in)"
                  value={audioIn}
                  min={0}
                  max={Math.max(0, (audioOut ?? audio.duration) - 0.1)}
                  onChange={setAudioIn}
                  display={fmtTime(audioIn)}
                />
                <Slider
                  label="Audio end (trim out)"
                  value={audioOut ?? audio.duration}
                  min={audioIn + 0.1}
                  max={audio.duration}
                  onChange={(v) => setAudioOut(v >= audio.duration ? null : v)}
                  display={fmtTime(audioOut ?? audio.duration)}
                />
                <div className="mb-2 text-[11px] text-[var(--color-muted)]">Inserted length: {fmtTime(audioLen)}</div>
              </>
            )}

            <div className="mb-2 flex gap-1">
              <ModeButton active={mode === "replace"} onClick={() => setMode("replace")} label="Replace audio" hint="drop original" />
              <ModeButton active={mode === "mix"} onClick={() => setMode("mix")} label="Mix over original" hint="keep both" />
            </div>
            {mode === "mix" && (
              <>
                <Slider label="New audio volume" value={overlayVolume} min={0} max={2} step={0.05} onChange={setOverlayVolume} display={`${Math.round(overlayVolume * 100)}%`} />
                <Slider label="Original volume" value={originalVolume} min={0} max={1.5} step={0.05} onChange={setOriginalVolume} display={`${Math.round(originalVolume * 100)}%`} />
              </>
            )}
          </Section>
        )}

        {error && <div className="rounded border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-[12px] text-rose-400">{error}</div>}
        {doneMsg && (
          <div className="md3-banner-in flex items-center gap-2 rounded border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-[12px] text-emerald-400">
            <Check size={14} /> Saved <b>{doneMsg}</b> next to the video (revealed in Finder).
          </div>
        )}
      </div>

      {/* Apply bar */}
      <div className="sticky bottom-0 border-t border-[var(--color-border)] bg-[var(--color-bg)] p-3">
        <button
          onClick={apply}
          disabled={!ready || applying}
          className="md3-press flex w-full items-center justify-center gap-2 rounded bg-[var(--color-accent)] px-4 py-2.5 text-[13px] font-semibold text-[var(--color-accent-fg)] disabled:opacity-40"
        >
          {applying ? <Loader2 size={15} className="animate-spin" /> : <Wand2 size={15} />}
          {applying ? "Processing…" : mode === "replace" ? "Replace audio & save" : "Mix audio & save"}
        </button>
      </div>
    </div>
  );
}

function Section({ icon, title, children }: { icon: React.ReactNode; title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-lg border border-[var(--color-border)] p-3">
      <div className="mb-2 flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">
        <span className="text-[var(--color-accent)]">{icon}</span>
        {title}
      </div>
      {children}
    </div>
  );
}

function ModeButton({ active, onClick, label, hint }: { active: boolean; onClick: () => void; label: string; hint: string }) {
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
  label,
  value,
  min,
  max,
  step = 0.1,
  onChange,
  display,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (v: number) => void;
  display: string;
}) {
  return (
    <div className="mb-2.5">
      <div className="mb-1 flex justify-between text-[11px]">
        <span className="text-[var(--color-muted)]">{label}</span>
        <span className="font-[var(--font-mono)] font-medium">{display}</span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        className="h-1.5 w-full cursor-pointer appearance-none rounded-full bg-[var(--color-surface)] accent-[var(--color-accent)]"
      />
    </div>
  );
}
