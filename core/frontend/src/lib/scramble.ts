/**
 * Pure helpers for the calculator's "slot-machine" result reveal
 * (`components/AnimatedNumber.tsx`). The displayed result string has its **digit
 * characters** spun through random values while the rest (signs, decimal point,
 * thousands separators, unit letters like `km`, hex `0x`/`f`, …) stay fixed, so
 * any result format — `1234.5`, `0xff`, `5 mi`, an ISO date — stays legible
 * while it animates.
 *
 * The reveal settles **left→right**: as the animation progresses each digit
 * locks to its final value at a position-dependent threshold (leftmost first),
 * giving the satisfying "the number is locking in" cascade rather than a flat
 * snap. Kept pure (rng injected) so it's deterministic and unit-tested.
 */

/** When (in [0,1] progress) the *leftmost* digit locks. The rightmost locks at
 *  progress 1, the rest fan out linearly between — so the cascade occupies the
 *  back ~65 % of the animation while everything spins for the first third. */
export const LOCK_START = 0.35;

function isDigit(ch: string): boolean {
  return ch >= "0" && ch <= "9";
}

/** Count the digit characters in `s` (the positions that spin). */
export function digitCount(s: string): number {
  let n = 0;
  for (let i = 0; i < s.length; i++) if (isDigit(s[i])) n++;
  return n;
}

/**
 * One frame of the reveal at `progress` ∈ [0,1]. Digit characters that haven't
 * reached their per-position lock threshold yet show a random digit (drawn from
 * `rng`); everything else is the final character. At `progress >= 1` the result
 * equals `target` exactly; at `progress = 0` every digit is scrambled.
 */
export function scrambleFrame(
  target: string,
  progress: number,
  rng: () => number = Math.random,
): string {
  const p = progress < 0 ? 0 : progress > 1 ? 1 : progress;
  const total = digitCount(target);
  let out = "";
  let dj = 0; // index among digit positions, left → right
  for (let i = 0; i < target.length; i++) {
    const ch = target[i];
    if (!isDigit(ch)) {
      out += ch;
      continue;
    }
    const lockAt = total <= 1 ? 1 : LOCK_START + (1 - LOCK_START) * (dj / (total - 1));
    out += p >= lockAt ? ch : String(Math.min(9, Math.floor(rng() * 10)));
    dj++;
  }
  return out;
}
