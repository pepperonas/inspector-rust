# Completion for command arguments

Inspector completes the **arguments** of search-bar commands, not only the
command word. The options are rows in the list — the one place where you
browse them with ↑/↓. The search field only shows the rest of the selected
word in grey (the "ghost").

## One surface: the list (v0.218.0)

Until v0.217 a small hint floated under the search field (`⇥ Tab play ·
abspielen +2`) and Tab cycled words inside the field, zsh-style. Both are
gone: the hint covered the first list row and repeated what that row already
said, and cycling inside the field showed only one option at a time. The rule
now follows launchers like Raycast and Spotlight and editor completion
(VS Code): **the list shows the options, the arrows choose, the keys act.**

```text
spotify ␣          →  now · history · play · pause · toggle · …
spotify history ␣  →  heute · gestern · woche · monat · artist · export
wakelock ␣         →  on · off · dark
```

Once there is a space after the keyword, a command with enumerated
parameters shows **all** its options as rows that take over the list, so ↓/↑
walk through them instead of dropping into the clipboard history.

## Keys

| Key | What it does |
|---|---|
| **↓ / ↑** | browse the options; the grey ghost follows the selected row |
| **Enter** | takes the selected option. A word with further options descends (`history` → the intervals appear); a word that ends the command **runs it** in the same keystroke (`wakelock ` ↓ Enter = `wakelock off` runs) |
| **Tab** | fills the selected word without running, so the line can still be edited |
| **→** at the end of the line | takes the grey ghost |
| **Tab** on a bare keyword | adds the space: `spotify` + Tab = `spotify ` with the options in the list |
| **Tab** on a free-value command | opens the presets as rows (see below) |
| **Esc** | closes a preset list Tab opened; otherwise closes the popup as before |

A **bare keyword** (no space yet) keeps its own command row, so Enter still
opens its panel.

**Free-value commands**, whose argument is really a number (`timer`, `iris`,
`pwgen`, `uuid`, `random`, `shot`), and a Spotify control that is already
complete (`spotify volume 7`) do not show their presets on their own — Enter
must run the number you typed, not swap it for a preset (`iris 5` stays
`iris 5`). Tab opens the presets as rows on request; typing or Esc closes
them again.

A **typo fix** shows in the row as `histroy → history`; there is no ghost,
because the word is replaced rather than extended.

## What is suggested

### Spotify intervals and complete controls (v0.215.1)

Spotify offers `heute`, `gestern`, `woche`, `monat` and the English
`today`, `yesterday`, `week`, `month` in either interface language. These are
mutually exclusive intervals. Choosing one replaces the previous interval
instead of appending more words. After a complete interval the list shows
every interval, the current one first, so ↓ + Enter switches it. A space
opens further filter/export options again.

For example, `spotify history heute` ↓ Enter becomes
`spotify history gestern`, keeping a single interval. If several intervals
are typed manually, Spotify uses the last one. Artist names remain free-text
values and are not rewritten as intervals.

Complete controls, including aliases (`stop` = `pause`, `vol` = `volume`),
run on Enter even while the Spotify panel is already focused. Numeric
presets do not take over Enter for valid values: `spotify volume 7` runs with
7%, while Tab opens the presets and 75% can be picked explicitly.
See [spotify.md](./spotify.md) for all controls, aliases and interval bounds.

### Available values

1. **Fixed words** of each command (`wakelock on|off|dark`, `spotify play|…`,
   `adb remote|apps|wifi`, `settings <section>` …).
2. **Typical values** where a command takes a number: `spotify volume` 25 / 50
   / 75 / 100, `timer` 5min / 10min / 25min / 1h, `shot` 3 / 5 / 10 s, `pwgen`
   16 / 24 / 32, `uuid`, `rnd`, `iris` thresholds. Any other number still works.
3. **Live data**: `spotify history artist ` offers the artists of your own
   Spotify history, most played first — names with spaces complete as one
   value.
4. **Typo correction**: when nothing matches by prefix, a word one slip away
   (two for long words; a swapped pair counts as one) is offered — shown as
   `histroy → history`. Choosing it replaces the typed word. Words under three letters
   and anything with digits are never "corrected" (`timer 7min` is not a typo
   of `5min`).

## Habits first

Every command you run remembers its argument words, per position
(`spotify history woche` counts `history` at the first position and `woche`
after `history`). Next time, the words you use most come first. Old habits
fade: a use counts half after 14 days. Nothing is ever hidden — only the
order changes. Free text (a search term, an unknown artist) is not remembered.

The memory lives in this Mac's webview storage (`localStorage`,
`ir.arg-usage.v1`), at most 30 words per position. Clearing it just brings
back the registry order.

## Where it applies

Wherever a command line is typed, unless the selected row has its own Tab:
other commands' keyword suggestions, figlet fonts, `bruno`'s mode switch, a
social link's video/audio switch. Commands with their own suggestion rows
(`weather` cities, `cv` units, `otp` issuers) keep them.

## Adding a command

One entry in `ARG_TREES` (`core/frontend/src/lib/arg-complete.ts`), keyed by
the command kind: a list of `{ value, aliases?, hint, next? }`. `next` leads to the
next level (it may point back to itself, like the combinable history
filters), `{ source, hint }` marks a free-text slot filled from live data.
Lists owned elsewhere (retro palettes, settings sections) are read from their
registries, never copied. The README badge `Tab completion` counts the
entries.
