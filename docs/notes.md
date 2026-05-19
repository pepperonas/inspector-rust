# Notes

Inspector Rust **notes** are persistent, categorized clipboard items. They live in their own SQLite table and are *not* affected by the 1 000-entry pruning of the clipboard history — so they're the right place for things you want to keep around indefinitely (server addresses, license keys, frequently-used screenshots, that one perfect curl command, …).

Notes were introduced in **v0.2.6**.

## Workflow

There are three ways to create a note:

### 1. From a clipboard entry (the common case)

1. Open the popup (`Ctrl+Shift+V`).
2. Hover over any **History** row.
3. A bookmark icon appears next to the timestamp on the right — click it.

The entry is copied (payload + content type) into the `notes` table under the `Uncategorized` bucket. From that moment on the note is **decoupled** from the original clip; even if the clip is later pruned out of history (because you copied 1 000 newer things), the note stays.

### 2. From scratch (text notes only)

1. Open the popup.
2. Click the **Notes** tab in the upper-right of the header.
3. Click **+ New Note** at the bottom of the categories sidebar.
4. Fill in *Title (optional)*, *Category*, and *Body*. Press **Create**.

### 3. From the system tray

The tray menu has a **Manage Notes** entry that opens the popup directly on the Notes tab — no hotkey + tab-click needed.

## Notes tab layout

Three panes side-by-side:

| Pane | Width | Contents |
|------|-------|----------|
| **Categories sidebar** | 25 % (min 180 px) | `All`, `Uncategorized`, then your categories alphabetically (case-insensitive). Each row shows a count. Below the list: **+ New Note** and **Clear All**. (Backup Export / Import moved to the Settings tab in v0.2.12 — see [`docs/backup.md`](./backup.md).) |
| **Note list** | 40 % | Notes in the selected category, newest first. Hover a row to reveal the per-row delete button. Click selects, double-click pastes. |
| **Detail / edit pane** | 35 % | Title (optional), Category (free-form, autocompletes from existing categories), Body editor. **Paste**, **Close**, **Save** at the bottom; **Delete** trash icon on the left. |

## Categories

Categories are **free-form strings** stored on each note as a `category` column.

- A note belongs to exactly **one** category. (Tags / multi-select are deliberately out of scope for v1 — keeps the UI simple.)
- The category list in the sidebar is `SELECT DISTINCT category FROM notes WHERE category != '' ORDER BY LOWER(category) ASC` — so categories appear automatically the first time you save a note with a new category name. There is no separate "create category" step.
- An empty category falls into the virtual `Uncategorized` group. Selecting it filters the list to those notes.
- The detail pane's category input has a `<datalist>` of existing categories, so typing `W` will autocomplete to `Work` if it exists.
- A category disappears from the sidebar the moment its last note is deleted or moved out of it.

To **rename** a category in bulk: there is no UI shortcut yet — open each note in that category and change the field, or do it directly in SQLite (`UPDATE notes SET category = 'NewName' WHERE category = 'OldName';`).

## Edit semantics per content type

When you save a clip as a note, Inspector Rust copies its `content_type` verbatim. The detail pane behaves differently depending on that type:

| Content type | Body editor                            | Paste behaviour                                |
|--------------|----------------------------------------|------------------------------------------------|
| `text`       | Plain `<textarea>`, fully editable     | Pasted as plain text                           |
| `html`       | `<textarea>` with raw HTML markup, editable — power-user feature | **Settings → Paste → Plain text only = on** (default since v0.4.0) → pasted as the plain-text preview. Toggle off → pasted as HTML (apps that understand it use the formatting; others fall back to plain text). |
| `rtf`        | `<textarea>` with raw RTF markup, editable — power-user feature | Same as `html` — plain-text preview by default, original RTF when the toggle is off. |
| `image`      | Inline preview (`<img>`), **read-only** | Pasted as image (the plain-text setting only affects html/rtf, not images/files) |
| `files`      | List of file paths, **read-only**       | Pasted as newline-joined paths (clipboard-rs cannot set real file lists from Rust on every OS) |

Title and Category are **always editable**, regardless of content type.

The backend short-circuits body updates for image/files notes — even if the frontend somehow sent a body, it's ignored. See [`core/rust-lib/src/notes.rs::update`](../core/rust-lib/src/notes.rs).

## Pasting a note

There are three ways to paste:

- **Double-click** the row in the list.
- **Single-click** to select, then click **Paste** in the detail pane.
- The note's `content_type` is preserved on paste — image notes paste as images, HTML notes paste as HTML, etc.

Same focus-restoration trick as the History tab: the popup is hidden first (on macOS the whole app is hidden via `NSApp.hide(nil)`), then `enigo` synthesizes Cmd+V / Ctrl+V into the previously frontmost window.

## Delete

- **Per-note delete:** trash icon on the right of each list row (visible on hover) **or** the trash icon at the left of the detail pane's button bar.
- **Clear All:** sidebar action at the bottom — guarded by a `window.confirm` dialog showing the note count.

There is no "delete category" — deleting all notes in a category effectively removes it from the sidebar.

## Database

Notes live in `notes` in the same SQLite database as `entries` (history) and `snippets`:

```sql
CREATE TABLE notes (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    content_type TEXT    NOT NULL,
    content_text TEXT    NOT NULL DEFAULT '',
    content_data TEXT    NOT NULL DEFAULT '',
    title        TEXT    NOT NULL DEFAULT '',
    category     TEXT    NOT NULL DEFAULT '',
    byte_size    INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);
CREATE INDEX idx_notes_category ON notes(category);
CREATE INDEX idx_notes_updated  ON notes(updated_at DESC);
```

`content_data` follows the same convention as `entries.content_data`:
- `text` / `html` / `rtf` → raw payload string
- `image` → base64-encoded PNG
- `files` → JSON array of paths

`byte_size` is for display only (`formatBytes` in the detail header).

Database file location:
- **Windows:** `%APPDATA%\InspectorRust\history.db`
- **macOS:** `~/Library/Application Support/InspectorRust/history.db`

## IPC surface (for integrators)

| Command                        | Args                              | Returns         |
|--------------------------------|-----------------------------------|-----------------|
| `list_notes`                   | —                                 | `Note[]`        |
| `list_note_categories`         | —                                 | `string[]`      |
| `save_clip_as_note`            | `clip_id, title, category`        | `i64` (note id) |
| `create_note`                  | `title, body, category`           | `i64`           |
| `update_note`                  | `id, title, body, category`       | `void`          |
| `delete_note`                  | `id`                              | `void`          |
| `clear_notes`                  | —                                 | `void`          |
| `paste_note`                   | `id`                              | `void` — honours `paste.plain_text_only` |
| `paste_note_formatted`         | `id`                              | `void` — bypasses the setting; always uses original content type |

`Note` shape:

```ts
interface Note {
  id: number;
  content_type: "text" | "rtf" | "html" | "image" | "files";
  content_text: string;
  content_data: string;
  title: string;
  category: string;
  byte_size: number;
  created_at: number;   // unix-millis
  updated_at: number;   // unix-millis
}
```

Frontend wrappers in [`core/frontend/src/lib/ipc.ts`](../core/frontend/src/lib/ipc.ts); backend in [`core/rust-lib/src/notes.rs`](../core/rust-lib/src/notes.rs) and [`core/rust-lib/src/commands.rs`](../core/rust-lib/src/commands.rs).

## Events

The Rust backend emits one note-related event:

| Event             | Source                                         | Frontend handler                       |
|-------------------|------------------------------------------------|----------------------------------------|
| `open-notes-tab`  | Tray menu → **Manage Notes**                   | `App.tsx` switches to the Notes tab and refreshes the list. |

## Testing

The notes module has 10 unit tests (`cargo test -p inspector-rust-core notes`):

| Test                                              | Asserts                                                            |
|---------------------------------------------------|--------------------------------------------------------------------|
| `create_text_inserts_a_note`                      | New text note lands with correct title, body, category             |
| `save_from_clip_copies_payload_and_returns_id`    | Snapshot copies content from `entries`                             |
| `save_from_clip_returns_none_when_clip_missing`   | Source clip already pruned → `Ok(None)`                            |
| `list_all_orders_by_updated_at_desc`              | Newest note first                                                  |
| `list_categories_returns_distinct_non_empty_sorted` | DISTINCT, no empty, sorted case-insensitively                    |
| `update_changes_title_body_and_category`          | All editable fields update + `byte_size` recalculated              |
| `update_ignores_body_for_image_notes`             | Image notes are read-only at the body level                        |
| `delete_removes_note`                             | Per-row delete                                                     |
| `clear_all_removes_every_note`                    | `Clear All` empties the table                                      |
| `append_imported_preserves_timestamps_and_payload` | Backup-restore path is lossless (used by [`docs/backup.md`](./backup.md)) |

## See also

- [`docs/backup.md`](./backup.md) — full-app JSON export/import that includes notes.
- [`docs/snippets-import.md`](./snippets-import.md) — snippets are a separate feature with their own JSON import path.
- [`docs/spec.md`](./spec.md) — original product specification.
