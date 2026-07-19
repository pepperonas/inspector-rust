# `figlet` — ASCII-art banners

Turn text into a FIGlet ASCII-art banner, right in the search bar. Type text and
the selected font's banner renders **live** in the preview (monospace); the left
list is a **font gallery** where each row previews *your* text in that font.
**Enter copies** the selected font's full banner to the clipboard — exactly the
rendered text, newlines and all.

Aliases: **`banner`**, **`ascii`** (identical).

## Grammar

```
figlet <text>                  banner in the default font + the gallery
figlet <text> @<font>          jump to / fuzzy-filter a font  (e.g. @slant, @sla)
figlet                         gallery with a placeholder sample
figlet <text> --width=<n>      wrap width in columns (0 = no wrap)
figlet <text> --center|--right|--left     alignment (default: left)
figlet <text> --box            box-drawing border around the banner
figlet <text> --comment=slashes|hash|block|html   wrap as a source comment
```

Argument order is irrelevant: anything that isn't a `@font` or a known `--flag`
is the banner text. An unknown `--token` is kept as literal text (so
`figlet --> go` renders `--> go`).

## Interacting

- **↑ / ↓** — browse fonts; the big preview updates live.
- **`@name`** — fuzzy-filter the gallery (the primary way to find one of the
  hundreds of fonts). Pinned fonts, then popular ones, come first by default.
- **Tab / →** — fill the selected font as `@font` into the search bar to keep
  tweaking before copying.
- **Enter** — copy the selected font's full banner + paste it where you were
  (and, unless disabled, add it to clipboard history).
- **Shift+Enter** — copy the banner as a **PNG image** instead: tightly cropped
  to the glyphs (blank edge lines, the common indent and trailing whitespace
  are stripped), rendered in the current theme's colours (opaque — readable on
  light *and* dark targets), 2× scale for crispness. Lands on the clipboard and
  in history as an image clip (`[figlet · <text>]`) — for chats/mails that
  mangle monospace text. The preview header shows the hint (`Enter copies ·
  ⇧⏎ PNG · ⌘⇧⏎ transparent`).
- **Cmd/Ctrl+Shift+Enter** — the **transparent** variant, and it does *both*
  outputs at once: the banner is rendered with a **transparent background**
  (only the glyphs, in the current theme's text colour), **saved to
  `~/Downloads`** (`figlet-<text-slug>-<timestamp>.png`, revealed in
  Finder/Explorer) **and copied to the clipboard** (+ history). Ideal for
  overlays/slides — but note a transparent PNG is only visible where the
  target background contrasts with the glyph colour (light theme → dark
  glyphs, dark theme → light glyphs). A figlet PNG already in history can
  also be saved later: every image clip's preview has a **Save to Downloads**
  button (**Cmd/Ctrl+S**), and command-generated images get descriptive
  filenames from their history label (`figlet-hello-…png` instead of a
  generic image name).
- **Option chips** (in the preview) — align, width, trailing-trim, comment-wrap
  and box border, toggled without re-typing. They also apply to what Enter
  copies (both text and PNG).

## Fonts

Hundreds of `.flf` fonts are bundled (compressed, inflated on first use — see
[THIRDPARTY-FONTS](../THIRDPARTY-FONTS) for attribution). The gallery groups a
curated **popular** subset by category (standard · slanted · block · banner ·
script · small · decorative); the long tail is reached with `@name` search.

## Dev extras

- **Comment wrap** — `//`, `#`, `/* */` (JSDoc), `<!-- -->`. Line-prefix styles
  are always valid; block styles guard their closing delimiter so the block
  stays re-parseable. Combine with `--box`: the border is drawn first, then the
  whole block is commented.
- **Trailing-trim** (default on) — removes only trailing spaces per line for a
  clean paste into code; internal alignment is preserved.

## Settings → Figlet

Default font, width, alignment, trailing-trim, comment style, pinned fonts, and
"save results to history" persist and apply without a restart.

## Caveats

- Most FIGlet fonts are **ASCII-only**. Accented/emoji characters are skipped
  with a visible "N characters not available" hint — never a silent loss.
  German umlauts (ÄÖÜäöüß) do render (they're in the required character set).
- **Very wide** banners scroll horizontally in the preview rather than wrapping
  (`--width` wraps on word boundaries — each wrapped row stays a full,
  smushing-intact FIGlet render).

## Architecture note

Rendering lives behind a small `RenderEngine` trait (`core/rust-lib/src/figlet/`)
— FIGlet is the first implementation. A future `boxes`/`cowsay`-style ASCII
engine can plug in behind the same trait (reachable via a reserved `@engine`
selector) without touching the parser, gallery, or layout pipeline.
