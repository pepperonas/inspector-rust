/**
 * Detect actionable content in a clipboard text entry and offer one-tap
 * actions in the preview (open a URL, compose an email, call a number, open
 * coordinates in Maps, make a QR). Pure + unit-tested; `App`/`PreviewPanel`
 * wire the actual `openUrl` / QR copy to each action's `href`/`kind`.
 */

export type SmartActionKind = "open-url" | "email" | "call" | "maps" | "qr";

export interface SmartAction {
  kind: SmartActionKind;
  /** Button label, e.g. "Open link". */
  label: string;
  /** For open-url/email/call/maps: the URL/URI to open. For `qr`: the raw
   *  text to encode (handled specially by the caller). */
  href: string;
}

const URL_RE = /^(https?:\/\/[^\s]+)$/i;
const BARE_DOMAIN_RE =
  /^(?:www\.)?([a-z0-9-]+\.)+[a-z]{2,}(?:\/[^\s]*)?$/i;
const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
// A phone number: optional +, then 7-20 chars of digits / space / ()-. ,
// with at least 7 digits total.
const PHONE_RE = /^\+?[\d\s().-]{7,20}$/;
const COORDS_RE = /^(-?\d{1,3}(?:\.\d+)?)\s*,\s*(-?\d{1,3}(?:\.\d+)?)$/;

function digitCount(s: string): number {
  return (s.match(/\d/g) ?? []).length;
}

/**
 * Return the actions applicable to `text`. The most specific action comes
 * first; a "Make QR" action is appended for any short, single-line value so
 * you can always beam a clip to your phone.
 */
export function detectSmartActions(text: string): SmartAction[] {
  const t = text.trim();
  const actions: SmartAction[] = [];
  if (!t) return actions;

  const singleLine = !/\n/.test(t);

  if (singleLine && URL_RE.test(t)) {
    actions.push({ kind: "open-url", label: "Open link", href: t });
  } else if (singleLine && BARE_DOMAIN_RE.test(t) && !EMAIL_RE.test(t)) {
    actions.push({ kind: "open-url", label: "Open link", href: `https://${t.replace(/^https?:\/\//i, "")}` });
  } else if (singleLine && EMAIL_RE.test(t)) {
    actions.push({ kind: "email", label: "Compose email", href: `mailto:${t}` });
  } else if (singleLine && COORDS_RE.test(t)) {
    const m = t.match(COORDS_RE)!;
    const lat = Number(m[1]);
    const lng = Number(m[2]);
    // Valid lat/lng ranges only.
    if (Math.abs(lat) <= 90 && Math.abs(lng) <= 180) {
      actions.push({
        kind: "maps",
        label: "Open in Maps",
        href: `https://www.google.com/maps/search/?api=1&query=${lat},${lng}`,
      });
    }
  } else if (singleLine && PHONE_RE.test(t) && digitCount(t) >= 7 && digitCount(t) <= 15) {
    const tel = t.replace(/[^\d+]/g, "");
    actions.push({ kind: "call", label: "Call", href: `tel:${tel}` });
  }

  // QR for any short, single-line value (cap so we never encode a huge blob).
  if (singleLine && t.length <= 512) {
    actions.push({ kind: "qr", label: "Make QR", href: t });
  }

  return actions;
}
