import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, Check, Hand, Keyboard, Pencil, Plus, RotateCcw, Trash2, X } from "lucide-react";
import {
  aiTasksState,
  appBundleId,
  gestureBindingsGet,
  gestureBindingsReset,
  gestureBindingsSet,
  gestureCapture,
  gestureLive,
  listActionHotkeys,
  listApps,
  type AppEntry,
} from "../lib/ipc";
import {
  ACTION_TYPES,
  MAX_FINGERS,
  MIN_FINGERS,
  TRIGGER_KINDS,
  actionLabel,
  captureFromLog,
  conflictingIds,
  defaultTypingGuard,
  emptyAction,
  isTipTap,
  latestSeq,
  mayCollideWithSystem,
  missingField,
  newBinding,
  tipTapHint,
  triggerLabel,
  type ActionContext,
  type BindingActionType,
  type GestureBinding,
} from "../lib/gesture-bindings";
import { HotkeyCapture } from "./HotkeyCapture";

/** How often "show the gesture" polls the live log. */
const CAPTURE_POLL_MS = 150;
/** Mirrors `CAPTURE_MAX_MS` in Rust — the backend ends the window itself. */
const CAPTURE_MAX_MS = 15_000;

const input =
  "rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px] text-[var(--color-fg)]";

/**
 * Settings → Touchpad gestures → the binding table: pick a gesture, pick
 * what it does, change or delete it any time. Saving swaps the list into
 * the running capture — no restart.
 */
export function GestureBindingsSection() {
  const [list, setList] = useState<GestureBinding[] | null>(null);
  const [customised, setCustomised] = useState(false);
  const [ctx, setCtx] = useState<ActionContext>({});
  const [hotkeys, setHotkeys] = useState<{ id: string; label: string }[]>([]);
  const [tasks, setTasks] = useState<{ id: number; name: string; approved: boolean }[]>([]);
  /** Index being edited; `list.length` = a new row. */
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState<GestureBinding | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [armedDelete, setArmedDelete] = useState<string | null>(null);
  const [armedReset, setArmedReset] = useState(false);

  useEffect(() => {
    gestureBindingsGet()
      .then((v) => {
        setList(v.bindings);
        setCustomised(v.customised);
      })
      .catch((e) => setError(String(e)));
    listActionHotkeys()
      .then((h) => {
        const usable = h.filter((x) => x.id !== "gestureunintended");
        setHotkeys(usable.map((x) => ({ id: x.id, label: x.label })));
        setCtx((c) => ({ ...c, hotkeys: Object.fromEntries(usable.map((x) => [x.id, x.label])) }));
      })
      .catch(() => {});
    aiTasksState()
      .then((s) => {
        setTasks(s.tasks.map((t) => ({ id: t.id, name: t.name, approved: t.approved })));
        setCtx((c) => ({ ...c, tasks: Object.fromEntries(s.tasks.map((t) => [t.id, t.name])) }));
      })
      .catch(() => {});
  }, []);

  const conflicts = useMemo(() => conflictingIds(list ?? []), [list]);

  const persist = async (next: GestureBinding[]): Promise<boolean> => {
    setBusy(true);
    setError(null);
    try {
      const v = await gestureBindingsSet(next);
      setList(v.bindings);
      setCustomised(v.customised);
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const startEdit = (i: number) => {
    if (!list) return;
    setEditing(i);
    setDraft(i < list.length ? structuredClone(list[i]) : newBinding());
    setError(null);
  };

  const cancelEdit = () => {
    setEditing(null);
    setDraft(null);
  };

  const saveDraft = async () => {
    if (!list || !draft || editing === null) return;
    const next = editing < list.length ? list.map((b, i) => (i === editing ? draft : b)) : [...list, draft];
    if (await persist(next)) cancelEdit();
  };

  const toggle = (i: number) => {
    if (!list) return;
    void persist(list.map((b, j) => (j === i ? { ...b, enabled: !b.enabled } : b)));
  };

  const remove = (id: string) => {
    if (!list) return;
    if (armedDelete !== id) {
      setArmedDelete(id);
      return;
    }
    setArmedDelete(null);
    void persist(list.filter((b) => b.id !== id));
  };

  const reset = async () => {
    if (!armedReset) {
      setArmedReset(true);
      return;
    }
    setArmedReset(false);
    setBusy(true);
    try {
      const v = await gestureBindingsReset();
      setList(v.bindings);
      setCustomised(v.customised);
      cancelEdit();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (!list) {
    return <p className="text-[12px] text-[var(--color-muted)]">{error ?? "Lädt …"}</p>;
  }

  return (
    <div data-testid="gesture-bindings">
      <ul className="flex flex-col gap-1.5">
        {list.map((b, i) => (
          <li
            key={b.id}
            className={`flex items-center gap-2 rounded-md border px-2.5 py-1.5 text-[12px] ${
              conflicts.has(b.id) ? "border-amber-500/60" : "border-[var(--color-border)]"
            } ${b.enabled ? "" : "opacity-60"}`}
          >
            <input
              type="checkbox"
              aria-label={`${triggerLabel(b.trigger)} aktiv`}
              checked={b.enabled}
              disabled={busy}
              onChange={() => toggle(i)}
              className="accent-[var(--color-accent)]"
            />
            <span className="min-w-0 flex-1">
              <span className="font-medium">{triggerLabel(b.trigger)}</span>
              <span className="text-[var(--color-muted)]"> → </span>
              <span>{actionLabel(b.action, ctx)}</span>
              {b.app && (
                <span className="ml-1.5 rounded bg-[var(--color-surface)] px-1.5 py-0.5 text-[10px] text-[var(--color-muted)]">
                  nur {b.app_name ?? b.app}
                </span>
              )}
              {!b.typing_guard && (
                <span
                  title="Wirkt auch direkt nach dem Tippen"
                  className="ml-1.5 inline-flex items-center gap-0.5 text-[10px] text-[var(--color-muted)]"
                >
                  <Keyboard size={10} /> ohne Tipp-Schutz
                </span>
              )}
            </span>
            <button
              type="button"
              title="Bearbeiten"
              aria-label="Bearbeiten"
              disabled={busy}
              onClick={() => startEdit(i)}
              className="md3-press rounded p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            >
              <Pencil size={12} />
            </button>
            <button
              type="button"
              title={armedDelete === b.id ? "Wirklich löschen?" : "Löschen"}
              aria-label={armedDelete === b.id ? "Wirklich löschen" : "Löschen"}
              disabled={busy}
              onClick={() => remove(b.id)}
              onBlur={() => setArmedDelete((a) => (a === b.id ? null : a))}
              className={`md3-press rounded p-1 ${
                armedDelete === b.id ? "bg-red-500/15 text-red-500" : "text-[var(--color-muted)] hover:text-red-500"
              }`}
            >
              {armedDelete === b.id ? <Check size={12} /> : <Trash2 size={12} />}
            </button>
          </li>
        ))}
        {list.length === 0 && (
          <li className="text-[12px] text-[var(--color-muted)]">Keine Zuordnungen — Gesten tun gerade nichts.</li>
        )}
      </ul>

      {conflicts.size > 0 && (
        <p className="mt-2 flex items-start gap-1 text-[11px] text-amber-500">
          <AlertTriangle size={11} className="mt-0.5 shrink-0" />
          Zwei aktive Zuordnungen teilen sich eine Geste — beim Speichern wird das abgelehnt.
        </p>
      )}

      {editing !== null && draft && (
        <BindingEditor
          draft={draft}
          onChange={setDraft}
          hotkeys={hotkeys}
          tasks={tasks}
          busy={busy}
          onSave={() => void saveDraft()}
          onCancel={cancelEdit}
        />
      )}

      {error && <p className="mt-2 text-[11px] text-red-500">{error}</p>}

      {editing === null && (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={() => startEdit(list.length)}
            className="md3-press flex items-center gap-1 rounded-md bg-[var(--color-accent)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-accent-fg)] disabled:opacity-40"
          >
            <Plus size={11} /> Neue Zuordnung
          </button>
          {customised && (
            <button
              type="button"
              disabled={busy}
              onClick={() => void reset()}
              onBlur={() => setArmedReset(false)}
              className={`md3-press flex items-center gap-1 rounded-md border px-2.5 py-1 text-[11px] ${
                armedReset ? "border-red-500 text-red-500" : "border-[var(--color-border)] text-[var(--color-muted)]"
              }`}
            >
              <RotateCcw size={11} /> {armedReset ? "Wirklich alle eigenen Zuordnungen verwerfen?" : "Standard wiederherstellen"}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

interface EditorProps {
  draft: GestureBinding;
  onChange: (b: GestureBinding) => void;
  hotkeys: { id: string; label: string }[];
  tasks: { id: number; name: string; approved: boolean }[];
  busy: boolean;
  onSave: () => void;
  onCancel: () => void;
}

function BindingEditor({ draft, onChange, hotkeys, tasks, busy, onSave, onCancel }: EditorProps) {
  const a = draft.action;
  const missing = missingField(draft);
  const set = (patch: Partial<GestureBinding>) => onChange({ ...draft, ...patch });

  const setActionType = (type: BindingActionType) => {
    const action = emptyAction(type);
    onChange({ ...draft, action, typing_guard: defaultTypingGuard(action) });
  };

  return (
    <div className="mt-3 rounded-md border border-[var(--color-accent)]/40 p-3 text-[12px]" data-testid="gesture-binding-editor">
      <div className="mb-1 font-medium">Geste</div>
      <div className="mb-1 flex flex-wrap items-center gap-2">
        <select
          aria-label="Geste"
          value={draft.trigger.kind}
          onChange={(e) => {
            const kind = e.target.value as GestureBinding["trigger"]["kind"];
            set({ trigger: { kind, fingers: isTipTap(kind) ? 2 : Math.max(draft.trigger.fingers, MIN_FINGERS) } });
          }}
          className={input}
        >
          {TRIGGER_KINDS.map((k) => (
            <option key={k.kind} value={k.kind}>
              {k.label}
            </option>
          ))}
        </select>
        {!isTipTap(draft.trigger.kind) && (
          <select
            aria-label="Finger"
            value={draft.trigger.fingers}
            onChange={(e) => set({ trigger: { ...draft.trigger, fingers: Number(e.target.value) } })}
            className={input}
          >
            {Array.from({ length: MAX_FINGERS - MIN_FINGERS + 1 }, (_, i) => MIN_FINGERS + i).map((n) => (
              <option key={n} value={n}>
                {n} Finger
              </option>
            ))}
          </select>
        )}
        <CaptureButton onCaptured={(trigger) => set({ trigger })} />
      </div>
      {tipTapHint(draft.trigger.kind) && (
        <p className="mb-1 text-[11px] text-[var(--color-muted)]">{tipTapHint(draft.trigger.kind)}</p>
      )}
      {mayCollideWithSystem(draft.trigger) && (
        <p className="mb-1 text-[11px] text-[var(--color-muted)]">
          Belegt das System dieselbe Geste (Systemeinstellungen → Trackpad → Weitere Gesten), lösen beide aus — dort
          abschalten.
        </p>
      )}

      <div className="mb-1 mt-3 font-medium">Aktion</div>
      <div className="flex flex-wrap items-center gap-2">
        <select
          aria-label="Aktion"
          value={a.type}
          onChange={(e) => setActionType(e.target.value as BindingActionType)}
          className={input}
        >
          {ACTION_TYPES.map((t) => (
            <option key={t.type} value={t.type}>
              {t.label}
            </option>
          ))}
        </select>
        {a.type === "hotkey" && (
          <select
            aria-label="Inspector-Rust-Aktion"
            value={a.action}
            onChange={(e) => set({ action: { type: "hotkey", action: e.target.value } })}
            className={input}
          >
            <option value="">— auswählen —</option>
            {hotkeys.map((h) => (
              <option key={h.id} value={h.id}>
                {h.label}
              </option>
            ))}
          </select>
        )}
        {a.type === "shortcut" && (
          <HotkeyCapture value={a.keys} onChange={(keys) => set({ action: { type: "shortcut", keys } })} />
        )}
        {a.type === "open" && (
          <input
            aria-label="Adresse oder Pfad"
            value={a.target}
            spellCheck={false}
            placeholder="https://… oder /Applications/…"
            onChange={(e) => set({ action: { type: "open", target: e.target.value } })}
            className={`${input} min-w-0 flex-1 font-mono`}
          />
        )}
        {a.type === "task" && (
          <select
            aria-label="KI-Task"
            value={a.id}
            onChange={(e) => set({ action: { type: "task", id: Number(e.target.value) } })}
            className={input}
          >
            <option value={0}>— auswählen —</option>
            {tasks.map((t) => (
              <option key={t.id} value={t.id} disabled={!t.approved}>
                {t.name}
                {t.approved ? "" : " (nicht freigegeben)"}
              </option>
            ))}
          </select>
        )}
      </div>
      {a.type === "task" && tasks.length === 0 && (
        <p className="mt-1 text-[11px] text-[var(--color-muted)]">
          Noch keine KI-Tasks — mit <code>task</code> in der Suche anlegen.
        </p>
      )}

      <div className="mb-1 mt-3 font-medium">Wo</div>
      <AppScope
        app={draft.app}
        appName={draft.app_name}
        onChange={(app, app_name) => set({ app, app_name })}
      />

      <label className="mt-3 flex cursor-pointer items-center gap-2">
        <input
          type="checkbox"
          checked={draft.typing_guard}
          onChange={(e) => set({ typing_guard: e.target.checked })}
          className="accent-[var(--color-accent)]"
        />
        <span className="text-[var(--color-muted)]">Tipp-Schutz: direkt nach dem Tippen ignorieren</span>
      </label>

      <div className="mt-3 flex items-center gap-2">
        <button
          type="button"
          disabled={busy || missing !== null}
          onClick={onSave}
          className="md3-press flex items-center gap-1 rounded-md bg-[var(--color-accent)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-accent-fg)] disabled:opacity-40"
        >
          <Check size={11} /> Speichern
        </button>
        <button
          type="button"
          onClick={onCancel}
          className="md3-press flex items-center gap-1 rounded-md border border-[var(--color-border)] px-2.5 py-1 text-[11px]"
        >
          <X size={11} /> Abbrechen
        </button>
        {missing && <span className="text-[11px] text-[var(--color-muted)]">{missing}</span>}
      </div>
    </div>
  );
}

/** "Show the gesture": hold dispatch off, wait for the next recognised
 *  gesture in the live log, take its kind and finger count. */
function CaptureButton({ onCaptured }: { onCaptured: (t: GestureBinding["trigger"]) => void }) {
  const [state, setState] = useState<"idle" | "waiting" | string>("idle");
  const timer = useRef<number | null>(null);

  const stop = useCallback(() => {
    if (timer.current !== null) window.clearInterval(timer.current);
    timer.current = null;
    void gestureCapture(false).catch(() => {});
  }, []);

  useEffect(() => stop, [stop]);

  const start = async () => {
    setState("waiting");
    try {
      const since = latestSeq((await gestureLive()).log);
      await gestureCapture(true);
      const began = Date.now();
      timer.current = window.setInterval(() => {
        void gestureLive().then((snap) => {
          const r = captureFromLog(snap.log, since);
          if (r && "trigger" in r) {
            stop();
            setState("idle");
            onCaptured(r.trigger);
          } else if (r) {
            stop();
            setState(r.error);
          } else if (Date.now() - began > CAPTURE_MAX_MS) {
            stop();
            setState("Keine Geste erkannt. Sind Gesten eingeschaltet?");
          }
        });
      }, CAPTURE_POLL_MS);
    } catch (e) {
      stop();
      setState(String(e));
    }
  };

  return (
    <span className="flex items-center gap-2">
      <button
        type="button"
        onClick={() => (state === "waiting" ? (stop(), setState("idle")) : void start())}
        className={`md3-press flex items-center gap-1 rounded-md border px-2 py-1 text-[11px] ${
          state === "waiting"
            ? "border-[var(--color-accent)] text-[var(--color-accent)]"
            : "border-[var(--color-border)]"
        }`}
      >
        <Hand size={11} className={state === "waiting" ? "animate-pulse motion-reduce:animate-none" : ""} />
        {state === "waiting" ? "Jetzt auf dem Trackpad ausführen …" : "Geste vormachen"}
      </button>
      {state !== "idle" && state !== "waiting" && <span className="text-[11px] text-amber-500">{state}</span>}
    </span>
  );
}

/** Everywhere, or only while one app is in front. */
function AppScope({
  app,
  appName,
  onChange,
}: {
  app: string | null;
  appName: string | null;
  onChange: (app: string | null, name: string | null) => void;
}) {
  const [apps, setApps] = useState<AppEntry[] | null>(null);
  const [query, setQuery] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const scoped = app !== null;

  useEffect(() => {
    if (scoped && apps === null) listApps().then(setApps).catch(() => setApps([]));
  }, [scoped, apps]);

  const pick = async (entry: AppEntry) => {
    setErr(null);
    const id = await appBundleId(entry.path).catch(() => null);
    if (!id) {
      setErr(`Kennung von ${entry.name} nicht lesbar.`);
      return;
    }
    onChange(id, entry.name);
    setQuery("");
  };

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!apps || !q) return [];
    return apps.filter((a) => a.name_lower.includes(q)).slice(0, 6);
  }, [apps, query]);

  return (
    <div>
      <div className="flex items-center gap-3">
        <label className="flex cursor-pointer items-center gap-1.5">
          <input type="radio" checked={!scoped} onChange={() => onChange(null, null)} className="accent-[var(--color-accent)]" />
          Überall
        </label>
        <label className="flex cursor-pointer items-center gap-1.5">
          <input
            type="radio"
            checked={scoped}
            onChange={() => onChange(app ?? "", appName)}
            className="accent-[var(--color-accent)]"
          />
          Nur in einer App
        </label>
      </div>
      {scoped && (
        <div className="mt-1.5">
          {app ? (
            <span className="text-[var(--color-muted)]">
              {appName ?? app} <span className="font-mono text-[10px]">({app})</span>
            </span>
          ) : (
            <span className="text-[11px] text-[var(--color-muted)]">App suchen und auswählen:</span>
          )}
          <input
            aria-label="App suchen"
            value={query}
            placeholder="App suchen …"
            onChange={(e) => setQuery(e.target.value)}
            className={`${input} mt-1 block w-full`}
          />
          {matches.length > 0 && (
            <ul className="mt-1 rounded border border-[var(--color-border)]">
              {matches.map((m) => (
                <li key={m.path}>
                  <button
                    type="button"
                    onClick={() => void pick(m)}
                    className="w-full px-2 py-1 text-left hover:bg-[var(--color-surface)]"
                  >
                    {m.name}
                  </button>
                </li>
              ))}
            </ul>
          )}
          {err && <p className="mt-1 text-[11px] text-red-500">{err}</p>}
        </div>
      )}
    </div>
  );
}
