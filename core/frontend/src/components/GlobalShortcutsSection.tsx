import { useEffect, useState } from "react";
import { RotateCcw } from "lucide-react";
import { HotkeyCapture } from "./HotkeyCapture";
import { formatHotkey } from "../lib/platform";
import {
  listActionHotkeys,
  resetActionHotkey,
  setActionHotkey,
  type ActionHotkey,
} from "../lib/ipc";

/**
 * Settings → "Global shortcuts": rebind every global *action* hotkey (OCR,
 * screenshot, eyedropper, Finder selection, Markdown→PDF, screen recording,
 * audio swap, Timesheet). Each row is a {@link HotkeyCapture} that writes the
 * binding immediately via `set_action_hotkey`; the backend validates against
 * every other binding and rejects conflicts (surfaced inline). Reset returns
 * the row to its built-in default; "Off" disables it (empty binding).
 */
export function GlobalShortcutsSection() {
  const [rows, setRows] = useState<ActionHotkey[] | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);

  const reload = () => {
    listActionHotkeys()
      .then(setRows)
      .catch(() => setRows([]));
  };
  useEffect(reload, []);

  const apply = async (id: string, spec: string) => {
    setBusy(id);
    setErrors((e) => ({ ...e, [id]: "" }));
    try {
      await setActionHotkey(id, spec);
    } catch (e) {
      setErrors((er) => ({ ...er, [id]: String(e) }));
    } finally {
      // Always re-read so the field reflects the actual stored state (a
      // rejected bind keeps the previous value).
      const fresh = await listActionHotkeys().catch(() => null);
      if (fresh) setRows(fresh);
      setBusy(null);
    }
  };

  const reset = async (id: string) => {
    setBusy(id);
    setErrors((e) => ({ ...e, [id]: "" }));
    try {
      await resetActionHotkey(id);
    } finally {
      const fresh = await listActionHotkeys().catch(() => null);
      if (fresh) setRows(fresh);
      setBusy(null);
    }
  };

  if (!rows) {
    return <div className="text-[12px] text-[var(--color-muted)]">Loading…</div>;
  }

  return (
    <div className="flex flex-col gap-2.5">
      {rows.map((r) => (
        <div key={r.id} className="flex flex-col gap-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="w-[150px] shrink-0 text-[12px] text-[var(--color-fg)]">
              {r.label}
            </span>
            <HotkeyCapture
              value={r.shortcut}
              onChange={(spec) => void apply(r.id, spec)}
              disabled={busy === r.id}
            />
            <button
              type="button"
              onClick={() => void reset(r.id)}
              disabled={busy === r.id || r.is_default}
              title={`Reset to ${formatHotkey(r.default)}`}
              className="md3-press flex items-center gap-1 rounded border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-fg)] disabled:opacity-40"
            >
              <RotateCcw size={11} />
              Reset
            </button>
            {r.shortcut ? (
              <button
                type="button"
                onClick={() => void apply(r.id, "")}
                disabled={busy === r.id}
                title="Disable this shortcut"
                className="md3-press rounded border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-muted)] hover:border-rose-500 hover:text-rose-400 disabled:opacity-40"
              >
                Off
              </button>
            ) : (
              <span className="text-[11px] italic text-[var(--color-muted)]">disabled</span>
            )}
          </div>
          {errors[r.id] && (
            <span className="pl-[158px] text-[11px] text-rose-400">⚠ {errors[r.id]}</span>
          )}
        </div>
      ))}
    </div>
  );
}
