# Full-app backup

ClipSnap's **Backup** feature exports the complete database (`history` + `snippets` + `notes`) to a single JSON file and merges that file back on import. This is the way to:

- move your collection to a new machine,
- snapshot your state before risky edits or before importing someone else's snippets,
- share a curated set of notes/snippets with a colleague (after editing the JSON to keep just what's relevant).

Backup was introduced in **v0.2.6**. Moved to the **Settings** tab and gained per-section selection in **v0.2.12**.

## How to export

1. Open the popup (`Ctrl+Shift+V`).
2. Click the **Settings** tab → **Backup & restore** section.
3. Tick which sections to include — *Clipboard history*, *Snippets*, *Notes*. All three are checked by default; uncheck any you don't want in the file.
4. Click **Export…**. The native save dialog opens (NSSavePanel on macOS, Win32 SaveFileDialog on Windows). Default filename is `clipsnap-backup-<ISO timestamp>.json`. Pick a location and confirm.

The status line shows the bytes written, e.g. `Exported 124.5 KB to clipsnap-backup-2026-04-25T09-30-15.json`.

> **Tip — share snippets without leaking history.** Untick *Clipboard history* and *Notes* for a snippets-only file you can hand to a colleague. They run **Import…** and only your snippets get merged into their database (history dedupes against an empty array, notes append zero rows).

Sections that are unticked are written as empty `Vec`s in the JSON — version + timestamp are still set so the resulting file is always parseable on import.

## How to import

1. Open the popup → **Settings** tab → **Backup & restore** → **Import…**.
2. Pick a `.json` file in the open dialog.

Import is always full-merge; whatever sections the file actually contains get merged into the live database. Empty sections in the file are no-ops.

The status line summarizes the merge:

```
Imported 12 notes, 8 snippets, 47 history
Imported 12 notes, 8 snippets, 47 history — note #3: ... (+2 more)
```

Per-row failures are collected — they don't abort the whole import.

## File format

```json
{
  "version": 1,
  "exported_at": 1714032615000,
  "history":  [ /* ClipEntry[] */ ],
  "snippets": [ /* Snippet[]   */ ],
  "notes":    [ /* Note[]      */ ]
}
```

### Top-level fields

| Field          | Type     | Notes                                                                    |
|----------------|----------|--------------------------------------------------------------------------|
| `version`      | `u32`    | Currently `1`. Bumped whenever the on-disk shape changes incompatibly.   |
| `exported_at`  | `i64`    | Unix milliseconds — purely informational.                                |
| `history`      | array    | Full clipboard history (not paginated). `ClipEntry` rows.                |
| `snippets`     | array    | All snippets. `Snippet` rows.                                            |
| `notes`        | array    | All notes. `Note` rows.                                                  |

### Per-row shapes

```ts
interface ClipEntry {
  id: number;
  content_type: "text" | "rtf" | "html" | "image" | "files";
  content_text: string;   // plain-text preview / search index
  content_data: string;   // raw payload (base64 for image, JSON array for files)
  hash: string;           // SHA-256 of content_type + content_data
  byte_size: number;
  created_at: number;     // unix-millis
  last_used_at: number;   // unix-millis
}

interface Snippet {
  id: number;
  abbreviation: string;
  title: string;
  body: string;
  created_at: number;
  updated_at: number;
}

interface Note {
  id: number;
  content_type: "text" | "rtf" | "html" | "image" | "files";
  content_text: string;
  content_data: string;
  title: string;
  category: string;
  byte_size: number;
  created_at: number;
  updated_at: number;
}
```

The `id` fields are **ignored on import** — SQLite assigns fresh autoincrement ids. Hashes (history) and abbreviations (snippets) are the natural keys used for dedup.

## Merge semantics

Import is a **merge**, not a replace. Each table has its own dedup strategy:

### Snippets — upsert by `abbreviation`

Same path used by the JSON snippet importer ([`docs/snippets-import.md`](./snippets-import.md)). If a snippet with the same `abbreviation` already exists, ClipSnap overwrites its `title`/`body` and bumps `updated_at`. The original `created_at` is preserved.

→ Re-importing the same backup is **idempotent** for snippets.

### History — upsert by SHA-256 hash

Each clipboard entry is hashed by `content_type + content_data` (see `db::hash_payload`). On import, that hash is looked up:
- **Existing row** → only `last_used_at` bumps; payload stays.
- **New row** → inserted, then `prune_locked` runs to enforce the 1 000-entry cap.

→ Re-importing the same backup adds nothing to history; restoring a backup into a *populated* database may push older entries out due to the cap, which is intentional.

### Notes — appended verbatim

Notes have no natural unique key (you may legitimately want two notes with the same title and category). On import, every note is inserted as a fresh row with the **original** `created_at` and `updated_at` preserved (so list ordering is stable across an export-import cycle).

→ Re-importing the same backup file **doubles** every note. If you want a clean replace, **Clear All** first, then import.

## Versioning

The exporter writes `"version": 1`. The importer:

| Backup version | Behaviour                                                   |
|----------------|-------------------------------------------------------------|
| `<= 1`         | Imported.                                                    |
| `> 1` (newer)  | **Rejected** with `backup version N is newer than this app supports (1)`. |

This protects against a newer ClipSnap writing fields the running build doesn't understand and silently discarding them. If you hit this, upgrade ClipSnap or hand-edit the JSON to drop unknown fields and downgrade `version`.

## Editing a backup before import

The JSON is human-readable and stable. Common surgeries with `jq`:

```bash
# Drop the entire history section before sharing with a colleague
jq '.history = []' clipsnap-backup.json > clipsnap-backup-no-history.json

# Keep only notes in category "Work"
jq '.notes |= map(select(.category == "Work"))' clipsnap-backup.json > work-only.json

# Strip image notes (they tend to be heavy)
jq '.notes |= map(select(.content_type != "image"))' clipsnap-backup.json > textual.json

# Merge two backup files (snippets/notes/history concatenated; ids will be re-assigned on import)
jq -s '
  {
    version: 1,
    exported_at: (now * 1000 | floor),
    history:  ((.[0].history  // []) + (.[1].history  // [])),
    snippets: ((.[0].snippets // []) + (.[1].snippets // [])),
    notes:    ((.[0].notes    // []) + (.[1].notes    // []))
  }
' a.json b.json > merged.json
```

## IPC surface

| Command                | Args                                                             | Returns                                            |
|------------------------|------------------------------------------------------------------|----------------------------------------------------|
| `export_backup`        | `include_history?, include_snippets?, include_notes?` (all default `true`) | `string` — pretty-printed JSON         |
| `save_backup_to_file`  | `path, include_history?, include_snippets?, include_notes?`       | `usize` — bytes written                            |
| `import_backup`        | `path`                                                           | `BackupImportResult`                               |

```ts
interface BackupExportOptions {
  includeHistory?: boolean;   // default true
  includeSnippets?: boolean;  // default true
  includeNotes?: boolean;     // default true
}

interface BackupImportResult {
  history_imported: number;
  snippets_imported: number;
  notes_imported: number;
  errors: string[];   // per-row, "snippet #3 (mfg): ..." etc.
}
```

Frontend wrappers in [`core/frontend/src/lib/ipc.ts`](../core/frontend/src/lib/ipc.ts); backend in [`core/rust-lib/src/backup.rs`](../core/rust-lib/src/backup.rs) (`ExportOptions::all()` / `ExportOptions::default()`) and [`core/rust-lib/src/commands.rs`](../core/rust-lib/src/commands.rs).

The Settings panel calls `save_backup_to_file` (one IPC hop = export + write) for export and `import_backup` after the file picker resolves for import.

## Capabilities

The popup window's `capabilities/default.json` (both `win/` and `macos/`) carries:

```json
"dialog:allow-open",   // file picker for Import
"dialog:allow-save"    // file picker for Export
```

If you fork the shells, make sure both are present.

## Testing

The `backup` module has 5 unit tests (`cargo test -p clipsnap-core backup`):

| Test                                        | Asserts                                                              |
|---------------------------------------------|----------------------------------------------------------------------|
| `export_and_import_roundtrip_into_empty_db` | Export → fresh empty db → import → all rows recovered                |
| `import_into_populated_db_merges_via_dedup` | Re-importing into the same db: history dedupes, snippets upsert, notes double |
| `import_rejects_newer_backup_version`       | Backup with `version = CURRENT + 1` → `Err`                          |
| `import_invalid_json_returns_err`           | Malformed JSON → `Err`, no DB writes                                 |
| `replace_all_clears_then_inserts`           | The (currently un-exposed) `replace_all` helper truly wipes first     |

`replace_all` is implemented but not yet wired into the UI — it's the "destructive replace" path for when we want to add it later (probably gated behind an explicit checkbox in the import dialog).

## See also

- [`docs/notes.md`](./notes.md) — Notes feature, which is included in every backup.
- [`docs/snippets-import.md`](./snippets-import.md) — snippet-only JSON import (older, narrower scope; uses the same upsert-by-`abbreviation` semantics).
- [`docs/RELEASING.md`](./RELEASING.md) — release procedure for ClipSnap itself.
