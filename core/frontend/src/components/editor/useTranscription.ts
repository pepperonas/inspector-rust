import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  EV_RECORD_TRANSCRIBE,
  editorTranscribe,
  editorTranscribeCancel,
  getMeetingConfig,
  meetingModelCancel,
  meetingModelDownload,
  meetingModelStatus,
  type MeetingModel,
  type MeetingState,
  type RecordTranscribeProgress,
  type RecordTranscribeResult,
} from "../../lib/ipc";

export type TranscribeLang = "auto" | "de" | "en";

/** What the captions and transcript tabs need of a transcription run. */
export interface Transcription {
  running: boolean;
  /** The newest progress event of this session, while running. */
  progress: RecordTranscribeProgress | null;
  /** The last finished run: what was skipped, a partial error, a cancel. */
  last: Pick<RecordTranscribeResult, "skipped" | "error" | "cancelled"> | null;
  /** An error that stopped the run before anything finished (a sentinel or text). */
  error: string | null;
  /** The model chosen in the meeting settings; `null` while unknown. */
  model: MeetingModel | null;
  /** Download in progress (bytes). */
  download: { done: number; total: number } | null;
  downloadError: string | null;
  start: (lang: TranscribeLang) => void;
  cancel: () => void;
  downloadModel: () => void;
  cancelDownload: () => void;
}

/** True when the model is known and not on disk (the tabs offer the download). */
export const modelMissing = (tr: Pick<Transcription, "model">) => !!tr.model && !tr.model.installed;

/**
 * One transcription run of the editor (Rust `editor_transcribe`), its progress
 * (`record-editor-transcribe`) and the speech model's status. `onResult` lays the finished
 * transcripts into the newest document; it runs at most once per run and never after unmount.
 */
export function useTranscription(
  sessionId: () => number,
  onResult: (r: RecordTranscribeResult, lang: TranscribeLang) => void,
): Transcription {
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<RecordTranscribeProgress | null>(null);
  const [last, setLast] = useState<Transcription["last"]>(null);
  const [error, setError] = useState<string | null>(null);
  const [model, setModel] = useState<MeetingModel | null>(null);
  const [download, setDownload] = useState<Transcription["download"]>(null);
  const [downloadError, setDownloadError] = useState<string | null>(null);
  const alive = useRef(true);
  const runningRef = useRef(false);
  const resultRef = useRef(onResult);
  useEffect(() => {
    resultRef.current = onResult;
  });

  const refreshModel = useCallback(() => {
    Promise.resolve()
      .then(() => Promise.all([getMeetingConfig(), meetingModelStatus()]))
      .then(([cfg, models]) => {
        if (alive.current) setModel(models.find((m) => m.id === cfg.model) ?? null);
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    alive.current = true;
    refreshModel();
    const offProgress = listen<RecordTranscribeProgress>(EV_RECORD_TRANSCRIBE, (e) => {
      if (e.payload.sessionId !== sessionId() || !runningRef.current) return;
      setProgress(e.payload);
    });
    const offMeeting = listen<MeetingState>("meeting-changed", (e) => {
      const d = e.payload.download;
      setDownload(d && !d.error ? { done: d.done, total: d.total } : null);
      if (d?.error) setDownloadError(d.error);
      if (!d) refreshModel();
    });
    return () => {
      alive.current = false;
      void offProgress.then((u) => u());
      void offMeeting.then((u) => u());
    };
  }, [refreshModel, sessionId]);

  const start = useCallback(
    (lang: TranscribeLang) => {
      if (runningRef.current) return;
      runningRef.current = true;
      setRunning(true);
      setError(null);
      setLast(null);
      setProgress(null);
      editorTranscribe(sessionId(), lang)
        .then((r) => {
          if (!alive.current) return;
          resultRef.current(r, lang);
          setLast({ skipped: r.skipped, error: r.error, cancelled: r.cancelled });
        })
        .catch((e) => {
          if (!alive.current) return;
          const s = String(e);
          if (s === "model.missing") refreshModel();
          // A cancel before anything finished is no error.
          if (s !== "transcribe.file_cancelled") setError(s);
        })
        .finally(() => {
          runningRef.current = false;
          if (!alive.current) return;
          setRunning(false);
          setProgress(null);
        });
    },
    [refreshModel, sessionId],
  );

  const cancel = useCallback(() => {
    void editorTranscribeCancel().catch(() => undefined);
  }, []);

  const downloadModel = useCallback(() => {
    if (!model) return;
    setDownloadError(null);
    setDownload({ done: 0, total: model.bytes });
    meetingModelDownload(model.id)
      .catch((e) => alive.current && setDownloadError(String(e)))
      .finally(() => {
        if (!alive.current) return;
        setDownload(null);
        refreshModel();
      });
  }, [model, refreshModel]);

  const cancelDownload = useCallback(() => {
    void meetingModelCancel().catch(() => undefined);
  }, []);

  return { running, progress, last, error, model, download, downloadError, start, cancel, downloadModel, cancelDownload };
}
