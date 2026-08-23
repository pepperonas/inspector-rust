import { useEffect, useState } from "react";
import { Moon } from "lucide-react";
import { IS_MAC } from "../lib/platform";
import { formatSleepCountdown, formatHolders } from "../lib/sleep-status";
import type { SleepStatus } from "../lib/ipc";

interface Props {
  index: number;
  total: number;
  /** App version, e.g. "0.2.6". Rendered as `v0.2.6` next to the counter
   *  when provided. Optional so unit tests don't need a Tauri context. */
  version?: string;
  /** Wakelock state — when `true`, a tiny red LED dot pulses next to
   *  the shortcut hints as a visual confirmation that the cursor
   *  jiggler is running. Optional so unit tests + cold popup mounts
   *  don't need to know the state. */
  wakelockActive?: boolean;
  /** Number of in-flight timers (v0.39.0+). When > 0, a small `⏰ N`
   *  badge surfaces in the footer to remind the user a timer is
   *  ticking. */
  activeTimerCount?: number;
  /** Timesheet tracking state — when active, a pulsing dot + REC badge
   *  surfaces (amber while idle-paused, green while recording). */
  trackingActive?: boolean;
  trackingPaused?: boolean;
  /** System sleep status (macOS, v0.114.0) — is something OTHER than us
   *  holding the machine awake, and for how long? Distinct from
   *  `wakelockActive`, which is Inspector's OWN wakelock: with IR's wakelock
   *  on, this indicator consequently shows ∞ — the two coexist on purpose
   *  (one is "what I asked for", the other "what the system is doing"). */
  sleepStatus?: SleepStatus | null;
  /** Dark wake (v0.116.0): system awake while the DISPLAY may sleep — the
   *  remote-reachability mode. `onDarkWakeToggle` makes the ☾ clickable;
   *  without the handler the button isn't rendered (unit tests / cold
   *  mounts). Coexists with `wakelockActive` (the FULL wakelock's red LED)
   *  on purpose — they are two modes of one backend, never shown both. */
  darkWake?: boolean;
  onDarkWakeToggle?: () => void;
}

export function Footer({
  index,
  total,
  version,
  wakelockActive,
  activeTimerCount,
  trackingActive,
  trackingPaused,
  sleepStatus,
  darkWake,
  onDarkWakeToggle,
}: Props) {
  const label = total === 0 ? "0/0" : `${index + 1}/${total}`;
  // OCR + Screenshot are the most-hidden global shortcuts — they fire
  // from anywhere on the system without needing the popup open.
  // Surfaced in the footer so users discover them without having to dig
  // into the tray menu or Settings → Keyboard shortcuts.
  const ocrKey = IS_MAC ? "⌃⇧O" : "Ctrl+⇧+O";
  const screenshotKey = IS_MAC ? "⌃⇧S" : "Ctrl+⇧+S";
  const colorKey = IS_MAC ? "⌃⇧C" : "Ctrl+⇧+C";
  return (
    // `min-h-8` (not fixed `h-8`) + `flex-wrap` so a cramped footer — e.g.
    // Windows, where `Ctrl+⇧+O` hints are wider than the macOS glyphs —
    // wraps onto a second line instead of clipping. Nothing is ever cut
    // off; the row just grows a little taller when it has to. The credit
    // (♥ Martin Pfeffer) moved to the inline About to keep this lean.
    <div className="flex min-h-8 flex-wrap items-center justify-between gap-x-3 gap-y-1 border-t border-[var(--color-border)] px-4 py-1 text-[11px] text-[var(--color-muted)]">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        {wakelockActive && <WakelockLed />}
        {onDarkWakeToggle && <DarkWakeButton on={!!darkWake} onToggle={onDarkWakeToggle} />}
        {sleepStatus && <SleepStatusLed status={sleepStatus} />}
        {trackingActive && <TrackingLed paused={!!trackingPaused} />}
        {activeTimerCount != null && activeTimerCount > 0 && (
          <TimerBadge count={activeTimerCount} />
        )}
        <Hint k="⏎" label="Paste" />
        <Hint k="↑↓" label="Navigate" />
        <Hint k="Esc" label="Close" />
        <Hint k={ocrKey} label="OCR" />
        <Hint k={screenshotKey} label="Shot" />
        <Hint k={colorKey} label="Color" />
      </div>
      <div className="flex shrink-0 items-center gap-3">
        {version && (
          <span title="Inspector Rust version" className="font-[var(--font-mono)]">
            v{version}
          </span>
        )}
        <span>{label}</span>
      </div>
    </div>
  );
}

function Hint({ k, label }: { k: string; label: string }) {
  return (
    <span className="flex items-center gap-1">
      <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px]">
        {k}
      </kbd>
      <span>{label}</span>
    </span>
  );
}

/**
 * Tiny red LED dot indicating the wakelock is on. Pulses slowly
 * (1.6 s cycle) via the shared `wakelockPulse` keyframe in
 * `styles.css` so the user's eye notices it without it being
 * distracting. The dot has a soft red box-shadow that mimics a real
 * LED bleed-glow.
 */
/**
 * Footer badge showing the count of in-flight `timer` commands.
 * Single timer → `⏰ 1`; multiple → `⏰ 3` etc. Tooltip nudges the
 * user toward `timer 0` (planned cancel UX) — currently the only way
 * to cancel is to wait or restart the app.
 */
function TimerBadge({ count }: { count: number }) {
  return (
    <span
      title={`${count} timer${count === 1 ? "" : "s"} running — will fire a macOS notification + Glass sound`}
      className="flex shrink-0 items-center gap-1 font-[var(--font-mono)] text-[10px] uppercase tracking-wider text-[var(--color-accent)]"
    >
      ⏰ {count}
    </span>
  );
}

/** Timesheet tracking indicator — green pulsing dot + REC while recording,
 *  amber + PAUSED while idle-auto-paused. */
function TrackingLed({ paused }: { paused: boolean }) {
  const color = paused ? "245, 158, 11" : "34, 197, 94"; // amber / green
  return (
    <span
      title={
        paused
          ? "Time tracking paused (idle). Type `track off` to stop, or `track` to open the timesheet."
          : "Time tracking active. Type `track off` to stop, or `track` to open the timesheet."
      }
      className="flex shrink-0 items-center gap-1"
    >
      <span
        aria-hidden
        className="h-2 w-2 rounded-full"
        style={{
          backgroundColor: `rgb(${color})`,
          boxShadow: `0 0 4px rgba(${color}, 0.85), 0 0 8px rgba(${color}, 0.45)`,
          animation: paused ? undefined : "wakelockPulse 1.6s ease-in-out infinite",
        }}
      />
      <span className="font-[var(--font-mono)] text-[10px] uppercase tracking-wider">
        {paused ? "paused" : "rec"}
      </span>
    </span>
  );
}

function WakelockLed() {
  return (
    <span
      title="Keep-awake active — the computer won't sleep or lock. Type `wakelock off` (or `caffeine off`) to turn it off."
      className="flex shrink-0 items-center gap-1"
    >
      <span
        aria-hidden
        className="h-2 w-2 rounded-full bg-red-500"
        style={{
          boxShadow: "0 0 4px rgba(239, 68, 68, 0.85), 0 0 8px rgba(239, 68, 68, 0.45)",
          animation: "wakelockPulse 1.6s ease-in-out infinite",
        }}
      />
      <span className="font-[var(--font-mono)] text-[10px] uppercase tracking-wider">
        wake
      </span>
    </span>
  );
}

/**
 * System-sleep indicator (macOS, v0.114.0): a glance answers "can I walk away
 * and let the Mac sleep?". Three states; hidden entirely when there is nothing
 * to say (unsupported platform, or nothing preventing sleep) — the footer must
 * not accumulate idle chrome.
 *
 *  · `sleep_disabled`  → amber dot, `no-sleep` — the ACTIVE pmset profile has
 *    `sleep 0`, so idle sleep never happens; the countdown would be a lie and
 *    is deliberately not shown even when assertions are also active.
 *  · prevented, timed  → `wach 4:12` — the countdown ticks down LOCALLY every
 *    second between the ~10 s polls, parks at 0:00 (never negative; the next
 *    poll corrects), tooltip names the holders.
 *  · prevented, ∞      → `wach ∞` — some holder has no timeout.
 */
function SleepStatusLed({ status }: { status: SleepStatus }) {
  const ticking =
    status.supported &&
    !status.sleep_disabled &&
    status.prevented &&
    !status.indefinite &&
    status.max_timeout_secs != null;
  // Locally ticking remainder, anchored to the moment THIS status object
  // arrived (re-anchors per poll). Lives in an effect — render stays pure
  // (no Date.now() during render; react-compiler lint enforces this).
  const [remaining, setRemaining] = useState<number | null>(null);
  useEffect(() => {
    if (!ticking) {
      setRemaining(null);
      return;
    }
    const base = status.max_timeout_secs ?? 0;
    const anchoredAt = Date.now();
    setRemaining(base);
    const id = window.setInterval(
      () => setRemaining(base - Math.floor((Date.now() - anchoredAt) / 1000)),
      1000,
    );
    return () => window.clearInterval(id);
  }, [status, ticking]);

  if (!status.supported) return null;

  if (status.sleep_disabled) {
    return (
      <span
        title="System-Sleep in pmset deaktiviert (sleep 0) — der Mac schläft nie von selbst."
        className="flex shrink-0 items-center gap-1"
      >
        <span
          aria-hidden
          className="h-2 w-2 rounded-full"
          style={{
            backgroundColor: "rgb(245, 158, 11)",
            boxShadow: "0 0 4px rgba(245, 158, 11, 0.85), 0 0 8px rgba(245, 158, 11, 0.45)",
          }}
        />
        <span className="font-[var(--font-mono)] text-[10px] uppercase tracking-wider">
          no-sleep
        </span>
      </span>
    );
  }

  if (!status.prevented) return null;

  const holderNote = status.holders.length
    ? ` Wachhalter: ${formatHolders(status.holders)}.`
    : "";
  const label = status.indefinite
    ? "wach ∞"
    : `wach ${formatSleepCountdown(remaining ?? status.max_timeout_secs ?? 0)}`;
  const title = status.indefinite
    ? `Der Mac wird ohne Zeitlimit wachgehalten (Assertion ohne Timeout).${holderNote}`
    : `Der Mac wird wachgehalten — so lange noch, bis Sleep wieder möglich ist.${holderNote}`;
  return (
    <span title={title} className="flex shrink-0 items-center gap-1">
      <span
        aria-hidden
        className="h-2 w-2 rounded-full"
        style={{
          backgroundColor: "rgb(56, 189, 248)", // sky — distinct from red wake / green rec
          boxShadow: "0 0 4px rgba(56, 189, 248, 0.85), 0 0 8px rgba(56, 189, 248, 0.45)",
        }}
      />
      <span className="font-[var(--font-mono)] text-[10px] tracking-wider">{label}</span>
    </span>
  );
}

/**
 * Dark-wake toggle (v0.116.0) — the one CLICKABLE element among the footer
 * LEDs: ☾ keeps the SYSTEM awake while the display may sleep (`caffeinate
 * -is`), so remote connections (SSH / Claude Code) stay reachable with the
 * screen dark. Always rendered (a toggle must be findable), muted while off,
 * violet + "srv" label + glow while on. Clicking from the FULL wakelock
 * switches to dark (the user explicitly wants the screen off); clicking while
 * dark turns everything off. ⚠️ Does not survive a lid close (OS-forced
 * clamshell sleep — no assertion prevents that).
 */
function DarkWakeButton({ on, onToggle }: { on: boolean; onToggle: () => void }) {
  return (
    <button
      type="button"
      onClick={onToggle}
      title={
        on
          ? "Dark Wake aktiv: System bleibt wach, Display darf schlafen — Remote-Verbindungen (SSH/Claude Code) bleiben erreichbar. Klick schaltet aus. Deckel muss offen bleiben."
          : "Dark Wake einschalten: Display darf schlafen, System bleibt wach — Remote-Verbindungen bleiben erreichbar (caffeinate -is, ohne sudo)."
      }
      className={
        "flex shrink-0 cursor-pointer items-center gap-1 rounded px-0.5 transition-colors " +
        (on ? "text-violet-400" : "text-[var(--color-muted)] opacity-60 hover:opacity-100")
      }
    >
      <Moon
        size={11}
        aria-hidden
        style={
          on
            ? {
                filter:
                  "drop-shadow(0 0 3px rgba(167, 139, 250, 0.9)) drop-shadow(0 0 7px rgba(167, 139, 250, 0.5))",
                animation: "wakelockPulse 1.6s ease-in-out infinite",
              }
            : undefined
        }
      />
      {on && (
        <span className="font-[var(--font-mono)] text-[10px] uppercase tracking-wider">
          srv
        </span>
      )}
    </button>
  );
}
