import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  Bot,
  Check,
  ChevronDown,
  ChevronRight,
  Loader2,
  Pause,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  ShieldCheck,
  Sparkles,
  Trash2,
} from "lucide-react";
import { useTauriEvent } from "../hooks/useTauriEvent";
import {
  aiGetConfig,
  aiProviderStatus,
  aiTaskApprove,
  aiTaskDelete,
  aiTaskGenerate,
  aiTaskRevoke,
  aiTaskRunNow,
  aiTaskRuns,
  aiTaskSave,
  aiTaskSetEnabled,
  aiTasksSetPaused,
  aiTasksState,
} from "../lib/ipc";
import {
  LANGUAGE_LABEL,
  TRIGGER_LABEL,
  TRIGGER_TYPES,
  weekdayShort,
  MAX_TIMEOUT_S,
  defaultTrigger,
  diffStats,
  draftFromTask,
  draftProblem,
  emptyDraft,
  lineDiff,
  relativeTime,
  riskHints,
  runState,
  runSummary,
  scriptChanged,
  shortPath,
  triggerSummary,
  type AiProviderId,
  type AiProviderStatus,
  type AiTask,
  type AiTaskDraft,
  type AiTaskRun,
  type AiTasksState,
  type Trigger,
} from "../lib/tasks";
import { t, tn, useT } from "../lib/i18n";

/**
 * The `task` / `ki` panel: list of AI tasks, the editor (describe → the AI
 * writes a script → review → approve), and a task's detail with its run log.
 *
 * The editor session lives at MODULE level, not in component state: writing
 * a script takes the AI 10–60 s, and the popup may close meanwhile (focus
 * loss). The request keeps running, its result lands here, and reopening the
 * panel shows it — nothing is lost and nothing is generated twice.
 */

type View = { kind: "list" } | { kind: "edit" } | { kind: "detail"; id: number };

interface Session {
  view: View;
  draft: AiTaskDraft | null;
  /** The saved task the draft edits (approval + diff baseline). */
  base: AiTask | null;
  generating: boolean;
  genError: string | null;
  feedback: string;
}

let session: Session = { view: { kind: "list" }, draft: null, base: null, generating: false, genError: null, feedback: "" };
const listeners = new Set<() => void>();
function patch(p: Partial<Session>) {
  session = { ...session, ...p };
  listeners.forEach((l) => l());
}
function useSession(): Session {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => session,
  );
}

/** For tests: start from a clean slate. */
export function resetTasksSession() {
  session = { view: { kind: "list" }, draft: null, base: null, generating: false, genError: null, feedback: "" };
  listeners.forEach((l) => l());
}

async function generate(feedback: string | null) {
  const d = session.draft;
  if (!d || session.generating) return;
  patch({ generating: true, genError: null });
  try {
    const revising = feedback !== null && d.script.trim() !== "";
    const g = await aiTaskGenerate({
      provider: d.provider,
      description: d.prompt,
      previousLanguage: revising ? d.language : null,
      previousScript: revising ? d.script : null,
      feedback,
    });
    const cur = session.draft ?? d;
    patch({
      draft: {
        ...cur,
        // Keep a name the user already typed; take the AI's otherwise.
        name: cur.name.trim() ? cur.name : g.name,
        language: g.language,
        script: g.script,
        explanation: g.explanation,
      },
      feedback: "",
      generating: false,
    });
  } catch (e) {
    patch({ generating: false, genError: String(e) });
  }
}

const field =
  "rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px] outline-none focus:border-[var(--color-accent)]";
const btn =
  "flex items-center gap-1 rounded-md border border-[var(--color-border)] px-2.5 py-1 text-[11px] hover:bg-[var(--color-surface)] disabled:opacity-40";
const primary =
  "flex items-center gap-1 rounded-md bg-[var(--color-accent)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-accent-fg)] disabled:opacity-40";

export function TasksPanel({
  arg,
  focused,
  onExit,
  onOpenSettings,
}: {
  arg: string;
  focused: boolean;
  onExit: () => void;
  onOpenSettings: () => void;
}) {
  useT();
  const s = useSession();
  const [state, setState] = useState<AiTasksState | null>(null);
  const [providers, setProviders] = useState<AiProviderStatus[] | null>(null);
  const [defaultProvider, setDefaultProvider] = useState<AiProviderId>("anthropic");
  const [now, setNow] = useState(() => Date.now());
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    aiTasksState()
      .then(setState)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refresh();
    aiProviderStatus()
      .then(setProviders)
      .catch(() => setProviders([]));
    aiGetConfig()
      .then((c) => setDefaultProvider(c.provider))
      .catch(() => {});
  }, [refresh]);
  useTauriEvent("ai-tasks-changed", refresh);
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(t);
  }, []);

  // `task <beschreibung>` / `ki <beschreibung>` opens the editor pre-filled —
  // once, and only on Enter (focus): while still typing, every prefix would
  // otherwise start a draft.
  const argUsed = useRef(false);
  useEffect(() => {
    const a = arg.trim();
    if (!focused || argUsed.current || !a || s.draft || s.generating) return;
    argUsed.current = true;
    const desc = a.replace(/^(neu|new)\b\s*/i, "");
    patch({ view: { kind: "edit" }, draft: { ...emptyDraft(defaultProvider), prompt: desc }, base: null });
  }, [arg, focused, defaultProvider, s.draft, s.generating]);

  // Esc: blur a field first, then step back (detail/editor → list → exit).
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.repeat) return;
      const t = e.target as HTMLElement | null;
      e.preventDefault();
      e.stopPropagation();
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT")) {
        t.blur();
        return;
      }
      if (session.view.kind !== "list") {
        patch({ view: { kind: "list" } });
        return;
      }
      onExit();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit]);

  const anyProvider = providers?.some((p) => p.configured) ?? true;

  const startNew = () => {
    patch({ view: { kind: "edit" }, draft: emptyDraft(defaultProvider), base: null, genError: null, feedback: "" });
  };

  return (
    <div className="flex h-full flex-col overflow-hidden text-[12px]" data-testid="tasks-panel">
      <div className="flex items-center gap-2 border-b border-[var(--color-border)] px-3 py-2">
        {s.view.kind !== "list" && (
          <button className="rounded p-1 hover:bg-[var(--color-surface)]" title={t("tasks.back_esc")} onClick={() => patch({ view: { kind: "list" } })}>
            <ArrowLeft size={14} />
          </button>
        )}
        <Bot size={15} className="text-[var(--color-accent)]" />
        <span className="font-semibold">{t("tasks.ai_tasks")}</span>
        {state?.paused && (
          <span className="rounded bg-amber-500/15 px-1.5 py-0.5 text-[10px] text-amber-500">{t("tasks.all_paused")}</span>
        )}
        <div className="ml-auto flex items-center gap-1.5">
          {state && s.view.kind === "list" && (
            <button
              className={btn}
              title={state.paused ? t("tasks.allow_automatic_runs_again") : t("tasks.stop_all_automatic_runs")}
              onClick={() => void aiTasksSetPaused(!state.paused).then(refresh)}
            >
              {state.paused ? <Play size={11} /> : <Pause size={11} />}
              {state.paused ? t("tasks.resume") : t("tasks.pause_all")}
            </button>
          )}
          {s.view.kind === "list" && (
            <button className={primary} onClick={startNew}>
              <Plus size={11} /> {t("tasks.new_task")}
            </button>
          )}
        </div>
      </div>

      {error && <p className="px-3 py-2 text-red-500">{error}</p>}

      {!anyProvider && (
        <div className="m-3 rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-[11px]">
          {t("tasks.no_provider")}{" "}
          <button className="underline" onClick={onOpenSettings}>
            {t("tasks.settings_ai_providers")}
          </button>
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto">
        {s.view.kind === "list" && state && (
          <TaskList
            state={state}
            now={now}
            onOpen={(id) => patch({ view: { kind: "detail", id } })}
            onNew={startNew}
            refresh={refresh}
          />
        )}
        {s.view.kind === "edit" && s.draft && (
          <Editor
            s={s}
            providers={providers ?? []}
            languages={state?.languages ?? ["zsh", "bash", "python"]}
            onSaved={(t) => {
              patch({ view: { kind: "detail", id: t.id }, draft: null, base: null });
              refresh();
            }}
          />
        )}
        {s.view.kind === "detail" && state && (
          <Detail
            task={state.tasks.find((t) => t.id === (s.view as { id: number }).id) ?? null}
            running={state.running.includes((s.view as { id: number }).id)}
            nextDue={state.next_due[String((s.view as { id: number }).id)] ?? null}
            paused={state.paused}
            now={now}
            refresh={refresh}
          />
        )}
      </div>
    </div>
  );
}

function StatusDot({ task, running }: { task: AiTask; running: boolean }) {
  if (running) return <Loader2 size={11} className="shrink-0 animate-spin text-[var(--color-accent)]" />;
  const r = task.last_run;
  const color = !task.approved
    ? "bg-amber-500"
    : !task.enabled
      ? "bg-[var(--color-muted)]"
      : !r
        ? "bg-sky-500"
        : runState(r) === "ok"
          ? "bg-emerald-500"
          : "bg-red-500";
  return <span className={`h-2 w-2 shrink-0 rounded-full ${color}`} />;
}

function TaskList({
  state,
  now,
  onOpen,
  onNew,
  refresh,
}: {
  state: AiTasksState;
  now: number;
  onOpen: (id: number) => void;
  onNew: () => void;
  refresh: () => void;
}) {
  if (state.tasks.length === 0) {
    return (
      <div className="flex flex-col items-center gap-3 px-6 py-10 text-center text-[var(--color-muted)]">
        <Sparkles size={22} className="text-[var(--color-accent)]" />
        <p>
          {t("tasks.describe_in_your_own_words")}
        </p>
        <p className="text-[11px]">{t("tasks.example")}</p>
        <button className={primary} onClick={onNew}>
          <Plus size={11} /> {t("tasks.create_the_first_task")}
        </button>
      </div>
    );
  }
  return (
    <ul className="divide-y divide-[var(--color-border)]">
      {state.tasks.map((task) => {
        const running = state.running.includes(task.id);
        const due = state.next_due[String(task.id)];
        return (
          <li key={task.id} className="flex items-center gap-2 px-3 py-2 hover:bg-[var(--color-surface)]">
            <button className="flex min-w-0 flex-1 items-center gap-2 text-left" onClick={() => onOpen(task.id)}>
              <StatusDot task={task} running={running} />
              <div className="min-w-0 flex-1">
                <div className="truncate font-medium">{task.name}</div>
                <div className="truncate text-[11px] text-[var(--color-muted)]">
                  {triggerSummary(task.trigger)}
                  {!task.approved
                    ? ` · ${t("tasks.needs_approval")}`
                    : running
                      ? ` · ${t("tasks.running")}`
                      : due && task.enabled && !state.paused
                        ? ` · ${t("tasks.next_run", { when: relativeTime(due, now) })}`
                        : task.last_run
                          ? ` · ${t("tasks.last_run", { when: relativeTime(task.last_run.started_ms, now) })}`
                          : ""}
                </div>
              </div>
            </button>
            <label className="flex shrink-0 items-center gap-1 text-[10px] text-[var(--color-muted)]" title={t("tasks.run_automatically")}>
              <input
                type="checkbox"
                checked={task.enabled}
                onChange={(e) => void aiTaskSetEnabled(task.id, e.target.checked).then(refresh)}
              />
              {t("tasks.active")}
            </label>
          </li>
        );
      })}
    </ul>
  );
}

function TriggerEditor({ value, onChange }: { value: Trigger; onChange: (t: Trigger) => void }) {
  return (
    <div className="flex flex-col gap-2">
      <select
        aria-label={t("tasks.trigger")}
        value={value.type}
        onChange={(e) => onChange(defaultTrigger(e.target.value as Trigger["type"]))}
        className={field}
      >
        {TRIGGER_TYPES.map((tt) => (
          <option key={tt} value={tt}>
            {TRIGGER_LABEL[tt]}
          </option>
        ))}
      </select>
      {value.type === "interval" && (
        <label className="flex items-center gap-2">
          {t("tasks.every")}
          <input
            type="number"
            min={1}
            value={value.minutes}
            onChange={(e) => onChange({ type: "interval", minutes: Math.max(1, Number(e.target.value) || 1) })}
            className={`${field} w-20`}
          />
          {t("tasks.minutes")}
        </label>
      )}
      {value.type === "schedule" && (
        <div className="flex flex-wrap items-center gap-1.5">
          {weekdayShort().map((d, i) => {
            const n = i + 1;
            const on = value.weekdays.includes(n);
            return (
              <button
                key={d}
                type="button"
                aria-pressed={on}
                onClick={() =>
                  onChange({
                    ...value,
                    weekdays: on ? value.weekdays.filter((x) => x !== n) : [...value.weekdays, n].sort(),
                  })
                }
                className={`rounded px-1.5 py-0.5 text-[11px] ${
                  on ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]" : "border border-[var(--color-border)]"
                }`}
              >
                {d}
              </button>
            );
          })}
          <input
            type="time"
            aria-label={t("tasks.time")}
            value={`${String(value.hour).padStart(2, "0")}:${String(value.minute).padStart(2, "0")}`}
            onChange={(e) => {
              const [h, m] = e.target.value.split(":").map(Number);
              if (Number.isFinite(h) && Number.isFinite(m)) onChange({ ...value, hour: h, minute: m });
            }}
            className={`${field} ml-1`}
          />
        </div>
      )}
      {value.type === "folder" && (
        <input
          value={value.path}
          spellCheck={false}
          placeholder="/Users/…/Downloads"
          onChange={(e) => onChange({ type: "folder", path: e.target.value })}
          className={`${field} font-mono`}
        />
      )}
      <p className="text-[11px] text-[var(--color-muted)]">{triggerSummary(value)}</p>
    </div>
  );
}

function ScriptDiff({ before, after }: { before: string; after: string }) {
  const d = useMemo(() => lineDiff(before, after), [before, after]);
  const st = diffStats(d);
  if (st.added === 0 && st.removed === 0) return null;
  return (
    <details className="rounded-md border border-[var(--color-border)]">
      <summary className="cursor-pointer px-2 py-1 text-[11px]">
        {t("tasks.changes_vs_approved")} <span className="text-emerald-500">+{st.added}</span>{" "}
        <span className="text-red-500">−{st.removed}</span>
      </summary>
      <pre className="max-h-48 overflow-auto px-2 pb-2 font-mono text-[11px] leading-snug">
        {d.map((l, i) => (
          <div
            key={i}
            className={l.kind === "add" ? "bg-emerald-500/10 text-emerald-500" : l.kind === "del" ? "bg-red-500/10 text-red-500" : "opacity-60"}
          >
            {l.kind === "add" ? "+ " : l.kind === "del" ? "− " : "  "}
            {l.text}
          </div>
        ))}
      </pre>
    </details>
  );
}

function RiskHints({ script }: { script: string }) {
  const hints = riskHints(script);
  if (hints.length === 0) return null;
  return (
    <div className="flex items-start gap-1.5 rounded-md border border-amber-500/40 bg-amber-500/10 px-2 py-1.5 text-[11px]">
      <AlertTriangle size={12} className="mt-0.5 shrink-0 text-amber-500" />
      <span>{t("tasks.risk_hint", { list: hints.join(", ") })}</span>
    </div>
  );
}

function Editor({
  s,
  providers,
  languages,
  onSaved,
}: {
  s: Session;
  providers: AiProviderStatus[];
  languages: AiTasksState["languages"];
  onSaved: (t: AiTask) => void;
}) {
  const d = s.draft!;
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const descRef = useRef<HTMLTextAreaElement>(null);
  const set = (p: Partial<AiTaskDraft>) => patch({ draft: { ...d, ...p } });
  const hasScript = d.script.trim() !== "";
  const problem = draftProblem(d);
  const changed = scriptChanged(d, s.base);
  // Saving an unchanged, approved script keeps the approval (the backend
  // compares hashes); anything else needs a fresh one.
  const keepsApproval = !!s.base?.approved && !changed;

  useEffect(() => {
    if (!hasScript) descRef.current?.focus();
    // only on entering the editor
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const save = async (approve: boolean) => {
    setSaving(true);
    setSaveError(null);
    try {
      let saved = await aiTaskSave(d);
      if (approve && !saved.approved) saved = await aiTaskApprove(saved.id, saved.script_hash);
      onSaved(saved);
    } catch (e) {
      setSaveError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex flex-col gap-3 p-3">
      <label className="flex flex-col gap-1">
        <span className="font-medium">{t("tasks.what_should_happen")}</span>
        <textarea
          ref={descRef}
          rows={3}
          value={d.prompt}
          placeholder={t("tasks.e_g_move_dmg_files")}
          onChange={(e) => set({ prompt: e.target.value })}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              void generate(hasScript ? "" : null);
            }
          }}
          className={`${field} resize-y`}
        />
      </label>
      <div className="flex items-center gap-2">
        <select
          aria-label="KI-Anbieter"
          value={d.provider}
          onChange={(e) => set({ provider: e.target.value as AiProviderId })}
          className={field}
        >
          {providers.map((p) => (
            <option key={p.id} value={p.id} disabled={!p.configured}>
              {p.label}
              {p.configured ? "" : t("tasks.not_set_up")}
            </option>
          ))}
        </select>
        <button className={primary} disabled={s.generating || !d.prompt.trim()} onClick={() => void generate(null)}>
          {s.generating ? <Loader2 size={11} className="animate-spin" /> : <Sparkles size={11} />}
          {hasScript ? t("tasks.generate_again") : t("tasks.generate_script")}
        </button>
        <span className="text-[10px] text-[var(--color-muted)]">⌘⏎</span>
      </div>
      {s.generating && (
        <p className="text-[11px] text-[var(--color-muted)]">
          {t("tasks.the_ai_is_writing_the")}
        </p>
      )}
      {s.genError && <p className="rounded-md bg-red-500/10 px-2 py-1.5 text-[11px] text-red-500">{s.genError}</p>}

      {hasScript && (
        <>
          <div className="flex items-center gap-2">
            <input
              aria-label={t("tasks.name")}
              value={d.name}
              placeholder={t("tasks.name")}
              onChange={(e) => set({ name: e.target.value })}
              className={`${field} min-w-0 flex-1 font-medium`}
            />
            <select
              aria-label={t("tasks.language")}
              value={d.language}
              onChange={(e) => set({ language: e.target.value as AiTaskDraft["language"] })}
              className={field}
            >
              {languages.map((l) => (
                <option key={l} value={l}>
                  {LANGUAGE_LABEL[l]}
                </option>
              ))}
            </select>
          </div>
          {d.explanation && <p className="text-[11px] leading-snug text-[var(--color-muted)]">{d.explanation}</p>}
          <RiskHints script={d.script} />
          <textarea
            aria-label={t("tasks.script")}
            value={d.script}
            spellCheck={false}
            rows={Math.min(18, Math.max(6, d.script.split("\n").length + 1))}
            onChange={(e) => set({ script: e.target.value })}
            className={`${field} resize-y font-mono text-[11px] leading-snug`}
          />
          {s.base && changed && <ScriptDiff before={s.base.script} after={d.script} />}

          <div className="flex items-center gap-2">
            <input
              value={s.feedback}
              placeholder={t("tasks.change_request_for_the_ai")}
              onChange={(e) => patch({ feedback: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Enter" && s.feedback.trim()) void generate(s.feedback);
              }}
              className={`${field} min-w-0 flex-1`}
            />
            <button className={btn} disabled={s.generating || !s.feedback.trim()} onClick={() => void generate(s.feedback)}>
              <RefreshCw size={11} /> {t("tasks.revise")}
            </button>
          </div>

          <div className="rounded-md border border-[var(--color-border)] p-2.5">
            <div className="mb-1.5 font-medium">{t("tasks.when_to_run")}</div>
            <TriggerEditor value={d.trigger} onChange={(trigger) => set({ trigger })} />
            <label className="mt-2 flex items-center gap-2 text-[11px] text-[var(--color-muted)]">
              {t("tasks.time_limit")}
              <input
                type="number"
                min={5}
                max={MAX_TIMEOUT_S}
                value={d.timeout_s}
                onChange={(e) => set({ timeout_s: Math.min(MAX_TIMEOUT_S, Math.max(5, Number(e.target.value) || 5)) })}
                className={`${field} w-20`}
              />
              {t("tasks.seconds")}
            </label>
          </div>

          {problem && <p className="text-[11px] text-amber-500">{problem}</p>}
          {saveError && <p className="text-[11px] text-red-500">{saveError}</p>}
          <div className="flex flex-wrap items-center gap-2">
            {!keepsApproval && (
              <button className={primary} disabled={!!problem || saving} onClick={() => void save(true)}>
                <ShieldCheck size={11} /> {t("tasks.save_approve")}
              </button>
            )}
            <button className={keepsApproval ? primary : btn} disabled={!!problem || saving} onClick={() => void save(false)}>
              <Check size={11} /> {keepsApproval ? t("common.save") : t("tasks.save_only_without_approval")}
            </button>
          </div>
          <p className="text-[10px] leading-snug text-[var(--color-muted)]">
            {t("tasks.approving_means_this_exact_version")}
          </p>
        </>
      )}
    </div>
  );
}

function RunRow({ run, now }: { run: AiTaskRun; now: number }) {
  const [open, setOpen] = useState(false);
  const st = runState(run);
  const color = st === "ok" ? "text-emerald-500" : "text-red-500";
  return (
    <li className="border-b border-[var(--color-border)] last:border-b-0">
      <button className="flex w-full items-center gap-1.5 px-2 py-1 text-left text-[11px]" onClick={() => setOpen(!open)}>
        {open ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
        <span className={color}>{runSummary(run)}</span>
        <span className="ml-auto shrink-0 text-[var(--color-muted)]">
          {run.trigger} · {relativeTime(run.started_ms, now)}
        </span>
      </button>
      {open && (
        <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words px-2 pb-2 font-mono text-[10.5px] leading-snug">
          {run.output || run.error || t("tasks.no_output")}
        </pre>
      )}
    </li>
  );
}

function Detail({
  task,
  running,
  nextDue,
  paused,
  now,
  refresh,
}: {
  task: AiTask | null;
  running: boolean;
  nextDue: number | null;
  paused: boolean;
  now: number;
  refresh: () => void;
}) {
  const [runs, setRuns] = useState<AiTaskRun[]>([]);
  const [busy, setBusy] = useState(false);
  const [confirmDel, setConfirmDel] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const id = task?.id ?? -1;
  const lastRunMs = task?.last_run?.started_ms ?? 0;

  useEffect(() => {
    if (id < 0) return;
    aiTaskRuns(id, 20)
      .then(setRuns)
      .catch(() => setRuns([]));
  }, [id, lastRunMs, running]);

  if (!task) return <p className="p-3 text-[var(--color-muted)]">{t("tasks.this_task_no_longer_exists")}</p>;

  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    setErr(null);
    try {
      await f();
      refresh();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-3 p-3">
      <div>
        <div className="flex items-center gap-2">
          <StatusDot task={task} running={running} />
          <span className="text-[13px] font-semibold">{task.name}</span>
          <span className="rounded bg-[var(--color-surface)] px-1.5 py-0.5 text-[10px]">{LANGUAGE_LABEL[task.language]}</span>
        </div>
        <p className="mt-1 text-[11px] text-[var(--color-muted)]">
          {triggerSummary(task.trigger)}
          {task.trigger.type === "folder" ? ` (${shortPath(task.trigger.path)})` : ""}
          {nextDue && task.approved && task.enabled && !paused ? ` · ${t("tasks.next_run", { when: relativeTime(nextDue, now) })}` : ""}
        </p>
      </div>

      {!task.approved ? (
        <div className="rounded-md border border-amber-500/40 bg-amber-500/10 p-2.5 text-[11px]">
          <p className="mb-2">
            {t("tasks.not_approved_yet_doesn_t")}
          </p>
          <RiskHints script={task.script} />
          <button
            className={`${primary} mt-2`}
            disabled={busy}
            onClick={() => void act(() => aiTaskApprove(task.id, task.script_hash))}
          >
            <ShieldCheck size={11} /> {t("tasks.approve_this_version")}
          </button>
        </div>
      ) : (
        <div className="flex items-center gap-1.5 text-[11px] text-emerald-500">
          <ShieldCheck size={12} /> {t("tasks.approved")}
          <button className="ml-1 text-[var(--color-muted)] underline" disabled={busy} onClick={() => void act(() => aiTaskRevoke(task.id))}>
            {t("tasks.withdraw_approval")}
          </button>
        </div>
      )}

      {task.explanation && <p className="text-[11px] leading-snug">{task.explanation}</p>}
      <details className="rounded-md border border-[var(--color-border)]" open={!task.approved}>
        <summary className="cursor-pointer px-2 py-1 text-[11px]">{tn("tasks.script_lines", task.script.split("\n").length)}</summary>
        <pre className="max-h-64 overflow-auto px-2 pb-2 font-mono text-[11px] leading-snug">{task.script}</pre>
      </details>
      {task.prompt && (
        <p className="text-[11px] text-[var(--color-muted)]">
          <span className="font-medium">{t("tasks.description")}</span> {task.prompt}
        </p>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <button className={primary} disabled={busy || running || !task.approved} onClick={() => void act(() => aiTaskRunNow(task.id))}>
          {running ? <Loader2 size={11} className="animate-spin" /> : <Play size={11} />} {t("tasks.run_now")}
        </button>
        <button
          className={btn}
          onClick={() => patch({ view: { kind: "edit" }, draft: draftFromTask(task), base: task, genError: null, feedback: "" })}
        >
          <Pencil size={11} /> {t("common.edit")}
        </button>
        <label className="flex items-center gap-1 text-[11px]">
          <input type="checkbox" checked={task.enabled} onChange={(e) => void act(() => aiTaskSetEnabled(task.id, e.target.checked))} />
          {t("tasks.run_automatically_2")}
        </label>
        <div className="ml-auto">
          {confirmDel ? (
            <span className="flex items-center gap-1 text-[11px]">
              {t("tasks.delete")}
              <button className="rounded bg-red-500 px-2 py-0.5 text-white" onClick={() =>
                  void act(async () => {
                    await aiTaskDelete(task.id);
                    patch({ view: { kind: "list" } });
                  })
                }>
                {t("common.yes")}
              </button>
              <button className="rounded border border-[var(--color-border)] px-2 py-0.5" onClick={() => setConfirmDel(false)}>
                {t("tasks.no")}
              </button>
            </span>
          ) : (
            <button className={btn} onClick={() => setConfirmDel(true)} title={t("tasks.delete_task")}>
              <Trash2 size={11} />
            </button>
          )}
        </div>
      </div>
      {err && <p className="text-[11px] text-red-500">{err}</p>}

      <div>
        <div className="mb-1 font-medium">{t("tasks.runs")}</div>
        {runs.length === 0 ? (
          <p className="text-[11px] text-[var(--color-muted)]">{t("tasks.none_yet")}</p>
        ) : (
          <ul className="rounded-md border border-[var(--color-border)]">
            {runs.map((r) => (
              <RunRow key={r.id} run={r} now={now} />
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
