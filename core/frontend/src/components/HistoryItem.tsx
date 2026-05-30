import { memo, useEffect, useState } from "react";
import { Activity, AppWindow, Bookmark, BookmarkCheck, Calculator, ChevronsRight, Euro, FileCode2, FileText, Files, Image, KeyRound, Palette, Skull, Sparkles, Terminal, Trash2, Type, Zap } from "lucide-react";
import { getAppIcon } from "../lib/ipc";
import type { ListEntry } from "../lib/types";
import { formatAbsolute, relativeTime, truncateOneLine } from "../lib/format";

interface Props {
  entry: ListEntry;
  selected: boolean;
  onClick: () => void;
  onDoubleClick: () => void;
  /** Save the underlying clipboard entry as a note. Only invoked for `kind: "clip"`. */
  onSaveAsNote?: () => void;
  /** Delete the underlying clipboard entry from history. Only invoked for `kind: "clip"`. */
  onDelete?: () => void;
  style?: React.CSSProperties;
}

function TypeIcon({ entry }: { entry: ListEntry }) {
  const cls = "shrink-0";
  const size = 14;
  if (entry.kind === "snippet") return <Zap size={size} className={cls} />;
  if (entry.kind === "calc") return <Calculator size={size} className={cls} />;
  if (entry.kind === "color") return <Palette size={size} className={cls} />;
  if (entry.kind === "command") return <Terminal size={size} className={cls} />;
  if (entry.kind === "command-suggestion") return <ChevronsRight size={size} className={cls} />;
  if (entry.kind === "kill-target") return <Skull size={size} className={cls} />;
  if (entry.kind === "opener") return <Sparkles size={size} className={cls} />;
  if (entry.kind === "bruno") return <Euro size={size} className={cls} />;
  if (entry.kind === "pwgen") return <KeyRound size={size} className={cls} />;
  if (entry.kind === "bpm") return <Activity size={size} className={cls} />;
  if (entry.kind === "app") {
    return (
      <AppIcon
        path={entry.data.path}
        fallback={<AppWindow size={size} className={cls} />}
      />
    );
  }
  if (entry.kind === "finder-file") {
    return entry.data.is_image
      ? <Image size={size} className={cls} />
      : <Files size={size} className={cls} />;
  }
  switch (entry.data.content_type) {
    case "text":  return <Type size={size} className={cls} />;
    case "image": return <Image size={size} className={cls} />;
    case "files": return <Files size={size} className={cls} />;
    case "html":  return <FileCode2 size={size} className={cls} />;
    case "rtf":   return <FileText size={size} className={cls} />;
  }
}

export const HistoryItem = memo(function HistoryItem({
  entry,
  selected,
  onClick,
  onDoubleClick,
  onSaveAsNote,
  onDelete,
  style,
}: Props) {
  const [bookmarkSaved, setBookmarkSaved] = useState(false);
  // Click on the relative-time chip toggles into the absolute date.
  // Hover always shows both timestamps via the `title` tooltip
  // regardless of toggle state — keeps the affordance discoverable
  // without forcing the user to click first.
  const [showAbsoluteTime, setShowAbsoluteTime] = useState(false);
  const isSnippet = entry.kind === "snippet";
  const isCalc = entry.kind === "calc";
  const isColor = entry.kind === "color";
  const isCommand = entry.kind === "command";
  const isSuggestion = entry.kind === "command-suggestion";
  const isKillTarget = entry.kind === "kill-target";
  const isOpener = entry.kind === "opener";
  const isBruno = entry.kind === "bruno";
  const isApp = entry.kind === "app";
  const isPwgen = entry.kind === "pwgen";
  const isBpm = entry.kind === "bpm";
  const isFinderFile = entry.kind === "finder-file";

  const label =
    isSnippet
      ? `${entry.data.abbreviation}  ${entry.data.title || entry.data.body.split("\n")[0]}`
      : isCalc || isColor || isCommand || isSuggestion || isKillTarget || isOpener || isBruno || isApp || isPwgen || isBpm || isFinderFile
        ? ""
        : truncateOneLine(entry.data.content_text || "(empty)", 80);

  const right = isSnippet ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/80"
          : "bg-[var(--color-accent)]/15 text-[var(--color-accent)]")
      }
    >
      snippet
    </span>
  ) : isCalc ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/80"
          : "bg-[var(--color-accent)]/15 text-[var(--color-accent)]")
      }
    >
      calc
    </span>
  ) : isColor ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/80"
          : "bg-[var(--color-accent)]/15 text-[var(--color-accent)]")
      }
    >
      color
    </span>
  ) : isCommand ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/80"
          : "bg-[var(--color-accent)]/15 text-[var(--color-accent)]")
      }
    >
      cmd
    </span>
  ) : isSuggestion ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/80"
          : "bg-[var(--color-muted)]/15 text-[var(--color-muted)]")
      }
    >
      hint
    </span>
  ) : isKillTarget && entry.kind === "kill-target" ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide tabular-nums " +
        (selected
          ? "bg-white/20 text-white/90"
          : "bg-red-500/15 text-red-500")
      }
      title={entry.data.force ? "SIGKILL — force quit" : "SIGTERM — graceful"}
    >
      {entry.data.force ? "kill -9" : "kill"}
    </span>
  ) : isOpener ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/80"
          : "bg-[var(--color-accent)]/15 text-[var(--color-accent)]")
      }
      title="Random pickup-line — ← / → cycles to the previous / next opener"
    >
      opener
    </span>
  ) : isFinderFile ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/80"
          : "bg-[var(--color-accent)]/15 text-[var(--color-accent)]")
      }
      title="Selected in Finder"
    >
      finder
    </span>
  ) : isBruno ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/80"
          : "bg-[var(--color-accent)]/15 text-[var(--color-accent)]")
      }
      title="Brutto → Netto (Steuerjahr 2025, vereinfacht)"
    >
      bruno
    </span>
  ) : isApp ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/80"
          : "bg-[var(--color-accent)]/15 text-[var(--color-accent)]")
      }
      title="Launch app (Spotlight-like)"
    >
      app
    </span>
  ) : isPwgen ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/80"
          : "bg-[var(--color-accent)]/15 text-[var(--color-accent)]")
      }
      title="Generated password — ⏎ copies, ⌥⏎ switches to alphanumeric + copies"
    >
      pwgen
    </span>
  ) : isBpm ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/80"
          : "bg-[var(--color-accent)]/15 text-[var(--color-accent)]")
      }
      title="Live BPM detector — listens to the microphone"
    >
      bpm
    </span>
  ) : (
    (() => {
      // Past this point `entry.data` is the ClipEntry shape (the other
      // kinds are all branched out above) — the type narrowing got
      // dropped on the implicit closure, so an explicit guard satisfies tsc.
      if (entry.kind !== "clip") return null;
      const captured = formatAbsolute(entry.data.created_at);
      const lastUsed = formatAbsolute(entry.data.last_used_at);
      const sameInstant = entry.data.created_at === entry.data.last_used_at;
      const tooltip = sameInstant
        ? `Captured: ${captured}\n(never re-used since)`
        : `Captured: ${captured}\nLast used: ${lastUsed}`;
      const display = showAbsoluteTime
        ? lastUsed
        : relativeTime(entry.data.last_used_at);
      return (
        <button
          type="button"
          onClick={(e) => {
            // The row's onClick selects the entry; toggling the time
            // shouldn't double-fire that. Stop propagation so a
            // single click on the chip is just a chip-toggle.
            e.stopPropagation();
            setShowAbsoluteTime((v) => !v);
          }}
          title={tooltip}
          className={
            "shrink-0 cursor-pointer rounded text-[11px] tabular-nums " +
            (selected
              ? "text-white/70 hover:text-white"
              : "text-[var(--color-muted)] hover:text-[var(--color-fg)]")
          }
          aria-label={tooltip.replace(/\n/g, " · ")}
        >
          {display}
        </button>
      );
    })()
  );

  return (
    <div
      style={style}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      className={
        "group flex cursor-pointer items-center gap-2 px-3 py-2 text-[13px] " +
        (selected
          ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
          : "hover:bg-[var(--color-surface)]")
      }
    >
      <span
        className={
          "shrink-0 " +
          (selected ? "text-white/80" : "text-[var(--color-muted)]")
        }
      >
        <TypeIcon entry={entry} />
      </span>
      <span className="flex-1 truncate">
        {isSnippet && entry.kind === "snippet" ? (
          <>
            <span className="font-[var(--font-mono)] font-semibold">
              {entry.data.abbreviation}
            </span>
            {(entry.data.title || entry.data.body.split("\n")[0]) && (
              <span className={selected ? "text-white/70" : "text-[var(--color-muted)]"}>
                {"  "}
                {truncateOneLine(entry.data.title || entry.data.body.split("\n")[0], 50)}
              </span>
            )}
          </>
        ) : isCalc && entry.kind === "calc" ? (
          <span className="font-[var(--font-mono)]">
            <span className={selected ? "text-white/70" : "text-[var(--color-muted)]"}>
              {truncateOneLine(entry.data.expression, 40)} ={" "}
            </span>
            <span className="font-semibold">{entry.data.display}</span>
          </span>
        ) : isColor && entry.kind === "color" ? (
          <span className="flex items-center gap-2">
            <span
              className="inline-block h-4 w-4 shrink-0 rounded border border-[var(--color-border)]"
              style={{ backgroundColor: entry.data.hex }}
              aria-hidden
            />
            <span className="font-[var(--font-mono)] font-semibold">
              {entry.data.hex}
            </span>
            <span
              className={
                "truncate text-[11px] " +
                (selected ? "text-white/70" : "text-[var(--color-muted)]")
              }
            >
              {entry.data.rgbString}
            </span>
          </span>
        ) : isCommand && entry.kind === "command" ? (
          <span className="flex flex-col">
            <span className="font-semibold truncate">{entry.data.label}</span>
            <span
              className={
                "truncate text-[11px] " +
                (selected ? "text-white/70" : "text-[var(--color-muted)]")
              }
            >
              {entry.data.hint}
            </span>
          </span>
        ) : isSuggestion && entry.kind === "command-suggestion" ? (
          <span className="flex flex-col">
            <span className="font-[var(--font-mono)]">
              <span className="font-semibold">{entry.data.keyword}</span>
              <span className={selected ? "text-white/60" : "text-[var(--color-muted)]"}>
                {entry.data.syntax.slice(entry.data.keyword.length)}
              </span>
            </span>
            <span
              className={
                "truncate text-[11px] " +
                (selected ? "text-white/70" : "text-[var(--color-muted)]")
              }
            >
              {entry.data.description}
            </span>
          </span>
        ) : isKillTarget && entry.kind === "kill-target" ? (
          <span className="flex flex-col">
            <span className="truncate font-[var(--font-mono)]">
              <span className="font-semibold">{entry.data.name}</span>
              <span className={selected ? "text-white/60" : "text-[var(--color-muted)]"}>
                {"  pid "}
                <span className="tabular-nums">{entry.data.pid}</span>
                {"  ·  "}
                <span className="tabular-nums">{entry.data.memory_mb.toFixed(1)}</span> MB
              </span>
            </span>
            {entry.data.exe && (
              <span
                className={
                  "truncate text-[11px] " +
                  (selected ? "text-white/70" : "text-[var(--color-muted)]")
                }
              >
                {entry.data.exe}
              </span>
            )}
          </span>
        ) : isOpener && entry.kind === "opener" ? (
          // Whole opener text — they're short (<200 chars) so a single
          // truncated line reads well without an extra hint row.
          <span className="truncate italic">{entry.data.text}</span>
        ) : isBruno && entry.kind === "bruno" ? (
          <span className="flex flex-col">
            <span className="font-semibold">
              {(() => {
                const fmt = new Intl.NumberFormat("de-DE", {
                  style: "currency",
                  currency: "EUR",
                  maximumFractionDigits: 0,
                });
                const v = entry.data.period === "monthly"
                  ? entry.data.netMonth
                  : entry.data.netYear;
                const label = entry.data.period === "monthly"
                  ? "/ Monat netto"
                  : "/ Jahr netto";
                return `${fmt.format(v)} ${label}`;
              })()}
            </span>
            <span
              className={
                "truncate text-[11px] " +
                (selected ? "text-white/70" : "text-[var(--color-muted)]")
              }
            >
              {(() => {
                const fmt = new Intl.NumberFormat("de-DE", {
                  style: "currency",
                  currency: "EUR",
                  maximumFractionDigits: 0,
                });
                const pct = new Intl.NumberFormat("de-DE", {
                  style: "percent",
                  maximumFractionDigits: 1,
                });
                return `Brutto ${fmt.format(entry.data.yearlyGross)} / Jahr · Abgaben ${pct.format(entry.data.deductionRate)}`;
              })()}
            </span>
          </span>
        ) : isPwgen && entry.kind === "pwgen" ? (
          <span className="flex flex-col">
            <span className="truncate font-[var(--font-mono)] font-semibold">
              {entry.data.password}
            </span>
            <span
              className={
                "truncate text-[11px] " +
                (selected ? "text-white/70" : "text-[var(--color-muted)]")
              }
            >
              {entry.data.length} chars · {entry.data.mode} · ⏎ copy · ⌥⏎ alnum
            </span>
          </span>
        ) : isBpm && entry.kind === "bpm" ? (
          <span className="flex flex-col">
            <span className="truncate font-semibold">{entry.data.label}</span>
            <span
              className={
                "truncate text-[11px] " +
                (selected ? "text-white/70" : "text-[var(--color-muted)]")
              }
            >
              ⏎ Listen to mic + detect BPM live · Esc to exit
            </span>
          </span>
        ) : isApp && entry.kind === "app" ? (
          <span className="flex flex-col">
            <span className="truncate font-semibold">{entry.data.name}</span>
            <span
              className={
                "truncate text-[11px] " +
                (selected ? "text-white/70" : "text-[var(--color-muted)]")
              }
            >
              ⏎ Launch · {entry.data.path}
            </span>
          </span>
        ) : isFinderFile && entry.kind === "finder-file" ? (
          <span className="flex flex-col">
            <span className="truncate font-semibold">{entry.data.name}</span>
            <span
              className={
                "truncate text-[11px] " +
                (selected ? "text-white/70" : "text-[var(--color-muted)]")
              }
            >
              {entry.data.path}
              {entry.data.size_bytes != null && (
                <>
                  {" · "}
                  <span className="tabular-nums">
                    {(entry.data.size_bytes / 1024).toFixed(1)}
                  </span>
                  {" KB"}
                </>
              )}
            </span>
          </span>
        ) : (
          label
        )}
      </span>
      {entry.kind === "clip" && onSaveAsNote && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onSaveAsNote();
            setBookmarkSaved(true);
            setTimeout(() => setBookmarkSaved(false), 1500);
          }}
          title={bookmarkSaved ? "Saved!" : "Save as note"}
          className={
            "shrink-0 rounded p-0.5 " +
            (bookmarkSaved
              ? "opacity-100 text-[var(--color-accent)]"
              : "opacity-0 group-hover:opacity-100 " +
                (selected
                  ? "text-white/80 hover:bg-white/20"
                  : "text-[var(--color-muted)] hover:bg-[var(--color-border)] hover:text-[var(--color-accent)]"))
          }
        >
          {bookmarkSaved ? <BookmarkCheck size={12} /> : <Bookmark size={12} />}
        </button>
      )}
      {entry.kind === "clip" && onDelete && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onDelete();
          }}
          title="Delete entry from history"
          className={
            "shrink-0 rounded p-0.5 opacity-0 group-hover:opacity-100 " +
            (selected
              ? "text-white/80 hover:bg-white/20"
              : "text-[var(--color-muted)] hover:bg-[var(--color-border)] hover:text-red-400")
          }
        >
          <Trash2 size={12} />
        </button>
      )}
      {right}
    </div>
  );
});

/**
 * Lazy-loads the macOS app icon for the currently-selected app row.
 * Triggers a single `get_app_icon` IPC the first time the component
 * mounts; the backend caches the result, so a re-mount (e.g.
 * re-selecting the same app after navigating away) returns instantly.
 *
 * Sized to match the row's `TypeIcon` (14 px) so the row layout
 * doesn't jump when the icon arrives. Until the IPC resolves we
 * render `null` — the surrounding `<TypeIcon>` already drew the
 * generic `<AppWindow>` lucide icon, so there's no visual gap.
 */
function AppIcon({ path, fallback }: { path: string; fallback: React.ReactNode }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    void getAppIcon(path)
      .then((b64) => {
        if (!cancelled) setSrc(`data:image/png;base64,${b64}`);
      })
      .catch(() => {
        // Failed extraction (rare — apps without standard .icns).
        // Stick with the lucide fallback.
      });
    return () => {
      cancelled = true;
    };
  }, [path]);
  if (!src) return <>{fallback}</>;
  return (
    <img
      src={src}
      alt=""
      aria-hidden
      className="h-3.5 w-3.5 shrink-0 rounded-sm"
      draggable={false}
    />
  );
}
