import { describe, it, expect } from "vitest";
import {
  NON_COMMAND_FEATURES,
  HIDDEN_TRIGGER_FEATURES,
  IN_POPUP_ACTIONS,
  HIDDEN_GAMES,
  HIDDEN_TRIGGERS,
} from "./feature-extras";

/**
 * The whole point of this data module: guarantee the in-app Features tab can't
 * silently drop a hidden trigger (the bug that left `equalizer` undocumented).
 */
describe("Features-tab completeness", () => {
  const documented = new Set(
    [...HIDDEN_TRIGGER_FEATURES, ...HIDDEN_GAMES]
      .map((r) => r.keyword)
      .filter((k): k is string => !!k),
  );

  it("documents every canonical hidden trigger by keyword", () => {
    for (const trigger of HIDDEN_TRIGGERS) {
      expect(documented, `hidden trigger "${trigger}" has no Features-tab row`).toContain(trigger);
    }
  });

  it("keeps equalizer covered (the trigger that once slipped through)", () => {
    expect(documented).toContain("equalizer");
    expect(documented).toContain("bpm");
  });

  it("has no keyword rows that aren't in the canonical list", () => {
    // A row carrying a keyword must be a known hidden trigger — otherwise the
    // canonical list is stale and the guard above is weaker than it looks.
    for (const k of documented) {
      expect(HIDDEN_TRIGGERS, `keyword "${k}" is not in HIDDEN_TRIGGERS`).toContain(k);
    }
  });
});

describe("Features-tab data integrity", () => {
  const all = [...NON_COMMAND_FEATURES, ...HIDDEN_TRIGGER_FEATURES, ...IN_POPUP_ACTIONS, ...HIDDEN_GAMES];

  it("every row has a non-empty name and trigger", () => {
    for (const r of all) {
      expect(r.name.trim()).not.toBe("");
      expect(r.trigger.trim()).not.toBe("");
    }
  });

  it("each group is non-empty", () => {
    expect(NON_COMMAND_FEATURES.length).toBeGreaterThan(0);
    expect(HIDDEN_TRIGGER_FEATURES.length).toBeGreaterThan(0);
    expect(IN_POPUP_ACTIONS.length).toBeGreaterThan(0);
    expect(HIDDEN_GAMES.length).toBeGreaterThan(0);
  });

  it("surfaces the recently-added in-popup actions", () => {
    const names = IN_POPUP_ACTIONS.map((r) => r.name);
    expect(names).toContain("Show only pinned clips");
    expect(names).toContain("Lineage rails");
    expect(names).toContain("Formatting options");
  });
});
