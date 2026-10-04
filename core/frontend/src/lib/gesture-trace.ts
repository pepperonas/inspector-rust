import type { GestureReplayRow } from "./ipc";

/** Length of a recording started from Settings. */
export const RECORD_SECS = 30;

const VERDICTS: Record<string, string> = {
  dispatched: "fired",
  typing_guard: "blocked (typing)",
  unmapped: "ignored (no binding)",
};

/** Human verdict for a replay row: `config:<reason>` keeps its reason. */
export function verdictLabel(verdict: string): string {
  if (verdict.startsWith("config:")) return `blocked (${verdict.slice(7)})`;
  return VERDICTS[verdict] ?? verdict;
}

/** One replay row as a single line, e.g. `12.34 s  tap ×3  → mute_toggle  fired`. */
export function formatReplayRow(r: GestureReplayRow): string {
  const t = (r.t_ms / 1000).toFixed(2).padStart(6);
  const action = r.action ? `→ ${r.action}  ` : "";
  return `${t} s  ${r.kind} ×${r.fingers}  ${action}${verdictLabel(r.verdict)}`;
}
