import { describe, expect, it } from "vitest";
import { formatReplayRow, verdictLabel } from "./gesture-trace";

describe("gesture trace replay labels", () => {
  it("names every verdict the backend emits", () => {
    expect(verdictLabel("dispatched")).toBe("fired");
    expect(verdictLabel("typing_guard")).toBe("blocked (typing)");
    expect(verdictLabel("unmapped")).toBe("ignored (no binding)");
    expect(verdictLabel("config:gestures.mute is off")).toBe("blocked (gestures.mute is off)");
    expect(verdictLabel("finger_count_changed")).toBe("blocked (finger count changed)");
    expect(verdictLabel("cooldown")).toBe("blocked (cooldown)");
  });

  it("formats a row with time, kind, fingers, action and verdict", () => {
    const line = formatReplayRow({
      t_ms: 12345,
      via: "tick",
      level: "typing",
      kind: "tap",
      fingers: 3,
      verdict: "typing_guard",
      action: "mute_toggle",
    });
    expect(line).toBe(" 12.35 s  tap ×3  → mute_toggle  blocked (typing)");
  });

  it("omits the arrow when nothing was bound", () => {
    const line = formatReplayRow({
      t_ms: 50,
      via: "frame",
      level: "config",
      kind: "swipe_down",
      fingers: 2,
      verdict: "unmapped",
      action: null,
    });
    expect(line).toBe("  0.05 s  swipe_down ×2  ignored (no binding)");
  });
});
