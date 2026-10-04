import panelSrc from "../components/SettingsPanel.tsx?raw";
import { describe, expect, it } from "vitest";
import { SETTINGS_CATEGORIES, SETTINGS_SECTIONS, categoryOf, matchSettingsSection, suggestSettingsSections } from "./settings-sections";
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

describe("suggestSettingsSections — main-search suggestions (v0.164.0)", () => {
  it("stays silent below the minimum length", () => {
    // Two typed characters match half the registry by prefix — the clip
    // search is the primary result set and must not be crowded that early.
    expect(suggestSettingsSections("th")).toEqual([]);
    expect(suggestSettingsSections("  a ")).toEqual([]);
    expect(suggestSettingsSections("")).toEqual([]);
  });

  it("hits on exact names, name prefixes and word starts", () => {
    expect(suggestSettingsSections("theme")[0]?.id).toBe("appearance");
    expect(suggestSettingsSections("hotk")[0]?.id).toBe("popup-hotkey");
    expect(suggestSettingsSections("short").some((s) => s.id === "global-shortcuts")).toBe(true);
    // ⚠️ "entries" exists ONLY as the second word of "max entries" — unlike
    // "short(cuts)", which is also a name prefix, this pin actually dies
    // when the word-start branch is removed (first probe was green-blind).
    expect(suggestSettingsSections("entri").some((s) => s.id === "clipboard-history")).toBe(true);
  });

  it("NEVER matches by fuzzy subsequence — that is the command's job", () => {
    // `settings hty` may fuzzy-resolve; the main list must not ("hty" would
    // surface settings rows while someone types ordinary clip queries).
    expect(suggestSettingsSections("hty")).toEqual([]);
    expect(suggestSettingsSections("xqz")).toEqual([]);
  });

  it("caps at two rows and ranks exact over prefix", () => {
    // "animation" is an exact synonym of Appearance AND a prefix elsewhere —
    // the exact hit must come first, and never more than `limit` rows.
    const hits = suggestSettingsSections("animation");
    expect(hits.length).toBeLessThanOrEqual(2);
    expect(hits[0]?.id).toBe("appearance");
    for (const s of SETTINGS_SECTIONS) {
      expect(suggestSettingsSections(s.names[0]).length).toBeLessThanOrEqual(2);
    }
  });

  it("finds the German animation stage wording", () => {
    expect(suggestSettingsSections("animationen")[0]?.id).toBe("appearance");
    expect(suggestSettingsSections("bewegung")[0]?.id).toBe("appearance");
  });

  it("each section resolves once, never duplicated across its synonyms", () => {
    const hits = suggestSettingsSections("clipboard", 5);
    const ids = hits.map((h) => h.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});

describe("Repositories section (v0.183.0)", () => {
  it("is found by German and English names", () => {
    expect(matchSettingsSection("klonen")?.id).toBe("repos");
    expect(matchSettingsSection("repositories")?.id).toBe("repos");
    expect(matchSettingsSection("github")?.id).toBe("repos");
  });
});

describe("SETTINGS_CATEGORIES", () => {
  it("files every registry section exactly once, and nothing else", () => {
    const filed = SETTINGS_CATEGORIES.flatMap((c) => [...c.sections]);
    expect(new Set(filed).size).toBe(filed.length);
    expect([...filed].sort()).toEqual(SETTINGS_SECTIONS.map((s) => s.id).sort());
  });

  it("has unique ids and no empty category", () => {
    const ids = SETTINGS_CATEGORIES.map((c) => c.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const c of SETTINGS_CATEGORIES) expect(c.sections.length).toBeGreaterThan(0);
  });

  it("puts the basics first and About last", () => {
    expect(SETTINGS_CATEGORIES[0].sections[0]).toBe("behavior");
    expect(SETTINGS_CATEGORIES[SETTINGS_CATEGORIES.length - 1].sections).toEqual(["about"]);
  });

  it("names the category of a section", () => {
    expect(categoryOf("cue")).toBeNull();
    expect(categoryOf("cloud-sync")?.id).toBe("sync");
    expect(categoryOf("gestures")?.label).toBe("Trackpad & Fenster");
  });

  it("the panel renders a block for every section the categories list", () => {
    const panel = panelSrc;
    for (const id of SETTINGS_CATEGORIES.flatMap((c) => [...c.sections])) {
      expect(panel, id).toContain(`"${id}":`);
    }
  });
  // Layout facts only a browser shows — pinned so they can't silently regress.
  it("the category bar stays readable and flush with the header", () => {
    const nav = panelSrc.match(/aria-label="Einstellungs-Kategorien"\s+className="([^"]+)"/)?.[1] ?? "";
    // A flex-column scroller shrinks an overflow-x child to a sliver.
    expect(nav.split(" ")).toContain("shrink-0");
    // Sticky offsets count from inside the scroller's 24 px padding.
    expect(nav.split(" ")).toContain("-top-6");
  });

  it("at the bottom end the last chip is the active one", () => {
    expect(panelSrc).toMatch(/scrollHeight - 4\) current = NAV_CATEGORIES\[NAV_CATEGORIES\.length - 1\]\.id/);
  });

  it("a deep link keeps its target pinned while the sections load", () => {
    expect(panelSrc).toMatch(/new ResizeObserver\(\(\) => \{\s*if \(pinning\) pin\(\);/);
  });
});
