// Inline-help trigger parsing (v0.84.273).
//
// A trailing `?` on the search query asks for help. The whole subtlety is
// telling a HELP request apart from a `?` that's a literal character — inside a
// template/quote (`faker tpl "warum? {name}"`), a glob/regex (`a?b`), a URL
// (`…?id=1`), or an argument (`bruno hallo?`). The rule, kept deliberately
// narrow, is: help triggers ONLY for a bare command (or command-prefix) token
// immediately followed — with at most one space — by a lone trailing `?`, plus
// `?` on its own for the global index. Anything with an argument between the
// command and the `?`, or a non-command-shaped token, is treated as literal.
//
// Pure + exhaustively unit-tested. It resolves to the concrete CommandDoc (or
// the index sentinel) so the caller is a one-liner.

import { COMMAND_DOCS, lookupDoc, type CommandDoc } from "./commandDocs";
import { commandSuggestions } from "./commands";

export type HelpTarget =
  | { kind: "index" } // `?` alone → the whole cheat-sheet
  | { kind: "doc"; doc: CommandDoc };

// `?` alone (whitespace-tolerant).
const INDEX_RE = /^\s*\?\s*$/;
// A single command-shaped token, then optional single-space, then a lone `?`.
// Command keywords are all lowercase alnum, so this shape rejects URLs (`:/.`),
// paths, and quoted/templated args — the whole collision class — because none
// of those reduce to a single `[a-z0-9]+` token before the trailing `?`.
const TOKEN_HELP_RE = /^\s*([a-z0-9]+)\s?\?\s*$/i;

/**
 * Parse a raw search query into a help request, or `null` when a trailing `?`
 * (if any) is a literal character rather than a help trigger.
 *
 * - `?`                    → the command index
 * - `<command>?` / `<command> ?` → that command's doc (aliases resolve too)
 * - `<prefix>?`            → the doc of the top autocomplete match for `<prefix>`
 * - everything else        → `null` (literal `?`, or no matching command)
 */
export function parseHelpQuery(query: string): HelpTarget | null {
  if (INDEX_RE.test(query)) return { kind: "index" };

  const m = TOKEN_HELP_RE.exec(query);
  if (!m) return null;
  const token = m[1].toLowerCase();

  // Exact command or alias (incl. hidden aliases like `cal`, `nmap`).
  const exact = lookupDoc(token);
  if (exact) return { kind: "doc", doc: exact };

  // A prefix/fuzzy partial → resolve to the top autocomplete match's doc.
  // Every non-hidden command has a doc (enforced by commandDocs.test.ts), so a
  // non-empty suggestion list always yields one.
  const top = commandSuggestions(token)[0];
  if (top) {
    const doc = lookupDoc(top.keyword);
    if (doc) return { kind: "doc", doc };
  }
  return null;
}

/** True when the query is any kind of help request (index or a command doc). */
export function isHelpQuery(query: string): boolean {
  return parseHelpQuery(query) !== null;
}

/** The full doc list for the `?` index, in registry order. */
export function allDocs(): readonly CommandDoc[] {
  return COMMAND_DOCS;
}
