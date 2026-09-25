// A resumed run must survive React StrictMode, which mounts effects twice in development. The canvas
// effect used to clear the "keep the restored run" flag on its first pass, so the second pass reseeded
// the game: the pipes vanished while the restored score stayed on screen. The canvas needs a real 2D
// context, which happy-dom lacks, so this pins the source instead: the flag is cleared only by a real
// restart (resetMatch), never inside the effect.
import { describe, expect, it } from "vitest";
import src from "./FlappyGame.tsx?raw";

const code = src.replace(/\/\/.*$/gm, "");

describe("FlappyGame resume under StrictMode", () => {
  it("clears keepResumed only in resetMatch", () => {
    const clears = [...code.matchAll(/keepResumed\.current\s*=\s*false/g)];
    expect(clears).toHaveLength(1);
    const reset = code.slice(code.indexOf("const resetMatch"), code.indexOf("};", code.indexOf("const resetMatch")));
    expect(reset).toMatch(/keepResumed\.current\s*=\s*false/);
  });

  it("the canvas effect only reseeds a fresh run", () => {
    expect(code).toMatch(/if\s*\(!keepResumed\.current\)\s*s\.game\s*=\s*initialState/);
  });
});
