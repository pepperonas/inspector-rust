/**
 * Minimal inline-Markdown tokenizer for doc text shown in the UI (v0.131.0).
 *
 * The CommandDoc registry, `feature-extras` notes and the `CommandSpec`
 * descriptions are written in light Markdown — `` `code` `` (177 spans) and
 * `**bold**` (7) — because the SAME strings also generate the README. The UI
 * used to render them raw, so users saw literal `**Info**` and backticks in
 * the preview. This turns them into tokens the `<InlineMd>` component renders
 * as real `<strong>` / `<code>` nodes (never `dangerouslySetInnerHTML`).
 *
 * Deliberately supports EXACTLY two constructs:
 *
 *  - **Code spans win** (CommonMark precedence): their content is never parsed
 *    further, which is what protects glob patterns like `src/**‌/*.ts` and any
 *    `**` inside backticks from being read as emphasis.
 *  - **`**bold**` only outside code**, and only when the content is non-empty
 *    and doesn't start/end with whitespace (so `a ** b` stays literal).
 *
 * NOT supported, on purpose: `*italic*` / `_italic_`. A repo scan found a real
 * caveat reading "PowerShell *function*" — a lone-star rule would silently
 * italicise it (and would be a live hazard for glob text). Links, headings and
 * lists never appear in these one-line strings.
 *
 * INVARIANT: re-serialising the tokens (with their markers) reproduces the
 * input exactly — an unclosed or unmatched marker stays literal, so no text
 * can ever be swallowed. Pinned by `reserialize` in the tests.
 */

export type InlineToken =
  | { kind: "text"; text: string }
  | { kind: "bold"; text: string }
  | { kind: "code"; text: string };

/** True when a `**…**` body is emphasis-worthy per CommonMark's flanking rule
 *  (non-empty, no leading/trailing whitespace). */
function isBoldBody(body: string): boolean {
  return body.length > 0 && !/^\s/.test(body) && !/\s$/.test(body);
}

/**
 * Tokenize one line of doc text. Pure; never throws; adjacent plain runs are
 * merged so the output has no empty or split text tokens.
 */
export function parseInline(input: string): InlineToken[] {
  const out: InlineToken[] = [];
  let plain = "";
  const flush = () => {
    if (plain) {
      out.push({ kind: "text", text: plain });
      plain = "";
    }
  };

  let i = 0;
  while (i < input.length) {
    const c = input[i];

    // ── Code span: highest precedence, content is literal. ──
    if (c === "`") {
      const end = input.indexOf("`", i + 1);
      const body = end === -1 ? "" : input.slice(i + 1, end);
      if (end !== -1 && body.length > 0) {
        flush();
        out.push({ kind: "code", text: body });
        i = end + 1;
        continue;
      }
      // Unclosed (or empty ``) → literal backtick.
      plain += c;
      i += 1;
      continue;
    }

    // ── Bold: `**…**`, outside code only. ──
    if (c === "*" && input[i + 1] === "*") {
      const end = input.indexOf("**", i + 2);
      if (end !== -1) {
        const body = input.slice(i + 2, end);
        // A body containing `*` (e.g. a glob) is not emphasis — keep literal.
        if (isBoldBody(body) && !body.includes("*")) {
          flush();
          out.push({ kind: "bold", text: body });
          i = end + 2;
          continue;
        }
      }
      // No valid pair → both stars stay literal (glob-safe).
      plain += "**";
      i += 2;
      continue;
    }

    plain += c;
    i += 1;
  }
  flush();
  return out;
}

/** True when the text contains anything this tokenizer would format — lets a
 *  caller skip the component entirely for the (common) plain case. */
export function hasInlineMarkup(input: string): boolean {
  return parseInline(input).some((t) => t.kind !== "text");
}

/** Re-serialise tokens back to Markdown. Exists for the round-trip test that
 *  pins the no-text-loss invariant. */
export function reserialize(tokens: readonly InlineToken[]): string {
  return tokens
    .map((t) => (t.kind === "code" ? `\`${t.text}\`` : t.kind === "bold" ? `**${t.text}**` : t.text))
    .join("");
}

/** Plain-text projection (markers dropped) — for `title=` tooltips and any
 *  place that needs a string rather than nodes. */
export function stripInlineMarkup(input: string): string {
  return parseInline(input)
    .map((t) => t.text)
    .join("");
}
