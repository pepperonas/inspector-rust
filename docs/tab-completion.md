# Tab completion for command arguments

Inspector completes the **arguments** of search-bar commands, not only the
command word. While you type, the rest of the word appears in grey right in
the search field, and a small hint under the field says which word Tab takes
and what it means.

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
