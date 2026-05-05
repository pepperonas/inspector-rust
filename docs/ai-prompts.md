# Bundled AI prompt snippets

ClipSnap ships with **25 curated AI-prompt templates** that get seeded into your snippet table on first launch. Type the abbreviation in the search field (or use the text expander), press Enter, and the full structured prompt lands in your destination app — ready to hand to an LLM without further massaging.

Introduced in **v0.5.0**.

## Why bundled defaults?

Most LLM workflows live and die by prompt quality. Generic prompts get generic answers. The bundled templates are opinionated: each enforces sections, bullets, and output-format directives that have been shown to produce better-structured output across Claude / GPT / Gemini.

You can edit, delete, or extend them — they're regular snippets stored in the same SQLite table as your own. Nothing magical.

## Where they live

- **Bundled in the binary** via `include_str!` of [`core/rust-lib/src/seed/ai_prompts.json`](../core/rust-lib/src/seed/ai_prompts.json). No external file needed at runtime.
- **Seeded on first launch** by [`core/rust-lib/src/seed.rs`](../core/rust-lib/src/seed.rs)::`maybe_seed_defaults`. The `seed.default_snippets_v1` flag in the settings table tracks whether the seed has run; subsequent launches don't re-import (so user-deleted prompts stay deleted).
- **Restorable** via the Snippets-tab sidebar's **Restore defaults** button (rotate-counter-clockwise icon, next to Import). That re-imports all 25 with upsert-by-abbreviation semantics: your edits to a default get overwritten, but custom snippets with different abbreviations are untouched.

## The 25 prompts

### Programming / DevX

| Abbreviation | Title |
|---|---|
| `aiplan` | Software architecture / implementation plan |
| `aireview` | Senior code review (security + performance + maintainability) |
| `airefactor` | Refactor for clarity (behavior-preserving) |
| `airegex` | Regex generation + explanation |
| `aisql` | SQL query from natural language |
| `aitest` | Generate test cases (happy path + edges + failures) |
| `aimigration` | Database migration plan (safe + reversible) |
| `aibench` | Microbenchmark setup |

### Web / Frontend

| Abbreviation | Title |
|---|---|
| `aithumb` | Open Graph thumbnail design (1200×630) |
| `aimobile` | PWA mobile optimization audit |
| `aia11y` | Accessibility audit (WCAG 2.2 AA) |
| `aiseo` | SEO audit + meta-data optimization |
| `aicomponent` | Reusable UI component design |

### IT Security

| Abbreviation | Title |
|---|---|
| `aithreat` | STRIDE threat model |
| `aipentest` | Web pentest checklist (OWASP Top 10 mapped) |
| `aiauth` | Auth design (sessions / tokens / OAuth) |
| `aigdpr` | GDPR / privacy compliance review |

### Business workflows

| Abbreviation | Title |
|---|---|
| `aibrief` | Project brief (scope + success criteria) |
| `airfp` | RFP evaluation matrix |
| `aiokr` | OKRs for a team / quarter |
| `aichange` | Change management plan |

### Data analysis

| Abbreviation | Title |
|---|---|
| `aidataq` | Data quality audit |
| `aiml` | ML pipeline design |
| `aidashboard` | Dashboard design (KPIs + layout) |

### Architecture / API

| Abbreviation | Title |
|---|---|
| `aiapi` | REST API design |

## Using a prompt

Three ways to put a default prompt to work:

### 1. Search field activation
Open the popup (`Ctrl+Shift+V`), type `aiplan`, press Enter. The full prompt body lands in the previously focused app.

### 2. Snippet expander hotkey (Settings → Text expander)
With the expander enabled, type `aiplan` directly in any text field, press your configured hotkey (default `Alt+Backquote` on a German keyboard / `Alt+\`` on US), the abbreviation gets replaced in place with the full prompt.

### 3. Snippets-tab paste button
Open the popup → **Snippets** tab → click any prompt to select → click **Paste** in the detail pane.

## Editing the prompts

Open the **Snippets** tab → click any `ai*` prompt → edit Abbreviation / Title / Body in the right pane → **Save**.

Common customisations:
- Translate to your preferred language (we ship English because it's the highest-floor across LLMs)
- Add `[CONTEXT]` placeholders specific to your codebase / domain
- Tighten section requirements that don't apply to your work

If you wreck a prompt and want it back, click **Restore defaults** in the sidebar — it'll upsert all 25 to their bundled versions while leaving your custom snippets alone.

## Adding your own AI prompts

Same pattern as any other snippet:
- Click **+ New Snippet** in the Snippets tab
- Pick an abbreviation (the `ai*` prefix isn't reserved — use `mfg-en`, `meeting-notes`, whatever fits your workflow)
- Write the body — for AI prompts, structure it the way the bundled ones are: clear sections, bulleted requirements, an explicit output-format directive at the end

Or batch-import via **Import…** with a JSON file matching the snippet schema (see [`docs/snippets-import.md`](./snippets-import.md)).

## Versioning

The bundled set is versioned via the `seed.default_snippets_v1` settings flag. If a future release ships an updated prompt library, the flag suffix changes (`_v2`, etc.), forcing a re-seed for everyone. We'll only do that for genuinely substantial improvements — not for minor wording tweaks — because the re-seed will overwrite users' edits to default-keyed prompts.

If you're maintaining your own version of a default prompt, give it a different abbreviation (`aiplan-mine`, `aiplan-de`) so it survives any future re-seed.

## See also

- [`docs/snippets-import.md`](./snippets-import.md) — bulk JSON import format used by the seed
- [`docs/text-expander.md`](./text-expander.md) — how to trigger snippets from outside the popup
- [`docs/backup.md`](./backup.md) — how to back up / share your customised prompt library
