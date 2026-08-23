import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Clock, Plus, X, Sun, Moon, Search } from "lucide-react";
import { getClockZones, setClockZones } from "../lib/ipc";
import {
  DEFAULT_ZONES,
  matchCities,
  normalizeZones,
  zoneByTz,
  tzFallbackCity,
  zoneTime,
  type CityZone,
  type ZoneTime,
} from "../lib/clock";

/**
 * `clock` — a world clock in the preview column (v0.121.0). A grid of live
 * time cards, one per saved IANA timezone; add cities via autocomplete,
 * remove with ×. Times come from the platform `Intl` API (correct DST), a
 * single 1 s tick re-renders the grid (cheap — a handful of cards). The zone
 * list persists server-side (settings `clock.zones`) so it survives popup
 * reopens. Enter-activated so the search field can take focus for autocomplete.
 */
export function ClockPanel({ focused, onExit }: { focused: boolean; onExit: () => void }) {
  const [zones, setZones] = useState<string[] | null>(null);
  const [now, setNow] = useState(() => new Date());
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0); // highlighted autocomplete row
  const inputRef = useRef<HTMLInputElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  // Load persisted zones (or defaults) on mount.
  useEffect(() => {
    getClockZones()
      .then((raw) => {
        const parsed = raw ? safeParse(raw) : null;
        const list = parsed ? normalizeZones(parsed) : [...DEFAULT_ZONES];
        setZones(list.length ? list : [...DEFAULT_ZONES]);
      })
      .catch(() => setZones([...DEFAULT_ZONES]));
  }, []);

  // 1 s tick.
  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 1000);
    return () => window.clearInterval(id);
  }, []);

  const persist = useCallback((list: string[]) => {
    setZones(list);
    void setClockZones(JSON.stringify(list)).catch(() => undefined);
  }, []);

  const add = useCallback(
    (tz: string) => {
      if (!zones || zones.includes(tz)) return;
      persist([...zones, tz]);
      setQuery("");
      setSel(0);
    },
    [zones, persist],
  );
  const remove = useCallback(
    (tz: string) => {
      if (!zones) return;
      persist(zones.filter((z) => z !== tz));
    },
    [zones, persist],
  );

  const matches = useMemo(
    () => (zones ? matchCities(query, zones) : []),
    [query, zones],
  );

  // Esc: clear the query first, else exit. Only when NOT navigating matches.
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        if (query) {
          setQuery("");
          setSel(0);
        } else {
          onExit();
        }
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit, query]);

  if (zones === null) {
    return (
      <Shell focused={focused}>
        <p className="text-[12px] text-[var(--color-muted)]">Lade Uhren…</p>
      </Shell>
    );
  }

  return (
    <div
      ref={scrollRef}
      className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]"
    >
      <div className="flex items-center gap-2 text-[13px] font-medium">
        <Clock size={15} className="text-[var(--color-accent)]" /> Weltzeit
      </div>

      {/* Add-city autocomplete. */}
      <div className="relative">
        <div className="flex items-center gap-2 rounded-lg border border-[var(--color-border)] px-2 py-1.5">
          <Search size={13} className="shrink-0 text-[var(--color-muted)]" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSel(0);
            }}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setSel((s) => Math.min(s + 1, matches.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setSel((s) => Math.max(s - 1, 0));
              } else if (e.key === "Enter" && matches[sel]) {
                e.preventDefault();
                add(matches[sel].tz);
              }
            }}
            placeholder="Stadt hinzufügen… (z. B. Tokio, Dubai)"
            className="w-full bg-transparent text-[12px] outline-none placeholder:text-[var(--color-muted)]"
          />
        </div>
        {matches.length > 0 && (
          <div className="absolute z-10 mt-1 w-full overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] shadow-lg">
            {matches.map((m, i) => (
              <button
                key={m.tz}
                type="button"
                onMouseEnter={() => setSel(i)}
                onClick={() => add(m.tz)}
                className={
                  "flex w-full items-center justify-between gap-2 px-2.5 py-1.5 text-left text-[12px] " +
                  (i === sel ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]" : "hover:bg-[var(--color-border)]")
                }
              >
                <span className="flex items-center gap-1.5">
                  <Plus size={11} className="opacity-70" />
                  {m.city}
                </span>
                <span className={i === sel ? "opacity-80" : "text-[var(--color-muted)]"}>{m.region}</span>
              </button>
            ))}
          </div>
        )}
      </div>

      {zones.length === 0 ? (
        <p className="rounded-lg border border-[var(--color-border)] p-3 text-center text-[12px] text-[var(--color-muted)]">
          Keine Uhren — oben eine Stadt hinzufügen.
        </p>
      ) : (
        <div className="grid grid-cols-2 gap-2">
          {zones.map((tz) => (
            <ClockCard key={tz} tz={tz} now={now} onRemove={() => remove(tz)} />
          ))}
        </div>
      )}

      {focused && (
        <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">
          Tippen zum Suchen · ↑↓ + ⏎ hinzufügen · Esc schließen
        </p>
      )}
    </div>
  );
}

function Shell({ focused, children }: { focused: boolean; children: React.ReactNode }) {
  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]">
      <div className="flex items-center gap-2 text-[13px] font-medium">
        <Clock size={15} className="text-[var(--color-accent)]" /> Weltzeit
      </div>
      {children}
      {focused && <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">Esc schließen</p>}
    </div>
  );
}

function ClockCard({ tz, now, onRemove }: { tz: string; now: Date; onRemove: () => void }) {
  const meta: CityZone | undefined = zoneByTz(tz);
  const city = meta?.city ?? tzFallbackCity(tz);
  const region = meta?.region ?? tz;
  const t: ZoneTime = zoneTime(now, tz);
  return (
    <div
      className="group relative overflow-hidden rounded-xl border border-[var(--color-border)] p-3 [contain:content]"
      style={{
        // Subtle day/night wash — warm by day, cool indigo by night.
        background: t.night
          ? "linear-gradient(135deg, color-mix(in srgb, #6366f1 12%, transparent), transparent 70%)"
          : "linear-gradient(135deg, color-mix(in srgb, #f59e0b 12%, transparent), transparent 70%)",
      }}
    >
      <div className="mb-1 flex items-center justify-between gap-1">
        <span className="flex min-w-0 items-center gap-1 text-[11px] font-medium">
          {t.night ? (
            <Moon size={11} className="shrink-0 text-indigo-400" />
          ) : (
            <Sun size={11} className="shrink-0 text-amber-500" />
          )}
          <span className="truncate" title={`${city} · ${tz}`}>
            {city}
          </span>
        </span>
        <button
          type="button"
          onClick={onRemove}
          title="Entfernen"
          className="shrink-0 rounded p-0.5 text-[var(--color-muted)] opacity-0 transition-opacity hover:text-red-500 group-hover:opacity-100"
        >
          <X size={12} />
        </button>
      </div>
      <div className="flex items-baseline gap-0.5">
        <span className="font-[var(--font-mono)] text-[26px] font-semibold leading-none tabular-nums">
          {t.time}
        </span>
        <span className="font-[var(--font-mono)] text-[12px] text-[var(--color-muted)] tabular-nums">
          {t.seconds}
        </span>
      </div>
      <div className="mt-1 flex items-center justify-between text-[10px] text-[var(--color-muted)]">
        <span className="truncate" title={region}>
          {t.date}
        </span>
        <span className="flex shrink-0 items-center gap-1 tabular-nums">
          {t.dayDelta !== 0 && (
            <span
              className={
                "rounded px-1 " +
                (t.dayDelta > 0 ? "bg-emerald-500/15 text-emerald-500" : "bg-amber-500/15 text-amber-500")
              }
            >
              {t.dayDelta > 0 ? "+1 Tag" : "−1 Tag"}
            </span>
          )}
          {t.offset}
        </span>
      </div>
    </div>
  );
}

function safeParse(raw: string): unknown {
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}
