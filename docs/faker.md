# `faker` — realistic fake test data (v0.84.270)

Generate realistic test data in any quantity, locale and output format straight
from the search bar, and paste it with one Enter. Backed by the Rust
[`fake`](https://crates.io/crates/fake) crate; the registry
(`core/rust-lib/src/faker/`) is the single source of truth shared by the command
**and** the `{faker:…}` snippet placeholder.

## Grammar

Argument order is irrelevant — tokens are classified, not positional.

```
faker                          → generator catalogue (live sample per row)
faker <gen>                    → default-count values (Settings → Faker)
faker <gen> <n>                → n values (1…10000), newline-separated
faker <gen> <n> --json         → JSON array
faker <gen> <n> --csv          → CSV with header
faker <gen> <n> --sql[=<table>]→ INSERT statements (table defaults to <gen>)
faker <gen> <n> --ts           → TS object-literal array (test fixtures)
faker <gen> @de | @en | @fr …  → locale override for this call
faker <gen> --seed=<u64>       → reproducible output
faker int 1..100               → numeric range
faker tpl "<template>" [n]     → free template (see below)
```

`fake` is an exact alias of `faker`. `1.000` / `1_000` are accepted for `1000`.
An unknown generator surfaces a *did-you-mean* suggestion (fuzzy over
name/alias/description), never a silent match — so typing "fake news" does
nothing destructive. **⌘/Ctrl+R rerolls** (new seed). n is capped at 10000.

- **Enter** on the catalogue list runs the highlighted generator; **Tab / →**
  fills its name into the search bar to keep typing.
- **Enter** on a complete command generates all values → clipboard → pastes into
  the previously-focused app → (optionally) a history entry — the same flow as
  every other power command.
- The right **preview pane** shows the output (a sample window above n=25),
  **format chips** to switch format without re-typing, the **locale** used (with
  an amber *fallback* chip when the generator isn't localised), the **seed**, and
  a **Reroll** button.

## Generators

| Category | Generators |
|---|---|
| Person | `first_name` `last_name` `name` `title` `suffix` `job` `gender` |
| Internet | `email` `free_email` `safe_email` `username` `password` `domain` `url` `ipv4` `ipv6` `mac` `user_agent` |
| Address | `street` `building` `city` `zip` `state` `state_abbr` `country` `country_code` `address` `latlng` |
| Phone | `phone` `cell` |
| Company | `company` `industry` `catchphrase` `buzzword` `profession` |
| Finance | `iban` `bic` `credit_card` `currency_code` `currency_name` `currency_symbol` `bitcoin` |
| Lorem | `word` `words` `sentence` `paragraph` |
| Time | `date` `time` `datetime` `iso8601` `unix` |
| Numbers | `int` `float` `bool` `digit` |
| Misc | `uuid` `isbn` `ean` `barcode` `license_plate` `color` `hex_color` `mime` `filepath` `semver` `hash` |
| Composite | `person` `user` `address_full` `company_full` `order` |

Common aliases: `mail`→email, `plz`→zip, `tel`→phone, `firma`→company,
`cc`→credit_card, `guid`→uuid, `vorname`→first_name, … (type `faker` and fuzzy-
search). **Composites** are the productivity lever: `faker person 50 --csv @de`
puts an importable table of 50 German person records on the clipboard in one
Enter. Within a composite, derived fields are coherent (the email is built from
the generated name).

Arguments: `faker int 1..100`, `faker float 0..1`, `faker words 8`,
`faker date:%d.%m.%Y` (strftime).

## Locales — the honest part

`fake` inherits unspecified data from English via trait defaults, so *every*
generator technically works in *every* locale but silently returns English where
a locale has no data. The registry therefore records the locales that **really**
localise each generator, and an unsupported request **falls back to EN visibly**
(amber chip in the catalogue row and preview) — never a silent English result
dressed up as localised.

The 14 locales fake 5.1 ships: `EN` `DE_DE` `FR_FR` `IT_IT` `PT_BR` `PT_PT`
`NL_NL` `JA_JP` `ZH_CN` `ZH_TW` `AR_SA` `CY_GB` `FA_IR` `TR_TR`. Short flags:
`@de @en @fr @it @pt(=pt_br) @pt_pt @nl @ja @zh(=zh_cn) @zh_tw @ar @cy @fa @tr`.

Real localisation coverage (verified against the fake-rs source, not guessed):

| Generator group | Localised in |
|---|---|
| **Names** (first_name, last_name, name, title, suffix) | all 14 |
| **Address** (street, building, city, zip, state, country, …) | all **except** JA_JP, ZH_TW, AR_SA |
| **Phone** (phone, cell) | all **except** DE_DE, ZH_CN, ZH_TW, AR_SA |
| **Lorem** (word, words) | EN, ZH_CN, FA_IR |
| Everything else (email, company name, job, finance, numbers, uuid, …) | EN / locale-independent |

So `faker street @de` yields German street names, but `faker phone @de` and
`faker company @de` fall back to EN (shown as such). The default locale is
**DE_DE** (Settings → Faker), applied to the command and `{faker:…}` placeholders.

## Output formats

- **plain** — scalars newline-joined; composites as `key: value` blocks.
- **json** — a valid JSON array (numbers/bools stay typed).
- **csv** — header + rows; values containing `,` `"` or newlines are quoted
  (internal `"` doubled). Scalars use the generator name as the single column.
- **sql** — `INSERT INTO <table> (cols) VALUES (…);` per row; strings quoted with
  `''` escaping, numbers/bools bare. `--sql=users` sets the table.
- **ts** — `const data = […]`: an array of string literals (scalars) or objects
  (composites), with quotes/backslashes escaped.

Format chips in the preview re-format the cached values without regenerating.

## Templates

```
faker tpl "{name} <{email}>, {city} — {company}"
faker tpl "INSERT INTO u VALUES ('{uuid}','{first_name}','{email}');" 25
```

Placeholder syntax is identical to the text expander: `{gen}`, `{gen:args}`
(`{int:1..100}`, `{date:%d.%m.%Y}`, `{words:5}`), `{gen@locale}`, and `{{`/`}}`
for literal braces. An unknown placeholder is left verbatim (never a silent
blank).

## Snippet placeholders — `{faker:…}`

Faker values expand at paste time inside snippet bodies, alongside `{date}` /
`{clipboard}` / `{cursor}`. See **[text-expander.md](./text-expander.md)**. The
`faker:` namespace is mandatory; `#label` binds a value (same label ⇒ same value
within one expansion); each expansion is freshly seeded (the command's `--seed`
never leaks in).

## Seeds

Without `--seed`, a random seed is chosen and **shown in the preview** so you can
pin it afterwards. `faker uuid 5 --seed=42` twice ⇒ byte-identical output.

## Caveats

- `faker password` is a **toy** — a seedable PRNG, not cryptographically random.
  For real passwords use **`pwgen`** (CSPRNG).
- `faker iban` / `bic` / `credit_card` produce **syntactically valid but
  fictional** values (IBAN has correct mod-97 check digits) — never a real
  account.
- No network, no telemetry, no file writes (only the standard clipboard/paste).
- Binary cost: bundling `fake` (+ rand 0.10 + the deunicode transliteration
  table + locale data) adds **~3.6 MiB** to the release binary.
