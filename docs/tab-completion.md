# Completion for command arguments

Inspector completes the **arguments** of search-bar commands, not only the
command word. While you type, the rest of the word appears in grey right in
the search field, and a small hint under the field says which word Tab takes
and what it means.

## Suggestions as a list (v0.214.0)

Once you are **in argument territory** — there is a space after the keyword —
a command with enumerated parameters shows **all** its options as rows at the
top of the list, and they take over the whole list (like the `kill`/`meme`
pickers). So **↓ / ↑ walk through the options** instead of dropping into the
clipboard history, and **Enter / Tab / →** accept the highlighted one: the
word is filled into the search bar (a word with further options adds a
trailing space so the next level appears), the popup stays open, and a second
Enter runs the now-complete command.

```text
spotify ␣          →  now · history · play · pause · toggle · …   (↓ to browse)
spotify history ␣  →  heute · gestern · woche · monat · artist · export
wakelock ␣         →  on · off · dark
```

A **bare keyword** (no space yet, e.g. `spotify`) keeps its command row and
the grey ghost — Enter still opens its panel. **Free-value commands** whose
argument is really a number — `timer`, `iris`, `pwgen`, `uuid`, `random`,
`shot` — keep only the grey ghost and never take over the list, so Enter
never overrides a custom value (`iris 5` runs `iris 5`, it is not turned into
`iris 55`).

```text
spotify hi|story            ⇥ Tab  history · Verlauf nach Tagen
spotify p|lay               ⇥ Tab  play · abspielen  +2
spotify histroy             ⇥ Tab  histroy → history · Verlauf nach Tagen
spotify history artist kr|aftwerk   ⇥ Tab  Kraftwerk · 31× gehört
```

## Keys

| Key | What it does |
|---|---|
| **Tab** | takes the suggestion (the grey text, or the corrected word) |
| **Tab** again | cycles to the next alternative (`play` → `pause` → `previous`); the hint shows the next one: `weiter: pause · 2/3` |
| **Shift+Tab** | cycles back (without a cycle: starts at the last alternative) |
| **→** at the end of the line | takes the grey text |
| **Space** | opens the next level: `spotify history ` → `heute`, `woche`, `export` … |

## What is suggested

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
   `histroy → history`. Tab replaces the typed word. Words under three letters
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
the command kind: a list of `{ value, hint, next? }`. `next` leads to the
next level (it may point back to itself, like the combinable history
filters), `{ source, hint }` marks a free-text slot filled from live data.
Lists owned elsewhere (retro palettes, settings sections) are read from their
registries, never copied. The README badge `Tab completion` counts the
entries.
