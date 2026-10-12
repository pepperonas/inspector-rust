// Pure display logic for the mailcheck panel. Mirrors the Rust wire types
// (serde snake_case) and derives per-row states + the overall label/tone, so
// the panel stays a thin renderer and this stays unit-testable.

import { t, tn } from "./i18n";

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

export const OVERALL: Record<Overall, { readonly label: string; tone: Tone }> = {
  deliverable: { get label() { return t("mail.overall.deliverable"); }, tone: "ok" },
  deliverable_likely: { get label() { return t("mail.overall.deliverable_likely"); }, tone: "ok" },
  risky: { get label() { return t("mail.overall.risky"); }, tone: "warn" },
  undeliverable: { get label() { return t("mail.overall.undeliverable"); }, tone: "fail" },
  invalid: { get label() { return t("mail.overall.invalid"); }, tone: "fail" },
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
      return t("mail.box.likely_valid");
    case "invalid":
      return t("mail.box.invalid");
    case "unlikely":
      return t("mail.box.unlikely");
    case "not_probed":
      return t("mail.box.not_probed");
    default:
      return t("mail.box.inconclusive");
  }
}

export function catchAllLabel(c: CatchAll): string {
  return c === "yes" ? t("mail.yes") : c === "no" ? t("mail.no") : t("mail.unknown");
}

/** The per-stage tone for the summary rows. */
export function dnsTone(dns: DnsResult): Tone {
  if (!dns.resolves) return "fail";
  return dns.implicit_mx ? "warn" : "ok";
}

/** Short human summary of the DNS/MX row. */
export function mxSummary(dns: DnsResult): string {
  if (!dns.resolves) return t("mail.mx.none");
  if (dns.implicit_mx) return t("mail.mx.implicit");
  return tn("mail.mx.records", dns.mx.length);
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
