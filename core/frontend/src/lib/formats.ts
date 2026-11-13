// Stage 13 of the recording editor: several formats (16:9, 9:16 …) of one project and the
// batch export over them. The format model itself lives in Rust (`record/formats.rs`); this
// file holds the labels and the batch's bookkeeping.
import type { RecordAspect } from "./ipc";

/** The formats that can be added (Rust: `formats::ADDABLE`). */
export const ADDABLE_FORMATS: readonly RecordAspect[] = ["16:9", "9:16", "1:1", "4:3", "3:4", "21:9"];

/** What a format chip says: the ratio itself, or a word for the two "as recorded" kinds. */
export function formatLabel(a: RecordAspect, auto: string, source: string): string {
  return a === "auto" ? auto : a === "source" ? source : a;
}

/** The formats still on offer for "add". */
export function addable(formats: readonly RecordAspect[]): RecordAspect[] {
  return ADDABLE_FORMATS.filter((a) => !formats.includes(a));
}

export type BatchResult = { aspect: RecordAspect; path: string } | { aspect: RecordAspect; error: string };

/** A batch export: the formats still to do (the first one runs), and what is finished. */
export interface Batch {
  queue: RecordAspect[];
  results: BatchResult[];
}

/** The formats to export, in the project's order. Nothing ticked means the active one. */
export function batchOrder(formats: readonly RecordAspect[], picked: ReadonlySet<RecordAspect>, active: RecordAspect): RecordAspect[] {
  const order = formats.filter((a) => picked.has(a));
  return order.length ? order : [active];
}

/** Every export of a batch but the last ends quietly (one Finder window, one toast). */
export const isQuiet = (b: Batch) => b.queue.length > 1;

/** The running export ended: its outcome is recorded and the next format is up. One failed
 *  format does not stop the others; a cancel stops the whole batch. */
export function batchStep(b: Batch, outcome: { path: string } | { error: string; cancelled: boolean }): Batch {
  const [aspect, ...rest] = b.queue;
  if (aspect === undefined) return b;
  if ("path" in outcome) return { queue: rest, results: [...b.results, { aspect, path: outcome.path }] };
  return { queue: outcome.cancelled ? [] : rest, results: [...b.results, { aspect, error: outcome.error }] };
}

/** A commit that sets the aspect to one the project already keeps as its own format must
 *  SWITCH to that format (its stored look), never overwrite it: the aspect to switch to,
 *  or `null` for an ordinary edit. */
export function aspectSwitchTarget(
  cur: { output: { aspect: RecordAspect }; variants?: { aspect: RecordAspect }[] },
  next: { output: { aspect: RecordAspect } },
): RecordAspect | null {
  const target = next.output.aspect;
  if (target === cur.output.aspect) return null;
  return (cur.variants ?? []).some((v) => v.aspect === target) ? target : null;
}

export interface BatchPos {
  format: RecordAspect;
  n: number;
  total: number;
}

/** Runs a batch: one export after the other. `run` starts one export (its end arrives
 *  through `outcome`, or as a rejection when it could not even start). */
export function makeBatchRunner(on: { pos: (p: BatchPos | null) => void; done: (results: BatchResult[]) => void }) {
  let batch: Batch | null = null;
  let run: (format: RecordAspect, quiet: boolean) => Promise<unknown> = async () => undefined;
  const next = (): void => {
    const b = batch;
    if (!b) return;
    const format = b.queue[0];
    if (format === undefined) {
      batch = null;
      on.pos(null);
      on.done(b.results);
      return;
    }
    on.pos({ format, n: b.results.length + 1, total: b.results.length + b.queue.length });
    run(format, isQuiet(b)).catch((e) => {
      if (batch !== b) return;
      batch = batchStep(b, { error: String(e), cancelled: false });
      next();
    });
  };
  return {
    active: () => batch !== null,
    start(order: RecordAspect[], runOne: typeof run) {
      run = runOne;
      batch = { queue: order, results: [] };
      next();
    },
    /** The running export ended (the done/failed event). */
    outcome(o: { path: string } | { error: string; cancelled: boolean }) {
      if (!batch) return;
      batch = batchStep(batch, o);
      next();
    },
  };
}
