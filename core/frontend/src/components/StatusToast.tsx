import { useEffect, useState } from "react";
import { AlarmClock, Coffee, Dices, Moon, Sparkles, Timer } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import {
  getStatusToast,
  hideStatusToast,
  type StatusToast as Payload,
} from "../lib/ipc";

/**
 * `status-toast` window — a brief, click-through, centred-on-screen
 * confirmation flourish (styled like the hidden-game intros) shown after
 * the popup hides. Currently driven by the wakelock on/off toggle.
 *
 * The window is reused between toasts (hidden, not closed), so the
 * payload is pulled via `getStatusToast` on mount AND re-pulled on every
 * `status-toast-changed` event; each pull bumps `animKey` to replay the
 * CSS animation, and (re)arms the auto-dismiss timer.
 */

/** Base hold (must cover the `statusToastPop` keyframe in styles.css). */
const HOLD_MS = 1600;
/** The random-number toast lingers longer so the result is easy to read. */
const HOLD_MS_RANDOM = 3600;

export function StatusToast() {
  const [payload, setPayload] = useState<Payload | null>(null);
  const [animKey, setAnimKey] = useState(0);

  useEffect(() => {
    let alive = true;
    const refresh = () => {
      getStatusToast()
        .then((p) => {
          if (!alive || !p) return;
          setPayload(p);
          setAnimKey((k) => k + 1);
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

  // Auto-dismiss once the animation has played. A fresh toast (animKey
  // bump) resets the timer so rapid toggles aren't cut short. The random
  // toast lingers longer so the rolled number is easy to read.
  useEffect(() => {
    if (animKey === 0) return;
    const hold = payload?.kind === "random" ? HOLD_MS_RANDOM : HOLD_MS;
    const t = window.setTimeout(() => void hideStatusToast(), hold);
    return () => window.clearTimeout(t);
  }, [animKey, payload?.kind]);

  if (!payload) return <div className="h-screen w-screen bg-transparent" />;

  const on = payload.on;
  // Icon by kind: wakelock toggles Coffee/Moon; timer/alarm use clocks;
  // clean uses sparkles.
  const Icon =
    payload.kind === "timer"
      ? Timer
      : payload.kind === "alarm"
        ? AlarmClock
        : payload.kind === "clean"
          ? Sparkles
          : payload.kind === "random"
            ? Dices
            : on
              ? Coffee
              : Moon;
  // The random toast shows the rolled number — make it big and prominent.
  const isRandom = payload.kind === "random";
  // Accent for everything except an explicit "off" state (muted).
  const accent =
    payload.kind === "wakelock" && !on
      ? "var(--color-muted)"
      : "var(--color-accent)";

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent select-none">
      <div
        key={animKey}
        className="status-toast-pop flex flex-col items-center gap-3 rounded-2xl border px-9 py-7"
        style={{
          borderColor: accent,
          backgroundColor: "var(--color-surface)",
          boxShadow: "0 16px 50px rgba(0,0,0,0.5)",
        }}
      >
        <div className="relative flex h-14 w-14 items-center justify-center">
          <span
            className="status-toast-ring absolute inset-0 rounded-full"
            style={{ border: `2px solid ${accent}` }}
          />
          <Icon
            className="status-toast-icon"
            size={52}
            strokeWidth={1.75}
            style={{ color: accent }}
          />
        </div>
        <div className="text-center">
          <div
            className={
              "font-[var(--font-mono)] font-black uppercase " +
              (isRandom ? "text-[52px] leading-none tracking-tight" : "text-[22px] tracking-[0.12em]")
            }
            style={{ color: accent, textShadow: `0 0 26px ${accent}` }}
          >
            {payload.title}
          </div>
          <div className="mt-1 text-[12px] text-[var(--color-muted)]">
            {payload.subtitle}
          </div>
        </div>
      </div>
    </div>
  );
}
