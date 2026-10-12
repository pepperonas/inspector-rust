import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Circle, Play, Trash2, X } from "lucide-react";
import {
  gestureRecordStart,
  gestureTraceDelete,
  gestureTraceDeleteAll,
  gestureTraceList,
  gestureTraceReplay,
  listActionHotkeys,
  type GestureRecordStatus,
  type GestureReplayRow,
  type GestureTraceFile,
} from "../lib/ipc";
import { formatReplayRow, RECORD_SECS } from "../lib/gesture-trace";
import { traceKindLabel, traceSize, traceWhen } from "../lib/gesture-calibrate";
import { formatHotkey } from "../lib/platform";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { t, useT } from "../lib/i18n";

/** The action id of the "that was unintended" hotkey (`hotkey::ActionId`). */
const UNINTENDED_ACTION = "gestureunintended";

/**
 * Recordings of the `gestures` panel (`gestures record`): start a 30-s
 * recording, list what is saved — including the misfire reports of the
 * "unintended" hotkey — replay one against the current thresholds, delete
 * one or all. Everything stays in the app data folder; nothing is sent.
 */
export function GestureRecordings({
  status,
  supported,
  autoStart,
}: {
  status: GestureRecordStatus | undefined;
  supported: boolean;
  /** Start once on mount (`gestures record` + Enter). */
  autoStart: boolean;
}) {
  useT();
  const [traces, setTraces] = useState<GestureTraceFile[]>([]);
  const [replay, setReplay] = useState<{ name: string; rows: GestureReplayRow[] } | null>(null);
  const [armed, setArmed] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hotkey, setHotkey] = useState<string | null>(null);
  const autoStarted = useRef(false);

  const refresh = useCallback(() => {
    gestureTraceList()
      .then(setTraces)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refresh();
    listActionHotkeys()
      .then((list) => {
        const spec = list.find((a) => a.id === UNINTENDED_ACTION)?.shortcut ?? "";
        setHotkey(spec ? formatHotkey(spec) : "");
      })
      .catch(() => setHotkey(null));
  }, [refresh]);

  useTauriEvent("gesture-trace-saved", () => refresh(), [refresh]);

  const start = useCallback(async () => {
    setError(null);
    try {
      await gestureRecordStart(RECORD_SECS);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (!autoStart || autoStarted.current || !supported) return;
    autoStarted.current = true;
    void start();
  }, [autoStart, supported, start]);

  const remove = async (name: string | "all") => {
    setArmed(null);
    try {
      if (name === "all") await gestureTraceDeleteAll();
      else await gestureTraceDelete(name);
      if (replay && (name === "all" || replay.name === name)) setReplay(null);
    } catch (e) {
      setError(String(e));
    }
    refresh();
  };

  const play = async (name: string) => {
    if (replay?.name === name) {
      setReplay(null);
      return;
    }
    setError(null);
    try {
      setReplay({ name, rows: await gestureTraceReplay(name) });
    } catch (e) {
      setError(String(e));
    }
  };

  const recording = status?.recording ?? false;
  // The saved-event can be missed (listener not yet attached); the end of a
  // recording refreshes the list as well.
  const wasRecording = useRef(recording);
  useEffect(() => {
    if (wasRecording.current && !recording) refresh();
    wasRecording.current = recording;
  }, [recording, refresh]);

  return (
    <section className="flex flex-col gap-2 rounded-lg border border-[var(--color-border)] p-3" aria-label={t("grec.title")}>
      <div className="flex items-center gap-2">
        <span className="font-medium">{t("grec.title")}</span>
        {traces.length > 1 &&
          (armed === "all" ? (
            <ConfirmPair label={t("grec.delete_all")} onYes={() => void remove("all")} onNo={() => setArmed(null)} />
          ) : (
            <button
              type="button"
              onClick={() => setArmed("all")}
              className="ml-auto rounded px-2 py-0.5 text-[var(--color-muted)] hover:bg-[var(--color-border)]/40"
            >
              {t("grec.delete_all_2")}
            </button>
          ))}
      </div>

      {supported ? (
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={() => void start()}
            disabled={recording}
            className="md3-press flex items-center gap-1.5 rounded-md border border-[var(--color-border)] px-3 py-1 hover:bg-[var(--color-border)]/40 disabled:opacity-80"
          >
            <Circle size={10} className={recording ? "fill-rose-500 text-rose-500" : "text-rose-400"} />
            {recording ? (
              <span className="tabular-nums">
                {t("grec.recording_live", { s: Math.ceil((status?.remaining_ms ?? 0) / 1000), n: status?.frames ?? 0 })}
              </span>
            ) : (
              t("grec.record_n", { n: RECORD_SECS })
            )}
          </button>
          <span className="text-[11px] text-[var(--color-muted)]">
            {t("grec.only_touches_and_the_times")}
          </span>
        </div>
      ) : (
        <p className="text-[var(--color-muted)]">{t("grec.recording_needs_the_individual_contacts")}</p>
      )}

      {supported && hotkey !== null && (
        <p className="text-[11px] text-[var(--color-muted)]">
          {hotkey
            ? t("grec.unintended_hint", { hotkey })
            : t("grec.the_gesture_was_unintended_shortcut")}
        </p>
      )}

      {error && <p className="text-amber-400">{error}</p>}

      {traces.length === 0 ? (
        <p className="text-[var(--color-muted)]">{t("grec.no_recording_yet")}</p>
      ) : (
        <ul className="flex flex-col gap-0.5">
          {traces.map((tr) => (
            <li key={tr.name} className="flex flex-col">
              <div className="flex items-center gap-2">
                <span
                  className={`rounded-full px-1.5 text-[10px] ${
                    tr.unintended ? "bg-amber-500/15 text-amber-400" : "bg-[var(--color-border)]/60 text-[var(--color-muted)]"
                  }`}
                >
                  {traceKindLabel(tr)}
                </span>
                <span className="tabular-nums" title={tr.name}>
                  {traceWhen(tr.name)}
                </span>
                <span className="text-[var(--color-muted)]">{traceSize(tr.bytes)}</span>
                <span className="ml-auto flex items-center gap-1">
                  {armed === tr.name ? (
                    <ConfirmPair label={t("grec.delete")} onYes={() => void remove(tr.name)} onNo={() => setArmed(null)} />
                  ) : (
                    <>
                      <button
                        type="button"
                        onClick={() => void play(tr.name)}
                        aria-label={t("grec.play_item", { when: traceWhen(tr.name) })}
                        aria-pressed={replay?.name === tr.name}
                        className="rounded p-1 text-[var(--color-accent)] hover:bg-[var(--color-border)]/40"
                      >
                        <Play size={12} />
                      </button>
                      <button
                        type="button"
                        onClick={() => setArmed(tr.name)}
                        aria-label={t("grec.delete_item", { when: traceWhen(tr.name) })}
                        className="rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-border)]/40"
                      >
                        <Trash2 size={12} />
                      </button>
                    </>
                  )}
                </span>
              </div>
              {replay?.name === tr.name && (
                <div className="my-1 rounded-md bg-[var(--color-border)]/25 px-2 py-1">
                  {replay.rows.length === 0 ? (
                    <span className="text-[var(--color-muted)]">{t("grec.with_the_current_thresholds_no")}</span>
                  ) : (
                    <ul className="flex flex-col font-mono text-[11px]">
                      {replay.rows.map((r, i) => (
                        <li key={i}>{formatReplayRow(r)}</li>
                      ))}
                    </ul>
                  )}
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function ConfirmPair({ label, onYes, onNo }: { label: string; onYes: () => void; onNo: () => void }) {
  useT();
  return (
    <span className="ml-auto flex items-center gap-1 text-[11px]">
      <span className="text-rose-400">{label}</span>
      <button type="button" onClick={onYes} aria-label={t("grec.yes_delete")} className="rounded p-1 text-rose-400 hover:bg-rose-500/15">
        <Check size={12} />
      </button>
      <button type="button" onClick={onNo} aria-label={t("grec.no")} className="rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-border)]/40">
        <X size={12} />
      </button>
    </span>
  );
}
