/**
 * Brand icons for 2FA issuers (v0.161.0) — maps a free-form TOTP `issuer`
 * string ("GitHub", "accounts.google.com", "Google (privat)") onto a Simple
 * Icons entry from the generated `totp-icons.json` (full 3457-icon set,
 * lazy-loaded — see scripts/gen-totp-icons.mjs).
 *
 * Pure + unit-tested; the only impure edge is `loadIconIndex` (memoised
 * dynamic import so the 4.5 MB chunk loads once, and only when the 2FA
 * overlay actually opens).
 *
 * ⚠️ No icon is ever invented: an issuer that matches nothing renders as a
 * deterministic monogram (initial + stable hue), never a look-alike brand.
 */

export type IconEntry = {
  slug: string;
  title: string;
  hex: string;
  path: string;
};

export type IconData = {
  v: string;
  icons: Record<string, [title: string, hex: string, path: string]>;
  alias: Record<string, string>;
};

export type IconIndex = {
  bySlug: Map<string, IconEntry>;
  /** normalised key → slug */
  lookup: Map<string, string>;
};

/** Lowercase + strip everything that isn't a letter or digit — the shape of a
 * Simple Icons slug ("Amazon Pay" → "amazonpay", ".ENV" → "env"). */
export function normalizeKey(s: string): string {
  return s.toLowerCase().replace(/[^a-z0-9]/g, "");
}

/**
 * Build the runtime lookup. Precedence when normalised keys collide:
 * slug > title > alias — a slug IS the canonical id, so nothing may shadow it.
 */
export function buildIconIndex(data: IconData): IconIndex {
  const bySlug = new Map<string, IconEntry>();
  const lookup = new Map<string, string>();
  for (const [slug, [title, hex, path]] of Object.entries(data.icons)) {
    bySlug.set(slug, { slug, title, hex, path });
    lookup.set(slug, slug);
  }
  for (const [slug, [title]] of Object.entries(data.icons)) {
    const k = normalizeKey(title);
    if (k && !lookup.has(k)) lookup.set(k, slug);
  }
  for (const [name, slug] of Object.entries(data.alias)) {
    const k = normalizeKey(name);
    if (k && !lookup.has(k) && bySlug.has(slug)) lookup.set(k, slug);
  }
  return { bySlug, lookup };
}

/** Domain labels that are never the brand ("github.com" → try "github"). */
const TLDISH = new Set([
  "www", "com", "net", "org", "io", "de", "at", "ch", "uk", "co",
  "app", "dev", "me", "eu", "us", "cloud", "login", "accounts", "auth", "id",
]);

/**
 * Resolve an issuer to an icon. Match order (first hit wins):
 * 1. the whole issuer, normalised ("GitHub" → "github");
 * 2. for dotted issuers, the domain labels right-to-left skipping TLD-ish
 *    parts ("accounts.google.com" → "google" before "accounts" — the label
 *    next to the TLD is the brand);
 * 3. the first word ("Google (privat)" → "google").
 * A miss is an honest null — the caller renders a monogram, never a guess.
 */
export function iconForIssuer(index: IconIndex, issuer: string): IconEntry | null {
  const raw = issuer.trim();
  if (!raw) return null;
  const hit = (key: string): IconEntry | null => {
    const slug = index.lookup.get(key);
    return slug ? (index.bySlug.get(slug) ?? null) : null;
  };
  const whole = hit(normalizeKey(raw));
  if (whole) return whole;
  if (raw.includes(".")) {
    const labels = raw.toLowerCase().replace(/^[a-z]+:\/\//, "").split(/[/?#]/)[0].split(".");
    for (let i = labels.length - 1; i >= 0; i--) {
      const label = labels[i];
      if (TLDISH.has(label)) continue;
      const h = hit(normalizeKey(label));
      if (h) return h;
    }
  }
  const first = raw.split(/\s+/)[0];
  if (first && first !== raw) {
    const h = hit(normalizeKey(first));
    if (h) return h;
  }
  return null;
}

/**
 * Whether a brand colour is too LIGHT for the light icon chip and needs the
 * dark chip instead. The chip is deliberately light in BOTH themes (the icon
 * catalogue's proven decision — GitHub #181717 is invisible on a dark
 * ground), so only near-white brands flip.
 */
export function chipNeedsDark(hex: string): boolean {
  const n = parseInt(hex.length === 6 ? hex : "808080", 16);
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  const lum =
    0.2126 * lin((n >> 16) & 255) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255);
  return lum > 0.62;
}

/** Deterministic 0..359 hue for the monogram fallback (FNV-1a). */
export function monogramHue(issuer: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < issuer.length; i++) {
    h ^= issuer.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h % 360;
}

/** First letter/digit, uppercased — "?" when there is none. */
export function monogramInitial(issuer: string): string {
  const m = issuer.match(/[\p{L}\p{N}]/u);
  return m ? m[0].toUpperCase() : "?";
}

let indexPromise: Promise<IconIndex> | null = null;

/** Memoised lazy load of the generated icon data (4.5 MB — only on demand). */
export function loadIconIndex(): Promise<IconIndex> {
  if (!indexPromise) {
    indexPromise = import("./totp-icons.json").then((m) =>
      buildIconIndex(m.default as unknown as IconData),
    );
  }
  return indexPromise;
}
