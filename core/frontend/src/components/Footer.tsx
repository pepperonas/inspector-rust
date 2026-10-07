import { useEffect, useState } from "react";
import { Folder, HardDrive, Moon } from "lucide-react";
import { IS_MAC } from "../lib/platform";
import { indicatorTicks, sleepIndicator, type SleepKind } from "../lib/sleep-indicator";
import type { FinderContext, FreeSpace, SleepStatus } from "../lib/ipc";
import { contextTitle, selectionLabel, shortDir } from "../lib/finder-context";
import { formatBytesDecimal, freeSpaceLevel, freeSpaceTitle } from "../lib/free-space";

interface Props {
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
  /** Free space on the home disk (v0.195.0) — always shown when known. */
  freeSpace?: FreeSpace | null;
  /** Working folder + selection of the front Finder window (v0.202.0) —
   *  what touch/echo/mkdir/terminal/daisy act on. Click reveals it. */
  finderContext?: FinderContext | null;
  onRevealFolder?: (dir: string) => void;
}

export function Footer({
  version,
  wakelockActive,
  activeTimerCount,
  trackingActive,
  trackingPaused,
  sleepStatus,
  darkWake,
  onDarkWakeToggle,
  freeSpace,
  finderContext,
  onRevealFolder,
}: Props) {
  // OCR + Screenshot are the most-hidden global shortcuts — they fire
  // from anywhere on the system without needing the popup open.
  // Surfaced in the footer so users discover them without having to dig
  // into the tray menu or Settings → Keyboard shortcuts.
  const ocrKey = IS_MAC ? "⌃⇧O" : "Ctrl+⇧+O";
  const screenshotKey = IS_MAC ? "⌃⇧S" : "Ctrl+⇧+S";
  const colorKey = IS_MAC ? "⌃⇧C" : "Ctrl+⇧+C";
  return (
    // Two fixed rows (v0.203.0). Top: status + global shortcuts, free space
    // on the right. Bottom: the Finder working folder on the left, the
    // version on the right. The ⏎ Paste / Esc Close hints and the 1/N
    // counter are gone — Enter, Esc and the arrows work as before.
    <div className="flex flex-col gap-y-0.5 border-t border-[var(--color-border)] px-4 py-1 text-[11px] text-[var(--color-muted)]">
      <div data-testid="footer-row-top" className="flex min-h-6 flex-wrap items-center justify-between gap-x-3 gap-y-1">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
          {/* ⚠️ The toggle comes FIRST. Before v0.152.0 the always-visible badge
              sat after it, so with the wakelock off this was the footer's
              leftmost item; merging the indicators moved it behind a labelled,
              coloured badge and it stopped being findable (field report). */}
          {onDarkWakeToggle && <DarkWakeButton on={!!darkWake} onToggle={onDarkWakeToggle} />}
          <SleepLed status={sleepStatus ?? null} wakelockActive={!!wakelockActive} />
          {trackingActive && <TrackingLed paused={!!trackingPaused} />}
          {activeTimerCount != null && activeTimerCount > 0 && (
            <TimerBadge count={activeTimerCount} />
          )}
          <Hint k={ocrKey} label="OCR" />
          <Hint k={screenshotKey} label="Shot" />
          <Hint k={colorKey} label="Color" />
        </div>
        {freeSpace && <FreeSpaceBadge space={freeSpace} />}
      </div>
      <div data-testid="footer-row-bottom" className="flex min-h-5 items-center justify-between gap-x-3">
        {/* flex-1: the path may use all the room up to the version; it only
            truncates when the row is actually too narrow. */}
        <div className="flex min-w-0 flex-1">
          {finderContext && <FinderChip ctx={finderContext} onReveal={onRevealFolder} />}
        </div>
        {/* Version sits at the FAR right of the bottom row — the fixed anchor. */}
        {version && (
          <span title="Inspector Rust version" className="shrink-0 font-[var(--font-mono)]">
            v{version}
          </span>
        )}
      </div>
    </div>
  );
}

/** Free disk space (v0.195.0): neutral while there's room, amber under
 *  20 GB / 10 %, red under 5 GB / 2 %. Tooltip uses System Settings' wording. */
function FreeSpaceBadge({ space }: { space: FreeSpace }) {
  const level = freeSpaceLevel(space.available, space.total);
  const color =
    level === "crit" ? "text-red-500" : level === "warn" ? "text-amber-500" : "";
  return (
    <span
      data-testid="free-space"
      data-level={level}
      title={freeSpaceTitle(space.name, space.available, space.total)}
      className={`flex items-center gap-1 font-[var(--font-mono)] ${color}`}
    >
      <HardDrive size={11} aria-hidden />
      {formatBytesDecimal(space.available)} frei
    </span>
  );
}

/** The working folder of touch/echo/mkdir/terminal/daisy + the selection. */
function FinderChip({ ctx, onReveal }: { ctx: FinderContext; onReveal?: (dir: string) => void }) {
  const sel = selectionLabel(ctx);
  return (
    <button
      type="button"
      data-testid="finder-context"
      title={contextTitle(ctx)}
      onClick={() => onReveal?.(ctx.dir)}
      className="flex min-w-0 max-w-full items-center gap-1 rounded px-1 -ml-1 hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]"
    >
      <Folder size={11} aria-hidden className="shrink-0" />
      <span className="truncate font-[var(--font-mono)]">{shortDir(ctx.dir)}</span>
      {sel && <span className="truncate text-[var(--color-fg)]">· {sel}</span>}
    </button>
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
      className="confirm-enter flex shrink-0 items-center gap-1 font-[var(--font-mono)] text-[10px] uppercase tracking-wider text-[var(--color-accent)]"
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
      className="confirm-enter flex shrink-0 items-center gap-1"
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


/**
 * The single "will this Mac sleep?" indicator (macOS, v0.152.0 — replaces the
 * wake LED and the separate sleep badge that used to sit side by side).
 *
 * ⚠️ Two badges answering one question is what read as inconsistent: the red
 * LED followed Inspector's wakelock, the amber badge followed the pmset
 * profile, and on a machine whose stored AC profile is `sleep 0` the amber one
 * could never react to the wakelock — toggling it changed nothing on screen.
 * The decision now lives in the pure, tested `sleepIndicator`; this component
 * only paints it.
 *
 * ⚠️ It is never absent on macOS. The old LED rendered only while ON, so "off"
 * looked identical to a broken indicator; `sleepable` says it out loud.
 */
const SLEEP_STYLE: Record<SleepKind, { dot: string; glow: string | undefined; pulse: boolean }> = {
  wake: {
    dot: "rgb(239, 68, 68)",
    glow: "0 0 4px rgba(239, 68, 68, 0.85), 0 0 8px rgba(239, 68, 68, 0.45)",
    pulse: true,
  },
  "no-sleep": {
    dot: "rgb(245, 158, 11)",
    glow: "0 0 4px rgba(245, 158, 11, 0.85), 0 0 8px rgba(245, 158, 11, 0.45)",
    pulse: false,
  },
  "awake-infinite": {
    dot: "rgb(56, 189, 248)",
    glow: "0 0 4px rgba(56, 189, 248, 0.85), 0 0 8px rgba(56, 189, 248, 0.45)",
    pulse: false,
  },
  "awake-timed": {
    dot: "rgb(56, 189, 248)",
    glow: "0 0 4px rgba(56, 189, 248, 0.85), 0 0 8px rgba(56, 189, 248, 0.45)",
    pulse: false,
  },
  // Quiet on purpose: a resting state must be readable without drawing the eye.
  sleepable: { dot: "var(--color-muted)", glow: undefined, pulse: false },
};

function SleepLed({
  status,
  wakelockActive,
}: {
  status: SleepStatus | null;
  wakelockActive: boolean;
}) {
  const ticking = indicatorTicks(status, wakelockActive);
  // Locally ticking remainder, anchored to the moment THIS status arrived
  // (re-anchors per poll). Lives in an effect — render stays pure (no
  // Date.now() during render; react-compiler lint enforces this).
  const [remaining, setRemaining] = useState<number | null>(null);
  useEffect(() => {
    if (!ticking) {
      setRemaining(null);
      return;
    }
    const base = status?.max_timeout_secs ?? 0;
    const anchoredAt = Date.now();
    setRemaining(base);
    const id = window.setInterval(
      () => setRemaining(Math.max(0, base - Math.floor((Date.now() - anchoredAt) / 1000))),
      1000,
    );
    return () => window.clearInterval(id);
  }, [status, ticking]);

  const ind = sleepIndicator(status, wakelockActive, remaining);
  if (!ind) return null;
  const style = SLEEP_STYLE[ind.kind];
  return (
    <span title={ind.title} className="flex shrink-0 items-center gap-1">
      {/* Keyed on the state kind: a state change (wake → sleep …) remounts
          the dot so it fades in as a CHANGE instead of silently recolouring
          (colour itself is not transitioned — not an allowed property). */}
      <span
        key={ind.kind}
        aria-hidden
        className="confirm-enter h-2 w-2 rounded-full"
        style={{
          backgroundColor: style.dot,
          boxShadow: style.glow,
          animation: style.pulse ? "wakelockPulse 1.6s ease-in-out infinite" : undefined,
        }}
      />
      <span className="font-[var(--font-mono)] text-[10px] uppercase tracking-wider">
        {ind.label}
      </span>
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
        // ⚠️ No `opacity-60` in the resting state. An 11 px glyph at 60 % of an
        // already-muted colour, with no label, next to labelled glowing
        // indicators, is invisible in practice -- that is how this control got
        // lost. It is a BUTTON and has to read as one.
        "flex shrink-0 cursor-pointer items-center gap-1 rounded px-0.5 transition-colors " +
        (on
          ? "text-violet-400"
          : "text-[var(--color-muted)] hover:text-[var(--color-fg)]")
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
      {/* ⚠️ Labelled in BOTH states. Every other footer item is glyph + an
          uppercase mono label; this one carried text only while active, so
          when off it was a bare 11 px moon nobody could place. */}
      <span className="font-[var(--font-mono)] text-[10px] uppercase tracking-wider">
        {on ? "srv" : "dark"}
      </span>
    </button>
  );
}
