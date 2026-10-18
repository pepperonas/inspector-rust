import { Calculator, ChevronRight, X } from "lucide-react";
import { forwardRef, useCallback, useLayoutEffect, useRef, useState } from "react";
import { touchPaste } from "../lib/touch";
import { translatePaste } from "../lib/commands";
import { useT } from "../lib/i18n";

/** Inline completion state (argument completion, see lib/arg-complete).
 * The choices themselves are rows in the list below — the field only shows
 * the rest of the selected word as grey ghost text. */
export interface CompletionHint {
  /** Grey text after the caret — the rest of the word Tab / → insert. */
  ghost: string;
}

interface Props {
  value: string;
  onChange: (v: string) => void;
  /** When true, swap the input glyph for a calculator icon (calc-mode hint). */
  calcMode?: boolean;
  completion?: CompletionHint | null;
}

export const SearchBar = forwardRef<HTMLInputElement, Props>(
  ({ value, onChange, calcMode, completion }, ref) => {
    const t = useT();
    // Own handle on the input (the forwarded ref belongs to App) — needed to
    // tell whether the text scrolls, where an overlay could not line up.
    const inner = useRef<HTMLInputElement | null>(null);
    const setRefs = useCallback(
      (el: HTMLInputElement | null) => {
        inner.current = el;
        if (typeof ref === "function") ref(el);
        else if (ref) ref.current = el;
      },
      [ref],
    );
    const ghost = completion?.ghost ?? "";
    const [overflowing, setOverflowing] = useState(false);
    useLayoutEffect(() => {
      const el = inner.current;
      setOverflowing(!!el && el.scrollWidth > el.clientWidth + 1);
    }, [value, ghost]);
    return (
      // pr reserves room for the absolute tab strip on the right so the
      // placeholder + value + the clear button never sit under the tabs.
      // The width is measured at runtime by TabBar and published as the
      // `--tab-reserve` CSS var on the header (App.tsx) — no hard-coded
      // number to keep in sync when tabs are added. `260px` is only the
      // pre-measure fallback for the first frame.
      <div
        className={
          "md3-focus-halo relative flex h-14 items-center gap-3 border-b pl-4 pr-[var(--tab-reserve,260px)] transition-colors duration-200 " +
          // Calculator/converter active → highlight the input like a command
          // (reddish accent + bar tint), so it's visually obvious you're computing.
          (calcMode ? "border-rose-500/60 bg-rose-500/5" : "border-[var(--color-border)]")
        }
      >
        {calcMode ? (
          // `key` remounts on calc-activation so the spring icon-pop replays once
          // (not on every keystroke — calcMode stays true while typing).
          <Calculator key="calc-icon" size={18} className="md3-cmd-icon text-rose-500" />
        ) : (
          <ChevronRight size={18} className="text-[var(--color-muted)]" />
        )}
        <div className="relative min-w-0 flex-1">
        {ghost && !overflowing && (
          // The ghost sits under the input: an invisible copy of the typed
          // text pushes the grey rest exactly behind the caret. Same font,
          // no padding on either — they line up as long as nothing scrolls.
          <div
            aria-hidden="true"
            data-testid="completion-ghost"
            className="pointer-events-none absolute inset-0 flex items-center overflow-hidden whitespace-pre text-[15px]"
          >
            <span className="invisible">{value}</span>
            <span className="text-[var(--color-muted)] opacity-70">{ghost}</span>
          </div>
        )}
        <input
          ref={setRefs}
          aria-autocomplete={ghost ? "inline" : undefined}
          type="text"
          autoFocus
          spellCheck={false}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          placeholder={t("search.placeholder")}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onPaste={(e) => {
            // A single-line <input> strips newlines from a paste. Inside the
            // translation text and `touch`/`echo` content they matter, so
            // they're kept as visible ↵ markers and restored by the parsers.
            const el = e.currentTarget;
            const args = [
              el.value,
              el.selectionStart ?? el.value.length,
              el.selectionEnd ?? el.value.length,
              e.clipboardData.getData("text/plain"),
            ] as const;
            const r = translatePaste(...args) ?? touchPaste(...args);
            if (!r) return;
            e.preventDefault();
            onChange(r.value);
            requestAnimationFrame(() => el.setSelectionRange(r.caret, r.caret));
          }}
          className={
            "relative w-full min-w-0 bg-transparent text-[15px] outline-none placeholder:text-[var(--color-muted)] " +
            (calcMode ? "caret-rose-500" : "")
          }
        />
        </div>
        {value && (
          // Clear the whole input. `onMouseDown preventDefault` keeps focus on
          // the field (a plain click would blur it), so typing continues right
          // after clearing. Sits at the right end of the input CONTENT box,
          // which the dynamic `--tab-reserve` keeps a clear gap to the left of
          // the tab strip — so it never lands under/among the tabs and stays
          // comfortably clickable. `p-1.5` gives it a ~28px hit target.
          <button
            type="button"
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => onChange("")}
            title={t("search.clear")}
            aria-label={t("search.clear")}
            className="shrink-0 rounded-md p-1.5 text-[var(--color-muted)] transition-colors hover:bg-[var(--color-border)]/40 hover:text-[var(--color-fg)]"
          >
            <X size={16} />
          </button>
        )}
      </div>
    );
  },
);
SearchBar.displayName = "SearchBar";
