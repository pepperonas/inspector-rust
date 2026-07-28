import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Palette, Pin, Trash2 } from "lucide-react";
import { ColorPickerModal } from "./ColorPickerModal";
import { HistoryItem } from "./HistoryItem";
import { computeLineage, railGutterPx } from "../lib/lineage";
import type { ListEntry } from "../lib/types";

interface Props {
  entries: ListEntry[];
  selectedIndex: number;
  onSelect: (i: number) => void;
  onActivate: (i: number) => void;
  /** Save-as-note handler: invoked on the bookmark icon for a clipboard entry. */
  onSaveAsNote?: (i: number) => void;
  /** Delete handler: invoked on the trash icon for a clipboard entry. */
  onDeleteClip?: (i: number) => void;
  /** Pin/unpin handler: invoked on the pin icon for a clipboard entry. */
  onTogglePin?: (i: number, pinned: boolean) => void;
  /** Clear-all-history handler: invoked from the toolbar button at the top. */
  onClearAll?: () => void;
  /** Draw the lineage rails connecting a derived copy to its source
   *  (v0.93.1). Off = the list renders exactly as before. */
  lineageHighlight?: boolean;
  /** Pinned-only filter (v0.94.0): when true the list shows only pinned clips;
   *  the toolbar toggle reflects/flips it. */
  pinnedOnly?: boolean;
  /** Total number of pinned clips (query-independent) — drives the toggle badge. */
  pinnedCount?: number;
  /** Flip the pinned-only filter. */
  onTogglePinnedOnly?: () => void;
}

const ROW_HEIGHT = 36;

export function HistoryList({
  entries,
  selectedIndex,
  onSelect,
  onActivate,
  onSaveAsNote,
  onDeleteClip,
  onTogglePin,
  onClearAll,
  lineageHighlight = true,
  pinnedOnly = false,
  pinnedCount = 0,
  onTogglePinnedOnly,
}: Props) {
  const parentRef = useRef<HTMLDivElement>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);

  // Lineage rails, computed over the WHOLE list (not just the visible window)
  // so a lane's span is right no matter where the user has scrolled. Non-clip
  // rows go in as `null` to keep positions aligned with the rendered rows.
  const rails = useMemo(() => {
    if (!lineageHighlight) return null;
    return computeLineage(entries.map((e) => (e.kind === "clip" ? e.data : null)));
  }, [entries, lineageHighlight]);

  // One uniform gutter for the whole list so rows never jitter as lanes come
  // and go while scrolling. Capped so a pathological number of concurrent
  // lineages can't eat the row.
  const railGutter = useMemo(() => railGutterPx(rails), [rails]);

  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
  });

  useEffect(() => {
    if (selectedIndex >= 0 && selectedIndex < entries.length) {
      virtualizer.scrollToIndex(selectedIndex, { align: "auto" });
    }
  }, [selectedIndex, virtualizer, entries.length]);

  // When the popup is dismissed (focus loss → hide_popup, or any other
  // path that closes the popup window), tear the modal down so re-opening
  // the popup shows the default History view, not whatever modal happened
  // to be up at hide-time. Note: the screen eyedropper hides the popup
  // via `w.hide()` *directly* — not through `hide_popup` — so it does
  // NOT emit `popup-hidden`, and the modal correctly stays open across
  // a sample.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void listen("popup-hidden", () => setPickerOpen(false)).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  // Number of clipboard entries (the toolbar's "Clear all" only refers to
  // those — snippets and calc rows are virtual and aren't deleted by it).
  const clipCount = useMemo(
    () => entries.reduce((n, e) => (e.kind === "clip" ? n + 1 : n), 0),
    [entries],
  );

  return (
    <div className="flex h-full flex-col">
      {/* Toolbar always rendered — the color picker is always available;
          the Clear All / count controls are conditional within. */}
      <div className="flex shrink-0 items-center justify-between border-b border-[var(--color-border)] px-3 py-1 text-[11px] text-[var(--color-muted)]">
          <div className="flex items-center gap-1">
            {onTogglePinnedOnly && (
              <button
                onClick={onTogglePinnedOnly}
                aria-pressed={pinnedOnly}
                className={
                  "flex items-center gap-1 rounded px-2 py-0.5 " +
                  (pinnedOnly
                    ? "bg-[var(--color-accent)]/15 text-[var(--color-accent)]"
                    : "hover:bg-[var(--color-surface)] hover:text-[var(--color-accent)]")
                }
                title={
                  pinnedOnly
                    ? "Show all clipboard entries"
                    : "Show only pinned clips"
                }
              >
                <Pin size={11} className={pinnedOnly ? "fill-current" : ""} />
                {pinnedOnly
                  ? `${clipCount} pinned`
                  : `Pinned${pinnedCount > 0 ? ` (${pinnedCount})` : ""}`}
              </button>
            )}
            {!pinnedOnly && (
              <span className="pl-1">
                {clipCount} clip{clipCount === 1 ? "" : "s"}
              </span>
            )}
          </div>
          <div className="flex items-center gap-1">
            <button
              onClick={() => setPickerOpen(true)}
              className="flex items-center gap-1 rounded px-2 py-0.5 hover:bg-[var(--color-surface)] hover:text-[var(--color-accent)]"
              title="Open the color picker; the chosen value can be copied in HEX, RGB, or HSL"
            >
              <Palette size={11} />
              Color picker
            </button>
            {onClearAll && clipCount > 0 && (
              confirmClear ? (
                <div className="flex items-center gap-1">
                  <span className="text-[11px] text-red-400">Delete {clipCount} clip{clipCount === 1 ? "" : "s"}?</span>
                  <button
                    onClick={() => { onClearAll(); setConfirmClear(false); }}
                    className="rounded px-2 py-0.5 text-[11px] text-red-400 hover:bg-red-400/10"
                  >
                    Yes
                  </button>
                  <button
                    onClick={() => setConfirmClear(false)}
                    className="rounded px-2 py-0.5 text-[11px] text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
                  >
                    Cancel
                  </button>
                </div>
              ) : (
                <button
                  onClick={() => setConfirmClear(true)}
                  className="flex items-center gap-1 rounded px-2 py-0.5 hover:bg-[var(--color-surface)] hover:text-red-400"
                  title="Delete all clipboard history"
                >
                  <Trash2 size={11} />
                  Clear all
                </button>
              )
            )}
          </div>
        </div>

      {entries.length === 0 ? (
        <div className="flex flex-1 items-center justify-center text-[13px] text-[var(--color-muted)]">
          <span className="md3-empty-float">
            {pinnedOnly ? "No pinned clips" : "No matches"}
          </span>
        </div>
      ) : (
        <div ref={parentRef} className="flex-1 overflow-auto">
          <div
            data-vlist
            style={{
              height: virtualizer.getTotalSize(),
              width: "100%",
              position: "relative",
            }}
          >
            {virtualizer.getVirtualItems().map((virtualRow) => {
              const entry = entries[virtualRow.index];
              const key =
                entry.kind === "snippet"
                  ? `s-${entry.data.id}`
                  : entry.kind === "calc"
                    ? // Stable across keystrokes (no expression in the key): the row
                      // updates in place as you type, so its command-style entrance
                      // + icon-pop fire once when the result appears, not per key.
                      `calc`
                    : entry.kind === "color"
                      ? `color-${entry.data.hex}`
                      : entry.kind === "command"
                        ? // Stable across keystrokes (no `rawInput`): typing an
                          // argument updates the row in place instead of
                          // remounting it, so its entrance/icon-pop animations
                          // fire once when the command appears, not per key.
                          `cmd-${entry.data.commandKind}`
                        : entry.kind === "command-suggestion"
                          ? `sug-${entry.data.keyword}`
                          : entry.kind === "kill-target"
                            ? `kill-${entry.data.pid}`
                            : entry.kind === "opener"
                              ? `opener-${entry.data.text.slice(0, 24)}`
                              : entry.kind === "finder-file"
                                ? `finder-${entry.data.path}`
                                : entry.kind === "bruno"
                                  ? `bruno-${entry.data.yearlyGross}-${entry.data.period}`
                                  : entry.kind === "app"
                                    ? `app-${entry.data.path}`
                                    : entry.kind === "pwgen"
                                      ? `pwgen-${entry.data.length}-${entry.data.mode}`
                                      : entry.kind === "bpm"
                                        ? `bpm-trigger`
                                        : entry.kind === "equalizer"
                                        ? `equalizer-trigger`
                                        : entry.kind === "totp-manage"
                                          ? `totp-manage`
                                          : entry.kind === "totp"
                                            ? `totp-${entry.data.id}`
                                            : entry.kind === "meme"
                                              ? `meme-${entry.data.path}`
                                              : entry.kind === "social"
                                                ? `social`
                                                : entry.kind === "figlet-font"
                                                  ? `fig-${entry.data.name}`
                                                  : entry.kind === "help"
                                                    ? `help-${entry.data.command}`
                                                    : `c-${entry.data.id}`;
              return (
                <HistoryItem
                  rails={
                    entry.kind === "clip" ? rails?.get(entry.data.id) : undefined
                  }
                  railGutter={railGutter}
                  key={key}
                  entry={entry}
                  selected={virtualRow.index === selectedIndex}
                  onClick={() => onSelect(virtualRow.index)}
                  onDoubleClick={() => onActivate(virtualRow.index)}
                  onSaveAsNote={
                    onSaveAsNote ? () => onSaveAsNote(virtualRow.index) : undefined
                  }
                  onTogglePin={
                    onTogglePin
                      ? (pinned) => onTogglePin(virtualRow.index, pinned)
                      : undefined
                  }
                  onDelete={
                    onDeleteClip ? () => onDeleteClip(virtualRow.index) : undefined
                  }
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    height: virtualRow.size,
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                />
              );
            })}
          </div>
        </div>
      )}

      <ColorPickerModal open={pickerOpen} onClose={() => setPickerOpen(false)} />
    </div>
  );
}
