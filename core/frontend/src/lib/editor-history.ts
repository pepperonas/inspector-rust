//! Snapshot-based undo/redo for the screenshot editor. Generic over the
//! document type `T` so it can be unit-tested with plain values; the editor
//! instantiates it with its `EditorDoc` (annotations + crop + background).
//!
//! The classic past/present/future triple: `commit` pushes the old present
//! onto the past and clears the future (a new edit invalidates redo); `undo`
//! and `redo` shuffle one step between the stacks. This replaces the old
//! append/pop-of-annotations stack, which could only represent "added the
//! last annotation" and had no way to undo a move/resize/restyle/crop.

export type HistoryState<T> = { past: T[]; present: T; future: T[] };
export type HistoryAction<T> =
  | { type: "commit"; next: T }
  | { type: "undo" }
  | { type: "redo" }
  | { type: "reset"; present: T };

export function initHistory<T>(present: T): HistoryState<T> {
  return { past: [], present, future: [] };
}

export function historyReducer<T>(
  s: HistoryState<T>,
  a: HistoryAction<T>,
  limit = 50,
): HistoryState<T> {
  switch (a.type) {
    case "commit": {
      const past = [...s.past, s.present];
      // Cap the past at `limit`, dropping the oldest entries.
      return { past: past.length > limit ? past.slice(past.length - limit) : past, present: a.next, future: [] };
    }
    case "undo": {
      if (s.past.length === 0) return s;
      const present = s.past[s.past.length - 1];
      return { past: s.past.slice(0, -1), present, future: [s.present, ...s.future] };
    }
    case "redo": {
      if (s.future.length === 0) return s;
      const present = s.future[0];
      return { past: [...s.past, s.present], present, future: s.future.slice(1) };
    }
    case "reset":
      return { past: [], present: a.present, future: [] };
  }
}

export function canUndo<T>(s: HistoryState<T>): boolean {
  return s.past.length > 0;
}

export function canRedo<T>(s: HistoryState<T>): boolean {
  return s.future.length > 0;
}
