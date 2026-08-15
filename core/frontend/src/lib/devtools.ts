/**
 * Small developer utilities backing the `uuid` / `slug` / `hash` / `json` /
 * `jwt` search-bar commands. Everything here is pure (or a thin wrapper over
 * Web Crypto) and unit-tested; the impure clipboard read/write happens in
 * `App.tsx`'s `dispatchCommand`.
 */

/** Lowercase, URL-safe slug: spaces/underscores → `-`, diacritics stripped,
 *  non-alphanumerics dropped, runs of `-` collapsed, trimmed. */
export function slugify(input: string): string {
  return input
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "") // strip combining diacritics
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-") // any run of non-alnum → single dash
    .replace(/^-+|-+$/g, ""); // trim leading/trailing dashes
}

/** Generate `n` (clamped 1..100) random v4 UUIDs, newline-joined. */
export function generateUuids(n: number): string {
  const count = Math.max(1, Math.min(Number.isFinite(n) ? Math.floor(n) : 1, 100));
  return Array.from({ length: count }, () => crypto.randomUUID()).join("\n");
}

/** Hex SHA-256 of a string (async — uses Web Crypto `subtle.digest`). */
export async function sha256Hex(text: string): Promise<string> {
  const bytes = new TextEncoder().encode(text);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

/** Pretty-print a JSON string with 2-space indent. Throws on invalid JSON. */
export function formatJson(input: string): string {
  const parsed = JSON.parse(input);
  return JSON.stringify(parsed, null, 2);
}

/** Decode the base64url **header + payload** of a JWT into pretty JSON.
 *  The signature is shown verbatim (it isn't JSON and isn't verified). */
export function decodeJwt(token: string): string {
  const parts = token.trim().split(".");
  if (parts.length < 2) {
    throw new Error("not a JWT (expected at least header.payload)");
  }
  const decodePart = (part: string): unknown => {
    // base64url → base64, pad, decode, parse.
    const b64 = part.replace(/-/g, "+").replace(/_/g, "/");
    const padded = b64 + "=".repeat((4 - (b64.length % 4)) % 4);
    // `atob` yields LATIN-1 bytes, so a UTF-8 payload has to be decoded — the
    // plain `atob` this used to do mojibaked every non-ASCII claim ("Jörg" →
    // "JÃ¶rg"), and `name`/`given_name` carry Umlauts all the time. Same
    // Uint8Array + TextDecoder path `text-transform.ts::base64Decode` uses;
    // the two were silently inconsistent.
    const bin = atob(padded);
    const json = new TextDecoder().decode(Uint8Array.from(bin, (c) => c.charCodeAt(0)));
    return JSON.parse(json);
  };
  const header = decodePart(parts[0]);
  const payload = decodePart(parts[1]);
  return JSON.stringify(
    { header, payload, signature: parts[2] ?? null },
    null,
    2,
  );
}
