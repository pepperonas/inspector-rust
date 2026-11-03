/**
 * The transcript cut of the `record` editor (stage 6, pure).
 *
 * A cut is a clip cut (`cutSourceRange` in lib/editor): deleting words splits their clip twice
 * and drops the middle, so every time mapping, the timeline and the scene keep working without
 * a second cut model. Words stay in the transcript; the ones no clip plays are the gaps
 * between neighbouring clips, and `restoreGap` joins those clips again.
 */
import { canRestoreGap, cutSourceRange } from "./editor";
import type { RecordEditDoc, RecordTranscript, RecordTranscriptWord } from "./ipc";

/** Silence kept next to a kept word when a transcript cut removes a pause (upstream 0.07 s). */
export const CUT_BREATH_MS = 70;
/** A cut edge closer than this many frames to a neighbouring cut snaps onto it. */
export const MERGE_CUT_FRAMES = 2;
/** Frame rate assumed for `MERGE_CUT_FRAMES` (upstream's fallback). */
export const CUT_FALLBACK_FPS = 30;

/** One word as the transcript pane shows it under a clip. */
export interface ClipWord {
  /** `clipIndex:wordId` — unique even when a duplicated clip plays the same word twice. */
  id: string;
  wordId: string;
  startMs: number;
  endMs: number;
  text: string;
  /** The user corrected the text (an `originalText` is stored). */
  edited: boolean;
}

const sourceOf = (doc: RecordEditDoc, clip: number): string | null => doc.clips[clip]?.source ?? null;

function transcriptFor(transcripts: readonly RecordTranscript[], source: string | null): RecordTranscript | undefined {
  return transcripts.find((t) => (t.source ?? null) === source);
}

const mid = (w: Pick<RecordTranscriptWord, "startMs" | "endMs">) => (w.startMs + w.endMs) / 2;

function toClipWord(clip: number, w: RecordTranscriptWord): ClipWord {
  return {
    id: `${clip}:${w.id}`,
    wordId: w.id,
    startMs: w.startMs,
    endMs: w.endMs,
    text: w.text,
    edited: w.originalText != null,
  };
}

/** The words clip `clip` plays: of its recording's transcript, those whose middle lies in the
 *  clip's source window `[startMs, endMs)`, in time order. */
export function clipWords(doc: RecordEditDoc, transcripts: readonly RecordTranscript[], clip: number): ClipWord[] {
  const c = doc.clips[clip];
  if (!c) return [];
  const t = transcriptFor(transcripts, sourceOf(doc, clip));
  if (!t) return [];
  return t.words
    .filter((w) => mid(w) >= c.startMs && mid(w) < c.endMs)
    .sort((a, b) => a.startMs - b.startMs || a.endMs - b.endMs)
    .map((w) => toClipWord(clip, w));
}

/** The cut edges `[startMs, endMs]` with breath: an edge in a pause next to a kept word moves
 *  into the pause so that `CUT_BREATH_MS` of it stays, at most half the pause; an edge inside a
 *  kept word, or with no kept word on its side, is left alone. `words` = the KEPT words. */
export function breathe(startMs: number, endMs: number, words: readonly ClipWord[]): [number, number] {
  const inside = (t: number) => words.some((w) => w.startMs < t && t < w.endMs);
  let s = startMs;
  let e = endMs;
  if (!inside(startMs)) {
    const prev = words
      .filter((w) => w.endMs <= startMs)
      .reduce<ClipWord | null>((b, w) => (!b || w.endMs > b.endMs ? w : b), null);
    if (prev) {
      const gap = startMs - prev.endMs;
      s = prev.endMs + Math.min(CUT_BREATH_MS, gap / 2);
    }
  }
  if (!inside(endMs)) {
    const next = words
      .filter((w) => w.startMs >= endMs)
      .reduce<ClipWord | null>((b, w) => (!b || w.startMs < b.startMs ? w : b), null);
    if (next) {
      const gap = next.startMs - endMs;
      e = next.startMs - Math.min(CUT_BREATH_MS, gap / 2);
    }
  }
  return [s, e];
}

/** Deletes words from clip `clip`: the cut runs from the first to the last selected word (all
 *  words between are cut too), breathes against the kept words, snaps onto a clip edge closer
 *  than two frames, and becomes a clip cut. Ids may be transcript word ids or `ClipWord.id`s. */
export function cutWords(doc: RecordEditDoc, clip: number, wordIds: readonly string[]): RecordEditDoc {
  const c = doc.clips[clip];
  if (!c) return doc;
  const words = clipWords(doc, doc.transcripts ?? [], clip);
  const ids = new Set(wordIds);
  const picked = words.map((w, i) => (ids.has(w.wordId) || ids.has(w.id) ? i : -1)).filter((i) => i >= 0);
  if (picked.length === 0) return doc;
  const first = Math.min(...picked);
  const last = Math.max(...picked);
  const kept = words.filter((_, i) => i < first || i > last);
  let [s, e] = breathe(words[first].startMs, words[last].endMs, kept);
  const snap = (MERGE_CUT_FRAMES * 1000) / CUT_FALLBACK_FPS;
  if (s - c.startMs < snap) s = c.startMs;
  if (c.endMs - e < snap) e = c.endMs;
  return cutSourceRange(doc, clip, s, e);
}

/** The cut words between clip `clip` and the next one (what `restoreGap` would bring back);
 *  empty when the two cannot be joined or nothing was spoken in the gap. */
export function gaps(
  doc: RecordEditDoc,
  transcripts: readonly RecordTranscript[],
  clip: number,
): { afterClip: number; words: ClipWord[] }[] {
  if (!canRestoreGap(doc, clip)) return [];
  const a = doc.clips[clip];
  const b = doc.clips[clip + 1];
  const t = transcriptFor(transcripts, sourceOf(doc, clip));
  if (!t) return [];
  const words = t.words
    .filter((w) => mid(w) >= a.endMs && mid(w) < b.startMs)
    .sort((x, y) => x.startMs - y.startMs)
    .map((w) => toClipWord(clip, w));
  return words.length ? [{ afterClip: clip, words }] : [];
}

/** Corrects one word's text (trimmed). The first change keeps what the recognizer heard in
 *  `originalText`; typing that back clears it. Empty text hides the word from the captions. */
export function setWordText(doc: RecordEditDoc, source: string | null, wordId: string, text: string): RecordEditDoc {
  const next = text.trim();
  let changed = false;
  const transcripts = (doc.transcripts ?? []).map((t) => {
    if ((t.source ?? null) !== source) return t;
    const words = t.words.map((w) => {
      if (w.id !== wordId) return w;
      const original = w.originalText ?? w.text;
      if (next === w.text) return w;
      changed = true;
      if (next === original) {
        const { originalText: _drop, ...rest } = w;
        return { ...rest, text: next };
      }
      return { ...w, text: next, originalText: original };
    });
    return { ...t, words };
  });
  return changed ? { ...doc, transcripts } : doc;
}

/** A fresh transcript of the same recording with the user's corrections carried over: a new
 *  word takes an old correction when the old word's `originalText` equals the new text and
 *  their times overlap (each old correction used once). */
export function carryOverEdits(old: RecordTranscript, next: RecordTranscript): RecordTranscript {
  const edits = old.words.filter((w) => w.originalText != null);
  const used = new Set<number>();
  const words = next.words.map((w) => {
    const i = edits.findIndex(
      (o, k) =>
        !used.has(k) &&
        (o.originalText ?? "").trim() === w.text.trim() &&
        o.startMs < w.endMs &&
        w.startMs < o.endMs,
    );
    if (i < 0) return w;
    used.add(i);
    return { ...w, text: edits[i].text, originalText: edits[i].originalText };
  });
  return { ...next, words };
}
