import catalogue from "../../../rust-lib/assets/record/effects/params.json";
import type { RecordEditDoc, RecordEffectValue, RecordTakeMeta } from "./ipc";

/**
 * The shader-effect catalogue for the UI. The single declaration lives in
 * `core/rust-lib/assets/record/effects/params.json` (Rust and the helper read the same file);
 * this module only types and queries it — it holds no copy.
 */
export type EffectParam =
  | { name: string; type: "number"; min: number; max: number; default: number }
  | { name: string; type: "toggle"; default: boolean }
  | { name: string; type: "color"; default: string };

export interface EffectSpec {
  id: string;
  /** The effect reads the recorded pointer (ripple, spotlight). */
  needsPointer: boolean;
  params: EffectParam[];
}

export const EFFECTS: readonly EffectSpec[] = catalogue as unknown as EffectSpec[];

export function effectSpec(id: string): EffectSpec | undefined {
  return EFFECTS.find((e) => e.id === id);
}

/** Parameter defaults by name. */
export function effectDefaults(id: string): Record<string, RecordEffectValue> {
  return Object.fromEntries((effectSpec(id)?.params ?? []).map((p) => [p.name, p.default]));
}

/** Defaults overlaid with a region's overrides — what the effect actually renders with. */
export function effectValues(id: string, overrides: Record<string, RecordEffectValue>): Record<string, RecordEffectValue> {
  return { ...effectDefaults(id), ...overrides };
}

/** `value` made valid for `param` (numbers clamped, colours `#rrggbb`); `undefined` = wrong type. */
export function coerceEffectValue(param: EffectParam, value: RecordEffectValue): RecordEffectValue | undefined {
  if (param.type === "number") {
    return typeof value === "number" && Number.isFinite(value) ? Math.min(param.max, Math.max(param.min, value)) : undefined;
  }
  if (param.type === "toggle") return typeof value === "boolean" ? value : undefined;
  return typeof value === "string" && /^#[0-9a-fA-F]{6}$/.test(value) ? value.toLowerCase() : undefined;
}

/** Length of an effect added from the Effects tab (source ms, clipped to the clip). */
export const EFFECT_ADD_MS = 2_000;

/**
 * Whether `clip` has recorded pointer data: an inserted take answers with its own meta
 * (`takes[source].hasCursor`, Rust's `take_metas`), the project's own take with
 * `EditorAssets.hasCursorData` — the same two sources the editor already reads.
 */
export function clipHasPointer(
  doc: RecordEditDoc,
  clip: number,
  ownHasCursor: boolean,
  takes: Record<string, RecordTakeMeta> | undefined,
): boolean {
  const c = doc.clips[clip];
  if (!c) return false;
  return c.source ? !!takes?.[c.source]?.hasCursor : ownHasCursor;
}
