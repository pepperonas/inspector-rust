import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import { Crop, Download, Gauge, Pause, Play, Redo2, RotateCcw, Scissors, Trash2, Undo2 } from "lucide-react";
import {
  editorCurrent,
  editorPlay,
  editorPosition,
  editorRestart,
  editorSeek,
  editorThumbs,
  editorUpdate,
  type EditorInit,
  type RecordEditDoc,
  type RecordEditorView,
} from "../lib/ipc";
import {
  History,
  MAX_MIC_DB,
  MIN_MIC_DB,
  addSpeed,
  aspectRatio,
  clipAt,
  cropForPreset,
  deleteClip,
  formatTimecode,
  isTypingTarget,
  removeSpeed,
  setCrop,
  setMicGain,
  setSpeedValue,
  splitAt,
  type CropPreset,
} from "../lib/editor";
import { useT, type MessageKey } from "../lib/i18n";
import { Preview } from "./editor/Preview";
import { Timeline } from "./editor/Timeline";
import { CropOverlay } from "./editor/CropOverlay";
import { ExportDialog } from "./editor/ExportDialog";
import { AudioPreview } from "./editor/AudioPreview";

const POSITION_POLL_MS = 66;
const THUMB_DEBOUNCE_MS = 600;
const THUMB_PX = 96;
const SAVED_AFTER_MS = 1000;
/** New speed regions span this much source time from the playhead. */
const NEW_SPEED_MS = 2000;

const errText = (t: (k: MessageKey) => string, reason: string) =>
  reason.startsWith("record.editor.") ? t(reason as MessageKey) : reason;

/** The `record-editor` window: preview, transport, timeline, crop mode, undo, export. */
export function RecordEditor() {
  const t = useT();
  const [init, setInit] = useState<EditorInit | null | undefined>(undefined);
  const [view, setView] = useState<RecordEditorView | null>(null);
  const history = useRef<History | null>(null);
  const [undoable, setUndoable] = useState({ undo: false, redo: false });
  const micStart = useRef<RecordEditDoc | null>(null);
  const [pos, setPos] = useState(0);
  const posRef = useRef(0);
  const [playing, setPlaying] = useState(false);
  const [fatal, setFatal] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [cropMode, setCropMode] = useState(false);
  const [cropSourceMs, setCropSourceMs] = useState(0);
  const [selected, setSelected] = useState<number | null>(null);
  const [exportOpen, setExportOpen] = useState(false);
  const [thumbs, setThumbs] = useState<string[]>([]);
  const [trackWidth, setTrackWidth] = useState(0);
  const chain = useRef<Promise<unknown>>(Promise.resolve());
  const savedTimer = useRef(0);

  useEffect(() => {
    let dead = false;
    void editorCurrent()
      .then((i) => {
        if (dead) return;
        setInit(i);
        if (i) {
          history.current = new History(i.view.doc);
          setView(i.view);
          setFatal(i.helperError);
        }
      })
      .catch(() => !dead && setInit(null));
    return () => {
      dead = true;
    };
  }, []);

  useEffect(() => {
    const off = listen<{ reason: string }>("record-editor-fatal", (e) => {
      setFatal(e.payload.reason || "?");
      setPlaying(false);
    });
    return () => void off.then((u) => u());
  }, []);

  const movePos = useCallback((p: number) => {
    posRef.current = p;
    setPos(p);
  }, []);

  // Playhead while playing (the app owns the clock in Rust; it also stops at the end).
  useEffect(() => {
    if (!playing) return;
    const id = window.setInterval(() => {
      editorPosition()
        .then((s) => {
          if (!s) return;
          movePos(s.positionMs);
          if (!s.playing) setPlaying(false);
          if (s.helperError) setFatal(s.helperError);
        })
        .catch(() => undefined);
    }, POSITION_POLL_MS);
    return () => window.clearInterval(id);
  }, [playing, movePos]);

  // Filmstrip: refreshed after edits/resizes settle.
  const programmeMs = view?.programmeMs ?? 0;
  useEffect(() => {
    if (!view || fatal || trackWidth <= 0) return;
    const count = Math.min(60, Math.max(4, Math.round(trackWidth / THUMB_PX)));
    const id = window.setTimeout(() => {
      editorThumbs(count).then(setThumbs).catch(() => undefined);
    }, THUMB_DEBOUNCE_MS);
    return () => window.clearTimeout(id);
  }, [view, trackWidth, fatal]);

  /** Sends `doc` to Rust (serialised) and shows the normalised answer. */
  const send = useCallback((doc: RecordEditDoc, record: boolean) => {
    const h = history.current;
    if (!h) return Promise.resolve();
    if (record) h.push(doc);
    setUndoable({ undo: h.canUndo(), redo: h.canRedo() });
    setSaving(true);
    window.clearTimeout(savedTimer.current);
    const next = chain.current.then(async () => {
      try {
        const v = await editorUpdate(doc);
        if (h.present === doc) h.replace(v.doc);
        setView(v);
        if (posRef.current > v.programmeMs) movePos(v.programmeMs);
      } catch (e) {
        setFatal(String(e));
      } finally {
        savedTimer.current = window.setTimeout(() => setSaving(false), SAVED_AFTER_MS);
      }
    });
    chain.current = next;
    return next;
  }, [movePos]);

  const commit = useCallback((doc: RecordEditDoc) => void send(doc, true), [send]);

  const undo = useCallback(() => {
    const d = history.current?.undo();
    if (d) void send(d, false);
  }, [send]);
  const redo = useCallback(() => {
    const d = history.current?.redo();
    if (d) void send(d, false);
  }, [send]);

  const seek = useCallback(
    (p: number) => {
      const clamped = Math.min(programmeMs, Math.max(0, p));
      movePos(clamped);
      return editorSeek(clamped).catch(() => null);
    },
    [programmeMs, movePos],
  );

  const togglePlay = useCallback(() => {
    const want = !playing;
    setPlaying(want);
    editorPlay(want)
      .then((s) => {
        movePos(s.positionMs);
        setPlaying(s.playing);
      })
      .catch(() => setPlaying(false));
  }, [playing, movePos]);

  const split = useCallback(async () => {
    const at = await editorSeek(posRef.current).catch(() => null);
    const doc = history.current?.present;
    if (!at || !doc) return;
    const next = splitAt(doc, at.clip, at.sourceMs);
    if (next !== doc) await send(next, true);
  }, [send]);

  /** The clip edits apply to: the selected one, else the one under `at`. */
  const clipFor = useCallback(
    (at: number) => (view ? (selected != null && selected < view.clips.length ? selected : clipAt(view, at)) : 0),
    [view, selected],
  );

  const removeClip = useCallback(() => {
    const doc = history.current?.present;
    if (!doc) return;
    const next = deleteClip(doc, clipFor(posRef.current));
    if (next !== doc) {
      setSelected(null);
      commit(next);
    }
  }, [commit, clipFor]);

  const addSpeedHere = useCallback(async () => {
    const at = await editorSeek(posRef.current).catch(() => null);
    const doc = history.current?.present;
    if (!at || !doc) return;
    const end = Math.min(doc.clips[at.clip]?.endMs ?? at.sourceMs, at.sourceMs + NEW_SPEED_MS);
    if (end - at.sourceMs < 100) return;
    commit(addSpeed(doc, at.clip, at.sourceMs, end, 2));
  }, [commit]);

  const enterCrop = useCallback(async () => {
    if (playing) togglePlay();
    const at = await editorSeek(posRef.current).catch(() => null);
    setCropSourceMs(at?.sourceMs ?? 0);
    setCropMode(true);
  }, [playing, togglePlay]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (exportOpen || isTypingTarget(e.target) || !view) return;
      const mod = e.metaKey || e.ctrlKey;
      const k = e.key.toLowerCase();
      if (mod && k === "z") {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
        return;
      }
      if (mod || e.altKey) return;
      const frameMs = 1000 / Math.max(1, init?.meta.fps ?? 30);
      if (e.key === "Escape" && cropMode) {
        e.preventDefault();
        setCropMode(false);
      } else if (e.key === " ") {
        e.preventDefault();
        togglePlay();
      } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        const step = e.shiftKey ? 1000 : frameMs;
        void seek(posRef.current + (e.key === "ArrowLeft" ? -step : step));
      } else if (k === "s") {
        e.preventDefault();
        void split();
      } else if (e.key === "Backspace" || e.key === "Delete") {
        e.preventDefault();
        removeClip();
      } else if (k === "c") {
        e.preventDefault();
        if (cropMode) setCropMode(false);
        else void enterCrop();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [view, init, exportOpen, cropMode, togglePlay, seek, split, removeClip, enterCrop, undo, redo]);

  // Mic gain: live while dragging (preview volume only), one undo step per gesture.
  const micLive = (db: number) => {
    const h = history.current;
    if (!h || !view) return;
    micStart.current ??= h.present;
    const next = setMicGain(h.present, db);
    h.replace(next);
    setView({ ...view, doc: next });
  };
  const micDone = () => {
    const h = history.current;
    const start = micStart.current;
    micStart.current = null;
    if (!h || !start) return;
    const final = h.present;
    h.replace(start);
    if (final.micGainDb !== start.micGainDb) commit(final);
  };

  const restart = async () => {
    try {
      const s = await editorRestart();
      setFatal(s.helperError);
    } catch (e) {
      setFatal(String(e));
    }
  };

  if (init === undefined) return <div className="h-screen w-screen bg-[var(--color-bg)]" />;
  if (init === null || !view) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-[var(--color-bg)] text-[13px] text-[var(--color-muted)]">
        {t("record.editor.no_session")}
      </div>
    );
  }

  const doc = view.doc;
  const meta = init.meta;
  const dir = init.project.dir;
  const ratio = aspectRatio(doc.output.aspect, meta.width, meta.height);
  const cropClip = clipFor(pos);
  const preset: CropPreset = doc.output.aspect === "source" ? "free" : doc.output.aspect;
  const asset = (f: string) => convertFileSrc(`${dir}/${f}`);

  const iconBtn =
    "flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-2.5 py-1.5 text-[12px] hover:bg-[var(--color-bg)] disabled:opacity-40";

  return (
    <div className="flex h-screen w-screen flex-col bg-[var(--color-bg)] text-[var(--color-fg)]">
      {/* Header */}
      <header className="flex items-center gap-3 border-b border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-2">
        <h1 className="min-w-0 truncate text-[14px] font-semibold">{init.project.title || t("rec2.default_title")}</h1>
        <span className="text-[11px] text-[var(--color-muted)]" data-testid="ed-save">
          {saving ? t("rec2.ed.saving") : t("rec2.ed.saved")}
        </span>
        <div className="flex-1" />
        <button className={iconBtn} onClick={undo} disabled={!undoable.undo} title={t("rec2.ed.undo")} aria-label={t("rec2.ed.undo")}>
          <Undo2 size={14} />
        </button>
        <button className={iconBtn} onClick={redo} disabled={!undoable.redo} title={t("rec2.ed.redo")} aria-label={t("rec2.ed.redo")}>
          <Redo2 size={14} />
        </button>
        <button
          onClick={() => {
            if (playing) togglePlay();
            setExportOpen(true);
          }}
          className="flex items-center gap-1.5 rounded-lg bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)]"
        >
          <Download size={14} /> {t("rec2.ed.export")}
        </button>
      </header>

      {/* Preview */}
      <Preview ratio={ratio} active={!fatal && !cropMode} dim={!!fatal}>
        {cropMode && !fatal && (
          <CropOverlay
            srcW={meta.width}
            srcH={meta.height}
            videoSrc={asset("screen.mp4")}
            sourceMs={cropSourceMs}
            rect={doc.crop[cropClip] ?? null}
            preset={preset}
            onPreset={(p) => {
              const aspect = p === "free" ? "source" : p;
              const rect = p === "free" ? (doc.crop[cropClip] ?? null) : cropForPreset(p, meta.width, meta.height);
              commit({ ...setCrop(doc, cropClip, rect), output: { aspect } });
            }}
            onCommit={(r) => commit(setCrop(doc, cropClip, r))}
            onReset={() => commit({ ...setCrop(doc, cropClip, null), output: { aspect: "source" } })}
          />
        )}
        {fatal && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 p-6 text-center" data-testid="ed-fatal">
            <p className="max-w-md text-[13px] text-white/90">{t("rec2.ed.fatal")}</p>
            <p className="max-w-md break-words text-[12px] text-white/60">{errText(t, fatal)}</p>
            <button
              onClick={restart}
              className="flex items-center gap-1.5 rounded-lg bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)]"
            >
              <RotateCcw size={14} /> {t("rec2.ed.restart")}
            </button>
          </div>
        )}
      </Preview>

      {/* Transport + tools */}
      <div className="flex items-center gap-2 border-t border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-2">
        <button
          className={iconBtn}
          onClick={togglePlay}
          disabled={!!fatal}
          aria-label={playing ? t("rec2.ed.pause") : t("rec2.ed.play")}
          title={`${playing ? t("rec2.ed.pause") : t("rec2.ed.play")} (Space)`}
        >
          {playing ? <Pause size={14} fill="currentColor" /> : <Play size={14} fill="currentColor" />}
        </button>
        <span className="text-[12px] tabular-nums" data-testid="ed-time">
          {formatTimecode(pos)} <span className="text-[var(--color-muted)]">/ {formatTimecode(programmeMs)}</span>
        </span>
        <div className="mx-2 h-5 w-px bg-[var(--color-border)]" />
        <button className={iconBtn} onClick={() => void split()} title={`${t("rec2.ed.split")} (S)`}>
          <Scissors size={14} /> {t("rec2.ed.split")}
        </button>
        <button className={iconBtn} onClick={removeClip} disabled={doc.clips.length <= 1} title={`${t("rec2.ed.delete")} (⌫)`}>
          <Trash2 size={14} /> {t("rec2.ed.delete")}
        </button>
        <button className={iconBtn} onClick={() => void addSpeedHere()} title={t("rec2.ed.speed_add_hint")}>
          <Gauge size={14} /> {t("rec2.ed.speed_add")}
        </button>
        <button
          className={iconBtn + (cropMode ? " border-[var(--color-accent)] text-[var(--color-accent)]" : "")}
          onClick={() => (cropMode ? setCropMode(false) : void enterCrop())}
          title={`${t("rec2.ed.crop")} (C)`}
        >
          <Crop size={14} /> {t("rec2.ed.crop")}
        </button>
        <div className="flex-1" />
        {meta.mic && (
          <label className="flex items-center gap-2 text-[12px] text-[var(--color-muted)]">
            {t("rec2.ed.mic")}
            <input
              type="range"
              min={MIN_MIC_DB}
              max={MAX_MIC_DB}
              step={1}
              value={Math.round(doc.micGainDb)}
              onChange={(e) => micLive(Number(e.target.value))}
              onPointerUp={micDone}
              onKeyUp={micDone}
              onBlur={micDone}
              className="w-28"
              aria-label={t("rec2.ed.mic")}
            />
            <span className="w-12 tabular-nums">{Math.round(doc.micGainDb)} dB</span>
          </label>
        )}
      </div>

      <div className="border-t border-[var(--color-border)] bg-[var(--color-surface)]">
        <Timeline
          view={view}
          thumbs={thumbs}
          positionMs={pos}
          durationMs={meta.durationMs}
          selectedClip={selected}
          onSelect={setSelected}
          onSeek={(p) => void seek(p)}
          onChange={commit}
          onSpeedValue={(i, v) => commit(setSpeedValue(doc, i, v))}
          onSpeedRemove={(i) => commit(removeSpeed(doc, i))}
          onWidth={setTrackWidth}
        />
      </div>

      <AudioPreview
        screenSrc={meta.systemAudio ? asset("screen.mp4") : null}
        micSrc={meta.mic ? asset("mic.wav") : null}
        micOffsetMs={meta.mic?.offsetMs ?? 0}
        view={view}
        playing={playing && !fatal}
        positionMs={pos}
      />
      {exportOpen && <ExportDialog onClose={() => setExportOpen(false)} />}
    </div>
  );
}
