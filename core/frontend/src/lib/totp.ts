/**
 * TOTP frontend types + helpers — pure, no DOM / no IPC.
 *
 * The IPC wrappers (`totpList`, `totpAdd`, etc.) live in `ipc.ts`;
 * this module is for shapes + autocomplete-match logic.
 */

export interface TotpEntry {
  id: number;
  issuer: string;
  account: string;
  digits: number;
  period: number;
  algorithm: string;
  /** Unix seconds. */
  created_at: number;
}

/** One entry's current code + how many seconds it stays valid. */
export interface TotpCode {
  id: number;
  code: string;
  seconds_remaining: number;
}

/**
 * Fuzzy match `query` against entries' issuer + account fields.
 * Case-insensitive substring on either; ranked by closeness of the
 * issuer match (prefix > infix > account-only-infix).
 *
 * Used by App.tsx to surface `otp ama` → Amazon at the top of the list.
 */
export function matchTotpEntries(query: string, entries: TotpEntry[]): TotpEntry[] {
  const q = query.trim().toLowerCase();
  if (!q) return entries;

  type Scored = { entry: TotpEntry; score: number };
  const scored: Scored[] = [];

  for (const e of entries) {
    const issuer = e.issuer.toLowerCase();
    const account = e.account.toLowerCase();
    let score = -1;
    if (issuer.startsWith(q)) {
      score = 100 - q.length;
    } else if (issuer.includes(q)) {
      score = 50 - issuer.indexOf(q);
    } else if (account.startsWith(q)) {
      score = 30;
    } else if (account.includes(q)) {
      score = 10;
    }
    if (score >= 0) {
      scored.push({ entry: e, score });
    }
  }
  scored.sort((a, b) => b.score - a.score);
  return scored.map((s) => s.entry);
}
