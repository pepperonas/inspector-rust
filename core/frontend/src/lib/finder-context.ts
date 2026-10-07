/**
 * Display logic for the footer's working-folder chip: which Finder folder
 * touch/echo/mkdir/terminal/daisy act on, and what is selected. Pure.
 */
import type { FinderContext } from "./ipc";

/** `/Users/<name>/x` → `~/x` (macOS), `C:\Users\<name>\x` → `~\x` (Windows). */
export function tildePath(path: string): string {
  const p = path.length > 1 ? path.replace(/[\\/]+$/, "") : path;
  return p
    .replace(/^\/Users\/[^/]+(?=\/|$)/, "~")
    .replace(/^[A-Za-z]:\\Users\\[^\\]+(?=\\|$)/i, "~");
}

/** The last two folders, so a deep path stays short: `…/claude/docs`. */
export function shortDir(path: string): string {
  const t = tildePath(path);
  const sep = t.includes("\\") && !t.includes("/") ? "\\" : "/";
  const parts = t.split(sep).filter(Boolean);
  if (parts.length <= 2) return t || sep;
  return `…${sep}${parts.slice(-2).join(sep)}`;
}

function baseName(path: string): string {
  const p = path.replace(/[\\/]+$/, "");
  return p.slice(Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\")) + 1);
}

/** "notiz.md" for one item, "3 ausgewählt" for several, "" for none. A
 *  selected FOLDER is the working folder itself — naming it again would
 *  repeat the folder, so a lone selected folder yields "". */
export function selectionLabel(ctx: FinderContext): string {
  const n = ctx.selected_count;
  if (n === 0) return "";
  if (n > 1) return `${n} ausgewählt`;
  const only = ctx.selected[0];
  if (!only) return "1 ausgewählt";
  const trimmed = only.replace(/[\\/]+$/, "");
  if (trimmed === ctx.dir.replace(/[\\/]+$/, "")) return "";
  return baseName(only);
}

/** Tooltip: the full folder and what is selected. */
export function contextTitle(ctx: FinderContext): string {
  const lines = [`Arbeitsordner für touch, echo, mkdir, terminal, daisy:`, tildePath(ctx.dir)];
  if (ctx.selected_count > 0) {
    lines.push("", `Ausgewählt (${ctx.selected_count}):`);
    for (const s of ctx.selected.slice(0, 5)) lines.push(`• ${baseName(s)}`);
    if (ctx.selected_count > 5) lines.push(`• … und ${ctx.selected_count - 5} weitere`);
  } else {
    lines.push("", "Nichts ausgewählt — vorderstes Finder-Fenster.");
  }
  lines.push("", "Klick: im Finder zeigen");
  return lines.join("\n");
}
