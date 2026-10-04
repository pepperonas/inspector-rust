import { describe, expect, it } from "vitest";
import type { GestureLogEntry } from "./ipc";
import {
  ACTION_TYPES,
  actionLabel,
  captureFromLog,
  conflictingIds,
  defaultTypingGuard,
  emptyAction,
  latestSeq,
  mayCollideWithSystem,
  missingField,
  newBinding,
  sameTrigger,
  shortTarget,
  triggerLabel,
  type GestureBinding,
} from "./gesture-bindings";

function b(patch: Partial<GestureBinding> & { id: string }): GestureBinding {
  return { ...newBinding(), ...patch };
}

function entry(seq: number, kind: GestureLogEntry["kind"], fingers: number | null): GestureLogEntry {
  return {
    seq,
    at_ms: seq,
    device: 0,
    level: "config",
    verdict: "unmapped",
    accepted: false,
    kind,
    fingers,
    action: null,
    via: "frame",
    touch_id: null,
  };
}

describe("labels", () => {
  it("names triggers with their finger count, tip-taps without", () => {
    expect(triggerLabel({ kind: "swipe_left", fingers: 4 })).toBe("4 Finger · Wischen nach links");
    expect(triggerLabel({ kind: "tip_tap_right", fingers: 2 })).toBe("Tip-Tap rechts");
  });

  it("labels actions from their context and shortens targets", () => {
    expect(actionLabel({ type: "volume_up" })).toBe("Lauter");
    expect(actionLabel({ type: "hotkey", action: "ocr" }, { hotkeys: { ocr: "OCR region → text" } })).toBe(
      "OCR region → text",
    );
    expect(actionLabel({ type: "task", id: 7 }, { tasks: { 7: "Downloads aufräumen" } })).toBe(
      "KI-Task: Downloads aufräumen",
    );
    expect(actionLabel({ type: "task", id: 9 })).toBe("KI-Task: #9");
    expect(actionLabel({ type: "open", target: "https://www.celox.io/" })).toBe("Öffnen: celox.io");
    expect(shortTarget("/Applications/Safari.app")).toBe("Safari.app");
  });

  it("offers every action type exactly once", () => {
    const types = ACTION_TYPES.map((t) => t.type);
    expect(new Set(types).size).toBe(types.length);
    for (const t of types) expect(emptyAction(t).type).toBe(t);
  });
});

describe("conflicts", () => {
  it("flags two active bindings on one gesture in one scope", () => {
    const list = [
      b({ id: "a", trigger: { kind: "swipe_left", fingers: 3 } }),
      b({ id: "b", trigger: { kind: "swipe_left", fingers: 3 } }),
      b({ id: "c", trigger: { kind: "swipe_left", fingers: 4 } }),
    ];
    expect([...conflictingIds(list)].sort()).toEqual(["a", "b"]);
  });

  it("allows a disabled twin and a different app scope", () => {
    const list = [
      b({ id: "a", trigger: { kind: "tap", fingers: 3 } }),
      b({ id: "b", trigger: { kind: "tap", fingers: 3 }, enabled: false }),
      b({ id: "c", trigger: { kind: "tap", fingers: 3 }, app: "com.apple.Safari", app_name: "Safari" }),
    ];
    expect(conflictingIds(list).size).toBe(0);
  });

  it("treats tip-taps as one gesture whatever count they carry", () => {
    expect(sameTrigger({ kind: "tip_tap_left", fingers: 2 }, { kind: "tip_tap_left", fingers: 4 })).toBe(true);
    expect(sameTrigger({ kind: "tap", fingers: 3 }, { kind: "tap", fingers: 4 })).toBe(false);
  });
});

describe("what is still missing", () => {
  it("names the missing target per action", () => {
    expect(missingField(b({ id: "", action: { type: "shortcut", keys: "" } }))).toBe("Tastenkürzel aufnehmen");
    expect(missingField(b({ id: "", action: { type: "open", target: "  " } }))).toBe("Adresse oder Pfad eingeben");
    expect(missingField(b({ id: "", action: { type: "task", id: 0 } }))).toBe("KI-Task auswählen");
    expect(missingField(b({ id: "", action: { type: "hotkey", action: "" } }))).toBe("Aktion auswählen");
    expect(missingField(b({ id: "", app: "" }))).toBe("App auswählen");
    expect(missingField(b({ id: "", trigger: { kind: "swipe_up", fingers: 2 } }))).toBe("3–5 Finger");
    expect(missingField(newBinding())).toBeNull();
  });

  it("exempts tab switching from the typing guard by default", () => {
    expect(defaultTypingGuard({ type: "next_tab" })).toBe(false);
    expect(defaultTypingGuard({ type: "open", target: "x" })).toBe(true);
  });

  it("warns about system collisions for swipes and multi-finger taps", () => {
    expect(mayCollideWithSystem({ kind: "swipe_up", fingers: 3 })).toBe(true);
    expect(mayCollideWithSystem({ kind: "tap", fingers: 3 })).toBe(false);
    expect(mayCollideWithSystem({ kind: "tap", fingers: 4 })).toBe(true);
    expect(mayCollideWithSystem({ kind: "tip_tap_left", fingers: 2 })).toBe(false);
  });
});

describe("show the gesture", () => {
  it("takes the newest gesture after the start, skipping contact entries and older ones", () => {
    // Newest first, as the live snapshot delivers.
    const log = [entry(12, null, null), entry(11, "swipe_right", 4), entry(10, "tap", 3)];
    expect(captureFromLog(log, 10)).toEqual({ trigger: { kind: "swipe_right", fingers: 4 } });
    expect(captureFromLog(log, 11)).toBeNull();
    expect(latestSeq(log)).toBe(12);
    expect(latestSeq([])).toBe(0);
  });

  it("stores tip-taps with their fixed posture", () => {
    expect(captureFromLog([entry(2, "tip_tap_left", 3)], 1)).toEqual({ trigger: { kind: "tip_tap_left", fingers: 2 } });
  });

  it("explains a gesture that can't be bound instead of clamping it", () => {
    const two = captureFromLog([entry(2, "swipe_up", 2)], 1);
    expect(two && "error" in two && two.error).toMatch(/2 Finger/);
    const six = captureFromLog([entry(2, "tap", 6)], 1);
    expect(six && "error" in six && six.error).toMatch(/höchstens 5/);
  });
});
