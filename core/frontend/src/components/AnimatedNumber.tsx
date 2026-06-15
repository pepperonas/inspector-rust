import { useEffect, useRef, useState } from "react";
import { scrambleFrame, digitCount } from "../lib/scramble";
import { prefersReducedMotion } from "../lib/md3-motion";

/**
 * "Slot-machine" reveal for the calculator result. Spins the digit characters
 * for ~0.5 s then settles them left→right, with a spring "pop" + accent flash on
 * lock-in. See `lib/scramble.ts` for the per-frame math.
 *
 * The settle deadline is **pushed forward on every value change**, so the digits
 * keep rolling while the user is still typing the expression and lock in 0.5 s
 * after they stop — graceful at any typing speed, no restart-flicker. Results
 * with no digits (e.g. `NaN`), or when the OS prefers reduced motion, render
 * instantly with no animation.
 */
const DURATION = 500; // ms — the "wild roll" window
const ROLL_MS = 33; // re-randomise unlocked digits ~30×/s (readable, not a blur)

export function AnimatedNumber({ value, className }: { value: string; className?: string }) {
  const [display, setDisplay] = useState(() =>
    digitCount(value) > 0 && !prefersReducedMotion() ? scrambleFrame(value, 0) : value,
  );
  // Bumped on each lock-in so the wrapper remounts and replays the pop.
  const [lockKey, setLockKey] = useState(0);
  const [locked, setLocked] = useState(false);

  const targetRef = useRef(value);
  const settleAtRef = useRef(0);
  const lastRollRef = useRef(0);
  const rafRef = useRef<number | null>(null);
  const runningRef = useRef(false);

  useEffect(() => {
    targetRef.current = value;

    // No digits to spin, or reduced motion → just show the value (and stop any
    // roll still in flight from a previous, digit-bearing value).
    if (digitCount(value) === 0 || prefersReducedMotion()) {
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
      runningRef.current = false;
      setDisplay(value);
      setLocked(true);
      return;
    }

    // (Re)start / extend the roll: lock in DURATION ms from *now*.
    settleAtRef.current = performance.now() + DURATION;
    setLocked(false);

    const tick = (now: number) => {
      const target = targetRef.current;
      const progress = (now - (settleAtRef.current - DURATION)) / DURATION;
      if (progress >= 1) {
        setDisplay(target);
        setLocked(true);
        setLockKey((k) => k + 1);
        runningRef.current = false;
        rafRef.current = null;
        return;
      }
      if (now - lastRollRef.current >= ROLL_MS) {
        lastRollRef.current = now;
        setDisplay(scrambleFrame(target, progress));
      }
      rafRef.current = requestAnimationFrame(tick);
    };

    if (!runningRef.current) {
      runningRef.current = true;
      lastRollRef.current = 0;
      rafRef.current = requestAnimationFrame(tick);
    }

    return () => {
      // Only tear down on unmount; a value change just retargets the live loop.
    };
  }, [value]);

  // Cancel the loop on unmount.
  useEffect(
    () => () => {
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
      runningRef.current = false;
    },
    [],
  );

  return (
    <span
      key={lockKey}
      className={(locked && lockKey > 0 ? "md3-num-lock " : "") + (className ?? "")}
      style={{ fontVariantNumeric: "tabular-nums" }}
    >
      {display}
    </span>
  );
}
