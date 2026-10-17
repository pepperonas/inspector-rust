# Translation — live preview for the `tr*` commands

The `tr*` search-bar commands translate a phrase without leaving the popup: the result appears **in the preview while you type**. <kbd>Enter</kbd> copies the translation to the clipboard; <kbd>Shift</kbd>+<kbd>Enter</kbd> opens Google Translate in the browser. Click either language box to copy that side.

Pasted multiline text keeps its line breaks, blank lines and indentation in the translation, preview and clipboard. The single-line search field displays line breaks as `↵`. Non-empty lines are translated separately so providers cannot collapse paragraph spacing; this requires one request per non-empty line per provider attempt.

Landed in **v0.93.0** (live preview); Enter=copy / click-to-copy in **v0.101.5**.
Multiline layout preservation is available in **v0.215.1**.

---

## Commands

All pairs are data-driven from a single `TRANSLATE_LANGS` map — adding a language pair is one entry plus one `COMMANDS` row.

| Command | Direction |
|---|---|
| `tr <text>` | auto-detect → German |
| `tren <text>` | English → German |
| `trde <text>` | German → English |
| `trde2it` / `trit2de` | German ↔ Italian |
| `trde2sp` / `trsp2de` | German ↔ Spanish (`sp` spells Google's `es`) |
| `trde2pl` / `trpl2de` | German ↔ Polish |

```
tren enhancement
┌─────────────────────────────────────────┐
│ TRANSLATE · ENGLISH → GERMAN            │
│                                         │
│ English                                 │
│ enhancement                             │
│                                         │
│ German                                  │
│ Verbesserung                            │
│ via google                              │
│                                         │
│ ⏎ copies · ⇧⏎ Google Translate · click to copy │
└─────────────────────────────────────────┘
```

---

## Multiline text and formatting (v0.215.1)

Type a translation keyword and a space, then paste the complete text. For
example, paste this after `tren `:

```text
Hello Anna,

Thank you for your message.


  Best regards
  Martin
```

The search bar stays one line and displays this as:

```text
tren Hello Anna,↵↵Thank you for your message.↵↵↵  Best regards↵  Martin
```

The source and translated boxes display actual lines. The two empty lines
before the closing, and the indentation of the closing, survive translation.
Leading/trailing empty lines and whitespace-only lines are retained too.
Pasting a complete command, such as `tren Hello…` followed by several lines,
also works. Pasting over a selection replaces just that selection and leaves
the surrounding text intact.

| Action | Result |
|---|---|
| Paste after any `tr*` keyword | Line breaks become visible `↵` markers in the search bar |
| View the source or translation | Actual newlines, blank lines and indentation are shown |
| Click the source box | Copies the original multiline text |
| Click the translated box or press Enter | Copies the translation with real newlines, without marker characters |
| Press Shift+Enter | Opens Google Translate with the multiline source encoded in the URL |

LF, Windows CRLF and bare CR pasted line endings are accepted and normalised
to LF by the search field. This preserves text layout; it does not retain
rich-text styling such as fonts, colours or bold formatting. A single-line
argument keeps the existing behaviour of trimming surrounding whitespace.
Enter keeps its copy action, and Shift+Enter keeps its browser action.

### How layout survives the providers

`translatePaste` intercepts the paste before the browser's single-line input
can discard newlines. `parseCommand` restores the markers before the source
reaches the preview, IPC, cache key or browser URL. Both preview boxes use
`whitespace-pre-wrap`, so HTML whitespace collapsing does not hide the layout.

The Rust `translate_lines` helper sends each non-empty line to the selected
provider and reassembles the result using the original separators and outer
whitespace of each line. Empty lines never produce requests. If any line
fails, that provider's partial result is discarded and the fallback retries
the entire text; a mixed or partially translated result is never returned.
The detected source language comes from the first line that reports one.

Each non-empty line requires a request, with the existing 2.5-second timeout
per request. Consequently long multiline text can take longer than a single
line, and translation context is limited to each line. The frontend still
debounces and caches the complete source, including its layout, and discards
stale responses after editing.

### Regression checks

Frontend tests cover every translation language pair, multiple consecutive
blank lines, indentation, leading/trailing blank lines, CRLF pastes, selection
replacement, complete-command pastes and browser URL encoding. SearchBar and
PreviewPanel tests cover paste interception and exact source/target clipboard
contents. Rust tests retain LF/CRLF layout and verify that a provider failing
on a later line cannot leak its earlier partial result into the fallback.

## Why these providers

The requirement was **free and keyless** — no account, no API key, no paid tier, no per-user setup. That rules out most of the field:

| Option | Cost / key | Verdict |
|---|---|---|
| **Google `gtx` endpoint** *(chosen — primary)* | Free, keyless | The endpoint the Google Translate web page itself calls. Same engine — and the same privacy trade-off — as the browser command this feature sits next to, so it introduces no *new* exposure. Unofficial: undocumented and could change without notice, which is precisely why there is a fallback. Handles `auto` source detection. |
| **MyMemory** *(chosen — fallback)* | Free, keyless (anonymous quota) | A translation-memory aggregator. Lower quality than Google on short idiomatic phrases, but genuinely keyless and a different operator — so it survives exactly the failure that would take Google out. Needs a concrete source language; cannot do `auto`. |
| **DeepL API Free** | **Key required** | Best quality of the lot, but every user would have to register and paste an API key. Rejected on the "no setup" requirement, not on quality. |
| **LibreTranslate** (public instances) | Free, sometimes keyless | Open source and self-hostable — attractive on principle. The public instances are heavily rate-limited, frequently down, and increasingly key-gated; depending on one would make the feature unreliable for everyone. Self-hosting shifts the burden onto the user. |
| **Argos Translate** (offline models) | Free, no network | Genuinely private and offline — the ideologically nicest option. Needs a Python runtime plus per-language-pair model downloads (hundreds of MB), which is a large amount of weight and packaging complexity for a search-bar convenience. |

**If translation quality ever needs to beat Google's**, DeepL is the upgrade — the provider abstraction below exists so it can be added as an opt-in, key-configured provider without touching anything else.

### Privacy

Only the text and the language pair leave the machine. Nothing is logged, and no identifier is attached. This is the same exposure as the pre-existing browser command — but it now happens **as you type** rather than only when you press <kbd>Enter</kbd>, which is worth knowing if you paste sensitive text into the search bar. Everything else in Inspector Rust remains offline.

---

## Architecture — a provider strategy

```rust
trait Provider {
    fn name(&self) -> &'static str;
    fn supports_source(&self, sl: &str) -> bool;
    fn translate(&self, text: &str, sl: &str, tl: &str, timeout: Duration)
        -> Result<Translation, String>;
}
```

`translate::translate` asks each provider in order until one succeeds:

1. **`GoogleGtx`** — `translate.googleapis.com/translate_a/single?client=gtx&dt=t`. Supports every source including `auto`.
2. **`MyMemory`** — `api.mymemory.translated.net/get?langpair=<sl>|<tl>`. `supports_source` returns `false` for `auto`, so it is **skipped**, not attempted and counted as a failure.

Each HTTP request has a **2.5 s** timeout (`TIMEOUT`). Multiline attempts make one request per non-empty line; the timeout is not a deadline for the whole document. If everything fails the error surfaces in the panel and <kbd>Shift</kbd>+<kbd>Enter</kbd> still opens Google Translate.

### Adding a provider

1. Implement `Provider`.
2. Add it to the list in `translate()` — order *is* the priority.
3. If its response shape is new, write a pure parser plus its tests.

Nothing else changes: the IPC, the frontend, the caching and the fallback behaviour are provider-agnostic.

### Testability

The orchestration is split from the concrete providers:

```rust
fn translate_with(providers: &[&dyn Provider], text, sl, tl, timeout) -> Result<Translation, String>
```

The real providers need the network, but *which* provider gets asked, in what order, and what happens when one is unavailable is the logic worth pinning down — so the tests drive `translate_with` with scripted stub providers. Covered: first-wins (the fallback is never contacted), fall-through on failure, an unsupported source being **skipped rather than failed**, the last error surviving when everything failed, blank input never reaching a provider, and trimming.

The response parsers are pure and tested against real captured payloads:

| Parser | Handles |
|---|---|
| `parse_google` | Concatenates the `[0]` sentence segments (long text arrives split); reads `[2]` as the detected source; rejects garbage / empty |
| `parse_mymemory` | Requires `responseStatus == 200` — as a **number or a string**, the API returns both; rejects quota messages |

---

## Frontend — debounce, cache, staleness

`App.tsx` derives a request from the parsed command and keys it on `sl|tl|text`:

| Concern | Mechanism |
|---|---|
| **Debounce** | 350 ms after the last keystroke. Typing a sentence produces one request, not one per character. |
| **Cache** | A per-session `Map` keyed by `sl|tl|text`. Re-typing, deleting back and retyping, or switching away and back resolves **instantly** with no network call. |
| **Staleness** | A monotonic sequence ref. A slow response for an older query is discarded, so fast typing always settles on the latest text — never on whichever request happened to return last. |
| **States** | `loading` (spinner) · `ok` (text + `via <provider>`) · `error` (hint stays visible). |

`PreviewPanel` renders `TranslatePreview` for any translate kind: source → target header (showing the *detected* language for `tr`), clickable source/target boxes (copy that side), and the footer

> ⏎ copies translation · ⇧⏎ opens Google Translate · click a box to copy

<kbd>Enter</kbd> copies the live result; <kbd>Shift</kbd>+<kbd>Enter</kbd> opens Google Translate for long text, alternative phrasings, or when the providers are unreachable.

---

## Where the code lives

| Concern | File |
|---|---|
| Providers, orchestration, parsers | [`core/rust-lib/src/translate.rs`](../core/rust-lib/src/translate.rs) |
| IPC (`translate_text`, off-main via `spawn_blocking`) | [`core/rust-lib/src/commands.rs`](../core/rust-lib/src/commands.rs) |
| Command catalogue + `translateUrl` | [`core/frontend/src/lib/commands.ts`](../core/frontend/src/lib/commands.ts) |
| Debounce / cache / staleness | [`core/frontend/src/App.tsx`](../core/frontend/src/App.tsx) |
| `TranslatePreview` | [`core/frontend/src/components/PreviewPanel.tsx`](../core/frontend/src/components/PreviewPanel.tsx) |

> **Note.** `shazam.rs` also talks to the Google `gtx` endpoint for lyrics translation, with its own parser for a different response shape (paired segments). The two are deliberately separate — one focused parser each, rather than a shared one bent to serve both.

See also: [inline-help.md](./inline-help.md) · [clipboard-shapes.md](./clipboard-shapes.md)
