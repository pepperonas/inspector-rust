import type { ListEntry } from "./types";

/**
 * A pasted link follows the `qr` command workflow (2026-09-26): a search
 * query that is exactly ONE web link gets a `qr` command row — the same
 * `command` entry `qr <link>` produces, so the preview, Enter-copies-PNG and
 * Save PNG / STL all come for free and can't drift from the command.
 *
 * Only a single token that is unmistakably a link counts (`http(s)://…` or
 * `www.…`): a bare word like `example.com` is also a plausible clip search.
 * Ignored when the query already parses as a command (then the command owns
 * the row — `qr https://…` must not get a second, duplicate QR row).
 */
export function linkForQr(query: string): string | null {
  const q = query.trim();
  if (!q || /\s/.test(q)) return null;
  if (!/^(https?:\/\/[^/\s]+\.[^\s]+|https?:\/\/localhost\b\S*|www\.[^\s]+\.[^\s]+)$/i.test(q)) {
    return null;
  }
  // Same payload cap as lib/qr.ts (UTF-8 bytes) — a longer link can't be encoded.
  if (new TextEncoder().encode(q).length > 2331) return null;
  return q;
}

export function qrLinkEntry(query: string, hasCommand: boolean): ListEntry | null {
  if (hasCommand) return null;
  const link = linkForQr(query);
  if (!link) return null;
  return {
    kind: "command",
    data: {
      commandKind: "qr",
      rawInput: query,
      arg: link,
      label: `QR code for link: ${link.length > 60 ? `${link.slice(0, 60)}…` : link}`,
      hint: "Preview on the right · Enter copies PNG · Save PNG / STL in the preview",
    },
  };
}
