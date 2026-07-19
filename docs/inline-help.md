# Inline help (`?`) — reference

Every search-bar power command carries structured, in-app documentation. Append
**`?`** to a command and the preview pane renders its full help; type **`?`**
alone for a browsable index. Help is part of the flow — you never leave the
popup or open a man page.

## Triggering help

| You type | You get |
|----------|---------|
| `?` | The **browsable index**: the left list fills with every command — **↑/↓** browses, the right pane shows the selected command's **full doc live**, **Enter** puts the command into the search bar (with a trailing space when it takes an argument) |
| `? <text>` | The index **filtered by full-text search** over all docs — keyword/alias fuzzy first, then tagline/description/tips matches (`? clip`, `? netzwerk`) |
| `kill?` / `kill ?` | Full help for `kill` (a lone space before `?` is allowed) |
| `cal?` / `nmap?` | Help resolves through **aliases** (→ `calendar`, → `sec`) |
| `sni?` | A **prefix** resolves to the top autocomplete match (→ `snitch`) |
| `KILL?` | Case-insensitive |

In a command's doc view, the **examples are clickable** — a click puts the
example into the search bar, ready to run or adapt — and the **← Index** chip
jumps back to the browsable index. Search that matches nothing falls back to
the full grouped index with a "no match" note.

The trigger is intentionally narrow: it fires only for a single
command-shaped token (lowercase alnum) plus at most one space before a lone
trailing `?`. That is what keeps a **literal** `?` from triggering help:

| You type | Result | Why |
|----------|--------|-----|
| `faker tpl "warum? {name}"` | no help | `?` is inside a template argument |
| `a?b` · `file?.txt` | no help | glob/regex — doesn't end in a bare `?` |
| `https://x.com/?id=1` · `…/?` | no help | not a command-shaped token |
| `bruno hallo?` · `faker tpl?` | no help | there's an argument before the `?` |

Parsing lives in the pure, unit-tested `core/frontend/src/lib/commandHelp.ts`
(`parseHelpQuery`); the collision cases above are pinned by
`commandHelp.test.ts`.

## What a doc contains

Each command's help shows: a one-line tagline, the **synopsis** (grammar),
a description, every **argument** and **flag** with a plain-English
explanation and default, **≥3 worked examples**, **tips**, **caveats**,
the **alias** list, clickable **related-command** chips, the version it
landed in, and — where deeper docs exist — a pointer into `docs/`.

Navigation stays in the search flow: clicking a command in the index, or a
related-command chip, rewrites the query to `<keyword>?`; deleting the `?`
runs the command; `Esc` closes the popup.

## Single source of truth

All of this comes from one declarative registry:

```
core/frontend/src/lib/commandDocs.ts   →  COMMAND_DOCS: CommandDoc[]
```

That registry is canonical and feeds three surfaces, which therefore can
never drift:

1. **Inline `?` help** — rendered by `components/CommandHelp.tsx`.
2. **The README command matrix** — `README.md` (English `tagline`) and
   `README.de.md` (`tagline_de ?? tagline`), between the
   `<!-- COMMANDS:START -->` / `<!-- COMMANDS:END -->` markers, written by
   `scripts/gen-docs.mjs`. **Never hand-edit that block.**
3. **The Features tab** — `components/FeaturesPanel.tsx` builds its command
   rows from `COMMAND_DOCS` at runtime.

### Adding or changing a command's help

1. Edit the command's entry in `commandDocs.ts` (or add one — a new
   command **must** have a doc; the completeness test fails by name
   otherwise).
2. Run `node scripts/gen-docs.mjs` to refresh the README blocks, and commit
   the result. `bash scripts/check.sh` runs `gen-docs --check`, which fails on
   drift or a missing `see_also` file.

### Gates

- `commandDocs.test.ts` — every non-hidden `COMMANDS` keyword resolves to a
  doc; each doc has a non-empty synopsis + description, **≥3 examples**, **≥1
  tip or caveat**, documented args/flags, real related-command links, and no
  orphan/duplicate keywords.
- `commandHelp.test.ts` — the `?`-trigger grammar and every literal-`?`
  collision case.
- `gen-docs --check` (in `scripts/check.sh`) — the README matrix is in sync and
  every `see_also` target exists.
