import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Copy,
  Crop,
  Download,
  FilePlus2,
  Gauge,
  PanelRightClose,
  PanelRightOpen,
  Pause,
  Play,
  Redo2,
  RotateCcw,
  Scissors,
  Trash2,
  Type,
  Undo2,
  ZoomIn,
  WandSparkles,
} from "lucide-react";
import {
  editorAssets,
  editorAutoZoom,
  editorCurrent,
  editorDuplicateClip,
  editorImportAsset,
  editorImportTake,
  editorMoveClip,
  editorPlay,
  editorPosition,
  editorRestart,
  editorSeek,
  editorThumbs,
  editorToSource,
  editorUpdate,
  editorVoiceoverCancel,
  editorVoiceoverStart,
  editorVoiceoverStop,
  editorTrackEdge,
  EV_VOICEOVER_FINISHED,
  EV_VOICEOVER_LEVEL,
  type EditorVoiceoverLevel,
  type RecordWaveformsEvent,
  type EditorAssets,
  type EditorInit,
  type EditorPlayback,
  type EditorSourcePos,
  type RecordAnnotationKind,
  type RecordEditDoc,
  type RecordEditorView,
} from "../lib/ipc";
import {
  History,
  LiveGesture,
  MAX_MIC_DB,
  MIN_MIC_DB,
  addAnnotation,
  addSpeed,
  addTrack,
  addZoom,
  clipAt,
  clipMedia,
  cropForPreset,
  deleteClip,
  formatTimecode,
  isTypingTarget,
  previewRatio,
  removeSpeed,
  setCrop,
  setMicGain,
  setSpeedValue,
  splitAt,
  updateAnnotation,
  updateTrack,
  updateZoom,
  zoomActiveAt,
  annotationShownAt,
  type ClipMedia,
  type CropPreset,
} from "../lib/editor";
import { tn, useT, type MessageKey } from "../lib/i18n";
import { Preview } from "./editor/Preview";
import { Timeline } from "./editor/Timeline";
import { CropOverlay } from "./editor/CropOverlay";
import { ExportDialog } from "./editor/ExportDialog";
import { TakePicker } from "./editor/TakePicker";
import { AudioPreview } from "./editor/AudioPreview";
import { Inspector } from "./editor/Inspector";
import { FocusOverlay } from "./editor/FocusOverlay";
import { AnnotationOverlay, type AnnotationPatch } from "./editor/AnnotationOverlay";
import { registerAnnotationFonts } from "./editor/AnnotationTab";
import { GAIN_STEP_DB, VoiceoverPanel, snap, type VoiceoverView } from "./editor/AudioTab";
import { VOICEOVER_DUCK_DB } from "../lib/audio-mix";

const POSITION_POLL_MS = 66;
const THUMB_DEBOUNCE_MS = 600;
const THUMB_PX = 96;
const SAVED_AFTER_MS = 1000;
/** New speed regions span this much source time from the playhead. */
const NEW_SPEED_MS = 2000;
const NOTICE_MS = 4000;
/** Per-viewer: whether the settings column is open (default open). */
const INSPECTOR_KEY = "ir.record-editor.inspector-open";

function readInspectorOpen(): boolean {
  try {
    return localStorage.getItem(INSPECTOR_KEY) !== "0";
  } catch {
    return true;
  }
}

/** Music the editor imports (Rust checks extension and content again). */
export const AUDIO_EXTENSIONS = ["mp3", "m4a", "aac", "wav", "flac"] as const;
/** A track's default name: the picked file's name without folder and extension. */
/** A document file name the preview may load: a plain name under `assets/` (as Rust's scene builder requires). */
export const isPlainAssetName = (f: string) => !!f && !/[\\/]/.test(f) && !f.startsWith(".");
export const fileStem = (path: string) =>
  (path.split(/[\\/]/).pop() ?? "").replace(/\.[^.]+$/, "").trim().slice(0, 80);

/** A shipped asset's path → URL (a `data:` URL, as the mockup sends, passes unchanged). */
const shippedUrl = (p: string) => (p.startsWith("data:") ? p : convertFileSrc(p));

const errText = (t: (k: MessageKey) => string, reason: string) =>
  reason.startsWith("record.editor.") ? t(reason as MessageKey) : reason;

/**
 * The `record-editor` window. Opening a project while the window already exists replaces
 * the Rust session (`record-editor-opened`); the whole editor then remounts, so no state of
 * the previous project survives — its doc must never be sent to the new one, and the
 * preview's frame generation restarts at 0 with the new helper scene.
 */
export function RecordEditor() {
  const [mountKey, setMountKey] = useState(0);
  /** Session the mounted editor shows (set once its `editorCurrent` answered). */
  const shown = useRef<number | null>(null);
  useEffect(() => {
    const off = listen<{ sessionId: number }>("record-editor-opened", (e) => {
      if (e.payload.sessionId === shown.current) return;
      shown.current = e.payload.sessionId;
      setMountKey((k) => k + 1);
    });
    return () => void off.then((u) => u());
  }, []);
  const onSession = useCallback((id: number) => {
    shown.current = id;
  }, []);
  return <EditorSession key={mountKey} onSession={onSession} />;
}

/** One editor session: preview, transport, timeline, crop mode, undo, export. */
function EditorSession({ onSession }: { onSession: (sessionId: number) => void }) {
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
  const [cue, setCue] = useState<{ source: EditorSourcePos | null; speed: number }>({ source: null, speed: 1 });
  const [notice, setNotice] = useState<string | null>(null);
  const chain = useRef<Promise<unknown>>(Promise.resolve());
  const savedTimer = useRef(0);
  const noticeTimer = useRef(0);
  /** Sequence of `editorUpdate` calls; only the newest answer may update the view. */
  const seq = useRef(0);
  const seeking = useRef(false);
  /** The session this editor was mounted for; every update names it. */
  const sessionId = useRef(0);
  const pendingSeek = useRef<number | null>(null);
  const [assets, setAssets] = useState<EditorAssets | null>(null);
  /** Inspector sliders: live preview (debounced), one undo step on release. */
  const sendRef = useRef<((doc: RecordEditDoc, record: boolean) => Promise<unknown>) | null>(null);
  const liveRef = useRef<LiveGesture | null>(null);
  /** Created on first use (event time, never during render). */
  const gesture = useCallback(
    () => (liveRef.current ??= new LiveGesture(() => history.current, (doc, record) => void sendRef.current?.(doc, record))),
    [],
  );
  const [inspectorOpen, setInspectorOpen] = useState(readInspectorOpen);
  const [selectedZoom, setSelectedZoomRaw] = useState<string | null>(null);
  const [selectedAnnotation, setSelectedAnnotationRaw] = useState<string | null>(null);
  const [selectedTrack, setSelectedTrackRaw] = useState<string | null>(null);
  // One region is selected at a time: Delete and the inspector act on exactly one.
  const setSelectedZoom = useCallback((id: string | null) => {
    setSelectedZoomRaw(id);
    if (id) {
      setSelectedAnnotationRaw(null);
      setSelectedTrackRaw(null);
    }
  }, []);
  const setSelectedAnnotation = useCallback((id: string | null) => {
    setSelectedAnnotationRaw(id);
    if (id) {
      setSelectedZoomRaw(null);
      setSelectedTrackRaw(null);
    }
  }, []);
  const setSelectedTrack = useCallback((id: string | null) => {
    setSelectedTrackRaw(id);
    if (id) {
      setSelectedZoomRaw(null);
      setSelectedAnnotationRaw(null);
    }
  }, []);
  /** A running voiceover take (state for the UI, ref for event handlers). */
  const [voiceover, setVoiceoverState] = useState<VoiceoverView | null>(null);
  const voRef = useRef<VoiceoverView | null>(null);
  const setVoiceover = useCallback((v: VoiceoverView | null) => {
    voRef.current = v;
    setVoiceoverState(v);
  }, []);
  /** Esc while the take was still starting: discard it once Rust answered. */
  const voCancelAsked = useRef(false);
  /** Neutral one-liner beside the save state (e.g. what auto-zoom added). */
  const [info, setInfo] = useState<string | null>(null);
  const infoTimer = useRef(0);

  /** IPC failures are shown, never swallowed. */
  const report = useCallback((e: unknown) => {
    const msg = String(e);
    console.error("record editor", msg);
    setNotice(msg);
    window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), NOTICE_MS);
  }, []);

  useEffect(() => {
    let dead = false;
    void editorCurrent()
      .then((i) => {
        if (dead) return;
        setInit(i);
        if (i) {
          sessionId.current = i.sessionId;
          onSession(i.sessionId);
          history.current = new History(i.view.doc);
          setView(i.view);
          setFatal(i.helperError);
          editorAssets()
            .then((a) => {
              if (dead) return;
              setAssets(a);
              registerAnnotationFonts(a.fonts ?? [], shippedUrl);
            })
            .catch((e) => !dead && report(e));
        }
      })
      .catch(() => !dead && setInit(null));
    return () => {
      dead = true;
    };
  }, [onSession, report]);

  useEffect(() => {
    const off = listen<{ reason: string }>("record-editor-fatal", (e) => {
      setFatal(e.payload.reason || "?");
      setPlaying(false);
    });
    return () => void off.then((u) => u());
  }, []);

  // Waveforms arrive once their peaks are decoded (Rust: `record-editor-waveforms`).
  useEffect(() => {
    const off = listen<RecordWaveformsEvent>("record-editor-waveforms", (e) => {
      if (e.payload.sessionId !== sessionId.current) return;
      setView((v) => (v ? { ...v, waveforms: e.payload.waveforms } : v));
    });
    return () => void off.then((u) => u());
  }, []);

  /** Positions are integral ms — they go to `u64` IPCs. */
  const movePos = useCallback((p: number) => {
    const v = Math.max(0, Math.round(p));
    posRef.current = v;
    setPos(v);
  }, []);

  const applyPlayback = useCallback(
    (s: EditorPlayback) => {
      movePos(s.positionMs);
      setPlaying(s.playing);
      setCue({ source: s.source ?? null, speed: s.speed ?? 1 });
      if (s.helperError) setFatal(s.helperError);
    },
    [movePos],
  );

  /** Reloads the session from Rust (after a restart or a failed update). */
  const resync = useCallback(async () => {
    try {
      const i = await editorCurrent();
      // Another project was opened meanwhile: the remount shows it, never merge it in here.
      if (!i || i.sessionId !== sessionId.current) return;
      seq.current += 1;
      history.current?.replace(i.view.doc);
      setView(i.view);
      setFatal(i.helperError);
    } catch (e) {
      report(e);
    }
  }, [report]);

  // Playhead while playing (the app owns the clock in Rust; it also stops at the end).
  useEffect(() => {
    if (!playing) return;
    const id = window.setInterval(() => {
      editorPosition()
        .then((s) => {
          if (s) applyPlayback(s);
        })
        .catch(report);
    }, POSITION_POLL_MS);
    return () => window.clearInterval(id);
  }, [playing, applyPlayback, report]);

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
    const mine = ++seq.current;
    const next = chain.current.then(async () => {
      try {
        const v = await editorUpdate(sessionId.current, doc);
        if (h.present === doc) h.replace(v.doc);
        // A newer edit is on its way: its answer draws the view, this one would flash back.
        if (mine !== seq.current) return;
        // Mid gesture (mic or an inspector slider) the controls show the live document, not
        // the answer's, which may already be a step behind the finger.
        if (micStart.current) setView({ ...v, doc: { ...v.doc, micGainDb: h.present.micGainDb } });
        else if (liveRef.current?.active) setView({ ...v, doc: h.present });
        else setView(v);
        if (posRef.current > v.programmeMs) movePos(v.programmeMs);
      } catch (e) {
        report(e);
        // Rust may hold the doc even though the preview failed: show what it has.
        if (mine === seq.current) await resync();
      } finally {
        savedTimer.current = window.setTimeout(() => setSaving(false), SAVED_AFTER_MS);
      }
    });
    chain.current = next;
    return next;
  }, [movePos, report, resync]);

  const commit = useCallback((doc: RecordEditDoc) => void send(doc, true), [send]);

  useEffect(() => {
    sendRef.current = send;
  }, [send]);
  useEffect(() => () => liveRef.current?.cancel(), []);
  const inspectorLive = useCallback(
    (next: RecordEditDoc) => {
      gesture().move(next);
      setView((v) => (v ? { ...v, doc: next } : v));
    },
    [gesture],
  );
  const inspectorLiveEnd = useCallback(() => liveRef.current?.end(), []);

  const importAsset = useCallback(
    async (kind: "image" | "cursor") => {
      try {
        const picked = await openDialog({
          multiple: false,
          directory: false,
          filters:
            kind === "cursor"
              ? [{ name: t("rec3.insp.filter_png"), extensions: ["png"] }]
              : [{ name: t("rec3.insp.filter_images"), extensions: ["png", "jpg", "jpeg"] }],
        });
        if (typeof picked !== "string") return null;
        return (await editorImportAsset(sessionId.current, picked, kind)).file;
      } catch (e) {
        report(e);
        return null;
      }
    },
    [report, t],
  );

  const toggleInspector = useCallback(() => {
    setInspectorOpen((o) => {
      const next = !o;
      try {
        localStorage.setItem(INSPECTOR_KEY, next ? "1" : "0");
      } catch {
        /* per-viewer convenience only */
      }
      return next;
    });
  }, []);

  const undo = useCallback(() => {
    const d = history.current?.undo();
    if (d) void send(d, false);
  }, [send]);
  const redo = useCallback(() => {
    const d = history.current?.redo();
    if (d) void send(d, false);
  }, [send]);

  /** Moves the playhead; one `editorSeek` in flight, the latest target wins (drags). */
  const seek = useCallback(
    (p: number) => {
      movePos(Math.min(programmeMs, Math.max(0, p)));
      if (seeking.current) {
        pendingSeek.current = posRef.current;
        return;
      }
      seeking.current = true;
      const run = (target: number) => {
        editorSeek(target)
          .then((at) => setCue((c) => ({ ...c, source: at })))
          .catch(report)
          .finally(() => {
            const next = pendingSeek.current;
            pendingSeek.current = null;
            if (next != null && next !== target) run(next);
            else seeking.current = false;
          });
      };
      run(posRef.current);
    },
    [programmeMs, movePos, report],
  );

  const togglePlay = useCallback(() => {
    const want = !playing;
    setPlaying(want);
    editorPlay(want)
      .then(applyPlayback)
      .catch((e) => {
        setPlaying(false);
        report(e);
      });
  }, [playing, applyPlayback, report]);

  /** Source position of the playhead, without moving it. */
  const sourceHere = useCallback(
    () =>
      editorToSource(posRef.current).catch((e) => {
        report(e);
        return null;
      }),
    [report],
  );
  const cur = useCallback(() => history.current?.present, []);

  const split = useCallback(async () => {
    const at = await sourceHere();
    const doc = cur();
    if (!at || !doc) return;
    const next = splitAt(doc, at.clip, at.sourceMs);
    if (next !== doc) await send(next, true);
  }, [send, sourceHere, cur]);

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
    const at = await sourceHere();
    const doc = cur();
    if (!at || !doc) return;
    const end = Math.min(doc.clips[at.clip]?.endMs ?? at.sourceMs, at.sourceMs + NEW_SPEED_MS);
    if (end - at.sourceMs < 100) return;
    commit(addSpeed(doc, at.clip, at.sourceMs, end, 2));
  }, [commit, sourceHere, cur]);

  const addZoomHere = useCallback(async () => {
    const at = await sourceHere();
    const doc = cur();
    if (!at || !doc) return;
    const next = addZoom(doc, at.clip, at.sourceMs);
    const made = next.zooms[next.zooms.length - 1];
    if (next === doc || !made) return;
    commit(next);
    setSelectedZoom(made.id);
  }, [commit, sourceHere, cur, setSelectedZoom]);

  const addAnnotationHere = useCallback(
    async (kind: RecordAnnotationKind) => {
      const at = await sourceHere();
      const doc = cur();
      if (!at || !doc) return;
      const next = addAnnotation(doc, kind, at.clip, at.sourceMs);
      const made = next.annotations[next.annotations.length - 1];
      if (next === doc || !made) return;
      commit(next);
      setSelectedAnnotation(made.id);
    },
    [commit, sourceHere, cur, setSelectedAnnotation],
  );

  /** Auto-zoom: Rust plans and stores; the answer becomes one undo step here. */
  const autoZoom = useCallback(() => {
    const h = history.current;
    if (!h) return;
    const next = chain.current.then(async () => {
      const before = h.present.zooms.length;
      try {
        const v = await editorAutoZoom(sessionId.current);
        seq.current += 1;
        const added = v.doc.zooms.length - before;
        if (added > 0) h.push(v.doc);
        else h.replace(v.doc);
        setUndoable({ undo: h.canUndo(), redo: h.canRedo() });
        setView(v);
        setInfo(added > 0 ? tn("rec3.zoom.planned", added) : t("rec3.zoom.planned_none"));
        window.clearTimeout(infoTimer.current);
        infoTimer.current = window.setTimeout(() => setInfo(null), NOTICE_MS);
      } catch (e) {
        report(e);
      }
    });
    chain.current = next;
  }, [report, t]);

  /**
   * A clip operation Rust performs on its document (`editor_move_clip`, `…_duplicate_clip`,
   * `…_import_take`): queued behind pending edits like auto-zoom, the answer is one undo step.
   */
  const rustEdit = useCallback(
    (op: () => Promise<RecordEditorView>, after?: (v: RecordEditorView) => void) => {
      const h = history.current;
      if (!h) return;
      const next = chain.current.then(async () => {
        try {
          const v = await op();
          seq.current += 1;
          h.push(v.doc);
          setUndoable({ undo: h.canUndo(), redo: h.canRedo() });
          setView(v);
          after?.(v);
        } catch (e) {
          report(e);
        }
      });
      chain.current = next;
    },
    [report],
  );

  const moveClip = useCallback(
    (from: number, to: number) => rustEdit(() => editorMoveClip(sessionId.current, from, to), () => setSelected(to)),
    [rustEdit],
  );

  const duplicateClip = useCallback(() => {
    const index = clipFor(posRef.current);
    rustEdit(() => editorDuplicateClip(sessionId.current, index), () => setSelected(index + 1));
  }, [rustEdit, clipFor]);

  const [takePicker, setTakePicker] = useState(false);
  const importTake = useCallback(
    (projectDir: string) => {
      setTakePicker(false);
      rustEdit(
        () => editorImportTake(sessionId.current, projectDir),
        (v) => setSelected(v.doc.clips.length - 1),
      );
    },
    [rustEdit],
  );

  /** A music file is being imported (copied and decoded for its length). */
  const [importingMusic, setImportingMusic] = useState(false);
  const importingRef = useRef(false);
  /** M: picks a music file, copies it into the project and lays it at the playhead. */
  const addMusic = useCallback(async () => {
    if (voRef.current || importingRef.current) return;
    importingRef.current = true;
    try {
      const picked = await openDialog({
        multiple: false,
        directory: false,
        filters: [{ name: t("rec4.audio.filter_audio"), extensions: [...AUDIO_EXTENSIONS] }],
      });
      if (typeof picked !== "string") return;
      setImportingMusic(true);
      const imported = await editorImportAsset(sessionId.current, picked, "audio");
      const at = await sourceHere();
      const doc = cur();
      if (!at || !doc) return;
      const next = addTrack(doc, "music", imported.file, imported.durationMs ?? 0, at.clip, at.sourceMs);
      const made = next.audioTracks[next.audioTracks.length - 1];
      if (next === doc || !made) return;
      commit(updateTrack(next, made.id, { label: fileStem(picked) }));
      setSelectedTrack(made.id);
    } catch (e) {
      report(e);
    } finally {
      importingRef.current = false;
      setImportingMusic(false);
    }
  }, [commit, cur, report, setSelectedTrack, sourceHere, t]);

  /** R12: Rust turns a track edge drag into the file window. */
  const resolveTrackEdge = useCallback(
    (index: number, id: string, edge: "start" | "end", programmeMs: number) =>
      editorTrackEdge(sessionId.current, index, id, edge, programmeMs).catch((e) => {
        report(e);
        return null;
      }),
    [report],
  );

  /** Re-reads the transport (a take started or ended playback in Rust). */
  const syncPlayback = useCallback(() => {
    editorPosition()
      .then((s) => {
        if (s) applyPlayback(s);
        else setPlaying(false);
      })
      .catch(report);
  }, [applyPlayback, report]);

  /** Ends a take: Rust lays the recording under the programme; the answer is one undo step. */
  const finishVoiceover = useCallback(() => {
    const v = voRef.current;
    if (!v || v.phase !== "recording") return;
    setVoiceover({ ...v, phase: "finishing" });
    const h = history.current;
    const before = new Set(h?.present.audioTracks.map((x) => x.id) ?? []);
    const next = chain.current.then(async () => {
      try {
        const out = await editorVoiceoverStop(sessionId.current);
        seq.current += 1;
        if (h) {
          h.push(out.doc);
          setUndoable({ undo: h.canUndo(), redo: h.canRedo() });
        }
        setView(out);
        const made = out.doc.audioTracks.find((x) => !before.has(x.id));
        if (made) setSelectedTrack(made.id);
      } catch (e) {
        report(e);
      } finally {
        setVoiceover(null);
        syncPlayback();
      }
    });
    chain.current = next;
  }, [report, setSelectedTrack, setVoiceover, syncPlayback]);

  /** Esc: the take is discarded (its file removed), playback stops. */
  const cancelVoiceover = useCallback(() => {
    const v = voRef.current;
    if (!v || v.phase === "finishing") return;
    if (v.phase === "starting") {
      voCancelAsked.current = true;
      return;
    }
    setVoiceover(null);
    setPlaying(false);
    editorVoiceoverCancel(sessionId.current)
      .catch(report)
      .finally(syncPlayback);
  }, [report, setVoiceover, syncPlayback]);

  /** V: records a voiceover from the playhead (native microphone; Rust starts playback). */
  const startVoiceover = useCallback(async () => {
    if (voRef.current || fatal || cropMode || exportOpen) return;
    voCancelAsked.current = false;
    setVoiceover({ phase: "starting", rms: 0, elapsedMs: 0, maxMs: 0 });
    try {
      // Pending edits land first: the take is laid under the document Rust has.
      await chain.current;
      const st = await editorVoiceoverStart(sessionId.current, posRef.current);
      const v = voRef.current as VoiceoverView | null;
      setVoiceover({ phase: "recording", rms: v?.rms ?? 0, elapsedMs: v?.elapsedMs ?? 0, maxMs: st.maxMs });
      setPlaying(true);
      if (voCancelAsked.current) {
        voCancelAsked.current = false;
        cancelVoiceover();
      }
    } catch (e) {
      setVoiceover(null);
      report(e);
    }
  }, [fatal, cropMode, exportOpen, setVoiceover, report, cancelVoiceover]);

  const finishRef = useRef(finishVoiceover);
  useEffect(() => {
    finishRef.current = finishVoiceover;
  });
  // The take's level, and its end: Rust ends it at a pause, a seek or the programme's end.
  useEffect(() => {
    const offLevel = listen<EditorVoiceoverLevel>(EV_VOICEOVER_LEVEL, (e) => {
      const v = voRef.current;
      if (!v) return;
      if (v.phase !== "finishing") setVoiceover({ ...v, rms: e.payload.rms, elapsedMs: e.payload.elapsedMs });
      if (e.payload.ended) finishRef.current();
    });
    const offDone = listen<{ sessionId: number }>(EV_VOICEOVER_FINISHED, (e) => {
      if (e.payload.sessionId === sessionId.current) finishRef.current();
    });
    return () => {
      void offLevel.then((u) => u());
      void offDone.then((u) => u());
    };
  }, [setVoiceover]);

  const enterCrop = useCallback(async () => {
    const d = history.current?.present;
    // A clip whose recording is missing has no frame to crop.
    if (view && d && clipMedia(d, view.takes, clipFor(posRef.current), { width: 0, height: 0, systemAudio: false, micOffsetMs: null }).missing) {
      report("record.editor.take_missing");
      return;
    }
    if (playing) togglePlay();
    const at = await sourceHere();
    setCropSourceMs(at?.sourceMs ?? 0);
    setCropMode(true);
  }, [playing, togglePlay, sourceHere, view, clipFor, report]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // The timeline took the key (Z, Delete on a selected zoom).
      if (e.defaultPrevented || exportOpen || isTypingTarget(e.target) || !view) return;
      // A take runs: Esc discards it, V or Space ends it, nothing else edits meanwhile.
      if (voRef.current) {
        if (e.repeat || e.metaKey || e.ctrlKey || e.altKey) return;
        if (e.key === "Escape") {
          e.preventDefault();
          cancelVoiceover();
        } else if (e.key.toLowerCase() === "v" || e.key === " ") {
          e.preventDefault();
          finishVoiceover();
        }
        return;
      }
      const arrow = e.key === "ArrowLeft" || e.key === "ArrowRight";
      // Held keys repeat only for stepping; a focused slider keeps its own arrows.
      if (e.repeat && !arrow) return;
      const target = e.target instanceof HTMLElement ? e.target : null;
      if (arrow && target?.tagName === "INPUT") return;
      const mod = e.metaKey || e.ctrlKey;
      const k = e.key.toLowerCase();
      if (mod && k === "z") {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
        return;
      }
      if (mod && k === "d" && !e.shiftKey && !cropMode) {
        e.preventDefault();
        duplicateClip();
        return;
      }
      if (mod || e.altKey) return;
      const frameMs = 1000 / Math.max(1, init?.meta.fps ?? 30);
      if (e.key === "Escape" && cropMode) {
        e.preventDefault();
        setCropMode(false);
      } else if (e.key === " ") {
        // A focused button activates on Space by itself; crop mode has no playback.
        if (target?.tagName === "BUTTON" || cropMode) return;
        e.preventDefault();
        togglePlay();
      } else if (arrow) {
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
      } else if (k === "m" && !cropMode) {
        e.preventDefault();
        void addMusic();
      } else if (k === "v" && !cropMode) {
        e.preventDefault();
        void startVoiceover();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    view,
    init,
    exportOpen,
    cropMode,
    togglePlay,
    seek,
    split,
    removeClip,
    enterCrop,
    undo,
    redo,
    duplicateClip,
    addMusic,
    startVoiceover,
    finishVoiceover,
    cancelVoiceover,
  ]);

  // The preview plays the music/voiceover files from Rust's programme entries.
  const projectDir = init?.project.dir ?? "";
  const audioTracksDoc = view?.doc.audioTracks;
  const previewTracks = useMemo(
    () =>
      (audioTracksDoc ?? []).flatMap((tr, index) =>
        isPlainAssetName(tr.file) ? [{ index, src: convertFileSrc(`${projectDir}/assets/${tr.file}`) }] : [],
      ),
    [audioTracksDoc, projectDir],
  );

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
    // The document and timeline come back from Rust, whatever happened before the crash.
    await resync();
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
  // The box shape is Rust's output size (padding/frame change it under Auto), never a TS copy.
  const ratio = previewRatio(view, meta.width, meta.height);
  const cropClip = clipFor(pos);
  const preset: CropPreset = doc.output.aspect === "source" || doc.output.aspect === "auto" ? "free" : doc.output.aspect;
  const asset = (f: string) => convertFileSrc(`${dir}/${f}`);
  const zoomSel = selectedZoom ? (doc.zooms.find((z) => z.id === selectedZoom) ?? null) : null;
  const zoomRect = zoomSel ? (view.screenRects?.[zoomSel.clip] ?? ([0, 0, 1, 1] as [number, number, number, number])) : null;
  const annSel = selectedAnnotation ? (doc.annotations.find((a) => a.id === selectedAnnotation) ?? null) : null;
  const annPatch = (p: AnnotationPatch) => {
    if (!annSel) return null;
    const { points, ...box } = p;
    const base = cur() ?? doc;
    return updateAnnotation(base, annSel.id, points ? { ...box, body: { points } } : box);
  };
  const hasCursorData = !!assets?.hasCursorData;
  const mainMedia = { width: meta.width, height: meta.height, systemAudio: meta.systemAudio, micOffsetMs: meta.mic?.offsetMs ?? null };
  const fileOf = (m: ClipMedia, f: string) => asset(m.dir ? `${m.dir}/${f}` : f);
  // The audio of the clip under the playhead: the project's own take or an inserted one.
  const playMedia = clipMedia(doc, view.takes, cue.source?.clip ?? 0, mainMedia);
  const audio = {
    screen: playMedia.systemAudio ? fileOf(playMedia, "screen.mp4") : null,
    mic: playMedia.hasMic ? fileOf(playMedia, "mic.wav") : null,
    micOffsetMs: playMedia.micOffsetMs,
  };
  // Crop works on the cropped clip's own recording and size (presets included).
  const cropMedia = clipMedia(doc, view.takes, cropClip, mainMedia);

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
        {importingMusic && (
          <span className="min-w-0 truncate text-[11px] text-[var(--color-muted)]" role="status" data-testid="ed-music-busy">
            {t("rec4.audio.importing")}
          </span>
        )}
        {info && (
          <span className="min-w-0 truncate text-[11px] text-[var(--color-muted)]" role="status" data-testid="ed-info">
            {info}
          </span>
        )}
        {notice && (
          <span className="min-w-0 truncate text-[11px] text-red-500" role="alert" data-testid="ed-notice" title={notice}>
            {errText(t, notice)}
          </span>
        )}
        <div className="flex-1" />
        <button className={iconBtn} onClick={undo} disabled={!undoable.undo || !!voiceover} title={t("rec2.ed.undo")} aria-label={t("rec2.ed.undo")}>
          <Undo2 size={14} />
        </button>
        <button className={iconBtn} onClick={redo} disabled={!undoable.redo || !!voiceover} title={t("rec2.ed.redo")} aria-label={t("rec2.ed.redo")}>
          <Redo2 size={14} />
        </button>
        <button
          onClick={() => {
            if (playing) togglePlay();
            setExportOpen(true);
          }}
          disabled={!!voiceover}
          className="flex items-center gap-1.5 rounded-lg bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)] disabled:opacity-40"
        >
          <Download size={14} /> {t("rec2.ed.export")}
        </button>
      </header>

      {/* Preview + settings column */}
      <div className="flex min-h-0 flex-1">
      <Preview
        ratio={ratio}
        active={!fatal && !cropMode}
        dim={!!fatal}
        overlay={
          voiceover ? null : annSel && !cropMode && !fatal ? (
            <AnnotationOverlay
              annotation={annSel}
              screenRect={view.screenRects?.[annSel.clip] ?? null}
              zoomActive={zoomActiveAt(view, annSel.clip, pos)}
              visible={annotationShownAt(view, annSel.id, pos)}
              onLive={(p) => {
                const next = annPatch(p);
                if (next) inspectorLive(next);
              }}
              onEnd={inspectorLiveEnd}
              onCommit={(p) => {
                const next = annPatch(p);
                if (next) commit(next);
              }}
            />
          ) : zoomSel && zoomRect && zoomSel.focusMode === "manual" && !cropMode && !fatal ? (
            <FocusOverlay
              rect={zoomRect}
              focus={{ x: zoomSel.focusX, y: zoomSel.focusY }}
              onPick={(x, y) => commit(updateZoom(cur() ?? doc, zoomSel.id, { focusX: x, focusY: y, origin: "manual" }))}
            />
          ) : null
        }
      >
        {cropMode && !fatal && (
          <CropOverlay
            srcW={cropMedia.width}
            srcH={cropMedia.height}
            videoSrc={fileOf(cropMedia, "screen.mp4")}
            sourceMs={cropSourceMs}
            rect={doc.crop[cropClip] ?? null}
            preset={preset}
            onPreset={(p) => {
              const aspect = p === "free" ? "source" : p;
              const rect = p === "free" ? ((cur() ?? doc).crop[cropClip] ?? null) : cropForPreset(p, cropMedia.width, cropMedia.height);
              commit({ ...setCrop(cur() ?? doc, cropClip, rect), output: { aspect } });
            }}
            onCommit={(r) => commit(setCrop(cur() ?? doc, cropClip, r))}
            onReset={() => commit({ ...setCrop(cur() ?? doc, cropClip, null), output: { aspect: "source" } })}
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
      <aside
        className={
          "flex min-h-0 shrink-0 flex-col border-l border-[var(--color-border)] bg-[var(--color-surface)] " +
          (inspectorOpen ? "w-72" : "w-10")
        }
      >
        <div className={"flex items-center px-2 pt-2 " + (inspectorOpen ? "justify-between" : "justify-center")}>
          {inspectorOpen && <span className="pl-1 text-[12px] font-medium">{t("rec3.insp.title")}</span>}
          <button
            className="rounded-md p-1.5 text-[var(--color-muted)] hover:bg-[var(--color-bg)]"
            onClick={toggleInspector}
            aria-label={inspectorOpen ? t("rec3.insp.hide") : t("rec3.insp.show")}
            title={inspectorOpen ? t("rec3.insp.hide") : t("rec3.insp.show")}
            aria-expanded={inspectorOpen}
            data-testid="ed-inspector-toggle"
          >
            {inspectorOpen ? <PanelRightClose size={15} /> : <PanelRightOpen size={15} />}
          </button>
        </div>
        {inspectorOpen && (
          <Inspector
            doc={doc}
            assets={assets}
            assetUrl={shippedUrl}
            projectAssetUrl={(f) => asset(`assets/${f}`)}
            onCommit={commit}
            onLive={inspectorLive}
            onLiveEnd={inspectorLiveEnd}
            onImport={importAsset}
            selectedZoom={zoomSel?.id ?? null}
            onSelectZoom={setSelectedZoom}
            onAutoZoom={autoZoom}
            selectedAnnotation={annSel?.id ?? null}
            onSelectAnnotation={setSelectedAnnotation}
            onAddAnnotation={(k) => void addAnnotationHere(k)}
            getDoc={() => cur() ?? doc}
            hasMic={!!meta.mic}
            selectedTrack={selectedTrack}
            onSelectTrack={setSelectedTrack}
            onAddMusic={() => void addMusic()}
            musicBusy={importingMusic}
            onVoiceover={() => void startVoiceover()}
            voiceover={voiceover}
            onVoiceoverStop={finishVoiceover}
            onVoiceoverCancel={cancelVoiceover}
          />
        )}
      </aside>
      </div>

      {/* Transport + tools */}
      <div className="flex items-center gap-2 border-t border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-2">
        <button
          className={iconBtn}
          onClick={togglePlay}
          disabled={!!fatal || !!voiceover}
          aria-label={playing ? t("rec2.ed.pause") : t("rec2.ed.play")}
          title={`${playing ? t("rec2.ed.pause") : t("rec2.ed.play")} (Space)`}
        >
          {playing ? <Pause size={14} fill="currentColor" /> : <Play size={14} fill="currentColor" />}
        </button>
        <span className="text-[12px] tabular-nums" data-testid="ed-time">
          {formatTimecode(pos)} <span className="text-[var(--color-muted)]">/ {formatTimecode(programmeMs)}</span>
        </span>
        <div className="mx-2 h-5 w-px bg-[var(--color-border)]" />
        {voiceover && <VoiceoverPanel take={voiceover} onStop={finishVoiceover} onCancel={cancelVoiceover} compact />}
        <div className={"flex min-w-0 flex-1 items-center gap-2 " + (voiceover ? "hidden" : "")} inert={!!voiceover}>
        <button className={iconBtn} onClick={() => void split()} title={`${t("rec2.ed.split")} (S)`}>
          <Scissors size={14} /> {t("rec2.ed.split")}
        </button>
        <button className={iconBtn} onClick={removeClip} disabled={doc.clips.length <= 1} title={`${t("rec2.ed.delete")} (⌫)`}>
          <Trash2 size={14} /> {t("rec2.ed.delete")}
        </button>
        <button className={iconBtn} onClick={duplicateClip} title={`${t("rec4.clip.duplicate")} (⌘D)`} data-testid="ed-duplicate">
          <Copy size={14} /> {t("rec4.clip.duplicate")}
        </button>
        <div className="relative">
          <button
            className={iconBtn + (takePicker ? " border-[var(--color-accent)] text-[var(--color-accent)]" : "")}
            onClick={() => setTakePicker((o) => !o)}
            title={t("rec4.take.insert_hint")}
            aria-expanded={takePicker}
            data-testid="ed-insert-take"
          >
            <FilePlus2 size={14} /> {t("rec4.take.insert")}
          </button>
          {takePicker && <TakePicker currentDir={dir} onPick={importTake} onClose={() => setTakePicker(false)} onError={report} />}
        </div>
        <button className={iconBtn} onClick={() => void addSpeedHere()} title={t("rec2.ed.speed_add_hint")}>
          <Gauge size={14} /> {t("rec2.ed.speed_add")}
        </button>
        <button className={iconBtn} onClick={() => void addZoomHere()} title={t("rec3.zoom.add_hint")}>
          <ZoomIn size={14} /> {t("rec3.zoom.add")}
        </button>
        <button className={iconBtn} onClick={() => void addAnnotationHere("text")} title={t("rec4.ann.toolbar_hint")} data-testid="ed-add-annotation">
          <Type size={14} /> {t("rec4.ann.toolbar")}
        </button>
        <button
          className={iconBtn}
          onClick={autoZoom}
          disabled={!hasCursorData}
          title={hasCursorData ? t("rec3.zoom.auto_hint") : t("rec3.insp.no_cursor")}
          data-testid="ed-auto-zoom"
        >
          <WandSparkles size={14} /> {t("rec3.zoom.auto")}
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
              step={GAIN_STEP_DB}
              value={doc.micGainDb}
              onChange={(e) => micLive(snap(Number(e.target.value), GAIN_STEP_DB, MIN_MIC_DB, MAX_MIC_DB))}
              onPointerUp={micDone}
              onKeyUp={micDone}
              onBlur={micDone}
              className="w-28"
              aria-label={t("rec2.ed.mic")}
            />
            <span className="w-14 tabular-nums">{doc.micGainDb.toFixed(1)} dB</span>
          </label>
        )}
        </div>
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
          onError={report}
          getDoc={() => cur() ?? doc}
          onSpeedValue={(i, v) => commit(setSpeedValue(cur() ?? doc, i, v))}
          onSpeedRemove={(i) => commit(removeSpeed(cur() ?? doc, i))}
          onWidth={setTrackWidth}
          selectedZoom={zoomSel?.id ?? null}
          onSelectZoom={setSelectedZoom}
          keysEnabled={!exportOpen && !cropMode}
          onMoveClip={moveClip}
          selectedAnnotation={annSel?.id ?? null}
          onSelectAnnotation={setSelectedAnnotation}
          selectedTrack={selectedTrack}
          onSelectTrack={setSelectedTrack}
          resolveTrackEdge={resolveTrackEdge}
          locked={!!voiceover}
        />
      </div>

      <AudioPreview
        screenSrc={audio.screen}
        micSrc={audio.mic}
        micOffsetMs={audio.micOffsetMs}
        micGainDb={doc.micGainDb}
        playing={playing && !fatal}
        source={cue.source}
        speed={cue.speed}
        positionMs={pos}
        systemGainDb={doc.systemGainDb}
        tracks={previewTracks}
        spans={view.tracks ?? []}
        duckDb={voiceover ? VOICEOVER_DUCK_DB : 0}
        clicks={{
          hits: view.clickHits ?? [],
          downSrc: assets?.clickDown ? shippedUrl(assets.clickDown) : null,
          upSrc: assets?.clickUp ? shippedUrl(assets.clickUp) : null,
          gainDb: doc.cursor.clickSoundGainDb,
          on: hasCursorData && doc.cursor.show && doc.cursor.clickSound,
        }}
      />
      {exportOpen && <ExportDialog onClose={() => setExportOpen(false)} />}
    </div>
  );
}
