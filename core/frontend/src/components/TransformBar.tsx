// The string-manipulation toolbar of the preview, split out of PreviewPanel
// so it is loaded lazily: it only shows for text entries, and PreviewPanel
// itself sits in the start-up bundle (open path) — see check-bundle.mjs.
import { useEffect, useRef, useState } from "react";
import { Check, Type, X } from "lucide-react";
import { commitTransformedText } from "../lib/ipc";
import { TRANSFORMS, applyTransform, feedbackPreview, transformLabel, type TransformKind } from "../lib/text-transform";
import { useModifierHeld } from "../hooks/useModifierHeld";
import { IS_MAC } from "../lib/platform";

/** String-manipulation toolbar for the selected *text* entry. Each chip
 *  applies a transform from `lib/text-transform.ts`; the result is
 *  committed to the clipboard + a new History entry via
 *  `commit_transformed_text`. The first nine transforms also bind to
 *  `Cmd/Ctrl+1…9` while a text entry is selected.
 *
 *  **The chip UI is only rendered while Cmd / Ctrl is held** (the same
 *  modifier the digit shortcuts fire on). Without the modifier, the
 *  preview pane is the full content; press + hold the modifier to see
 *  which digit triggers which transform. The keyboard handler itself
 *  is always mounted so the shortcuts still fire even if the user
 *  doesn't bother peeking at the chip overlay. */
/** How long the "copied" snackbar stays. */
const FLASH_MS = 1800;

export function TransformBar({
  text,
  sourceId,
  initiallyOpen = false,
}: {
  text: string;
  sourceId?: number;
  /** Show the chips straight away (the typed-text row exists to transform). */
  initiallyOpen?: boolean;
}) {
  const modHeld = useModifierHeld();
  // Clicking the hint keeps the chips open without holding the modifier — the
  // options stay reachable for mouse users (and while reading a long label).
  const [pinned, setPinned] = useState(initiallyOpen);

  // Short "copied" feedback. The digit shortcuts work without the chips
  // being visible, so without this a transform was a completely silent
  // clipboard write — you couldn't tell whether ⌘1 did anything.
  const [flash, setFlash] = useState<{ ok: boolean; label: string; preview: string; n: number } | null>(null);
  const flashTimer = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (flashTimer.current !== null) window.clearTimeout(flashTimer.current);
    },
    [],
  );
  const showFlash = (ok: boolean, label: string, preview: string) => {
    setFlash((f) => ({ ok, label, preview, n: (f?.n ?? 0) + 1 }));
    if (flashTimer.current !== null) window.clearTimeout(flashTimer.current);
    flashTimer.current = window.setTimeout(() => setFlash(null), FLASH_MS);
  };

  const run = async (kind: TransformKind) => {
    const result = applyTransform(kind, text);
    try {
      // The result is a NEW entry at the top; `sourceId`/`kind` record where it
      // came from so the list can draw the lineage rail back to the original.
      await commitTransformedText(result, sourceId, kind);
      showFlash(true, transformLabel(kind), feedbackPreview(result));
    } catch (e) {
      console.error("transform commit failed", e);
      showFlash(false, transformLabel(kind), "Clipboard write failed");
    }
  };

  const feedback = flash && (
    <div
      key={flash.n}
      role="status"
      aria-live="polite"
      className={
        "md3-success-pop pointer-events-none fixed bottom-12 left-1/2 z-50 flex max-w-[80%] -translate-x-1/2 items-center gap-2 rounded-full px-3 py-1.5 text-[11px] shadow-lg " +
        (flash.ok
          ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
          : "bg-rose-600 text-white")
      }
    >
      {flash.ok ? <Check size={12} className="shrink-0" /> : <X size={12} className="shrink-0" />}
      <span className="shrink-0 font-semibold">{flash.ok ? `Copied · ${flash.label}` : flash.label}</span>
      <span className="truncate font-[var(--font-mono)] opacity-80">{flash.preview}</span>
    </div>
  );

  // Cmd/Ctrl+1…9 → the digit-bound transforms. Digits alone can't be
  // used (they'd type into the search bar); Cmd/Ctrl+digit is the same
  // CmdOrCtrl pattern as ⌘B / ⌘S.
  //
  // Cmd/Ctrl+^ → "Plain text" (strip HTML / RTF styling). Accepts
  // either Shift state since `^` requires Shift on US layouts
  // (Shift+6) but is a bare keypress on German ISO. Only Alt is
  // rejected to leave the German Alt+^ free for whatever the OS
  // might map it to.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      if (e.key === "^") {
        e.preventDefault();
        void run("plain-text");
        return;
      }
      // Digit shortcut path: reject shift so Shift+digit (which types
      // !@#$… on US) doesn't trigger a transform.
      if (e.shiftKey) return;
      if (!/^[1-9]$/.test(e.key)) return;
      const spec = TRANSFORMS.find((t) => t.digit === Number(e.key));
      if (!spec) return;
      e.preventDefault();
      void run(spec.kind);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // `text` is the only thing `run` closes over that changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text]);

  const mod = IS_MAC ? "⌘" : "Ctrl+";
  const modKey = IS_MAC ? "⌘" : "Ctrl";

  // Until the modifier goes down the chips are hidden — which used to mean the
  // whole feature was invisible. Show what unlocks it instead of nothing; the
  // hint doubles as a button so the options are reachable by mouse too.
  if (!modHeld && !pinned) {
    return (
      <>
      {feedback}
      <button
        type="button"
        onClick={() => setPinned(true)}
        title="Copy this entry in another shape — plain text, UPPERCASE, Base64, …"
        className="md3-press mt-2 flex shrink-0 items-center gap-1.5 self-start rounded-lg border border-dashed border-[var(--color-border)] px-2 py-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
      >
        <Type size={11} />
        <span>
          Hold{" "}
          <kbd className="rounded bg-[var(--color-surface)] px-1 font-[var(--font-mono)] text-[9px] normal-case">
            {modKey}
          </kbd>{" "}
          for formatting options
        </span>
      </button>
      </>
    );
  }

  return (
    <div className="mt-2 shrink-0 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2">
      {feedback}
      <div className="mb-1.5 flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">
        <Type size={12} className="text-[var(--color-accent)]" />
        Transform → new entry + clipboard
        {pinned && (
          <button
            type="button"
            onClick={() => setPinned(false)}
            title="Hide the formatting options"
            aria-label="Hide the formatting options"
            className="ml-auto rounded px-1 text-[12px] leading-none hover:text-[var(--color-accent)]"
          >
            ×
          </button>
        )}
      </div>
      <div className="flex flex-wrap gap-1.5">
        {TRANSFORMS.map((t) => {
          // Special-case the plain-text transform — it carries no digit
          // but is bound to Cmd/Ctrl+^ via the keyboard handler above.
          const badge =
            t.digit != null
              ? `${mod}${t.digit}`
              : t.kind === "plain-text"
                ? `${mod}^`
                : null;
          return (
            <button
              key={t.kind}
              onClick={() => void run(t.kind)}
              title={badge ?? undefined}
              className="md3-press flex items-center gap-1 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[11px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
            >
              {badge && (
                <kbd className="rounded bg-[var(--color-surface)] px-1 font-[var(--font-mono)] text-[9px] text-[var(--color-muted)]">
                  {badge}
                </kbd>
              )}
              {t.label}
            </button>
          );
        })}
      </div>
    </div>
  );
}
