import { Calculator, ChevronRight, X } from "lucide-react";
import { forwardRef } from "react";

interface Props {
  value: string;
  onChange: (v: string) => void;
  /** When true, swap the input glyph for a calculator icon (calc-mode hint). */
  calcMode?: boolean;
}

export const SearchBar = forwardRef<HTMLInputElement, Props>(
  ({ value, onChange, calcMode }, ref) => {
    return (
      // pr reserves room for the absolute tab strip on the right so the
      // placeholder + value + the clear button never sit under the tabs.
      // The width is measured at runtime by TabBar and published as the
      // `--tab-reserve` CSS var on the header (App.tsx) — no hard-coded
      // number to keep in sync when tabs are added. `260px` is only the
      // pre-measure fallback for the first frame.
      <div
        className={
          "md3-focus-halo flex h-14 items-center gap-3 border-b pl-4 pr-[var(--tab-reserve,260px)] transition-colors duration-200 " +
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
        <input
          ref={ref}
          type="text"
          autoFocus
          spellCheck={false}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          placeholder="Search or calculate…"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className={
            "min-w-0 flex-1 bg-transparent text-[15px] outline-none placeholder:text-[var(--color-muted)] " +
            (calcMode ? "caret-rose-500" : "")
          }
        />
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
            title="Eingabe löschen"
            aria-label="Eingabe löschen"
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
