import { describe, expect, it } from "vitest";
import { downsampleTo16kInt16 } from "./mic-record";

describe("downsampleTo16kInt16", () => {
  it("passes through at 16 kHz, converting float → i16", () => {
    const input = new Float32Array([0, 1, -1, 0.5]);
    const out = downsampleTo16kInt16(input, 16000);
    expect(out.length).toBe(4);
    expect(out[0]).toBe(0);
    expect(out[1]).toBe(0x7fff);
    expect(out[2]).toBe(-0x8000);
    expect(out[3]).toBe(Math.trunc(0.5 * 0x7fff)); // Int16Array truncates (16383)
  });

  it("halves the length resampling 32 kHz → 16 kHz", () => {
    const input = new Float32Array(3200); // 0.1 s @ 32 kHz
    const out = downsampleTo16kInt16(input, 32000);
    expect(out.length).toBe(1600); // 0.1 s @ 16 kHz
  });

  it("resamples 48 kHz → 16 kHz at a 1/3 ratio", () => {
    const input = new Float32Array(4800);
    const out = downsampleTo16kInt16(input, 48000);
    expect(out.length).toBe(1600);
  });

  it("clamps out-of-range floats", () => {
    const out = downsampleTo16kInt16(new Float32Array([2, -2]), 16000);
    expect(out[0]).toBe(0x7fff);
    expect(out[1]).toBe(-0x8000);
  });
});
