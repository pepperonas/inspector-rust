import { describe, expect, it } from "vitest";

// The stylesheet contract is pinned at the SOURCE (the readme-badges pattern)
// — but ⚠️ NOT via `styles.css?raw`: the Tailwind Vite plugin claims every
// .css request in the test pipeline too and returns an empty module (found
// red here). Read from disk instead. The frontend has no @types/node, so the
// specifier is computed — tsc then skips module resolution while vitest
// (running on Node) resolves the builtin fine.
const { readFileSync } = (await import("node:" + "fs")) as unknown as {
  readFileSync(path: string, encoding: "utf8"): string;
};
// import.meta.url is a server-style `/src/…` URL under vitest — resolve from
// the package root instead (vitest always runs with cwd = core/frontend).
const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
const css = readFileSync(cwd + "/src/styles.css", "utf8");
import {
  applyAnimationStage,
  effectiveAnimationStage,
  normaliseAnimationStage,
  STAGE_CLASSES,
  stageLabel,
} from "./motion-stage";

describe("normaliseAnimationStage", () => {
  it("passes the three valid stages through", () => {
    expect(normaliseAnimationStage("full")).toBe("full");
    expect(normaliseAnimationStage("reduced")).toBe("reduced");
    expect(normaliseAnimationStage("off")).toBe("off");
  });

  it("collapses garbage / unset to the shipped default", () => {
    // A hand-edited DB or a future build's value must render as the default
    // experience, never wedge the UI into an unknown state.
    for (const raw of [null, undefined, "", "OFF", "none", "voll", " reduced "]) {
      expect(normaliseAnimationStage(raw)).toBe("full");
    }
  });
});

describe("effectiveAnimationStage", () => {
  it("'full' defers to the OS reduced-motion hint", () => {
    expect(effectiveAnimationStage("full", false)).toBe("full");
    expect(effectiveAnimationStage("full", true)).toBe("reduced");
  });

  it("explicit 'reduced' and 'off' are absolute", () => {
    expect(effectiveAnimationStage("reduced", false)).toBe("reduced");
    expect(effectiveAnimationStage("off", false)).toBe("off");
    // ⚠️ "off" must never be UPGRADED by the OS hint — off is off.
    expect(effectiveAnimationStage("off", true)).toBe("off");
    expect(effectiveAnimationStage("reduced", true)).toBe("reduced");
  });
});

describe("applyAnimationStage", () => {
  it("sets exactly one stage class and clears the others", () => {
    const root = document.createElement("div");
    applyAnimationStage("reduced", root);
    expect(root.classList.contains("anim-reduced")).toBe(true);
    expect(root.classList.contains("anim-full")).toBe(false);
    expect(root.classList.contains("anim-off")).toBe(false);
    applyAnimationStage("off", root);
    expect(root.classList.contains("anim-off")).toBe(true);
    expect(root.classList.contains("anim-reduced")).toBe(false);
  });
});

describe("the 'Off' stage is wired through the stylesheet", () => {
  // Vitest's happy-dom does not compute styles from the real stylesheet, so
  // the guarantee "Off disables every transition" is pinned at the SOURCE
  // (the house pattern for CSS contracts): the global kill rule must exist,
  // carry !important, spare .anim-keep, and zero delays too (staggers).
  it("carries the global kill under html.anim-off", () => {
    const rule = css.match(/html\.anim-off \*[^{]*\{[^}]*\}/s)?.[0] ?? "";
    expect(rule).toContain(":not(.anim-keep)");
    expect(rule).toContain("transition-duration: 0s !important");
    expect(rule).toContain("animation-duration: 0s !important");
    expect(rule).toContain("animation-delay: 0s !important");
  });

  it("zeroes every timing token under reduced AND off", () => {
    const block = css.match(/html\.anim-reduced,\s*html\.anim-off \{[^}]*\}/s)?.[0] ?? "";
    for (const token of [
      "--duration-instant",
      "--duration-fast",
      "--duration-base",
      "--duration-slow",
      "--md3-short1",
      "--md3-short2",
      "--md3-short4",
      "--md3-medium2",
      "--md3-medium4",
    ]) {
      expect(block, token).toContain(`${token}: 0`);
    }
  });

  it("enter transitions are gated on the feature class (old WebKit = instant)", () => {
    // Etappe 3: the panel swap uses @starting-style, which needs Safari
    // 17.4+. Without the has-enter-anim gate an old WKWebView would apply
    // the transition rule but never the starting style — elements could
    // hang in a half state instead of appearing instantly.
    expect(css).toContain(".has-enter-anim .panel-enter");
    expect(css).toContain("@starting-style");
  });

  it("the stage classes in CSS and TS are the same three", () => {
    for (const cls of Object.values(STAGE_CLASSES)) {
      if (cls === "anim-full") continue; // full = absence of damping, no rule needed
      expect(css).toContain(`html.${cls}`);
    }
  });
});

describe("stageLabel", () => {
  it("labels all three stages", () => {
    expect(stageLabel("full")).toBe("Full");
    expect(stageLabel("reduced")).toBe("Reduced");
    expect(stageLabel("off")).toBe("Off");
  });
});
