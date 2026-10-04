/**
 * `limits` / `quota` — the Claude subscription's usage limits (session, week,
 * per-model week, extra usage) as claude.ai shows them. Data and caching live
 * in `claude_limits.rs`: this panel asks every 30 s while it is open, and Rust
 * only touches the network when the poll interval is due. R forces a refresh,
 * Esc exits.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { AlertCircle, Gauge, Loader2, RefreshCw } from "lucide-react";
import {
  claudeLimitsStatus,
  setClaudeLimitsPoll,
  type ClaudeLimit,
  type ClaudeLimitsStatus,
} from "../lib/ipc";
import {
  POLL_CHOICES,
  absoluteReset,
  barFill,
  formatMoney,
  formatPercent,
  limitTone,
  limitsErrorText,
  limitsPhase,
  relativeReset,
  sinceLabel,
  standLabel,
  type LimitTone,
} from "../lib/claude-limits";

const ASK_EVERY_MS = 30_000;

const TONE_BAR: Record<LimitTone, string> = {
  ok: "bg-[color:var(--color-accent)]",
  warn: "bg-amber-500",
  crit: "bg-rose-500",
};

function LimitRow({ l, now }: { l: ClaudeLimit; now: number }) {
  const reset = l.resets_at ? Date.parse(l.resets_at) : NaN;
  return (
    <div className="rounded-lg bg-[color:var(--color-surface)] px-3 py-2">
      <div className="flex items-baseline justify-between gap-2">
        <div className="min-w-0 truncate font-medium text-[color:var(--color-fg)]">
          {l.name}
          {!l.known && (
            <span
              className="ml-1.5 rounded bg-[color:var(--color-bg)] px-1 text-[10px] font-normal text-[color:var(--color-muted)]"
              title="Von dieser Version nicht erkannter Limit-Typ — Rohname der API"
            >
              unbekannt
            </span>
          )}
        </div>
        <div className="shrink-0 font-mono text-[12px] tabular-nums text-[color:var(--color-fg)]">
          {formatPercent(l.percent)}
        </div>
      </div>
      <div className="mt-1.5 h-1.5 overflow-hidden rounded-full bg-[color:var(--color-bg)]">
        <div
          className={`h-full origin-left rounded-full transition-transform duration-(--duration-slow) ease-sharp ${TONE_BAR[limitTone(l.percent)]}`}
          style={{ transform: `scaleX(${barFill(l.percent) / 100})` }}
        />
      </div>
      <div className="mt-1 flex justify-between gap-2 text-[11px] text-[color:var(--color-muted)]">
        <span>
          {Number.isFinite(reset)
            ? `Zurückgesetzt ${relativeReset(reset, now)}`
            : "Kein Rücksetzzeitpunkt"}
        </span>
        <span className="tabular-nums">
          {l.money
            ? `${formatMoney(l.money.used, l.money.currency)} / ${formatMoney(l.money.limit, l.money.currency)}`
            : Number.isFinite(reset)
              ? absoluteReset(reset)
              : ""}
        </span>
      </div>
      {l.money && Number.isFinite(reset) && (
        <div className="text-right text-[11px] tabular-nums text-[color:var(--color-muted)]">
          {absoluteReset(reset)}
        </div>
      )}
    </div>
  );
}

function SectionTitle({ title, note }: { title: string; note?: string }) {
  return (
    <div className="flex items-baseline justify-between px-1 pt-2 text-[11px] font-semibold uppercase tracking-wide text-[color:var(--color-muted)]">
      <span>{title}</span>
      {note && <span className="font-normal normal-case tracking-normal">{note}</span>}
    </div>
  );
}

export function ClaudeLimitsPanel({
  focused,
  onExit,
}: {
  focused: boolean;
  onExit: () => void;
}) {
  const [st, setSt] = useState<ClaudeLimitsStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const seq = useRef(0);
  const scrollRef = useRef<HTMLDivElement>(null);

  const load = useCallback(async (force: boolean) => {
    const my = ++seq.current;
    setLoading(true);
    try {
      const s = await claudeLimitsStatus(force);
      if (my === seq.current) setSt(s);
    } catch (e) {
      if (my === seq.current)
        setSt((prev) => ({
          report: prev?.report ?? null,
          fetched_at_ms: prev?.fetched_at_ms ?? null,
          error: String(e),
          error_detail: null,
          retry_at_ms: null,
          poll_minutes: prev?.poll_minutes ?? 5,
          codex: prev?.codex ?? null,
          antigravity: prev?.antigravity ?? null,
        }));
    } finally {
      if (my === seq.current) setLoading(false);
      setNow(Date.now());
    }
  }, []);

  // Ask on open and every 30 s while open. Rust decides whether that is a
  // network request or the cache.
  useEffect(() => {
    void load(false);
    const t = window.setInterval(() => void load(false), ASK_EVERY_MS);
    return () => window.clearInterval(t);
  }, [load]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onExit();
        return;
      }
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable))
        return;
      if (e.key === "r" || e.key === "R") {
        e.preventDefault();
        void load(true);
      } else if (e.key === "ArrowDown" || e.key === "PageDown") {
        e.preventDefault();
        scrollRef.current?.scrollBy({ top: e.key === "PageDown" ? 180 : 48 });
      } else if (e.key === "ArrowUp" || e.key === "PageUp") {
        e.preventDefault();
        scrollRef.current?.scrollBy({ top: e.key === "PageUp" ? -180 : -48 });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focused, onExit, load]);

  const report = st?.report ?? null;
  const phase = limitsPhase(!!report, st?.error ?? null, st?.fetched_at_ms ?? null, st?.poll_minutes ?? 5, now);
  const errText = limitsErrorText(st?.error ?? null);
  const year = new Date(now).getFullYear();

  return (
    <div className="flex h-full min-h-0 flex-col gap-2 p-3 text-[13px]">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5 font-medium text-[color:var(--color-fg)]">
          <Gauge size={14} className="text-rose-500" />
          Claude-Limits
        </div>
        <div className="flex items-center gap-1.5 text-[11px] text-[color:var(--color-muted)]">
          {phase === "veraltet" && (
            <span className="rounded bg-amber-500/15 px-1.5 py-0.5 text-amber-600">veraltet</span>
          )}
          {st?.fetched_at_ms != null && <span>{standLabel(st.fetched_at_ms)}</span>}
          <select
            value={st?.poll_minutes ?? 5}
            onChange={async (e) => {
              await setClaudeLimitsPoll(Number(e.target.value));
              void load(false);
            }}
            className="rounded bg-[color:var(--color-surface)] px-1 py-0.5 text-[11px] text-[color:var(--color-fg)]"
            title="Abfrage-Intervall"
            aria-label="Abfrage-Intervall"
          >
            {POLL_CHOICES.map((m) => (
              <option key={m} value={m}>
                alle {m} min
              </option>
            ))}
          </select>
          <button
            type="button"
            onClick={() => void load(true)}
            className="md3-press rounded-md p-1 hover:bg-[color:var(--color-surface)]"
            title="Aktualisieren (R)"
            aria-label="Aktualisieren"
          >
            <RefreshCw size={13} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
      </div>

      {errText && (
        <div
          className={`flex items-start gap-1.5 rounded-lg px-2.5 py-1.5 text-[12px] ${
            phase === "fehler" ? "bg-rose-500/10 text-rose-600" : "bg-amber-500/10 text-amber-700"
          }`}
        >
          <AlertCircle size={13} className="mt-0.5 shrink-0" />
          <span>
            {errText}
            {phase === "veraltet" && st?.fetched_at_ms != null && ` Angezeigt: ${standLabel(st.fetched_at_ms)}.`}
            {st?.retry_at_ms != null && ` Nächster Versuch ${relativeReset(st.retry_at_ms, now)}.`}
          </span>
        </div>
      )}

      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto pr-0.5">
        {phase === "laden" && (
          <div className="flex items-center gap-2 py-6 text-[color:var(--color-muted)]">
            <Loader2 size={14} className="animate-spin" /> lädt…
          </div>
        )}
        {(report || st?.codex || st?.antigravity) && (
          <SectionTitle title="Claude" />
        )}
        {report && (
          <div className="flex flex-col gap-1.5">
            {report.limits.map((l) => (
              <LimitRow key={l.id} l={l} now={now} />
            ))}
            {report.limits.length === 0 && (
              <div className="py-4 text-[color:var(--color-muted)]">Die API meldet keine Limits.</div>
            )}

            {report.extra && (
              <div className="rounded-lg bg-[color:var(--color-surface)] px-3 py-2 text-[12px]">
                <div className="flex justify-between">
                  <span className="font-medium text-[color:var(--color-fg)]">Zusätzliche Nutzung</span>
                  <span className="text-[color:var(--color-muted)]">
                    {report.extra.enabled ? "aktiv" : "aus"}
                  </span>
                </div>
                <div className="mt-0.5 tabular-nums text-[color:var(--color-muted)]">
                  {formatMoney(report.extra.used, report.extra.currency)} von{" "}
                  {formatMoney(report.extra.limit, report.extra.currency)} · {formatPercent(report.extra.percent)}
                </div>
              </div>
            )}

            {report.breakdown.length > 0 && (
              <div className="px-1 pt-1 text-[11px] text-[color:var(--color-muted)]">
                Wochennutzung nach Bereich:{" "}
                {report.breakdown.map((b) => `${b.name} ${formatPercent(b.percent)}`).join(" · ")}
              </div>
            )}
            {report.legacy && (
              <div className="px-1 text-[11px] text-[color:var(--color-muted)]">
                Ältere API-Form (five_hour / seven_day) — weniger Details.
              </div>
            )}
          </div>
        )}

        {st?.codex && (
          <div className="flex flex-col gap-1.5">
            <SectionTitle
              title={`Codex${st.codex.plan ? ` · ${st.codex.plan}` : ""}`}
              note={
                st.codex.as_of
                  ? `Stand ${sinceLabel(Date.parse(st.codex.as_of), now)} (letzte Codex-Nutzung)`
                  : undefined
              }
            />
            {st.codex.limits.map((l) => (
              <LimitRow key={l.id} l={l} now={now} />
            ))}
          </div>
        )}

        {st?.antigravity && (
          <div className="flex flex-col gap-1.5">
            <SectionTitle title="Antigravity" />
            <div className="rounded-lg bg-[color:var(--color-surface)] px-3 py-2 text-[12px]">
              {st.antigravity.blocked && st.antigravity.resets_at ? (
                <>
                  <div className="font-medium text-rose-600">Kontingent erschöpft</div>
                  <div className="mt-0.5 text-[color:var(--color-muted)]">
                    Wieder frei {relativeReset(Date.parse(st.antigravity.resets_at), now)} ·{" "}
                    {absoluteReset(Date.parse(st.antigravity.resets_at))}
                  </div>
                </>
              ) : (
                <div className="text-[color:var(--color-fg)]">Keine aktive Sperre bekannt</div>
              )}
              <div className="mt-1 text-[11px] text-[color:var(--color-muted)]">
                Antigravity zeigt lokal keine Prozente — nur, wann ein Limit erreicht wurde.
              </div>
            </div>
          </div>
        )}
      </div>

      <div className="flex justify-between border-t border-[color:var(--color-border)] pt-1.5 text-[10px] text-[color:var(--color-muted)]">
        <span>Inoffizielle Schnittstelle · R aktualisiert · Esc zurück</span>
        <span>© {year} Martin Pfeffer | celox.io</span>
      </div>
    </div>
  );
}
