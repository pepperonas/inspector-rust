import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { X } from "lucide-react";
import { EV_RECORD_SHORTCUTS, getRecordShortcuts, setRecordShortcuts } from "../../lib/ipc";
import {
  ACTIONS,
  ACTION_LABEL,
  DEFAULT_BINDINGS,
  FIXED_KEYS,
  bindingFromEvent,
  findConflict,
  formatBinding,
  mergeWithDefaults,
  storedDeviations,
  swapBinding,
  type ActionId,
  type Binding,
  type Bindings,
} from "../../lib/editor-shortcuts";
import { useT } from "../../lib/i18n";

/**
 * The editor key map from the `record.shortcuts` setting, kept in step across windows (the
 * editor and Settings save the same map). Until it loads — or if it can't — the defaults apply.
 */
export function useEditorShortcuts(): [Bindings, (next: Bindings) => void] {
  const [bindings, setBindings] = useState<Bindings>(DEFAULT_BINDINGS);
  useEffect(() => {
    let dead = false;
    getRecordShortcuts()
      .then((s) => !dead && setBindings(mergeWithDefaults(s)))
      .catch(() => undefined);
    const off = listen<unknown>(EV_RECORD_SHORTCUTS, (e) => setBindings(mergeWithDefaults(e.payload)));
    return () => {
      dead = true;
      void off.then((u) => u()).catch(() => undefined);
    };
  }, []);
  const save = useCallback((next: Bindings) => {
    setBindings(next);
    void setRecordShortcuts(storedDeviations(next))
      .then((s) => setBindings(mergeWithDefaults(s)))
      .catch(() => undefined);
  }, []);
  return [bindings, save];
}

/** How a binding reads in the editor (tooltips, lane hints, the dialog). */
export function useBindingLabel(): (b: Binding) => string {
  const t = useT();
  return useCallback((b: Binding) => formatBinding(b, t("rec4.keys.space")), [t]);
}

type Pending = { action: ActionId; binding: Binding; other: ActionId } | { action: ActionId; binding: Binding; fixed: true };

/**
 * Every editor action with its key. A click on a key listens for the next one; a fixed key is
 * refused with a note, another action's key offers a swap.
 */
export function ShortcutsDialog({
  bindings,
  onSave,
  onClose,
}: {
  bindings: Bindings;
  onSave: (next: Bindings) => void;
  onClose: () => void;
}) {
  const t = useT();
  const label = useBindingLabel();
  const [recording, setRecording] = useState<ActionId | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);

  // Capture phase: the dialog owns the keyboard while open — no editor key, and Escape never
  // reaches the popup's close-on-Esc fallback.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (recording) {
        e.preventDefault();
        e.stopPropagation();
        if (e.key === "Escape") {
          setRecording(null);
          return;
        }
        const b = bindingFromEvent(e);
        if (!b) return;
        setRecording(null);
        const c = findConflict(bindings, recording, b);
        if (!c) {
          setPending(null);
          onSave({ ...bindings, [recording]: b });
        } else if (c.kind === "fixed") {
          setPending({ action: recording, binding: b, fixed: true });
        } else {
          setPending({ action: recording, binding: b, other: c.action });
        }
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        if (pending) setPending(null);
        else onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [recording, pending, bindings, onSave, onClose]);

  const keyText = (a: ActionId) => {
    const b = bindings[a];
    // Delete works like Backspace wherever Backspace is bound.
    return b.key === "Backspace" && !b.ctrl && !b.alt && !b.shift ? "⌫ / ⌦" : label(b);
  };
  const kbd = "rounded border border-[var(--color-border)] px-1.5 py-0.5 font-mono text-[11px] tabular-nums";

  return (
    <div className="md3-overlay-in fixed inset-0 z-30 flex items-center justify-center bg-black/50" onPointerDown={onClose}>
      <div
        role="dialog"
        aria-label={t("rec4.keys.title")}
        data-testid="ed-shortcuts"
        className="md3-pop-in flex max-h-[90vh] w-[420px] flex-col rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 text-[var(--color-fg)] shadow-2xl"
        onPointerDown={(e) => e.stopPropagation()}
      >
        <div className="mb-1 flex items-center justify-between">
          <h2 className="text-[14px] font-semibold">{t("rec4.keys.title")}</h2>
          <button onClick={onClose} aria-label={t("rec2.ed.close")} className="rounded p-1 hover:bg-[var(--color-bg)]">
            <X size={14} />
          </button>
        </div>
        <p className="mb-3 text-[11px] text-[var(--color-muted)]">{t("rec4.keys.edit_hint")}</p>
        <div className="min-h-0 overflow-y-auto">
          <ul className="flex flex-col gap-1">
            {ACTIONS.map((a) => (
              <li key={a} className="flex items-center justify-between gap-3 text-[12px]" data-action={a}>
                <span>{t(ACTION_LABEL[a])}</span>
                <button
                  data-testid={`ed-key-${a}`}
                  onClick={() => {
                    setPending(null);
                    setRecording(a);
                  }}
                  aria-pressed={recording === a}
                  className={
                    kbd +
                    " hover:bg-[var(--color-bg)]" +
                    (recording === a ? " border-[var(--color-accent)] text-[var(--color-accent)]" : "")
                  }
                >
                  {recording === a ? t("rec4.keys.press") : keyText(a)}
                </button>
              </li>
            ))}
          </ul>
          {pending && (
            <div role="alert" className="mt-3 rounded-lg border border-[var(--color-border)] p-2 text-[12px]" data-testid="ed-key-conflict">
              <p>
                {"fixed" in pending
                  ? t("rec4.keys.conflict_fixed", { key: label(pending.binding) })
                  : t("rec4.keys.conflict_action", { key: label(pending.binding), action: t(ACTION_LABEL[pending.other]) })}
              </p>
              <div className="mt-2 flex justify-end gap-2">
                {!("fixed" in pending) && (
                  <button
                    data-testid="ed-key-swap"
                    className="rounded-lg bg-[var(--color-accent)] px-2.5 py-1 text-[12px] font-medium text-[var(--color-accent-fg)]"
                    onClick={() => {
                      onSave(swapBinding(bindings, pending.action, pending.binding));
                      setPending(null);
                    }}
                  >
                    {t("rec4.keys.swap")}
                  </button>
                )}
                <button
                  data-testid="ed-key-cancel"
                  className="rounded-lg border border-[var(--color-border)] px-2.5 py-1 text-[12px] hover:bg-[var(--color-bg)]"
                  onClick={() => setPending(null)}
                >
                  {t("common.cancel")}
                </button>
              </div>
            </div>
          )}
          <h3 className="mb-1 mt-4 text-[11px] font-medium uppercase tracking-wide text-[var(--color-muted)]">
            {t("rec4.keys.fixed_heading")}
          </h3>
          <ul className="flex flex-col gap-1">
            {FIXED_KEYS.map((f) => (
              <li key={f.label} className="flex items-center justify-between gap-3 text-[12px] text-[var(--color-muted)]">
                <span>{t(f.label)}</span>
                <span className={kbd}>{f.keys.map(label).join(" ")}</span>
              </li>
            ))}
          </ul>
        </div>
        <div className="mt-3 flex justify-end">
          <button
            data-testid="ed-keys-reset"
            className="rounded-lg border border-[var(--color-border)] px-2.5 py-1 text-[12px] hover:bg-[var(--color-bg)]"
            onClick={() => {
              setPending(null);
              setRecording(null);
              onSave(DEFAULT_BINDINGS);
            }}
          >
            {t("rec4.keys.reset_all")}
          </button>
        </div>
      </div>
    </div>
  );
}
