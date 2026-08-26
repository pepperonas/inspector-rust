import { Fragment } from "react";
import { parseInline } from "../lib/inline-md";

/**
 * Renders one line of doc text with its inline Markdown applied (v0.131.0):
 * `` `code` `` → a tinted mono chip, `**bold**` → `<strong>`. Used everywhere a
 * CommandDoc / CommandSpec / feature-extras string reaches the UI — the `?`
 * help, the preview's command box, the list rows and the Features tab — so the
 * registry can stay Markdown (it also generates the README) without users
 * seeing raw `**` and backticks.
 *
 * ⚠️ **No hard-coded colours.** These strings render in four different
 * contexts: the preview (normal fg), muted hint rows, the Features tab, and
 * the SELECTED command row — which is white-on-rose. A fixed `--color-accent`
 * chip would be invisible or clashing there, so the chip derives from
 * `currentColor` (`color-mix`) and bold only sets weight. That way every
 * context, both themes, stays legible with zero per-call-site tuning.
 *
 * Real React nodes — never `dangerouslySetInnerHTML`.
 */
export function InlineMd({ text, className }: { text: string; className?: string }) {
  const tokens = parseInline(text);
  const body = tokens.map((t, i) => {
    if (t.kind === "code") {
      return (
        <code
          key={i}
          className="rounded-[3px] bg-[color-mix(in_srgb,currentColor_14%,transparent)] px-[0.32em] py-[0.06em] font-[var(--font-mono)] text-[0.92em]"
        >
          {t.text}
        </code>
      );
    }
    if (t.kind === "bold") {
      return (
        <strong key={i} className="font-semibold">
          {t.text}
        </strong>
      );
    }
    return <Fragment key={i}>{t.text}</Fragment>;
  });
  return className ? <span className={className}>{body}</span> : <>{body}</>;
}
