import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  AlarmClock,
  AudioLines,
  Clock,
  Coffee,
  Dices,
  Moon,
  Music,
  Sparkles,
  Timer,
  Volume,
  Volume1,
  Volume2,
  VolumeX, Siren } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import {
  getStatusToast,
  hideStatusToast,
  type StatusToast as Payload,
} from "../lib/ipc";
import { digitColumns, rollDirection, waveIntensity } from "../lib/volume-hud";

/**
 * `status-toast` window — a brief, click-through, centred-on-screen
 * confirmation flourish shown after the popup hides (wakelock, timer, …) and,
 * passively, by touchpad gestures (volume / mute).
 *
 * Two behaviours:
 * - **One-shot** kinds (wakelock/timer/random/clean/track): pop in-and-out via
 *   `statusToastPop`, hidden after the hold — unchanged.
 * - **Persistent** kinds (`volume`/`mute`): pop IN and stay, then on each rapid
 *   re-trigger **update in place** (value bump, timer reset) rather than
 *   re-popping; fade out only once the triggers stop. This is what makes
 *   "3× louder in a row" feel like one continuous readout.
 */

const HOLD_MS = 1600;
const HOLD_MS_RANDOM = 3600;
/** Persistent toasts linger this long after the LAST re-trigger, then fade. */
const HOLD_MS_PERSISTENT = 1100;
/** Must match `statusToastOut` in styles.css. */
const OUT_MS = 260;

const isPersistent = (kind: string | undefined) => kind === "volume" || kind === "mute";

export function StatusToast() {
  const [payload, setPayload] = useState<Payload | null>(null);
  const [animKey, setAnimKey] = useState(0); // bumps only for a *fresh* entrance
  const [tick, setTick] = useState(0); // bumps on every (re)trigger → timer + value bump
  const [exiting, setExiting] = useState(false);
  const payloadRef = useRef<Payload | null>(null);
  const visibleRef = useRef(false);

  useEffect(() => {
    let alive = true;
    const refresh = () => {
      getStatusToast()
        .then((p) => {
          if (!alive || !p) return;
          // A continuation = the same persistent kind, still on screen → update
          // in place (no entrance replay). Anything else is a fresh entrance.
          const cont =
            visibleRef.current && isPersistent(p.kind) && payloadRef.current?.kind === p.kind;
          payloadRef.current = p;
          setPayload(p);
          setExiting(false);
          if (!cont) setAnimKey((k) => k + 1);
          setTick((t) => t + 1);
          visibleRef.current = true;
        })
        .catch(() => {});
    };
    refresh();
    const un = listen("status-toast-changed", refresh);
    return () => {
      alive = false;
      void un.then((f) => f());
    };
  }, []);

  // Auto-dismiss. Each (re)trigger (tick) resets the timer. Persistent toasts
  // play a fade-out first; one-shots hide directly (their pop already faded).
  useEffect(() => {
    if (tick === 0) return;
    const persistent = isPersistent(payloadRef.current?.kind);
    const hold = persistent
      ? HOLD_MS_PERSISTENT
      : payloadRef.current?.kind === "random"
        ? HOLD_MS_RANDOM
        : HOLD_MS;
    const t = window.setTimeout(() => {
      if (persistent) {
        setExiting(true); // triggers the fade-out effect below
      } else {
        visibleRef.current = false;
        void hideStatusToast();
      }
    }, hold);
    return () => window.clearTimeout(t);
  }, [tick]);

  // Persistent fade-out → hide the window once the out animation has played.
  useEffect(() => {
    if (!exiting) return;
    const t = window.setTimeout(() => {
      visibleRef.current = false;
      void hideStatusToast();
    }, OUT_MS);
    return () => window.clearTimeout(t);
  }, [exiting]);

  if (!payload) return <div className="h-screen w-screen bg-transparent" />;

  // Volume / mute get a dedicated, premium macOS-HUD-style overlay (frosted
  // glass, spring entrance, a velocity-preserving rAF spring bar, level-aware
  // speaker icons). Other kinds keep the original flourish below.
  if (payload.kind === "volume" || payload.kind === "mute") {
    return <VolumeOverlay payload={payload} tick={tick} animKey={animKey} exiting={exiting} />;
  }

  const on = payload.on;
  const persistent = isPersistent(payload.kind);
  // Volume level for the bar (NaN for the "+"/"−" no-read-back fallback).
  const level = payload.kind === "volume" ? parseInt(payload.title, 10) : NaN;
  const muted = payload.kind === "mute" && on;

  const Icon =
    payload.kind === "iris"
      ? Siren
      : payload.kind === "boom"
        ? AudioLines
      : payload.kind === "timer"
        ? Timer
      : payload.kind === "alarm"
        ? AlarmClock
        : payload.kind === "clean"
          ? Sparkles
          : payload.kind === "random"
            ? Dices
            : payload.kind === "shazam"
              ? Music
              : payload.kind === "track"
                ? Clock
              : payload.kind === "volume"
                ? (level === 0 ? VolumeX : Volume2)
                : payload.kind === "mute"
                  ? (muted ? VolumeX : Volume2)
                  : on
                    ? Coffee
                    : Moon;
  const isRandom = payload.kind === "random";
  const accent =
    (payload.kind === "wakelock" && !on) || muted
      ? "var(--color-muted)"
      : "var(--color-accent)";

  const cardClass = persistent
    ? exiting
      ? "status-toast-out"
      : "status-toast-in"
    : "status-toast-pop";

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent select-none">
      <div
        key={animKey}
        className={
          cardClass + " flex flex-col items-center gap-3 rounded-2xl border px-9 py-7"
        }
        style={{
          borderColor: accent,
          backgroundColor: "var(--color-surface)",
          boxShadow: "0 16px 50px rgba(0,0,0,0.5)",
          minWidth: persistent ? "13rem" : undefined,
        }}
      >
        <div className="relative flex h-14 w-14 items-center justify-center">
          {!persistent && (
            <span
              className="status-toast-ring absolute inset-0 rounded-full"
              style={{ border: `2px solid ${accent}` }}
            />
          )}
          {/* Icon bumps on each re-trigger for persistent toasts. */}
          <Icon
            key={persistent ? tick : undefined}
            className={persistent ? "status-toast-bump" : "status-toast-icon"}
            size={52}
            strokeWidth={1.75}
            style={{ color: accent }}
          />
        </div>
        <div className="w-full text-center">
          <div
            key={persistent ? tick : undefined}
            className={
              "font-[var(--font-mono)] font-black uppercase " +
              (persistent ? "status-toast-bump " : "") +
              (isRandom ? "text-[52px] leading-none tracking-tight" : "text-[22px] tracking-[0.12em]")
            }
            style={{ color: accent, textShadow: `0 0 26px ${accent}` }}
          >
            {payload.title}
          </div>
          {/* Volume level bar (only when the OS gave a numeric read-back). */}
          {payload.kind === "volume" && !Number.isNaN(level) && (
            <div
              className="mt-2 h-2 w-full overflow-hidden rounded-full"
              style={{ backgroundColor: "var(--color-border)" }}
            >
              <div
                className="h-full rounded-full"
                style={{
                  width: `${Math.max(0, Math.min(100, level))}%`,
                  backgroundColor: accent,
                  transition: "width 140ms cubic-bezier(0.22,1,0.36,1)",
                }}
              />
            </div>
          )}
          {!persistent && (
            <div className="mt-1 text-[12px] text-[var(--color-muted)]">{payload.subtitle}</div>
          )}
        </div>
      </div>
    </div>
  );
}

// ── Volume / mute overlay (premium HUD) ──────────────────────────────────────

const clamp01 = (n: number) => (n < 0 ? 0 : n > 1 ? 1 : n);
// Under-damped spring — a little overshoot for life, settles quickly.
const SPRING = { stiffness: 190, damping: 22, mass: 1 };

/** Level-aware speaker icon: muted/0 → X, low → no/one wave, high → two waves. */
function volumeIcon(level: number, muted: boolean, hasLevel: boolean): { Icon: LucideIcon; key: string } {
  if (muted) return { Icon: VolumeX, key: "x" };
  if (!hasLevel) return { Icon: Volume2, key: "2" };
  if (level <= 0) return { Icon: VolumeX, key: "x" };
  if (level < 15) return { Icon: Volume, key: "0" };
  if (level < 55) return { Icon: Volume1, key: "1" };
  return { Icon: Volume2, key: "2" };
}

function VolumeOverlay({
  payload,
  tick,
  animKey,
  exiting,
}: {
  payload: Payload;
  tick: number;
  animKey: number;
  exiting: boolean;
}) {
  const muted = payload.kind === "mute" && payload.on;
  const parsed = parseInt(payload.title, 10);
  const hasLevel = payload.kind === "volume" && !Number.isNaN(parsed);
  const level = hasLevel ? Math.max(0, Math.min(100, parsed)) : 0;
  const { Icon, key: iconKey } = volumeIcon(level, muted, hasLevel);
  const accent = muted ? "var(--color-muted)" : "var(--color-accent)";

  // "Klangaura" (v0.118.0): the direction of THIS trigger drives the sonar
  // waves (up → outward bloom, down → inward collapse), the digit roll and
  // the streak. Derived per tick against the previous reading via the
  // official render-phase derived-state pattern (setState of the SAME
  // component during render — refs during render would trip
  // react-hooks/refs). A repeat at a boundary (holding ⇧↓ at 0) yields
  // "none" and fires nothing.
  const [aura, setAura] = useState<{
    tick: number;
    prevLevel: number | null;
    prevMuted: boolean | null;
    dir: ReturnType<typeof rollDirection>;
    muteFlip: "dip" | "rebound" | null;
  }>({ tick: -1, prevLevel: null, prevMuted: null, dir: "none", muteFlip: null });
  if (aura.tick !== tick) {
    setAura({
      tick,
      prevLevel: hasLevel ? level : aura.prevLevel,
      prevMuted: muted,
      dir: hasLevel ? rollDirection(aura.prevLevel, level) : "none",
      muteFlip:
        aura.prevMuted == null || aura.prevMuted === muted
          ? null
          : muted
            ? "dip"
            : "rebound",
    });
  }
  const dir = aura.dir;
  const muteFlip = aura.muteFlip;

  // rAF spring for the bar fill (scaleX), velocity-preserving across rapid
  // re-triggers. Driven imperatively (DOM) so there's no React re-render/frame.
  const fillRef = useRef<HTMLDivElement>(null);
  const headRef = useRef<HTMLDivElement>(null);
  const initialFill = hasLevel ? level / 100 : 0.0001;
  const xRef = useRef(initialFill);
  const vRef = useRef(0);
  const targetRef = useRef(initialFill);
  const rafRef = useRef(0);
  const lastRef = useRef(0);
  const reduce =
    typeof window !== "undefined" &&
    window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

  // Set the initial bar width before paint (no flash to full). The comet head
  // rides the fill's leading edge — positioned from the SAME spring value
  // (left %, one writer, no CSS transition fighting the rAF).
  const applyBar = (x: number) => {
    const v = clamp01(x);
    if (fillRef.current) fillRef.current.style.transform = `scaleX(${v})`;
    if (headRef.current) headRef.current.style.left = `${v * 100}%`;
  };
  useLayoutEffect(() => {
    applyBar(xRef.current);
  }, []);

  useEffect(() => {
    // Mute with no read-back leaves the bar where it is (just dims via CSS).
    if (hasLevel) targetRef.current = clamp01(level / 100);
    if (reduce) {
      xRef.current = targetRef.current;
      vRef.current = 0;
      applyBar(targetRef.current);
      return;
    }
    if (!rafRef.current) {
      lastRef.current = 0;
      rafRef.current = requestAnimationFrame(step);
    }
    function step(now: number) {
      if (!lastRef.current) lastRef.current = now;
      let dt = (now - lastRef.current) / 1000;
      lastRef.current = now;
      if (dt > 0.05) dt = 0.05; // clamp after a tab/Space switch
      const { stiffness, damping, mass } = SPRING;
      const a = (-stiffness * (xRef.current - targetRef.current) - damping * vRef.current) / mass;
      vRef.current += a * dt;
      xRef.current += vRef.current * dt;
      applyBar(xRef.current);
      if (Math.abs(xRef.current - targetRef.current) < 0.0005 && Math.abs(vRef.current) < 0.0005) {
        xRef.current = targetRef.current;
        vRef.current = 0;
        applyBar(targetRef.current);
        rafRef.current = 0;
        return;
      }
      rafRef.current = requestAnimationFrame(step);
    }
    // tick is a dependency so each re-trigger re-aims the spring.
    void tick;
  }, [tick, level, muted, hasLevel, reduce]);

  useEffect(() => () => { if (rafRef.current) cancelAnimationFrame(rafRef.current); }, []);

  const showBar = hasLevel || muted;

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent select-none">
      <div key={animKey} className={exiting ? "vol-overlay-out" : "vol-overlay-in"}>
        <div className="vol-card">
          <span className="relative flex" style={{ color: accent }}>
            {/* Sonar waves — replayed per trigger; direction-aware; intensity
                is the level (loud radiates, quiet whispers). Mute collapse /
                unmute bloom reuse the in/out shapes. None while reduced. */}
            {!reduce && (dir !== "none" || muteFlip) && (
              <span
                key={`w-${tick}`}
                aria-hidden
                className="pointer-events-none absolute inset-0"
                style={{ "--wave-o": String(waveIntensity(hasLevel ? level : 60)) } as React.CSSProperties}
              >
                <span className={`vol-wave ${dir === "down" || muteFlip === "dip" ? "vol-wave-down" : "vol-wave-up"}`} />
                <span className={`vol-wave ${dir === "down" || muteFlip === "dip" ? "vol-wave-down" : "vol-wave-up"}`} />
              </span>
            )}
            <span key={iconKey} className="vol-icon">
              <Icon size={30} strokeWidth={2} />
            </span>
          </span>
          <div className="vol-body">
            {hasLevel ? (
              <div className="vol-readout" style={{ color: muted ? "var(--color-muted)" : "var(--color-fg)" }}>
                <span className="vol-num">
                  {digitColumns(level).map((d, i) => (
                    <span
                      // Key includes the digit → only CHANGED columns replay
                      // their roll; unchanged digits sit still (odometer).
                      key={`${i}-${d}`}
                      className={
                        "vol-digit" +
                        (reduce || dir === "none"
                          ? ""
                          : dir === "up"
                            ? " vol-digit-up"
                            : " vol-digit-down")
                      }
                    >
                      {d}
                    </span>
                  ))}
                </span>
                <span className="vol-pct">%</span>
              </div>
            ) : (
              <div className="vol-label" style={{ color: muted ? "var(--color-muted)" : "var(--color-fg)" }}>
                {payload.title}
              </div>
            )}
            {showBar && (
              <div
                key={muteFlip ? `m-${tick}` : undefined}
                className={
                  "vol-track relative" +
                  (reduce || !muteFlip ? "" : muteFlip === "dip" ? " vol-dip" : " vol-rebound")
                }
              >
                <div
                  ref={fillRef}
                  className="vol-fill"
                  style={{
                    backgroundColor: accent,
                    opacity: muted ? 0.5 : 1,
                    transition: "background-color 220ms ease, opacity 220ms ease",
                  }}
                />
                {/* Comet head on the fill's leading edge (spring-driven). */}
                <div
                  ref={headRef}
                  aria-hidden
                  className="vol-head"
                  style={{ color: accent, opacity: muted || !hasLevel ? 0 : 1 }}
                />
                {/* One light streak along the bar on each louder-trigger. */}
                {!reduce && dir === "up" && !muted && (
                  <div key={`s-${tick}`} aria-hidden className="vol-streak" />
                )}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
