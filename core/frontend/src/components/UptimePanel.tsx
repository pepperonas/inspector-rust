import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { Timer } from "lucide-react";
import { getUptimeSecs } from "../lib/ipc";
import {
  uptimeBreakdown,
  odometerValue,
  integerDigitCount,
  odometerPowers,
} from "../lib/uptime";

/**
 * Live system-uptime readout in the right preview column (`uptime` command).
 * The hero is the uptime in **seconds with 6 decimals (down to microseconds)**,
 * rendered as clean, tabular monospace digits that update every animation frame.
 * Each digit gets a "heat" treatment: when its value changes it flares to the
 * accent colour with a glow and cools back over ~0.6 s — so the fast (sub-second)
 * digits stay permanently lit and shimmering while the slower ones pulse as they
 * tick. The motion is always visible, but every digit shows exactly one clean
 * glyph (no half-digit smearing).
 *
 * Driven by one `requestAnimationFrame` loop writing `textContent` + a CSS class
 * to DOM refs (no React re-render per frame). The base uptime is fetched once
 * and anchored to `performance.now()` so the value flows smoothly. Esc leaves.
 */
const FRAC_DIGITS = 6; // microseconds

export function UptimePanel({
  focused,
  onExit,
}: {
  focused: boolean;
  onExit: () => void;
}) {
  const [base, setBase] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sinceText, setSinceText] = useState("");
  const anchorPerf = useRef(0);
  const digitRefs = useRef<(HTMLSpanElement | null)[]>([]);
  const humanRef = useRef<HTMLSpanElement>(null);
  const sinceRef = useRef<HTMLSpanElement>(null);
  const heroRef = useRef<HTMLDivElement>(null);
  const lastSec = useRef(-1);

  // Fetch the base uptime once, anchor it to the high-res clock.
  useEffect(() => {
    let cancelled = false;
    getUptimeSecs()
      .then((s) => {
        if (cancelled) return;
        anchorPerf.current = performance.now();
        // Boot wall-clock = now − uptime (in this async callback, not render).
        setSinceText(new Date(Date.now() - s * 1000).toLocaleString());
        setBase(s);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Digit powers (high → low). One headroom integer digit so a power-of-ten
  // rollover mid-view can't overflow.
  const powers = useMemo(() => {
    if (base === null) return [];
    const intDigits = Math.max(2, integerDigitCount(base) + 1);
    return odometerPowers(intDigits, FRAC_DIGITS);
  }, [base]);
  const intDigits = powers.length - FRAC_DIGITS;

  // The animation loop — pure DOM writes, no React state.
  useEffect(() => {
    if (base === null || powers.length === 0) return;
    let raf = 0;
    const loop = () => {
      const t = base + (performance.now() - anchorPerf.current) / 1000;

      // Leading-zero count (integer part), to dim them.
      const intLen = Math.max(1, Math.floor(Math.log10(Math.max(1, Math.floor(t)))) + 1);
      const leading = intDigits - intLen;

      for (let i = 0; i < powers.length; i++) {
        const el = digitRefs.current[i];
        if (!el) continue;
        const ch = String(Math.floor(odometerValue(t, powers[i])));
        if (el.textContent !== ch) {
          el.textContent = ch;
          el.classList.add("hot"); // flare on change; CSS cools it back
        } else {
          el.classList.remove("hot");
        }
        const isLeadingZero = powers[i] >= 0 && i < leading;
        el.style.opacity = isLeadingZero ? "0.16" : "";
      }

      // Human-readable breakdown (cheap textContent write).
      const { days, hours, minutes, seconds } = uptimeBreakdown(t);
      const pad = (n: number) => String(n).padStart(2, "0");
      if (humanRef.current) {
        humanRef.current.textContent =
          (days > 0 ? `${days}d ` : "") + `${pad(hours)}:${pad(minutes)}:${pad(seconds)}`;
      }

      // Pop the hero each time the whole-seconds value ticks.
      const wholeSec = Math.floor(t);
      if (wholeSec !== lastSec.current) {
        lastSec.current = wholeSec;
        const hero = heroRef.current;
        if (hero) {
          hero.classList.remove("uptime-tick");
          void hero.offsetWidth; // restart the animation
          hero.classList.add("uptime-tick");
        }
      }

      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [base, powers, intDigits]);

  useEffect(() => {
    if (sinceRef.current) sinceRef.current.textContent = sinceText;
  }, [sinceText]);

  // Esc leaves; read-only otherwise.
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit]);

  return (
    <div className="flex h-full flex-col items-center justify-center gap-5 overflow-hidden p-4 text-[var(--color-fg)]">
      <div className="flex items-center gap-2 self-start text-[13px] font-medium">
        <Timer size={15} className="text-[var(--color-accent)]" /> Uptime
      </div>

      {error ? (
        <p className="text-[12px] text-[var(--color-muted)]">{error}</p>
      ) : base === null ? (
        <p className="text-[12px] text-[var(--color-muted)]">Reading uptime…</p>
      ) : (
        <div className="flex flex-1 flex-col items-center justify-center gap-3">
          <div ref={heroRef} className="uptime-hero">
            <div
              className="uptime-odo whitespace-nowrap text-[clamp(20px,4.4vw,32px)]"
              aria-label="Live uptime in seconds"
            >
              {powers.map((p, i) => (
                <Fragment key={p}>
                  {p === -1 && <span className="uptime-dot">.</span>}
                  <span
                    className="uptime-digit"
                    ref={(el) => {
                      digitRefs.current[i] = el;
                    }}
                  >
                    0
                  </span>
                </Fragment>
              ))}
              <span className="uptime-unit">s</span>
            </div>
          </div>

          <div className="text-center text-[15px] font-medium tabular-nums text-[var(--color-muted)]">
            <span ref={humanRef} />
          </div>
          <div className="text-[11px] text-[var(--color-muted)]">
            since <span ref={sinceRef} />
          </div>
        </div>
      )}

      {focused && !error && base !== null && (
        <p className="self-center text-[11px] text-[var(--color-muted)]">Esc close</p>
      )}
    </div>
  );
}
