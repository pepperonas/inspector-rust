/**
 * Animation stage — Full / Reduced / Off (v0.163.0).
 *
 * The bridge between the persisted preference (SQLite `appearance.animations`,
 * whitelist mirrored by Rust `normalise_animation_stage`) and the stage
 * classes in `styles.css` (`anim-full` / `anim-reduced` / `anim-off`).
 * This module is the ONLY writer of those classes.
 *
 * Semantics: `"full"` defers to the OS — with `prefers-reduced-motion` active
 * the EFFECTIVE stage is `"reduced"`. The explicit choices `"reduced"` and
 * `"off"` are absolute. So `anim-reduced` is also the class present under an
 * OS reduction, which is what lets the class-based CSS rules serve both
 * cases; the `@media` blocks stay as the pre-JS baseline.
 *
 * House split: pure decision functions (unit-tested) + a thin impure runtime
 * (`initMotion`) that main.tsx calls in EVERY window. The Tauri imports are
 * dynamic so this file loads in tests / outside Tauri without throwing, and
 * so the eager entry chunk doesn't grow by the IPC layer.
 */

export type AnimationStage = "full" | "reduced" | "off";

/** Coerce an arbitrary stored string to a valid stage. Unknown values (a
 *  hand-edited DB, a future build's value) collapse to `"full"` — the shipped
 *  default — mirroring the Rust whitelist. */
export function normaliseAnimationStage(value: string | null | undefined): AnimationStage {
  return value === "reduced" || value === "off" ? value : "full";
}

/** The stage that actually governs rendering: an explicit "reduced"/"off" is
 *  absolute; "full" honours the OS reduced-motion hint. */
export function effectiveAnimationStage(
  stored: AnimationStage,
  osReduced: boolean,
): AnimationStage {
  return stored === "full" && osReduced ? "reduced" : stored;
}

export const STAGE_CLASSES: Record<AnimationStage, string> = {
  full: "anim-full",
  reduced: "anim-reduced",
  off: "anim-off",
};

/** Write the stage class on a root element, removing the other two. Pure DOM
 *  side-effect, idempotent; the root is injectable for tests. */
export function applyAnimationStage(
  stage: AnimationStage,
  root: Pick<HTMLElement, "classList"> = document.documentElement,
): void {
  for (const [key, cls] of Object.entries(STAGE_CLASSES)) {
    root.classList.toggle(cls, key === stage);
  }
}

/** Settings-UI label for a stage. */
export function stageLabel(stage: AnimationStage): string {
  switch (stage) {
    case "full":
      return "Full";
    case "reduced":
      return "Reduced";
    case "off":
      return "Off";
  }
}

// ── Runtime singleton ────────────────────────────────────────────────────────

let storedStage: AnimationStage = "full";
let effective: AnimationStage = "full";
let osReducedNow = false;
const listeners = new Set<(stage: AnimationStage) => void>();

/** The currently effective stage — read by JS-driven animations (the CRT
 *  popup entrance) that CSS classes can't reach. Defaults to "full" until
 *  `initMotion` has resolved the stored setting. */
export function currentAnimationStage(): AnimationStage {
  return effective;
}

export function subscribeAnimationStage(cb: (stage: AnimationStage) => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

function recompute(): void {
  const next = effectiveAnimationStage(storedStage, osReducedNow);
  if (next === effective) {
    applyAnimationStage(next); // idempotent — keeps a stripped class honest
    return;
  }
  effective = next;
  applyAnimationStage(next);
  for (const cb of listeners) cb(next);
}

/**
 * Called once per window from main.tsx, BEFORE render:
 * 1. synchronous feature detection — `@starting-style`/`transition-behavior`
 *    need Safari 17.4+; on older WKWebView/WebKitGTK the `has-enter-anim`
 *    class is absent and enter-transitions simply don't apply, so elements
 *    appear instantly instead of hanging in a half state;
 * 2. the OS reduced-motion hint (live via matchMedia change events);
 * 3. the persisted stage (async IPC) + the `animation-stage-changed` event,
 *    so a Settings change applies everywhere without restart. Outside Tauri
 *    (unit tests, a plain browser) step 3 fails silently and the stage stays
 *    at the OS-derived default.
 */
export function initMotion(): void {
  try {
    if (CSS.supports("transition-behavior: allow-discrete")) {
      document.documentElement.classList.add("has-enter-anim");
    }
  } catch {
    /* very old engine → no enter animations, which is exactly the fallback */
  }

  try {
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    osReducedNow = mq.matches;
    mq.addEventListener?.("change", (e) => {
      osReducedNow = e.matches;
      recompute();
    });
  } catch {
    /* no matchMedia (test DOM) → treat as not reduced */
  }
  recompute();

  void (async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      storedStage = normaliseAnimationStage(await invoke<string>("get_animation_stage"));
      recompute();
      const { listen } = await import("@tauri-apps/api/event");
      await listen<string>("animation-stage-changed", (e) => {
        storedStage = normaliseAnimationStage(e.payload);
        recompute();
      });
    } catch {
      /* outside Tauri — stay at the OS-derived stage */
    }
  })();
}
