// AI tasks (`task` / `ki`) — types mirroring `ai_tasks/*.rs` plus the pure
// helpers the panel uses. Nothing here talks to the backend.

export type AiProviderId = "anthropic" | "claude_cli" | "gemini" | "openai";
export type ScriptLanguage = "zsh" | "bash" | "python" | "applescript" | "powershell";

export type Trigger =
  | { type: "interval"; minutes: number }
  | { type: "schedule"; weekdays: number[]; hour: number; minute: number }
  | { type: "folder"; path: string }
  | { type: "app_start" }
  | { type: "wake" }
  | { type: "manual" };

export type TriggerType = Trigger["type"];

export interface AiProviderStatus {
  id: AiProviderId;
  label: string;
  configured: boolean;
  hint: string | null;
  model: string;
  default_model: string;
}

export interface AiConfig {
  provider: AiProviderId;
  models: Record<string, string>;
}

export interface Generated {
  name: string;
  language: ScriptLanguage;
  script: string;
  explanation: string;
}

export interface AiTaskRun {
  id: number;
  task_id: number;
  started_ms: number;
  duration_ms: number;
  exit_code: number | null;
  timed_out: boolean;
  trigger: string;
  output: string;
  error: string | null;
}

export interface AiTask {
  id: number;
  name: string;
  prompt: string;
  language: ScriptLanguage;
  script: string;
  explanation: string;
  provider: AiProviderId;
  trigger: Trigger;
  enabled: boolean;
  timeout_s: number;
  script_hash: string;
  approved: boolean;
  created_ms: number;
  updated_ms: number;
  last_fire_ms: number | null;
  last_run: AiTaskRun | null;
}

export interface AiTaskDraft {
  id: number | null;
  name: string;
  prompt: string;
  language: ScriptLanguage;
  script: string;
  explanation: string;
  provider: AiProviderId;
  trigger: Trigger;
  enabled: boolean;
  timeout_s: number;
}

export interface AiTasksState {
  tasks: AiTask[];
  running: number[];
  paused: boolean;
  languages: ScriptLanguage[];
  /** task id → next scheduled start (timed tasks that are eligible). */
  next_due: Record<string, number>;
}

export const LANGUAGE_LABEL: Record<ScriptLanguage, string> = {
  zsh: "zsh",
  bash: "Bash",
  python: "Python",
  applescript: "AppleScript",
  powershell: "PowerShell",
};

export const TRIGGER_LABEL: Record<TriggerType, string> = {
  interval: "Intervall",
  schedule: "Uhrzeit / Wochentag",
  folder: "Ordner ändert sich",
  app_start: "App-Start",
  wake: "Aufwachen aus dem Ruhezustand",
  manual: "Nur von Hand",
};

/** Order of the trigger picker. */
export const TRIGGER_TYPES: TriggerType[] = ["interval", "schedule", "folder", "app_start", "wake", "manual"];

export const WEEKDAY_SHORT = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];

/** Mirrors `trigger.rs` / `runner.rs` — pinned by a cross-language test. */
export const MIN_INTERVAL_MIN = 1;
export const MAX_INTERVAL_MIN = 7 * 24 * 60;
export const DEFAULT_TIMEOUT_S = 120;
export const MAX_TIMEOUT_S = 3600;

export function defaultTrigger(type: TriggerType): Trigger {
  switch (type) {
    case "interval":
      return { type, minutes: 60 };
    case "schedule":
      return { type, weekdays: [1, 2, 3, 4, 5], hour: 9, minute: 0 };
    case "folder":
      return { type, path: "" };
    default:
      return { type } as Trigger;
  }
}

const pad = (n: number) => String(n).padStart(2, "0");

/** "werktags", "Mo, Mi, Fr", "täglich" … */
export function weekdaysLabel(days: number[]): string {
  const d = [...new Set(days.filter((x) => x >= 1 && x <= 7))].sort((a, b) => a - b);
  if (d.length === 0 || d.length === 7) return "täglich";
  if (d.join() === "1,2,3,4,5") return "werktags";
  if (d.join() === "6,7") return "am Wochenende";
  return d.map((x) => WEEKDAY_SHORT[x - 1]).join(", ");
}

export function intervalLabel(minutes: number): string {
  if (minutes % (24 * 60) === 0) {
    const d = minutes / (24 * 60);
    return d === 1 ? "täglich" : `alle ${d} Tage`;
  }
  if (minutes % 60 === 0) {
    const h = minutes / 60;
    return h === 1 ? "stündlich" : `alle ${h} Stunden`;
  }
  return minutes === 1 ? "jede Minute" : `alle ${minutes} Minuten`;
}

/** One line, German: what makes this task run. */
export function triggerSummary(t: Trigger): string {
  switch (t.type) {
    case "interval":
      return intervalLabel(t.minutes);
    case "schedule":
      return `${weekdaysLabel(t.weekdays)} um ${pad(t.hour)}:${pad(t.minute)}`;
    case "folder":
      return t.path ? `wenn sich ${shortPath(t.path)} ändert` : "wenn sich ein Ordner ändert";
    case "app_start":
      return "beim Start von Inspector Rust";
    case "wake":
      return "nach dem Aufwachen";
    case "manual":
      return "nur von Hand";
  }
}

/** `/Users/x/Downloads` → `~/Downloads` (display only). */
export function shortPath(p: string): string {
  return p.replace(/^\/Users\/[^/]+/, "~").replace(/^\/home\/[^/]+/, "~");
}

/** Why a draft can't be saved yet, or null. */
export function draftProblem(d: Pick<AiTaskDraft, "name" | "script" | "trigger">): string | null {
  if (!d.name.trim()) return "Der Task braucht einen Namen.";
  if (!d.script.trim()) return "Noch kein Skript — erst beschreiben und erzeugen lassen.";
  const t = d.trigger;
  if (t.type === "folder" && !/^(\/|[A-Za-z]:[\\/])/.test(t.path.trim()))
    return "Für den Ordner einen vollständigen Pfad angeben (z. B. /Users/…/Downloads).";
  if (t.type === "interval" && !(t.minutes >= MIN_INTERVAL_MIN && t.minutes <= MAX_INTERVAL_MIN))
    return `Intervall zwischen ${MIN_INTERVAL_MIN} Minute und 7 Tagen.`;
  return null;
}

export type RunState = "ok" | "failed" | "timeout" | "error";

export function runState(r: AiTaskRun): RunState {
  if (r.error) return "error";
  if (r.timed_out) return "timeout";
  return r.exit_code === 0 ? "ok" : "failed";
}

export function runSummary(r: AiTaskRun): string {
  switch (runState(r)) {
    case "ok":
      return `erfolgreich · ${formatDuration(r.duration_ms)}`;
    case "failed":
      return `Fehler (Exit ${r.exit_code ?? "?"}) · ${formatDuration(r.duration_ms)}`;
    case "timeout":
      return `abgebrochen nach ${formatDuration(r.duration_ms)} (Zeitlimit)`;
    case "error":
      return `nicht gestartet: ${r.error}`;
  }
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.max(0, Math.round(ms))} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s < 10 ? s.toFixed(1).replace(".", ",") : Math.round(s)} s`;
  const m = Math.floor(s / 60);
  return `${m} min ${Math.round(s % 60)} s`;
}

/** "vor 3 min", "in 2 h" — relative to `now`. */
export function relativeTime(ms: number, now: number): string {
  const diff = ms - now;
  const abs = Math.abs(diff);
  const unit =
    abs < 60_000
      ? `${Math.max(1, Math.round(abs / 1000))} s`
      : abs < 3_600_000
        ? `${Math.round(abs / 60_000)} min`
        : abs < 86_400_000
          ? `${Math.round(abs / 3_600_000)} h`
          : `${Math.round(abs / 86_400_000)} Tg.`;
  return diff >= 0 ? `in ${unit}` : `vor ${unit}`;
}

export type DiffLine = { kind: "same" | "add" | "del"; text: string };

/**
 * Line diff (LCS) between the approved and the proposed script — so a
 * revision is reviewed as a change, not re-read in full. Above ~2000×2000
 * lines it falls back to "all removed, all added" rather than allocating a
 * huge table; scripts are capped at 64 KB, so that's a corner.
 */
export function lineDiff(before: string, after: string): DiffLine[] {
  const a = before.replace(/\n$/, "").split("\n");
  const b = after.replace(/\n$/, "").split("\n");
  if (before === "") return b.map((text) => ({ kind: "add", text }));
  if (a.length * b.length > 4_000_000) {
    return [...a.map((text) => ({ kind: "del" as const, text })), ...b.map((text) => ({ kind: "add" as const, text }))];
  }
  const n = a.length;
  const m = b.length;
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ kind: "same", text: a[i] });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      out.push({ kind: "del", text: a[i++] });
    } else {
      out.push({ kind: "add", text: b[j++] });
    }
  }
  while (i < n) out.push({ kind: "del", text: a[i++] });
  while (j < m) out.push({ kind: "add", text: b[j++] });
  return out;
}

/** Counts for "+3 −1". */
export function diffStats(d: DiffLine[]): { added: number; removed: number } {
  return {
    added: d.filter((l) => l.kind === "add").length,
    removed: d.filter((l) => l.kind === "del").length,
  };
}

/**
 * Lines worth a second look before approving: deleting, privilege, network
 * downloads piped into a shell. A hint, not a verdict — the user decides.
 */
const RISKY: [RegExp, string][] = [
  [/\brm\s+(-[a-zA-Z]*[rf]|--recursive|--force)/, "löscht Dateien (rm -r/-f)"],
  [/\bsudo\b/, "verlangt Administratorrechte (sudo)"],
  [/(curl|wget)[^|\n]*\|\s*(ba|z)?sh\b/, "lädt Code aus dem Netz und führt ihn aus"],
  [/\bshutil\.rmtree\b|\bos\.remove\b|\bos\.unlink\b/, "löscht Dateien (Python)"],
  [/\bRemove-Item\b/, "löscht Dateien (PowerShell)"],
  [/\bdd\s+if=|\bmkfs\b|\bdiskutil\s+(erase|partition)/, "schreibt direkt auf Datenträger"],
  [/\bchmod\s+(-R\s+)?777\b/, "macht Dateien für alle beschreibbar"],
  [/\bkillall\b|\bpkill\b/, "beendet Programme"],
];

export function riskHints(script: string): string[] {
  const out: string[] = [];
  for (const [re, label] of RISKY) if (re.test(script) && !out.includes(label)) out.push(label);
  return out;
}

/** Keep the old draft cache shape stable for the panel (survives remounts). */
export function emptyDraft(provider: AiProviderId): AiTaskDraft {
  return {
    id: null,
    name: "",
    prompt: "",
    language: "zsh",
    script: "",
    explanation: "",
    provider,
    trigger: defaultTrigger("interval"),
    enabled: true,
    timeout_s: DEFAULT_TIMEOUT_S,
  };
}

export function draftFromTask(t: AiTask): AiTaskDraft {
  return {
    id: t.id,
    name: t.name,
    prompt: t.prompt,
    language: t.language,
    script: t.script,
    explanation: t.explanation,
    provider: t.provider,
    trigger: t.trigger,
    enabled: t.enabled,
    timeout_s: t.timeout_s,
  };
}

/** Did the user change the script (language or text) relative to the saved task? */
export function scriptChanged(d: AiTaskDraft, saved: AiTask | null): boolean {
  if (!saved) return true;
  return d.language !== saved.language || d.script !== saved.script;
}
