import { useState } from "react";
import { Download, FileAudio } from "lucide-react";
import { downloadPercent, errorText, modelSizeLabel } from "../../lib/meeting";
import { tn, useT, type MessageKey } from "../../lib/i18n";
import { Chip, Chips, chipClass } from "./InspectorParts";
import { modelMissing, type TranscribeLang, type Transcription } from "./useTranscription";

const LANGS: { id: TranscribeLang; label: string | MessageKey }[] = [
  { id: "de", label: "DE" },
  { id: "en", label: "EN" },
  { id: "auto", label: "rec6.tr.lang_auto" },
];

/** A transcription error as a sentence: the editor's own sentinels, then the helper's. */
export function transcribeError(t: (k: MessageKey) => string, e: string): string {
  if (e.startsWith("record.editor.")) {
    const s = t(e as MessageKey);
    if (s !== e) return s;
  }
  return errorText(e) ?? e;
}

/**
 * Start / progress / result of a transcription, shared by the captions and the transcript tab:
 * language chips DE/EN/Auto, the model download when it is missing, a cancel while running.
 * `compact`: a project that already has a transcript (one line: "transcribe again").
 */
export function TranscribeBox({
  tr,
  lang,
  compact,
}: {
  tr: Transcription;
  /** The language the last run used (the chips start there). */
  lang: TranscribeLang;
  compact?: boolean;
}) {
  const t = useT();
  const [pick, setPick] = useState<TranscribeLang>(lang);
  const missing = modelMissing(tr) || tr.error === "model.missing";

  if (missing && tr.model) {
    const pct = tr.download ? downloadPercent(tr.download.done, tr.download.total) : null;
    return (
      <div className="space-y-2 rounded-md border border-[var(--color-border)] p-2.5 text-[12px]" data-testid="ed-tr-model">
        <div className="font-medium">{t("mtg.model_missing")}</div>
        <p className="text-[11px] text-[var(--color-muted)]">{t("mtg.model_missing_body", { size: modelSizeLabel(tr.model.bytes) })}</p>
        {tr.download ? (
          <div className="flex items-center gap-2">
            <div className="h-1.5 flex-1 overflow-hidden rounded bg-[var(--color-border)]">
              <div className="h-full bg-[var(--color-accent)]" style={{ width: `${pct ?? 0}%` }} />
            </div>
            <span className="tabular-nums text-[11px]">{pct ?? 0} %</span>
            <button type="button" className="text-[11px] underline" onClick={tr.cancelDownload}>
              {t("common.cancel")}
            </button>
          </div>
        ) : (
          <button type="button" data-testid="ed-tr-download" className={chipClass(true) + " inline-flex items-center gap-1"} onClick={tr.downloadModel}>
            <Download size={12} /> {t("mtg.download", { size: modelSizeLabel(tr.model.bytes) })}
          </button>
        )}
        {tr.downloadError && <p className="text-[11px] text-red-500">{errorText(tr.downloadError)}</p>}
      </div>
    );
  }

  if (tr.running) {
    const p = tr.progress;
    const file = p && p.fileTotalMs > 0 ? Math.min(1, p.fileDoneMs / p.fileTotalMs) : 0;
    const whole = p && p.total > 0 ? Math.min(100, Math.round(((p.done + file) / p.total) * 100)) : 0;
    return (
      <div className="space-y-1.5 text-[12px]" data-testid="ed-tr-running">
        <div className="flex items-center justify-between gap-2">
          <span>{p && p.total > 1 ? t("rec6.tr.running_n", { n: Math.min(p.total, p.done + 1), total: p.total }) : t("rec6.tr.running")}</span>
          <button type="button" className={chipClass(false)} onClick={tr.cancel} data-testid="ed-tr-cancel">
            {t("common.cancel")}
          </button>
        </div>
        <div className="h-1.5 overflow-hidden rounded bg-[var(--color-border)]">
          <div className="anim-keep h-full bg-[var(--color-accent)] transition-transform duration-(--duration-base) ease-sharp origin-left" style={{ transform: `scaleX(${whole / 100})` }} />
        </div>
      </div>
    );
  }

  const notes: string[] = [];
  if (tr.last) {
    if (tr.last.skipped.length) notes.push(tn("rec6.tr.skipped", tr.last.skipped.length));
    if (tr.last.error) notes.push(t("rec6.tr.partial", { reason: transcribeError(t, tr.last.error) }));
    if (tr.last.cancelled) notes.push(t("rec6.tr.cancelled_kept"));
  }

  return (
    <div className="space-y-2" data-testid="ed-tr-start">
      {!compact && (
        <p className="flex items-start gap-1.5 text-[12px] text-[var(--color-muted)]">
          <FileAudio size={14} className="mt-0.5 shrink-0" /> {t("rec6.tr.none_hint")}
        </p>
      )}
      <div className="flex flex-wrap items-center gap-1">
        <Chips>
          {LANGS.map((l) => (
            <Chip key={l.id} label={l.id === "auto" ? t(l.label as MessageKey) : l.label} active={pick === l.id} onClick={() => setPick(l.id)} />
          ))}
        </Chips>
        <button type="button" className={chipClass(!compact) + " ml-auto"} onClick={() => tr.start(pick)} data-testid="ed-tr-go">
          {compact ? t("rec6.tr.again") : t("rec6.tr.go")}
        </button>
      </div>
      {tr.error && tr.error !== "model.missing" && (
        <p className="text-[11px] text-red-500" data-testid="ed-tr-error">
          {transcribeError(t, tr.error)}
        </p>
      )}
      {notes.length > 0 && (
        <p className="text-[11px] text-[var(--color-muted)]" data-testid="ed-tr-note">
          {notes.join(" ")}
        </p>
      )}
    </div>
  );
}
