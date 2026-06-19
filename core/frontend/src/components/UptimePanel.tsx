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
 *
 * The **hero** is the uptime in a normal, readable clock format —
 * `Dd HH:MM:SS` — big and clean; each digit briefly "heats" (accent + glow)
 * when it ticks and cools back, so the seconds pulse pleasantly. Below it, the
 * **converted** representation — total seconds down to **microseconds** — is
 * shown small and dimmed but kept animated: its sub-second digits change every
 * frame and stay lit via the same heat treatment, a subtle constant shimmer.
 *
 * Driven by one `requestAnimationFrame` loop writing `textContent` + a CSS class
 * to DOM refs (no React re-render per frame; paint-only). The base uptime is
 * fetched once and anchored to `performance.now()` so it flows smoothly. Esc
 * leaves.
 */
const FRAC_DIGITS = 6; // microseconds in the subtle "total seconds" line

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
  const clockRefs = useRef<(HTMLSpanElement | null)[]>([]); // [Ht,Hu,Mt,Mu,St,Su]
  const daysRef = useRef<HTMLSpanElement>(null);
  const subRefs = useRef<(HTMLSpanElement | null)[]>([]); // total-seconds.µs digits
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

  // Digit powers for the subtle total-seconds line (high → low + 6 decimals).
  const powers = useMemo(() => {
    if (base === null) return [];
    const intDigits = Math.max(2, integerDigitCount(base) + 1);
    return odometerPowers(intDigits, FRAC_DIGITS);
  }, [base]);
  const intDigits = powers.length - FRAC_DIGITS;

  // Animation loop — pure DOM writes, no React state.
  useEffect(() => {
    if (base === null) return;
    let raf = 0;

    // Heat helper: set a digit cell's glyph and flare it on change.
    const setDigit = (el: HTMLSpanElement | null, ch: string) => {
      if (!el) return;
      if (el.textContent !== ch) {
        el.textContent = ch;
        el.classList.add("hot");
      } else {
        el.classList.remove("hot");
      }
    };

    const loop = () => {
      const t = base + (performance.now() - anchorPerf.current) / 1000;
      const { days, hours, minutes, seconds } = uptimeBreakdown(t);

      // ── Hero clock: Dd HH:MM:SS
      if (daysRef.current) {
        const d = days > 0 ? `${days}d` : "";
        if (daysRef.current.textContent !== d) daysRef.current.textContent = d;
      }
      const clock = [
        Math.floor(hours / 10),
        hours % 10,
        Math.floor(minutes / 10),
        minutes % 10,
        Math.floor(seconds / 10),
        seconds % 10,
      ];
      for (let i = 0; i < clock.length; i++) {
        setDigit(clockRefs.current[i], String(clock[i]));
      }

      // ── Subtle converted line: total seconds with µs (constant shimmer)
      const intLen = Math.max(1, Math.floor(Math.log10(Math.max(1, Math.floor(t)))) + 1);
      const leading = intDigits - intLen;
      for (let i = 0; i < powers.length; i++) {
        const el = subRefs.current[i];
        if (!el) continue;
        setDigit(el, String(Math.floor(odometerValue(t, powers[i]))));
        const isLeadingZero = powers[i] >= 0 && i < leading;
        el.style.opacity = isLeadingZero ? "0.25" : "";
      }

      // Pop the hero each whole-second tick.
      const wholeSec = Math.floor(t);
      if (wholeSec !== lastSec.current) {
        lastSec.current = wholeSec;
        const hero = heroRef.current;
        if (hero) {
          hero.classList.remove("uptime-tick");
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

  // Hero clock cells: 6 digits with two colons between the pairs.
  const clockCells = [0, 1, 2, 3, 4, 5];

  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 overflow-hidden p-4 text-[var(--color-fg)]">
      <div className="flex items-center gap-2 self-start text-[13px] font-medium">
        <Timer size={15} className="text-[var(--color-accent)]" /> Uptime
      </div>

      {error ? (
        <p className="text-[12px] text-[var(--color-muted)]">{error}</p>
      ) : base === null ? (
        <p className="text-[12px] text-[var(--color-muted)]">Reading uptime…</p>
      ) : (
        <div className="flex flex-1 flex-col items-center justify-center gap-3">
          {/* Hero — readable clock format */}
          <div ref={heroRef} className="uptime-hero">
            <div className="uptime-odo whitespace-nowrap text-[clamp(30px,7vw,52px)]">
              <span ref={daysRef} className="uptime-days" />
              {clockCells.map((i) => (
                <Fragment key={i}>
                  {(i === 2 || i === 4) && <span className="uptime-colon">:</span>}
                  <span
                    className="uptime-digit"
                    ref={(el) => {
                      clockRefs.current[i] = el;
                    }}
                  >
                    0
                  </span>
                </Fragment>
              ))}
            </div>
          </div>

          {/* Subtle converted line — total seconds with microseconds, animated */}
          <div className="uptime-odo uptime-sub whitespace-nowrap text-[clamp(13px,2.4vw,17px)]">
            {powers.map((p, i) => (
              <Fragment key={p}>
                {p === -1 && <span className="uptime-dot">.</span>}
                <span
                  className="uptime-digit"
                  ref={(el) => {
                    subRefs.current[i] = el;
                  }}
                >
                  0
                </span>
              </Fragment>
            ))}
            <span className="uptime-unit">s</span>
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
