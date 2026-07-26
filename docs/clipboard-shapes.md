# Copy shapes, rich-text fidelity, and lineage rails

How Inspector Rust decides *what* a clipboard entry actually contains, what happens when you copy the same content in a different shape, and how the list shows that the two belong together.

Landed across **v0.93.1** (fidelity + lineage) and **v0.93.2** (discoverability).

---

## 1. A rich copy carries two things

When you copy from an app that formats text — a Markdown editor, a browser, a word processor — the system clipboard receives **several representations of the same selection at once**:

| Flavour | What it holds |
|---|---|
| `public.html` / `text/html` | The styled version: tags, inline CSS, the rendered look |
| `public.utf8-plain-text` / `text/plain` | The **source text** — what you'd get pasting into a plain editor |
| `public.rtf` | The rich-text version (some apps) |

Inspector Rust stores rich clips in two columns:

- `content_data` — the raw rich payload (HTML/RTF). This is what a *formatted* paste writes back.
- `content_text` — the plain-text representation. This is what the list row shows, what search matches against, and — with the default `paste.plain_text_only` — **what <kbd>Enter</kbd> pastes**.

### The bug this replaced

Before v0.93.1, `content_text` was *derived* from the rich payload by a tag stripper. That stripper collapses runs of whitespace:

```rust
out.split_whitespace().collect::<Vec<_>>().join(" ")
```

For prose that's harmless. For **Markdown it is destructive** — every newline becomes a space, so

```markdown
# Title

- one
- two
```

was stored (and pasted) as `Title one two`. The reported symptom was *"Markdown is shown as HTML in the list"*: the entry was tagged `HTML`, its preview was a flattened line, and pasting it produced that same flattened line instead of the Markdown you copied.

### The fix

The capture branches for HTML and RTF now prefer the clipboard's **own text flavour**:

```rust
let plain = self.ctx.get_text().ok();
let text = preview_text(plain.as_deref(), || strip_html(&html));
```

`preview_text` is pure and unit-tested; the stripper is kept as a **lazy** fallback (it isn't even computed when a usable text flavour exists) for the rare rich-without-text clipboard.

| Source | `content_data` | `content_text` |
|---|---|---|
| Markdown editor | rendered HTML | the **Markdown source**, verbatim |
| Browser selection | page HTML | the visible text, with its line breaks |
| Rich text, no text flavour | RTF | stripper output (fallback) |

**Caveat — this is not retroactive.** Entries captured before v0.93.1 keep their flattened text; the repair applies from the next copy onward.

---

## 2. Three rules for what a copy does to the list

| Action | Clipboard gets | History |
|---|---|---|
| <kbd>Enter</kbd> on an entry | the entry's own content (per `paste.plain_text_only`) | that entry moves **back to the top** |
| <kbd>Shift</kbd>+<kbd>Enter</kbd> | the original formatted payload | same — moves to the top |
| A **transform** (<kbd>⌘</kbd>/<kbd>Ctrl</kbd>+<kbd>1…9</kbd>, "Plain text") | the transformed text | a **new** entry at the top; the original keeps its content **and its position** |

The third rule is the important one: converting never rewrites what you already had. Uppercasing a clip gives you *two* entries — the original, untouched and exactly where it was, and the uppercase copy at the top.

Re-running the same transform is idempotent: the payload hashes identically, so the existing copy is bumped back to the top instead of a duplicate being created.

**Recency now shows immediately.** Pasting always refreshed the entry's `last_used_at`, but nothing notified the frontend — the reordering only became visible the next time the popup opened. Both paste commands now emit `clipboard-changed`.

---

## 3. Lineage — the data model

A derived copy records where it came from:

| Column | Type | Meaning |
|---|---|---|
| `derived_from` | `INTEGER` | Row id of the entry this clip was copied from |
| `derived_kind` | `TEXT` | Which manipulation produced it (`upper`, `base64-encode`, `plain-text`, …) |

Both are nullable, plaintext (nothing sensitive — they're a shape name and a row id), and added by a lazy `ALTER TABLE` migration like `pinned` and `note`.

### Written only on insert

`db::upsert_clip_derived` sets lineage **only when it inserts a new row**. On a hash collision the existing row just gets its recency bumped:

> A transform that happens to produce text you once copied on its own is a **coincidence**. Retroactively labelling that old entry as "a copy of X" would invent a relationship that never existed.

This is asserted by `organic_clip_is_never_retroactively_relabelled_as_derived`.

### Backup restore is the deliberate exception

`derived_from` is a foreign key, and row ids are assigned fresh on import — so a restore cannot copy the number across. `backup::apply` builds an old-id → new-id map while importing the history, then re-points each rail in a second pass via `db::set_lineage_if_absent`.

Here filling an empty lineage *is* correct: the backup states a fact about this very clip rather than a coincidence. A rail whose source didn't make it into the backup (pruned before the export) is **dropped** — better no rail than one aimed at a stranger.

---

## 4. Lineage rails — the rendering

The relationship is drawn as a commit-graph-style rail down the left edge of the list: a node dot on the copy and on its source, a line running through the rows in between.

```
 ●─  HELLO WORLD          ← the copy (node)
 │   some unrelated clip  ← the lane passes through
 ●─  Hello World          ← the source (node)
     another clip         ← outside the span, no rail
```

### The algorithm (`lib/lineage.ts`, pure + 20 tests)

1. **Families.** A union-find over the `derived_from` edges groups clips into connected lineages. A chain (`A ← B ← C`) is *one* family in one colour, not two branches.
2. **Spans.** A family occupies `min..max` of its member positions. The source is **not** necessarily below its copy — pasting the original bumps it back to the top — so nothing may assume a direction.
3. **Lanes.** A greedy allocator walks families top-down and gives each the lowest lane not occupied by a family it overlaps. Non-overlapping families reuse lanes, exactly like a commit graph.
4. **Colours.** Assigned **by lane**, not by clip id, so two lineages drawn next to each other can never share a colour.

### Deliberate edge-case behaviour

| Situation | What you see |
|---|---|
| Source pruned / deleted | The copy shows a **lone node** — you can still tell it's a copy |
| Middle of a chain pruned (`A ← B̶ ← C`) | `C` stands alone; it is **not** wired to `A`, which it never came from |
| A clip whose only copies are off-list | Nothing — having descendants somewhere isn't a lineage to draw |
| Non-clip rows (commands, snippets) interleaved | Passed as `null` so lane spans still match the rendered rows |
| More lanes than fit | Deeper lanes are not drawn (see below) |

### Gutter and clamp must agree

Every lane widens the gutter the list reserves on the left, so the number of drawable lanes is capped (`MAX_LANES`). The reserved width and the rendered rails derive from the **same** constants:

```ts
export const LANE_W = 5;
export const MAX_LANES = 4;
export function railGutterPx(rails) { … }   // what HistoryList reserves
export function visibleRails(rails) { … }   // what HistoryItem draws
```

They lived in two files briefly, with the gutter capped and the renderer uncapped — past four concurrent lineages the rails would have drawn over the row text. `railGutterPx / visibleRails` pins the two together and a test asserts every drawn lane fits inside the reserved gutter.

The gutter is uniform across the whole list (computed over *all* entries, not the virtual window) so rows never jitter while scrolling, and it is `0` when the rails are off — switching them changes nothing about layout.

---

## 5. Finding the formatting options

The transform chips only render while the platform modifier is held — <kbd>⌘</kbd> on macOS, <kbd>Ctrl</kbd> on Windows and Linux (`useModifierHeld` accepts either). Until v0.93.2 the slot rendered *nothing* when it wasn't, which made the whole feature invisible unless you already knew about it.

The preview now shows the hint in that slot instead:

> ⌨ Hold <kbd>⌘</kbd> for formatting options

Clicking the hint pins the chips open, so the options are reachable by mouse as well; a `×` in the header collapses them. The bar is only mounted for text / HTML / RTF clips, so the hint never appears on images or file lists.

---

## 6. Settings

| Setting | Key | Default | Effect |
|---|---|---|---|
| **Lineage rails** (Settings → Appearance) | `history.lineage_highlight` | on | Draws the coloured paths. Purely visual — nothing about what gets copied, stored or pasted changes |
| **Paste as plain text** | `paste.plain_text_only` | on | <kbd>Enter</kbd> pastes `content_text` (the Markdown source, after §1) rather than the styled payload |

The rails setting is re-read whenever the History tab comes back into view — the only place it can be changed is the Settings tab, so returning is exactly when a fresh value is needed. No event plumbing, and it cannot go stale.

---

## 7. Where the code lives

| Concern | File |
|---|---|
| Capture + `preview_text` | [`core/rust-lib/src/clipboard_watcher.rs`](../core/rust-lib/src/clipboard_watcher.rs) |
| Schema, `upsert_clip_derived`, `set_lineage_if_absent` | [`core/rust-lib/src/db.rs`](../core/rust-lib/src/db.rs) |
| Paste + `commit_transformed_text` | [`core/rust-lib/src/commands.rs`](../core/rust-lib/src/commands.rs) |
| Lineage restore across ids | [`core/rust-lib/src/backup.rs`](../core/rust-lib/src/backup.rs) |
| Lane layout (pure) | [`core/frontend/src/lib/lineage.ts`](../core/frontend/src/lib/lineage.ts) |
| Rail rendering + gutter | [`HistoryItem.tsx`](../core/frontend/src/components/HistoryItem.tsx), [`HistoryList.tsx`](../core/frontend/src/components/HistoryList.tsx) |
| Transform chips + hint | [`PreviewPanel.tsx`](../core/frontend/src/components/PreviewPanel.tsx) |

### Test coverage

| Area | Tests |
|---|---|
| `preview_text` (prefers text flavour, falls back, lazy) | 3 (Rust) |
| Derived upsert: own entry, source untouched, no relabelling, idempotent | 3 (Rust) |
| Every read path returns the lineage (column-index guard) | 1 (Rust) |
| `set_lineage_if_absent` fills only an empty lineage | 1 (Rust) |
| Backup: id remapping + dangling rail dropped | 2 (Rust) |
| Lane layout, spans, chains, pruning gaps, gutter/clamp | 20 (frontend) |

See also: [backup.md](./backup.md) · [encryption.md](./encryption.md) · [colors.md](./colors.md)
