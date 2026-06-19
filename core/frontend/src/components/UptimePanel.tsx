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
 * Live system-uptime readout in the right preview column — entered with the
 * `uptime` command. The hero is the uptime in **seconds with 6 decimals (down
 * to microseconds)** rendered as a continuous **odometer**: every digit is a
 * vertical 0–9 strip translated each animation frame to its *continuous* value,
 * so the fast (sub-second) digits scroll/blur nonstop while the slower ones
 * tick — the motion is always visible, exactly as asked.
 *
 * Driven entirely by a `requestAnimationFrame` loop writing `transform` to DOM
 * refs (no React re-render per frame). The base uptime is fetched once and
 * anchored to `performance.now()` so the value flows smoothly from there. Esc
 * leaves (`onExit`); read-only otherwise.
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
  const stripRefs = useRef<(HTMLSpanElement | null)[]>([]);
  const cellRefs = useRef<(HTMLSpanElement | null)[]>([]);
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
        // Boot wall-clock = now − uptime (computed here, in an async callback,
        // not during render — `Date.now()` is impure).
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

  // Digit powers (high → low) once the base width is known. One headroom digit
  // so a power-of-ten rollover mid-view can't overflow.
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

      // Position every odometer strip + dim leading-zero integer cells.
      const intLen = Math.max(1, Math.floor(Math.log10(Math.max(1, Math.floor(t)))) + 1);
      const leading = intDigits - intLen;
      for (let i = 0; i < powers.length; i++) {
        const strip = stripRefs.current[i];
        if (strip) {
          const cv = odometerValue(t, powers[i]);
          strip.style.transform = `translateY(${-cv}em)`;
        }
        const cell = cellRefs.current[i];
        if (cell) {
          // Integer cells before the first significant digit are leading zeros.
          const isLeadingZero = powers[i] >= 0 && i < leading;
          cell.style.opacity = isLeadingZero ? "0.18" : "1";
        }
      }

      // Human-readable breakdown + boot time (cheap textContent writes).
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
          // Force reflow so the animation restarts.
          void hero.offsetWidth;
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

  const DIGITS = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];

  return (
    <div className="flex h-full flex-col items-center justify-center gap-6 overflow-hidden p-4 text-[var(--color-fg)]">
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
              className="uptime-odo text-[clamp(22px,5.5vw,40px)]"
              aria-label="Live uptime in seconds"
            >
              {powers.map((p, i) => {
                const isFrac = p < 0;
                return (
                  <Fragment key={p}>
                    {p === -1 && <span className="uptime-dot">.</span>}
                    <span
                      className={"uptime-cell" + (isFrac ? " uptime-frac" : "")}
                      ref={(el) => {
                        cellRefs.current[i] = el;
                      }}
                    >
                      <span
                        className="uptime-strip"
                        ref={(el) => {
                          stripRefs.current[i] = el;
                        }}
                      >
                        {DIGITS.map((d, j) => (
                          <span key={j}>{d}</span>
                        ))}
                      </span>
                    </span>
                  </Fragment>
                );
              })}
              <span className="uptime-unit ml-1 text-[0.5em]">s</span>
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
