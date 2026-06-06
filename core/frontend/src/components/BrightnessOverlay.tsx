import { useEffect, useRef, useState } from "react";
import { MonitorIcon, X } from "lucide-react";
import {
  brightnessClose,
  listBrightnessMonitors,
  setMonitorBrightness,
  type MonitorInfo,
} from "../lib/ipc";

/**
 * Brightness slider overlay (the `brightness-overlay` window — routed in
 * `main.tsx`). One slider per DDC-capable monitor (incl. secondary/external),
 * plus an "all" master. Lunar / TwinkleTray style. Opened by the `brightness`
 * (alias `bri`) command, which hides the popup and shows this window.
 *
 * DDC writes can be slow / rate-limited, so slider input is debounced: while
 * dragging we update the local value immediately but only push the *latest*
 * value to the monitor ~80 ms after the last change.
 */
export function BrightnessOverlay() {
  const [monitors, setMonitors] = useState<MonitorInfo[] | null>(null);
  const [values, setValues] = useState<Record<number, number>>({});
  const timers = useRef<Record<number, number>>({});

  useEffect(() => {
    let alive = true;
    listBrightnessMonitors()
      .then((m) => {
        if (!alive) return;
        setMonitors(m);
        setValues(Object.fromEntries(m.map((x) => [x.id, x.brightness])));
      })
      .catch(() => {
        if (alive) setMonitors([]);
      });
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void brightnessClose();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      alive = false;
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  // Debounced push to a single monitor.
  const push = (id: number, percent: number) => {
    if (timers.current[id]) window.clearTimeout(timers.current[id]);
    timers.current[id] = window.setTimeout(() => {
      void setMonitorBrightness(id, percent).catch(() => undefined);
    }, 80);
  };

  const onChange = (id: number, percent: number) => {
    setValues((v) => ({ ...v, [id]: percent }));
    push(id, percent);
  };

  const setAll = (percent: number) => {
    if (!monitors) return;
    const next: Record<number, number> = {};
    for (const m of monitors) {
      if (!m.supports_ddc) continue;
      next[m.id] = percent;
      push(m.id, percent);
    }
    setValues((v) => ({ ...v, ...next }));
  };

  const ddc = monitors?.filter((m) => m.supports_ddc) ?? [];

  return (
    <div className="flex h-screen w-screen flex-col gap-3 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 text-[var(--color-fg)] shadow-2xl">
      <div data-tauri-drag-region className="flex items-center justify-between">
        <span className="flex items-center gap-2 text-[13px] font-medium">
          <MonitorIcon size={15} className="text-[var(--color-accent)]" /> Brightness
        </span>
        <button
          onClick={() => void brightnessClose()}
          title="Close (Esc)"
          className="flex h-6 w-6 items-center justify-center rounded-full text-[var(--color-muted)] hover:bg-[var(--color-bg)] hover:text-[var(--color-fg)]"
        >
          <X size={14} />
        </button>
      </div>

      {monitors === null ? (
        <p className="text-[12px] text-[var(--color-muted)]">Probing monitors…</p>
      ) : ddc.length === 0 ? (
        <p className="text-[12px] text-[var(--color-muted)]">
          No DDC-capable monitors found. External monitors are controlled over
          DDC/CI — built-in laptop panels aren&apos;t supported yet.
        </p>
      ) : (
        <div className="flex flex-col gap-3 overflow-y-auto">
          {ddc.length > 1 && (
            <Slider
              label="All monitors"
              value={Math.round(
                ddc.reduce((s, m) => s + (values[m.id] ?? m.brightness), 0) / ddc.length,
              )}
              onChange={setAll}
              emphasis
            />
          )}
          {ddc.map((m) => (
            <Slider
              key={m.id}
              label={m.name}
              value={values[m.id] ?? m.brightness}
              onChange={(p) => onChange(m.id, p)}
            />
          ))}
          {(monitors.length - ddc.length) > 0 && (
            <p className="text-[11px] text-[var(--color-muted)]">
              {monitors.length - ddc.length} monitor(s) didn&apos;t answer DDC and
              are hidden.
            </p>
          )}
        </div>
      )}
    </div>
  );
}

function Slider({
  label,
  value,
  onChange,
  emphasis,
}: {
  label: string;
  value: number;
  onChange: (percent: number) => void;
  emphasis?: boolean;
}) {
  return (
    <label className="flex flex-col gap-1">
      <span
        className={
          "flex items-center justify-between text-[12px] " +
          (emphasis ? "font-medium" : "text-[var(--color-muted)]")
        }
      >
        <span className="truncate pr-2">{label}</span>
        <span className="tabular-nums">{value}%</span>
      </span>
      <input
        type="range"
        min={0}
        max={100}
        value={value}
        onChange={(e) => onChange(parseInt(e.target.value, 10))}
        className="w-full accent-[var(--color-accent)]"
      />
    </label>
  );
}
