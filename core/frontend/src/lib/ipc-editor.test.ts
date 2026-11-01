// The editor's millisecond IPCs take `u64` in Rust: serde rejects `1234.56`, and the old seek
// swallowed that error — clicks on the timeline moved only the local playhead (review C1).
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import { decodeEditorFrame, editorSeek, editorToProgramme, editorToSource } from "./ipc";

function frame(gen: bigint, w: number, h: number, px: number[]): ArrayBuffer {
  const buf = new ArrayBuffer(16 + px.length);
  const v = new DataView(buf);
  v.setBigUint64(0, gen, true);
  v.setUint32(8, w, true);
  v.setUint32(12, h, true);
  new Uint8Array(buf, 16).set(px);
  return buf;
}

describe("decodeEditorFrame (editor_frame's raw layout)", () => {
  it("reads gen, size and pixels little-endian", () => {
    const f = decodeEditorFrame(frame(7n, 2, 1, [1, 2, 3, 4, 5, 6, 7, 8]));
    expect(f).not.toBeNull();
    expect(f!.gen).toBe(7);
    expect([f!.width, f!.height]).toEqual([2, 1]);
    expect(Array.from(f!.rgba)).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it("an empty response means nothing newer", () => {
    expect(decodeEditorFrame(new ArrayBuffer(0))).toBeNull();
  });

  it("refuses a body whose pixel count does not match its size", () => {
    expect(decodeEditorFrame(frame(1n, 2, 2, [0, 0, 0, 0]))).toBeNull();
  });
});

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue(null);
});

describe("editor ms IPCs send integers", () => {
  it("a fractional seek (1000/30 per frame, pointer px) is rounded", async () => {
    await editorSeek(1234.56);
    expect(mockInvoke).toHaveBeenCalledWith("editor_seek", { programmeMs: 1235 });
    await editorSeek(1000 / 30);
    expect(mockInvoke).toHaveBeenLastCalledWith("editor_seek", { programmeMs: 33 });
    await editorSeek(-4.2);
    expect(mockInvoke).toHaveBeenLastCalledWith("editor_seek", { programmeMs: 0 });
  });

  it("the mapping queries round too", async () => {
    await editorToSource(99.5);
    expect(mockInvoke).toHaveBeenCalledWith("editor_to_source", { programmeMs: 100 });
    await editorToProgramme(1, 4000.4);
    expect(mockInvoke).toHaveBeenLastCalledWith("editor_to_programme", { clip: 1, sourceMs: 4000 });
  });
});
