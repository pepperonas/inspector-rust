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

/** Wording for how a slot got its project — the user needs to know which rows
 *  are a guess before booking them. */
const ORIGIN_LABEL: Record<SlotOrigin, string> = {
  tagged: "manuell zugeordnet",
  claude: "aus Claude-Projekt",
  title: "aus Fenstertitel",
  neighbour: "aus Nachbarschaft",
  unassigned: "kein Projekt",
};

const ORIGIN_TONE: Record<SlotOrigin, string> = {
  tagged: "bg-emerald-500/15 text-emerald-500",
  claude: "bg-violet-500/15 text-violet-500",
  title: "bg-sky-500/15 text-sky-500",
  neighbour: "bg-amber-500/15 text-amber-500",
  unassigned: "bg-[var(--color-border)] text-[var(--color-muted)]",
};

export function SlotsView({ date }: { date: string }) {
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
      const parts = [`${r.added} übernommen`];
      if (r.skipped > 0) parts.push(`${r.skipped} übersprungen (Zeit schon belegt)`);
      if (r.unmapped.length > 0) parts.push(`ohne Zuordnung: ${r.unmapped.join(", ")}`);
      setNote(
        r.added === 0 && r.skipped === 0 && r.unmapped.length > 0
          ? `Nichts gesendet — ${r.unmapped.join(", ")} braucht einen bcsbook-Shortcut.`
          : `An bcsbook gesendet (${parts.join(" · ")}).`,
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
    return <p className="py-3 text-[12px] text-[var(--color-muted)]">Wird berechnet…</p>;
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
          {slots.length} {slots.length === 1 ? "Slot" : "Slots"} · {formatDuration(totalS)}
        </span>
        <span className="flex-1" />
        <button
          type="button"
          onClick={() => void load()}
          className="md3-press flex items-center gap-1 rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[12px] hover:bg-[var(--color-surface)]"
        >
          <RefreshCw size={13} /> Neu berechnen
        </button>
        <button
          type="button"
          onClick={() => setShowParams((v) => !v)}
          className="md3-press flex items-center gap-1 rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[12px] hover:bg-[var(--color-surface)]"
        >
          <SlidersHorizontal size={13} /> Regeln
        </button>
        <button
          type="button"
          disabled={busy || slots.length === 0}
          onClick={() => void push(false)}
          className="md3-press flex items-center gap-1 rounded-full bg-[var(--color-accent)] px-3 py-1 text-[12px] font-semibold text-[var(--color-accent-fg)] disabled:opacity-50"
          title="Ergänzt den Tag in bcsbook; bestehende Einträge bleiben unangetastet."
        >
          <Send size={13} /> An bcsbook
        </button>
      </div>

      {note && <p className="text-[12px] text-[var(--color-muted)]">{note}</p>}
      {err && <p className="text-[12px] text-[var(--color-negative,#ef4444)]">{err}</p>}

      {showParams && (
        <div className="flex flex-col gap-3 rounded-xl border border-[var(--color-border)] p-3">
          <p className="text-[11px] leading-relaxed text-[var(--color-muted)]">
            Diese Regeln bestimmen nur die <strong>Darstellung</strong> — die aufgezeichneten
            Rohdaten bleiben unverändert und lassen sich jederzeit anders zusammenfassen.
          </p>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
            <NumField
              label="Lücke überbrücken (s)"
              hint="Gleiches Projekt nach kurzer Unterbrechung bleibt ein Slot."
              value={cfg.bridge_gap_s}
              onChange={(v) => void patchCfg({ bridge_gap_s: v })}
            />
            <NumField
              label="Kurzstörung (s)"
              hint="Fremde App darunter zerreißt den Block nicht."
              value={cfg.noise_s}
              onChange={(v) => void patchCfg({ noise_s: v })}
            />
            <NumField
              label="Echte Pause ab (s)"
              hint="Leerlauf darüber trennt den Tag."
              value={cfg.min_break_s}
              onChange={(v) => void patchCfg({ min_break_s: v })}
            />
            <NumField
              label="Mindestlänge (s)"
              hint="Kürzere Blöcke werden nicht angeboten."
              value={cfg.min_slot_s}
              onChange={(v) => void patchCfg({ min_slot_s: v })}
            />
            <NumField
              label="Raster (min)"
              hint="0 = exakte Zeiten statt Viertelstunden."
              value={cfg.grid_min}
              onChange={(v) => void patchCfg({ grid_min: v })}
            />
            <NumField
              label="Nachbarschaft (s)"
              hint="Ungetaggte Zeit erbt das Projekt daneben. 0 = aus."
              value={cfg.neighbour_gap_s}
              onChange={(v) => void patchCfg({ neighbour_gap_s: v })}
            />
          </div>
          <label className="flex flex-col gap-1">
            <span className="text-[12px] font-medium">Projekt → bcsbook-Shortcut</span>
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
              Eine Zeile je Projekt. Nur zugeordnete Projekte werden an bcsbook gesendet.
            </span>
          </label>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={cfg.private_filter}
              onChange={(e) => void patchCfg({ private_filter: e.target.checked })}
            />
            <span className="text-[12px] font-medium">Private Apps ausblenden</span>
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-[12px] font-medium">Private Apps / Hosts (aus Slots + Export gefiltert)</span>
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
              Ein Muster je Zeile (App-Name oder Host, Teilstring). Eine Zeile entfernen = App wieder
              erfassen. Die Roh-Timeline bleibt vollständig.
            </span>
          </label>
          <button
            type="button"
            disabled={busy || slots.length === 0}
            onClick={() => void push(true)}
            className="md3-press self-start rounded-full border border-[var(--color-negative,#ef4444)] px-3 py-1 text-[12px] text-[var(--color-negative,#ef4444)] disabled:opacity-50"
            title="Ersetzt den kompletten Tag in bcsbook — auch dort bereits vorhandene Einträge."
          >
            Tag in bcsbook ersetzen
          </button>
        </div>
      )}

      {slots.length === 0 ? (
        <p className="py-2 text-[12px] text-[var(--color-muted)]">
          Keine Slots für diesen Tag — entweder wurde nichts aufgezeichnet, oder alle Blöcke liegen
          unter der Mindestlänge.
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
                    {ORIGIN_LABEL[s.origin]}
                  </span>
                  {s.confidence < 0.75 && (
                    <span
                      className="flex items-center gap-1 text-[10px] text-amber-500"
                      title={`Nur ${Math.round(s.confidence * 100)} % der Zeit entfallen auf das führende Projekt — gemischter Block, vor dem Buchen prüfen.`}
                    >
                      <AlertTriangle size={11} /> gemischt
                    </span>
                  )}
                  {needsMapping && (
                    <span
                      className="text-[10px] text-[var(--color-muted)]"
                      title="Für dieses Projekt fehlt ein bcsbook-Shortcut — es wird nicht gesendet."
                    >
                      kein Shortcut
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
                    aus {s.event_ids.length} Einträgen
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
