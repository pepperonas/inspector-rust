/**
 * `rnd` / `random` — shows the rolled number **directly** in the preview column
 * (like `calendar`, live while you type) plus a **Roll again** button that
 * regenerates within the same range. Enter pastes exactly the shown number.
 *
 * The range math is the pure, unit-tested `parseRandomArg` (accepts `rnd`,
 * `rnd 100`, `rnd 5 500`, and `rnd 1-2`); the roll is the CSPRNG `randomInt`.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { Dices, RefreshCw } from "lucide-react";
import { parseRandomArg, randomInt } from "../lib/commands";

export function RandomPanel({
  arg,
  onValue,
  onInteract,
}: {
  /** Live command argument (`rnd 5 500`) — the range. */
  arg: string;
  /** Report the currently shown number (or null) so Enter can paste exactly it. */
  onValue: (n: number | null) => void;
  /** Called after a click so the parent keeps the search field focused. */
  onInteract?: () => void;
}) {
  const range = parseRandomArg(arg);
  const rangeKey = range ? `${range.min}-${range.max}` : "invalid";
  const [value, setValue] = useState<number | null>(null);
  const [pop, setPop] = useState(0); // re-mount key → replays the flash
  const lastKey = useRef<string>("");

  const roll = useCallback(() => {
    if (!range) {
      setValue(null);
      onValue(null);
      return;
    }
    const n = randomInt(range.min, range.max);
    setValue(n);
    onValue(n);
    setPop((p) => p + 1);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rangeKey, onValue]);

  // Re-roll whenever the parsed range changes (typing `rnd 100` → `rnd 5 500`).
  useEffect(() => {
    if (lastKey.current === rangeKey) return;
    lastKey.current = rangeKey;
    roll();
  }, [rangeKey, roll]);

  if (!range) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center">
        <Dices size={26} className="text-[var(--color-muted)]" />
        <p className="text-[13px] font-medium text-[var(--color-fg)]">Invalid range</p>
        <p className="text-[12px] leading-relaxed text-[var(--color-muted)]">
          Try <code>rnd</code> · <code>rnd 100</code> · <code>rnd 5 500</code> · <code>rnd 1-2</code>
        </p>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col items-center justify-center gap-5 p-6">
      <div className="flex items-center gap-1.5 text-[11px] uppercase tracking-[0.14em] text-[var(--color-muted)]">
        <Dices size={13} /> Random · {range.min}–{range.max}
      </div>
      <div
        key={pop}
        className="md3-num-lock font-mono text-[72px] font-bold leading-none text-[var(--color-fg)]"
        style={{ fontVariantNumeric: "tabular-nums" }}
      >
        {value}
      </div>
      <button
        onClick={() => {
          roll();
          onInteract?.();
        }}
        className="md3-press flex items-center gap-2 rounded-full bg-[var(--color-accent)] px-5 py-2.5 text-[13px] font-medium text-[var(--color-accent-fg)]"
      >
        <RefreshCw size={15} /> Roll again
      </button>
      <p className="text-[11px] text-[var(--color-muted)]">⏎ Enter pastes this number</p>
    </div>
  );
}
