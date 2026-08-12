/**
 * TOTP frontend types + helpers — pure, no DOM / no IPC.
 *
 * The IPC wrappers (`totpList`, `totpAdd`, etc.) live in `ipc.ts`;
 * this module is for shapes + autocomplete-match logic.
 */

import { is2faTrigger, parse2faAdd } from "./commands";

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

/** The `totp-manage` row's payload (list = the classic manager; add =
 *  straight to the Add form, `issuer` pre-filled from `2fa add <issuer>`). */
export interface TotpManageView {
  label: string;
  mode: "list" | "add";
  issuer?: string;
}

/** The visible `2fa`↔`2fa add` sub-row (CommandSuggestionView shape). */
export interface TotpSubView {
  keyword: string;
  syntax: string;
  description: string;
  completion: string;
}

/**
 * Which command rows the query `2fa` / `otp` / `2fa add [issuer]` yields
 * (v0.104.0). Pure — App.tsx wraps the results into ListEntries.
 *
 * - bare `2fa`/`otp` → manage row (list) + a sub-row suggesting `2fa add`
 * - `2fa add [issuer]` → manage row (add, issuer pre-filled) + a sub-row
 *   back to the manager
 * - anything else (incl. `2fa <issuer>` searches like `2fa addepar`)
 *   → neither; the otp autocomplete owns those queries.
 */
export function totpCommandRows(query: string): {
  manage: TotpManageView | null;
  sub: TotpSubView | null;
} {
  const add = parse2faAdd(query);
  if (add) {
    return {
      manage: {
        label: add.issuer ? `2FA · Add "${add.issuer}"` : "2FA · Add new account",
        mode: "add",
        issuer: add.issuer,
      },
      sub: {
        keyword: "2fa",
        syntax: "2fa",
        description: "Manage TOTP entries (list · import · export)",
        completion: "2fa",
      },
    };
  }
  if (is2faTrigger(query)) {
    return {
      manage: { label: "2FA · Manage TOTP", mode: "list" },
      sub: {
        keyword: "2fa add",
        syntax: "2fa add [issuer]",
        description: "Add a new 2FA account (issuer · login · secret)",
        completion: "2fa add ",
      },
    };
  }
  return { manage: null, sub: null };
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
