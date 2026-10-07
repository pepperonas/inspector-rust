import { describe, it, expect } from "vitest";
import { LINE_BREAK, isUnixTouch, parseFileWrite, parseTouchArg, touchLineCount, touchPaste } from "./touch";

const LIST = "Mister Midge\nLittle Buddy\nMister Fluffy\nTiny Tim\nBumble\nPip";

/** What the field holds after pasting `text` with the caret at the end. */
const pasted = (value: string, text: string) => {
  const r = touchPaste(value, value.length, value.length, text);
  if (!r) throw new Error("paste not intercepted");
  return r;
};
const argOf = (query: string) => query.replace(/^\s*touch\s+/i, "");

describe("touchPaste", () => {
  it("keeps a pasted list's line breaks as visible markers", () => {
    const r = pasted("touch namen.txt > ", LIST);
    expect(r.value).toBe(`touch namen.txt > Mister Midge ${LINE_BREAK} Little Buddy ${LINE_BREAK} Mister Fluffy ${LINE_BREAK} Tiny Tim ${LINE_BREAK} Bumble ${LINE_BREAK} Pip`);
    expect(r.caret).toBe(r.value.length);
  });

  it("round-trips to a file with one name per line", () => {
    const r = pasted("touch namen.txt > ", LIST);
    expect(parseTouchArg(argOf(r.value))).toEqual({ name: "namen.txt", content: LIST + "\n" });
  });

  it("intercepts the whole command pasted into an empty field", () => {
    const r = pasted("", "touch a.txt > Pip\nBiscuit");
    expect(parseTouchArg(argOf(r.value)).content).toBe("Pip\nBiscuit\n");
  });

  it("normalises CRLF and drops leading/trailing blank lines", () => {
    const r = pasted("touch a.txt > ", "\r\nPip\r\nBiscuit\r\n\r\n");
    expect(parseTouchArg(argOf(r.value)).content).toBe("Pip\nBiscuit\n");
  });

  it("keeps indentation and inner blank lines", () => {
    const text = "a:\n  b: 1\n\n  c: 2";
    const r = pasted("touch x.yaml > ", text);
    expect(parseTouchArg(argOf(r.value)).content).toBe(text + "\n");
  });

  it("replaces a selection and leaves text after it", () => {
    const v = "touch a.txt > XXX tail";
    const s = v.indexOf("XXX");
    const r = touchPaste(v, s, s + 3, "A\nB")!;
    expect(r.value).toBe(`touch a.txt > A ${LINE_BREAK} B tail`);
    expect(r.caret).toBe(s + `A ${LINE_BREAK} B`.length);
  });

  it("leaves every other paste to the browser", () => {
    expect(touchPaste("touch a.txt > ", 14, 14, "one line")).toBeNull();
    expect(touchPaste("touch a.txt", 11, 11, "A\nB")).toBeNull(); // name part, no `>`
    expect(touchPaste("", 0, 0, LIST)).toBeNull(); // a plain search
    expect(touchPaste("mkdir x > ", 10, 10, "A\nB")).toBeNull();
  });
});

describe("parseTouchArg", () => {
  it("single-line content stays exactly as before (no trailing newline)", () => {
    expect(parseTouchArg("hallo.txt > das ist ein test")).toEqual({ name: "hallo.txt", content: "das ist ein test" });
    expect(parseTouchArg("notes.md")).toEqual({ name: "notes.md", content: "" });
  });

  it("counts lines", () => {
    expect(touchLineCount("")).toBe(0);
    expect(touchLineCount("x")).toBe(1);
    expect(touchLineCount(LIST + "\n")).toBe(6);
  });
});

describe("parseFileWrite (Unix form)", () => {
  const MD = "# Titel\n\n> ein Zitat mit \"Anführungszeichen\"\n\n    eingerückter Code\n- Liste  mit  Leerzeichen";

  it("echo and touch take the text first and the file after `>`", () => {
    expect(parseFileWrite("echo", '"hallo welt" > datei.md')).toEqual({ name: "datei.md", content: "hallo welt\n", append: false });
    expect(parseFileWrite("touch", '"hallo welt" > datei.md')).toEqual({ name: "datei.md", content: "hallo welt\n", append: false });
    expect(parseFileWrite("echo", "'single quotes' > a.txt")?.content).toBe("single quotes\n");
  });

  it("keeps every space inside the quotes", () => {
    expect(parseFileWrite("echo", '"  zwei  Leerzeichen\tTab  " > a.txt')?.content).toBe("  zwei  Leerzeichen\tTab  \n");
  });

  it("a pasted Markdown text survives exactly — quotes and > inside it included", () => {
    const r = pasted('echo "', MD);
    const arg = (r.value + '" > notiz.md').replace(/^\s*echo\s+/i, "");
    expect(parseFileWrite("echo", arg)).toEqual({ name: "notiz.md", content: MD + "\n", append: false });
    const t = pasted('touch "', MD);
    expect(parseFileWrite("touch", argOf(t.value + '" > notiz.md'))?.content).toBe(MD + "\n");
  });

  it("the whole command pasted at once works too", () => {
    const r = pasted("", `echo "${MD}" > notiz.md`);
    expect(parseFileWrite("echo", r.value.replace(/^\s*echo\s+/i, ""))?.content).toBe(MD + "\n");
  });

  it("an inner quote right before > (HTML) does not end the text", () => {
    const html = '<a href="https://x.de">Link</a> > weiter';
    expect(parseFileWrite("echo", `"${html}" > seite.html`)).toEqual({ name: "seite.html", content: html + "\n", append: false });
  });

  it(">> appends", () => {
    expect(parseFileWrite("echo", '"noch eine Zeile" >> log.txt')).toEqual({ name: "log.txt", content: "noch eine Zeile\n", append: true });
  });

  it('\\" is a quote, other backslashes stay', () => {
    expect(parseFileWrite("echo", '"sag \\"hi\\" C:\\\\pfad\\n" > a.txt')?.content).toBe('sag "hi" C:\\\\pfad\\n\n');
  });

  it("quoted file names and nested paths", () => {
    expect(parseFileWrite("echo", '"x" > "meine notiz.md"')?.name).toBe("meine notiz.md");
    expect(parseFileWrite("echo", '"x" > docs/a.md')?.name).toBe("docs/a.md");
  });

  it("echo also takes unquoted text", () => {
    expect(parseFileWrite("echo", "hallo welt > a.txt")).toEqual({ name: "a.txt", content: "hallo welt\n", append: false });
    expect(parseFileWrite("echo", "eins >> a.txt")?.append).toBe(true);
  });

  it("incomplete input is not a write", () => {
    expect(parseFileWrite("echo", '"nur text"')).toBeNull();
    expect(parseFileWrite("echo", '"text" > ')).toBeNull();
    expect(parseFileWrite("echo", "ohne pfeil")).toBeNull();
    // touch without quotes is the legacy form, never the Unix one.
    expect(parseFileWrite("touch", "notes.md > hallo")).toBeNull();
    expect(isUnixTouch("notes.md > hallo")).toBe(false);
    expect(isUnixTouch('"hallo" > notes.md')).toBe(true);
  });

  it("a paste into the file name of a plain touch is still left alone", () => {
    expect(touchPaste("touch a.txt", 11, 11, "A\nB")).toBeNull();
    expect(touchPaste("", 0, 0, LIST)).toBeNull();
  });
});
