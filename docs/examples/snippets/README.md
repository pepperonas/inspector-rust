# Snippet examples

Ready-to-import JSON files. Each one is a stand-alone, valid input for the **Snippets → Import** picker in Inspector Rust. Pick one as a starting point, edit, and import.

| File | Snippets | Theme |
|------|----------|-------|
| [`getting-started.json`](./getting-started.json) | `mfg`, `addr`, `email` (3) | A minimal **first import** — sample address, email, German "Mit freundlichen Grüßen". Use this to verify the import flow works. |
| [`signatures.json`](./signatures.json) | `sig`, `sigDe`, `sigShort`, `sigOoo` (4) | **Email signatures** — short, long, German variant, and an out-of-office template with `<DATE>` / `<COLLEAGUE>` placeholders. |
| [`dev.json`](./dev.json) | `shebang`, `license`, `todo`, `fixme`, `rsfn`, `tsfn`, `gitignore`, `ccmsg` (8) | **Developer boilerplates** — Bash shebang, MIT header, language-specific function skeletons, `.gitignore`, Conventional-Commits message. |
| [`markdown.json`](./markdown.json) | `mdh1`, `mdtable`, `mdcollapse`, `mdcb`, `prtmpl` (5) | **Markdown / GitHub** — heading scaffold, table skeleton, collapsible `<details>`, TS code-fence, PR-body template. |
| [`wrapped-form.json`](./wrapped-form.json) | `tel`, `vat` (2) | Demonstrates the **wrapped object form** `{ "snippets": [...] }` (instead of a top-level array). Both shapes are accepted by the importer. |

## How to use

1. Open Inspector Rust (`Ctrl+Shift+V`).
2. **Snippets** tab → **Import**.
3. Select one of the files above.
4. The status row shows e.g. `Imported 8` and the new entries appear in the list.

You can import multiple files in sequence — abbreviations are upserted by name, so collisions overwrite the previous version (re-import is idempotent).

## Combining files

`jq` makes it easy to merge several example files into one import:

```bash
# concatenate all bare arrays into one
jq -s 'add' \
  docs/examples/snippets/getting-started.json \
  docs/examples/snippets/signatures.json \
  docs/examples/snippets/dev.json \
  > /tmp/inspector-rust-bundle.json
```

Then import `/tmp/inspector-rust-bundle.json` once.

## See also

- Format reference and semantics: [`docs/snippets-import.md`](../../snippets-import.md)
- Source implementation: [`core/rust-lib/src/snippets.rs`](../../../core/rust-lib/src/snippets.rs)
