/**
 * `boom` — audio-enhancement controller in the right preview column (same inline
 * family as stats/hue). **Phase 1a:** this configures the tested DSP engine
 * (EQ / presets / boost / effects) + persists it; the live Core-Audio process-tap
 * routing that makes it audible lands in phase 1b (a banner says so). Esc exits.
 */
import { useEffect, useRef, useState } from "react";
import { Volume2, Power, AlertTriangle, Download, Loader2 } from "lucide-react";
import {
  boomAvailable,
  boomDriverInstalled,
  boomInstallDriver,
  boomUninstallDriver,
  boomPresets,
  getBoomConfig,
  setBoomConfig,
  BOOM_BANDS,
  type BoomConfig,
  type BoomPreset,
} from "../lib/ipc";

const EFFECTS: { key: keyof BoomConfig["effects"]; label: string }[] = [
  { key: "bass", label: "Bass" },
  { key: "clarity", label: "Clarity" },
  { key: "ambience", label: "Ambience" },
  { key: "fidelity", label: "Fidelity" },
  { key: "night", label: "Night" },
];

export function BoomPanel({ focused, onExit }: { focused: boolean; onExit: () => void }) {
  const [available, setAvailable] = useState<boolean | null>(null);
  const [driverInstalled, setDriverInstalled] = useState<boolean | null>(null);
  const [installing, setInstalling] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const [cfg, setCfg] = useState<BoomConfig | null>(null);
  const [presets, setPresets] = useState<BoomPreset[]>([]);
  const saveTimer = useRef<number | undefined>(undefined);
  const cfgRef = useRef<BoomConfig | null>(null);

  useEffect(() => {
    boomAvailable().then(setAvailable).catch(() => setAvailable(false));
    boomDriverInstalled().then(setDriverInstalled).catch(() => setDriverInstalled(false));
    boomPresets().then(setPresets).catch(() => {});
    getBoomConfig().then(setCfg).catch(() => {});
  }, []);

  const install = async () => {
    setInstalling(true);
    setInstallError(null);
    try {
      await boomInstallDriver();
      // coreaudiod restart takes a moment — poll for the device to appear.
      for (let i = 0; i < 12; i++) {
        await new Promise((r) => setTimeout(r, 500));
        if (await boomDriverInstalled()) {
          setDriverInstalled(true);
          break;
        }
      }
    } catch (e) {
      setInstallError(String(e));
    } finally {
      setInstalling(false);
    }
  };

  const uninstall = async () => {
    if (cfg?.enabled) update({ enabled: false });
    setInstalling(true);
    try {
      await boomUninstallDriver();
      setDriverInstalled(false);
    } catch (e) {
      setInstallError(String(e));
    } finally {
      setInstalling(false);
    }
  };

  // Debounced persist (the DSP recompute is cheap; avoid a write per drag tick).
  const update = (patch: Partial<BoomConfig>) => {
    setCfg((c) => {
      if (!c) return c;
      const next = { ...c, ...patch };
      cfgRef.current = next;
      window.clearTimeout(saveTimer.current);
      saveTimer.current = window.setTimeout(() => {
        void setBoomConfig(next).catch(() => {});
      }, 250);
      return next;
    });
  };

  // Flush a pending save on unmount.
  useEffect(
    () => () => {
      window.clearTimeout(saveTimer.current);
      if (cfgRef.current) void setBoomConfig(cfgRef.current).catch(() => {});
    },
    [],
  );

  // Esc exits; let form controls keep their own keys (slider arrows etc.).
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "SELECT" || t.tagName === "BUTTON")) {
        if (e.key !== "Escape") return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit]);

  const setBand = (i: number, v: number) => {
    if (!cfg) return;
    const bands = cfg.band_gains_db.slice();
    bands[i] = v;
    update({ band_gains_db: bands, preset: "Custom" });
  };

  const applyPreset = (name: string) => {
    const p = presets.find((x) => x.name === name);
    if (!p) {
      update({ preset: name });
      return;
    }
    update({ preset: name, band_gains_db: p.gains.slice() });
  };

  if (available === false) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center text-[var(--color-muted)]">
        <Volume2 size={22} className="text-[var(--color-accent)]" />
        <p className="text-[13px] text-[var(--color-fg)]">boom needs macOS 14.2 or newer</p>
        <p className="text-[11px]">The driverless audio engine uses Apple’s Core-Audio process taps (macOS 14.2+).</p>
      </div>
    );
  }
  if (!cfg) {
    return <p className="p-4 text-[12px] text-[var(--color-muted)]">Loading…</p>;
  }

  // The virtual audio driver must be installed before boom can route audio.
  if (driverInstalled === false) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
        <Volume2 size={24} className="text-[var(--color-accent)]" />
        <p className="text-[13px] font-medium text-[var(--color-fg)]">Install the boom Audio driver</p>
        <p className="text-[11px] text-[var(--color-muted)]">
          boom needs a small virtual audio device (one-time). The installer asks for your admin
          password and briefly restarts the audio service (~1 s). It routes all system audio through
          the EQ; uninstall any time.
        </p>
        <button
          type="button"
          onClick={() => void install()}
          disabled={installing}
          className="mt-1 flex items-center gap-1.5 rounded-full bg-[var(--color-accent)] px-3.5 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)] disabled:opacity-60"
        >
          {installing ? (
            <>
              <Loader2 size={13} className="animate-spin" /> Installing…
            </>
          ) : (
            <>
              <Download size={13} /> Install driver
            </>
          )}
        </button>
        {installError && <p className="text-[11px] text-red-400">{installError}</p>}
      </div>
    );
  }

  const boosting = cfg.boost_pct > 100;

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)]" style={{ transform: "translateZ(0)" }}>
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <Volume2 size={15} className="text-[var(--color-accent)]" /> boom
        </div>
        <button
          type="button"
          onClick={() => update({ enabled: !cfg.enabled })}
          className={
            "flex items-center gap-1.5 rounded-full px-3 py-1 text-[12px] font-medium transition-colors " +
            (cfg.enabled
              ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
              : "border border-[var(--color-border)] text-[var(--color-muted)]")
          }
        >
          <Power size={13} /> {cfg.enabled ? "On" : "Off"}
        </button>
      </div>

      {/* Driver status + uninstall. */}
      <div className="flex items-center justify-between text-[10px] text-[var(--color-muted)]">
        <span>Routes system audio through the boom Audio driver.</span>
        <button
          type="button"
          onClick={() => void uninstall()}
          disabled={installing}
          className="shrink-0 underline decoration-dotted hover:text-[var(--color-fg)] disabled:opacity-50"
        >
          {installing ? "…" : "Uninstall driver"}
        </button>
      </div>
      {installError && <p className="text-[10px] text-red-400">{installError}</p>}

      {/* Preset */}
      <label className="flex items-center justify-between gap-2 text-[12px]">
        <span className="text-[var(--color-muted)]">Preset</span>
        <select
          value={cfg.preset}
          onChange={(e) => applyPreset(e.target.value)}
          className="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5 text-[12px] text-[var(--color-fg)] outline-none"
        >
          {cfg.preset === "Custom" && <option value="Custom">Custom</option>}
          <optgroup label="Genre">
            {presets.filter((p) => p.group === "Genre").map((p) => (
              <option key={p.name} value={p.name}>{p.name}</option>
            ))}
          </optgroup>
          <optgroup label="Device">
            {presets.filter((p) => p.group === "Device").map((p) => (
              <option key={p.name} value={p.name}>{p.name}</option>
            ))}
          </optgroup>
        </select>
      </label>

      {/* Pre-amp */}
      <Slider
        label="Pre-amp"
        value={cfg.preamp_db}
        min={-12}
        max={12}
        step={0.5}
        suffix=" dB"
        onChange={(v) => update({ preamp_db: v })}
      />

      {/* 10-band graphic EQ */}
      <div className="rounded-xl border border-[var(--color-border)] p-2.5">
        <div className="mb-2 text-[11px] font-medium text-[var(--color-muted)]">Graphic EQ</div>
        <div className="flex items-end justify-between gap-1">
          {BOOM_BANDS.map((label, i) => {
            const g = cfg.band_gains_db[i] ?? 0;
            return (
              <div key={label} className="flex flex-1 flex-col items-center gap-1">
                <span className="text-[9px] tabular-nums text-[var(--color-muted)]">
                  {g > 0 ? "+" : ""}{g.toFixed(0)}
                </span>
                <input
                  type="range"
                  min={-12}
                  max={12}
                  step={0.5}
                  value={g}
                  onChange={(e) => setBand(i, Number(e.target.value))}
                  className="accent-[var(--color-accent)]"
                  style={{ writingMode: "vertical-lr", direction: "rtl", height: 96, width: 16 }}
                />
                <span className="text-[9px] text-[var(--color-muted)]">{label}</span>
              </div>
            );
          })}
        </div>
      </div>

      {/* Volume boost */}
      <div>
        <Slider
          label="Volume boost"
          value={cfg.boost_pct}
          min={0}
          max={300}
          step={5}
          suffix="%"
          tick={100}
          accent={boosting ? "#f59e0b" : undefined}
          onChange={(v) => update({ boost_pct: v })}
        />
        {boosting && (
          <p className="mt-1 flex items-center gap-1 text-[10px] text-amber-500">
            <AlertTriangle size={11} /> Above 100 % — sustained boost can stress internal speakers (limiter active).
          </p>
        )}
        <label className="mt-1.5 flex cursor-pointer items-center gap-2 text-[11px] text-[var(--color-muted)]">
          <input
            type="checkbox"
            checked={cfg.controlled_boost}
            onChange={(e) => update({ controlled_boost: e.target.checked })}
            className="accent-[var(--color-accent)]"
          />
          Controlled boost (stronger limiting for distortion-free high boost)
        </label>
      </div>

      {/* Enhancement effects */}
      <div className="rounded-xl border border-[var(--color-border)] p-2.5">
        <div className="mb-2 text-[11px] font-medium text-[var(--color-muted)]">Enhancement</div>
        <div className="flex flex-col gap-2">
          {EFFECTS.map((fx) => (
            <Slider
              key={fx.key}
              label={fx.label}
              value={Math.round((cfg.effects[fx.key] ?? 0) * 100)}
              min={0}
              max={100}
              step={1}
              suffix="%"
              onChange={(v) => update({ effects: { ...cfg.effects, [fx.key]: v / 100 } })}
            />
          ))}
        </div>
      </div>

      {focused && (
        <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">Esc close</p>
      )}
    </div>
  );
}

function Slider({
  label,
  value,
  min,
  max,
  step,
  suffix = "",
  tick,
  accent,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  suffix?: string;
  tick?: number;
  accent?: string;
  onChange: (v: number) => void;
}) {
  const tickPct = tick != null ? ((tick - min) / (max - min)) * 100 : null;
  return (
    <div>
      <div className="mb-0.5 flex items-center justify-between text-[11px]">
        <span className="text-[var(--color-muted)]">{label}</span>
        <span className="tabular-nums">{value % 1 === 0 ? value : value.toFixed(1)}{suffix}</span>
      </div>
      <div className="relative">
        <input
          type="range"
          min={min}
          max={max}
          step={step}
          value={value}
          onChange={(e) => onChange(Number(e.target.value))}
          className="w-full"
          style={{ accentColor: accent ?? "var(--color-accent)" }}
        />
        {tickPct != null && (
          <span
            className="pointer-events-none absolute top-1/2 h-3 w-px -translate-y-1/2 bg-[var(--color-muted)]"
            style={{ left: `${tickPct}%` }}
          />
        )}
      </div>
    </div>
  );
}
