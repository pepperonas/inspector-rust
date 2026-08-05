import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { irisStatus } from "../lib/ipc";
import {
  makeBlobs,
  type IrisBurst,
  makeBurst,
  burstGapMs,
  burstIntensity,
  BURST_MAX,
} from "../lib/iris";

/**
 * The `iris` screen effect — one of these renders per monitor in its own
 * click-through, always-on-top window.
 *
 * Two layers, and the split matters:
 *
 * 1. **A muted edge field** — eight blobs from `makeBlobs`, jittered off the
 *    corners and edge midpoints, drifting on mutually incommensurate periods so
 *    the motion has no visible loop. This is only the constant "you are over"
 *    signal; it deliberately sits back.
 * 2. **The bursts** — discrete impulses that carry the character. This is the
 *    part the first port missed entirely: the raspi5 dB-analysis page's punch
 *    does **not** come from its background wash (that layer is static there —
 *    its `iris-breathe` keyframes are never defined anywhere in that repo). It
 *    comes from `spawnIrisWord()`, which fires an element on a randomised gap
 *    that snaps in hard, holds, then fades while growing. `iris-burst-pop`
 *    keeps that shape but deliberately diverges from the verbatim
 *    `0/18/62/100` envelope (v0.102.3): the attack strobes, the hold
 *    shimmers, and the fade begins earlier and drops steeply first — see the
 *    keyframes comment. The reference's `cubic-bezier(0.2,0,0,1)` stays.
 *
 * Departures from the reference, all deliberate:
 *
 * * **No lettering.** The reference bursts are the word "IRIS" in a variable
 *   display face; here each impulse is a rotated, multi-lobed light flare —
 *   1–3 overlapping gradients, so the outline is irregular but every edge is
 *   a falloff (v0.102.3; the v0.102.2 clip-path shards were too harsh).
 * * **Level-reactive cadence.** The reference fires blind at 1.7–4.8 s for as
 *   long as the flag is set. Here the gap interpolates toward 0.42–1.2 s and
 *   the impulses grow brighter, bigger and shorter the further over the
 *   threshold the room is — so a loud room reads as urgent, not merely busy.
 *   The window still never settles into a fixed beat at any volume
 *   (`burstGapMs` is unit-tested for that).
 * * The palette is the reference's `IRIS_TINTS`, warm-graded red → salmon →
 *   amber → near-white, which is what keeps it from reading as one flat red.
 *
 * No text is ever drawn, and BOTH layers keep the middle of the screen clear:
 * `makeBlobs` for the field and `edgePosition` for the impulses (each
 * unit-tested — quadratically rim-biased, never deeper than 24 % from an
 * edge). That, plus a never-opaque peak (≤ 0.95) and blink-short lives, is what
 * keeps the machine usable while iris is lit.
 */

/** Trough/peak of the strip's breathe — `BRI_TROUGH / BRI_PEAK` in hue_warn.py. */
const BREATHE_TROUGH = 90 / 254;
/** The reference's `transition:background .55s` release. */
const RELEASE_S = 0.55;
/** `--ease` in stats.html — MD3 emphasized-decelerate. Used by both the
 *  arm/disarm choreography and the burst pop, exactly as in the reference. */
const EASE = "cubic-bezier(0.2,0,0,1)";
/** Overshoot for the ignite, so the light arrives with weight. */
const SPRING = "cubic-bezier(.16,1.24,.3,1)";
/** Belt-and-braces removal margin over a burst's own lifetime — mirrors the
 *  reference's `setTimeout(..., (life + 0.6) * 1000)`. If `animationend` never
 *  arrives (a throttled window is enough) the node still goes away instead of
 *  occupying the concurrency cap forever. */
const BURST_REAP_MARGIN_MS = 600;

const CSS = `
.iris-root{
  position:fixed; inset:0; pointer-events:none; overflow:hidden;
  isolation:isolate;                 /* so screen-blend mixes layers, not the desktop */
  contain:layout paint;
}

/* ── the muted constant field ─────────────────────────────────────────── */
.iris-blob{
  position:absolute;
  left:var(--x); top:var(--y);
  width:var(--w); height:var(--h);
  margin-left:calc(var(--w) / -2);
  margin-top:calc(var(--h) / -2);
  mix-blend-mode:screen;
  will-change:transform, opacity;
  opacity:0;
  transform:translate(var(--ox), var(--oy)) scale(1.16);
  transition:
    opacity ${RELEASE_S}s ${EASE},
    transform calc(${RELEASE_S}s * 1.35) ${EASE};
  transition-delay:calc((7 - var(--i)) * 42ms);
}
.iris-root.on .iris-blob{
  opacity:1;
  transform:translate(0,0) scale(1);
  transition:
    opacity .34s ${EASE},
    transform .62s ${SPRING};
  transition-delay:calc(var(--i) * 46ms);
}
.iris-blob-i{
  position:absolute; inset:0;
  background:radial-gradient(circle at 50% 50%,
    var(--c0)  0%,
    var(--c1) 30%,
    var(--c2) 52%,
    transparent 68%);
  filter:blur(var(--blur));
  animation:iris-drift var(--dur) ease-in-out infinite alternate;
  animation-delay:var(--del);
}
@keyframes iris-drift{
  from{ transform:translate3d(0,0,0) scale(1); opacity:${BREATHE_TROUGH} }
  to  { transform:translate3d(var(--dx), var(--dy), 0) scale(var(--ds)); opacity:1 }
}

/* ── the impulses ─────────────────────────────────────────────────────── */
.iris-burst{
  position:absolute;
  left:var(--x); top:var(--y);
  width:var(--size); height:calc(var(--size) * var(--aspect));
  border-radius:50%;
  mix-blend-mode:screen;
  will-change:transform, opacity;
  opacity:0;
  /* The silhouette is 1-3 overlapping radial gradients (inline background,
     built per burst) — irregular, but every edge is a falloff. The blur only
     feathers what the gradients already soften. */
  filter:blur(var(--bblur));
  animation:iris-burst-pop var(--life) ${EASE} forwards;
}
/* Livelier than the reference's plain 18/62/100 envelope, on purpose
   (hardened again in v0.102.4 — "aggressiver, aufblitzender"):
   - the attack strobes HARD (first hit at 3 %, dips to 8/15 % of peak — real
     near-dark flashes, not a soft stutter),
   - the hold SHIMMERS instead of sitting still (36/46 % dip and recover —
     the same trick as the reference's GRAD font-axis pulse: apparent
     intensity moves while the geometry barely does),
   - the fade starts earlier (~56 %) and drops steeply first (72 % is already
     down to ~14 %), then trails out while growing — a progressive out, so the
     screen clears fast but the impulse doesn't just pop off.
   The impulse also turns slightly over its life (--rotd): a still flare reads
   frozen, a turning one reads alive. */
@keyframes iris-burst-pop{
  0%   { opacity:0;                        transform:translate(-50%,-50%) rotate(var(--rot)) scale(.5) }
  3%   { opacity:var(--peak);              transform:translate(-50%,-50%) rotate(var(--rot)) scale(1.14) }
  6%   { opacity:calc(var(--peak) * .08);  transform:translate(-50%,-50%) rotate(var(--rot)) scale(1.06) }
  10%  { opacity:var(--peak);              transform:translate(-50%,-50%) rotate(calc(var(--rot) + var(--rotd) * .1)) scale(1.1) }
  14%  { opacity:calc(var(--peak) * .15);  transform:translate(-50%,-50%) rotate(calc(var(--rot) + var(--rotd) * .18)) scale(1.05) }
  19%  { opacity:var(--peak);              transform:translate(-50%,-50%) rotate(calc(var(--rot) + var(--rotd) * .26)) scale(1.06) }
  30%  { opacity:calc(var(--peak) * .75);  transform:translate(-50%,-50%) rotate(calc(var(--rot) + var(--rotd) * .4)) scale(1.02) }
  40%  { opacity:var(--peak);              transform:translate(-50%,-50%) rotate(calc(var(--rot) + var(--rotd) * .5)) scale(1) }
  50%  { opacity:calc(var(--peak) * .4);   transform:translate(-50%,-50%) rotate(calc(var(--rot) + var(--rotd) * .64)) scale(1.04) }
  68%  { opacity:calc(var(--peak) * .1);   transform:translate(-50%,-50%) rotate(calc(var(--rot) + var(--rotd) * .84)) scale(1.12) }
  100% { opacity:0;                        transform:translate(-50%,-50%) rotate(calc(var(--rot) + var(--rotd))) scale(1.24) }
}

/* One short beat on ignite, so arming itself lands. */
.iris-flash{
  position:absolute; inset:0;
  background:radial-gradient(ellipse 120% 120% at 50% 50%,
    rgba(255,120,90,0) 34%,
    rgba(255,90,64,.20) 72%,
    rgba(255,70,55,.42) 100%);
  animation:iris-flash .38s ${EASE} 1 both;
  mix-blend-mode:screen;
}
@keyframes iris-flash{
  0%  { opacity:0; transform:scale(1.06) }
  20% { opacity:1; transform:scale(1) }
  100%{ opacity:0; transform:scale(1) }
}

@media (prefers-reduced-motion: reduce){
  .iris-blob{ transition:opacity .2s linear; transform:translate(0,0) scale(1) }
  .iris-root.on .iris-blob{ transition:opacity .2s linear; transform:translate(0,0) scale(1) }
  .iris-blob-i{ animation:none; opacity:.8 }
  .iris-flash{ display:none }
  /* The reference's own reduced-motion fallback is opacity-only — same here. */
  .iris-burst{ animation:iris-burst-fade var(--life) ${EASE} forwards }
  @keyframes iris-burst-fade{
    0%{opacity:0} 20%{opacity:var(--peak)} 70%{opacity:var(--peak)} 100%{opacity:0}
  }
}
`;

/** Muted relative to v0.102.0: the field is now the backdrop, not the show. */
const PALETTE = {
  hot: {
    c0: "rgba(255,168,96,.58)",
    c1: "rgba(255,72,48,.34)",
    c2: "rgba(176,26,22,.14)",
  },
  deep: {
    c0: "rgba(255,86,64,.54)",
    c1: "rgba(206,30,26,.32)",
    c2: "rgba(122,18,18,.12)",
  },
} as const;

/** The soft, multi-lobed body of one impulse: each lobe is a white-hot core
 *  falling through the tint into transparency — the requested Verlauf. Lobes
 *  overlap into an irregular outline with no hard edge anywhere. */
function burstBackground(b: IrisBurst): string {
  return b.lobes
    .map(
      (l) =>
        `radial-gradient(circle at ${l.cx}% ${l.cy}%, rgba(255,255,255,.96) 0%, ${b.tint} ${Math.round(l.r * 0.24)}%, color-mix(in srgb, ${b.tint} 38%, transparent) ${Math.round(l.r * 0.58)}%, transparent ${Math.round(l.r * 0.94)}%)`,
    )
    .join(", ");
}

export function IrisOverlay() {
  const [over, setOver] = useState(false);
  const [rise, setRise] = useState(0);
  const [bursts, setBursts] = useState<IrisBurst[]>([]);
  const overRef = useRef(false);
  /** Live 0..1 loudness above the threshold, read by the scheduler without
   *  re-subscribing it on every level event (those arrive ~10×/s). */
  const intensityRef = useRef(0);
  const timerRef = useRef<number | null>(null);
  const seqRef = useRef(0);
  const aliveRef = useRef(true);

  const blobs = useMemo(() => makeBlobs(Math.random), []);

  useEffect(() => {
    const el = document.createElement("style");
    el.textContent = CSS;
    document.head.appendChild(el);
    // No background reset needed: `styles.css` already declares
    // `html, body, #root { background: transparent }`.
    const prevOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      el.remove();
      document.body.style.overflow = prevOverflow;
    };
  }, []);

  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  // Backend state. `iris-over` is edge-triggered (Rust owns the decision);
  // `iris-level` is the ~10 Hz stream that drives the cadence.
  useEffect(() => {
    let alive = true;
    const apply = (next: boolean) => {
      if (!alive) return;
      if (next && !overRef.current) setRise((r) => r + 1);
      overRef.current = next;
      setOver(next);
    };
    const unOver = listen<{ over: boolean }>("iris-over", (e) =>
      apply(!!e.payload?.over),
    );
    const unLevel = listen<{ spl: number; over: boolean; threshold: number }>(
      "iris-level",
      (e) => {
        const p = e.payload;
        if (!p) return;
        intensityRef.current = burstIntensity(p.spl, p.threshold);
      },
    );
    irisStatus()
      .then((s) => {
        if (s?.over) apply(true);
      })
      .catch(() => {});
    return () => {
      alive = false;
      unOver.then((f) => f()).catch(() => {});
      unLevel.then((f) => f()).catch(() => {});
    };
  }, []);

  // The burst chain. Self-scheduling with a fresh random gap each time, so it
  // never settles into a beat — the reference is explicit that a fixed cadence
  // "reads mechanical".
  useEffect(() => {
    if (!over) {
      // Stop scheduling but let whatever is in flight finish its own animation
      // (same intent as the reference's delayed disarm).
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      return;
    }

    const drop = (id: number) => {
      if (!aliveRef.current) return;
      setBursts((prev) => prev.filter((b) => b.id !== id));
    };

    const spawn = () => {
      const intensity = intensityRef.current;
      setBursts((prev) => {
        // Cap on the live array length, never on a separate counter: a counter
        // drifts the moment one `animationend` fails to arrive and then blocks
        // spawning forever, silently. (Learned the hard way in the reference.)
        if (prev.length >= BURST_MAX) return prev;
        const b = makeBurst(++seqRef.current, intensity, Math.random);
        window.setTimeout(() => drop(b.id), b.life * 1000 + BURST_REAP_MARGIN_MS);
        return [...prev, b];
      });
      schedule();
    };

    const schedule = () => {
      timerRef.current = window.setTimeout(
        spawn,
        burstGapMs(intensityRef.current, Math.random),
      );
    };

    // The reference deliberately does not spawn on arm, because a one-tick poll
    // dropout re-armed it constantly. Our `over` is edge-triggered in Rust with
    // hysteresis and a minimum hold, so it cannot flap — and waiting up to 4.8 s
    // for the first impulse would make arming feel dead. Fire promptly, then
    // fall into the random cadence.
    timerRef.current = window.setTimeout(spawn, 80);
    return () => {
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [over]);

  return (
    <div className={`iris-root${over ? " on" : ""}`} aria-hidden="true">
      {blobs.map((b, i) => {
        const p = b.hot ? PALETTE.hot : PALETTE.deep;
        return (
          <div
            key={i}
            className="iris-blob"
            style={
              {
                "--x": `${b.x}%`,
                "--y": `${b.y}%`,
                "--w": `${b.w}vmax`,
                "--h": `${b.h}vmax`,
                "--ox": `${b.ox}%`,
                "--oy": `${b.oy}%`,
                "--i": i,
              } as React.CSSProperties
            }
          >
            <div
              className="iris-blob-i"
              style={
                {
                  "--dx": `${b.dx}%`,
                  "--dy": `${b.dy}%`,
                  "--ds": b.ds,
                  "--dur": `${b.dur}s`,
                  "--del": `${b.del}s`,
                  "--blur": `${b.blur}px`,
                  "--c0": p.c0,
                  "--c1": p.c1,
                  "--c2": p.c2,
                } as React.CSSProperties
              }
            />
          </div>
        );
      })}

      {bursts.map((b) => (
        <div
          key={b.id}
          className="iris-burst"
          onAnimationEnd={() =>
            aliveRef.current && setBursts((prev) => prev.filter((x) => x.id !== b.id))
          }
          style={
            {
              "--x": `${b.x}%`,
              "--y": `${b.y}%`,
              "--size": `${b.size}vmax`,
              "--aspect": b.aspect,
              "--life": `${b.life}s`,
              "--peak": b.peak,
              "--rot": `${b.rot}deg`,
              "--rotd": `${b.rotDrift}deg`,
              "--bblur": `${b.blur}px`,
              background: burstBackground(b),
            } as React.CSSProperties
          }
        />
      ))}

      {over && <div key={rise} className="iris-flash" />}
    </div>
  );
}
