/**
 * `tokens` — Claude Code usage from the local Token Tracker (`:5010`).
 * Inline preview panel: period chips · Overview (tokens/cost + cache toggle) ·
 * Projects/Sessions · Models. Esc exits. Connection refused → start-hint card.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import {
  Activity,
  AlertCircle,
  Coins,
  FolderKanban,
  Layers,
  Loader2,
  RefreshCw,
} from "lucide-react";
import {
  tokenUsageFetch,
  TOKENS_UNREACHABLE,
  type TokenUsageSnapshot,
  type TokenUsageOverview,
  type TokenUsageDayPeek,
} from "../lib/ipc";
import {
  TOKEN_PERIODS,
  type TokenPeriod,
  displayCost,
  displayTokens,
  formatActiveMin,
  formatCost,
  formatTokens,
  shortProject,
} from "../lib/token-usage";

type Tab = "overview" | "projects" | "models";
type ListMode = "projects" | "sessions";

export function TokensPanel({
  focused,
  onExit,
}: {
  focused: boolean;
  onExit: () => void;
}) {
  const [period, setPeriod] = useState<TokenPeriod>("today");
  const [tab, setTab] = useState<Tab>("overview");
  const [listMode, setListMode] = useState<ListMode>("projects");
  const [includeCache, setIncludeCache] = useState(true);
  const [snap, setSnap] = useState<TokenUsageSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const scrollRef = useRef<HTMLDivElement>(null);
  const alive = useRef(true);
  const seq = useRef(0);

  const load = useCallback(async (p: TokenPeriod, includeSessions: boolean) => {
    const my = ++seq.current;
    setLoading(true);
    setError(null);
    try {
      const s = await tokenUsageFetch(p, includeSessions);
      if (!alive.current || my !== seq.current) return;
      setSnap((prev) => {
        // Keep previously loaded sessions when a fast refresh omits them.
        if (
          !includeSessions &&
          prev &&
          prev.sessions_loaded &&
          prev.from === s.from &&
          prev.to === s.to
        ) {
          return {
            ...s,
            sessions: prev.sessions,
            sessions_loaded: true,
          };
        }
        return s;
      });
    } catch (e) {
      if (!alive.current || my !== seq.current) return;
      const msg = e instanceof Error ? e.message : String(e);
      setSnap(null);
      setError(msg.includes(TOKENS_UNREACHABLE) ? TOKENS_UNREACHABLE : msg);
    } finally {
      if (alive.current && my === seq.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    void load(period, false);
    return () => {
      alive.current = false;
    };
  }, [period, load]);

  // Lazy-load the heavy sessions list only when that sub-view is selected.
  useEffect(() => {
    if (tab !== "projects" || listMode !== "sessions") return;
    if (!snap || snap.sessions_loaded || loading) return;
    void load(period, true);
  }, [tab, listMode, snap, loading, period, load]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onExit();
        return;
      }
      const t = e.target as HTMLElement | null;
      if (
        t &&
        (t.tagName === "INPUT" ||
          t.tagName === "TEXTAREA" ||
          t.isContentEditable)
      ) {
        return;
      }
      if (e.key === "r" || e.key === "R") {
        e.preventDefault();
        void load(period, listMode === "sessions");
        return;
      }
      if (e.key === "ArrowDown" || e.key === "PageDown") {
        e.preventDefault();
        scrollRef.current?.scrollBy({ top: e.key === "PageDown" ? 180 : 48 });
      } else if (e.key === "ArrowUp" || e.key === "PageUp") {
        e.preventDefault();
        scrollRef.current?.scrollBy({ top: e.key === "PageUp" ? -180 : -48 });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focused, onExit, load, period, listMode]);

  return (
    <div className="flex h-full min-h-0 flex-col gap-2 p-3 text-[13px]">
      {/* Header */}
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5 font-medium text-[color:var(--color-fg)]">
          <Coins size={14} className="text-rose-500" />
          Claude tokens
        </div>
        <button
          type="button"
          onClick={() => void load(period, listMode === "sessions")}
          className="md3-press rounded-md p-1 text-[color:var(--color-muted)] hover:bg-[color:var(--color-surface)]"
          title="Refresh (R)"
          aria-label="Refresh"
        >
          <RefreshCw size={13} className={loading ? "animate-spin" : ""} />
        </button>
      </div>

      {/* Period chips */}
      <div className="flex flex-wrap gap-1">
        {TOKEN_PERIODS.map((p) => (
          <button
            key={p.id}
            type="button"
            onClick={() => setPeriod(p.id)}
            className={`md3-press rounded-full px-2.5 py-0.5 text-[11px] font-medium transition-colors ${
              period === p.id
                ? "bg-rose-600 text-white"
                : "bg-[color:var(--color-surface)] text-[color:var(--color-muted)] hover:text-[color:var(--color-fg)]"
            }`}
          >
            {p.label}
          </button>
        ))}
      </div>

      {/* Tabs */}
      <div className="flex gap-1 border-b border-[color:var(--color-border)] pb-1">
        {(
          [
            { id: "overview", label: "Overview", icon: Activity },
            { id: "projects", label: "Projects", icon: FolderKanban },
            { id: "models", label: "Models", icon: Layers },
          ] as const
        ).map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            onClick={() => setTab(id)}
            className={`md3-press inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11px] font-medium ${
              tab === id
                ? "bg-rose-600/15 text-rose-600"
                : "text-[color:var(--color-muted)] hover:text-[color:var(--color-fg)]"
            }`}
          >
            <Icon size={12} />
            {label}
          </button>
        ))}
      </div>

      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto pr-0.5">
        {error === TOKENS_UNREACHABLE ? (
          <UnreachableCard />
        ) : error ? (
          <ErrorCard message={error} onRetry={() => void load(period, false)} />
        ) : loading && !snap ? (
          <div className="flex h-32 items-center justify-center text-[color:var(--color-muted)]">
            <Loader2 size={18} className="animate-spin" />
          </div>
        ) : snap ? (
          tab === "overview" ? (
            <OverviewTab
              o={snap.overview}
              period={period}
              priorDay={snap.prior_day ?? null}
              includeCache={includeCache}
              onToggleCache={() => setIncludeCache((v) => !v)}
              onShowPeriod={setPeriod}
            />
          ) : tab === "projects" ? (
            <ProjectsTab
              snap={snap}
              listMode={listMode}
              onListMode={setListMode}
              includeCache={includeCache}
              loadingSessions={loading && listMode === "sessions" && !snap.sessions_loaded}
            />
          ) : (
            <ModelsTab snap={snap} includeCache={includeCache} />
          )
        ) : null}
      </div>
    </div>
  );
}

function UnreachableCard() {
  return (
    <div className="flex flex-col gap-2 rounded-xl border border-[color:var(--color-border)] bg-[color:var(--color-surface)] p-4">
      <div className="flex items-center gap-2 font-medium text-[color:var(--color-fg)]">
        <AlertCircle size={16} className="text-amber-500" />
        Token Tracker not running
      </div>
      <p className="text-[12px] leading-relaxed text-[color:var(--color-muted)]">
        Start the local Claude Token Tracker dashboard (port{" "}
        <code className="text-[11px]">5010</code>) — e.g. the LaunchAgent{" "}
        <code className="text-[11px]">io.celox.token-tracker</code> or{" "}
        <code className="text-[11px]">node server.js</code> in the
        token-tracker repo — then press R to refresh.
      </p>
    </div>
  );
}

function ErrorCard({
  message,
  onRetry,
}: {
  message: string;
  onRetry: () => void;
}) {
  return (
    <div className="flex flex-col gap-2 rounded-xl border border-[color:var(--color-border)] bg-[color:var(--color-surface)] p-4">
      <div className="font-medium text-[color:var(--color-fg)]">Couldn’t load usage</div>
      <p className="text-[12px] text-[color:var(--color-muted)]">{message}</p>
      <button
        type="button"
        onClick={onRetry}
        className="md3-press self-start rounded-md bg-rose-600 px-2.5 py-1 text-[11px] font-medium text-white"
      >
        Retry
      </button>
    </div>
  );
}

function OverviewTab({
  o,
  period,
  priorDay,
  includeCache,
  onToggleCache,
  onShowPeriod,
}: {
  o: TokenUsageOverview;
  period: TokenPeriod;
  priorDay: TokenUsageDayPeek | null;
  includeCache: boolean;
  onToggleCache: () => void;
  onShowPeriod: (p: TokenPeriod) => void;
}) {
  const tokens = displayTokens(o, includeCache);
  const cost = displayCost(o, includeCache);
  const emptyToday =
    period === "today" && tokens === 0 && (priorDay?.total_tokens ?? 0) > 0;
  return (
    <div className="flex flex-col gap-3">
      {emptyToday && priorDay && (
        <div className="rounded-xl border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-[12px] leading-relaxed text-[color:var(--color-fg)]">
          <div className="font-medium">No Claude Code tokens yet today</div>
          <p className="mt-0.5 text-[color:var(--color-muted)]">
            Cursor / Composer chats aren’t in the Token Tracker — only Claude
            Code JSONL is. Yesterday ({priorDay.date}):{" "}
            <span className="font-medium text-[color:var(--color-fg)]">
              {formatTokens(priorDay.total_tokens)}
            </span>{" "}
            · {formatCost(priorDay.estimated_cost)}.
          </p>
          <button
            type="button"
            onClick={() => onShowPeriod("7d")}
            className="md3-press mt-1.5 rounded-md bg-rose-600 px-2.5 py-1 text-[11px] font-medium text-white"
          >
            Show last 7 days
          </button>
        </div>
      )}

      <div className="grid grid-cols-2 gap-2">
        <Kpi label="Tokens" value={formatTokens(tokens)} />
        <Kpi label="Cost" value={formatCost(cost)} accent />
        <Kpi label="Sessions" value={String(o.sessions)} />
        <Kpi label="Messages" value={formatTokens(o.messages)} />
        <Kpi label="Active time" value={formatActiveMin(o.total_active_min)} />
        <Kpi
          label="Avg / day"
          value={formatActiveMin(o.avg_active_min_per_day)}
        />
      </div>

      <button
        type="button"
        onClick={onToggleCache}
        className={`md3-press self-start rounded-full px-2.5 py-0.5 text-[11px] font-medium ${
          includeCache
            ? "bg-rose-600/15 text-rose-600"
            : "bg-[color:var(--color-surface)] text-[color:var(--color-muted)]"
        }`}
        title="Include cache-read / cache-create tokens in totals"
      >
        {includeCache ? "Cache included" : "Cache excluded"}
      </button>

      <div className="grid grid-cols-2 gap-x-3 gap-y-1 text-[11px] text-[color:var(--color-muted)]">
        <BreakdownRow label="Input" tokens={o.input_tokens} cost={o.input_cost} />
        <BreakdownRow label="Output" tokens={o.output_tokens} cost={o.output_cost} />
        {includeCache && (
          <>
            <BreakdownRow
              label="Cache read"
              tokens={o.cache_read_tokens}
              cost={o.cache_read_cost}
            />
            <BreakdownRow
              label="Cache create"
              tokens={o.cache_create_tokens}
              cost={o.cache_create_cost}
            />
          </>
        )}
      </div>

      <div className="text-[11px] text-[color:var(--color-muted)]">
        Lines +{formatTokens(o.lines_added)} / −{formatTokens(o.lines_removed)}
        {o.rate_limit_hits > 0
          ? ` · ${o.rate_limit_hits} rate-limit hit${o.rate_limit_hits === 1 ? "" : "s"}`
          : ""}
        {o.period_from && o.period_to
          ? ` · ${o.period_from} → ${o.period_to}`
          : ""}
      </div>
    </div>
  );
}

function Kpi({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent?: boolean;
}) {
  return (
    <div
      className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface)] px-2.5 py-2"
      style={{ contain: "content" }}
    >
      <div className="text-[10px] uppercase tracking-wide text-[color:var(--color-muted)]">
        {label}
      </div>
      <div
        className={`mt-0.5 font-semibold tabular-nums ${
          accent ? "text-rose-600" : "text-[color:var(--color-fg)]"
        }`}
      >
        {value}
      </div>
    </div>
  );
}

function BreakdownRow({
  label,
  tokens,
  cost,
}: {
  label: string;
  tokens: number;
  cost: number;
}) {
  return (
    <div className="flex justify-between gap-2">
      <span>{label}</span>
      <span className="tabular-nums">
        {formatTokens(tokens)} · {formatCost(cost)}
      </span>
    </div>
  );
}

function ProjectsTab({
  snap,
  listMode,
  onListMode,
  includeCache,
  loadingSessions,
}: {
  snap: TokenUsageSnapshot;
  listMode: ListMode;
  onListMode: (m: ListMode) => void;
  includeCache: boolean;
  loadingSessions?: boolean;
}) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex gap-1">
        {(["projects", "sessions"] as const).map((m) => (
          <button
            key={m}
            type="button"
            onClick={() => onListMode(m)}
            className={`md3-press rounded-full px-2.5 py-0.5 text-[11px] font-medium capitalize ${
              listMode === m
                ? "bg-rose-600 text-white"
                : "bg-[color:var(--color-surface)] text-[color:var(--color-muted)]"
            }`}
          >
            {m}
          </button>
        ))}
      </div>

      {listMode === "projects" ? (
        <ul className="flex flex-col gap-1">
          {snap.projects.length === 0 ? (
            <Empty />
          ) : (
            snap.projects.map((p) => (
              <li
                key={p.name}
                className="flex items-baseline justify-between gap-2 rounded-md px-1.5 py-1 hover:bg-[color:var(--color-surface)]"
              >
                <div className="min-w-0">
                  <div
                    className="truncate font-medium text-[color:var(--color-fg)]"
                    title={p.name}
                  >
                    {shortProject(p.name)}
                  </div>
                  <div className="text-[10px] text-[color:var(--color-muted)]">
                    {p.sessions} sess · {formatTokens(p.messages)} msg
                  </div>
                </div>
                <div className="shrink-0 text-right tabular-nums">
                  <div className="text-rose-600">{formatCost(p.cost)}</div>
                  <div className="text-[10px] text-[color:var(--color-muted)]">
                    {formatTokens(
                      includeCache
                        ? p.total_tokens
                        : p.input_tokens + p.output_tokens,
                    )}
                  </div>
                </div>
              </li>
            ))
          )}
        </ul>
      ) : loadingSessions ? (
        <div className="flex h-20 items-center justify-center text-[color:var(--color-muted)]">
          <Loader2 size={16} className="animate-spin" />
        </div>
      ) : (
        <ul className="flex flex-col gap-1">
          {snap.sessions.length === 0 ? (
            <Empty />
          ) : (
            snap.sessions.map((s) => (
              <li
                key={s.id}
                className="flex items-baseline justify-between gap-2 rounded-md px-1.5 py-1 hover:bg-[color:var(--color-surface)]"
              >
                <div className="min-w-0">
                  <div
                    className="truncate font-medium text-[color:var(--color-fg)]"
                    title={s.project}
                  >
                    {shortProject(s.project)}
                  </div>
                  <div className="truncate text-[10px] text-[color:var(--color-muted)]">
                    {s.models.join(", ") || "—"} · {formatActiveMin(s.active_min)}
                  </div>
                </div>
                <div className="shrink-0 text-right tabular-nums">
                  <div className="text-rose-600">{formatCost(s.cost)}</div>
                  <div className="text-[10px] text-[color:var(--color-muted)]">
                    {formatTokens(s.total_tokens)}
                  </div>
                </div>
              </li>
            ))
          )}
        </ul>
      )}
    </div>
  );
}

function ModelsTab({
  snap,
  includeCache,
}: {
  snap: TokenUsageSnapshot;
  includeCache: boolean;
}) {
  const total = snap.models.reduce(
    (a, m) => a + (includeCache ? m.total_tokens : m.input_tokens + m.output_tokens),
    0,
  );
  return (
    <ul className="flex flex-col gap-1">
      {snap.models.length === 0 ? (
        <Empty />
      ) : (
        snap.models.map((m) => {
          const tok = includeCache
            ? m.total_tokens
            : m.input_tokens + m.output_tokens;
          const pct = total > 0 ? Math.round((tok / total) * 100) : 0;
          return (
            <li
              key={m.model}
              className="flex flex-col gap-1 rounded-md px-1.5 py-1.5 hover:bg-[color:var(--color-surface)]"
            >
              <div className="flex items-baseline justify-between gap-2">
                <div className="min-w-0 truncate font-medium text-[color:var(--color-fg)]">
                  {m.label}
                </div>
                <div className="shrink-0 text-right tabular-nums">
                  <span className="text-rose-600">{formatCost(m.cost)}</span>
                  <span className="ml-2 text-[10px] text-[color:var(--color-muted)]">
                    {formatTokens(tok)} · {pct}%
                  </span>
                </div>
              </div>
              <div className="h-1 overflow-hidden rounded-full bg-[color:var(--color-border)]">
                <div
                  className="h-full rounded-full bg-rose-600"
                  style={{
                    width: `${pct}%`,
                    transformOrigin: "left",
                    transform: `scaleX(1)`,
                  }}
                />
              </div>
            </li>
          );
        })
      )}
    </ul>
  );
}

function Empty() {
  return (
    <div className="py-8 text-center text-[12px] text-[color:var(--color-muted)] md3-empty-float">
      No data for this period
    </div>
  );
}
