# Snippet import / export (JSON)

Inspector Rust can bulk-import and export snippets as JSON — to seed the app with your existing templates, share libraries between machines, or **edit your snippets in another app and bring them back without losing their grouping**.

## How to import

Either way works; both accept every shape documented below and import **only** snippets + their groups.

- **Drag & drop** (v0.84.263): drop a `.json` file anywhere on the **Snippets** tab. A "Drop to import" overlay appears while you drag; the result line shows up when it lands. (The popup stays open for the duration of the drag — dragging in from Finder would otherwise steal focus and dismiss it.)
- **File picker:** Snippets tab → the **⬆** button in the toolbar → select a `.json` file (NSOpenPanel on macOS, OpenFileDialog on Windows).

Result is shown as a one-line status:

```
Imported 5
Imported 4, skipped 1 — #2 (mfg): body is empty
```

The full list refreshes automatically.

## File format

Three top-level shapes are accepted. The **backup envelope** is the only one that carries groups — it's what the export button writes and what an external editor should round-trip through. The other two are the lean, hand-written shapes.

### Bare array

```json
[
  {
    "abbreviation": "mfg",
    "title": "Mit freundlichen Grüßen",
    "body": "Mit freundlichen Grüßen,\n\nMartin Pfeffer"
  },
  {
    "abbreviation": "addr",
    "body": "Some Street 1\n12345 City"
  }
]
```

### Wrapped object

```json
{
  "snippets": [
    { "abbreviation": "mfg", "title": "Mit freundlichen Grüßen", "body": "…" },
    { "abbreviation": "addr", "body": "…" }
  ]
}
```

The wrapped form is preferred when you want to extend the schema later (e.g., add a top-level `version`, `metadata`, etc.) without breaking the parser. **Neither lean shape carries groups** — snippets imported from them land ungrouped (an existing snippet keeps whatever group it already had).

### Backup envelope — the shape with groups (v0.84.259+)

This is what **Export** writes and what the importer prefers. It's a normal backup document with only the snippet sections filled; the empty sections mean "don't touch" on import, so the file is a safe snippets-only exchange format.

```json
{
  "version": 2,
  "exported_at": 1752300000000,
  "snippet_categories": [
    { "name": "AI Prompts", "sort_order": 1 },
    { "name": "Colors", "sort_order": 2 }
  ],
  "snippets": [
    {
      "id": 1,
      "abbreviation": "aiplan",
      "title": "Implementation plan",
      "body": "…",
      "created_at": 1750000000000,
      "updated_at": 1750000000000,
      "category": "AI Prompts"
    }
  ],
  "history": [], "notes": [], "totp_entries": [], "settings": {}
}
```

Rules that make this round-trip losslessly:

- **Groups travel by NAME**, not by id (ids are machine-local). A group that doesn't exist on the target is created; `snippet_categories` also carries **empty** groups and the ordering, so those survive too.
- **Group assignment is three-valued** (v0.84.262):

  | `category` | Meaning on import |
  |---|---|
  | `"AI Prompts"` | Put the snippet in that group (created if missing) |
  | `""` (empty string) | **Explicitly ungroup** the snippet |
  | `null` / absent | Leave an existing snippet's group **untouched** |

  The `null`-means-leave-alone rule is what stops a group-less re-import (a lean file, a hand-written array) from wiping your grouping — and it's exactly why the empty string exists as the opposite signal. Export always writes `null` for an ungrouped snippet; `""` is an import-only affordance for external editors.
- **Dropping a *full* app backup** on the Snippets tab imports only its snippets + groups. History, notes, 2FA and settings in the file are ignored — you can't restore an app by accident here (use Settings → Backup & restore for that).
- **Encrypted backups are rejected** with a pointer to Settings → Backup & restore.

## Field reference

| Field          | Required | Type   | Notes                                                          |
|----------------|----------|--------|----------------------------------------------------------------|
| `abbreviation` | yes      | string | Trimmed; must be non-empty after trim. Unique per database.    |
| `title`        | no       | string | Defaults to empty. Trimmed. Shown as the secondary list label. |
| `body`         | yes      | string | Must be non-empty after trim. Pasted verbatim — newlines kept. |

## Semantics

- **Upsert by `abbreviation`.** If a snippet with the same abbreviation already exists, Inspector Rust overwrites its title and body and bumps `updated_at`. The original `created_at` is preserved.
- **Per-row error tolerance.** A row with a missing field doesn't abort the whole import — it's counted as "skipped" with the index and abbreviation in the error list.
- **Order-sensitive duplicates within a file.** If your file has two rows with the same abbreviation, the *last* one wins (each row is upserted in document order).
- **Whitespace trimming.** Leading/trailing whitespace is stripped from `abbreviation` and `title`. The `body` is preserved exactly — leading spaces and trailing newlines you put in your file end up in the paste.
- **JSON parse errors abort.** A malformed file produces a single error string; nothing is written.

## Sample files

Several themed examples live under [`docs/examples/snippets/`](./examples/snippets/) — pick one as a starting point and import it directly to verify the flow:

| File | Snippets | Theme |
|------|----------|-------|
| [`getting-started.json`](./examples/snippets/getting-started.json) | 3 | Minimal first-run sample (address, email, German signature) |
| [`signatures.json`](./examples/snippets/signatures.json) | 4 | Email signatures (short, long, German, OOO template) |
| [`dev.json`](./examples/snippets/dev.json) | 8 | Developer boilerplates (shebang, MIT header, fn skeletons, gitignore, commit-msg) |
| [`markdown.json`](./examples/snippets/markdown.json) | 5 | Markdown / GitHub scaffolds (headings, table, `<details>`, PR-body) |
| [`wrapped-form.json`](./examples/snippets/wrapped-form.json) | 2 | Demonstrates the `{ "snippets": [...] }` wrapped shape |

**Try it:**

1. Open Inspector Rust (`Ctrl+Space`)
2. **Snippets** tab → **Import**
3. Select e.g. [`docs/examples/snippets/getting-started.json`](./examples/snippets/getting-started.json) — three new entries (`addr`, `email`, `mfg`) appear in the list.

To merge several example files into one import, see [`docs/examples/snippets/README.md`](./examples/snippets/README.md).

## Tips & anti-patterns

- **Use abbreviations that don't collide with normal text you type.** `mfg` is unique enough; `the` would match every search.
- **Prefer short prefix-friendly abbreviations.** Inspector Rust matches abbreviation prefixes first, so `sigDe` wins over `sig` only after you type the `D`.
- **Avoid trailing whitespace on a line you don't intend.** The body is pasted verbatim — including stray trailing spaces.
- **Keep one file per theme** rather than one mega-file — easier to share, edit, and re-import selectively.
- **Don't hard-code dynamic data** (timestamps, current commit SHA, etc.). Inspector Rust doesn't templatize; what's in the body is what gets pasted. Use placeholders like `<DATE>` and edit after pasting.

## Export (v0.84.263)

Snippets tab → the **⬇** button in the toolbar → pick a path. It writes **all** snippets *with their groups* (empty groups included) as the backup envelope shown above, defaulting to `ir-snippets-<YYYY-MM-DD>.json`.

That file is the exchange format: edit it elsewhere, then drop it back on the Snippets tab. Round-tripping it into another Inspector Rust install works the same way (drop it, or Settings → Backup & restore → Import).

**Import is a merge, never a replace.** Snippets are upserted by `abbreviation`, so:

- A snippet you **deleted** in the external editor still exists here — the file simply doesn't mention it. Delete it in the app.
- **Renaming an abbreviation** creates a *new* snippet on import; the old one stays. The abbreviation is the identity.

Full backup-format reference: [`docs/backup.md`](./backup.md).

### SQLite + jq (legacy, hand-curated)

Only useful for a groups-less dump — the export button above is strictly better. Note this reads the DB directly, so it only works when [encryption at rest](./encryption.md) hasn't kicked in for `body` (i.e. legacy plaintext rows):

```bash
# macOS
sqlite3 "$HOME/Library/Application Support/InspectorRust/history.db" \
  "SELECT json_group_array(json_object('abbreviation', abbreviation, 'title', title, 'body', body)) FROM snippets;" \
  | jq . > my-snippets.json
```

The output is the bare-array form documented above and directly re-importable — **without** group assignments.

## IPC surface (for integrators)

Two commands cover the import path. Both return `ImportResult`:

```ts
interface ImportResult {
  imported: number;   // rows written (insert + update)
  skipped: number;    // rows that failed validation
  errors: string[];   // per-row error messages, "#<idx> (<abbr>): <reason>"
}
```

| Command                                | Use when                                                      |
|----------------------------------------|---------------------------------------------------------------|
| `import_snippets(json: String)`        | You already have the JSON in memory (e.g., from a Tauri event, paste, or in-memory generation). |
| `import_snippets_from_file(path: String)` | You have a filesystem path — after `dialog.open()` **or** from a drag-and-drop payload. Rust reads the file, then runs the same parser. |
| `export_snippets_to_file(path: String)` | Write all snippets + groups as the backup envelope. Returns the number of snippets written. |

Both import commands route through `backup::import_snippets_json`, which dispatches on the file's shape (backup envelope → snippets + groups only; lean array / wrapped → `snippets::import_from_json`) — so the picker, the drop target and a programmatic call all behave identically.

Frontend wrapper used by the Snippets tab:

```ts
import { open } from "@tauri-apps/plugin-dialog";
import { importSnippetsFromFile } from "../lib/ipc";

const selected = await open({
  multiple: false,
  directory: false,
  filters: [{ name: "JSON", extensions: ["json"] }],
  title: "Select snippets JSON file",
});
if (selected) {
  const result = await importSnippetsFromFile(selected);
}
```

Backend implementation: [`snippets.rs::import_from_json`](../core/rust-lib/src/snippets.rs) (lean shapes) and [`backup.rs::import_snippets_json` / `export_snippets_json`](../core/rust-lib/src/backup.rs) (the envelope + the dispatch), wired up in [`commands.rs`](../core/rust-lib/src/commands.rs).

## Testing

Run with `cargo test --workspace`.

The lean-shape parser (`snippets.rs`):

| Test                                          | Asserts                                                   |
|-----------------------------------------------|-----------------------------------------------------------|
| `import_bare_array_inserts_each_row`          | Bare array → all rows inserted                            |
| `import_wrapped_object_form_works`            | `{snippets: [...]}` form parses + inserts                 |
| `import_skips_rows_with_missing_fields`       | Empty `abbreviation` or `body` are skipped, not aborted   |
| `import_overwrites_existing_abbreviation`     | Re-import upserts in place — no duplicate row             |
| `import_invalid_json_returns_err`             | Malformed JSON returns an `Err`, no DB writes             |
| `import_trims_abbreviation_whitespace`        | Whitespace trimming on `abbreviation`                     |
| `upsert_category_keep_clear_set`              | The three-valued group assignment (keep / clear / set)    |
| `category_less_reimport_preserves_grouping`   | A group-less re-import doesn't wipe existing grouping     |

The envelope + exchange path (`backup.rs`):

| Test                                             | Asserts                                                        |
|--------------------------------------------------|----------------------------------------------------------------|
| `snippets_only_export_import_round_trips_groups`  | Export → import into a fresh DB: groups by name, empty group survives |
| `snippet_categories_round_trip_by_name`           | Same, through the full backup                                   |
| `empty_category_string_ungroups_a_snippet`        | `"category": ""` → explicitly ungrouped                         |
| `missing_category_field_preserves_grouping`       | `null` / absent → existing group untouched                      |
| `snippet_import_of_a_full_backup_touches_only_snippets` | Dropping a full backup imports no history / notes         |
| `snippet_import_accepts_the_lean_array_shape`     | The dispatch falls back to the lean parser                      |
| `snippet_import_rejects_an_encrypted_backup`      | Encrypted file → clear error, no writes                         |
