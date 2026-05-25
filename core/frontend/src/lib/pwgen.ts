/**
 * `pwgen N` — password generator surfaced as a search-bar command.
 *
 * Four modes, all using CSPRNG via Web Crypto (`crypto.getRandomValues`):
 *
 *   - `all`       — alphanumeric + symbols (`!@#$%^&*()_+-=[]{};:,.?`)
 *   - `alnum`     — alphanumeric only (uppercase + lowercase + digits)
 *   - `dict`      — English dictionary words, CapitalisedConcatenated,
 *                   padded with random digits to the exact target length
 *   - `leet`      — same as `dict`, then leet-substituted (a→@, e→3, …)
 *
 * Pure-TS; no IPC. The active mode is component state in App.tsx; the
 * password regenerates on every (query × mode) change so the user can
 * try-mash-enter for a new random.
 *
 * All four generators return EXACTLY `length` characters — dict / leet
 * truncate or pad with digits as needed.
 */

import { DICT_WORDS } from "./pwgen-dict";

export type PwgenMode = "all" | "alnum" | "dict" | "leet";

const CHARSET_ALL =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789" +
  "!@#$%^&*()_+-=[]{};:,.?";
const CHARSET_ALNUM =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/** Uniform random integer in `[0, max)` from a CSPRNG. Uses rejection
 *  sampling to eliminate modulo bias when `max` doesn't divide 2^32.
 *  Fallback to `Math.random` only when Web Crypto is unavailable (test
 *  environments without `crypto.getRandomValues`).
 */
function randInt(max: number): number {
  if (max <= 0) return 0;
  if (typeof crypto !== "undefined" && crypto.getRandomValues) {
    // Rejection-sample to avoid modulo bias. 32-bit ints are plenty
    // for max < 2^31 (we only ever ask for ≤ 1024 or so).
    const limit = Math.floor(0x100000000 / max) * max;
    const buf = new Uint32Array(1);
    while (true) {
      crypto.getRandomValues(buf);
      if (buf[0] < limit) return buf[0] % max;
    }
  }
  // Test / non-Tauri fallback. NOT cryptographically secure.
  return Math.floor(Math.random() * max);
}

/** Pick `n` characters uniformly from `charset` (CSPRNG). */
function randomChars(charset: string, n: number): string {
  let out = "";
  for (let i = 0; i < n; i++) {
    out += charset[randInt(charset.length)];
  }
  return out;
}

/** One random word from the dictionary. */
function randomWord(): string {
  return DICT_WORDS[randInt(DICT_WORDS.length)];
}

/** Capitalise the first letter (lowercase the rest is a no-op since
 *  the dict is already lowercase). */
function capitalise(w: string): string {
  return w.length === 0 ? w : w[0].toUpperCase() + w.slice(1);
}

/** Build a dict-words password of EXACTLY `length` chars:
 *  - keep concatenating Capitalised words while the next word fits,
 *  - then pad the remainder with random digits to reach the target.
 *
 *  The pad-with-digits step keeps individual words intact (no mid-word
 *  truncation) while still hitting the exact length the user asked for.
 */
function generateDict(length: number): string {
  let s = "";
  // Cap word-add attempts so a pathological dict (all huge words +
  // tiny target) can't infinite-loop. 50 is generous.
  for (let i = 0; s.length < length && i < 50; i++) {
    const w = capitalise(randomWord());
    if (s.length + w.length > length) break;
    s += w;
  }
  // Fill remainder with random digits via CSPRNG.
  while (s.length < length) {
    s += String(randInt(10));
  }
  return s.slice(0, length);
}

/** Leet substitution. Conservative — only the most readable swaps,
 *  to preserve some recognisability while frustrating dictionary
 *  attacks. */
const LEET: Record<string, string> = {
  a: "@", e: "3", i: "1", o: "0", s: "$", t: "7", l: "1", g: "9", b: "8",
  A: "@", E: "3", I: "1", O: "0", S: "$", T: "7", L: "1", G: "9", B: "8",
};

function leetTransform(s: string): string {
  let out = "";
  for (const ch of s) out += LEET[ch] ?? ch;
  return out;
}

/** Generate a password for the given mode + length. */
export function generatePassword(mode: PwgenMode, length: number): string {
  const len = Math.max(1, Math.min(256, Math.floor(length)));
  switch (mode) {
    case "all":
      return randomChars(CHARSET_ALL, len);
    case "alnum":
      return randomChars(CHARSET_ALNUM, len);
    case "dict":
      return generateDict(len);
    case "leet":
      return leetTransform(generateDict(len));
  }
}
