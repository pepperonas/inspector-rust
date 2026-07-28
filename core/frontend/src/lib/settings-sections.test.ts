import { describe, expect, it } from "vitest";
import { SETTINGS_SECTIONS, matchSettingsSection } from "./settings-sections";
import { parseCommand } from "./commands";

describe("settings command parsing", () => {
  it("`settings` and the hidden `config` alias parse as the settings kind", () => {
    expect(parseCommand("settings")?.spec.kind).toBe("settings");
    expect(parseCommand("settings cue")?.spec.kind).toBe("settings");
    expect(parseCommand("config")?.spec.kind).toBe("settings");
  });
  it("does not fire mid-history-search", () => {
    expect(parseCommand("my settings notes")).toBeNull();
    expect(parseCommand("settingsx")).toBeNull();
  });
});

describe("SETTINGS_SECTIONS registry", () => {
  it("ids are unique, kebab-case, and every entry has names", () => {
    const ids = SETTINGS_SECTIONS.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const s of SETTINGS_SECTIONS) {
      expect(s.id).toMatch(/^[a-z0-9-]+$/);
      expect(s.names.length).toBeGreaterThan(0);
      for (const n of s.names) expect(n).toBe(n.toLowerCase());
    }
  });
});

describe("matchSettingsSection", () => {
  it("empty / whitespace arg → null (open at the top)", () => {
    expect(matchSettingsSection("")).toBeNull();
    expect(matchSettingsSection("   ")).toBeNull();
  });

  it("exact names resolve", () => {
    expect(matchSettingsSection("overlay")?.id).toBe("behavior");
    expect(matchSettingsSection("cue")?.id).toBe("cloud-sync");
    expect(matchSettingsSection("bruno")?.id).toBe("bruno");
    expect(matchSettingsSection("backup")?.id).toBe("backup");
  });

  it("German synonyms resolve", () => {
    expect(matchSettingsSection("zeiterfassung")?.id).toBe("timesheet");
    expect(matchSettingsSection("gesten")?.id).toBe("gestures");
    expect(matchSettingsSection("aufräumen")?.id).toBe("cleaning");
  });

  it("routes snippets + storage synonyms to the Snippets section", () => {
    expect(matchSettingsSection("snippets")?.id).toBe("snippets");
    expect(matchSettingsSection("storage")?.id).toBe("snippets");
    expect(matchSettingsSection("speicher")?.id).toBe("snippets");
  });

  it("prefixes and fuzzy subsequences resolve", () => {
    expect(matchSettingsSection("sy")?.id).toBe("cloud-sync"); // prefix of "sync"
    expect(matchSettingsSection("brn")?.id).toBe("bruno"); // subsequence
    expect(matchSettingsSection("gest")?.id).toBe("gestures");
  });

  it("is case-insensitive and trims", () => {
    expect(matchSettingsSection("  CUE  ")?.id).toBe("cloud-sync");
  });

  it("garbage → null", () => {
    expect(matchSettingsSection("qqqqxyz")).toBeNull();
  });

  it("every registry name resolves to its own section (no shadowing)", () => {
    for (const s of SETTINGS_SECTIONS) {
      // The FIRST name is the section's canonical term — it must never be
      // beaten by another section's synonym.
      expect(matchSettingsSection(s.names[0])?.id).toBe(s.id);
    }
  });
});
