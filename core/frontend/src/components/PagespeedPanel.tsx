import { useCallback, useEffect, useRef, useState } from "react";
import { Gauge, RefreshCw, Download, Monitor, Smartphone } from "lucide-react";
import {
  pagespeedAnalyze,
  pagespeedExport,
  type PageSpeedReport,
  type PsRun,
} from "../lib/ipc";
import { scoreBand, bandColor } from "../lib/pagespeed";

/**
 * `pagespeed <url>` — Google PageSpeed Insights in the preview column
 * (v0.142.0). Enter-activated: two cold Lighthouse runs are 10–40 s each.
 *
 * Desktop and mobile are shown TOGETHER, never one at a time — a page is
 * routinely fine on one and poor on the other, and seeing half of that
 * invites the wrong conclusion. The export puts both in one document too.
 */
export function PagespeedPanel({
  arg,
  focused,
  onExit,
}: {
  arg: string;
  focused: boolean;
  onExit: () => void;
}) {
  const [report, setReport] = useState<PageSpeedReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [exporting, setExporting] = useState<string | null>(null);
  const [exported, setExported] = useState<string | null>(null);
  const aliveRef = useRef(true);
  const seqRef = useRef(0);
  const scrollRef = useRef<HTMLDivElement>(null);

  const run = useCallback((url: string) => {
    if (!url.trim()) return;
    const seq = ++seqRef.current;
    setBusy(true);
    setError(null);
    pagespeedAnalyze(url)
      .then((r) => {
        if (!aliveRef.current || seq !== seqRef.current) return;
        setReport(r);
        setBusy(false);
      })
      .catch((e) => {
        if (!aliveRef.current || seq !== seqRef.current) return;
        setError(String(e));
        setBusy(false);
      });
  }, []);

  useEffect(() => {
    aliveRef.current = true;
    run(arg);
    return () => {
      aliveRef.current = false;
    };
  }, [arg, run]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      const tgt = e.target as HTMLElement | null;
      const typing =
        tgt && (tgt.tagName === "INPUT" || tgt.tagName === "TEXTAREA" || tgt.isContentEditable);
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
        return;
      }
      if (typing) return;
      const el = scrollRef.current;
      if (e.key === "r" || e.key === "R") {
        e.preventDefault();
        run(arg);
      } else if (e.key === "ArrowDown" && el) {
        e.preventDefault();
        el.scrollBy({ top: 64 });
      } else if (e.key === "ArrowUp" && el) {
        e.preventDefault();
        el.scrollBy({ top: -64 });
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit, run, arg]);

  return (
    <div
      ref={scrollRef}
      className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]"
    >
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2 text-[13px] font-medium">
          <Gauge size={15} className="shrink-0 text-[var(--color-accent)]" />
          <span className="truncate">PageSpeed</span>
          {report && (
            <span className="truncate text-[var(--color-muted)]">· {report.url}</span>
          )}
        </div>
        <button
          type="button"
          onClick={() => run(arg)}
          title="Neu messen (R)"
          className="shrink-0 rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
        >
          <RefreshCw size={13} className={busy ? "animate-spin" : undefined} />
        </button>
      </div>

      {busy && !report && (
        <div className="rounded-xl border border-[var(--color-border)] p-4 text-[12px]">
          <p className="font-medium">Messung läuft…</p>
          <p className="mt-1 text-[11px] leading-snug text-[var(--color-muted)]">
            Google fährt zwei Lighthouse-Läufe (Desktop und Mobil) parallel. Das dauert
            typischerweise 10–40 Sekunden.
          </p>
        </div>
      )}

      {error && (
        <div className="rounded-xl border border-[var(--color-border)] p-4">
          <p className="text-[12px] font-medium">Messung fehlgeschlagen</p>
          <p className="mt-1 text-[11px] leading-snug text-[var(--color-muted)]">{error}</p>
        </div>
      )}

      {report && (
        <>
          {report.errors.length > 0 && (
            <div className="rounded-lg border border-amber-500/40 bg-amber-500/10 p-2.5 text-[11px] leading-snug text-amber-500">
              {report.errors.map((e) => (
                <p key={e}>{e}</p>
              ))}
            </div>
          )}
          <ExportRow
            busy={exporting}
            done={exported}
            disabled={!report.desktop && !report.mobile}
            onExport={(fmt) => {
              setExporting(fmt);
              setExported(null);
              pagespeedExport(report, fmt)
                .then((p) => setExported(p.split("/").pop() ?? p))
                .catch((e) => setExported(String(e)))
                .finally(() => setExporting(null));
            }}
          />
          {report.desktop && <StrategyCard run={report.desktop} kind="desktop" />}
          {report.mobile && <StrategyCard run={report.mobile} kind="mobile" />}
        </>
      )}

      {focused && (
        <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">
          R neu messen · ↑↓ scrollen · Esc schließen
        </p>
      )}
    </div>
  );
}

function StrategyCard({ run, kind }: { run: PsRun; kind: "desktop" | "mobile" }) {
  const Icon = kind === "desktop" ? Monitor : Smartphone;
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
      <p className="mb-3 flex items-center gap-1.5 text-[11px] font-medium">
        <Icon size={12} className="text-[var(--color-muted)]" />
        {kind === "desktop" ? "Desktop" : "Mobil"}
      </p>
      <div className="mb-3 flex flex-wrap gap-3">
        {run.categories.map((c) => (
          <ScoreRing key={c.id} label={c.label} score={c.score} />
        ))}
      </div>
      <div className="flex flex-col gap-0.5">
        {run.metrics.map((m) => (
          <div key={m.id} className="flex items-center gap-2 text-[11px]">
            <span
              className="h-2 w-2 shrink-0 rounded-sm"
              style={{ background: bandColor(scoreBand(m.score)) }}
            />
            <span className="min-w-0 flex-1 truncate text-[var(--color-muted)]">{m.label}</span>
            <span className="shrink-0 tabular-nums font-medium">{m.display}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

/** Lighthouse's ring. SVG, so it renders the same here and in the export. */
function ScoreRing({ label, score }: { label: string; score: number | null }) {
  const col = bandColor(scoreBand(score));
  const r = 22;
  const circ = 2 * Math.PI * r;
  const filled = ((score ?? 0) / 100) * circ;
  return (
    <div className="w-[68px] text-center">
      <svg viewBox="0 0 56 56" width={56} height={56} aria-hidden>
        <circle cx="28" cy="28" r={r} fill="none" stroke={col} strokeOpacity={0.18} strokeWidth={5} />
        <circle
          cx="28"
          cy="28"
          r={r}
          fill="none"
          stroke={col}
          strokeWidth={5}
          strokeLinecap="round"
          strokeDasharray={`${filled} ${circ}`}
          transform="rotate(-90 28 28)"
        />
        <text x="28" y="33" textAnchor="middle" fontSize="15" fontWeight="600" fill={col}>
          {score ?? "–"}
        </text>
      </svg>
      <span className="block text-[10px] leading-tight text-[var(--color-muted)]">{label}</span>
    </div>
  );
}

/** Same shape as the loc export row — one habit for both reports. */
function ExportRow({
  busy,
  done,
  disabled,
  onExport,
}: {
  busy: string | null;
  done: string | null;
  disabled: boolean;
  onExport: (fmt: "html" | "pdf") => void;
}) {
  const formats: Array<"html" | "pdf"> = ["html", "pdf"];
  return (
    <div className="flex flex-wrap items-center gap-1.5 text-[11px]">
      <Download size={12} className="shrink-0 text-[var(--color-muted)]" />
      <span className="text-[var(--color-muted)]">Export (Desktop + Mobil):</span>
      {formats.map((f) => (
        <button
          key={f}
          type="button"
          disabled={disabled || busy !== null}
          onClick={() => onExport(f)}
          className="rounded-full border border-[var(--color-border)] px-2 py-0.5 uppercase tracking-wide hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-40"
        >
          {busy === f ? "…" : f}
        </button>
      ))}
      {done && <span className="truncate text-[var(--color-muted)]">→ {done}</span>}
    </div>
  );
}
