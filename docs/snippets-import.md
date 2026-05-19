# Snippet import (JSON)

Inspector Rust can bulk-import snippets from a JSON file. This is the fastest way to seed the app with your existing templates, share snippet libraries between machines, or back up your collection.

## How to import

1. Open the popup with `Ctrl+Shift+V`.
2. Click the **Snippets** tab in the upper-right of the header.
3. Click **Import** (top-right of the snippet list).
4. The native file picker opens (NSOpenPanel on macOS, OpenFileDialog on Windows). Select a `.json` file.

Result is shown as a one-line status:

```
Imported 5
Imported 4, skipped 1 — #2 (mfg): body is empty
```

The full list refreshes automatically.

## File format

Two top-level shapes are accepted:

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

The wrapped form is preferred when you want to extend the schema later (e.g., add a top-level `version`, `metadata`, etc.) without breaking the parser.

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

1. Open Inspector Rust (`Ctrl+Shift+V`)
2. **Snippets** tab → **Import**
3. Select e.g. [`docs/examples/snippets/getting-started.json`](./examples/snippets/getting-started.json) — three new entries (`addr`, `email`, `mfg`) appear in the list.

To merge several example files into one import, see [`docs/examples/snippets/README.md`](./examples/snippets/README.md).

## Tips & anti-patterns

- **Use abbreviations that don't collide with normal text you type.** `mfg` is unique enough; `the` would match every search.
- **Prefer short prefix-friendly abbreviations.** Inspector Rust matches abbreviation prefixes first, so `sigDe` wins over `sig` only after you type the `D`.
- **Avoid trailing whitespace on a line you don't intend.** The body is pasted verbatim — including stray trailing spaces.
- **Keep one file per theme** rather than one mega-file — easier to share, edit, and re-import selectively.
- **Don't hard-code dynamic data** (timestamps, current commit SHA, etc.). Inspector Rust doesn't templatize; what's in the body is what gets pasted. Use placeholders like `<DATE>` and edit after pasting.

## Export

### Built-in (recommended): Settings → Backup & restore

Since v0.2.12 the Settings tab's *Backup & restore* section can write a JSON file containing only your snippets. Untick *Clipboard history* and *Notes*, click **Export…**, pick a path. The resulting file matches the full-backup schema (`{ version, exported_at, history: [], snippets: [...], notes: [] }`); the snippet importer below also accepts the bare-array and `{ snippets: [...] }` shapes for hand-curated files.

To round-trip into another Inspector Rust install: paste the file via **Settings → Backup & restore → Import…**. Snippets are upserted by `abbreviation`; the empty `history` and `notes` arrays are no-ops on the destination side.

Full backup-format reference: [`docs/backup.md`](./backup.md).

### SQLite + jq (legacy, hand-curated)

If you want a snippets-only file without the wrapping `{ version, …, history, notes }` envelope, query the database directly:

```bash
# macOS
sqlite3 "$HOME/Library/Application Support/InspectorRust/history.db" \
  "SELECT json_group_array(json_object('abbreviation', abbreviation, 'title', title, 'body', body)) FROM snippets;" \
  | jq . > my-snippets.json

# Windows (PowerShell)
sqlite3 "$env:APPDATA\InspectorRust\history.db" `
  "SELECT json_group_array(json_object('abbreviation', abbreviation, 'title', title, 'body', body)) FROM snippets;" `
  | ConvertFrom-Json | ConvertTo-Json -Depth 5 > my-snippets.json
```

The output is the bare-array form documented above and directly re-importable via **Snippets → Import**.

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
| `import_snippets_from_file(path: String)` | You have a filesystem path (typical case after `dialog.open()`). Rust reads the file with `std::fs::read_to_string` then runs the same parser. |

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

Backend implementation: [`core/rust-lib/src/snippets.rs::import_from_json`](../core/rust-lib/src/snippets.rs) and [`core/rust-lib/src/commands.rs::import_snippets_from_file`](../core/rust-lib/src/commands.rs).

## Testing

Six unit tests cover the import path (run with `cargo test --workspace`):

| Test                                          | Asserts                                                   |
|-----------------------------------------------|-----------------------------------------------------------|
| `import_bare_array_inserts_each_row`          | Bare array → all rows inserted                            |
| `import_wrapped_object_form_works`            | `{snippets: [...]}` form parses + inserts                 |
| `import_skips_rows_with_missing_fields`       | Empty `abbreviation` or `body` are skipped, not aborted   |
| `import_overwrites_existing_abbreviation`     | Re-import upserts in place — no duplicate row             |
| `import_invalid_json_returns_err`             | Malformed JSON returns an `Err`, no DB writes             |
| `import_trims_abbreviation_whitespace`        | Whitespace trimming on `abbreviation`                     |
