import type { SleepStatus } from "./ipc";
import { formatHolders, formatSleepCountdown } from "./sleep-status";

/**
 * One footer indicator for "will this Mac sleep?", replacing the two that used
 * to sit side by side (v0.152.0).
 *
 * ⚠️ Two badges answering the SAME question is what made the footer read as
 * inconsistent: the red wake LED followed Inspector's own wakelock, while the
 * amber `no-sleep` badge followed the pmset profile — and on a machine whose
 * stored AC profile is `sleep 0` the amber one could never react to the
 * wakelock at all (its branch returned before assertions were even looked at).
 * Toggling the wakelock visibly changed nothing, which is exactly the reported
 * defect. One indicator, priority-ordered by CAUSE, cannot contradict itself.
 *
 * ⚠️ It is also never absent on macOS. The old wake LED rendered only while ON,
 * so "off" was indistinguishable from a broken indicator — the `sleepable`
 * state exists to say "the Mac may sleep" out loud.
 */
export type SleepKind =
  /** Inspector's own wakelock holds the Mac awake. */
  | "wake"
  /** The active pmset profile has `sleep 0` — idle sleep never happens. */
  | "no-sleep"
  /** Someone holds an assertion without a timeout. */
  | "awake-infinite"
  /** Assertions hold, but they expire. */
  | "awake-timed"
  /** Nothing prevents sleep. */
  | "sleepable";

export interface SleepIndicator {
  kind: SleepKind;
  label: string;
  title: string;
}

/**
 * Decide what the footer shows. Pure — `remainingSecs` is the locally ticked
 * countdown (null = use the status value).
 *
 * ⚠️ The wakelock outranks everything, including `sleep_disabled`: it is the
 * one cause the user just triggered themselves, and showing it is the whole
 * point of the indicator. The profile state is still reachable — turn the
 * wakelock off and the badge falls through to it.
 *
 * Returns `null` only where there is nothing truthful to say: a platform
 * without support, or a status that has not loaded yet AND no wakelock.
 */
export function sleepIndicator(
  status: SleepStatus | null,
  wakelockActive: boolean,
  remainingSecs: number | null,
): SleepIndicator | null {
  const holders = status?.holders.length ? ` Wachhalter: ${formatHolders(status.holders)}.` : "";

  if (wakelockActive) {
    return {
      kind: "wake",
      label: "wake",
      title:
        "Inspector Rust hält den Mac wach — er schläft nicht und sperrt nicht. " +
        "Mit `wakelock off` (oder `caffeine off`) ausschalten." +
        holders,
    };
  }
  if (!status?.supported) return null;

  if (status.sleep_disabled) {
    return {
      kind: "no-sleep",
      label: "no-sleep",
      title:
        "Das Energieprofil steht auf `sleep 0` — der Mac schläft nie von selbst. " +
        "Das kommt vom Profil, nicht vom Wakelock; mit `nosleep off` zurückstellen." +
        holders,
    };
  }
  if (status.prevented && status.indefinite) {
    return {
      kind: "awake-infinite",
      label: "wach ∞",
      title: `Der Mac wird ohne Zeitlimit wachgehalten (Assertion ohne Timeout).${holders}`,
    };
  }
  if (status.prevented) {
    const secs = remainingSecs ?? status.max_timeout_secs ?? 0;
    return {
      kind: "awake-timed",
      label: `wach ${formatSleepCountdown(secs)}`,
      title: `Der Mac wird wachgehalten — so lange noch, bis Sleep wieder möglich ist.${holders}`,
    };
  }
  return {
    kind: "sleepable",
    label: "sleep",
    title: "Nichts hält den Mac wach — er darf einschlafen.",
  };
}

/** Does this state tick a local countdown between polls? */
export function indicatorTicks(status: SleepStatus | null, wakelockActive: boolean): boolean {
  return (
    !wakelockActive &&
    !!status?.supported &&
    !status.sleep_disabled &&
    status.prevented &&
    !status.indefinite &&
    status.max_timeout_secs != null
  );
}
