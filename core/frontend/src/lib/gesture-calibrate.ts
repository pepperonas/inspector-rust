// Pure helpers for the calibration card and the recordings list of the
// `gestures` panel (gesture guard Phase 5). Rendering lives in
// `components/GestureCalibration.tsx` and `components/GestureRecordings.tsx`.

import type {
  GestureCalibrationChange,
  GestureCalibrationStatus,
  GestureTraceFile,
} from "./ipc";
import { locale, t } from "./i18n";

/** The three steps, as the backend runs them (`gestures::calibrate`). */
export const CALIBRATION_STEPS: { readonly title: string; readonly instruction: string }[] = [
  {
    get title() { return t("gcal.step.palm"); },
    get instruction() { return t("gcal.step.palm_do"); },
  },
  {
    get title() { return t("gcal.step.thumb"); },
    get instruction() { return t("gcal.step.thumb_do"); },
  },
  {
    get title() { return t("gcal.step.fingers"); },
    get instruction() { return t("gcal.step.fingers_do"); },
  },
];

export type CalibrationView =
  | { kind: "idle" }
  | { kind: "countdown"; seconds: number }
  | { kind: "step"; index: number; of: number; title: string; instruction: string; progress: number; seconds: number }
  | { kind: "computing" }
  | { kind: "done" }
  | { kind: "failed"; error: string };

/** What the calibration card shows for a backend status. */
export function calibrationView(s: GestureCalibrationStatus | undefined): CalibrationView {
  if (!s || s.state === "idle") return { kind: "idle" };
  if (s.state === "done") return { kind: "done" };
  if (s.state === "failed") return { kind: "failed", error: s.error };
  const p = s.phase;
  if (p.phase === "countdown") return { kind: "countdown", seconds: Math.ceil(p.remaining_ms / 1000) };
  // The backend finishes the run a moment after the last step ends.
  if (p.phase === "finished") return { kind: "computing" };
  const step = CALIBRATION_STEPS[p.step - 1] ?? CALIBRATION_STEPS[CALIBRATION_STEPS.length - 1];
  const elapsed = p.step_ms - p.remaining_ms;
  return {
    kind: "step",
    index: p.step,
    of: p.steps,
    title: step.title,
    instruction: step.instruction,
    progress: p.step_ms > 0 ? Math.min(1, Math.max(0, elapsed / p.step_ms)) : 0,
    seconds: Math.ceil(p.remaining_ms / 1000),
  };
}

const CHANGE_LABEL: Record<GestureCalibrationChange["key"], { readonly label: string; unit: string; scale: number }> = {
  palm_size: { get label() { return t("gcal.change.palm_size"); }, unit: "", scale: 1 },
  thumb_ratio: { get label() { return t("gcal.change.thumb_ratio"); }, unit: "", scale: 1 },
  thumb_min_size: { get label() { return t("gcal.change.thumb_min_size"); }, unit: "", scale: 1 },
  thumb_zone: { get label() { return t("gcal.change.thumb_zone"); }, unit: "%", scale: 100 },
};

/** One proposed change as a table row: "Handballen ab Größe  2,0 → 1,9". */
export function describeChange(c: GestureCalibrationChange) {
  const meta = CHANGE_LABEL[c.key] ?? { label: c.key, unit: "", scale: 1 };
  const fmt = (v: number) => {
    const n = Math.round(v * meta.scale * 100) / 100;
    return `${n.toLocaleString(locale())}${meta.unit ? ` ${meta.unit}` : ""}`;
  };
  return { key: c.key, label: meta.label, from: fmt(c.current), to: fmt(c.proposed), up: c.proposed > c.current };
}

/** "04.10. 14:23:05" from a trace file name; the raw name when it doesn't parse. */
export function traceWhen(name: string): string {
  const m = /(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})\.json$/.exec(name);
  if (!m) return name;
  return `${m[3]}.${m[2]}. ${m[4]}:${m[5]}:${m[6]}`;
}

export function traceKindLabel(trace: GestureTraceFile): string {
  return trace.unintended ? t("gcal.trace.unintended") : t("gcal.trace.recording");
}

/** "12 KB" / "1,4 MB". */
export function traceSize(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / 1024 / 1024).toLocaleString(locale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 })} MB`;
}
