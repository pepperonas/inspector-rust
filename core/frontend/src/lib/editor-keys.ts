/**
 * One window `keydown` listener for the record editor, calling its handlers in a FIXED layer
 * order — the timeline (regions: delete/copy/paste/add) before the editor (clips, playback).
 *
 * Before this, both registered their own window listener and "the timeline runs first" held
 * only while both effects re-registered in the same commit: a re-render that re-registered
 * just the timeline (crop toggled, a take cancelled) put it behind the editor, and ⌫ with a
 * selected region deleted the clip instead. The order now comes from the layer, never from
 * when a handler was (re)registered.
 *
 * Bubble phase, like the listeners it replaces: element handlers (a focused slider's arrows)
 * still run first. The annotation overlay keeps its own capture-phase listener, which runs
 * before this one by phase. A handler that takes a key calls `preventDefault`; later layers
 * check `defaultPrevented` themselves.
 */

export type EditorKeyHandler = (e: KeyboardEvent) => void;

export const KEY_LAYER = { timeline: 0, editor: 1 } as const;
export type KeyLayer = (typeof KEY_LAYER)[keyof typeof KEY_LAYER];

const layers = new Map<KeyLayer, Set<EditorKeyHandler>>();
let installed = false;

function dispatch(e: KeyboardEvent) {
  const order = [...layers.keys()].sort((a, b) => a - b);
  for (const layer of order) {
    for (const h of [...(layers.get(layer) ?? [])]) h(e);
  }
}

function sync() {
  const any = [...layers.values()].some((s) => s.size > 0);
  if (any && !installed) {
    window.addEventListener("keydown", dispatch);
    installed = true;
  } else if (!any && installed) {
    window.removeEventListener("keydown", dispatch);
    installed = false;
  }
}

/** Registers `handler` on `layer`; returns the unregister function (for an effect cleanup). */
export function onEditorKey(layer: KeyLayer, handler: EditorKeyHandler): () => void {
  let set = layers.get(layer);
  if (!set) {
    set = new Set();
    layers.set(layer, set);
  }
  set.add(handler);
  sync();
  return () => {
    layers.get(layer)?.delete(handler);
    sync();
  };
}
