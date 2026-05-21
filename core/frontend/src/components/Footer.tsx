import { IS_MAC } from "../lib/platform";

interface Props {
  index: number;
  total: number;
  /** App version, e.g. "0.2.6". Rendered as `v0.2.6` next to the counter
   *  when provided. Optional so unit tests don't need a Tauri context. */
  version?: string;
}

export function Footer({ index, total, version }: Props) {
  const label = total === 0 ? "0/0" : `${index + 1}/${total}`;
  // OCR + Screenshot are the most-hidden global shortcuts — they fire
  // from anywhere on the system without needing the popup open.
  // Surfaced in the footer so users discover them without having to dig
  // into the tray menu or Settings → Keyboard shortcuts.
  const ocrKey = IS_MAC ? "⌃⇧O" : "Ctrl+⇧+O";
  const screenshotKey = IS_MAC ? "⌃⇧S" : "Ctrl+⇧+S";
  const colorKey = IS_MAC ? "⌃⇧C" : "Ctrl+⇧+C";
  return (
    <div className="flex h-8 items-center justify-between gap-3 overflow-hidden border-t border-[var(--color-border)] px-4 text-[11px] text-[var(--color-muted)]">
      {/* `shrink-0` + `whitespace-nowrap` so a cramped footer clips at
          the edge instead of wrapping items onto a second line and
          overflowing the fixed `h-8` height. */}
      <div className="flex shrink-0 items-center gap-3 whitespace-nowrap">
        <Hint k="⏎" label="Paste" />
        <Hint k="↑↓" label="Navigate" />
        <Hint k="Esc" label="Close" />
        <Hint k={ocrKey} label="OCR" />
        <Hint k={screenshotKey} label="Shot" />
        <Hint k={colorKey} label="Color" />
      </div>
      <div className="flex shrink-0 items-center gap-3 whitespace-nowrap">
        {/* Shortened from "made with ♥ by Martin Pfeffer" — the full
            credit lives in the title tooltip + the About dialog. */}
        <span title="Made with ♥ by Martin Pfeffer">
          <span className="text-red-400">♥</span> Martin Pfeffer
        </span>
        {version && (
          <span title="Inspector Rust version" className="font-[var(--font-mono)]">
            v{version}
          </span>
        )}
        <span>{label}</span>
      </div>
    </div>
  );
}

function Hint({ k, label }: { k: string; label: string }) {
  return (
    <span className="flex items-center gap-1">
      <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px]">
        {k}
      </kbd>
      <span>{label}</span>
    </span>
  );
}
