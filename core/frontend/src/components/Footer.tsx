import { IS_MAC } from "../lib/platform";

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
}

export function Footer({ index, total, version, wakelockActive, activeTimerCount }: Props) {
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
