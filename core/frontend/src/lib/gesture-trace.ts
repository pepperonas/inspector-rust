import type { GestureReplayRow } from "./ipc";

/** Length of a recording (`gestures record`). */
export const RECORD_SECS = 30;

const VERDICTS: Record<string, string> = {
  dispatched: "fired",
  palm: "ignored (palm)",
  thumb: "ignored (thumb)",
  edge_zone: "ignored (edge zone)",
  typing_guard: "blocked (typing)",
  typing_await_center: "blocked (typing — waiting for a centre touch)",
  palm_on_pad: "blocked (palm on the pad)",
  finger_count_changed: "blocked (finger count changed)",
  too_slow: "blocked (too slow)",
  uneven_fingers: "blocked (fingers moved unevenly)",
  cooldown: "blocked (cooldown)",
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
