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
import { AlertTriangle, Briefcase, CalendarDays, Copy, Download, ExternalLink, GitFork, Link2, Loader2, Lock, MapPin, RefreshCw, Star } from "lucide-react";
import {
  githubDashboard,
  githubDeviceCancel,
  githubDeviceStart,
  githubExport,
  githubRefresh,
  githubRepo,
  githubRepoExtras,
  githubStatus,
  type GithubDashboard,
  type GithubDeltas,
  type GithubKpi,
  type GithubMonthGain,
  type GithubPoint,
  type GithubProfile,
  type GithubRepoDetail,
  type GithubRepoExtras,
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
  ageParts,
  composition,
  concentration,
  createdByYear,
  fastestGrowing,
  formatAge,
  formatRate,
  formatSize,
  languageShares,
  metricBars,
  monthLabel,
  repoInsight,
  statusLine,
  timeDomain,
  toSeries,
  yearsSince,
  DORMANT_DAYS,
  type GithubCmd,
  type GithubRange,
} from "../lib/github";
import { areaPath, linePath, seriesExtent, stepAreaPath, stepPath } from "../lib/stats-chart";
import { donutSegments, languageColor } from "../lib/loc";
import { locale, t, tn, useT, type MessageKey } from "../lib/i18n";

/** Signals already handled — module-level so a remount never replays one. */
let lastHandledRun = 0;
/** The range survives closing the panel (per session). */
let rememberedRange: GithubRange = "30d";

const KPI_LABEL: Record<GithubKpi["key"], MessageKey> = {
  followers: "github.kpi.followers",
  stars: "github.kpi.stars",
  repos: "github.kpi.repos",
  forks: "github.kpi.forks",
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

/** Up green, down red, no change (and unknown) neutral — a zero is not good news. */
export const TONE_CLASS: Record<ReturnType<typeof deltaTone>, string> = {
  up: "text-emerald-500",
  down: "text-rose-500",
  flat: "text-[var(--color-muted)]",
  none: "text-[var(--color-muted)]",
};

function Delta({ label, d }: { label: string; d: number | null }) {
  const cls = TONE_CLASS[deltaTone(d)];
  return (
    <span className="tabular-nums" title={d == null ? t("github.insufficient_history") : undefined}>
      <span className="text-[var(--color-muted)]">{label} </span>
      <span className={cls}>{formatDelta(d)}</span>
    </span>
  );
}

function DeltaRow({ d }: { d: GithubDeltas }) {
  if (historyCollecting(d)) {
    return <div className="text-[10px] text-[var(--color-muted)]">{t("github.history_collecting")}</div>;
  }
  return (
    <div className="flex flex-wrap gap-x-2 text-[10px]">
      <Delta label="24h" d={d.d24h} />
      <Delta label="7d" d={d.d7d} />
      <Delta label="30d" d={d.d30d} />
      <Delta label="90d" d={d.d90d} />
      <Delta label="1y" d={d.d1y} />
    </div>
  );
}

function KpiCard({ kpi }: { kpi: GithubKpi }) {
  const d30 = kpi.deltas.d30d;
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2" data-testid="gh-kpi">
      <div className="text-[10px] uppercase tracking-wide text-[var(--color-muted)]">{t(KPI_LABEL[kpi.key])}</div>
      <div className="flex items-baseline gap-2">
        <span className="text-lg font-semibold tabular-nums text-[var(--color-fg)]">
          {kpi.value.toLocaleString(locale())}
        </span>
        {d30 != null && (
          <span className={`text-xs tabular-nums ${TONE_CLASS[deltaTone(d30)]}`}>
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

/**
 * A history chart. Counts (stars, forks, followers) change only at events,
 * so they are drawn as STEPS — a diagonal would claim values that never
 * existed. `markAt` = where real observation starts: left of it the line is
 * reconstructed from GitHub's stargazer / fork lists. One point is not a
 * line, so it says so.
 */
function LineChart({
  title,
  lines,
  step = true,
  markAt,
  testId = "gh-chart",
}: {
  title: string;
  lines: { name: string; points: GithubPoint[] }[];
  step?: boolean;
  markAt?: number | null;
  testId?: string;
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
  const path = step ? stepPath : linePath;
  const area = step ? stepAreaPath : areaPath;
  const markFrac =
    markAt != null && domain && markAt / 1000 > domain[0] && markAt / 1000 < domain[1]
      ? (markAt / 1000 - domain[0]) / (domain[1] - domain[0] || 1)
      : null;
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2" data-testid={testId}>
      <div className="mb-1 flex items-center justify-between text-xs">
        <span className="text-[var(--color-muted)]">{title}</span>
        {current != null && <span className="tabular-nums text-[var(--color-fg)]">{current.toLocaleString(locale())}</span>}
      </div>
      {!enough || !domain ? (
        <div className="flex h-[70px] items-center justify-center text-xs text-[var(--color-muted)]">
          {tn("github.collecting_n", values.length)}
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
              <path d={area(series[0], domain[0], domain[1], CHART_W, CHART_H, vMin, vMax)} fill={`url(#${gradId})`} />
            )}
            {markFrac != null && (
              <line
                data-testid="gh-chart-mark"
                x1={markFrac * CHART_W}
                x2={markFrac * CHART_W}
                y1={0}
                y2={CHART_H}
                stroke="var(--color-muted)"
                strokeWidth={1}
                vectorEffect="non-scaling-stroke"
                strokeDasharray="3 3"
              />
            )}
            {series.map((s, i) => (
              <path
                key={lines[i].name}
                d={path(s, domain[0], domain[1], CHART_W, CHART_H, vMin, vMax)}
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
          {markFrac != null && markFrac > 0.3 && (
            <span
              className="pointer-events-none absolute top-0 -translate-x-full whitespace-nowrap rounded bg-[var(--color-surface)]/80 px-0.5 text-[9px] text-[var(--color-muted)]"
              style={{ left: `${markFrac * 100}%` }}
              title={t("github.reconstructed_hint")}
            >
              {t("github.reconstructed")}
            </span>
          )}
          <span className="pointer-events-none absolute left-0 top-0 rounded bg-[var(--color-surface)]/80 px-0.5 text-[9px] tabular-nums text-[var(--color-muted)]">
            {t("github.max")} {fmtAxis(dataMax)}
          </span>
          <span className="pointer-events-none absolute bottom-0 right-0 rounded bg-[var(--color-surface)]/80 px-0.5 text-[9px] tabular-nums text-[var(--color-muted)]">
            {t("github.min")} {fmtAxis(dataMin)}
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
            <span className="w-10 shrink-0 text-right tabular-nums">{b.value.toLocaleString(locale())}</span>
          </button>
        ))}
      </div>
    </Card>
  );
}

/** Language mix as a donut, by repositories or by stars. */
/** `NO_LANGUAGE` is a grouping key; show it in the current language. */
function langLabel(name: string): string {
  return name === NO_LANGUAGE ? t("github.no_language") : name;
}

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
      title={t("github.languages")}
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
              {k === "repos" ? t("github.repos") : t("github.sort.stars")}
            </button>
          ))}
        </span>
      }
    >
      {segs.length === 0 ? (
        <div className="flex h-[96px] items-center justify-center text-xs text-[var(--color-muted)]">{t("github.no_stars_yet")}</div>
      ) : (
        <div className="flex items-center gap-2">
          <div className="relative h-[96px] w-[96px] shrink-0">
            <svg viewBox="0 0 100 100" className="h-full w-full">
              {segs.map((seg) => (
                <path key={seg.name} d={seg.d} fill={seg.color}>
                  <title>{`${langLabel(seg.name)} · ${seg.pct.toFixed(0)} %`}</title>
                </path>
              ))}
            </svg>
            <div className="absolute inset-0 flex flex-col items-center justify-center leading-none">
              <span className="text-sm font-semibold tabular-nums text-[var(--color-fg)]">{total.toLocaleString(locale())}</span>
              <span className="text-[9px] text-[var(--color-muted)]">{by === "repos" ? "repos" : "stars"}</span>
            </div>
          </div>
          <div className="min-w-0 flex-1 space-y-0.5">
            {segs.slice(0, 6).map((seg) => (
              <div key={seg.name} className="flex items-center gap-1.5 text-[11px]">
                <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: seg.color }} />
                <span className="truncate text-[var(--color-fg)]">{langLabel(seg.name)}</span>
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
      title={t("github.last_push")}
      testId="gh-activity"
      aside={<span className="text-[10px] tabular-nums text-[var(--color-muted)]">{t("github.active_12mo", { n: active })}</span>}
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

/** Generic columns: one bar per bucket; null = unknown (hatched, not 0). */
function Columns({
  items,
  color = "var(--color-accent)",
  height = 56,
  highlightLast = false,
}: {
  items: { key: string | number; label: string; value: number | null; title?: string }[];
  color?: string;
  height?: number;
  highlightLast?: boolean;
}) {
  const max = Math.max(1, ...items.map((m) => m.value ?? 0));
  const every = Math.max(1, Math.ceil(items.length / 12));
  return (
    <div className="flex items-stretch gap-[2px]" style={{ height: height + 22 }}>
      {items.map((m, i) => (
        <div
          key={m.key}
          className="flex min-w-0 flex-1 flex-col items-center justify-end gap-0.5"
          title={m.title ?? `${m.label}: ${m.value == null ? "—" : m.value.toLocaleString(locale())}`}
        >
          {m.value != null && m.value > 0 && items.length <= 16 && (
            <span className="text-[8px] tabular-nums text-[var(--color-muted)]">{fmtAxis(m.value)}</span>
          )}
          <div
            className="w-full rounded-t-sm"
            style={{
              height: `${m.value == null ? 2 : m.value === 0 ? 2 : Math.max(4, (m.value / max) * height)}px`,
              background:
                m.value == null
                  ? "repeating-linear-gradient(90deg, var(--color-border) 0 2px, transparent 2px 4px)"
                  : m.value === 0
                    ? "var(--color-border)"
                    : highlightLast && i === items.length - 1
                      ? "#10b981"
                      : color,
              opacity: m.value ? 0.55 + 0.45 * (m.value / max) : 1,
            }}
          />
          <span className="h-[10px] text-[8px] leading-[10px] text-[var(--color-muted)]">{i % every === 0 ? m.label : ""}</span>
        </div>
      ))}
    </div>
  );
}

/** Stars gained per month (last 12). Unknown months are shown as such. */
function GainBars({ title, gains, testId }: { title: string; gains: GithubMonthGain[]; testId?: string }) {
  if (gains.length === 0) return null;
  const known = gains.filter((g) => g[1] != null);
  const sum = known.reduce((a, g) => a + (g[1] ?? 0), 0);
  return (
    <Card
      title={title}
      testId={testId ?? "gh-gains"}
      aside={
        known.length > 0 ? (
          <span className="text-[10px] tabular-nums text-[var(--color-muted)]">
            {t("github.gains_sum", { n: formatDelta(sum), months: known.length })}
          </span>
        ) : undefined
      }
    >
      <Columns
        items={gains.map(([ms, v]) => ({ key: ms, label: monthLabel(ms), value: v }))}
        color="#f59e0b"
        highlightLast
      />
    </Card>
  );
}

/** Who the account is: since when, where, what — and per-year rates. */
function ProfileCard({ profile, stars, repos, now }: { profile: GithubProfile; stars: number; repos: number; now: number }) {
  const created = profile.created_at_ms;
  const years = created != null ? yearsSince(created, now) : null;
  const facts = [
    profile.company && { icon: <Briefcase size={10} />, text: profile.company },
    profile.location && { icon: <MapPin size={10} />, text: profile.location },
    profile.blog && { icon: <Link2 size={10} />, text: profile.blog.replace(/^https?:\/\//, "") },
  ].filter(Boolean) as { icon: React.ReactNode; text: string }[];
  const ratio = profile.following > 0 ? profile.followers / profile.following : null;
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2" data-testid="gh-profile">
      <div className="flex items-start gap-2">
        {profile.avatar_url ? (
          <img src={profile.avatar_url} alt="" className="h-9 w-9 shrink-0 rounded-full" />
        ) : (
          <span className="mt-0.5 text-[var(--color-muted)]">
            <GithubMark size={28} />
          </span>
        )}
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-semibold text-[var(--color-fg)]">
            {profile.name ?? profile.login} <span className="font-normal text-[var(--color-muted)]">@{profile.login}</span>
          </div>
          {profile.bio && <div className="line-clamp-2 text-[11px] text-[var(--color-muted)]">{profile.bio}</div>}
          {facts.length > 0 && (
            <div className="mt-0.5 flex flex-wrap gap-x-2 text-[10px] text-[var(--color-muted)]">
              {facts.map((f) => (
                <span key={f.text} className="flex items-center gap-0.5">
                  {f.icon}
                  {f.text}
                </span>
              ))}
            </div>
          )}
        </div>
        <div className="shrink-0 text-right" data-testid="gh-account-age">
          <div className="text-[10px] uppercase tracking-wide text-[var(--color-muted)]">{t("github.account_age")}</div>
          <div className="text-lg font-semibold tabular-nums text-[var(--color-fg)]">{formatAge(created, now)}</div>
          {created != null && (
            <div className="flex items-center justify-end gap-0.5 text-[10px] text-[var(--color-muted)]">
              <CalendarDays size={10} /> {t("github.member_since", { date: formatDate(created) })}
            </div>
          )}
        </div>
      </div>
      <div className="mt-1.5 grid grid-cols-4 gap-1 text-center text-[10px]">
        <Stat label={t("github.stars_per_year")} value={years && years >= 0.25 ? formatRate(stars / years) : "—"} />
        <Stat label={t("github.repos_per_year")} value={years && years >= 0.25 ? formatRate(repos / years) : "—"} />
        <Stat
          label={t("github.follow_ratio")}
          value={ratio == null ? "—" : formatRate(ratio)}
          title={t("github.follow_ratio_hint", { followers: profile.followers, following: profile.following })}
        />
        <Stat label={t("github.gists")} value={profile.public_gists.toLocaleString(locale())} />
      </div>
    </div>
  );
}

function Stat({ label, value, title }: { label: string; value: string; title?: string }) {
  return (
    <div className="rounded bg-[var(--color-bg)]/40 px-1 py-0.5" title={title}>
      <div className="text-sm font-semibold tabular-nums text-[var(--color-fg)]">{value}</div>
      <div className="truncate text-[9px] uppercase tracking-wide text-[var(--color-muted)]">{label}</div>
    </div>
  );
}

/** Original / fork, active / dormant / archived as stacked bars. */
function CompositionCard({ rows, now }: { rows: GithubRepoRow[]; now: number }) {
  const c = composition(rows, now);
  if (c.total === 0) return null;
  const bar = (parts: { n: number; color: string; label: string }[]) => (
    <div className="flex h-2.5 overflow-hidden rounded-full bg-[var(--color-border)]">
      {parts
        .filter((p) => p.n > 0)
        .map((p) => (
          <div key={p.label} style={{ width: `${(p.n / c.total) * 100}%`, background: p.color }} title={`${p.label}: ${p.n}`} />
        ))}
    </div>
  );
  const legend = (parts: { n: number; color: string; label: string }[]) => (
    <div className="mt-0.5 flex flex-wrap gap-x-2 text-[10px] text-[var(--color-muted)]">
      {parts.map((p) => (
        <span key={p.label} className="flex items-center gap-1">
          <span className="h-2 w-2 rounded-full" style={{ background: p.color }} />
          {p.label} <span className="tabular-nums text-[var(--color-fg)]">{p.n}</span>
        </span>
      ))}
    </div>
  );
  const origin = [
    { n: c.originals, color: "var(--color-accent)", label: t("github.originals") },
    { n: c.forks, color: "#8b5cf6", label: t("github.forks_of_others") },
  ];
  const life = [
    { n: c.active, color: "#10b981", label: t("github.active") },
    { n: c.dormant, color: "#f59e0b", label: t("github.dormant", { days: DORMANT_DAYS }) },
    { n: c.archived, color: "#6b7280", label: t("github.archived") },
  ];
  return (
    <Card title={t("github.composition")} testId="gh-composition" aside={<span className="text-[10px] tabular-nums text-[var(--color-muted)]">{c.total}</span>}>
      {bar(origin)}
      {legend(origin)}
      <div className="h-1.5" />
      {bar(life)}
      {legend(life)}
    </Card>
  );
}

/** How concentrated the stars are. */
function ConcentrationCard({ rows }: { rows: GithubRepoRow[] }) {
  const c = concentration(rows);
  if (!c.top) return null;
  return (
    <Card title={t("github.concentration")} testId="gh-concentration">
      <div className="space-y-1 text-xs">
        <div className="flex justify-between">
          <span className="truncate text-[var(--color-muted)]">{t("github.top_repo_share", { name: c.top.name })}</span>
          <span className="tabular-nums text-[var(--color-fg)]">{c.top.pct.toFixed(0)} %</span>
        </div>
        <div className="flex justify-between">
          <span className="text-[var(--color-muted)]">{t("github.top3_share")}</span>
          <span className="tabular-nums text-[var(--color-fg)]">{c.top3Pct.toFixed(0)} %</span>
        </div>
        <div className="flex justify-between">
          <span className="text-[var(--color-muted)]">{t("github.half_by")}</span>
          <span className="tabular-nums text-[var(--color-fg)]">{tn("github.repos_n", c.halfBy)}</span>
        </div>
      </div>
    </Card>
  );
}

/** Most-used topics over the repositories. */
function TopicsCard({ rows }: { rows: GithubRepoRow[] }) {
  const counts = new Map<string, number>();
  for (const r of rows) for (const tp of r.topics ?? []) counts.set(tp, (counts.get(tp) ?? 0) + 1);
  const top = [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, 16);
  if (top.length === 0) return null;
  return (
    <Card title={t("github.topics")} testId="gh-topics">
      <div className="flex flex-wrap gap-1">
        {top.map(([name, n]) => (
          <span key={name} className="rounded-full bg-[var(--color-accent)]/15 px-1.5 py-0.5 text-[10px] text-[var(--color-fg)]">
            {name} <span className="tabular-nums text-[var(--color-muted)]">{n}</span>
          </span>
        ))}
      </div>
    </Card>
  );
}

/** Small tile: label + value (+ hint). */
function Tile({ label, value, hint, testId }: { label: React.ReactNode; value: React.ReactNode; hint?: React.ReactNode; testId?: string }) {
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2" data-testid={testId}>
      <div className="flex items-center gap-1 text-[10px] uppercase tracking-wide text-[var(--color-muted)]">{label}</div>
      <div className="text-lg font-semibold tabular-nums text-[var(--color-fg)]">{value}</div>
      {hint && <div className="text-[10px] text-[var(--color-muted)]">{hint}</div>}
    </div>
  );
}

/** Commit activity, releases, languages and traffic of one repository. */
function RepoExtrasView({ extras, now }: { extras: GithubRepoExtras; now: number }) {
  const langs = extras.languages ?? [];
  const totalBytes = langs.reduce((a, l) => a + l[1], 0);
  const segs =
    totalBytes > 0
      ? donutSegments(
          langs.map(([name, b]) => ({ name, pct: (b / totalBytes) * 100 })),
          { cx: 50, cy: 50, rOuter: 46, rInner: 30, minPct: 3 },
        )
      : [];
  const commits = extras.commits ?? [];
  const commitTotal = commits.reduce((a, w) => a + w[1], 0);
  const rel = extras.releases;
  return (
    <>
      <Card
        title={t("github.commit_activity")}
        testId="gh-commits"
        aside={
          commits.length > 0 ? (
            <span className="text-[10px] tabular-nums text-[var(--color-muted)]">{t("github.commits_52w", { n: commitTotal.toLocaleString(locale()) })}</span>
          ) : undefined
        }
      >
        {extras.commits_pending && commits.length === 0 ? (
          <div className="text-xs text-[var(--color-muted)]">{t("github.commits_pending")}</div>
        ) : commits.length === 0 ? (
          <div className="text-xs text-[var(--color-muted)]">{t("github.no_data")}</div>
        ) : (
          <Columns
            items={commits.map(([ms, n]) => ({
              key: ms,
              label: new Date(ms).toLocaleDateString(locale(), { month: "short" }),
              value: n,
              title: `${formatDate(ms)}: ${n}`,
            }))}
            height={44}
          />
        )}
      </Card>
      <div className="grid grid-cols-2 gap-2">
        {segs.length > 0 && (
          <Card title={t("github.languages_bytes")} testId="gh-repo-languages">
            <div className="flex items-center gap-2">
              <svg viewBox="0 0 100 100" className="h-[72px] w-[72px] shrink-0">
                {segs.map((seg) => (
                  <path key={seg.name} d={seg.d} fill={seg.color}>
                    <title>{`${seg.name} · ${seg.pct.toFixed(1)} %`}</title>
                  </path>
                ))}
              </svg>
              <div className="min-w-0 flex-1 space-y-0.5">
                {segs.slice(0, 5).map((seg) => (
                  <div key={seg.name} className="flex items-center gap-1 text-[11px]">
                    <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: seg.color ?? languageColor(seg.name) }} />
                    <span className="truncate text-[var(--color-fg)]">{seg.name}</span>
                    <span className="ml-auto tabular-nums text-[var(--color-muted)]">{seg.pct.toFixed(0)}%</span>
                  </div>
                ))}
              </div>
            </div>
          </Card>
        )}
        {rel && rel.count > 0 && (
          <Card title={t("github.releases")} testId="gh-releases">
            <div className="space-y-0.5 text-xs">
              <div className="flex justify-between">
                <span className="text-[var(--color-muted)]">{t("github.release_count")}</span>
                <span className="tabular-nums">{rel.count}{rel.count >= 100 ? "+" : ""}</span>
              </div>
              {rel.latest_tag && (
                <div className="flex justify-between gap-2">
                  <span className="text-[var(--color-muted)]">{t("github.latest_release")}</span>
                  <span className="truncate tabular-nums" title={rel.latest_ms ? formatDate(rel.latest_ms) : undefined}>
                    {rel.latest_tag}
                    {rel.latest_ms && <span className="text-[var(--color-muted)]"> · {formatAge(rel.latest_ms, now)}</span>}
                  </span>
                </div>
              )}
              <div className="flex justify-between">
                <span className="text-[var(--color-muted)]">{t("github.downloads")}</span>
                <span className="tabular-nums">{rel.downloads.toLocaleString(locale())}</span>
              </div>
              {rel.dates.length >= 2 && (
                <div className="flex justify-between">
                  <span className="text-[var(--color-muted)]">{t("github.release_cadence")}</span>
                  <span className="tabular-nums">
                    {t("github.every_days", {
                      n: Math.max(1, Math.round((rel.dates[0] - rel.dates[rel.dates.length - 1]) / 86_400_000 / (rel.dates.length - 1))),
                    })}
                  </span>
                </div>
              )}
            </div>
          </Card>
        )}
      </div>
      {(extras.views || extras.clones) && (
        <div className="grid grid-cols-2 gap-2" data-testid="gh-traffic">
          {extras.views && (
            <LineChart
              title={t("github.views_14d", { n: extras.views.count, u: extras.views.uniques })}
              lines={[{ name: "views", points: extras.views.days }]}
              step={false}
            />
          )}
          {extras.clones && (
            <LineChart
              title={t("github.clones_14d", { n: extras.clones.count, u: extras.clones.uniques })}
              lines={[{ name: "clones", points: extras.clones.days }]}
              step={false}
            />
          )}
        </div>
      )}
    </>
  );
}

/** The detail matches the query: name, full name or id (case-insensitive). */
export function detailMatches(d: GithubRepoDetail, query: string): boolean {
  const q = query.trim().toLowerCase();
  return d.repo.name.toLowerCase() === q || d.repo.full_name.toLowerCase() === q || String(d.repo.id) === q;
}

/** Where the history comes from: reconstructed lists + real snapshots. */
function BackfillNote({ dash }: { dash: GithubDashboard }) {
  const b = dash.backfill;
  if (b.total === 0) return null;
  const complete = b.done >= b.total;
  return (
    <div className="text-[10px] text-[var(--color-muted)]" data-testid="gh-backfill" title={t("github.reconstructed_hint")}>
      {complete
        ? t("github.backfill_done", { date: formatDate(dash.reconstructed_from_ms ?? dash.history_since_ms) })
        : t("github.backfill_progress", { done: b.done, total: b.total })}
      {b.truncated > 0 && ` · ${tn("github.backfill_truncated", b.truncated)}`}
    </div>
  );
}

/** Repositories that gather stars fastest (per month since creation). */
function FastestCard({ rows, now, onOpen }: { rows: GithubRepoRow[]; now: number; onOpen: (n: string) => void }) {
  const top = fastestGrowing(rows, now);
  if (top.length === 0) return null;
  const max = Math.max(...top.map((x) => x.rate));
  return (
    <Card title={t("github.fastest")} testId="gh-fastest">
      <div className="space-y-1">
        {top.map((x) => (
          <button type="button" key={x.name} onClick={() => onOpen(x.name)} className="flex w-full items-center gap-2 text-left text-xs hover:opacity-80">
            <span className="w-28 shrink-0 truncate text-[var(--color-fg)]">{x.name}</span>
            <div className="h-2 flex-1 overflow-hidden rounded-full bg-[var(--color-border)]">
              <div className="h-2 rounded-full bg-emerald-500" style={{ width: `${Math.max(3, (x.rate / max) * 100)}%` }} />
            </div>
            <span className="w-16 shrink-0 text-right tabular-nums">{t("github.per_month", { n: formatRate(x.rate) })}</span>
          </button>
        ))}
      </div>
    </Card>
  );
}

/** Repositories without a push for half a year (not archived) — candidates to archive. */
function DormantCard({ rows, now, onOpen }: { rows: GithubRepoRow[]; now: number; onOpen: (n: string) => void }) {
  const dormant = rows
    .map((r) => ({ r, i: repoInsight(r, now) }))
    .filter((x) => x.i.dormant)
    .sort((a, b) => (b.i.daysSincePush ?? 0) - (a.i.daysSincePush ?? 0));
  if (dormant.length === 0) return null;
  return (
    <Card title={t("github.dormant_title", { days: DORMANT_DAYS })} testId="gh-dormant" aside={<span className="text-[10px] tabular-nums text-[var(--color-muted)]">{dormant.length}</span>}>
      <div className="space-y-0.5">
        {dormant.slice(0, 8).map(({ r }) => (
          <button type="button" key={r.id} onClick={() => onOpen(r.name)} className="flex w-full justify-between gap-2 text-left text-xs hover:opacity-80">
            <span className="truncate text-[var(--color-fg)]">{r.name}</span>
            <span className="shrink-0 tabular-nums text-[var(--color-muted)]">{formatAge(r.pushed_at_ms ?? r.updated_at_ms, now)}</span>
          </button>
        ))}
      </div>
    </Card>
  );
}

function RepoDetailView({ d, extras, now }: { d: GithubRepoDetail; extras: GithubRepoExtras | null; now: number }) {
  const r = d.repo;
  const ins = repoInsight(r, now);
  const reconstructed = d.stars_synced != null;
  const age = r.created_at_ms != null ? ageParts(r.created_at_ms, now) : null;
  return (
    <div className="space-y-2" data-testid="gh-detail">
      <div>
        <div className="flex items-center gap-1 font-semibold text-[var(--color-fg)]">
          {r.private && <Lock size={12} />}
          {r.full_name}
          {r.archived && <span className="text-[10px] uppercase text-amber-500">{t("github.archived")}</span>}
          {r.fork && (
            <span className="flex items-center gap-0.5 text-[10px] text-[var(--color-muted)]">
              <GitFork size={10} /> {t("github.fork")}
            </span>
          )}
          <button
            type="button"
            className="ml-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            onClick={() => void openUrl(r.html_url)}
            title={t("github.open_on_github")}
          >
            <ExternalLink size={12} />
          </button>
        </div>
        {r.description && <div className="text-xs text-[var(--color-muted)]">{r.description}</div>}
        <div className="text-[10px] text-[var(--color-muted)]">
          {[
            r.language,
            r.license,
            r.created_at_ms != null && t("github.created_on", { date: formatDate(r.created_at_ms), age: formatAge(r.created_at_ms, now) }),
            r.size_kb != null && formatSize(r.size_kb),
            r.default_branch,
          ]
            .filter(Boolean)
            .join(" · ")}
        </div>
        {r.homepage && (
          <button type="button" className="text-[10px] text-[var(--color-accent)]" onClick={() => void openUrl(r.homepage!)}>
            {r.homepage.replace(/^https?:\/\//, "")}
          </button>
        )}
        {r.topics.length > 0 && (
          <div className="mt-1 flex flex-wrap gap-1" data-testid="gh-repo-topics">
            {r.topics.map((tp) => (
              <span key={tp} className="rounded-full bg-[var(--color-accent)]/15 px-1.5 py-0.5 text-[10px] text-[var(--color-fg)]">
                {tp}
              </span>
            ))}
          </div>
        )}
      </div>
      <div className="grid grid-cols-2 gap-2">
        <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2">
          <div className="flex items-center gap-1 text-[10px] uppercase text-[var(--color-muted)]">
            <Star size={10} /> {t("github.sort.stars")}
          </div>
          <div className="text-lg font-semibold tabular-nums">{r.stars.toLocaleString(locale())}</div>
          <DeltaRow d={d.star_deltas} />
        </div>
        <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2">
          <div className="flex items-center gap-1 text-[10px] uppercase text-[var(--color-muted)]">
            <GitFork size={10} /> {t("github.sort.forks")}
          </div>
          <div className="text-lg font-semibold tabular-nums">{r.forks.toLocaleString(locale())}</div>
          <DeltaRow d={d.fork_deltas} />
        </div>
      </div>
      <div className="grid grid-cols-4 gap-1.5" data-testid="gh-repo-insights">
        <Tile label={t("github.age_label")} value={age ? formatAge(r.created_at_ms, now) : "—"} />
        <Tile
          label={t("github.stars_per_month_short")}
          value={formatRate(ins.starsPerMonth)}
          hint={r.created_at_ms ? t("github.since_creation") : undefined}
        />
        <Tile
          label={t("github.fork_ratio")}
          value={ins.forkRatio == null ? "—" : `${(ins.forkRatio * 100).toFixed(0)} %`}
          hint={t("github.forks_per_100")}
        />
        <Tile
          label={t("github.last_push_label")}
          value={ins.daysSincePush == null ? "—" : formatAge(r.pushed_at_ms ?? r.updated_at_ms, now)}
          hint={ins.dormant ? <span className="text-amber-500">{t("github.dormant_short")}</span> : undefined}
        />
        <Tile label={t("github.watchers")} value={r.watchers ?? "—"} />
        <Tile label={t("github.open_issues")} value={r.open_issues} hint={t("github.issues_incl_prs")} />
        <Tile
          label={t("github.stars_30d")}
          value={formatDelta(d.star_deltas.d30d)}
        />
        <Tile label={t("github.stars_1y")} value={formatDelta(d.star_deltas.d1y)} />
      </div>
      <LineChart title={t("github.stars_history")} lines={[{ name: "stars", points: d.stars }]} markAt={d.first_seen_ms} testId="gh-repo-stars" />
      <div className="text-[10px] text-[var(--color-muted)]">
        {reconstructed
          ? `${t("github.reconstructed_note")}${d.stars_synced?.truncated ? ` ${t("github.truncated_note")}` : ""}`
          : t("github.reconstruction_pending")}
      </div>
      <GainBars title={t("github.stars_per_month")} gains={d.star_gains} testId="gh-repo-gains" />
      <div className="grid grid-cols-2 gap-2">
        <LineChart title={t("github.sort.forks")} lines={[{ name: "forks", points: d.forks }]} markAt={d.first_seen_ms} testId="gh-repo-forks" />
        <LineChart title={t("github.watchers")} lines={[{ name: "watchers", points: d.watchers }]} />
      </div>
      <LineChart title={t("github.open_issues")} lines={[{ name: "issues", points: d.open_issues }]} />
      {extras ? (
        <RepoExtrasView extras={extras} now={now} />
      ) : (
        <div className="flex items-center gap-2 text-[10px] text-[var(--color-muted)]">
          <Loader2 size={10} className="animate-spin" /> {t("github.loading_extras")}
        </div>
      )}
    </div>
  );
}

function RepoTable({ rows, onOpen }: { rows: GithubRepoRow[]; onOpen: (name: string) => void }) {
  if (rows.length === 0) return <div className="text-xs text-[var(--color-muted)]">{t("github.no_repositories")}</div>;
  return (
    <table className="w-full text-xs" data-testid="gh-repos">
      <thead className="text-[10px] uppercase tracking-wide text-[var(--color-muted)]">
        <tr>
          <th className="text-left font-normal">{t("github.repository")}</th>
          <th className="text-right font-normal">{t("github.sort.stars")}</th>
          <th className="text-right font-normal">{t("github.sort.forks")}</th>
          <th className="text-right font-normal">{t("github.watch")}</th>
          <th className="text-right font-normal">{t("github.issues")}</th>
          <th className="pl-2 text-left font-normal">{t("github.language")}</th>
          <th className="text-right font-normal">{t("github.updated")}</th>
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
              {r.archived && <span className="ml-1 rounded px-1 text-[9px] uppercase text-amber-500">{t("github.archived")}</span>}
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
        <div className="text-xs text-[var(--color-muted)]">{t("github.enter_this_code_on_github")}</div>
        <div className="my-1 font-mono text-2xl tracking-widest text-[var(--color-fg)]">{dev.user_code}</div>
        <div className="flex flex-wrap gap-2 text-xs">
          <button
            type="button"
            className="flex items-center gap-1 rounded bg-[var(--color-accent)] px-2 py-1 text-[var(--color-accent-fg)]"
            onClick={() => void openUrl(dev.verification_uri)}
          >
            <ExternalLink size={12} /> {t("common.open")} {dev.verification_uri.replace(/^https?:\/\//, "")}
          </button>
          <button
            type="button"
            className="flex items-center gap-1 rounded bg-[var(--color-border)] px-2 py-1"
            onClick={() => {
              void writeText(dev.user_code).then(() => setCopied(true));
            }}
          >
            <Copy size={12} /> {copied ? t("common.copied") : t("github.copy_code")}
          </button>
          <button type="button" className="rounded px-2 py-1 text-[var(--color-muted)]" onClick={() => void githubDeviceCancel()}>
            {t("common.cancel")}
          </button>
        </div>
        <div className="mt-2 flex items-center gap-1 text-[10px] text-[var(--color-muted)]">
          <Loader2 size={10} className="animate-spin" /> {t("github.waiting_for_github")}
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
        <GithubMark size={14} /> {t("github.connect")}
      </button>
      {dev.state === "failed" && <div className="mt-2 text-rose-500">{errorText(dev.message)}</div>}
      <div className="mt-2 text-[var(--color-muted)]">
        {status.client_id_configured
          ? t("github.opens_github_com_in_your")
          : errorText("github.no_client_id")}{" "}
        {status.gh_available ? t("github.alt_gh") : t("github.alt")}
      </div>
    </div>
  );
}

export function GithubPanel({ cmd, focused, runSignal, onExit, onQuery }: Props) {
  useT();
  const [status, setStatus] = useState<GithubStatus | null>(null);
  const [dash, setDash] = useState<GithubDashboard | null>(null);
  const [detail, setDetail] = useState<GithubRepoDetail | null>(null);
  const [extras, setExtras] = useState<{ query: string; data: GithubRepoExtras } | null>(null);
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
        .then((d) => {
          if (cancelled) return;
          setDetail(d);
          setMessage(null);
        })
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

  // Commit activity, releases, languages, traffic: fetched on demand (cached
  // a day in the backend) — separately, so the detail never waits for them.
  useEffect(() => {
    if (!repoName) return;
    let cancelled = false;
    const t = window.setTimeout(() => {
      githubRepoExtras(repoName)
        .then((data) => !cancelled && setExtras({ query: repoName, data }))
        .catch(() => {});
    }, 350);
    return () => {
      cancelled = true;
      window.clearTimeout(t);
    };
  }, [repoName, revision]);

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
        if (r.gate.kind === "too_soon") return t("github.refreshed_less_than_a_minute");
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
  // Only the repository asked for — never the previous one while the next loads.
  const shownDetail = detail && repoName && detailMatches(detail, repoName) ? detail : null;
  const shownExtras = extras && repoName && extras.query === repoName ? extras.data : null;
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
          {status?.connected && status.login ? t("github.connected_as", { login: status.login }) : "GitHub"}
        </span>
        {status?.source && <span className="text-[10px] text-[var(--color-muted)]">{SOURCE_LABEL[status.source] ? t(SOURCE_LABEL[status.source]) : status.source}</span>}
        <button
          type="button"
          title={t("github.refresh_now")}
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
              {t(cmd.includePrivate ? "github.export_hint_private" : "github.export_hint", { format: cmd.format.toUpperCase() })}
            </>
          ) : (
            t("github.enter_runs_it")
          )}
        </div>
      )}
      {message && <div className={`text-xs ${message.ok ? "text-emerald-500" : "text-rose-500"}`}>{message.text}</div>}

      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto">
        {status && !status.connected && <ConnectCard status={status} onConnect={() => void connect()} />}

        {!dash && status?.connected && (
          <div className="flex items-center gap-2 text-xs text-[var(--color-muted)]">
            <Loader2 size={12} className="animate-spin" /> {t("github.loading_your_github_statistics")}
          </div>
        )}

        {dash && (
          <>
            <div className="flex items-center justify-between gap-2">
              {/* The ranking uses fixed 7/30-day growth — a range would do nothing there. */}
              {view !== "repos" && view !== "trending" ? <Chips items={RANGES} value={range} onPick={setRange} /> : <span />}
              {dash.history_since_ms && (
                <span className="text-[10px] text-[var(--color-muted)]">{t("github.since", { date: formatDate(dash.history_since_ms) })}</span>
              )}
            </div>

            {view === "dashboard" && (
              <>
                {dash.profile && (
                  <ProfileCard
                    profile={dash.profile}
                    stars={kpis.find((k) => k.key === "stars")?.value ?? 0}
                    repos={kpis.find((k) => k.key === "repos")?.value ?? 0}
                    now={now}
                  />
                )}
                <div className="grid grid-cols-2 gap-2">
                  {kpis.map((k) => (
                    <KpiCard key={k.key} kpi={k} />
                  ))}
                </div>
                <BackfillNote dash={dash} />
                <div className="grid grid-cols-2 gap-2">
                  <LineChart title={t("github.total_stars")} lines={[{ name: "stars", points: dash.stars }]} markAt={dash.history_since_ms} />
                  <LineChart title={t("github.followers")} lines={[{ name: "followers", points: dash.followers }]} />
                </div>
                <GainBars title={t("github.stars_per_month")} gains={dash.star_gains} />
                <div className="grid grid-cols-2 gap-2">
                  <LanguageDonut rows={dash.rows} />
                  <ActivityColumns rows={dash.rows} now={now} />
                </div>
                <div className="grid grid-cols-2 gap-2">
                  <MetricBars title={t("github.stars_by_repository")} rows={dash.rows} metric="stars" color="#f59e0b" onOpen={openRepo} />
                  <MetricBars title={t("github.forks_by_repository")} rows={dash.rows} metric="forks" color="#38bdf8" onOpen={openRepo} />
                </div>
                <RepoTable rows={rows.slice(0, 5)} onOpen={openRepo} />
                <div className="flex gap-3">
                  {dash.rows.length > 5 && (
                    <button type="button" className="text-xs text-[var(--color-accent)]" onClick={() => onQuery("github repos")}>
                      {t("github.all_repos", { n: dash.rows.length })}
                    </button>
                  )}
                  <button type="button" className="text-xs text-[var(--color-accent)]" onClick={() => onQuery("github insights")}>
                    {t("github.more_insights")}
                  </button>
                </div>
              </>
            )}

            {view === "insights" && (
              <div className="space-y-2" data-testid="gh-insights">
                {dash.profile && (
                  <ProfileCard
                    profile={dash.profile}
                    stars={kpis.find((k) => k.key === "stars")?.value ?? 0}
                    repos={kpis.find((k) => k.key === "repos")?.value ?? 0}
                    now={now}
                  />
                )}
                <BackfillNote dash={dash} />
                <div className="grid grid-cols-2 gap-2">
                  <CompositionCard rows={dash.rows} now={now} />
                  <ConcentrationCard rows={dash.rows} />
                </div>
                <div className="grid grid-cols-2 gap-2">
                  <LineChart title={t("github.total_forks")} lines={[{ name: "forks", points: dash.forks }]} markAt={dash.history_since_ms} />
                  <LineChart title={t("github.repo_count")} lines={[{ name: "repos", points: dash.repos }]} markAt={dash.history_since_ms} />
                </div>
                <Card title={t("github.created_per_year")} testId="gh-created-years">
                  <Columns
                    items={createdByYear(dash.rows, now).map((y) => ({ key: y.year, label: String(y.year).slice(2), value: y.count, title: `${y.year}: ${y.count}` }))}
                    color="#8b5cf6"
                    height={44}
                  />
                </Card>
                <FastestCard rows={dash.rows} now={now} onOpen={openRepo} />
                <TopicsCard rows={dash.rows} />
                <DormantCard rows={dash.rows} now={now} onOpen={openRepo} />
              </div>
            )}

            {(view === "repos" || view === "trending") && (
              <>
                {view === "repos" && <Chips items={SORTS} value={sort} onPick={setSortPick} />}
                {view === "trending" && rows.length === 0 && (
                  <div className="text-xs text-[var(--color-muted)]">{t("github.no_repository_gained_stars_in")}</div>
                )}
                <RepoTable rows={rows} onOpen={openRepo} />
              </>
            )}

            {view === "followers" && followersKpi && (
              <>
                <KpiCard kpi={followersKpi} />
                <LineChart title={t("github.followers")} lines={[{ name: "followers", points: dash.followers }]} />
                {dash.profile && (
                  <div className="text-[10px] text-[var(--color-muted)]">{t("github.followers_no_history")}</div>
                )}
              </>
            )}

            {view === "stars" && starsKpi && (
              <>
                <KpiCard kpi={starsKpi} />
                <LineChart title={t("github.total_stars")} lines={[{ name: "stars", points: dash.stars }]} markAt={dash.history_since_ms} />
                <GainBars title={t("github.stars_per_month")} gains={dash.star_gains} />
                <MetricBars title={t("github.stars_by_repository")} rows={dash.rows} metric="stars" color="#f59e0b" onOpen={openRepo} />
                <LineChart title={t("github.top_repositories_stars_over_time")} lines={dash.growth} markAt={dash.history_since_ms} />
              </>
            )}
          </>
        )}

        {view === "repo" && !shownDetail && !message && (
          <div className="flex items-center gap-2 text-xs text-[var(--color-muted)]">
            <Loader2 size={12} className="animate-spin" /> {t("github.loading_repo")}
          </div>
        )}
        {view === "repo" && shownDetail && <RepoDetailView d={shownDetail} extras={shownExtras} now={now} />}
        {status?.connected && status.last_ok_ms == null && !status.last_error && !dash && (
          <div className="text-[10px] text-[var(--color-muted)]">{t("github.first_refresh_this_takes_a")}</div>
        )}
        {dash && dash.private_count > 0 && (
          <div className="text-[10px] text-[var(--color-muted)]">
            <Lock size={9} className="mr-1 inline" />
            {tn("github.private_note", dash.private_count)}
          </div>
        )}
        {status && (
          <div className="text-[10px] text-[var(--color-muted)]">
            {status.rate && `${t("github.rate_left", { left: status.rate.remaining, limit: status.rate.limit })} · `}
            {status.next_refresh_ms && !status.rate_limited_until && status.config.auto_refresh && t("github.next_refresh", { when: formatIn(status.next_refresh_ms, now) })}
          </div>
        )}
      </div>
    </div>
  );
}
