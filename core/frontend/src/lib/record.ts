import type { MessageKey } from "./i18n";

export type RecordPhase = "idle" | "picking" | "countdown" | "recording" | "paused" | "finishing";

export interface RecordStatusLike {
  phase: RecordPhase;
  source?: string | null;
  supported?: boolean;
}

/** `m:ss`, from one hour `h:mm:ss`. */
export function formatElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

const PHASE_KEYS: Record<RecordPhase, MessageKey> = {
  idle: "rec2.phase.idle",
  picking: "rec2.phase.picking",
  countdown: "rec2.phase.countdown",
  recording: "rec2.phase.recording",
  paused: "rec2.phase.paused",
  finishing: "rec2.phase.finishing",
};

export function phaseLabelKey(phase: RecordPhase): MessageKey {
  return PHASE_KEYS[phase] ?? "rec2.phase.idle";
}

/** A source is chosen, nothing runs, and recording is supported here. */
export function canStart(state: RecordStatusLike): boolean {
  return state.phase === "idle" && !!state.source && state.supported !== false;
}

/** Elapsed ms to show: the last reported value plus local time, frozen unless recording. */
export function displayElapsed(baseMs: number, receivedAt: number, now: number, phase: RecordPhase): number {
  return phase === "recording" ? baseMs + Math.max(0, now - receivedAt) : baseMs;
}
