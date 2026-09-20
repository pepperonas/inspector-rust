// Pure display logic for the mailcheck panel. Mirrors the Rust wire types
// (serde snake_case) and derives per-row states + the overall label/tone, so
// the panel stays a thin renderer and this stays unit-testable.

export type MailboxStatus =
  | "likely_valid"
  | "unknown"
  | "unlikely"
  | "invalid"
  | "not_probed";

export type CatchAll = "yes" | "no" | "unknown";

export type Overall =
  | "deliverable"
  | "deliverable_likely"
  | "risky"
  | "undeliverable"
  | "invalid";

export interface MxRecord {
  preference: number;
  host: string;
}

export interface DnsResult {
  resolves: boolean;
  mx: MxRecord[];
  implicit_mx: boolean;
  note: string | null;
}

export interface SmtpResult {
  attempted: boolean;
  connected: boolean;
  tried_host: string | null;
  banner: string | null;
  rcpt_code: number | null;
  mailbox: MailboxStatus;
  catch_all: CatchAll;
  note: string;
}

export interface MailCheckResult {
  email: string;
  syntax_valid: boolean;
  domain: string | null;
  disposable: boolean;
  dns: DnsResult;
  smtp: SmtpResult;
  overall: Overall;
}

/** Per-row status glyph state — never "color only" (§6 style rule elsewhere). */
export type Tone = "ok" | "fail" | "unknown" | "skipped" | "warn";

export function toneGlyph(t: Tone): string {
  switch (t) {
    case "ok":
      return "✓";
    case "fail":
      return "✕";
    case "warn":
      return "!";
    case "skipped":
      return "–";
    default:
      return "?";
  }
}

export const OVERALL: Record<Overall, { label: string; tone: Tone }> = {
  deliverable: { label: "DELIVERABLE", tone: "ok" },
  deliverable_likely: { label: "DELIVERABLE LIKELY", tone: "ok" },
  risky: { label: "RISKY", tone: "warn" },
  undeliverable: { label: "UNDELIVERABLE", tone: "fail" },
  invalid: { label: "INVALID", tone: "fail" },
};

export function mailboxTone(m: MailboxStatus): Tone {
  switch (m) {
    case "likely_valid":
      return "ok";
    case "invalid":
    case "unlikely":
      return "fail";
    case "not_probed":
      return "skipped";
    default:
      return "unknown";
  }
}

export function mailboxLabel(m: MailboxStatus): string {
  switch (m) {
    case "likely_valid":
      return "Likely valid";
    case "invalid":
      return "Invalid";
    case "unlikely":
      return "Unlikely";
    case "not_probed":
      return "Not probed";
    default:
      return "Inconclusive";
  }
}

export function catchAllLabel(c: CatchAll): string {
  return c === "yes" ? "Yes" : c === "no" ? "No" : "Unknown";
}

/** The per-stage tone for the summary rows. */
export function dnsTone(dns: DnsResult): Tone {
  if (!dns.resolves) return "fail";
  return dns.implicit_mx ? "warn" : "ok";
}

/** Short human summary of the DNS/MX row. */
export function mxSummary(dns: DnsResult): string {
  if (!dns.resolves) return "No MX / address record";
  if (dns.implicit_mx) return "No MX — implicit (A record)";
  const n = dns.mx.length;
  return `${n} MX record${n === 1 ? "" : "s"}`;
}

/** A pasteable one-line summary of the whole result. */
export function summaryLine(r: MailCheckResult): string {
  return [
    `${r.email} — ${OVERALL[r.overall].label}`,
    `syntax=${r.syntax_valid ? "ok" : "invalid"}`,
    `mx=${r.dns.resolves ? r.dns.mx.length : 0}`,
    `mailbox=${mailboxLabel(r.smtp.mailbox)}`,
    `catch-all=${catchAllLabel(r.smtp.catch_all)}`,
    `disposable=${r.disposable ? "yes" : "no"}`,
  ].join(" · ");
}
