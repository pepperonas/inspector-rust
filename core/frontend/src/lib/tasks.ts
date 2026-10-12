// AI tasks (`task` / `ki`) — types mirroring `ai_tasks/*.rs` plus the pure
// helpers the panel uses. Nothing here talks to the backend.

import { locale, t, t as tr, type MessageKey } from "./i18n";

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

/** Labels read the catalogue on access (getters), so they follow the language. */
export const TRIGGER_LABEL: Record<TriggerType, string> = {
  get interval() { return t("tasks.trigger.interval"); },
  get schedule() { return t("tasks.trigger.schedule"); },
  get folder() { return t("tasks.trigger.folder"); },
  get app_start() { return t("tasks.trigger.app_start"); },
  get wake() { return t("tasks.trigger.wake"); },
  get manual() { return t("tasks.trigger.manual"); },
};

/** Order of the trigger picker. */
export const TRIGGER_TYPES: TriggerType[] = ["interval", "schedule", "folder", "app_start", "wake", "manual"];

/** Monday-first short weekday names in the current language. */
export function weekdayShort(): string[] {
  return t("common.weekdays_short").split(",");
}

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
  if (d.length === 0 || d.length === 7) return t("tasks.daily");
  if (d.join() === "1,2,3,4,5") return t("tasks.weekdays");
  if (d.join() === "6,7") return t("tasks.weekend");
  const names = weekdayShort();
  return d.map((x) => names[x - 1]).join(", ");
}

export function intervalLabel(minutes: number): string {
  if (minutes % (24 * 60) === 0) {
    const d = minutes / (24 * 60);
    return d === 1 ? t("tasks.daily") : t("tasks.every_days", { n: d });
  }
  if (minutes % 60 === 0) {
    const h = minutes / 60;
    return h === 1 ? t("tasks.hourly") : t("tasks.every_hours", { n: h });
  }
  return minutes === 1 ? t("tasks.every_minute") : t("tasks.every_minutes", { n: minutes });
}

/** One line: what makes this task run. */
export function triggerSummary(t: Trigger): string {
  switch (t.type) {
    case "interval":
      return intervalLabel(t.minutes);
    case "schedule":
      return tr("tasks.at_time", { days: weekdaysLabel(t.weekdays), time: `${pad(t.hour)}:${pad(t.minute)}` });
    case "folder":
      return t.path ? tr("tasks.when_folder", { path: shortPath(t.path) }) : tr("tasks.when_any_folder");
    case "app_start":
      return tr("tasks.on_start");
    case "wake":
      return tr("tasks.after_wake");
    case "manual":
      return tr("tasks.manual_only");
  }
}

/** `/Users/x/Downloads` → `~/Downloads` (display only). */
export function shortPath(p: string): string {
  return p.replace(/^\/Users\/[^/]+/, "~").replace(/^\/home\/[^/]+/, "~");
}

/** Why a draft can't be saved yet, or null. */
export function draftProblem(d: Pick<AiTaskDraft, "name" | "script" | "trigger">): string | null {
  if (!d.name.trim()) return t("tasks.problem.name");
  if (!d.script.trim()) return t("tasks.problem.script");
  const trig = d.trigger;
  if (trig.type === "folder" && !/^(\/|[A-Za-z]:[\\/])/.test(trig.path.trim()))
    return t("tasks.problem.folder");
  if (trig.type === "interval" && !(trig.minutes >= MIN_INTERVAL_MIN && trig.minutes <= MAX_INTERVAL_MIN))
    return t("tasks.problem.interval", { min: MIN_INTERVAL_MIN });
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
      return t("tasks.run.ok", { dur: formatDuration(r.duration_ms) });
    case "failed":
      return t("tasks.run.failed", { code: r.exit_code ?? "?", dur: formatDuration(r.duration_ms) });
    case "timeout":
      return t("tasks.run.timeout", { dur: formatDuration(r.duration_ms) });
    case "error":
      return t("tasks.run.error", { error: r.error ?? "" });
  }
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.max(0, Math.round(ms))} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s < 10 ? s.toLocaleString(locale(), { maximumFractionDigits: 1, minimumFractionDigits: 1 }) : Math.round(s)} s`;
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
          : t("tasks.days_short", { n: Math.round(abs / 86_400_000) });
  return diff >= 0 ? t("tasks.in", { unit }) : t("tasks.ago", { unit });
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
const RISKY: [RegExp, MessageKey][] = [
  [/\brm\s+(-[a-zA-Z]*[rf]|--recursive|--force)/, "tasks.risk.rm"],
  [/\bsudo\b/, "tasks.risk.sudo"],
  [/(curl|wget)[^|\n]*\|\s*(ba|z)?sh\b/, "tasks.risk.curl_sh"],
  [/\bshutil\.rmtree\b|\bos\.remove\b|\bos\.unlink\b/, "tasks.risk.py_rm"],
  [/\bRemove-Item\b/, "tasks.risk.ps_rm"],
  [/\bdd\s+if=|\bmkfs\b|\bdiskutil\s+(erase|partition)/, "tasks.risk.disk"],
  [/\bchmod\s+(-R\s+)?777\b/, "tasks.risk.chmod"],
  [/\bkillall\b|\bpkill\b/, "tasks.risk.kill"],
];

export function riskHints(script: string): string[] {
  const out: string[] = [];
  for (const [re, key] of RISKY) {
    const label = t(key);
    if (re.test(script) && !out.includes(label)) out.push(label);
  }
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
