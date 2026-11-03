import type { RecordProjectItem } from "./ipc";

/**
 * The record project list's pure rules (shared by the `record` panel and the editor's ⌘O
 * dialog): what time a project sorts and shows by, and the search over name and date.
 */

/** The last edit, else the creation (Rust `ProjectSummary::last_change`). */
export function lastChange(p: Pick<RecordProjectItem, "createdAt" | "updatedAt">): number {
  return p.updatedAt && p.updatedAt > 0 ? p.updatedAt : p.createdAt;
}

/** Case- and accent-insensitive form: "Überblick" and "uberblick" meet. */
export function foldText(s: string): string {
  return s.toLowerCase().normalize("NFD").replace(/\p{Diacritic}/gu, "").replace(/ß/g, "ss");
}

function isoDay(ms: number): string {
  const d = new Date(ms);
  const two = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())}`;
}

/** Everything a search may hit: the title, and each date as ISO day and in the locale. */
export function searchText(p: RecordProjectItem, loc: string): string {
  const parts = [p.title];
  for (const ms of new Set([p.createdAt, lastChange(p)])) {
    if (ms > 0) {
      parts.push(isoDay(ms), new Date(ms).toLocaleDateString(loc, { day: "numeric", month: "long", year: "numeric" }));
      parts.push(new Date(ms).toLocaleDateString(loc));
    }
  }
  return foldText(parts.join(" "));
}

/** Projects matching every whitespace-separated term (order kept). Empty query = all. */
export function filterProjects(items: RecordProjectItem[], query: string, loc: string): RecordProjectItem[] {
  const terms = foldText(query).split(/\s+/).filter(Boolean);
  if (terms.length === 0) return items;
  return items.filter((p) => {
    const hay = searchText(p, loc);
    return terms.every((t) => hay.includes(t));
  });
}

/**
 * Is `p` the project open in the editor? Rust marks it on canonical paths (`openInEditor`),
 * which also holds when the recordings folder sits behind a symlink; the plain path compare
 * is the fallback for a list without the mark.
 */
export function isOpenProject(p: RecordProjectItem, currentDir: string): boolean {
  const trim = (s: string) => s.replace(/\/+$/, "");
  return p.openInEditor === true || trim(p.dir) === trim(currentDir);
}
