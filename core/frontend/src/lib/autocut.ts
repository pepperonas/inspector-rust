// Stage 12 of the recording editor: noise suppression on the microphone and the clean-up
// suggestions (pauses, filler words). The detection lives in Rust (`record/autocut.rs`,
// `record/denoise.rs`); this file holds the document edits and the small view rules.
import type { RecordCutSuggestion, RecordEditDoc, RecordEditorView } from "./ipc";
import { cutSourceRange } from "./editor";

/** The strengths the editor offers (Rust: `denoise::LEVELS`); 0 = off. */
export const DENOISE_LEVELS = [50, 75, 100] as const;

/** The nearest offered strength (Rust: `denoise::normalize_level`). */
export function normalizeDenoise(level: number | undefined | null): number {
  const v = Number.isFinite(level) ? Number(level) : 0;
  if (v < DENOISE_LEVELS[0] / 2) return 0;
  return DENOISE_LEVELS.reduce<number>((best, l) => (Math.abs(l - v) < Math.abs(best - v) ? l : best), DENOISE_LEVELS[0]);
}

export function setMicDenoise(doc: RecordEditDoc, level: number): RecordEditDoc {
  const next = normalizeDenoise(level);
  return normalizeDenoise(doc.micDenoise) === next ? doc : { ...doc, micDenoise: next };
}

/** The cleaned file's name next to `mic.wav` (Rust: `denoise::cache_name`). */
export const denoisedName = (level: number) => `mic.denoised-${level}.wav`;

/** The microphone file the preview plays for the recording in `dir` (`""` = own take): the
 *  noise-suppressed one only when Rust says it exists — the same rule as the export's scene. */
export function micFileFor(view: Pick<RecordEditorView, "doc" | "denoised">, dir: string): string {
  const level = normalizeDenoise(view.doc.micDenoise);
  return level > 0 && (view.denoised ?? []).includes(dir) ? denoisedName(level) : "mic.wav";
}

/** Noise suppression is on and a playing recording's cleaned file is still missing. */
export function denoisePending(view: Pick<RecordEditorView, "doc" | "micSources" | "denoised">): boolean {
  if (normalizeDenoise(view.doc.micDenoise) === 0) return false;
  const have = view.denoised ?? [];
  return (view.micSources ?? []).some((s) => !have.includes(s));
}

export type Cut = Pick<RecordCutSuggestion, "clip" | "startMs" | "endMs">;

/** Removes every cut from the document as ONE edit. Cuts are applied from the last to the
 *  first (by clip, then by time), so a cut never moves the clips an earlier one refers to. */
export function cutMany(doc: RecordEditDoc, cuts: readonly Cut[]): RecordEditDoc {
  const ordered = [...cuts].sort((a, b) => b.clip - a.clip || b.startMs - a.startMs);
  return ordered.reduce((d, c) => cutSourceRange(d, c.clip, c.startMs, c.endMs), doc);
}

/** What the cuts take out of the SOURCE, in ms (overlaps within a clip counted once). */
export function cutTotalMs(cuts: readonly Cut[]): number {
  const ordered = [...cuts].sort((a, b) => a.clip - b.clip || a.startMs - b.startMs);
  let total = 0;
  let clip = -1;
  let end = 0;
  for (const c of ordered) {
    if (c.clip !== clip) {
      clip = c.clip;
      end = 0;
    }
    const s = Math.max(c.startMs, end);
    if (c.endMs > s) {
      total += c.endMs - s;
      end = c.endMs;
    }
  }
  return total;
}

/** The pause lengths the clean-up offers (ms); inside Rust's `MIN_GAP_RANGE_MS`. */
export const GAP_CHOICES = [500, 700, 1000, 2000] as const;
export const DEFAULT_GAP_MS = 700;

/** `1.7 s`-style length in the app language. */
export function formatSeconds(ms: number, locale: string): string {
  return `${(ms / 1000).toLocaleString(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 })} s`;
}
