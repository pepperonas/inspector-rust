import { useEffect, useRef, useState } from "react";
import {
  CELL_DRAFT_DEBOUNCE_MS,
  PALETTE_MAX_CELLS,
  PALETTE_MIN_CELLS,
  commitCellCount,
  validCellCount,
} from "../lib/palette-cells";

/**
 * One hex-grid density field. Keeps a draft while you type; saves on Enter,
 * on leaving the field, or after a short pause once the draft is a valid
 * number. Esc restores the saved value. Never disabled while a save runs —
 * a disabled input drops focus mid-typing.
 */
export function CellCountField({
  value,
  onCommit,
  label,
}: {
  value: number;
  onCommit: (n: number) => void;
  label: string;
}) {
  const [draft, setDraft] = useState(String(value));
  const [editing, setEditing] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Follow the saved value while not editing (a save, a reset elsewhere).
  // Render-phase derived state (react-hooks/refs forbids ref writes here).
  const [seen, setSeen] = useState(value);
  if (!editing && seen !== value) {
    setSeen(value);
    setDraft(String(value));
  }

  const clearTimer = () => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
  };
  useEffect(() => clearTimer, []);

  const commit = (text: string) => {
    clearTimer();
    const n = commitCellCount(text, value);
    setDraft(String(n));
    if (n !== value) onCommit(n);
  };

  return (
    <input
      type="number"
      inputMode="numeric"
      min={PALETTE_MIN_CELLS}
      max={PALETTE_MAX_CELLS}
      value={draft}
      aria-label={label}
      onFocus={() => setEditing(true)}
      onChange={(e) => {
        const text = e.target.value;
        setDraft(text);
        clearTimer();
        const n = validCellCount(text);
        if (n !== null && n !== value) {
          timer.current = setTimeout(() => {
            timer.current = null;
            onCommit(n);
          }, CELL_DRAFT_DEBOUNCE_MS);
        }
      }}
      onBlur={(e) => {
        setEditing(false);
        commit(e.target.value);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          commit(e.currentTarget.value);
        } else if (e.key === "Escape") {
          // Revert the draft; the popup's Esc handling stays untouched only
          // when there was nothing to revert.
          if (e.currentTarget.value !== String(value)) {
            e.preventDefault();
            e.stopPropagation();
            clearTimer();
            setDraft(String(value));
          }
        }
      }}
      className="w-12 rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5 text-[var(--color-fg)]"
    />
  );
}
