import { useEffect, useState, type ReactNode } from "react";
import { Check } from "lucide-react";
import { t, useT } from "../lib/i18n";

/** How long an armed button waits for the second click. */
export const CONFIRM_ARM_MS = 4000;

/**
 * A delete button that needs two clicks: the first arms it (red, with a
 * check mark and "Really delete?"), the second runs the action. It disarms
 * by itself after a few seconds or when the pointer leaves it.
 *
 * For actions that cannot be undone and are one small icon away from an
 * accidental click.
 */
export function ConfirmButton({
  onConfirm,
  title,
  className,
  armedClassName = "rounded p-0.5 text-red-400 bg-red-400/10 opacity-100",
  children,
  armedLabel,
}: {
  onConfirm: () => void;
  title: string;
  className: string;
  /** Replaces `className` while armed. */
  armedClassName?: string;
  children: ReactNode;
  /** Text shown next to the check mark while armed (omit for icon-only rows). */
  armedLabel?: string;
}) {
  useT();
  const [armed, setArmed] = useState(false);

  useEffect(() => {
    if (!armed) return;
    const id = window.setTimeout(() => setArmed(false), CONFIRM_ARM_MS);
    return () => window.clearTimeout(id);
  }, [armed]);

  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        if (!armed) {
          setArmed(true);
          return;
        }
        setArmed(false);
        onConfirm();
      }}
      onMouseLeave={() => setArmed(false)}
      onBlur={() => setArmed(false)}
      className={armed ? armedClassName : className}
      title={armed ? t("common.really_delete") : title}
      aria-label={armed ? t("common.really_delete") : title}
      data-armed={armed || undefined}
    >
      {armed ? (
        <span className="flex items-center gap-1">
          <Check size={12} />
          {armedLabel}
        </span>
      ) : (
        children
      )}
    </button>
  );
}
