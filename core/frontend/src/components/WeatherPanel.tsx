/**
 * `weather` — current conditions + a 5-day forecast for your location, inline
 * in the right preview column (same panel family as stats / hue / calendar).
 *
 * "My location" is IP-geolocated by the backend; a `weather <city>` argument
 * (the live command arg) overrides it. Read-only: while `focused`, Esc exits,
 * ↑/↓ scroll, R refreshes. Each condition renders an **animated scene** (sun,
 * drifting clouds, falling rain/snow, lightning, mist), all
 * `prefers-reduced-motion`-guarded. The deterministic bits (labels, unit
 * conversion, dates) are the pure, unit-tested `lib/weather.ts`.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AlertTriangle,
  Droplets,
  Gauge,
  KeyRound,
  MapPin,
  RefreshCw,
  Wind,
} from "lucide-react";
import {
  setWeatherConfig,
  weatherFetch,
  WEATHER_NO_KEY,
  type WeatherKind,
  type WeatherReport,
} from "../lib/ipc";
import {
  dayName,
  hasPrecip,
  isNight,
  isToday,
  mpsToKmh,
  roundTemp,
  windCompass,
  WEATHER_GRADIENT,
  WEATHER_LABELS,
} from "../lib/weather";

// ── Animation stylesheet (injected once) ────────────────────────────────────

const WX_STYLE_ID = "wx-scene-styles";
const WX_CSS = `
@keyframes wx-spin { to { transform: rotate(360deg); } }
@keyframes wx-glow { 0%,100% { opacity:.55; transform:scale(1); } 50% { opacity:.9; transform:scale(1.08); } }
@keyframes wx-bob { 0%,100% { transform:translateY(0); } 50% { transform:translateY(-6%); } }
@keyframes wx-drift { from { transform:translateX(-18%);} to { transform:translateX(118%);} }
@keyframes wx-drift-slow { from { transform:translateX(-25%);} to { transform:translateX(125%);} }
@keyframes wx-rain { 0% { transform:translateY(-120%); opacity:0; } 12% { opacity:.9; } 100% { transform:translateY(240%); opacity:0; } }
@keyframes wx-snow { 0% { transform:translateY(-120%) translateX(0); opacity:0; } 15% { opacity:.95; } 100% { transform:translateY(260%) translateX(8px); opacity:0; } }
@keyframes wx-twinkle { 0%,100% { opacity:.25; } 50% { opacity:1; } }
@keyframes wx-flash { 0%,92%,100% { opacity:0; } 93%,97% { opacity:.85; } 95% { opacity:.25; } }
@keyframes wx-mist { 0%,100% { transform:translateX(-6%); opacity:.5; } 50% { transform:translateX(6%); opacity:.85; } }
@media (prefers-reduced-motion: reduce) {
  .wx-scene * { animation: none !important; }
}
`;
function ensureWxStyles() {
  if (typeof document === "undefined") return;
  if (document.getElementById(WX_STYLE_ID)) return;
  const el = document.createElement("style");
  el.id = WX_STYLE_ID;
  el.textContent = WX_CSS;
  document.head.appendChild(el);
}

// ── The animated scene ──────────────────────────────────────────────────────

/** A self-contained animated weather scene filling its (relative) parent.
 *  `compact` thins particle counts for the small forecast cards. */
function WeatherScene({ kind, compact = false }: { kind: WeatherKind; compact?: boolean }) {
  useEffect(ensureWxStyles, []);
  const [top, bottom] = WEATHER_GRADIENT[kind];
  const night = isNight(kind);

  // Deterministic-ish particle layouts, memoised so they don't reshuffle on
  // every render/refresh (index-based positions — no reflow).
  const drops = useMemo(
    () =>
      Array.from({ length: compact ? 6 : 14 }, (_, i) => ({
        left: `${(i * 37 + 7) % 96}%`,
        delay: `${(i * 0.17) % 1.6}s`,
        dur: `${0.8 + ((i * 13) % 7) / 10}s`,
      })),
    [compact],
  );
  const flakes = useMemo(
    () =>
      Array.from({ length: compact ? 7 : 16 }, (_, i) => ({
        left: `${(i * 29 + 5) % 95}%`,
        delay: `${(i * 0.23) % 2.4}s`,
        dur: `${2.4 + ((i * 17) % 9) / 10}s`,
        size: compact ? 3 : 4 + ((i * 7) % 4),
      })),
    [compact],
  );
  const stars = useMemo(
    () =>
      Array.from({ length: compact ? 6 : 22 }, (_, i) => ({
        left: `${(i * 41 + 9) % 97}%`,
        top: `${(i * 23 + 5) % 62}%`,
        delay: `${(i * 0.31) % 3}s`,
        size: 1 + ((i * 5) % 3),
      })),
    [compact],
  );

  const precip = hasPrecip(kind);
  const showCloud = kind === "clouds" || kind === "drizzle" || kind === "rain" || kind === "thunderstorm" || kind === "snow" || kind === "mist";

  return (
    <div
      className="wx-scene absolute inset-0 overflow-hidden"
      style={{ background: `linear-gradient(to bottom, ${top}, ${bottom})` }}
      aria-hidden
    >
      {/* Clear sun / moon */}
      {kind === "clear-day" && (
        <div
          className="absolute"
          style={{
            top: compact ? "18%" : "22%",
            left: "50%",
            transform: "translateX(-50%)",
            animation: "wx-bob 6s ease-in-out infinite",
          }}
        >
          <div className="relative" style={{ width: compact ? 42 : 78, height: compact ? 42 : 78 }}>
            {/* Rays */}
            <div
              className="absolute inset-0"
              style={{
                animation: "wx-spin 60s linear infinite",
                background:
                  "conic-gradient(from 0deg, transparent 0 8%, rgba(255,240,170,.7) 9% 11%, transparent 12% 100%)",
                WebkitMaskImage:
                  "radial-gradient(circle, transparent 46%, #000 47%)",
                maskImage: "radial-gradient(circle, transparent 46%, #000 47%)",
                borderRadius: "50%",
              }}
            />
            {/* Glow */}
            <div
              className="absolute inset-[18%] rounded-full"
              style={{
                background: "radial-gradient(circle, #fff6c9, #ffd85e 70%)",
                boxShadow: "0 0 26px 8px rgba(255,220,120,.75)",
                animation: "wx-glow 4s ease-in-out infinite",
              }}
            />
          </div>
        </div>
      )}
      {night && (
        <>
          {stars.map((s, i) => (
            <div
              key={i}
              className="absolute rounded-full bg-white"
              style={{
                left: s.left,
                top: s.top,
                width: s.size,
                height: s.size,
                animation: `wx-twinkle ${2 + (i % 3)}s ease-in-out ${s.delay} infinite`,
              }}
            />
          ))}
          <div
            className="absolute rounded-full"
            style={{
              top: compact ? "16%" : "20%",
              right: compact ? "18%" : "24%",
              width: compact ? 34 : 58,
              height: compact ? 34 : 58,
              background: "radial-gradient(circle at 35% 35%, #fdf6d8, #d8dcc0 75%)",
              boxShadow: "0 0 22px 4px rgba(230,230,200,.5)",
              animation: "wx-bob 7s ease-in-out infinite",
            }}
          />
        </>
      )}

      {/* Drifting clouds (shared by cloudy / precip scenes) */}
      {showCloud && (
        <>
          <Cloud
            top={compact ? "12%" : "16%"}
            scale={compact ? 0.6 : 1}
            dur={compact ? 26 : 34}
            tint={precip || kind === "mist" ? "#e9edf2" : "#ffffff"}
          />
          {!compact && (
            <Cloud top="34%" scale={0.72} dur={46} delay={-12} tint={precip ? "#dfe5ec" : "#f3f6fa"} />
          )}
        </>
      )}

      {/* Rain / drizzle drops */}
      {precip &&
        drops.map((d, i) => (
          <div
            key={i}
            className="absolute rounded-full"
            style={{
              left: d.left,
              top: "30%",
              width: kind === "drizzle" ? 1.5 : 2,
              height: kind === "drizzle" ? 7 : compact ? 8 : 12,
              background: "linear-gradient(to bottom, rgba(200,225,255,.15), rgba(200,225,255,.85))",
              animation: `wx-rain ${d.dur} linear ${d.delay} infinite`,
            }}
          />
        ))}

      {/* Snow */}
      {kind === "snow" &&
        flakes.map((f, i) => (
          <div
            key={i}
            className="absolute rounded-full bg-white"
            style={{
              left: f.left,
              top: "26%",
              width: f.size,
              height: f.size,
              opacity: 0.9,
              animation: `wx-snow ${f.dur} linear ${f.delay} infinite`,
            }}
          />
        ))}

      {/* Lightning flash overlay */}
      {kind === "thunderstorm" && (
        <div
          className="absolute inset-0"
          style={{
            background: "radial-gradient(circle at 50% 30%, rgba(255,255,255,.9), transparent 55%)",
            animation: "wx-flash 6s ease-out infinite",
          }}
        />
      )}

      {/* Mist bands */}
      {kind === "mist" &&
        Array.from({ length: compact ? 3 : 5 }, (_, i) => (
          <div
            key={i}
            className="absolute rounded-full"
            style={{
              left: "-10%",
              right: "-10%",
              top: `${30 + i * 12}%`,
              height: compact ? 6 : 10,
              background: "linear-gradient(90deg, transparent, rgba(255,255,255,.7), transparent)",
              animation: `wx-mist ${5 + i}s ease-in-out ${-i * 0.7}s infinite`,
            }}
          />
        ))}

      {/* Bottom legibility scrim */}
      <div
        className="absolute inset-x-0 bottom-0 h-1/2"
        style={{ background: "linear-gradient(to top, rgba(0,0,0,.35), transparent)" }}
      />
    </div>
  );
}

/** A soft, drifting cloud puff built from overlapping rounded rectangles. */
function Cloud({
  top,
  scale,
  dur,
  delay = 0,
  tint,
}: {
  top: string;
  scale: number;
  dur: number;
  delay?: number;
  tint: string;
}) {
  return (
    <div
      className="absolute"
      style={{
        top,
        left: 0,
        transform: `scale(${scale})`,
        transformOrigin: "left center",
        animation: `wx-drift ${dur}s linear ${delay}s infinite`,
      }}
    >
      <div className="relative" style={{ width: 90, height: 34 }}>
        <div className="absolute rounded-full" style={{ left: 0, top: 12, width: 46, height: 22, background: tint, opacity: 0.92 }} />
        <div className="absolute rounded-full" style={{ left: 22, top: 2, width: 40, height: 32, background: tint, opacity: 0.95 }} />
        <div className="absolute rounded-full" style={{ left: 46, top: 10, width: 44, height: 24, background: tint, opacity: 0.9 }} />
        <div className="absolute rounded-full" style={{ left: 8, top: 18, width: 78, height: 16, background: tint, opacity: 0.85 }} />
      </div>
    </div>
  );
}

// ── The panel ───────────────────────────────────────────────────────────────

type Status = "loading" | "ok" | "error" | "no-key";

export function WeatherPanel({
  focused,
  arg,
  onExit,
  onInteract,
}: {
  focused: boolean;
  /** Live command argument (`weather berlin`) — a city override. Empty = IP. */
  arg: string;
  onExit: () => void;
  onInteract?: () => void;
}) {
  const [report, setReport] = useState<WeatherReport | null>(null);
  const [status, setStatus] = useState<Status>("loading");
  const [error, setError] = useState("");
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [savingKey, setSavingKey] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const seq = useRef(0);

  const load = useCallback(async (city: string) => {
    const mine = ++seq.current;
    setStatus((s) => (s === "ok" ? "ok" : "loading"));
    setError("");
    try {
      const rep = await weatherFetch(city);
      if (mine !== seq.current) return; // stale
      setReport(rep);
      setStatus("ok");
      setUpdatedAt(Date.now());
    } catch (e) {
      if (mine !== seq.current) return;
      const msg = String(e);
      if (msg.includes(WEATHER_NO_KEY)) {
        setStatus("no-key");
      } else {
        setStatus("error");
        setError(msg.replace(/^.*Error:\s*/, ""));
      }
    }
  }, []);

  // Load on mount + whenever the city arg changes (debounced), and refresh
  // every 10 minutes.
  useEffect(() => {
    const city = arg.trim();
    const t = setTimeout(() => void load(city), report ? 350 : 0);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [arg, load]);

  useEffect(() => {
    const id = setInterval(() => void load(arg.trim()), 10 * 60 * 1000);
    return () => clearInterval(id);
  }, [arg, load]);

  // Keyboard while focused.
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      // Escape exits weather from anywhere (a city name never contains Esc).
      if (e.key === "Escape") {
        onExit();
        e.preventDefault();
        e.stopPropagation();
        return;
      }
      // The city is TYPED into the search field, so the search input owns the
      // keyboard — the panel must NEVER swallow a key destined for a text input.
      // (This was the bug: the `r`/`R` refresh shortcut ate the "r" in
      // `weather darmstadt`, leaving "damstadt".) The panel's scroll/refresh
      // keys therefore only fire when the user is NOT typing (e.g. clicked the
      // panel), so every character of the city — r included — reaches the field.
      const tgt = e.target as HTMLElement | null;
      if (
        tgt &&
        (tgt.tagName === "INPUT" ||
          tgt.tagName === "TEXTAREA" ||
          tgt.isContentEditable)
      ) {
        return;
      }
      const el = scrollRef.current;
      switch (e.key) {
        case "ArrowDown":
          el?.scrollBy({ top: 60 });
          break;
        case "ArrowUp":
          el?.scrollBy({ top: -60 });
          break;
        case "PageDown":
          el?.scrollBy({ top: 240 });
          break;
        case "PageUp":
          el?.scrollBy({ top: -240 });
          break;
        case "r":
        case "R":
          void load(arg.trim());
          break;
        default:
          return;
      }
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit, load, arg]);

  const saveKey = async () => {
    const k = keyInput.trim();
    if (!k) return;
    setSavingKey(true);
    try {
      await setWeatherConfig(k, null);
      setKeyInput("");
      await load(arg.trim());
    } catch (e) {
      setError(String(e));
    } finally {
      setSavingKey(false);
    }
  };

  // ── No-key connect card ───────────────────────────────────────────────────
  if (status === "no-key") {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4 p-6 text-center">
        <div className="rounded-full bg-[var(--color-accent)]/10 p-4">
          <KeyRound size={28} className="text-[var(--color-accent)]" />
        </div>
        <div>
          <h3 className="text-[15px] font-semibold text-[var(--color-fg)]">Add your OpenWeather key</h3>
          <p className="mx-auto mt-1 max-w-xs text-[12px] leading-relaxed text-[var(--color-muted)]">
            Weather needs a free{" "}
            <span className="text-[var(--color-fg)]">OpenWeatherMap</span> API key. Paste it
            below (or set it in Settings → Weather). It's stored locally, never sent anywhere but
            OpenWeather.
          </p>
        </div>
        <div className="flex w-full max-w-xs items-center gap-2">
          <input
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void saveKey();
              e.stopPropagation();
            }}
            onFocus={() => onInteract?.()}
            placeholder="OpenWeather API key"
            className="min-w-0 flex-1 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5 font-mono text-[12px] outline-none focus:border-[var(--color-accent)]"
          />
          <button
            onClick={() => void saveKey()}
            disabled={savingKey || !keyInput.trim()}
            className="rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)] disabled:opacity-40"
          >
            {savingKey ? "Saving…" : "Save"}
          </button>
        </div>
        <button
          onClick={() => void openUrl("https://home.openweathermap.org/api_keys")}
          className="text-[11px] text-[var(--color-accent)] underline"
        >
          Get a free key ↗
        </button>
      </div>
    );
  }

  // ── Error card ────────────────────────────────────────────────────────────
  if (status === "error" && !report) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
        <AlertTriangle size={26} className="text-amber-500" />
        <p className="max-w-xs text-[12px] leading-relaxed text-[var(--color-muted)]">{error}</p>
        <button
          onClick={() => void load(arg.trim())}
          className="flex items-center gap-1.5 rounded-md border border-[var(--color-border)] px-3 py-1.5 text-[12px] text-[var(--color-fg)]"
        >
          <RefreshCw size={13} /> Retry
        </button>
      </div>
    );
  }

  // ── Loading (first fetch) ─────────────────────────────────────────────────
  if (!report) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-[var(--color-muted)]">
        <RefreshCw size={22} className="animate-spin" />
        <p className="text-[12px]">Fetching weather…</p>
      </div>
    );
  }

  const c = report.current;
  const forecast = report.daily.filter((d) => !isToday(d.date)).slice(0, 5);
  // OWM returns wind in m/s for metric, mph for imperial. Only metric needs the
  // km/h conversion; imperial is already mph.
  const wind =
    report.units === "imperial"
      ? `${Math.round(c.wind_speed)} mph`
      : `${mpsToKmh(c.wind_speed)} km/h`;
  const windDir = windCompass(c.wind_deg);

  return (
    <div ref={scrollRef} className="flex h-full flex-col overflow-auto">
      {/* Hero — animated scene with the current conditions overlaid */}
      <div className="relative min-h-[220px] shrink-0 overflow-hidden">
        <WeatherScene kind={c.kind} />
        <div className="relative z-10 flex h-full flex-col justify-between p-4 text-white">
          <div className="flex items-start justify-between">
            <div className="flex items-center gap-1.5 text-[13px] font-medium drop-shadow">
              <MapPin size={14} />
              <span className="truncate">{report.location || "Your location"}</span>
            </div>
            <button
              onClick={() => {
                void load(arg.trim());
                onInteract?.();
              }}
              title="Refresh (R)"
              className="rounded-full bg-black/20 p-1.5 text-white/90 transition hover:bg-black/35"
            >
              <RefreshCw size={13} className={status === "loading" ? "animate-spin" : ""} />
            </button>
          </div>

          <div className="mt-3">
            <div className="flex items-end gap-2">
              <span
                className="font-mono text-[56px] font-bold leading-none drop-shadow-md"
                style={{ fontVariantNumeric: "tabular-nums" }}
              >
                {roundTemp(c.temp)}°
              </span>
              <span className="mb-2 text-[15px] font-medium capitalize drop-shadow">
                {c.description || WEATHER_LABELS[c.kind]}
              </span>
            </div>
            <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[12px] text-white/90 drop-shadow">
              <span>Feels {roundTemp(c.feels_like)}°</span>
              <span>H {roundTemp(c.temp_max)}° · L {roundTemp(c.temp_min)}°</span>
            </div>
          </div>
        </div>
      </div>

      {/* Current stat chips */}
      <div className="grid shrink-0 grid-cols-3 gap-px border-b border-[var(--color-border)] bg-[var(--color-border)]">
        <Stat icon={<Droplets size={14} />} label="Humidity" value={`${c.humidity}%`} />
        <Stat
          icon={<Wind size={14} />}
          label="Wind"
          value={`${wind}${windDir ? " " + windDir : ""}`}
        />
        <Stat icon={<Gauge size={14} />} label="Pressure" value={`${c.pressure} hPa`} />
      </div>

      {/* Forecast */}
      {forecast.length > 0 && (
        <div className="p-3">
          <div className="mb-2 text-[11px] font-semibold uppercase tracking-[0.1em] text-[var(--color-muted)]">
            {forecast.length}-day forecast
          </div>
          <div className="grid grid-cols-5 gap-2">
            {forecast.map((d) => (
              <div
                key={d.date}
                className="overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]"
              >
                <div className="relative h-[52px]">
                  <WeatherScene kind={d.kind} compact />
                </div>
                <div className="px-1.5 py-1.5 text-center">
                  <div className="text-[11px] font-medium text-[var(--color-fg)]">{dayName(d.date)}</div>
                  <div className="mt-0.5 text-[11px] tabular-nums text-[var(--color-muted)]">
                    <span className="text-[var(--color-fg)]">{roundTemp(d.temp_max)}°</span>{" "}
                    {roundTemp(d.temp_min)}°
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Footer */}
      <div className="mt-auto flex items-center justify-between px-3 pb-2 pt-1 text-[10px] text-[var(--color-muted)]">
        <span>OpenWeather{arg.trim() ? "" : " · IP-located"}</span>
        {updatedAt && <span>Updated {new Date(updatedAt).toLocaleTimeString()}</span>}
      </div>
    </div>
  );
}

function Stat({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
  return (
    <div className="flex flex-col items-center gap-0.5 bg-[var(--color-bg)] px-2 py-2.5">
      <span className="text-[var(--color-accent)]">{icon}</span>
      <span className="text-[12px] font-medium tabular-nums text-[var(--color-fg)]">{value}</span>
      <span className="text-[9px] uppercase tracking-wide text-[var(--color-muted)]">{label}</span>
    </div>
  );
}
