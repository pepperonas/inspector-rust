/**
 * `boom` — audio-enhancement controller in the right preview column (same inline
 * family as stats/hue). **Phase 1a:** this configures the tested DSP engine
 * (EQ / presets / boost / effects) + persists it; the live Core-Audio process-tap
 * routing that makes it audible lands in phase 1b (a banner says so). Esc exits.
 */
import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Volume2, AlertTriangle, Download, Loader2, RefreshCw } from "lucide-react";
import { IS_WINDOWS } from "../lib/platform";
import {
  boomAvailable,
  boomDriverInstalled,
  boomInstallDriver,
  boomUninstallDriver,
  boomPresets,
  boomLevels,
  getBoomConfig,
  setBoomConfig,
  BOOM_BANDS,
  type BoomConfig,
  type BoomPreset,
  boomUnmute,
} from "../lib/ipc";
import { t, useT } from "../lib/i18n";

const EFFECTS: { key: keyof BoomConfig["effects"]; readonly label: string }[] = [
  { key: "bass", get label() { return t("boom.fx.bass"); } },
  { key: "clarity", get label() { return t("boom.fx.clarity"); } },
  { key: "ambience", get label() { return t("boom.fx.ambience"); } },
  { key: "fidelity", get label() { return t("boom.fx.fidelity"); } },
  // Night is a compressor — no Equalizer-APO equivalent on Windows.
  ...(IS_WINDOWS ? [] : [{ key: "night" as const, get label() { return t("boom.fx.night"); } }]),
];

const EQAPO_URL = "https://sourceforge.net/projects/equalizerapo/";

export function BoomPanel({
  focused,
  onExit,
  onInteract,
}: {
  focused: boolean;
  onExit: () => void;
  /** Called after a mouse interaction so the parent can keep the search field focused. */
  onInteract?: () => void;
}) {
  useT();
  const [available, setAvailable] = useState<boolean | null>(null);
  const [driverInstalled, setDriverInstalled] = useState<boolean | null>(null);
  const [installing, setInstalling] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const [meterIn, setMeterIn] = useState(0);
  const [meterOut, setMeterOut] = useState(0);
  const [clip, setClip] = useState(false);
  const [muted, setMuted] = useState(false);
  const clipTimer = useRef<number | undefined>(undefined);
  const [cfg, setCfg] = useState<BoomConfig | null>(null);
  const [presets, setPresets] = useState<BoomPreset[]>([]);
  const saveTimer = useRef<number | undefined>(undefined);
  const cfgRef = useRef<BoomConfig | null>(null);

  useEffect(() => {
    boomAvailable()
      .then(setAvailable)
      .catch(() => setAvailable(false));
    boomDriverInstalled()
      .then(setDriverInstalled)
      .catch(() => setDriverInstalled(false));
    boomPresets()
      .then(setPresets)
      .catch(() => {});
    getBoomConfig()
      .then(setCfg)
      .catch(() => {});
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
  // `immediate` saves at once (discrete actions: on/off, preset) so they apply
  // instantly; continuous changes (sliders) stay debounced.
  const update = (patch: Partial<BoomConfig>, immediate = false) => {
    setCfg((c) => {
      if (!c) return c;
      const next = { ...c, ...patch };
      cfgRef.current = next;
      window.clearTimeout(saveTimer.current);
      if (immediate) {
        void setBoomConfig(next).catch(() => {});
      } else {
        saveTimer.current = window.setTimeout(() => {
          void setBoomConfig(next).catch(() => {});
        }, 250);
      }
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

  // Poll live levels while the engine is on (read-only; ~8 Hz).
  const enabled = cfg?.enabled ?? false;
  useEffect(() => {
    if (!enabled || IS_WINDOWS) {
      setMeterIn(0);
      setMeterOut(0);
      setClip(false);
      setMuted(false);
      return;
    }
    const id = window.setInterval(() => {
      boomLevels()
        .then((l) => {
          setMeterIn(l.input);
          setMeterOut(l.output);
          setMuted(l.muted);
          if (l.clip) {
            setClip(true);
            window.clearTimeout(clipTimer.current);
            clipTimer.current = window.setTimeout(() => setClip(false), 600);
          }
        })
        .catch(() => {});
    }, 120);
    return () => window.clearInterval(id);
  }, [enabled]);

  // Esc exits; let form controls keep their own keys (slider arrows etc.).
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (
        t &&
        (t.tagName === "INPUT" || t.tagName === "SELECT" || t.tagName === "BUTTON")
      ) {
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
    update({ band_gains_db: bands, preset: t("boom.custom") });
  };

  const applyPreset = (name: string) => {
    const p = presets.find((x) => x.name === name);
    if (!p) {
      update({ preset: name }, true);
      return;
    }
    update({ preset: name, band_gains_db: p.gains.slice() }, true);
  };

  if (available === false) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center text-[var(--color-muted)]">
        <Volume2 size={22} className="text-[var(--color-accent)]" />
        <p className="text-[13px] text-[var(--color-fg)]">
          {t("boom.boom_needs_macos_14_2")}
        </p>
        <p className="text-[11px]">
          {t("boom.the_driverless_audio_engine_uses")}
        </p>
      </div>
    );
  }
  if (!cfg) {
    return <p className="p-4 text-[12px] text-[var(--color-muted)]">{t("boom.loading")}</p>;
  }

  // The audio backend must be present first. Windows: Equalizer APO (boom
  // writes its config — installed manually, link + recheck). macOS: the
  // bundled boom Audio virtual driver (installed in-app).
  if (driverInstalled === false && IS_WINDOWS) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
        <Volume2 size={24} className="text-[var(--color-accent)]" />
        <p className="text-[13px] font-medium text-[var(--color-fg)]">
          {t("boom.install_equalizer_apo")}
        </p>
        <p className="text-[11px] text-[var(--color-muted)]">
          {t("boom.on_windows_boom_drives_equalizer")}
        </p>
        <div className="mt-1 flex items-center gap-2">
          <button
            type="button"
            onClick={() => void openUrl(EQAPO_URL)}
            className="flex items-center gap-1.5 rounded-full bg-[var(--color-accent)] px-3.5 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)]"
          >
            <Download size={13} /> {t("boom.get_equalizer_apo")}
          </button>
          <button
            type="button"
            onClick={() =>
              void boomDriverInstalled()
                .then(setDriverInstalled)
                .catch(() => {})
            }
            className="flex items-center gap-1.5 rounded-full border border-[var(--color-border)] px-3.5 py-1.5 text-[12px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          >
            <RefreshCw size={13} /> {t("boom.check_again")}
          </button>
        </div>
      </div>
    );
  }
  if (driverInstalled === false) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
        <Volume2 size={24} className="text-[var(--color-accent)]" />
        <p className="text-[13px] font-medium text-[var(--color-fg)]">
          {t("boom.install_the_boom_audio_driver")}
        </p>
        <p className="text-[11px] text-[var(--color-muted)]">
          {t("boom.boom_needs_a_small_virtual")}
        </p>
        <button
          type="button"
          onClick={() => void install()}
          disabled={installing}
          className="mt-1 flex items-center gap-1.5 rounded-full bg-[var(--color-accent)] px-3.5 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)] disabled:opacity-60"
        >
          {installing ? (
            <>
              <Loader2 size={13} className="animate-spin" /> {t("boom.installing")}
            </>
          ) : (
            <>
              <Download size={13} /> {t("boom.install_driver")}
            </>
          )}
        </button>
        {installError && <p className="text-[11px] text-red-400">{installError}</p>}
      </div>
    );
  }

  const boosting = cfg.boost_pct > 100;

  return (
    <div
      className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)]"
      style={{ transform: "translateZ(0)" }}
      // After any mouse interaction (button click, slider release) hand focus
      // back to the search field, so the user can type a command without
      // re-clicking the input.
      onPointerUp={() => onInteract?.()}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <Volume2 size={15} className="text-[var(--color-accent)]" /> boom
        </div>
        {/* State-first switch: an "On"-labelled pill read as an ACTION ("press
            to turn on") — users couldn't tell whether boom was active. A real
            switch (knob position + colour) plus an explicit state word is
            unambiguous. */}
        <button
          type="button"
          role="switch"
          aria-checked={cfg.enabled}
          onClick={() => update({ enabled: !cfg.enabled }, true)}
          className="flex items-center gap-2 text-[12px] font-medium"
        >
          <span
            className={
              cfg.enabled ? "text-[var(--color-accent)]" : "text-[var(--color-muted)]"
            }
          >
            {cfg.enabled ? t("boom.active") : t("common.off")}
          </span>
          <span
            className={
              "relative inline-block h-5 w-9 rounded-full transition-colors duration-(--duration-fast) ease-sharp " +
              (cfg.enabled ? "bg-[var(--color-accent)]" : "bg-[var(--color-border)]")
            }
          >
            <span
              className={
                "absolute top-0.5 left-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform duration-(--duration-fast) ease-sharp " +
                (cfg.enabled ? "translate-x-4" : "")
              }
            />
          </span>
        </button>
      </div>

      {/* Driver status + uninstall. */}
      <div className="flex items-center justify-between text-[10px] text-[var(--color-muted)]">
        <span>
          {cfg.enabled
            ? IS_WINDOWS
              ? t("boom.active_applies_system_wide_via")
              : t("boom.active_routing_system_audio_through")
            : t("boom.inactive_audio_passes_through_untouched")}
        </span>
        {!IS_WINDOWS && (
          <button
            type="button"
            onClick={() => void uninstall()}
            disabled={installing}
            className="shrink-0 underline decoration-dotted hover:text-[var(--color-fg)] disabled:opacity-50"
          >
            {installing ? "…" : t("boom.uninstall_driver")}
          </button>
        )}
      </div>
      {installError && <p className="text-[10px] text-red-400">{installError}</p>}

      {/* While boom is OFF the whole settings surface is hidden — audio passes
          through untouched (the engine is stopped / EqAPO renders passthrough),
          so showing EQ/boost/effect controls would suggest they do something. */}
      {!cfg.enabled && (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 p-4 text-center text-[var(--color-muted)]">
          <Volume2 size={20} className="opacity-40" />
          <p className="text-[12px]">{t("boom.boom_is_off_audio_passes")}</p>
          <p className="text-[11px]">{t("boom.turn_it_on_to_configure")}</p>
        </div>
      )}

      {/* Output-muted warning (v0.112.0): a mute on boom Audio has no HUD and
          persisted invisibly — with boom on it read as "boom is broken". */}
      {cfg.enabled && muted && (
        <div className="flex items-center justify-between gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 p-2 text-[11px] text-amber-300">
          <span>{t("boom.output_is_muted_that_s")}</span>
          <button
            type="button"
            className="rounded-md bg-amber-500/20 px-2 py-1 text-[11px] font-medium text-amber-200 hover:bg-amber-500/30"
            onClick={() => {
              void boomUnmute()
                .then(() => setMuted(false))
                .catch(() => {});
            }}
          >
            {t("boom.unmute")}
          </button>
        </div>
      )}

      {/* Live level meters (only while running; no readout on the EqAPO backend). */}
      {cfg.enabled && !IS_WINDOWS && (
        <div className="flex flex-col gap-1 rounded-lg border border-[var(--color-border)] p-2">
          <div className="flex items-center justify-between text-[10px] text-[var(--color-muted)]">
            <span>{t("boom.levels")}</span>
            <span className={"flex items-center gap-1 " + (clip ? "text-red-400" : "")}>
              <span
                className={
                  "inline-block h-1.5 w-1.5 rounded-full " +
                  (clip ? "bg-red-500" : "bg-[var(--color-border)]")
                }
              />
              {t("boom.clip")}
            </span>
          </div>
          <Meter label="in" value={meterIn} />
          <Meter label="out" value={meterOut} />
        </div>
      )}

      {cfg.enabled && (
        <>
          {/* Preset */}
          <label className="flex items-center justify-between gap-2 text-[12px]">
            <span className="text-[var(--color-muted)]">{t("boom.preset")}</span>
            <select
              value={cfg.preset}
              onChange={(e) => {
                applyPreset(e.target.value);
                onInteract?.();
              }}
              className="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5 text-[12px] text-[var(--color-fg)] outline-none"
            >
              {cfg.preset === t("boom.custom") && <option value="Custom">Custom</option>}
              <optgroup label={t("boom.genre")}>
                {presets
                  .filter((p) => p.group === "Genre")
                  .map((p) => (
                    <option key={p.name} value={p.name}>
                      {p.name}
                    </option>
                  ))}
              </optgroup>
              <optgroup label={t("boom.device")}>
                {presets
                  .filter((p) => p.group === "Device")
                  .map((p) => (
                    <option key={p.name} value={p.name}>
                      {p.name}
                    </option>
                  ))}
              </optgroup>
            </select>
          </label>

          {/* Pre-amp */}
          <Slider
            label={t("boom.pre_amp")}
            value={cfg.preamp_db}
            min={-12}
            max={12}
            step={0.5}
            suffix=" dB"
            onChange={(v) => update({ preamp_db: v })}
          />

          {/* 10-band graphic EQ */}
          <div className="rounded-xl border border-[var(--color-border)] p-2.5">
            <div className="mb-2 text-[11px] font-medium text-[var(--color-muted)]">
              {t("boom.graphic_eq")}
            </div>
            <div className="flex items-end justify-between gap-1">
              {BOOM_BANDS.map((label, i) => {
                const g = cfg.band_gains_db[i] ?? 0;
                return (
                  <div key={label} className="flex flex-1 flex-col items-center gap-1">
                    <span className="text-[9px] tabular-nums text-[var(--color-muted)]">
                      {g > 0 ? "+" : ""}
                      {g.toFixed(0)}
                    </span>
                    <input
                      type="range"
                      min={-12}
                      max={12}
                      step={0.5}
                      value={g}
                      onChange={(e) => setBand(i, Number(e.target.value))}
                      className="accent-[var(--color-accent)]"
                      style={{
                        writingMode: "vertical-lr",
                        direction: "rtl",
                        height: 96,
                        width: 16,
                      }}
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
              label={t("boom.volume_boost")}
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
                <AlertTriangle size={11} /> {t("boom.above_100_sustained_boost_can")}
              </p>
            )}
            <label className="mt-1.5 flex cursor-pointer items-center gap-2 text-[11px] text-[var(--color-muted)]">
              <input
                type="checkbox"
                checked={cfg.controlled_boost}
                onChange={(e) => update({ controlled_boost: e.target.checked })}
                className="accent-[var(--color-accent)]"
              />
              {t("boom.controlled_boost_stronger_limiting_for")}
            </label>
          </div>

          {/* Enhancement effects */}
          <div className="rounded-xl border border-[var(--color-border)] p-2.5">
            <div className="mb-2 text-[11px] font-medium text-[var(--color-muted)]">
              {t("boom.enhancement")}
            </div>
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
                  onChange={(v) =>
                    update({ effects: { ...cfg.effects, [fx.key]: v / 100 } })
                  }
                />
              ))}
            </div>
          </div>
        </>
      )}

      {focused && (
        <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">{t("boom.esc_close")}</p>
      )}
    </div>
  );
}

/** A VU-style level meter bar (RMS 0..1, scaled for visibility; amber near peak). */
function Meter({ label, value }: { label: string; value: number }) {
  const w = Math.min(1, Math.max(0, value * 2.5));
  return (
    <div className="flex items-center gap-2">
      <span className="w-6 text-[9px] text-[var(--color-muted)]">{label}</span>
      <div className="relative h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--color-border)]">
        <div
          className="absolute inset-0 origin-left rounded-full"
          style={{
            transform: `scaleX(${w})`,
            backgroundColor:
              w > 0.85 ? "#ef4444" : w > 0.6 ? "#f59e0b" : "var(--color-accent)",
            transition: "transform 90ms linear",
          }}
        />
      </div>
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
        <span className="tabular-nums">
          {value % 1 === 0 ? value : value.toFixed(1)}
          {suffix}
        </span>
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
