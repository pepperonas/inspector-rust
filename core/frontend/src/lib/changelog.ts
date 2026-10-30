/**
 * The changelog shown in Settings → Changelog: `CHANGELOG.md` from the repo
 * root, parsed into releases and blocks. Pure, no DOM.
 *
 * The file is Keep-a-Changelog shaped but grew by hand over 600 releases, so
 * the parser takes what is actually in it: `## [x.y.z] — date` headings (dash,
 * en dash or em dash), `###`/`####` sub-headings, bullets with nested levels
 * and wrapped continuation lines, numbered items, paragraphs, fenced code and
 * a few pipe tables. Links are reduced to their label: the settings page is
 * a place to read, not to open URLs from.
 */

export type ChangelogBlock =
  | { kind: "heading"; text: string }
  | { kind: "item"; text: string; depth: number; ordered: boolean }
  | { kind: "para"; text: string }
  | { kind: "code"; text: string }
  | { kind: "table"; rows: string[][] };

export interface ChangelogRelease {
  version: string;
  /** `YYYY-MM-DD`, or `null` when the heading has none (Unreleased). */
  date: string | null;
  blocks: ChangelogBlock[];
}

const RELEASE = /^##\s+\[([^\]]+)\]\s*(?:[-–—]\s*(\d{4}-\d{2}-\d{2}))?/;
const BULLET = /^(\s*)[-*]\s+(.*)$/;
const ORDERED = /^(\s*)\d+[.)]\s+(.*)$/;
const LINK = /\[([^\]]+)\]\((?:[^)\s]+)\)/g;

/** `[label](url)` → `label`. */
export function stripLinks(text: string): string {
  return text.replace(LINK, "$1");
}

function depthOf(indent: string): number {
  return Math.floor(indent.replace(/\t/g, "  ").length / 2);
}

function isTableRow(line: string): boolean {
  return line.trim().startsWith("|");
}

function tableCells(line: string): string[] {
  const inner = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  return inner.split("|").map((c) => stripLinks(c.trim()));
}

function isTableRule(cells: string[]): boolean {
  return cells.length > 0 && cells.every((c) => /^:?-+:?$/.test(c));
}

/** Parse the whole file. Text before the first release heading is dropped. */
export function parseChangelog(md: string): ChangelogRelease[] {
  const lines = md.replace(/\r\n?/g, "\n").split("\n");
  const releases: ChangelogRelease[] = [];
  let cur: ChangelogRelease | null = null;
  let i = 0;

  const push = (b: ChangelogBlock) => cur?.blocks.push(b);

  while (i < lines.length) {
    const line = lines[i];
    const rel = RELEASE.exec(line);
    if (rel) {
      cur = { version: rel[1].trim(), date: rel[2] ?? null, blocks: [] };
      releases.push(cur);
      i++;
      continue;
    }
    if (!cur) {
      i++;
      continue;
    }
    if (line.trim() === "") {
      i++;
      continue;
    }
    if (line.trimStart().startsWith("```")) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trimStart().startsWith("```")) {
        body.push(lines[i]);
        i++;
      }
      i++; // closing fence (or end of file)
      push({ kind: "code", text: body.join("\n") });
      continue;
    }
    const heading = /^#{3,6}\s+(.*)$/.exec(line);
    if (heading) {
      push({ kind: "heading", text: stripLinks(heading[1].trim()) });
      i++;
      continue;
    }
    if (isTableRow(line)) {
      const rows: string[][] = [];
      while (i < lines.length && isTableRow(lines[i])) {
        const cells = tableCells(lines[i]);
        if (!isTableRule(cells)) rows.push(cells);
        i++;
      }
      push({ kind: "table", rows });
      continue;
    }
    const bullet = BULLET.exec(line);
    const ordered = bullet ? null : ORDERED.exec(line);
    const item = bullet ?? ordered;
    if (item) {
      const depth = depthOf(item[1]);
      const parts = [item[2].trim()];
      i++;
      // Wrapped continuation: indented, not a new item / heading / fence / table.
      while (i < lines.length) {
        const next = lines[i];
        if (next.trim() === "" || !/^\s+\S/.test(next)) break;
        if (BULLET.test(next) || ORDERED.test(next) || isTableRow(next)) break;
        if (next.trimStart().startsWith("```")) break;
        parts.push(next.trim());
        i++;
      }
      push({ kind: "item", text: stripLinks(parts.join(" ")), depth, ordered: !bullet });
      continue;
    }
    const parts = [line.trim()];
    i++;
    while (i < lines.length) {
      const next = lines[i];
      if (next.trim() === "" || RELEASE.test(next) || /^#{3,6}\s/.test(next)) break;
      if (BULLET.test(next) || ORDERED.test(next) || isTableRow(next)) break;
      if (next.trimStart().startsWith("```")) break;
      parts.push(next.trim());
      i++;
    }
    push({ kind: "para", text: stripLinks(parts.join(" ")) });
  }
  // An empty "Unreleased" placeholder says nothing.
  return releases.filter((r) => r.blocks.length > 0);
}

/** Releases shown on open, and how many each "older" click adds. */
export const CHANGELOG_FIRST = 10;
export const CHANGELOG_MORE = 25;

/** Case-insensitive search over version, date and every block's text. */
export function filterReleases(releases: ChangelogRelease[], query: string): ChangelogRelease[] {
  const q = query.trim().toLowerCase();
  if (!q) return releases;
  return releases.filter((r) => {
    if (r.version.toLowerCase().includes(q) || (r.date ?? "").includes(q)) return true;
    return r.blocks.some((b) => {
      if (b.kind === "table") return b.rows.some((row) => row.join(" ").toLowerCase().includes(q));
      return b.text.toLowerCase().includes(q);
    });
  });
}
