import { describe, expect, it } from "vitest";
import { newFlap, updateFlap, flapView, type Flap } from "./flap-counter";

describe("flap-counter", () => {
  it("snaps to the first real target without a flip", () => {
    const f = newFlap();
    updateFlap(f, 42, 1000, 16);
    expect(f.shown).toBe(42);
    expect(f.t).toBe(1);
    expect(flapView(f)).toEqual({ value: 42, scaleY: 1 });
  });

  it("blanks (NaN) when the target is not finite", () => {
    const f = newFlap();
    updateFlap(f, 42, 0, 16); // init
    updateFlap(f, NaN, 100, 16);
    expect(Number.isNaN(f.shown)).toBe(true);
    expect(Number.isNaN(flapView(f).value)).toBe(true);
  });

  it("steps one integer at a time toward the target (flaps up)", () => {
    const f = newFlap();
    let now = 0;
    updateFlap(f, 10, now, 16); // snap to 10
    const seen: number[] = [];
    for (let i = 0; i < 400; i++) {
      now += 25;
      updateFlap(f, 13, now, 25, 100, 50);
      if (f.t >= 1 && seen[seen.length - 1] !== f.shown) seen.push(f.shown);
    }
    // never jumped straight from 10 to 13 — it flapped through each integer
    expect(seen).toEqual([11, 12, 13]);
    expect(f.shown).toBe(13);
  });

  it("flaps down through each integer too", () => {
    const f = newFlap();
    let now = 0;
    updateFlap(f, 13, now, 16); // snap to 13
    const seen: number[] = [];
    for (let i = 0; i < 400; i++) {
      now += 25;
      updateFlap(f, 10, now, 25, 100, 50);
      if (f.t >= 1 && seen[seen.length - 1] !== f.shown) seen.push(f.shown);
    }
    expect(seen).toEqual([12, 11, 10]);
  });

  it("honours the dwell — the next flip waits for the stop", () => {
    const f = newFlap();
    updateFlap(f, 10, 0, 16, 100, 50); // snap 10
    updateFlap(f, 12, 100, 16, 100, 50); // start flip 10→11 (t=0)
    updateFlap(f, 12, 160, 60, 100, 50); // t=0.6
    updateFlap(f, 12, 220, 60, 100, 50); // t→1, shown=11, hold=270
    expect(f.shown).toBe(11);
    expect(f.t).toBe(1);
    // before the dwell elapses: no new flip
    updateFlap(f, 12, 260, 16, 100, 50);
    expect(f.t).toBe(1);
    expect(f.shown).toBe(11);
    // after the dwell: the next flip begins
    updateFlap(f, 12, 300, 16, 100, 50);
    expect(f.t).toBeLessThan(1);
    expect(f.from).toBe(11);
    expect(f.to).toBe(12);
  });

  it("stays put when already at the target", () => {
    const f = newFlap();
    updateFlap(f, 7, 0, 16);
    updateFlap(f, 7, 100, 16);
    expect(f.t).toBe(1);
    expect(f.shown).toBe(7);
  });

  it("flapView shows `from` (shrinking) in the first half, `to` (growing) in the second", () => {
    const first: Flap = { shown: 11, from: 10, to: 11, t: 0.25, hold: 0 };
    const a = flapView(first);
    expect(a.value).toBe(10);
    expect(a.scaleY).toBeGreaterThan(0);
    expect(a.scaleY).toBeLessThan(1);

    const second: Flap = { shown: 11, from: 10, to: 11, t: 0.75, hold: 0 };
    const b = flapView(second);
    expect(b.value).toBe(11);
    expect(b.scaleY).toBeGreaterThan(0);
    expect(b.scaleY).toBeLessThan(1);
  });

  it("scaleY collapses toward the flip midpoint (edge-on)", () => {
    const near: Flap = { shown: 11, from: 10, to: 11, t: 0.49, hold: 0 };
    expect(flapView(near).scaleY).toBeLessThan(0.1); // almost edge-on at t≈0.5
  });
});
