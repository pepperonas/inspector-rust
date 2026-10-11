/**
 * `github` — GitHub analytics panel (v0.209.0): followers, stars, repository
 * ranking and their history, from local snapshots in `history.db`.
 *
 * Shown while the command is typed and rendered from the local cache only —
 * a refresh never sits on the render path. On open it asks for a refresh,
 * which the backend gates by the interval and the rate limit. Actions
 * (refresh, export, connect) run on Enter via `runSignal`. Pure logic:
 * `lib/github.ts`. Reference: docs/github.md.
 */
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { AlertTriangle, Copy, Download, ExternalLink, Loader2, Lock, RefreshCw, Star } from "lucide-react";
import {
  githubDashboard,
  githubDeviceCancel,
  githubDeviceStart,
  githubExport,
  githubRefresh,
  githubRepo,
  githubStatus,
  type GithubDashboard,
  type GithubDeltas,
  type GithubKpi,
  type GithubPoint,
  type GithubRepoDetail,
  type GithubRepoRow,
  type GithubSortKey,
  type GithubStatus,
} from "../lib/ipc";
import {
  NO_LANGUAGE,
  RANGES,
  SORTS,
  SOURCE_LABEL,
  deltaTone,
  errorText,
  formatIn,
  formatDate,
  formatDelta,
  historyCollecting,
  activityByMonth,
  languageShares,
  metricBars,
  statusLine,
  timeDomain,
  toSeries,
  type GithubCmd,
  type GithubRange,
} from "../lib/github";
import { areaPath, linePath, seriesExtent } from "../lib/stats-chart";
import { donutSegments } from "../lib/loc";

/** Signals already handled — module-level so a remount never replays one. */
let lastHandledRun = 0;
/** The range survives closing the panel (per session). */
let rememberedRange: GithubRange = "30d";

const KPI_LABEL: Record<GithubKpi["key"], string> = {
  followers: "Followers",
  stars: "Total Stars",
  repos: "Repositories",
  forks: "Forks",
};
const LINE_COLORS = ["var(--color-accent)", "#f59e0b", "#10b981", "#ec4899", "#8b5cf6"];

/** The GitHub mark (Simple Icons; lucide has no brand set). */
function GithubMark({ size }: { size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
    </svg>
  );
}

interface Props {
  cmd: GithubCmd;
  focused: boolean;
  runSignal: number;
  onExit: () => void;
  onQuery: (q: string) => void;
}

function Delta({ label, d }: { label: string; d: number | null }) {
  const tone = deltaTone(d);
  const cls =
    tone === "up"
      ? "text-emerald-500"
      : tone === "down"
        ? "text-rose-500"
        : "text-[var(--color-muted)]";
  return (
    <span className="tabular-nums" title={d == null ? "Insufficient history" : undefined}>
      <span className="text-[var(--color-muted)]">{label} </span>
      <span className={cls}>{formatDelta(d)}</span>
    </span>
  );
}

function DeltaRow({ d }: { d: GithubDeltas }) {
  if (historyCollecting(d)) {
    return <div className="text-[10px] text-[var(--color-muted)]">History collecting…</div>;
  }
  return (
    <div className="flex flex-wrap gap-x-2 text-[10px]">
      <Delta label="24h" d={d.d24h} />
      <Delta label="7d" d={d.d7d} />
      <Delta label="30d" d={d.d30d} />
      <Delta label="90d" d={d.d90d} />
    </div>
  );
}

function KpiCard({ kpi }: { kpi: GithubKpi }) {
  const d30 = kpi.deltas.d30d;
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2" data-testid="gh-kpi">
      <div className="text-[10px] uppercase tracking-wide text-[var(--color-muted)]">{KPI_LABEL[kpi.key]}</div>
      <div className="flex items-baseline gap-2">
        <span className="text-lg font-semibold tabular-nums text-[var(--color-fg)]">
          {kpi.value.toLocaleString("en-US")}
        </span>
        {d30 != null && (
          <span className={`text-xs tabular-nums ${deltaTone(d30) === "down" ? "text-rose-500" : "text-emerald-500"}`}>
            {formatDelta(d30)} <span className="text-[var(--color-muted)]">30d</span>
          </span>
        )}
      </div>
      <DeltaRow d={kpi.deltas} />
    </div>
  );
}

const CHART_W = 300;
const CHART_H = 70;

/** Real snapshots only: one point is not a line, so it says so. */
function LineChart({
  title,
  lines,
}: {
  title: string;
  lines: { name: string; points: GithubPoint[] }[];
}) {
  const series = lines.map((l) => toSeries(l.points));
  const domain = timeDomain(series);
  const enough = series.some((s) => s.length >= 2);
  const values = series.flat().map((p) => p.v);
  const [vMin, vMax] = seriesExtent(values);
  // Labels name the real extremes, not the padded drawing range.
  const dataMin = values.length ? Math.min(...values) : 0;
  const dataMax = values.length ? Math.max(...values) : 0;
  const current = lines.length === 1 ? lines[0].points[lines[0].points.length - 1]?.[1] : undefined;
  const last = series.length === 1 ? series[0][series[0].length - 1] : undefined;
  const gradId = `gh-grad-${useId().replace(/:/g, "")}`;
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2" data-testid="gh-chart">
      <div className="mb-1 flex items-center justify-between text-xs">
        <span className="text-[var(--color-muted)]">{title}</span>
        {current != null && <span className="tabular-nums text-[var(--color-fg)]">{current.toLocaleString("en-US")}</span>}
      </div>
      {!enough || !domain ? (
        <div className="flex h-[70px] items-center justify-center text-xs text-[var(--color-muted)]">
          History collecting… ({values.length} {values.length === 1 ? "snapshot" : "snapshots"})
        </div>
      ) : (
        <div className="relative">
          <svg viewBox={`0 0 ${CHART_W} ${CHART_H}`} preserveAspectRatio="none" className="h-[70px] w-full" data-testid="gh-chart-svg">
            <defs>
              <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor={LINE_COLORS[0]} stopOpacity={0.35} />
                <stop offset="100%" stopColor={LINE_COLORS[0]} stopOpacity={0} />
              </linearGradient>
            </defs>
            {[0.25, 0.5, 0.75].map((f) => (
              <line key={f} x1={0} x2={CHART_W} y1={CHART_H * f} y2={CHART_H * f} stroke="var(--color-border)" strokeWidth={1} vectorEffect="non-scaling-stroke" strokeDasharray="2 3" />
            ))}
            {lines.length === 1 && (
              <path d={areaPath(series[0], domain[0], domain[1], CHART_W, CHART_H, vMin, vMax)} fill={`url(#${gradId})`} />
            )}
            {series.map((s, i) => (
              <path
                key={lines[i].name}
                d={linePath(s, domain[0], domain[1], CHART_W, CHART_H, vMin, vMax)}
                fill="none"
                stroke={LINE_COLORS[i % LINE_COLORS.length]}
                strokeWidth={1.8}
                strokeLinejoin="round"
                vectorEffect="non-scaling-stroke"
              />
            ))}
          </svg>
          {/* HTML overlays: the SVG is stretched, so text and dots would distort. */}
          {lines.length === 1 && last && (
            <span
              className="pointer-events-none absolute h-2 w-2 -translate-x-1/2 -translate-y-1/2 rounded-full ring-2 ring-[var(--color-surface)]"
              style={{
                left: `${((last.t - domain[0]) / (domain[1] - domain[0] || 1)) * 100}%`,
                top: `${(1 - (last.v - vMin) / (vMax - vMin || 1)) * 100}%`,
                background: LINE_COLORS[0],
              }}
            />
          )}
          <span className="pointer-events-none absolute left-0 top-0 rounded bg-[var(--color-surface)]/80 px-0.5 text-[9px] tabular-nums text-[var(--color-muted)]">
            max {fmtAxis(dataMax)}
          </span>
          <span className="pointer-events-none absolute bottom-0 right-0 rounded bg-[var(--color-surface)]/80 px-0.5 text-[9px] tabular-nums text-[var(--color-muted)]">
            min {fmtAxis(dataMin)}
          </span>
        </div>
      )}
      {enough && domain && (
        <div className="mt-0.5 flex justify-between text-[9px] text-[var(--color-muted)]">
          <span>{formatDate(domain[0] * 1000)}</span>
          <span>{formatDate(domain[1] * 1000)}</span>
        </div>
      )}
      {lines.length > 1 && (
        <div className="mt-1 flex flex-wrap gap-x-2 text-[10px] text-[var(--color-muted)]">
          {lines.map((l, i) => (
            <span key={l.name} className="flex items-center gap-1">
              <span className="inline-block h-1.5 w-3 rounded" style={{ background: LINE_COLORS[i % LINE_COLORS.length] }} />
              {l.name}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

/** Axis label: whole numbers stay whole, large ones get a k. */
function fmtAxis(v: number): string {
  if (Math.abs(v) >= 10_000) return `${(v / 1000).toFixed(0)}k`;
  if (Math.abs(v) >= 1000) return `${(v / 1000).toFixed(1)}k`;
  return Number.isInteger(v) ? String(v) : v.toFixed(1);
}

function Card({ title, aside, children, testId }: { title: string; aside?: React.ReactNode; children: React.ReactNode; testId?: string }) {
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2" data-testid={testId}>
      <div className="mb-1.5 flex items-center justify-between text-xs">
        <span className="text-[var(--color-muted)]">{title}</span>
        {aside}
      </div>
      {children}
    </div>
  );
}

/** Horizontal bars for one metric across the repositories. Needs no history. */
function MetricBars({
  title,
  rows,
  metric,
  color,
  onOpen,
}: {
  title: string;
  rows: GithubRepoRow[];
  metric: "stars" | "forks" | "open_issues";
  color: string;
  onOpen: (name: string) => void;
}) {
  const bars = metricBars(rows, metric);
  if (bars.length === 0) return null;
  return (
    <Card title={title} testId="gh-bars">
      <div className="space-y-1">
        {bars.map((b) => (
          <button
            type="button"
            key={b.name}
            onClick={() => onOpen(b.name)}
            className="flex w-full items-center gap-2 text-left text-xs hover:opacity-80"
          >
            <span className="w-28 shrink-0 truncate text-[var(--color-fg)]">{b.name}</span>
            <div className="h-2 flex-1 overflow-hidden rounded-full bg-[var(--color-border)]">
              <div
                className="h-2 origin-left rounded-full"
                style={{ width: `${Math.max(3, b.frac * 100)}%`, background: `linear-gradient(90deg, ${color}99, ${color})` }}
              />
            </div>
            <span className="w-10 shrink-0 text-right tabular-nums">{b.value.toLocaleString("en-US")}</span>
          </button>
        ))}
      </div>
    </Card>
  );
}

/** Language mix as a donut, by repositories or by stars. */
function LanguageDonut({ rows }: { rows: GithubRepoRow[] }) {
  const [by, setBy] = useState<"repos" | "stars">("repos");
  const shares = languageShares(rows, by);
  const segs = donutSegments(
    shares.map((s) => ({ name: s.name, pct: s.pct })),
    { cx: 50, cy: 50, rOuter: 46, rInner: 30, minPct: 4 },
  ).map((seg) => (seg.name === NO_LANGUAGE ? { ...seg, color: "#6b7280" } : seg));
  const total = shares.reduce((a, s) => a + s.value, 0);
  return (
    <Card
      title="Languages"
      testId="gh-languages"
      aside={
        <span className="flex gap-1 text-[10px]">
          {(["repos", "stars"] as const).map((k) => (
            <button
              type="button"
              key={k}
              onClick={() => setBy(k)}
              className={`rounded px-1 ${by === k ? "bg-[var(--color-accent)]/20 text-[var(--color-fg)]" : "text-[var(--color-muted)]"}`}
            >
              {k === "repos" ? "Repos" : "Stars"}
            </button>
          ))}
        </span>
      }
    >
      {segs.length === 0 ? (
        <div className="flex h-[96px] items-center justify-center text-xs text-[var(--color-muted)]">No stars yet.</div>
      ) : (
        <div className="flex items-center gap-2">
          <div className="relative h-[96px] w-[96px] shrink-0">
            <svg viewBox="0 0 100 100" className="h-full w-full">
              {segs.map((seg) => (
                <path key={seg.name} d={seg.d} fill={seg.color}>
                  <title>{`${seg.name} · ${seg.pct.toFixed(0)} %`}</title>
                </path>
              ))}
            </svg>
            <div className="absolute inset-0 flex flex-col items-center justify-center leading-none">
              <span className="text-sm font-semibold tabular-nums text-[var(--color-fg)]">{total.toLocaleString("en-US")}</span>
              <span className="text-[9px] text-[var(--color-muted)]">{by === "repos" ? "repos" : "stars"}</span>
            </div>
          </div>
          <div className="min-w-0 flex-1 space-y-0.5">
            {segs.slice(0, 6).map((seg) => (
              <div key={seg.name} className="flex items-center gap-1.5 text-[11px]">
                <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: seg.color }} />
                <span className="truncate text-[var(--color-fg)]">{seg.name}</span>
                <span className="ml-auto tabular-nums text-[var(--color-muted)]">{seg.pct.toFixed(0)}%</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </Card>
  );
}

/** Columns: repositories by the month of their last push (last 12 months). */
function ActivityColumns({ rows, now }: { rows: GithubRepoRow[]; now: number }) {
  const months = activityByMonth(rows, now, 12);
  const max = Math.max(1, ...months.map((m) => m.count));
  const active = months.reduce((a, m) => a + m.count, 0);
  return (
    <Card
      title="Last push"
      testId="gh-activity"
      aside={<span className="text-[10px] tabular-nums text-[var(--color-muted)]">{active} active · 12 mo</span>}
    >
      <div className="flex h-[78px] items-stretch gap-[3px]">
        {months.map((m, i) => (
          <div key={m.start} className="flex min-w-0 flex-1 flex-col items-center justify-end gap-0.5" title={`${m.label}: ${m.count}`}>
            {m.count > 0 && <span className="text-[8px] tabular-nums text-[var(--color-muted)]">{m.count}</span>}
            <div
              className="w-full rounded-t-sm"
              style={{
                height: `${m.count === 0 ? 2 : Math.max(8, (m.count / max) * 56)}px`,
                background: m.count === 0 ? "var(--color-border)" : i === months.length - 1 ? "#10b981" : "var(--color-accent)",
                opacity: m.count === 0 ? 1 : 0.55 + 0.45 * (m.count / max),
              }}
            />
            <span className="text-[8px] text-[var(--color-muted)]">{m.label.slice(0, 1)}</span>
          </div>
        ))}
      </div>
    </Card>
  );
}

function RepoTable({ rows, onOpen }: { rows: GithubRepoRow[]; onOpen: (name: string) => void }) {
  if (rows.length === 0) return <div className="text-xs text-[var(--color-muted)]">No repositories.</div>;
  return (
    <table className="w-full text-xs" data-testid="gh-repos">
      <thead className="text-[10px] uppercase tracking-wide text-[var(--color-muted)]">
        <tr>
          <th className="text-left font-normal">Repository</th>
          <th className="text-right font-normal">Stars</th>
          <th className="text-right font-normal">Forks</th>
          <th className="text-right font-normal">Watch</th>
          <th className="text-right font-normal">Issues</th>
          <th className="pl-2 text-left font-normal">Language</th>
          <th className="text-right font-normal">Updated</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr
            key={r.id}
            className="cursor-pointer hover:bg-[var(--color-surface)]"
            onClick={() => onOpen(r.name)}
            data-testid="gh-repo-row"
          >
            <td className="max-w-[10rem] truncate py-0.5 text-[var(--color-fg)]">
              {r.private && <Lock size={10} className="mr-1 inline text-[var(--color-muted)]" aria-label="private" />}
              {r.name}
              {r.archived && <span className="ml-1 rounded px-1 text-[9px] uppercase text-amber-500">archived</span>}
            </td>
            <td className="text-right tabular-nums">
              {r.stars}
              {r.growth_7d != null && r.growth_7d !== 0 && (
                <span className={r.growth_7d > 0 ? "ml-1 text-emerald-500" : "ml-1 text-rose-500"}>
                  {formatDelta(r.growth_7d)}
                </span>
              )}
            </td>
            <td className="text-right tabular-nums">{r.forks}</td>
            <td className="text-right tabular-nums">{r.watchers ?? "—"}</td>
            <td className="text-right tabular-nums">{r.open_issues}</td>
            <td className="truncate pl-2 text-[var(--color-muted)]">{r.language ?? "—"}</td>
            <td className="text-right tabular-nums text-[var(--color-muted)]">
              {formatDate(r.pushed_at_ms ?? r.updated_at_ms)}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Chips<T extends string>({
  items,
  value,
  onPick,
}: {
  items: { id: T; label: string }[];
  value: T;
  onPick: (v: T) => void;
}) {
  return (
    <div className="flex flex-wrap gap-1">
      {items.map((it) => (
        <button
          key={it.id}
          type="button"
          onClick={() => onPick(it.id)}
          className={`rounded-full px-2 py-0.5 text-[11px] ${
            it.id === value
              ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
              : "bg-[var(--color-surface)] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          }`}
        >
          {it.label}
        </button>
      ))}
    </div>
  );
}

function ConnectCard({ status, onConnect }: { status: GithubStatus; onConnect: () => void }) {
  const dev = status.device;
  const [copied, setCopied] = useState(false);
  if (dev.state === "waiting") {
    return (
      <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3" data-testid="gh-device">
        <div className="text-xs text-[var(--color-muted)]">Enter this code on GitHub:</div>
        <div className="my-1 font-mono text-2xl tracking-widest text-[var(--color-fg)]">{dev.user_code}</div>
        <div className="flex flex-wrap gap-2 text-xs">
          <button
            type="button"
            className="flex items-center gap-1 rounded bg-[var(--color-accent)] px-2 py-1 text-[var(--color-accent-fg)]"
            onClick={() => void openUrl(dev.verification_uri)}
          >
            <ExternalLink size={12} /> Open {dev.verification_uri.replace(/^https?:\/\//, "")}
          </button>
          <button
            type="button"
            className="flex items-center gap-1 rounded bg-[var(--color-border)] px-2 py-1"
            onClick={() => {
              void writeText(dev.user_code).then(() => setCopied(true));
            }}
          >
            <Copy size={12} /> {copied ? "Copied" : "Copy code"}
          </button>
          <button type="button" className="rounded px-2 py-1 text-[var(--color-muted)]" onClick={() => void githubDeviceCancel()}>
            Cancel
          </button>
        </div>
        <div className="mt-2 flex items-center gap-1 text-[10px] text-[var(--color-muted)]">
          <Loader2 size={10} className="animate-spin" /> Waiting for GitHub…
        </div>
      </div>
    );
  }
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-xs" data-testid="gh-connect">
      <button
        type="button"
        disabled={!status.client_id_configured}
        onClick={onConnect}
        className="flex items-center gap-1 rounded bg-[var(--color-accent)] px-3 py-1.5 text-[var(--color-accent-fg)] disabled:opacity-50"
      >
        <GithubMark size={14} /> Connect GitHub
      </button>
      {dev.state === "failed" && <div className="mt-2 text-rose-500">{errorText(dev.message)}</div>}
      <div className="mt-2 text-[var(--color-muted)]">
        {status.client_id_configured
          ? "Opens github.com in your browser — read-only, public data unless you allow private repositories in Settings."
          : errorText("github.no_client_id")}{" "}
        Alternatively: a personal access token in Settings → GitHub
        {status.gh_available ? ", or log in with `gh auth login`." : "."}
      </div>
    </div>
  );
}

export function GithubPanel({ cmd, focused, runSignal, onExit, onQuery }: Props) {
  const [status, setStatus] = useState<GithubStatus | null>(null);
  const [dash, setDash] = useState<GithubDashboard | null>(null);
  const [detail, setDetail] = useState<GithubRepoDetail | null>(null);
  const [range, setRangeState] = useState<GithubRange>(rememberedRange);
  const [sortPick, setSortPick] = useState<GithubSortKey | null>(null);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
  const [now, setNow] = useState(() => Date.now());
  const asked = useRef(false);

  const view = cmd.kind === "view" ? cmd.view : cmd.kind === "repo" ? "repo" : "dashboard";
  const sort: GithubSortKey = sortPick ?? (cmd.kind === "view" ? cmd.sort : undefined) ?? "stars";
  const setRange = (r: GithubRange) => {
    rememberedRange = r;
    setRangeState(r);
  };

  const loadStatus = useCallback(() => {
    githubStatus()
      .then((s) => {
        setStatus(s);
        setNow(Date.now());
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    loadStatus();
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen("github-changed", () => {
      loadStatus();
      setRevision((n) => n + 1);
    }).then((u) => (cancelled ? u() : (unlisten = u)));
    const tick = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => {
      cancelled = true;
      unlisten?.();
      window.clearInterval(tick);
    };
  }, [loadStatus]);

  // Refresh on use when stale — the backend's gate decides whether GitHub is
  // actually asked (interval + rate limit), so this is cheap to call.
  useEffect(() => {
    if (!status?.connected || asked.current) return;
    asked.current = true;
    void githubRefresh(false).catch(() => {});
  }, [status?.connected]);

  useEffect(() => {
    let cancelled = false;
    githubDashboard(range, sort)
      .then((d) => !cancelled && setDash(d))
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [range, sort, revision]);

  const repoName = cmd.kind === "repo" ? cmd.name : null;
  useEffect(() => {
    if (!repoName) return;
    let cancelled = false;
    const t = window.setTimeout(() => {
      githubRepo(repoName, range)
        .then((d) => !cancelled && setDetail(d))
        .catch((e) => {
          if (cancelled) return;
          setDetail(null);
          setMessage({ ok: false, text: errorText(String(e)) ?? String(e) });
        });
    }, 200);
    return () => {
      cancelled = true;
      window.clearTimeout(t);
    };
  }, [repoName, range, revision]);

  const run = useCallback(
    async (f: () => Promise<string | null>) => {
      setBusy(true);
      try {
        const text = await f();
        setMessage(text ? { ok: true, text } : null);
      } catch (e) {
        setMessage({ ok: false, text: errorText(String(e)) ?? String(e) });
      } finally {
        setBusy(false);
        loadStatus();
      }
    },
    [loadStatus],
  );

  const refreshNow = useCallback(
    () =>
      run(async () => {
        const r = await githubRefresh(true);
        if (r.gate.kind === "rate_limited") return null; // the status line says when
        if (r.gate.kind === "too_soon") return "Refreshed less than a minute ago.";
        return null;
      }),
    [run],
  );
  const connect = useCallback(() => run(async () => (await githubDeviceStart(), null)), [run]);

  useEffect(() => {
    if (runSignal <= lastHandledRun) return;
    lastHandledRun = runSignal;
    if (cmd.kind === "refresh") void refreshNow();
    else if (cmd.kind === "connect") void connect();
    else if (cmd.kind === "export") {
      const { format, includePrivate } = cmd;
      void run(async () => `Exported: ${(await githubExport(format, includePrivate)).split("/").pop()}`);
    }
  }, [runSignal, cmd, refreshNow, connect, run]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
        return;
      }
      const target = e.target as HTMLElement | null;
      if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA")) return;
      if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        const i = RANGES.findIndex((r) => r.id === rememberedRange);
        const next = RANGES[(i + (e.key === "ArrowRight" ? 1 : RANGES.length - 1)) % RANGES.length];
        setRange(next.id);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit]);

  const rows = useMemo(() => {
    if (!dash) return [];
    if (view === "trending") return dash.rows.filter((r) => (r.growth_7d ?? 0) > 0);
    return dash.rows;
  }, [dash, view]);

  const line = status ? statusLine(status, now) : null;
  const openRepo = (name: string) => onQuery(`github repo ${name}`);
  const kpis = dash?.kpis ?? [];
  const followersKpi = kpis.find((k) => k.key === "followers");
  const starsKpi = kpis.find((k) => k.key === "stars");

  return (
    <div className="flex h-full flex-col gap-2 overflow-hidden p-3 text-sm" data-testid="github-panel">
      <div className="flex items-center gap-2 text-[var(--color-fg)]">
        {status?.avatar_url ? (
          <img src={status.avatar_url} alt="" className="h-5 w-5 rounded-full" />
        ) : (
          <GithubMark size={16} />
        )}
        <span className="font-semibold">
          {status?.connected && status.login ? `Connected as @${status.login}` : "GitHub"}
        </span>
        {status?.source && <span className="text-[10px] text-[var(--color-muted)]">{SOURCE_LABEL[status.source]}</span>}
        <button
          type="button"
          title="Refresh now"
          disabled={busy || !status?.connected || !!status?.rate_limited_until}
          onClick={() => void refreshNow()}
          className="ml-auto rounded p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)] disabled:opacity-40"
        >
          <RefreshCw size={14} className={busy || status?.refreshing ? "animate-spin" : ""} />
        </button>
      </div>

      {line && (status?.last_ok_ms != null || status?.last_error) && (
        <div
          data-testid="gh-status-line"
          className={`text-xs ${
            line.tone === "error" ? "text-rose-500" : line.tone === "warn" ? "text-amber-500" : "text-[var(--color-muted)]"
          }`}
        >
          {line.text}
        </div>
      )}
      {cmd.kind === "invalid" && (
        <div className="flex items-start gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 p-2 text-xs">
          <AlertTriangle size={14} className="mt-0.5 shrink-0 text-amber-400" />
          <span>{cmd.message}</span>
        </div>
      )}
      {(cmd.kind === "refresh" || cmd.kind === "export" || cmd.kind === "connect") && !focused && (
        <div className="text-xs text-[var(--color-muted)]">
          {cmd.kind === "export" ? (
            <>
              <Download size={11} className="mr-1 inline" />
              Enter writes {cmd.format.toUpperCase()} to Downloads
              {cmd.includePrivate ? " — including private repositories." : " — private repositories left out (add `private`)."}
            </>
          ) : (
            "Enter runs it."
          )}
        </div>
      )}
      {message && <div className={`text-xs ${message.ok ? "text-emerald-500" : "text-rose-500"}`}>{message.text}</div>}

      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto">
        {status && !status.connected && <ConnectCard status={status} onConnect={() => void connect()} />}

        {!dash && status?.connected && (
          <div className="flex items-center gap-2 text-xs text-[var(--color-muted)]">
            <Loader2 size={12} className="animate-spin" /> Loading your GitHub statistics…
          </div>
        )}

        {dash && (
          <>
            <div className="flex items-center justify-between gap-2">
              {/* The ranking uses fixed 7/30-day growth — a range would do nothing there. */}
              {view !== "repos" && view !== "trending" ? <Chips items={RANGES} value={range} onPick={setRange} /> : <span />}
              {dash.history_since_ms && (
                <span className="text-[10px] text-[var(--color-muted)]">since {formatDate(dash.history_since_ms)}</span>
              )}
            </div>

            {view === "dashboard" && (
              <>
                <div className="grid grid-cols-2 gap-2">
                  {kpis.map((k) => (
                    <KpiCard key={k.key} kpi={k} />
                  ))}
                </div>
                <div className="grid grid-cols-2 gap-2">
                  <LineChart title="Followers" lines={[{ name: "followers", points: dash.followers }]} />
                  <LineChart title="Total stars" lines={[{ name: "stars", points: dash.stars }]} />
                </div>
                <div className="grid grid-cols-2 gap-2">
                  <LanguageDonut rows={dash.rows} />
                  <ActivityColumns rows={dash.rows} now={now} />
                </div>
                <div className="grid grid-cols-2 gap-2">
                  <MetricBars title="Stars by repository" rows={dash.rows} metric="stars" color="#f59e0b" onOpen={openRepo} />
                  <MetricBars title="Forks by repository" rows={dash.rows} metric="forks" color="#38bdf8" onOpen={openRepo} />
                </div>
                <RepoTable rows={rows.slice(0, 5)} onOpen={openRepo} />
                {dash.rows.length > 5 && (
                  <button type="button" className="text-xs text-[var(--color-accent)]" onClick={() => onQuery("github repos")}>
                    All {dash.rows.length} repositories →
                  </button>
                )}
              </>
            )}

            {(view === "repos" || view === "trending") && (
              <>
                {view === "repos" && <Chips items={SORTS} value={sort} onPick={setSortPick} />}
                {view === "trending" && rows.length === 0 && (
                  <div className="text-xs text-[var(--color-muted)]">No repository gained stars in the last 7 days.</div>
                )}
                <RepoTable rows={rows} onOpen={openRepo} />
              </>
            )}

            {view === "followers" && followersKpi && (
              <>
                <KpiCard kpi={followersKpi} />
                <LineChart title="Followers" lines={[{ name: "followers", points: dash.followers }]} />
              </>
            )}

            {view === "stars" && starsKpi && (
              <>
                <KpiCard kpi={starsKpi} />
                <LineChart title="Total stars" lines={[{ name: "stars", points: dash.stars }]} />
                <MetricBars title="Stars by repository" rows={dash.rows} metric="stars" color="#f59e0b" onOpen={openRepo} />
                <LineChart title="Top repositories — stars over time" lines={dash.growth} />
              </>
            )}
          </>
        )}

        {view === "repo" && detail && (
          <div className="space-y-2" data-testid="gh-detail">
            <div>
              <div className="flex items-center gap-1 font-semibold text-[var(--color-fg)]">
                {detail.repo.private && <Lock size={12} />}
                {detail.repo.full_name}
                {detail.repo.archived && <span className="text-[10px] uppercase text-amber-500">archived</span>}
                <button
                  type="button"
                  className="ml-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                  onClick={() => void openUrl(detail.repo.html_url)}
                  title="Open on GitHub"
                >
                  <ExternalLink size={12} />
                </button>
              </div>
              {detail.repo.description && <div className="text-xs text-[var(--color-muted)]">{detail.repo.description}</div>}
              <div className="text-[10px] text-[var(--color-muted)]">
                {[detail.repo.language, detail.repo.license, `observed since ${formatDate(detail.first_seen_ms)}`]
                  .filter(Boolean)
                  .join(" · ")}
              </div>
            </div>
            <div className="grid grid-cols-2 gap-2">
              <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2">
                <div className="flex items-center gap-1 text-[10px] uppercase text-[var(--color-muted)]">
                  <Star size={10} /> Stars
                </div>
                <div className="text-lg font-semibold tabular-nums">{detail.repo.stars}</div>
                <DeltaRow d={detail.star_deltas} />
              </div>
              <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2">
                <div className="text-[10px] uppercase text-[var(--color-muted)]">Forks</div>
                <div className="text-lg font-semibold tabular-nums">{detail.repo.forks}</div>
                <DeltaRow d={detail.fork_deltas} />
              </div>
              <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2 text-xs">
                Watchers <span className="float-right tabular-nums">{detail.repo.watchers ?? "—"}</span>
              </div>
              <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2 text-xs">
                Open issues <span className="float-right tabular-nums">{detail.repo.open_issues}</span>
              </div>
            </div>
            <LineChart title="Stars" lines={[{ name: "stars", points: detail.stars }]} />
            <div className="grid grid-cols-2 gap-2">
              <LineChart title="Forks" lines={[{ name: "forks", points: detail.forks }]} />
              <LineChart title="Open issues" lines={[{ name: "issues", points: detail.open_issues }]} />
            </div>
          </div>
        )}
        {status?.connected && status.last_ok_ms == null && !status.last_error && !dash && (
          <div className="text-[10px] text-[var(--color-muted)]">First refresh — this takes a few seconds.</div>
        )}
        {dash && dash.private_count > 0 && (
          <div className="text-[10px] text-[var(--color-muted)]">
            <Lock size={9} className="mr-1 inline" />
            {dash.private_count} private {dash.private_count === 1 ? "repository" : "repositories"} — never exported
            unless you add `private`.
          </div>
        )}
        {status && (
          <div className="text-[10px] text-[var(--color-muted)]">
            {status.rate && `API: ${status.rate.remaining}/${status.rate.limit} requests left · `}
            {status.next_refresh_ms && !status.rate_limited_until && status.config.auto_refresh && `next automatic refresh ${formatIn(status.next_refresh_ms, now)}`}
          </div>
        )}
      </div>
    </div>
  );
}
