import { MIN_ZOOM_MS, removeAnnotation, removeTrack, removeZoom } from "./editor";
import type { RecordAnnotation, RecordAudioTrack, RecordEditDoc, RecordZoomRegion } from "./ipc";

/**
 * What the record editor has selected: exactly one object — a clip (by index) or a zoom,
 * annotation or audio track (by id) — or nothing. Delete, copy and the inspector act on it.
 */
export type Selection =
  | { kind: "clip"; index: number }
  | { kind: "zoom"; id: string }
  | { kind: "annotation"; id: string }
  | { kind: "track"; id: string }
  | null;

/** The editor's region clipboard: a snapshot of the copied regions (never the live objects). */
export interface ClipboardRegions {
  zooms: RecordZoomRegion[];
  annotations: RecordAnnotation[];
  tracks: RecordAudioTrack[];
}

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

/** The selected region as clipboard data; `null` for a clip, nothing or a missing id. */
export function copyRegions(doc: RecordEditDoc, selection: Selection): ClipboardRegions | null {
  if (!selection || selection.kind === "clip") return null;
  const out: ClipboardRegions = { zooms: [], annotations: [], tracks: [] };
  if (selection.kind === "zoom") {
    const z = doc.zooms.find((x) => x.id === selection.id);
    if (!z) return null;
    out.zooms.push(clone(z));
  } else if (selection.kind === "annotation") {
    const a = doc.annotations.find((x) => x.id === selection.id);
    if (!a) return null;
    out.annotations.push(clone(a));
  } else {
    const t = doc.audioTracks.find((x) => x.id === selection.id);
    if (!t) return null;
    out.tracks.push(clone(t));
  }
  return out;
}

/** Copy + remove as one document (one undo step); `null` when there is nothing to cut. */
export function cutRegions(doc: RecordEditDoc, selection: Selection): { data: ClipboardRegions; doc: RecordEditDoc } | null {
  const data = copyRegions(doc, selection);
  if (!data || !selection || selection.kind === "clip") return null;
  const next =
    selection.kind === "zoom"
      ? removeZoom(doc, selection.id)
      : selection.kind === "annotation"
        ? removeAnnotation(doc, selection.id)
        : removeTrack(doc, selection.id);
  return { data, doc: next };
}

/**
 * Pastes `data` at `sourceMs` (SOURCE ms — the playhead as Rust mapped it) of `clip`: new ids,
 * the earliest region starts at the paste position and the others keep their offsets; every
 * start is clamped into the clip. A region that would run past the clip end keeps its length
 * by starting earlier (like `addAnnotation`); only a clip shorter than the region cuts it to
 * the clip. A region that would still be too short for Rust to keep (a zoom under
 * `MIN_ZOOM_MS`) is left out — and when nothing is left, `doc` comes back unchanged.
 * Zooms become the user's (`manual`), annotations go on top. Tracks only move their anchor —
 * their length is the file, which Rust lays across the programme.
 */
export function pasteRegions(doc: RecordEditDoc, clip: number, sourceMs: number, data: ClipboardRegions): RecordEditDoc {
  const c = doc.clips[clip];
  if (!c) return doc;
  const starts = [...data.zooms, ...data.annotations, ...data.tracks].map((r) => r.startMs);
  if (starts.length === 0) return doc;
  const base = Math.min(...starts);
  const at = Math.round(sourceMs);
  const place = (startMs: number) => Math.min(c.endMs, Math.max(c.startMs, at + (startMs - base)));
  const span = (r: { startMs: number; endMs: number }) => {
    const len = Math.min(r.endMs - r.startMs, c.endMs - c.startMs);
    const startMs = Math.max(c.startMs, Math.min(place(r.startMs), c.endMs - len));
    return { startMs, endMs: startMs + len };
  };
  let z = Math.max(0, ...doc.annotations.map((a) => a.zIndex));
  const zooms = data.zooms
    .map((r) => ({ ...clone(r), id: crypto.randomUUID(), clip, ...span(r), origin: "manual" as const }))
    .filter((r) => r.endMs - r.startMs >= MIN_ZOOM_MS);
  const annotations = data.annotations
    .map((r) => ({ ...clone(r), id: crypto.randomUUID(), clip, ...span(r) }))
    .filter((r) => r.endMs > r.startMs)
    .map((r) => ({ ...r, zIndex: (z += 1) }));
  const tracks = data.tracks.map((r) => ({ ...clone(r), id: crypto.randomUUID(), clip, startMs: place(r.startMs) }));
  if (!zooms.length && !annotations.length && !tracks.length) return doc;
  return {
    ...doc,
    zooms: zooms.length ? [...doc.zooms, ...zooms] : doc.zooms,
    annotations: annotations.length ? [...doc.annotations, ...annotations] : doc.annotations,
    audioTracks: tracks.length ? [...doc.audioTracks, ...tracks] : doc.audioTracks,
  };
}

/**
 * What to select after `pasteRegions` turned `before` into `after`: the first pasted region
 * (`null` when nothing was pasted — never an id Rust would drop).
 */
export function selectionAfterPaste(before: RecordEditDoc, after: RecordEditDoc): Selection {
  const added = <T extends { id: string }>(a: T[], b: T[]) => b.slice(a.length)[0]?.id;
  const z = added(before.zooms, after.zooms);
  if (z) return { kind: "zoom", id: z };
  const a = added(before.annotations, after.annotations);
  if (a) return { kind: "annotation", id: a };
  const t = added(before.audioTracks, after.audioTracks);
  return t ? { kind: "track", id: t } : null;
}
