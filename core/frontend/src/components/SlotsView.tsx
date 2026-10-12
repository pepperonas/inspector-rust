// Consolidated-slots view of the Timesheet tab.
//
// The raw timeline answers "what did the machine see" — hundreds of fragments
// of a few seconds each. This view answers the question a timesheet actually
// asks: "which blocks of work happened today, on what?" It is derived, never
// stored, so the parameters can be changed and the day recomputed at will
// while the raw events stay untouched underneath.
//
// From here the day can be handed to bcsbook, which reviews and books it into
// BCS. That push merges rather than replaces (see tracking/bcsbook.rs).

import { useCallback, useEffect, useState } from "react";
import { Send, RefreshCw, SlidersHorizontal, AlertTriangle } from "lucide-react";
import {
  getSlotConfig,
  setSlotConfig,
  trackPushBcsbook,
  trackSlots,
  type Slot,
  type SlotConfig,
  type SlotOrigin,
} from "../lib/ipc";
import { formatClock, formatDuration } from "../lib/timesheet";
import { t, tn, useT, type MessageKey } from "../lib/i18n";
import { InlineMd } from "./InlineMd";

/** Wording for how a slot got its project — the user needs to know which rows
 *  are a guess before booking them. */
const ORIGIN_LABEL: Record<SlotOrigin, MessageKey> = {
  tagged: "slots.origin.tagged",
  claude: "slots.origin.claude",
  title: "slots.origin.title",
  neighbour: "slots.origin.neighbour",
  unassigned: "slots.origin.unassigned",
};

const ORIGIN_TONE: Record<SlotOrigin, string> = {
  tagged: "bg-emerald-500/15 text-emerald-500",
  claude: "bg-violet-500/15 text-violet-500",
  title: "bg-sky-500/15 text-sky-500",
  neighbour: "bg-amber-500/15 text-amber-500",
  unassigned: "bg-[var(--color-border)] text-[var(--color-muted)]",
};

export function SlotsView({ date }: { date: string }) {
  useT();
  const [slots, setSlots] = useState<Slot[] | null>(null);
  const [cfg, setCfg] = useState<SlotConfig | null>(null);
  const [showParams, setShowParams] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setErr(null);
      const [s, c] = await Promise.all([trackSlots(date), getSlotConfig()]);
      setSlots(s);
      setCfg(c);
    } catch (e) {
      setErr(String(e));
    }
  }, [date]);

  useEffect(() => {
    void load();
  }, [load]);

  // Persist a parameter change and immediately recompute, so the effect of a
  // setting is visible on the spot instead of after a manual refresh.
  const patchCfg = async (patch: Partial<SlotConfig>) => {
    if (!cfg) return;
    const next = { ...cfg, ...patch };
    setCfg(next);
    try {
      await setSlotConfig(next);
      setSlots(await trackSlots(date));
    } catch (e) {
      setErr(String(e));
    }
  };

  const push = async (replace: boolean) => {
    setBusy(true);
    setErr(null);
    setNote(null);
    try {
      const r = await trackPushBcsbook(date, null, replace);
      const parts = [t("slots.push.added", { n: r.added })];
      if (r.skipped > 0) parts.push(t("slots.push.skipped", { n: r.skipped }));
      if (r.unmapped.length > 0) parts.push(t("slots.push.unmapped", { list: r.unmapped.join(", ") }));
      setNote(
        r.added === 0 && r.skipped === 0 && r.unmapped.length > 0
          ? t("slots.push.nothing", { list: r.unmapped.join(", ") })
          : t("slots.push.sent", { parts: parts.join(" · ") }),
      );
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (err && !slots) {
    return <p className="py-3 text-[12px] text-[var(--color-negative,#ef4444)]">{err}</p>;
  }
  if (!slots || !cfg) {
    return <p className="py-3 text-[12px] text-[var(--color-muted)]">{t("slots.calculating")}</p>;
  }

  const totalS = slots.reduce((a, s) => a + s.span_s, 0);
  const mapped = new Set(
    cfg.project_map
      .split("\n")
      .map((l) => l.split("=")[0]?.trim().toLowerCase())
      .filter(Boolean),
  );

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-[12px] text-[var(--color-muted)]">
          {tn("slots.count", slots.length)} · {formatDuration(totalS)}
        </span>
        <span className="flex-1" />
        <button
          type="button"
          onClick={() => void load()}
          className="md3-press flex items-center gap-1 rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[12px] hover:bg-[var(--color-surface)]"
        >
          <RefreshCw size={13} /> {t("slots.recalculate")}
        </button>
        <button
          type="button"
          onClick={() => setShowParams((v) => !v)}
          className="md3-press flex items-center gap-1 rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[12px] hover:bg-[var(--color-surface)]"
        >
          <SlidersHorizontal size={13} /> {t("slots.rules")}
        </button>
        <button
          type="button"
          disabled={busy || slots.length === 0}
          onClick={() => void push(false)}
          className="md3-press flex items-center gap-1 rounded-full bg-[var(--color-accent)] px-3 py-1 text-[12px] font-semibold text-[var(--color-accent-fg)] disabled:opacity-50"
          title={t("slots.adds_to_the_day_in")}
        >
          <Send size={13} /> {t("slots.send_to_bcsbook")}
        </button>
      </div>

      {note && <p className="text-[12px] text-[var(--color-muted)]">{note}</p>}
      {err && <p className="text-[12px] text-[var(--color-negative,#ef4444)]">{err}</p>}

      {showParams && (
        <div className="flex flex-col gap-3 rounded-xl border border-[var(--color-border)] p-3">
          <p className="text-[11px] leading-relaxed text-[var(--color-muted)]">
            <InlineMd text={t("slots.rules_note")} />
          </p>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
            <NumField
              label={t("slots.bridge_gaps_s")}
              hint={t("slots.the_same_project_after_a")}
              value={cfg.bridge_gap_s}
              onChange={(v) => void patchCfg({ bridge_gap_s: v })}
            />
            <NumField
              label={t("slots.short_interruption_s")}
              hint={t("slots.a_different_app_shorter_than")}
              value={cfg.noise_s}
              onChange={(v) => void patchCfg({ noise_s: v })}
            />
            <NumField
              label={t("slots.real_break_from_s")}
              hint={t("slots.idle_time_above_this_splits")}
              value={cfg.min_break_s}
              onChange={(v) => void patchCfg({ min_break_s: v })}
            />
            <NumField
              label={t("slots.minimum_length_s")}
              hint={t("slots.shorter_blocks_are_not_offered")}
              value={cfg.min_slot_s}
              onChange={(v) => void patchCfg({ min_slot_s: v })}
            />
            <NumField
              label={t("slots.grid_min")}
              hint={t("slots.0_exact_times_instead_of")}
              value={cfg.grid_min}
              onChange={(v) => void patchCfg({ grid_min: v })}
            />
            <NumField
              label={t("slots.neighbourhood_s")}
              hint={t("slots.untagged_time_inherits_the_project")}
              value={cfg.neighbour_gap_s}
              onChange={(v) => void patchCfg({ neighbour_gap_s: v })}
            />
          </div>
          <label className="flex flex-col gap-1">
            <span className="text-[12px] font-medium">{t("slots.project_bcsbook_shortcut")}</span>
            <textarea
              rows={4}
              spellCheck={false}
              value={cfg.project_map}
              placeholder={"kiez-finder = kiez\nbcsbook = bcs"}
              onChange={(e) => setCfg({ ...cfg, project_map: e.target.value })}
              onBlur={() => void patchCfg({ project_map: cfg.project_map })}
              className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 font-mono text-[12px]"
            />
            <span className="text-[11px] text-[var(--color-muted)]">
              {t("slots.one_line_per_project_only")}
            </span>
          </label>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={cfg.private_filter}
              onChange={(e) => void patchCfg({ private_filter: e.target.checked })}
            />
            <span className="text-[12px] font-medium">{t("slots.hide_private_apps")}</span>
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-[12px] font-medium">{t("slots.private_apps_hosts_filtered_from")}</span>
            <textarea
              rows={5}
              spellCheck={false}
              disabled={!cfg.private_filter}
              value={cfg.private_apps}
              placeholder={"spotify\nwhatsapp\ntelegram\nyoutube.com"}
              onChange={(e) => setCfg({ ...cfg, private_apps: e.target.value })}
              onBlur={() => void patchCfg({ private_apps: cfg.private_apps })}
              className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 font-mono text-[12px] disabled:opacity-50"
            />
            <span className="text-[11px] text-[var(--color-muted)]">
              {t("slots.one_pattern_per_line_app")}
            </span>
          </label>
          <button
            type="button"
            disabled={busy || slots.length === 0}
            onClick={() => void push(true)}
            className="md3-press self-start rounded-full border border-[var(--color-negative,#ef4444)] px-3 py-1 text-[12px] text-[var(--color-negative,#ef4444)] disabled:opacity-50"
            title={t("slots.replaces_the_whole_day_in")}
          >
            {t("slots.replace_the_day_in_bcsbook")}
          </button>
        </div>
      )}

      {slots.length === 0 ? (
        <p className="py-2 text-[12px] text-[var(--color-muted)]">
          {t("slots.no_slots_for_this_day")}
        </p>
      ) : (
        <div className="flex flex-col">
          {slots.map((s, i) => {
            const needsMapping =
              !!s.project && !mapped.has(s.project.toLowerCase());
            return (
              <div
                key={`${s.start_ms}-${i}`}
                className="flex flex-col gap-1 border-t border-[var(--color-border)]/60 py-2 first:border-t-0"
              >
                <div className="flex flex-wrap items-center gap-2 text-[12px]">
                  <span className="font-mono tabular-nums text-[var(--color-muted)]">
                    {formatClock(s.start_ms)}–{formatClock(s.end_ms)}
                  </span>
                  <span className="font-mono tabular-nums font-semibold">
                    {formatDuration(s.span_s)}
                  </span>
                  <span className="font-semibold">{s.label}</span>
                  <span className={"rounded-full px-2 py-0.5 text-[10px] " + ORIGIN_TONE[s.origin]}>
                    {t(ORIGIN_LABEL[s.origin])}
                  </span>
                  {s.confidence < 0.75 && (
                    <span
                      className="flex items-center gap-1 text-[10px] text-amber-500"
                      title={t("slots.mixed_title", { pct: Math.round(s.confidence * 100) })}
                    >
                      <AlertTriangle size={11} /> {t("slots.mixed")}
                    </span>
                  )}
                  {needsMapping && (
                    <span
                      className="text-[10px] text-[var(--color-muted)]"
                      title={t("slots.this_project_has_no_bcsbook")}
                    >
                      {t("slots.no_shortcut")}
                    </span>
                  )}
                </div>
                <div className="text-[12px] text-[var(--color-muted)]">{s.description}</div>
                <div className="flex flex-wrap gap-1">
                  {s.apps.slice(0, 6).map((a) => (
                    <span
                      key={a.app}
                      className="rounded-full bg-[var(--color-surface)] px-2 py-0.5 text-[10px] text-[var(--color-muted)]"
                      title={`${a.app}: ${formatDuration(a.seconds)}`}
                    >
                      {a.app} {formatDuration(a.seconds)}
                    </span>
                  ))}
                  <span className="px-1 text-[10px] text-[var(--color-muted)]">
                    {tn("slots.from_events", s.event_ids.length)}
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function NumField({
  label,
  hint,
  value,
  onChange,
}: {
  label: string;
  hint: string;
  value: number;
  onChange: (v: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  return (
    <label className="flex flex-col gap-0.5" title={hint}>
      <span className="text-[11px] font-medium">{label}</span>
      <input
        type="number"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          const n = Number(draft);
          if (Number.isFinite(n) && n !== value) onChange(n);
          else setDraft(String(value));
        }}
        className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px]"
      />
    </label>
  );
}
