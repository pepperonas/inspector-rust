import { useEffect, useRef, useState } from "react";
import { AlarmClock, Clock, Coffee, Dices, Moon, Sparkles, Timer, Volume2, VolumeX } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import {
  getStatusToast,
  hideStatusToast,
  type StatusToast as Payload,
} from "../lib/ipc";

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

  const on = payload.on;
  const persistent = isPersistent(payload.kind);
  // Volume level for the bar (NaN for the "+"/"−" no-read-back fallback).
  const level = payload.kind === "volume" ? parseInt(payload.title, 10) : NaN;
  const muted = payload.kind === "mute" && on;

  const Icon =
    payload.kind === "timer"
      ? Timer
      : payload.kind === "alarm"
        ? AlarmClock
        : payload.kind === "clean"
          ? Sparkles
          : payload.kind === "random"
            ? Dices
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
