import { describe, expect, it } from "vitest";

/**
 * A panel that decides "is the user typing?" by looking at the tag name alone
 * loses every key, because the search field keeps the focus while the panel
 * is open. The decision belongs to `lib/panel-keys`.
 */
const sources = import.meta.glob("../components/*Panel.tsx", { query: "?raw", import: "default", eager: true }) as Record<
  string,
  string
>;

describe("panel key guards", () => {
  it("finds the panels", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(30);
  });

  it("no panel hand-rolls the INPUT check in a window key handler", () => {
    const offenders: string[] = [];
    for (const [file, src] of Object.entries(sources)) {
      if (!/addEventListener\("keydown"/.test(src)) continue;
      // StatsPanel reads the search-field marker itself (the model for the rule).
      if (file.endsWith("/StatsPanel.tsx")) continue;
      const guard = /tagName === "INPUT"/.test(src);
      if (guard && !/isTyping\(/.test(src)) offenders.push(file);
    }
    expect(offenders).toEqual([]);
  });
});
