import { describe, expect, it } from "vitest";
import { WORLD_MASK_H, WORLD_MASK_W, isLand, project } from "./worldmask";

describe("isLand", () => {
  it("rejects out-of-bounds cells (never throws, returns false)", () => {
    expect(isLand(-1, 0)).toBe(false);
    expect(isLand(0, -1)).toBe(false);
    expect(isLand(WORLD_MASK_W, 0)).toBe(false);
    expect(isLand(0, WORLD_MASK_H)).toBe(false);
    expect(isLand(WORLD_MASK_W, WORLD_MASK_H)).toBe(false);
    expect(isLand(9999, 9999)).toBe(false);
  });

  it("treats every in-bounds cell as a boolean without error", () => {
    // Spot-check the four grid corners + centre resolve to a real boolean.
    for (const [c, r] of [
      [0, 0],
      [WORLD_MASK_W - 1, 0],
      [0, WORLD_MASK_H - 1],
      [WORLD_MASK_W - 1, WORLD_MASK_H - 1],
      [180, 90],
    ]) {
      expect(typeof isLand(c, r)).toBe("boolean");
    }
  });

  it("marks known land cells and known ocean cells correctly", () => {
    // Central Sahara (~24°N, 12°E) is unambiguous land; the mid-Pacific
    // (~0°N, 150°W) is unambiguous open ocean. Convert via project() so the
    // test survives any future grid re-origin.
    const sahara = project(12, 24);
    const pacific = project(-150, 0);
    const scol = Math.floor(sahara.fx * WORLD_MASK_W);
    const srow = Math.floor(sahara.fy * WORLD_MASK_H);
    const pcol = Math.floor(pacific.fx * WORLD_MASK_W);
    const prow = Math.floor(pacific.fy * WORLD_MASK_H);
    expect(isLand(scol, srow)).toBe(true);
    expect(isLand(pcol, prow)).toBe(false);
  });

  it("has both land and ocean cells overall (mask is not all-0 / all-1)", () => {
    let land = 0;
    let ocean = 0;
    for (let r = 0; r < WORLD_MASK_H; r++) {
      for (let c = 0; c < WORLD_MASK_W; c++) {
        if (isLand(c, r)) land++;
        else ocean++;
      }
    }
    expect(land).toBeGreaterThan(0);
    expect(ocean).toBeGreaterThan(0);
    // Earth is ~29% land; a decoded mask should be in a sane ballpark, not
    // e.g. 99% (would mean the bit order got flipped).
    const frac = land / (land + ocean);
    expect(frac).toBeGreaterThan(0.1);
    expect(frac).toBeLessThan(0.6);
  });
});

describe("project", () => {
  it("maps the four corners of the lon/lat domain to the unit square", () => {
    expect(project(-180, 90)).toEqual({ fx: 0, fy: 0 }); // top-left
    expect(project(180, -90)).toEqual({ fx: 1, fy: 1 }); // bottom-right
    expect(project(-180, -90)).toEqual({ fx: 0, fy: 1 });
    expect(project(180, 90)).toEqual({ fx: 1, fy: 0 });
  });

  it("maps the origin (0,0) to the centre", () => {
    expect(project(0, 0)).toEqual({ fx: 0.5, fy: 0.5 });
  });

  it("moves east → right (fx up) and north → up (fy down)", () => {
    expect(project(90, 0).fx).toBeGreaterThan(project(-90, 0).fx);
    expect(project(0, 45).fy).toBeLessThan(project(0, -45).fy);
  });

  it("is linear in longitude and latitude", () => {
    // A 60° eastward step is always +1/6 of the width, anywhere.
    expect(project(60, 0).fx - project(0, 0).fx).toBeCloseTo(1 / 6, 10);
    expect(project(0, 30).fy - project(0, 60).fy).toBeCloseTo(1 / 6, 10);
  });
});
