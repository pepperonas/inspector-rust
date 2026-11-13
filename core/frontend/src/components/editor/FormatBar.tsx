import { useEffect, useRef, useState } from "react";
import { Plus, X } from "lucide-react";
import type { RecordAspect } from "../../lib/ipc";
import { addable, formatLabel } from "../../lib/formats";
import { useT } from "../../lib/i18n";

export interface FormatBarProps {
  /** Every format of the project, in list order (Rust). */
  formats: readonly RecordAspect[];
  active: RecordAspect;
  disabled?: boolean;
  onSwitch: (a: RecordAspect) => void;
  onAdd: (a: RecordAspect) => void;
  onRemove: (a: RecordAspect) => void;
}

/**
 * The project's formats as chips: one project, several shapes (16:9 for YouTube, 9:16 for
 * Shorts/Reels/TikTok). Cuts, zooms and sound are shared; camera layout and caption place
 * belong to each format. "+" adds one, the × on the active chip removes it.
 */
export function FormatBar({ formats, active, disabled, onSwitch, onAdd, onRemove }: FormatBarProps) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [armed, setArmed] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const label = (a: RecordAspect) => formatLabel(a, t("rec13.format.auto"), t("rec13.format.source"));
  const more = addable(formats);

  useEffect(() => {
    if (!open) return;
    const close = (e: PointerEvent) => {
      if (!box.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("pointerdown", close);
    return () => window.removeEventListener("pointerdown", close);
  }, [open]);
  // The remove button asks twice; it lets go after a moment or when the format changes.
  useEffect(() => {
    if (!armed) return;
    const id = window.setTimeout(() => setArmed(false), 4000);
    return () => window.clearTimeout(id);
  }, [armed]);
  const [seen, setSeen] = useState(active);
  if (seen !== active) {
    setSeen(active);
    setArmed(false);
  }

  const chip = (on: boolean) =>
    "rounded-md border px-2 py-1 text-[11px] font-medium disabled:opacity-40 " +
    (on
      ? "border-[var(--color-accent)] bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
      : "border-[var(--color-border)] hover:bg-[var(--color-bg)]");

  return (
    <div ref={box} className="relative flex items-center gap-1" role="group" aria-label={t("rec13.format.title")} data-testid="ed-formats">
      {formats.map((a) => (
        <button
          key={a}
          type="button"
          aria-pressed={a === active}
          disabled={disabled}
          onClick={() => a !== active && onSwitch(a)}
          title={t("rec13.format.switch_hint")}
          className={chip(a === active)}
          data-testid={`ed-format-${a}`}
        >
          {label(a)}
        </button>
      ))}
      {formats.length > 1 && (
        <button
          type="button"
          disabled={disabled}
          onClick={() => {
            if (!armed) return setArmed(true);
            setArmed(false);
            onRemove(active);
          }}
          title={t("rec13.format.remove_hint", { format: label(active) })}
          aria-label={t("rec13.format.remove_hint", { format: label(active) })}
          className={
            "flex items-center gap-1 rounded-md border px-1.5 py-1 text-[11px] disabled:opacity-40 " +
            (armed ? "border-red-500 text-red-500" : "border-[var(--color-border)] hover:bg-[var(--color-bg)]")
          }
          data-testid="ed-format-remove"
        >
          <X size={12} />
          {armed && t("rec13.format.remove_confirm")}
        </button>
      )}
      {more.length > 0 && (
        <button
          type="button"
          disabled={disabled}
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
          title={t("rec13.format.add_hint")}
          aria-label={t("rec13.format.add")}
          className={chip(false) + " flex items-center gap-1"}
          data-testid="ed-format-add"
        >
          <Plus size={12} /> {formats.length === 1 && t("rec13.format.add")}
        </button>
      )}
      {open && (
        <div
          role="menu"
          className="pop-enter absolute left-0 top-full z-20 mt-1 flex flex-col rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-1 shadow-xl"
        >
          {more.map((a) => (
            <button
              key={a}
              type="button"
              role="menuitem"
              onClick={() => {
                setOpen(false);
                onAdd(a);
              }}
              className="rounded px-2.5 py-1 text-left text-[12px] hover:bg-[var(--color-bg)]"
              data-testid={`ed-format-add-${a}`}
            >
              {a}
              {a === "9:16" && <span className="ml-2 text-[var(--color-muted)]">{t("rec13.format.portrait_note")}</span>}
              {a === "16:9" && <span className="ml-2 text-[var(--color-muted)]">{t("rec13.format.landscape_note")}</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
