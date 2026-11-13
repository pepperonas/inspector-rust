import { useState } from "react";
import { Eraser } from "lucide-react";
import type { RecordAutocutOpts, RecordAutocutResult, RecordEditDoc } from "../../lib/ipc";
import { formatTimecode } from "../../lib/editor";
import { DEFAULT_GAP_MS, GAP_CHOICES, cutMany, cutTotalMs, formatSeconds } from "../../lib/autocut";
import { locale, useT } from "../../lib/i18n";
import { Chip, Group, Toggle, chipClass } from "./InspectorParts";

export interface CleanupBoxProps {
  doc: RecordEditDoc;
  /** Asks Rust for the pauses and filler words of the open project (changes nothing). */
  suggest: (opts: RecordAutocutOpts) => Promise<RecordAutocutResult>;
  onCommit: (doc: RecordEditDoc) => void;
  onSeek: (programmeMs: number) => void;
}

/**
 * "Clean up": finds pauses (in the microphone's waveform) and filler words (in the
 * transcript), lists them with a tick each, and cuts the ticked ones as one undo step.
 * Nothing is cut before the user says so.
 */
export function CleanupBox({ doc, suggest, onCommit, onSeek }: CleanupBoxProps) {
  const t = useT();
  const [silences, setSilences] = useState(true);
  const [fillers, setFillers] = useState(true);
  const [gapMs, setGapMs] = useState<number>(DEFAULT_GAP_MS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [found, setFound] = useState<{ doc: RecordEditDoc; res: RecordAutocutResult } | null>(null);
  const [off, setOff] = useState<ReadonlySet<string>>(new Set());
  // Suggestions describe one document; after any edit they are stale and go.
  const res = found && found.doc === doc ? found.res : null;

  const search = () => {
    setBusy(true);
    setError(false);
    const asked = doc;
    suggest({ silences, fillers, minGapMs: gapMs })
      .then((r) => {
        setFound({ doc: asked, res: r });
        setOff(new Set());
      })
      .catch(() => setError(true))
      .finally(() => setBusy(false));
  };

  const list = res?.suggestions ?? [];
  const chosen = list.filter((s) => !off.has(s.id));
  const toggle = (id: string) =>
    setOff((cur) => {
      const next = new Set(cur);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const apply = () => {
    if (!chosen.length) return;
    const next = cutMany(doc, chosen);
    setFound(null);
    if (next !== doc) onCommit(next);
  };

  return (
    <Group label={t("rec12.clean.title")}>
      <div className="space-y-2" data-testid="ed-cleanup">
        <Toggle label={t("rec12.clean.silences")} checked={silences} onChange={setSilences} />
        {silences && (
          <div className="flex flex-wrap items-center gap-1" role="group" aria-label={t("rec12.clean.gap")}>
            <span className="mr-1 text-[11px] text-[var(--color-muted)]">{t("rec12.clean.gap")}</span>
            {GAP_CHOICES.map((g) => (
              <Chip key={g} label={formatSeconds(g, locale())} active={gapMs === g} onClick={() => setGapMs(g)} />
            ))}
          </div>
        )}
        <Toggle label={t("rec12.clean.fillers")} checked={fillers} onChange={setFillers} />
        <button
          type="button"
          onClick={search}
          disabled={busy || (!silences && !fillers)}
          className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-2.5 py-1.5 text-[12px] hover:bg-[var(--color-bg)] disabled:opacity-40"
          data-testid="ed-cleanup-find"
        >
          <Eraser size={13} /> {busy ? t("rec12.clean.searching") : t("rec12.clean.find")}
        </button>
        {error && <p className="text-[11px] text-[var(--color-muted)]" role="alert">{t("rec12.clean.failed")}</p>}
        {res && (
          <div className="space-y-1.5" data-testid="ed-cleanup-result">
            {silences && !res.hasMic && <p className="text-[11px] text-[var(--color-muted)]">{t("rec12.clean.no_mic")}</p>}
            {silences && res.micPending && <p className="text-[11px] text-[var(--color-muted)]">{t("rec12.clean.mic_pending")}</p>}
            {fillers && !res.hasTranscript && <p className="text-[11px] text-[var(--color-muted)]">{t("rec12.clean.no_transcript")}</p>}
            {list.length === 0 ? (
              <p className="text-[12px]">{t("rec12.clean.none")}</p>
            ) : (
              <>
                <ul className="max-h-56 space-y-0.5 overflow-y-auto pr-1">
                  {list.map((s) => (
                    <li key={s.id} className="flex items-center gap-2 text-[12px]">
                      <input
                        type="checkbox"
                        checked={!off.has(s.id)}
                        onChange={() => toggle(s.id)}
                        aria-label={s.kind === "filler" ? s.text : t("rec12.clean.pause")}
                        data-testid={`ed-cleanup-check-${s.id}`}
                      />
                      <button
                        type="button"
                        onClick={() => onSeek(s.programmeMs)}
                        title={t("rec12.clean.jump")}
                        className="tabular-nums text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                      >
                        {formatTimecode(s.programmeMs)}
                      </button>
                      <span className="min-w-0 flex-1 truncate">
                        {s.kind === "filler" ? t("rec12.clean.filler_word", { word: s.text }) : t("rec12.clean.pause")}
                      </span>
                      <span className="tabular-nums text-[var(--color-muted)]">{formatSeconds(s.endMs - s.startMs, locale())}</span>
                    </li>
                  ))}
                </ul>
                <div className="flex flex-wrap items-center gap-1.5">
                  <button type="button" onClick={apply} disabled={!chosen.length} className={chipClass(true)} data-testid="ed-cleanup-apply">
                    {t("rec12.clean.apply", { n: chosen.length, time: formatSeconds(cutTotalMs(chosen), locale()) })}
                  </button>
                  <button
                    type="button"
                    onClick={() => setOff(chosen.length ? new Set(list.map((s) => s.id)) : new Set())}
                    className={chipClass(false)}
                  >
                    {chosen.length ? t("rec12.clean.none_selected") : t("rec12.clean.all_selected")}
                  </button>
                </div>
              </>
            )}
            {fillers && res.hasTranscript && <p className="text-[11px] text-[var(--color-muted)]">{t("rec12.clean.filler_note")}</p>}
          </div>
        )}
      </div>
    </Group>
  );
}
