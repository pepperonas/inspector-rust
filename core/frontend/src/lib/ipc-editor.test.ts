import { describe, expect, it } from "vitest";
import { decodeEditorFrame } from "./ipc";

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
