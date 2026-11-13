import { useMemo, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, MouseEvent as ReactMouseEvent } from "react";
import { RotateCcw } from "lucide-react";
import type { EditorSourcePos, RecordAutocutOpts, RecordAutocutResult, RecordEditDoc } from "../../lib/ipc";
import { CleanupBox } from "./CleanupBox";
import { formatTimecode, restoreGap } from "../../lib/editor";
import { clipWords, cutWords, gaps, setWordText, type ClipWord } from "../../lib/transcript";
import { matchAction } from "../../lib/editor-shortcuts";
import { useT } from "../../lib/i18n";
import { useBindings } from "./bindings-context";
import { chipClass } from "./InspectorParts";
import { TranscribeBox } from "./TranscribeBox";
import type { Transcription } from "./useTranscription";

export interface TranscriptTabProps {
  doc: RecordEditDoc;
  /** The playhead's clip and source position (Rust); highlights the word being spoken. */
  cue: EditorSourcePos | null;
  tr: Transcription;
  onCommit: (doc: RecordEditDoc) => void;
  /** Source → programme ms, answered by Rust (`editor_to_programme`). Never computed here. */
  toProgramme: (clip: number, sourceMs: number) => Promise<number | null>;
  /** Moves the playhead to programme ms. */
  onSeek: (programmeMs: number) => void;
  /** Words were selected: other selections (a region, a clip) let go so ⌫ means the words. */
  onWordsSelected?: () => void;
  /** Pauses and filler words that could be cut (Rust); without it the clean-up is not offered. */
  suggest?: (opts: RecordAutocutOpts) => Promise<RecordAutocutResult>;
}

type Sel = { clip: number; anchor: number; focus: number };

/**
 * The transcript under each clip: click a word to jump there, shift-click to extend, ⌫ cuts
 * the selected words (one undo step), a double-click corrects a word. The words a cut removed
 * stand struck through between their clips and come back with one click.
 */
export function TranscriptTab({ doc, cue, tr, onCommit, toProgramme, onSeek, onWordsSelected, suggest }: TranscriptTabProps) {
  const t = useT();
  const bindings = useBindings();
  const transcripts = useMemo(() => doc.transcripts ?? [], [doc.transcripts]);
  const has = transcripts.some((x) => x.words.length > 0);
  const blocks = useMemo(() => doc.clips.map((_, i) => clipWords(doc, transcripts, i)), [doc, transcripts]);
  const [sel, setSel] = useState<Sel | null>(null);
  const [editing, setEditing] = useState<{ clip: number; id: string; text: string } | null>(null);
  /** The open edit already ended (Enter/Esc): a blur from the input leaving must not save again. */
  const done = useRef(false);
  const startEdit = (v: { clip: number; id: string; text: string }) => {
    done.current = false;
    setEditing(v);
  };
  const box = useRef<HTMLDivElement>(null);
  // A selection whose clip changed shape (a cut, an undo) is dropped, never stretched.
  const [seenBlocks, setSeenBlocks] = useState(blocks);
  if (seenBlocks !== blocks) {
    setSeenBlocks(blocks);
    if (sel) setSel(null);
  }

  const originals = useMemo(() => {
    const m = new Map<string, string>();
    for (const tx of transcripts) for (const w of tx.words) if (w.originalText != null) m.set(`${tx.source ?? ""}\u0000${w.id}`, w.originalText);
    return m;
  }, [transcripts]);
  const original = (clip: number, w: ClipWord) => originals.get(`${doc.clips[clip]?.source ?? ""}\u0000${w.wordId}`) ?? "";

  const selected = (clip: number, i: number) =>
    !!sel && sel.clip === clip && i >= Math.min(sel.anchor, sel.focus) && i <= Math.max(sel.anchor, sel.focus);

  const click = (clip: number, i: number, w: ClipWord, e: ReactMouseEvent) => {
    const extend = e.shiftKey;
    setSel((cur) => (extend && cur && cur.clip === clip ? { ...cur, focus: i } : { clip, anchor: i, focus: i }));
    onWordsSelected?.();
    // The keys (⌫) go to the transcript; WKWebView does not focus a clicked button by itself.
    box.current?.focus({ preventScroll: true });

    void toProgramme(clip, w.startMs).then((ms) => {
      if (ms != null) onSeek(ms);
    });
  };

  const onKeyDown = (e: ReactKeyboardEvent) => {
    if (editing || !sel || e.defaultPrevented || e.repeat) return;
    if (matchAction(e.nativeEvent, bindings) !== "deleteSelected") return;
    const words = blocks[sel.clip] ?? [];
    const ids = words.filter((_, i) => selected(sel.clip, i)).map((w) => w.id);
    if (!ids.length) return;
    // The editor's own Delete (the clip) and the timeline's (a region) stand still.
    e.preventDefault();
    const next = cutWords(doc, sel.clip, ids);
    setSel(null);
    if (next !== doc) onCommit(next);
  };

  const save = () => {
    if (!editing || done.current) return;
    done.current = true;
    const source = doc.clips[editing.clip]?.source ?? null;
    const next = setWordText(doc, source, editing.id, editing.text);
    setEditing(null);
    if (next !== doc) onCommit(next);
  };

  return (
    <>
      <TranscribeBox tr={tr} lang={doc.captions?.lang ?? "auto"} compact={has} />
      {has && (
        <div
          ref={box}
          tabIndex={0}
          onKeyDown={onKeyDown}
          aria-label={t("rec6.tr.title")}
          className="space-y-3 rounded-md outline-none focus-visible:ring-1 focus-visible:ring-[var(--color-accent)]"
          data-testid="ed-transcript"
        >
          {blocks.map((words, ci) => {
            const clip = doc.clips[ci];
            const gap = gaps(doc, transcripts, ci)[0];
            return (
              <div key={ci} className="space-y-1">
                <section data-testid={`ed-tr-clip-${ci}`} className="space-y-1">
                  <h3 className="text-[11px] font-medium text-[var(--color-muted)]">
                    {t("rec6.tr.clip", { n: ci + 1 })} · {formatTimecode(clip.startMs)}
                  </h3>
                  {words.length === 0 ? (
                    <p className="text-[11px] text-[var(--color-muted)]">{t("rec6.tr.clip_empty")}</p>
                  ) : (
                    <p className="flex flex-wrap gap-x-0.5 gap-y-0.5 text-[13px] leading-relaxed">
                      {words.map((w, i) => {
                        if (editing && editing.clip === ci && editing.id === w.wordId) {
                          return (
                            <input
                              key={w.id}
                              autoFocus
                              value={editing.text}
                              aria-label={t("rec6.tr.edit_word")}
                              onChange={(e) => setEditing({ ...editing, text: e.target.value })}
                              onBlur={save}
                              onKeyDown={(e) => {
                                e.stopPropagation();
                                if (e.key === "Enter") {
                                  e.preventDefault();
                                  save();
                                } else if (e.key === "Escape") {
                                  e.preventDefault();
                                  done.current = true;
                                  setEditing(null);
                                }
                              }}
                              className="w-24 rounded border border-[var(--color-accent)] bg-transparent px-1 text-[13px]"
                              data-testid="ed-tr-edit"
                            />
                          );
                        }
                        const current = !!cue && cue.clip === ci && w.startMs <= cue.sourceMs && cue.sourceMs < w.endMs;
                        const hidden = !w.text;
                        return (
                          <button
                            key={w.id}
                            type="button"
                            data-word={w.wordId}
                            data-current={current || undefined}
                            aria-pressed={selected(ci, i)}
                            title={w.edited ? t("rec6.tr.edited", { original: original(ci, w) }) : undefined}
                            onClick={(e) => click(ci, i, w, e)}
                            onDoubleClick={() => startEdit({ clip: ci, id: w.wordId, text: w.text })}
                            className={
                              "rounded px-0.5 transition-colors duration-(--duration-fast) ease-sharp " +
                              (selected(ci, i)
                                ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                                : current
                                  ? "bg-[color-mix(in_srgb,var(--color-accent)_25%,transparent)]"
                                  : "hover:bg-[var(--color-bg)]") +
                              (w.edited ? " underline decoration-dotted" : "") +
                              (hidden ? " italic text-[var(--color-muted)] line-through" : "")
                            }
                          >
                            {hidden ? original(ci, w) || "…" : w.text}
                          </button>
                        );
                      })}
                    </p>
                  )}
                </section>
                {gap && (
                  <div className="flex items-start gap-1.5 text-[12px]" data-testid={`ed-tr-gap-${ci}`}>
                    <span className="flex-1 text-[var(--color-muted)] line-through">{gap.words.map((w) => w.text).join(" ")}</span>
                    <button
                      type="button"
                      className={chipClass(false) + " inline-flex shrink-0 items-center gap-1"}
                      onClick={() => onCommit(restoreGap(doc, ci))}
                      data-testid={`ed-tr-restore-${ci}`}
                    >
                      <RotateCcw size={11} /> {t("rec6.tr.restore")}
                    </button>
                  </div>
                )}
              </div>
            );
          })}
          <p className="text-[11px] text-[var(--color-muted)]">{t("rec6.tr.help")}</p>
        </div>
      )}
      {suggest && <CleanupBox doc={doc} suggest={suggest} onCommit={onCommit} onSeek={onSeek} />}
    </>
  );
}
