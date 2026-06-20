import { useEffect, useState } from "react";
import { AlarmClock } from "lucide-react";
import { alarmOverlayLabel, stopAlarm } from "../lib/ipc";

/**
 * The loud, dismiss-to-stop **alarm overlay** (the window labelled
 * `alarm-overlay`, the default when a timer/countdown fires). Rust raises the
 * system volume and loops the alarm sound; this overlay must be clicked (or
 * Esc/Enter/Space pressed) to silence it. A pulsing bell + ringing rings make
 * it impossible to miss.
 */
export function AlarmOverlay() {
  const [label, setLabel] = useState<string>("");

  useEffect(() => {
    alarmOverlayLabel()
      .then((l) => setLabel(l ?? ""))
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    const dismiss = () => void stopAlarm().catch(() => undefined);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" || e.key === "Enter" || e.key === " " || e.code === "Space") {
        e.preventDefault();
        e.stopPropagation();
        dismiss();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  const dismiss = () => void stopAlarm().catch(() => undefined);

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent p-1">
      {/* The whole card is clickable to dismiss — "drücken um den Alarm zu beenden". */}
      <button
        type="button"
        onClick={dismiss}
        className="alarm-card md3-pop-in flex h-full w-full cursor-pointer flex-col items-center justify-center gap-5 rounded-2xl border border-[var(--color-accent)]/40 bg-[var(--color-bg)] p-6 text-center"
      >
        <div className="relative flex h-24 w-24 items-center justify-center">
          <span className="alarm-ring" />
          <span className="alarm-ring alarm-ring-2" />
          <AlarmClock size={52} className="alarm-bell text-[var(--color-accent)]" />
        </div>

        <div>
          <div className="text-[20px] font-bold text-[var(--color-fg)]">
            {label || "Timer"}
          </div>
          <div className="mt-1 text-[13px] text-[var(--color-muted)]">Time's up</div>
        </div>

        <span className="md3-press rounded-full bg-[var(--color-accent)] px-7 py-2.5 text-[15px] font-semibold text-[var(--color-accent-fg)] shadow-lg">
          Stop alarm
        </span>
        <span className="text-[11px] text-[var(--color-muted)]">
          click anywhere · Esc / Enter / Space
        </span>
      </button>
    </div>
  );
}
