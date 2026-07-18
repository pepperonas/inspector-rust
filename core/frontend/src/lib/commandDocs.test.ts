import { describe, it, expect } from "vitest";
import { COMMANDS, type CommandSpec } from "./commands";
import { COMMAND_DOCS, lookupDoc, groupedIndex } from "./commandDocs";

// The whole point of the inline-help system: EVERY user-facing power command
// must be fully documented. These are coverage GATES, not behaviour tests —
// a new command without a doc (or a doc missing examples/tips) fails here by
// name, which is exactly the signal "document it before shipping".

/** Non-hidden commands are the ones that surface in autocomplete — those are
 *  the ones a user can discover and therefore must be able to `?`. */
const visible: CommandSpec[] = COMMANDS.filter((c) => !c.hidden);

/** Every keyword in the catalogue (hidden aliases included) — used to prove
 *  no doc documents a command/alias that doesn't exist. */
const allKeywords = new Set(COMMANDS.map((c) => c.keyword.toLowerCase()));

describe("commandDocs — coverage", () => {
  it("every visible command has a doc reachable by keyword", () => {
    const missing = visible.filter((c) => !lookupDoc(c.keyword)).map((c) => c.keyword);
    expect(missing, `commands without an inline-help doc: ${missing.join(", ")}`).toEqual([]);
  });

  it("every doc's command + aliases are real catalogue keywords (no orphans)", () => {
    for (const doc of COMMAND_DOCS) {
      const names = [doc.command, ...doc.aliases];
      for (const n of names) {
        expect(
          allKeywords.has(n.toLowerCase()),
          `doc "${doc.command}" references keyword "${n}" which is not in COMMANDS`,
        ).toBe(true);
      }
    }
  });

  it("no two docs claim the same command/alias keyword", () => {
    const seen = new Map<string, string>();
    for (const doc of COMMAND_DOCS) {
      for (const n of [doc.command, ...doc.aliases]) {
        const key = n.toLowerCase();
        const prev = seen.get(key);
        expect(prev, `keyword "${n}" is claimed by both "${prev}" and "${doc.command}"`).toBeUndefined();
        seen.set(key, doc.command);
      }
    }
  });
});

describe("commandDocs — quality of each doc", () => {
  for (const doc of COMMAND_DOCS) {
    describe(doc.command, () => {
      it("has a non-empty tagline, synopsis and description", () => {
        expect(doc.tagline.trim().length).toBeGreaterThan(0);
        expect(doc.synopsis.trim().length).toBeGreaterThan(0);
        expect(doc.description.trim().length).toBeGreaterThan(20);
      });

      it("has a category and a version_added", () => {
        expect(doc.category.trim().length).toBeGreaterThan(0);
        expect(doc.version_added).toMatch(/^\d+\.\d+\.\d+$/);
      });

      it("has at least 3 examples, each with a non-empty input + result", () => {
        expect(doc.examples.length).toBeGreaterThanOrEqual(3);
        for (const ex of doc.examples) {
          expect(ex.input.trim().length, `empty example input in "${doc.command}"`).toBeGreaterThan(0);
          expect(ex.result.trim().length, `empty example result in "${doc.command}"`).toBeGreaterThan(0);
        }
      });

      it("has at least one tip or caveat", () => {
        expect(doc.tips.length + doc.caveats.length).toBeGreaterThanOrEqual(1);
      });

      it("documents every argument and flag with a description", () => {
        for (const a of doc.arguments) {
          expect(a.name.trim().length).toBeGreaterThan(0);
          expect(a.description.trim().length, `arg "${a.name}" of "${doc.command}" lacks a description`).toBeGreaterThan(0);
        }
        for (const f of doc.flags) {
          expect(f.flag.trim().length).toBeGreaterThan(0);
          expect(f.description.trim().length, `flag "${f.flag}" of "${doc.command}" lacks a description`).toBeGreaterThan(0);
        }
      });

      it("only relates to real documented commands", () => {
        for (const r of doc.related) {
          expect(
            lookupDoc(r),
            `"${doc.command}" relates to "${r}" which has no doc`,
          ).toBeDefined();
        }
      });

      if (doc.see_also) {
        it("has a well-formed repo-root-relative docs/*.md see_also", () => {
          // The file's actual existence is enforced by `gen-docs --check`
          // (a Node script that can hit the filesystem); here we keep the
          // gate node-free and just pin the shape so a typo is caught.
          expect(doc.see_also).toMatch(/^docs\/[\w-]+\.md$/);
        });
      }
    });
  }
});

describe("commandDocs — lookup + index", () => {
  it("looks up by primary name and by alias, case-insensitively", () => {
    expect(lookupDoc("faker")?.command).toBe("faker");
    expect(lookupDoc("FAKE")?.command).toBe("faker"); // hidden alias
    expect(lookupDoc("nmap")?.command).toBe("sec"); // tool-keyword alias
    expect(lookupDoc("caffeine")?.command).toBe("wakelock");
    expect(lookupDoc("shotfull")?.command).toBe("shot");
    expect(lookupDoc("nope-not-a-command")).toBeUndefined();
  });

  it("groups every doc into exactly one category bucket", () => {
    const groups = groupedIndex();
    const total = groups.reduce((n, g) => n + g.docs.length, 0);
    expect(total).toBe(COMMAND_DOCS.length);
    // Category names are unique across buckets.
    const cats = groups.map((g) => g.category);
    expect(new Set(cats).size).toBe(cats.length);
  });
});
