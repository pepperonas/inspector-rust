import { ACTION_TYPES, emptyAction, type BindingAction, type BindingActionType } from "../lib/gesture-bindings";
import { HotkeyCapture } from "./HotkeyCapture";
import { useT } from "../lib/i18n";

const input =
  "rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px] text-[var(--color-fg)]";

export interface ActionChoices {
  hotkeys: { id: string; label: string }[];
  tasks: { id: number; name: string; approved: boolean }[];
}

/**
 * One action, or none: the action types of the gesture bindings plus a
 * "nothing" entry whose wording the caller picks. A type that needs a target
 * (shortcut, address, task) shows its field next to the select.
 */
export function ActionPicker({
  value,
  onChange,
  noneLabel,
  label,
  choices,
}: {
  value: BindingAction | null;
  onChange: (next: BindingAction | null) => void;
  noneLabel: string;
  /** Accessible name of the select. */
  label: string;
  choices: ActionChoices;
}) {
  const t = useT();
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-1.5">
      <select
        aria-label={label}
        value={value?.type ?? ""}
        onChange={(e) => onChange(e.target.value ? emptyAction(e.target.value as BindingActionType) : null)}
        className={input}
      >
        <option value="">{noneLabel}</option>
        {ACTION_TYPES.map((at) => (
          <option key={at.type} value={at.type}>
            {at.label}
          </option>
        ))}
      </select>
      {value?.type === "hotkey" && (
        <select
          aria-label={t("gesture.action.hotkey")}
          value={value.action}
          onChange={(e) => onChange({ type: "hotkey", action: e.target.value })}
          className={input}
        >
          <option value="">{t("common.choose_dash")}</option>
          {choices.hotkeys.map((h) => (
            <option key={h.id} value={h.id}>
              {h.label}
            </option>
          ))}
        </select>
      )}
      {value?.type === "shortcut" && (
        <HotkeyCapture value={value.keys} onChange={(keys) => onChange({ type: "shortcut", keys })} />
      )}
      {value?.type === "open" && (
        <input
          aria-label={t("gesture.field.target")}
          value={value.target}
          spellCheck={false}
          placeholder={t("gesture.field.target_placeholder")}
          onChange={(e) => onChange({ type: "open", target: e.target.value })}
          className={`${input} min-w-0 flex-1 font-mono`}
        />
      )}
      {value?.type === "task" && (
        <select
          aria-label={t("gesture.field.task")}
          value={value.id}
          onChange={(e) => onChange({ type: "task", id: Number(e.target.value) })}
          className={input}
        >
          <option value={0}>{t("common.choose_dash")}</option>
          {choices.tasks.map((task) => (
            <option key={task.id} value={task.id} disabled={!task.approved}>
              {task.name}
              {task.approved ? "" : ` ${t("gesture.task_unapproved")}`}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}
