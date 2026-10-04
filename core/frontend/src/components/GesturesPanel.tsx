import { memo, useCallback, useEffect, useRef, useState } from "react";
import { Hand, Keyboard, RotateCcw, Trash2 } from "lucide-react";
import {
  getGestureConfig,
  gestureDefaultGuard,
  gestureLive,
  gestureLiveClear,
  gestureSetGuard,
  setGestureConfig,
  type GestureConfig,
  type GestureGuardConfig,
  type GestureLiveSnapshot,
} from "../lib/ipc";
import {
  CLASS_STYLE,
  centerRect,
  controlValue,
  describeEntry,
  differsFrom,
  EDGE_MAX_PCT,
  EDGE_SIDES,
  edgeRects,
  edgesOf,
  freshFrame,
  GUARD_CONTROLS,
  LIVE_POLL_MS,
  padMillimetres,
  profileFor,
  touchEllipse,
  withControl,
  withEdge,
  type PadProfile,
} from "../lib/gestures-live";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { GestureCalibration } from "./GestureCalibration";
import { GestureRecordings } from "./GestureRecordings";

/** Debounce for saving slider changes — a drag sends one write, not fifty. */
const SAVE_DEBOUNCE_MS = 250;
const PAD_W = 320;

/**
 * The `gestures` panel: a live view of the trackpad with each contact's
 * classification, sliders for the guard thresholds (edge zones per device
 * profile), the last 20 decisions, the guided calibration and the
 * recordings. Polls only while the popup is visible.
 */
export function GesturesPanel({ start = null }: { start?: "calibrate" | "record" | null } = {}) {
  const [cfg, setCfg] = useState<GestureConfig | null>(null);
  const [defaults, setDefaults] = useState<GestureGuardConfig | null>(null);
  const [snap, setSnap] = useState<GestureLiveSnapshot | null>(null);
  const [profile, setProfile] = useState<PadProfile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [visible, setVisible] = useState(true);
  const busy = useRef(false);
  const saveTimer = useRef<number | null>(null);

  useEffect(() => {
    getGestureConfig().then(setCfg).catch((e) => setError(String(e)));
    gestureDefaultGuard().then(setDefaults).catch(() => {});
  }, []);

  useTauriEvent("popup-hidden", () => setVisible(false));
  useTauriEvent("window-shown", () => setVisible(true));

  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    const poll = async () => {
      if (busy.current) return;
      busy.current = true;
      try {
        const s = await gestureLive();
        if (!cancelled) setSnap(s);
      } catch {
        /* a missed poll is redrawn by the next one */
      } finally {
        busy.current = false;
      }
    };
    void poll();
    const id = window.setInterval(() => void poll(), LIVE_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [visible]);

  useEffect(
    () => () => {
      if (saveTimer.current !== null) window.clearTimeout(saveTimer.current);
    },
    [],
  );

  const changeGuard = useCallback((g: GestureGuardConfig) => {
    setCfg((c) => (c ? { ...c, guard: g } : c));
    if (saveTimer.current !== null) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      saveTimer.current = null;
      gestureSetGuard(g)
        .then((saved) => setCfg((c) => (c ? { ...c, guard: saved.guard } : c)))
        .catch((e) => setError(String(e)));
    }, SAVE_DEBOUNCE_MS);
  }, []);

  const toggleEnabled = async () => {
    if (!cfg) return;
    try {
      setCfg(await setGestureConfig({ ...cfg, enabled: !cfg.enabled }));
    } catch (e) {
      setError(String(e));
    }
  };

  const frameDevice = snap?.frame ? snap.devices[snap.frame.device] : snap?.devices[0];
  const activeProfile = profileFor(frameDevice);
  const shown = profile ?? activeProfile;

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto px-4 py-3 text-[12px]">
      <header className="flex items-center gap-2">
        <Hand size={16} className="text-[var(--color-accent)]" />
        <span className="text-sm font-semibold">Gesten-Schutz</span>
        <StatusChip snap={snap} enabled={cfg?.enabled ?? false} />
        <button
          type="button"
          role="switch"
          aria-checked={cfg?.enabled ?? false}
          aria-label="Touchpad-Gesten an/aus"
          onClick={() => void toggleEnabled()}
          disabled={!cfg}
          className={`ml-auto relative h-5 w-9 rounded-full transition-colors duration-(--duration-fast) ${
            cfg?.enabled ? "bg-[var(--color-accent)]" : "bg-[var(--color-border)]"
          }`}
        >
          <span
            className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-transform duration-(--duration-fast) ${
              cfg?.enabled ? "translate-x-4" : "translate-x-0.5"
            }`}
          />
        </button>
      </header>

      {error && <p className="rounded-md bg-rose-500/10 px-2 py-1 text-rose-400">{error}</p>}

      <PadView snap={snap} guard={cfg?.guard ?? null} profile={activeProfile} />

      <GestureCalibration
        status={snap?.calibration}
        supported={snap?.contacts_supported ?? true}
        autoStart={start === "calibrate"}
        onApplied={setCfg}
      />

      {cfg && (
        <Controls
          guard={cfg.guard}
          defaults={defaults}
          profile={shown}
          activeProfile={activeProfile}
          onProfile={setProfile}
          onChange={changeGuard}
        />
      )}

      <DecisionLog snap={snap} />

      <GestureRecordings
        status={snap?.recording}
        supported={snap?.contacts_supported ?? true}
        autoStart={start === "record"}
      />
    </div>
  );
}

function StatusChip({ snap, enabled }: { snap: GestureLiveSnapshot | null; enabled: boolean }) {
  const [label, tone] = !enabled
    ? ["aus", "bg-[var(--color-border)]/60 text-[var(--color-muted)]"]
    : snap?.running
      ? ["läuft", "bg-emerald-500/15 text-emerald-400"]
      : ["keine Erfassung", "bg-amber-500/15 text-amber-400"];
  return <span className={`rounded-full px-2 py-0.5 text-[10px] ${tone}`}>{label}</span>;
}

function PadView({
  snap,
  guard,
  profile,
}: {
  snap: GestureLiveSnapshot | null;
  guard: GestureGuardConfig | null;
  profile: PadProfile;
}) {
  if (snap && !snap.contacts_supported) {
    return (
      <p className="rounded-lg border border-[var(--color-border)] px-3 py-2 text-[var(--color-muted)]">
        Die Live-Ansicht der Kontakte gibt es bisher nur unter macOS. Die Entscheidungen unten werden
        auch hier protokolliert.
      </p>
    );
  }
  const frame = snap ? freshFrame(snap.frame, snap.now_ms) : null;
  const device = snap?.devices[frame?.device ?? 0];
  const mm = padMillimetres(device);
  const h = Math.round((PAD_W * mm.height) / mm.width);
  const zones = guard ? edgesOf(guard, profile) : null;
  return (
    <div className="flex flex-col items-center gap-1.5">
      <div className="relative w-full" style={{ maxWidth: PAD_W }}>
        <svg
          viewBox={`0 0 ${PAD_W} ${h}`}
          className="block w-full"
          role="img"
          aria-label={`Trackpad, ${frame?.touches.length ?? 0} Kontakte`}
        >
          <rect x={0.5} y={0.5} width={PAD_W - 1} height={h - 1} rx={12} fill="var(--color-surface)" stroke="var(--color-border)" />
          {zones &&
            edgeRects(zones, PAD_W, h).map((r) => (
              <rect key={r.side} x={r.x} y={r.y} width={r.w} height={r.h} fill="#f43f5e" opacity={0.12} />
            ))}
          {guard?.release_by_center && (
            <rect
              {...rectProps(centerRect(guard.center_size, PAD_W, h))}
              fill="none"
              stroke="var(--color-muted)"
              strokeDasharray="4 4"
              opacity={snap?.await_center ? 0.9 : 0.35}
            />
          )}
          {frame?.touches.map((t) => {
            const e = touchEllipse(t, PAD_W, h, mm);
            const c = CLASS_STYLE[t.class].color;
            return (
              <g key={t.id} transform={`translate(${e.cx} ${e.cy}) rotate(${e.rotateDeg})`}>
                <ellipse
                  rx={e.rx}
                  ry={e.ry}
                  fill={t.in_edge ? "none" : c}
                  fillOpacity={0.35}
                  stroke={c}
                  strokeWidth={1.5}
                  strokeDasharray={t.in_edge ? "3 3" : undefined}
                />
              </g>
            );
          })}
        </svg>
        {snap?.typing_block && (
          <span className="absolute right-2 top-2 flex items-center gap-1 rounded-full bg-amber-500/20 px-2 py-0.5 text-[10px] text-amber-300">
            <Keyboard size={10} /> Tippen — Lautstärke und Stumm gesperrt
          </span>
        )}
      </div>
      <div className="flex flex-wrap justify-center gap-x-3 gap-y-1 text-[10px] text-[var(--color-muted)]">
        {(["finger", "unclear", "thumb", "palm"] as const).map((k) => (
          <span key={k} className="flex items-center gap-1">
            <span className="inline-block h-2 w-2 rounded-full" style={{ background: CLASS_STYLE[k].color }} />
            {CLASS_STYLE[k].label}
          </span>
        ))}
        <span className="flex items-center gap-1">
          <span className="inline-block h-2 w-3 bg-rose-500/30" /> Randzone
        </span>
      </div>
    </div>
  );
}

function rectProps(r: { x: number; y: number; w: number; h: number }) {
  return { x: r.x, y: r.y, width: r.w, height: r.h };
}

const Controls = memo(function Controls({
  guard,
  defaults,
  profile,
  activeProfile,
  onProfile,
  onChange,
}: {
  guard: GestureGuardConfig;
  defaults: GestureGuardConfig | null;
  profile: PadProfile;
  activeProfile: PadProfile;
  onProfile: (p: PadProfile) => void;
  onChange: (g: GestureGuardConfig) => void;
}) {
  const zones = edgesOf(guard, profile);
  return (
    <section className="flex flex-col gap-2 rounded-lg border border-[var(--color-border)] p-3">
      <div className="flex items-center gap-2">
        <span className="font-medium">Randzonen</span>
        <div className="ml-auto flex gap-1">
          {(["builtin", "external"] as const).map((p) => (
            <button
              key={p}
              type="button"
              onClick={() => onProfile(p)}
              aria-pressed={profile === p}
              className={`rounded-full px-2 py-0.5 text-[10px] ${
                profile === p
                  ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                  : "bg-[var(--color-border)]/50 text-[var(--color-muted)]"
              }`}
            >
              {p === "builtin" ? "Eingebaut" : "Extern"}
              {activeProfile === p ? " · aktiv" : ""}
            </button>
          ))}
        </div>
      </div>
      <div className="grid grid-cols-2 gap-x-3 gap-y-1.5">
        {EDGE_SIDES.map(({ side, label }) => (
          <Slider
            key={side}
            label={label}
            unit="%"
            min={0}
            max={EDGE_MAX_PCT}
            step={1}
            value={Math.round(zones[side] * 100)}
            onChange={(v) => onChange(withEdge(guard, profile, side, v / 100))}
          />
        ))}
      </div>

      <span className="mt-1 font-medium">Schwellen</span>
      {GUARD_CONTROLS.map((c) => (
        <Slider
          key={c.key}
          label={c.label}
          hint={c.hint}
          unit={c.unit}
          min={c.min}
          max={c.max}
          step={c.step}
          value={controlValue(guard, c)}
          onChange={(v) => onChange(withControl(guard, c, v))}
        />
      ))}

      <Toggle
        label="Handballen sperrt alle Gesten"
        checked={guard.palm_blocks_all}
        onChange={(v) => onChange({ ...guard, palm_blocks_all: v })}
      />
      <Toggle
        label="Nach dem Tippen erst Berührung in der Mitte"
        checked={guard.release_by_center}
        onChange={(v) => onChange({ ...guard, release_by_center: v })}
      />
      <Toggle
        label="Wischen nur mit gleichbleibender Fingerzahl"
        checked={guard.constant_count}
        onChange={(v) => onChange({ ...guard, constant_count: v })}
      />

      {defaults && differsFrom(guard, defaults) && (
        <button
          type="button"
          onClick={() => onChange(defaults)}
          className="md3-press mt-1 flex items-center gap-1 self-start rounded-md border border-[var(--color-border)] px-2 py-1 hover:bg-[var(--color-border)]/40"
        >
          <RotateCcw size={12} /> Standardwerte
        </button>
      )}
    </section>
  );
});

function Slider(props: {
  label: string;
  hint?: string;
  unit: string;
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (v: number) => void;
}) {
  return (
    <label className="flex flex-col gap-0.5" title={props.hint}>
      <span className="flex justify-between text-[11px]">
        <span>{props.label}</span>
        <span className="tabular-nums text-[var(--color-muted)]">
          {props.value}
          {props.unit && ` ${props.unit}`}
        </span>
      </span>
      <input
        type="range"
        min={props.min}
        max={props.max}
        step={props.step}
        value={props.value}
        onChange={(e) => props.onChange(Number(e.target.value))}
        className="accent-[var(--color-accent)]"
      />
    </label>
  );
}

function Toggle(props: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex cursor-pointer items-center gap-2 text-[11px]">
      <input type="checkbox" checked={props.checked} onChange={(e) => props.onChange(e.target.checked)} />
      {props.label}
    </label>
  );
}

const TONE: Record<string, string> = {
  fired: "text-emerald-400",
  blocked: "text-amber-400",
  contact: "text-[var(--color-muted)]",
};

function DecisionLog({ snap }: { snap: GestureLiveSnapshot | null }) {
  const rows = snap ? snap.log.map((e) => describeEntry(e, snap.now_ms)) : [];
  return (
    <section className="flex flex-col gap-1">
      <div className="flex items-center">
        <span className="font-medium">Letzte Entscheidungen</span>
        {rows.length > 0 && (
          <button
            type="button"
            onClick={() => void gestureLiveClear()}
            aria-label="Protokoll leeren"
            className="ml-auto rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-border)]/40"
          >
            <Trash2 size={12} />
          </button>
        )}
      </div>
      {rows.length === 0 ? (
        <p className="text-[var(--color-muted)]">Noch keine Geste erkannt.</p>
      ) : (
        <ol className="flex flex-col gap-0.5">
          {rows.map((r) => (
            <li key={r.key} className="flex gap-2 tabular-nums">
              <span className="w-16 shrink-0 text-[var(--color-muted)]">{r.when}</span>
              <span className="min-w-0 flex-1 truncate">
                {r.what}
                {r.action && <span className="text-[var(--color-muted)]"> → {r.action}</span>}
              </span>
              <span className={`shrink-0 ${TONE[r.tone]}`} title={r.level ?? undefined}>
                {r.verdict}
              </span>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
