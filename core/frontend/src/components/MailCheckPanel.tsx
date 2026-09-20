import { useCallback, useEffect, useRef, useState } from "react";
import { AlertCircle, Copy, Mail, RefreshCw } from "lucide-react";
import { mailcheckRun } from "../lib/ipc";
import {
  OVERALL,
  catchAllLabel,
  dnsTone,
  mailboxLabel,
  mailboxTone,
  mxSummary,
  summaryLine,
  toneGlyph,
  type MailCheckResult,
  type Tone,
} from "../lib/mailcheck";

interface Props {
  email: string;
  focused: boolean;
  onExit: () => void;
}

const TONE_TEXT: Record<Tone, string> = {
  ok: "text-emerald-400",
  fail: "text-red-400",
  warn: "text-amber-400",
  unknown: "text-[var(--color-muted)]",
  skipped: "text-[var(--color-muted)]",
};

export function MailCheckPanel({ email, focused, onExit }: Props) {
  const [result, setResult] = useState<MailCheckResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [runId, setRunId] = useState(0);

  const containerRef = useRef<HTMLDivElement>(null);
  const seqRef = useRef(0);

  useEffect(() => {
    const target = email.trim();
    if (!target) {
      setResult(null);
      setError(null);
      return;
    }
    const seq = ++seqRef.current;
    setLoading(true);
    setError(null);
    mailcheckRun(target)
      .then((r) => {
        if (seq === seqRef.current) {
          setResult(r);
          setLoading(false);
        }
      })
      .catch((e) => {
        if (seq === seqRef.current) {
          setError(String(e));
          setLoading(false);
        }
      });
  }, [email, runId]);

  useEffect(() => {
    if (focused) containerRef.current?.focus();
  }, [focused]);

  const copySummary = useCallback(async () => {
    if (!result) return;
    try {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(summaryLine(result));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    } catch {
      /* ignore */
    }
  }, [result]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onExit();
    } else if ((e.key === "r" || e.key === "R") && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      setRunId((n) => n + 1);
    }
  };

  return (
    <div
      ref={containerRef}
      tabIndex={-1}
      onKeyDown={onKeyDown}
      className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] outline-none [contain:paint]"
    >
      <div className="flex shrink-0 items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2 text-[13px] font-medium">
          <Mail className="h-4 w-4 shrink-0 text-[var(--color-accent)]" />
          <span className="truncate">Mail Check</span>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <button
            onClick={copySummary}
            disabled={!result}
            title="Zusammenfassung kopieren"
            className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)] disabled:opacity-30"
          >
            <Copy className="h-4 w-4" />
          </button>
          <button
            onClick={() => setRunId((n) => n + 1)}
            title="Erneut prüfen (R)"
            className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          >
            <RefreshCw className={`h-4 w-4 ${loading ? "motion-safe:animate-spin" : ""}`} />
          </button>
        </div>
      </div>

      <div className="truncate font-[var(--font-mono)] text-[12px] text-[var(--color-muted)]">
        {email.trim() || "—"}
      </div>

      {error && (
        <div className="md3-banner-in flex items-start gap-2 rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-[11px] leading-snug">
          <AlertCircle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-red-400" />
          <span>{error}</span>
        </div>
      )}

      {loading && !result && (
        <div className="flex flex-1 items-center justify-center gap-2 text-[12px] text-[var(--color-muted)]">
          <RefreshCw className="h-4 w-4 motion-safe:animate-spin" /> Prüfe Zustellbarkeit …
        </div>
      )}

      {result && (
        <>
          {/* overall status banner */}
          <div className="flex flex-col gap-2 rounded-lg border border-[var(--color-border)] p-3">
            <div className="flex items-center gap-2">
              <span
                className={`inline-block h-2.5 w-2.5 rounded-full ${
                  OVERALL[result.overall].tone === "ok"
                    ? "bg-emerald-500"
                    : OVERALL[result.overall].tone === "warn"
                      ? "bg-amber-500"
                      : "bg-red-500"
                } ${loading ? "motion-safe:animate-pulse" : ""}`}
              />
              <span className={`text-[13px] font-semibold ${TONE_TEXT[OVERALL[result.overall].tone]}`}>
                {OVERALL[result.overall].label}
              </span>
            </div>
            <div className="flex flex-col gap-1 text-[11px]">
              <SummaryRow label="Syntax" tone={result.syntax_valid ? "ok" : "fail"} />
              <SummaryRow
                label="Domain"
                tone={result.domain ? "ok" : "fail"}
                value={result.domain ?? "—"}
              />
              <SummaryRow label="MX" tone={dnsTone(result.dns)} value={mxSummary(result.dns)} />
              <SummaryRow
                label="SMTP"
                tone={mailboxTone(result.smtp.mailbox)}
                value={mailboxLabel(result.smtp.mailbox)}
              />
              <SummaryRow
                label="Disposable"
                tone={result.disposable ? "warn" : "ok"}
                value={result.disposable ? "Yes" : "No"}
                plain
              />
              <SummaryRow
                label="Catch-all"
                tone={
                  result.smtp.catch_all === "yes"
                    ? "warn"
                    : result.smtp.catch_all === "no"
                      ? "ok"
                      : "unknown"
                }
                value={catchAllLabel(result.smtp.catch_all)}
                plain
              />
            </div>
          </div>

          {/* MX records */}
          {result.dns.mx.length > 0 && (
            <div className="flex flex-col gap-1">
              <div className="text-[10px] font-medium uppercase tracking-wide text-[var(--color-muted)]">
                MX Records
              </div>
              <div className="rounded-md border border-[var(--color-border)] p-2 font-[var(--font-mono)] text-[11px]">
                {result.dns.mx.map((m, i) => (
                  <div key={i} className="flex gap-3">
                    <span className="w-8 shrink-0 tabular-nums text-[var(--color-muted)]">
                      {m.preference}
                    </span>
                    <span className="min-w-0 truncate">{m.host}</span>
                  </div>
                ))}
                {result.dns.implicit_mx && (
                  <div className="mt-1 text-[9px] text-amber-400">
                    Kein MX-Record — A-Record als impliziter Mailserver.
                  </div>
                )}
              </div>
            </div>
          )}

          {/* SMTP detail note */}
          {result.smtp.attempted && (
            <div className="flex flex-col gap-1 rounded-md border border-[var(--color-border)] p-2 text-[11px]">
              <div className="flex items-center justify-between">
                <span className="text-[10px] font-medium uppercase tracking-wide text-[var(--color-muted)]">
                  Mailbox
                </span>
                <span className={`text-[11px] font-medium ${TONE_TEXT[mailboxTone(result.smtp.mailbox)]}`}>
                  {toneGlyph(mailboxTone(result.smtp.mailbox))} {mailboxLabel(result.smtp.mailbox)}
                </span>
              </div>
              <p className="text-[10px] leading-relaxed text-[var(--color-muted)]">
                {result.smtp.note}
              </p>
              {result.smtp.tried_host && (
                <p className="font-[var(--font-mono)] text-[9px] text-[var(--color-muted)]">
                  {result.smtp.connected ? "via" : "tried"} {result.smtp.tried_host}
                  {result.smtp.rcpt_code ? ` · RCPT ${result.smtp.rcpt_code}` : ""}
                </p>
              )}
            </div>
          )}

          {/* honest limits note */}
          <p className="text-[9px] leading-relaxed text-[var(--color-muted)]">
            Mailbox existence cannot always be verified because many mail servers
            intentionally prevent recipient enumeration and outbound port 25 is
            often blocked. This never claims a mailbox exists — only what the
            signals support.
          </p>

          {copied && <div className="text-[9px] text-[var(--color-accent)]">Zusammenfassung kopiert ✓</div>}
        </>
      )}
    </div>
  );
}

function SummaryRow({
  label,
  tone,
  value,
  plain,
}: {
  label: string;
  tone: Tone;
  value?: string;
  plain?: boolean;
}) {
  return (
    <div className="flex items-center justify-between gap-2">
      <span className="text-[var(--color-muted)]">{label}</span>
      <span className={`flex items-center gap-1.5 ${TONE_TEXT[tone]}`}>
        {value && <span className={plain ? "text-[var(--color-fg)]" : "font-[var(--font-mono)] text-[10px]"}>{value}</span>}
        {!plain && <span className="font-semibold">{toneGlyph(tone)}</span>}
      </span>
    </div>
  );
}
