import { useCallback, useEffect, useRef, useState } from "react";
import { AudioLines, Download, FolderOpen, Mic, Monitor, Square, Circle } from "lucide-react";
import { useTauriEvent } from "../hooks/useTauriEvent";
import {
  getMeetingConfig,
  meetingModelDownload,
  meetingModelCancel,
  meetingModelStatus,
  meetingSessions,
  meetingSetSource,
  meetingStart,
  meetingStatus,
  meetingStop,
  revealPath,
  setMeetingConfig,
  type MeetingConfig,
  type MeetingModel,
  type MeetingSession,
  type MeetingSource,
  type MeetingState,
} from "../lib/ipc";
import { useT } from "../lib/i18n";
import {
  clockTime,
  downloadPercent,
  errorText,
  formatElapsed,
  levelOf,
  modelSizeLabel,
  sessionTitle,
  sourceLabel,
} from "../lib/meeting";

/** Enter runs once per signal, even if the panel remounts. */
let lastHandledRun = 0;

const ACCENT: Record<MeetingSource, string> = { mic: "#38bdf8", sys: "#f59e0b" };

interface Props {
  focused: boolean;
  runSignal: number;
  onExit: () => void;
}

/**
 * `meeting` — live transcription of microphone + system audio. Shows while
 * typed; Enter starts or stops. The session runs in the background helper,
 * so closing the popup never stops it.
 */
export function MeetingPanel({ focused, runSignal, onExit }: Props) {
  const t = useT();
  const [state, setState] = useState<MeetingState | null>(null);
  const [cfg, setCfg] = useState<MeetingConfig | null>(null);
  const [models, setModels] = useState<MeetingModel[]>([]);
  const [sessions, setSessions] = useState<MeetingSession[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  const refresh = useCallback(() => {
    void meetingStatus().then(setState).catch(() => {});
    void meetingModelStatus().then(setModels).catch(() => {});
    void meetingSessions().then(setSessions).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
    void getMeetingConfig().then(setCfg).catch(() => {});
  }, [refresh]);

  useTauriEvent<MeetingState>("meeting-changed", (e) => {
    setState(e.payload);
    if (!e.payload.running && !e.payload.starting) {
      void meetingSessions().then(setSessions).catch(() => {});
    }
    if (e.payload.download === null) void meetingModelStatus().then(setModels).catch(() => {});
  });

  const running = !!state?.running;
  useEffect(() => {
    if (!running) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [running]);

  const model = models.find((m) => m.id === cfg?.model);
  const modelMissing = !!cfg && models.length > 0 && !model?.installed;

  const toggle = useCallback(async () => {
    setError(null);
    try {
      if (state?.running || state?.starting) await meetingStop();
      else setState(await meetingStart());
    } catch (e) {
      setError(String(e));
    }
  }, [state]);

  // Enter waits until the model status is known — before that it cannot
  // tell "start" from "offer the download".
  const ready = !!cfg && models.length > 0;
  useEffect(() => {
    if (!ready || runSignal <= lastHandledRun) return;
    lastHandledRun = runSignal;
    if (!modelMissing) void toggle();
  }, [runSignal, toggle, modelMissing, ready]);

  const toggleSource = async (s: MeetingSource) => {
    if (!cfg) return;
    const next = { ...cfg, [s]: !cfg[s] };
    setCfg(next);
    try {
      setCfg(await setMeetingConfig(next));
      await meetingSetSource(s, next[s]);
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return;
      if (e.key === "Escape") {
        e.preventDefault();
        onExit();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focused, onExit]);

  // The newest line stays in view (the box scrolls, the panel does not jump).
  const linesRef = useRef<HTMLDivElement>(null);
  const lineCount = state?.line_count ?? 0;
  useEffect(() => {
    const el = linesRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lineCount, state?.hypotheses]);

  const pct = state?.download ? downloadPercent(state.download.done, state.download.total) : null;
  const shownError = errorText(error ?? state?.error);
  const elapsed = running && state?.started_ms ? formatElapsed(now - state.started_ms) : null;

  return (
    <div className="h-full space-y-3 overflow-y-auto p-4 text-[13px]" data-testid="meeting-panel">
      <div className="flex items-center gap-2">
        <AudioLines size={18} className="text-[var(--color-accent)]" />
        <h2 className="text-[15px] font-semibold">{t("mtg.title")}</h2>
        {running && (
          <span className="anim-keep ml-1 inline-flex items-center gap-1 rounded bg-red-500/15 px-1.5 py-0.5 text-[11px] font-semibold text-red-500">
            <Circle size={8} fill="currentColor" /> REC {elapsed}
          </span>
        )}
        <button
          type="button"
          data-testid="meeting-toggle"
          disabled={modelMissing && !running}
          onClick={() => void toggle()}
          className={`ml-auto inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-[12px] font-semibold disabled:opacity-50 ${
            running || state?.starting ? "bg-red-600 text-white" : "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
          }`}
        >
          {running || state?.starting ? <Square size={12} /> : <Circle size={12} />}
          {state?.starting ? t("mtg.starting") : running ? t("mtg.stop") : t("mtg.start")}
        </button>
      </div>
      <p className="text-[12px] text-[var(--color-muted)]">{t("mtg.hint")}</p>

      {modelMissing && model && (
        <div className="rounded-lg border border-[var(--color-border)] p-3" data-testid="meeting-model-card">
          <div className="mb-1 font-semibold">{t("mtg.model_missing")}</div>
          <p className="mb-2 text-[12px] text-[var(--color-muted)]">
            {t("mtg.model_missing_body", { size: modelSizeLabel(model.bytes) })}
          </p>
          {state?.download && !state.download.error ? (
            <div className="flex items-center gap-2">
              <div className="h-1.5 flex-1 overflow-hidden rounded bg-[var(--color-border)]">
                <div className="h-full bg-[var(--color-accent)]" style={{ width: `${pct ?? 0}%` }} />
              </div>
              <span className="tabular-nums text-[11px]">{pct ?? 0} %</span>
              <button type="button" className="text-[11px] underline" onClick={() => void meetingModelCancel()}>
                {t("common.cancel")}
              </button>
            </div>
          ) : (
            <button
              type="button"
              data-testid="meeting-download"
              onClick={() => void meetingModelDownload(model.id).catch((e) => setError(String(e)))}
              className="inline-flex items-center gap-1.5 rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-semibold text-[var(--color-accent-fg)]"
            >
              <Download size={12} /> {t("mtg.download", { size: modelSizeLabel(model.bytes) })}
            </button>
          )}
          {state?.download?.error && <p className="mt-2 text-[12px] text-red-500">{errorText(state.download.error)}</p>}
        </div>
      )}

      <div className="grid grid-cols-2 gap-2">
        {(["mic", "sys"] as MeetingSource[]).map((s) => {
          const on = cfg ? cfg[s] : true;
          const src = state?.sources[s];
          const lost = src?.state === "lost";
          return (
            <button
              key={s}
              type="button"
              data-testid={`meeting-source-${s}`}
              onClick={() => void toggleSource(s)}
              className={`flex flex-col gap-1.5 rounded-lg border p-2 text-left ${
                on ? "border-[var(--color-border)]" : "border-dashed border-[var(--color-border)] opacity-60"
              }`}
            >
              <span className="flex items-center gap-1.5 text-[12px] font-semibold">
                {s === "mic" ? <Mic size={13} /> : <Monitor size={13} />}
                <span style={{ color: ACCENT[s] }}>{sourceLabel(s)}</span>
                <span className="font-normal text-[var(--color-muted)]">
                  {t(s === "mic" ? "mtg.src_mic" : "mtg.src_sys")}
                </span>
                <span className="ml-auto text-[11px] font-normal text-[var(--color-muted)]">
                  {!on ? t("common.off") : lost ? t("mtg.lost") : running ? "" : t("common.on")}
                </span>
              </span>
              <span className="h-1 overflow-hidden rounded bg-[var(--color-border)]">
                <span
                  className="block h-full transition-[width] duration-(--duration-fast)"
                  style={{ width: `${Math.round(levelOf(src?.level ?? 0) * 100)}%`, background: ACCENT[s] }}
                />
              </span>
              {lost && src?.detail && <span className="text-[11px] text-red-500">{src.detail}</span>}
            </button>
          );
        })}
      </div>

      {shownError && <p className="rounded bg-red-500/10 p-2 text-[12px] text-red-500" data-testid="meeting-error">{shownError}</p>}
      {state?.warnings.map((w) => (
        <p key={w} className="rounded bg-amber-500/10 p-2 text-[12px] text-amber-600">{w}</p>
      ))}

      {(running || (state?.lines.length ?? 0) > 0) && (
        <div ref={linesRef} className="flex max-h-[22rem] min-h-[120px] flex-col gap-1 overflow-y-auto rounded-lg border border-[var(--color-border)] p-2" data-testid="meeting-lines">
          {state?.lines.slice(-30).map((l) => (
            <div key={`${l.source}-${l.seq}`} className="flex gap-2">
              <span className="shrink-0 tabular-nums text-[11px] text-[var(--color-muted)]">{clockTime(l.start_ms)}</span>
              <span className="shrink-0 text-[11px] font-semibold" style={{ color: ACCENT[l.source] }}>{sourceLabel(l.source)}</span>
              <span>{l.text}</span>
            </div>
          ))}
          {(["mic", "sys"] as MeetingSource[]).map((s) => {
            const h = state?.hypotheses[s];
            if (!h) return null;
            return (
              <div key={`h-${s}`} className="flex gap-2 italic text-[var(--color-muted)]">
                <span className="shrink-0 text-[11px] font-semibold not-italic" style={{ color: ACCENT[s] }}>{sourceLabel(s)}</span>
                <span>
                  {h.stable} <span className="opacity-70">{h.pending}</span>
                </span>
              </div>
            );
          })}
          {running && state?.lines.length === 0 && !state.hypotheses.mic && !state.hypotheses.sys && (
            <span className="text-[12px] text-[var(--color-muted)]">{t("mtg.listening")}</span>
          )}
        </div>
      )}

      {state?.dir && (
        <button type="button" onClick={() => void revealPath(state.dir!)} className="inline-flex items-center gap-1.5 text-[12px] underline">
          <FolderOpen size={12} /> {t("mtg.show_folder")}
        </button>
      )}

      {sessions.length > 0 && (
        <div className="flex flex-col gap-1" data-testid="meeting-sessions">
          <div className="text-[11px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">{t("mtg.sessions")}</div>
          {sessions.slice(0, 8).map((s) => (
            <button
              key={s.dir}
              type="button"
              onClick={() => void revealPath(s.dir)}
              className="flex items-center gap-2 rounded px-1 py-0.5 text-left hover:bg-[var(--color-surface)]"
            >
              <span>{sessionTitle(s.meta)}</span>
              <span className="text-[11px] text-[var(--color-muted)]">{t("mtg.lines", { n: s.lines })}</span>
              {s.meta.status === "aborted" && (
                <span className="rounded bg-amber-500/15 px-1 text-[10px] font-semibold text-amber-600">{t("mtg.aborted")}</span>
              )}
              {s.meta.status === "running" && !running && (
                <span className="rounded bg-amber-500/15 px-1 text-[10px] font-semibold text-amber-600">{t("mtg.aborted")}</span>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
