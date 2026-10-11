/**
 * Whether the disco engine runs — split out of `disco-engine.ts` so App can
 * label the `disco` row without importing the engine (and the BPM analyser
 * and mic feed behind it) into the start-up bundle. The engine is the only
 * writer; it lives in one module instance, so this flag is the truth.
 */
let running = false;

export function discoRunning(): boolean {
  return running;
}

export function setDiscoRunning(value: boolean): void {
  running = value;
}
