import { useEffect, useRef, useState } from "react";
import { Lightbulb, RefreshCw, Sun } from "lucide-react";
import { getAmbientLight, type AmbientLightReading } from "../lib/ipc";
import { formatLux, lightBand, luxLevel } from "../lib/lumen";

const POLL_MS = 350;

export function LumenPanel() {
  const [reading, setReading] = useState<AmbientLightReading | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [failed, setFailed] = useState(false);
  const busy = useRef(false);

  useEffect(() => {
    let cancelled = false;
    const poll = async () => {
      if (busy.current) return;
      busy.current = true;
      try {
        const next = await getAmbientLight();
        if (!cancelled) {
          setReading(next);
          setLoaded(true);
          setFailed(false);
        }
      } catch {
        if (!cancelled) {
          setLoaded(true);
          setFailed(true);
        }
      } finally {
        busy.current = false;
      }
    };
    void poll();
    const id = window.setInterval(() => void poll(), POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, []);

  if (!loaded) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-[var(--color-muted)]">
        <RefreshCw size={22} className="animate-spin" />
        <span className="text-xs">Reading ambient light…</span>
      </div>
    );
  }

  if (!reading || failed) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 px-8 text-center">
        <div className="grid h-12 w-12 place-items-center rounded-full bg-[var(--color-border)]/40">
          <Lightbulb size={22} className="text-[var(--color-muted)]" />
        </div>
        <div>
          <p className="text-sm font-medium">No light sensor available</p>
          <p className="mt-1 max-w-64 text-xs leading-relaxed text-[var(--color-muted)]">
            This device does not expose a supported ambient-light reading.
          </p>
        </div>
      </div>
    );
  }

  const band = lightBand(reading.lux);
  const level = luxLevel(reading.lux);
  return (
    <div className="flex h-full flex-col items-center justify-center overflow-hidden px-7 py-6 text-center">
      <div
        className="relative mb-5 grid h-36 w-36 place-items-center rounded-full border border-white/10"
        style={{
          background: `radial-gradient(circle, ${band.color}35 0%, ${band.color}12 45%, transparent 72%)`,
          boxShadow: `0 0 ${24 + level * 0.45}px ${band.color}30`,
        }}
      >
        <div
          className="absolute inset-3 rounded-full border border-white/10 transition-transform duration-500"
          style={{ transform: `scale(${0.82 + level * 0.0018})` }}
        />
        <Sun
          size={48}
          className="lumen-sun relative"
          style={{ color: band.color, filter: `drop-shadow(0 0 12px ${band.color})` }}
        />
      </div>

      <div className="flex items-end justify-center gap-2 tabular-nums">
        <span className="text-5xl font-semibold tracking-tight">{formatLux(reading.lux)}</span>
        <span className="mb-1 text-lg text-[var(--color-muted)]">lx</span>
      </div>
      <div className="mt-2 text-sm font-medium" style={{ color: band.color }}>
        {band.label}
      </div>
      <div className="mt-1 text-xs text-[var(--color-muted)]">{band.description}</div>

      <div className="mt-6 w-full max-w-72">
        <div className="h-2 overflow-hidden rounded-full bg-[var(--color-border)]/60">
          <div
            className="h-full origin-left rounded-full transition-transform duration-300 ease-out"
            style={{
              transform: `scaleX(${level / 100})`,
              background: "linear-gradient(90deg, #6366f1, #8b5cf6, #f59e0b, #eab308)",
            }}
          />
        </div>
        <div className="mt-1.5 flex justify-between text-[9px] text-[var(--color-muted)]">
          <span>dark</span>
          <span>indoor</span>
          <span>daylight</span>
        </div>
      </div>

      <div className="mt-5 flex items-center gap-1.5 text-[10px] text-[var(--color-muted)]">
        <Lightbulb size={11} /> {reading.source} · live
      </div>
    </div>
  );
}
