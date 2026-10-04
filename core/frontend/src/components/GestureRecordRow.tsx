import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import {
  gestureRecordStart,
  gestureRecordStatus,
  gestureTraceList,
  gestureTraceReplay,
  type GestureRecordStatus,
  type GestureReplayRow,
  type GestureTraceFile,
} from "../lib/ipc";
import { formatReplayRow, RECORD_SECS } from "../lib/gesture-trace";

/**
 * Record the raw trackpad frames for a while and replay a saved recording
 * against the current settings — the evidence for "that gesture fired by
 * itself". Recordings hold touch data and key-down TIMES only, never which key.
 */
export function GestureRecordRow() {
  const [status, setStatus] = useState<GestureRecordStatus | null>(null);
  const [traces, setTraces] = useState<GestureTraceFile[]>([]);
  const [replay, setReplay] = useState<{ name: string; rows: GestureReplayRow[] } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refreshList = useCallback(() => {
    gestureTraceList().then(setTraces).catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refreshList();
    let unlisten: (() => void) | undefined;
    listen("gesture-trace-saved", () => refreshList())
      .then((u) => (unlisten = u))
      .catch(() => {});
    return () => unlisten?.();
  }, [refreshList]);

  // Poll the countdown only while a recording runs.
  const recording = status?.recording ?? false;
  useEffect(() => {
    if (!recording) return;
    const id = window.setInterval(() => {
      gestureRecordStatus()
        .then((s) => {
          setStatus(s);
          if (!s.recording) refreshList();
        })
        .catch(() => {});
    }, 500);
    return () => window.clearInterval(id);
  }, [recording, refreshList]);

  const start = async () => {
    setError(null);
    try {
      await gestureRecordStart(RECORD_SECS);
      setStatus(await gestureRecordStatus());
    } catch (e) {
      setError(String(e));
    }
  };

  const play = async (name: string) => {
    setError(null);
    try {
      setReplay({ name, rows: await gestureTraceReplay(name) });
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="flex flex-col gap-2 text-[12px]">
      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={() => void start()}
          disabled={recording}
          className="md3-press rounded-md border border-[var(--color-border)] px-3 py-1 hover:bg-[var(--color-border)]/40 disabled:opacity-60"
        >
          {recording
            ? `Recording… ${Math.ceil((status?.remaining_ms ?? 0) / 1000)} s · ${status?.frames ?? 0} frames`
            : `Record ${RECORD_SECS} s`}
        </button>
        <span className="text-[var(--color-muted)]">
          Touch data and key-down times only — never which keys. Stays on this Mac.
        </span>
      </div>
      {error && <span className="text-amber-500">{error}</span>}
      {traces.length > 0 && (
        <ul className="flex flex-col gap-1">
          {traces.slice(0, 5).map((t) => (
            <li key={t.name} className="flex items-center gap-2">
              <span className="font-mono text-[11px]">{t.name}</span>
              <span className="text-[var(--color-muted)]">{Math.round(t.bytes / 1024)} KB</span>
              <button
                type="button"
                onClick={() => void play(t.name)}
                className="rounded px-2 py-0.5 text-[var(--color-accent)] hover:bg-[var(--color-border)]/40"
              >
                Replay
              </button>
            </li>
          ))}
        </ul>
      )}
      {replay && (
        <div className="rounded-md border border-[var(--color-border)] p-2">
          <div className="mb-1 font-mono text-[11px] text-[var(--color-muted)]">{replay.name}</div>
          {replay.rows.length === 0 ? (
            <div className="text-[var(--color-muted)]">No gesture recognised.</div>
          ) : (
            <ul className="flex flex-col gap-0.5 font-mono text-[11px]">
              {replay.rows.map((r, i) => (
                <li key={i}>{formatReplayRow(r)}</li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
