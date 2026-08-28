import { useCallback, useEffect, useRef, useState } from "react";
import { GitBranch, RefreshCw, Users, GitCommit, Flame, CalendarDays } from "lucide-react";
import { repoAnalyze, repoExport, type RepoStats } from "../lib/ipc";
import { ExportRow, type ExportFormat } from "./ExportRow";
import {
  WEEKDAY_LABELS,
  categoryColor,
  formatNum,
  shortDate,
  barPct,
  peakLabel,
  sparkPoints,
  totalChurn,
} from "../lib/repo";

/**
 * `repo` / `export` — git repository activity stats in the preview column
 * (v0.123.0), oriented on the repo2viz project. Enter-activated (it clones /
 * runs git). Shows KPIs, weekday + hour charts, a month timeline sparkline,
 * commit-category bars, and top files/types/authors. `export` (or the button)
 * writes the same analysis as a self-contained HTML to ~/Downloads.
 */
export function RepoPanel({
  arg,
  autoExport,
  focused,
  onExit,
}: {
  /** Repo URL / local path; blank = the Finder-selected .git folder. */
  arg: string;
  /** When true (the `export` command), export immediately after analysing. */
  autoExport: boolean;
  focused: boolean;
  onExit: () => void;
}) {
  const [stats, setStats] = useState<RepoStats | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const [phase, setPhase] = useState("Analysiere…");
  const [note, setNote] = useState<string | null>(null);
  const aliveRef = useRef(true);
  const seqRef = useRef(0);
  const scrollRef = useRef<HTMLDivElement>(null);

  const target = arg.trim() ? arg.trim() : null;

  const run = useCallback(() => {
    const seq = ++seqRef.current;
    setBusy(true);
    setErr(null);
    setPhase(target && /:\/\//.test(target) ? "Klone & analysiere…" : "Analysiere…");
    repoAnalyze(target)
      .then((s) => {
        if (!aliveRef.current || seq !== seqRef.current) return;
        setStats(s);
        setBusy(false);
      })
      .catch((e) => {
        if (!aliveRef.current || seq !== seqRef.current) return;
        setErr(String(e));
        setBusy(false);
      });
  }, [target]);

  useEffect(() => {
    aliveRef.current = true;
    run();
    return () => {
      aliveRef.current = false;
    };
  }, [run]);

  const [exporting, setExporting] = useState<string | null>(null);
  const doExport = useCallback(
    (fmt: ExportFormat = "html") => {
      setExporting(fmt);
      setNote("Exportiere…");
      repoExport(target, fmt === "pdf" ? "pdf" : "html")
        .then((path) => setNote(`Gespeichert: ${path.split("/").pop()}`))
        .catch((e) => setNote(String(e)))
        .finally(() => setExporting(null));
    },
    [target],
  );

  // Auto-export once the analysis is on screen (the `export` command path).
  const exportedRef = useRef(false);
  useEffect(() => {
    if (autoExport && stats && !exportedRef.current) {
      exportedRef.current = true;
      doExport();
    }
  }, [autoExport, stats, doExport]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      // ⚠️ The export shortcuts are CHORDS, not bare letters. A bare `E`/`P`
      // was unreachable: this panel never takes DOM focus (no tabIndex, no
      // `.focus()`), so the search field keeps it and the `typing` guard below
      // was permanently true — `E` had been advertised in the button tooltip
      // and could never fire. Making bare letters work instead would recreate
      // the weather bug: `repo` keeps its argument editable, and a `p` typed
      // into a path would silently start an export.
      const tgt = e.target as HTMLElement | null;
      const typing = tgt && (tgt.tagName === "INPUT" || tgt.isContentEditable);
      const chord = e.metaKey || e.ctrlKey;
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
      } else if (chord && (e.key === "e" || e.key === "E")) {
        e.preventDefault();
        doExport("html");
      } else if (chord && (e.key === "p" || e.key === "P")) {
        e.preventDefault();
        doExport("pdf");
      } else if (!typing && (e.key === "e" || e.key === "E")) {
        // Still honoured when focus is genuinely off the search field.
        e.preventDefault();
        doExport("html");
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit, doExport]);

  if (busy && !stats) {
    return (
      <Shell focused={focused}>
        <div className="flex flex-col items-center gap-2 rounded-xl border border-[var(--color-border)] p-6">
          <div className="disk-scan-orb" aria-hidden />
          <p className="text-[12px] font-medium">{phase}</p>
          <p className="text-[11px] text-[var(--color-muted)]">Große Repos brauchen einen Moment.</p>
        </div>
      </Shell>
    );
  }
  if (err) {
    const noTarget = err.includes("repo.no_target");
    return (
      <Shell focused={focused}>
        <div className="rounded-xl border border-[var(--color-border)] p-4">
          <p className="text-[12px] font-medium">{noTarget ? "Kein Repository." : "Analyse fehlgeschlagen"}</p>
          <p className="mt-1 text-[11px] leading-snug text-[var(--color-muted)]">
            {noTarget
              ? "Eine GitHub-URL angeben (repo https://github.com/user/projekt) — oder im Finder einen Ordner mit .git auswählen."
              : err}
          </p>
        </div>
      </Shell>
    );
  }
  if (!stats) return <Shell focused={focused}>{null}</Shell>;

  const churn = totalChurn(stats.insertions, stats.deletions);
  const wdMax = Math.max(1, ...stats.by_weekday);
  const hrMax = Math.max(1, ...stats.by_hour);
  const catMax = Math.max(1, ...stats.categories.map((c) => c.commits));
  const fileMax = Math.max(1, ...stats.top_files.map((f) => f.changes));

  return (
    <div ref={scrollRef} className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]">
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2 text-[13px] font-medium">
          <GitBranch size={15} className="shrink-0 text-[var(--color-accent)]" />
          <span className="truncate" title={stats.source}>{stats.name}</span>
        </div>
        <div className="flex items-center gap-1">
          <button type="button" onClick={run} title="Neu analysieren" className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]">
            <RefreshCw size={13} className={busy ? "animate-spin" : undefined} />
          </button>
        </div>
      </div>
      <p className="-mt-1 text-[10px] text-[var(--color-muted)]">
        {shortDate(stats.first_commit)} → {shortDate(stats.last_commit)}
      </p>
      {note && <p className="text-[11px] text-emerald-500">{note}</p>}
      <ExportRow
        formats={["html", "pdf"]}
        busy={exporting}
        done={null}
        onExport={(f) => doExport(f)}
      />

      {/* KPI tiles. */}
      <div className="grid grid-cols-3 gap-1.5">
        <Kpi icon={<GitCommit size={12} />} value={formatNum(stats.commits)} label="Commits" />
        <Kpi icon={<Users size={12} />} value={formatNum(stats.contributors)} label="Mitwirkende" />
        <Kpi icon={<CalendarDays size={12} />} value={formatNum(stats.active_days)} label="Aktive Tage" />
        <Kpi icon={<Flame size={12} />} value={formatNum(stats.longest_streak)} label="Längste Serie" />
        <Kpi value={`+${formatNum(stats.insertions)}`} label="Zeilen ein" tone="pos" />
        <Kpi value={`−${formatNum(stats.deletions)}`} label="Zeilen aus" tone="neg" />
      </div>

      {/* Month timeline sparkline. */}
      {stats.by_month.length > 1 && (
        <Card title={`Aktivität · ${stats.by_month.length} Monate`}>
          <svg viewBox="0 0 300 44" preserveAspectRatio="none" className="h-11 w-full">
            <polyline
              points={sparkPoints(stats.by_month.map((m) => m.commits), 300, 44)}
              fill="none"
              stroke="var(--color-accent)"
              strokeWidth={1.5}
              vectorEffect="non-scaling-stroke"
            />
          </svg>
          <div className="mt-0.5 flex justify-between text-[10px] text-[var(--color-muted)]">
            <span>{stats.by_month[0].month}</span>
            <span>{stats.by_month[stats.by_month.length - 1].month}</span>
          </div>
        </Card>
      )}

      {/* Weekday + hour. */}
      <Card title={`Wochentag · Spitze ${peakLabel(stats.by_weekday, WEEKDAY_LABELS)}`}>
        {/* ⚠️ NICHT `items-end`: das ließe jede Spalte auf Inhaltshöhe
            schrumpfen, und die Prozenthöhe des Balkens hätte keinen definiten
            Bezug mehr — sie fiel auf 0, das Diagramm war leer (seit ≤ v0.138.0,
            auch im Galerie-Screenshot). Die Spalten müssen die 56 px füllen;
            unten ausgerichtet wird INNERHALB von `Column`. */}
        <div className="flex gap-1" style={{ height: 56 }}>
          {stats.by_weekday.map((v, i) => (
            <Column key={i} pct={barPct(v, wdMax)} label={WEEKDAY_LABELS[i]} value={v} color="#8ab4f8" />
          ))}
        </div>
      </Card>
      <Card title={`Uhrzeit · Spitze ${peakLabel(stats.by_hour, HOUR_LABELS)} Uhr`}>
        <div className="flex gap-[2px]" style={{ height: 48 }}>
          {stats.by_hour.map((v, i) => (
            <Column key={i} pct={barPct(v, hrMax)} label={i % 6 === 0 ? String(i) : ""} value={v} color="#c58af9" thin />
          ))}
        </div>
      </Card>

      {/* Commit categories. */}
      {stats.categories.length > 0 && (
        <Card title="Commit-Kategorien">
          <div className="flex flex-col gap-1">
            {stats.categories.map((c) => (
              <div key={c.cat} className="flex items-center gap-2 text-[11px]">
                <span className="w-16 shrink-0 text-[var(--color-muted)]">{c.cat}</span>
                <span className="relative h-2.5 flex-1 overflow-hidden rounded-full bg-[var(--color-border)]">
                  <span className="absolute inset-y-0 left-0 rounded-full" style={{ width: `${barPct(c.commits, catMax)}%`, background: categoryColor(c.cat) }} />
                </span>
                <span className="w-10 shrink-0 text-right tabular-nums text-[var(--color-muted)]">{formatNum(c.commits)}</span>
              </div>
            ))}
          </div>
        </Card>
      )}

      {/* Top files. */}
      <Card title="Aktivste Dateien">
        <RankList
          rows={stats.top_files.slice(0, 10).map((f) => ({ label: f.path, bar: barPct(f.changes, fileMax), value: `${f.changes}× · ${formatNum(f.churn)}` }))}
          mono
        />
      </Card>

      {/* Extensions + authors, two compact tables. */}
      <div className="grid grid-cols-2 gap-2">
        <Card title="Dateitypen">
          <div className="flex flex-col gap-0.5 text-[11px]">
            {stats.top_exts.slice(0, 8).map((e) => (
              <div key={e.ext} className="flex justify-between gap-2">
                <span className="truncate font-[var(--font-mono)]">.{e.ext}</span>
                <span className="shrink-0 tabular-nums text-[var(--color-muted)]">{formatNum(e.churn)}</span>
              </div>
            ))}
          </div>
        </Card>
        <Card title="Mitwirkende">
          <div className="flex flex-col gap-0.5 text-[11px]">
            {stats.top_authors.slice(0, 8).map((a) => (
              <div key={a.name} className="flex justify-between gap-2">
                <span className="truncate" title={a.name}>{a.name}</span>
                <span className="shrink-0 tabular-nums text-[var(--color-muted)]">{formatNum(a.commits)}</span>
              </div>
            ))}
          </div>
        </Card>
      </div>

      <p className="text-[10px] text-[var(--color-muted)]">
        {formatNum(churn)} Zeilen bewegt · ⌀ {stats.avg_msg_len} Zeichen/Message · orientiert an repo2viz
      </p>
      {focused && (
        <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">E = HTML-Export · Esc schließen</p>
      )}
    </div>
  );
}

const HOUR_LABELS = Array.from({ length: 24 }, (_, i) => String(i));

function Shell({ focused, children }: { focused: boolean; children: React.ReactNode }) {
  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]">
      <div className="flex items-center gap-2 text-[13px] font-medium">
        <GitBranch size={15} className="text-[var(--color-accent)]" /> Repository
      </div>
      {children}
      {focused && <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">Esc schließen</p>}
    </div>
  );
}

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
      <p className="mb-2 text-[11px] font-medium">{title}</p>
      {children}
    </div>
  );
}

function Kpi({ icon, value, label, tone }: { icon?: React.ReactNode; value: string; label: string; tone?: "pos" | "neg" }) {
  return (
    <div className="rounded-lg border border-[var(--color-border)] px-2 py-1.5 text-center [contain:content]">
      <div
        className="flex items-center justify-center gap-1 text-[14px] font-semibold tabular-nums"
        style={{ color: tone === "pos" ? "#81c995" : tone === "neg" ? "#f28b82" : undefined }}
      >
        {icon && <span className="text-[var(--color-accent)]">{icon}</span>}
        {value}
      </div>
      <div className="text-[10px] text-[var(--color-muted)]">{label}</div>
    </div>
  );
}

function Column({ pct, label, value, color, thin }: { pct: number; label: string; value: number; color: string; thin?: boolean }) {
  return (
    <div className="flex flex-1 flex-col items-center gap-1" title={`${label || ""}: ${value}`}>
      <div className="flex w-full flex-1 items-end">
        <div className="w-full rounded-t" style={{ height: `${Math.max(2, pct)}%`, background: color, minWidth: thin ? 2 : undefined }} />
      </div>
      <span className="text-[9px] text-[var(--color-muted)]">{label}</span>
    </div>
  );
}

function RankList({ rows, mono }: { rows: { label: string; bar: number; value: string }[]; mono?: boolean }) {
  return (
    <div className="flex flex-col gap-1">
      {rows.map((r, i) => (
        <div key={i} className="flex items-center gap-2 text-[11px]">
          <div className="relative min-w-0 flex-1">
            <div className="absolute inset-y-0 left-0 rounded bg-[var(--color-accent)] opacity-15" style={{ width: `${r.bar}%` }} />
            <span className={"relative block truncate px-1 py-0.5 " + (mono ? "font-[var(--font-mono)]" : "")} title={r.label}>
              {r.label}
            </span>
          </div>
          <span className="shrink-0 tabular-nums text-[var(--color-muted)]">{r.value}</span>
        </div>
      ))}
    </div>
  );
}
