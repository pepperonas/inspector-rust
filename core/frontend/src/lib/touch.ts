// `touch <name> > <text>` — keeping line breaks.
//
// The search bar is a single-line <input>, and the browser's value
// sanitisation for `type="text"` STRIPS every newline: a pasted list
// "Pip\nBiscuit" arrived as "PipBiscuit", glued together in the file. So a
// multi-line paste into the content part of a `touch` command is intercepted
// and its line breaks are stored as a visible marker (`LINE_BREAK`), which
// `parseTouchArg` turns back into real newlines when the file is written.
// Pure + import-free so it's unit-testable without a DOM.

/** Visible stand-in for a newline inside the single-line search field. */
export const LINE_BREAK = "↵";

/** `touch <name> >` — caret is inside the content part. Case-insensitive. */
const TOUCH_CONTENT_PREFIX = /^\s*touch\s+[^>]*>/i;

export interface TouchArg {
  name: string;
  content: string;
}

/** Split `name > content`, restoring the line breaks. Multi-line content gets
 * a trailing newline (a POSIX text file ends with one); single-line content
 * is written exactly as typed, as before. */
export function parseTouchArg(arg: string): TouchArg {
  const gt = arg.indexOf(">");
  const name = (gt >= 0 ? arg.slice(0, gt) : arg).trim();
  const raw = gt >= 0 ? arg.slice(gt + 1) : "";
  // Exactly ONE space on each side of a marker is the separator the paste
  // inserted for readability; anything beyond it is content (indentation of
  // pasted code/YAML must survive). The ends of the whole text are trimmed
  // like a single-line `touch`.
  const parts = raw.split(LINE_BREAK);
  const lines = parts.map((l, i) => {
    let s = l;
    s = i === 0 ? s.trimStart() : s.replace(/^ /, "");
    s = i === parts.length - 1 ? s.trimEnd() : s.replace(/ $/, "");
    return s;
  });
  // Leading/trailing empty lines (a paste starting/ending with a break).
  while (lines.length > 1 && lines[0] === "") lines.shift();
  while (lines.length > 1 && lines[lines.length - 1] === "") lines.pop();
  const content = lines.length > 1 ? lines.join("\n") + "\n" : lines[0] ?? "";
  return { name, content };
}

/** Number of lines the file will have (0 for no content). */
export function touchLineCount(content: string): number {
  if (!content) return 0;
  return content.replace(/\n$/, "").split("\n").length;
}

/** If `text` is multi-line and the caret sits in the content part of a
 * `touch` command, return the field value with the paste inserted (newlines
 * → `LINE_BREAK`) and the new caret position. `null` = let the browser
 * handle the paste normally (every other query keeps its old behaviour). */
export function touchPaste(
  value: string,
  selStart: number,
  selEnd: number,
  text: string,
): { value: string; caret: number } | null {
  if (!/[\r\n]/.test(text)) return null;
  const before = value.slice(0, selStart);
  // The caret already sits after `touch name >` — or the paste itself brings
  // the command along (`touch x.txt > A\nB` pasted into an empty field).
  const firstLine = (before + text).split(/\r?\n/)[0];
  if (!TOUCH_CONTENT_PREFIX.test(firstLine)) return null;
  const marked = text
    .replace(/\r\n?/g, "\n")
    .replace(/^\n+|\n+$/g, "")
    .split("\n")
    .join(` ${LINE_BREAK} `);
  const next = before + marked + value.slice(selEnd);
  return { value: next, caret: before.length + marked.length };
}
