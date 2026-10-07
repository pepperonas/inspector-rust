import { useEffect, useRef, useState } from "react";
import { AudioLines, Bluetooth, Check, ChevronUp, Headphones, Monitor, Speaker, Volume2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { audioLabel, type AudioKind, type AudioRouteView } from "../lib/audio-route";

const KIND_ICON: Record<AudioKind, LucideIcon> = {
  speaker: Speaker,
  headphones: Headphones,
  bluetooth: Bluetooth,
  display: Monitor,
  other: Volume2,
};

interface Props {
  view: AudioRouteView;
  onSelect: (id: string) => void;
  onToggleBoom: () => void;
  /** Called when the menu opens — a fresh read (the user may have switched
   *  outputs in the menu bar since the last poll). */
  onOpen?: () => void;
}

/**
 * Footer audio switch (v0.204.0): shows where the sound comes out — with boom
 * on as "boom → device", because the system default is then the virtual boom
 * Audio and would say nothing — and opens a small custom menu upward (the
 * footer is the bottom edge) to pick the output or toggle boom.
 *
 * Picking a device while boom runs re-routes the bridge to it (boom's
 * default-output listener re-bridges), so one list serves both states.
 */
export function AudioMenu({ view, onSelect, onToggleBoom, onOpen }: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const itemsRef = useRef<(HTMLButtonElement | null)[]>([]);
  const Icon = view.viaBoom ? AudioLines : KIND_ICON[view.activeKind];
  const label = audioLabel(view);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    // Focus the active row (else the first) so the arrows work at once.
    const idx = Math.max(0, view.rows.findIndex((r) => r.active));
    itemsRef.current[idx]?.focus();
    return () => document.removeEventListener("mousedown", onDown);
    // Focus only on OPEN — a poll re-render must not yank focus back.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const toggle = () => {
    if (!open) onOpen?.();
    setOpen((o) => !o);
  };

  const onMenuKey = (e: React.KeyboardEvent) => {
    // ⚠️ Stop here: the list navigation and the global Esc-closes-the-popup
    // fallback both listen on window — Esc must close only this menu.
    const items = itemsRef.current.filter((b): b is HTMLButtonElement => !!b);
    const at = items.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
      rootRef.current?.querySelector<HTMLButtonElement>("[data-audio-trigger]")?.focus();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      const step = e.key === "ArrowDown" ? 1 : -1;
      items[(at + step + items.length) % items.length]?.focus();
    } else if (e.key === "Enter" || e.key === " ") {
      // Let the focused button's click run, but keep Enter from pasting a clip.
      e.stopPropagation();
    }
  };

  return (
    <div ref={rootRef} className="relative flex shrink-0">
      <button
        type="button"
        data-audio-trigger
        data-testid="audio-menu"
        aria-haspopup="menu"
        aria-expanded={open}
        title={`Audioausgabe: ${label} — Klick wählt Gerät oder schaltet boom`}
        onClick={toggle}
        className={
          "flex max-w-[180px] items-center gap-1 rounded px-0.5 transition-colors " +
          (open || view.viaBoom
            ? "text-[var(--color-fg)]"
            : "text-[var(--color-muted)] hover:text-[var(--color-fg)]")
        }
      >
        <Icon size={11} aria-hidden className={`shrink-0 ${view.viaBoom ? "text-[var(--color-accent)]" : ""}`} />
        <span className="truncate font-[var(--font-mono)] text-[10px]">{label}</span>
        <ChevronUp
          size={10}
          aria-hidden
          className={`shrink-0 transition-transform duration-(--duration-fast) ease-sharp ${open ? "" : "rotate-180"}`}
        />
      </button>
      {open && (
        <div
          role="menu"
          aria-label="Audioausgabe"
          onKeyDown={onMenuKey}
          className="pop-enter absolute bottom-full left-0 z-50 mb-1.5 min-w-[230px] max-w-[300px] overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] py-1 text-[11px] shadow-lg"
          style={{ transformOrigin: "bottom left" }}
        >
          <div className="px-3 pb-1 pt-0.5 text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
            Ausgabe
          </div>
          {view.rows.map((r, i) => {
            const RowIcon = KIND_ICON[r.kind];
            return (
              <button
                key={r.id}
                ref={(el) => {
                  itemsRef.current[i] = el;
                }}
                type="button"
                role="menuitemradio"
                aria-checked={r.active}
                onClick={() => {
                  setOpen(false);
                  if (!r.active) onSelect(r.id);
                }}
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[var(--color-fg)] outline-none hover:bg-[var(--color-surface)] focus-visible:bg-[var(--color-surface)]"
              >
                <RowIcon size={13} aria-hidden className="shrink-0 text-[var(--color-muted)]" />
                <span className="min-w-0 flex-1 truncate">{r.name}</span>
                {r.active && <Check size={12} aria-hidden className="shrink-0 text-[var(--color-accent)]" />}
              </button>
            );
          })}
          {view.rows.length === 0 && (
            <div className="px-3 py-1.5 text-[var(--color-muted)]">Keine Ausgabegeräte gefunden</div>
          )}
          {view.boomInstalled && (
            <>
              <div className="my-1 border-t border-[var(--color-border)]" />
              <button
                ref={(el) => {
                  itemsRef.current[view.rows.length] = el;
                }}
                type="button"
                role="menuitemcheckbox"
                aria-checked={view.boomEnabled}
                data-testid="audio-menu-boom"
                onClick={() => {
                  setOpen(false);
                  onToggleBoom();
                }}
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[var(--color-fg)] outline-none hover:bg-[var(--color-surface)] focus-visible:bg-[var(--color-surface)]"
              >
                <AudioLines size={13} aria-hidden className="shrink-0 text-[var(--color-muted)]" />
                <span className="min-w-0 flex-1">
                  boom EQ
                  <span className="block text-[10px] text-[var(--color-muted)]">
                    {view.boomEnabled ? "Klang läuft über boom" : "Aus — Ton geht direkt raus"}
                  </span>
                </span>
                {/* State-first switch, like the boom panel's (v0.84.232). */}
                <span
                  aria-hidden
                  className={
                    "relative h-3.5 w-6 shrink-0 rounded-full transition-colors " +
                    (view.boomEnabled ? "bg-[var(--color-accent)]" : "bg-[var(--color-border)]")
                  }
                >
                  <span
                    className={
                      "absolute top-0.5 h-2.5 w-2.5 rounded-full bg-white transition-transform duration-(--duration-fast) ease-sharp " +
                      (view.boomEnabled ? "translate-x-3" : "translate-x-0.5")
                    }
                  />
                </span>
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
