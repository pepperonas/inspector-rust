import { memo, useEffect, useRef, useState } from "react";
import {
  BookOpen, Activity, AudioLines, AppWindow, Bookmark, BookmarkCheck, Calculator, ChevronsRight, Download, Drama, Euro, Flame, FileCode2, FileText, Files, Image, KeyRound, Laugh, Palette, Pin, Skull, Sparkles, StickyNote, Terminal, Trash2, Type, Zap } from "lucide-react";
import { getAppIcon } from "../lib/ipc";
import type { ListEntry } from "../lib/types";
import { LANE_W, derivedKindLabel, visibleRails, type Rail } from "../lib/lineage";
import { formatAbsolute, relativeTime, truncateOneLine } from "../lib/format";
import { InlineMd } from "./InlineMd";
import { platformLabel } from "../lib/social";

interface Props {
  entry: ListEntry;
  selected: boolean;
  onClick: () => void;
  onDoubleClick: () => void;
  /** Save the underlying clipboard entry as a note. Only invoked for `kind: "clip"`. */
  onSaveAsNote?: () => void;
  /** Delete the underlying clipboard entry from history. Only invoked for `kind: "clip"`. */
  onDelete?: () => void;
  /** Pin / unpin the clip (floats to top, never pruned). Only for `kind: "clip"`. */
  onTogglePin?: (pinned: boolean) => void;
  /** Lineage rails crossing this row (v0.93.1) — see `lib/lineage.ts`. */
  rails?: Rail[];
  /** Width reserved for the rail gutter, in px. Uniform across the whole list
   *  (so rows never jitter) and 0 when the rails are off. */
  railGutter?: number;
  style?: React.CSSProperties;
}

/**
 * The git-graph rails on a row's left edge: a thin vertical line per lane the
 * row participates in, plus a dot where the row *is* a member of that lineage
 * (a derived copy or the clip it was made from). Absolutely positioned inside
 * the row's padding — zero layout impact, so the toggle is purely visual.
 */
function LineageRails({ rails, kind }: { rails?: Rail[]; kind?: string | null }) {
  // Only the lanes the list reserved gutter for — otherwise a deep lane would
  // draw over the row's text.
  const drawn = visibleRails(rails);
  if (drawn.length === 0) return null;
  return (
    <span
      aria-hidden
      className="pointer-events-none absolute inset-y-0 left-0"
      title={derivedKindLabel(kind)}
    >
      {drawn.map((r) => (
        <span key={r.lane}>
          <span
            className="absolute inset-y-0 w-[2px] opacity-60"
            style={{ left: 2 + r.lane * LANE_W, background: r.color }}
          />
          {r.node && (
            <span
              className="absolute top-1/2 h-[6px] w-[6px] -translate-y-1/2 rounded-full"
              style={{ left: r.lane * LANE_W, background: r.color }}
            />
          )}
        </span>
      ))}
    </span>
  );
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
  if (entry.kind === "help") return <BookOpen size={size} className={cls} />;
  if (entry.kind === "pwgen") return <KeyRound size={size} className={cls} />;
  if (entry.kind === "bpm") return <Activity size={size} className={cls} />;
  if (entry.kind === "xhype") return <Flame size={size} className={cls} />;
  if (entry.kind === "equalizer") return <AudioLines size={size} className={cls} />;
  if (entry.kind === "totp-manage") return <KeyRound size={size} className={cls} />;
  if (entry.kind === "totp") return <KeyRound size={size} className={cls} />;
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
  if (entry.kind === "meme") return <Laugh size={size} className={cls} />;
  if (entry.kind === "clown") return <Drama size={size} className={cls} />;
  if (entry.kind === "figlet-font") return <Type size={size} className={cls} />;
  if (entry.kind === "social") return <Download size={size} className={cls} />;
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
  rails,
  railGutter = 0,
  onTogglePin,
  style,
}: Props) {
  const [bookmarkSaved, setBookmarkSaved] = useState(false);
  // Track the "Saved!" feedback timer so it can be cleared on unmount —
  // rows unmount frequently in the virtualized list, and a pending timeout
  // would otherwise fire setBookmarkSaved on a dead instance.
  const bookmarkTimerRef = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (bookmarkTimerRef.current !== null) {
        window.clearTimeout(bookmarkTimerRef.current);
      }
    },
    [],
  );
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
  const isTotpManage = entry.kind === "totp-manage";
  const isBruno = entry.kind === "bruno";
  const isHelp = entry.kind === "help";
  const isPwgen = entry.kind === "pwgen";
  const isBpm = entry.kind === "bpm";
  const isXhype = entry.kind === "xhype";
  const isEqualizer = entry.kind === "equalizer";
  const isTotp = entry.kind === "totp";
  const isKillTarget = entry.kind === "kill-target";
  const isMeme = entry.kind === "meme";
  const isClown = entry.kind === "clown";
  const isFiglet = entry.kind === "figlet-font";
  const isSocial = entry.kind === "social";
  // Custom commands get a reddish treatment so the user immediately sees
  // they're about to trigger a command rather than paste a clip / launch an
  // app. This covers EVERY row reached by typing a command keyword —
  // uniformly, including with a parameter (`kill slack`, `meme cat`, …): the
  // generic `command` + its suggestions, the dedicated keyword-command rows
  // (2fa, otp, pwgen, bruno, bpm), AND the whole-list command pickers
  // (kill-target, meme). Only expression results (calc / color, where you type
  // an expression rather than a keyword) and non-command rows (app, finder,
  // clip, snippet, opener) keep the neutral accent.
  const isCustomCommand =
    isCommand ||
    isSuggestion ||
    isHelp ||
    isTotpManage ||
    isBruno ||
    isPwgen ||
    isBpm ||
    isEqualizer ||
    isTotp ||
    isKillTarget ||
    isMeme ||
    isClown ||
    isXhype ||
    // figlet's font gallery is a whole-list takeover exactly like kill/meme —
    // it was left out when `figlet` landed (v0.85.0), so `figlet hello` filled
    // the list with neutral rows and gave no signal you were in a command.
    isFiglet ||
    isSocial ||
    // Calculator / converter results: highlighted + animated like a command
    // (v0.84.27) — typing an expression should feel as "active" as a keyword.
    isCalc;
  const isOpener = entry.kind === "opener";
  const isApp = entry.kind === "app";
  const isFinderFile = entry.kind === "finder-file";
  // A clip carrying a user note → highlighted yellow in the list.
  const isNotedClip = entry.kind === "clip" && !!entry.data.note;
  // Which manipulation produced this clip — the lineage rail's tooltip.
  const lineageKind = entry.kind === "clip" ? entry.data.derived_kind : null;
  // Styled-text clips (HTML/RTF) get a subtle format tag so they're
  // distinguishable from plain text at a glance (plain text shows no tag).
  const styledFormat =
    entry.kind === "clip" &&
    (entry.data.content_type === "html" || entry.data.content_type === "rtf")
      ? entry.data.content_type.toUpperCase()
      : null;

  const label =
    isSnippet
      ? `${entry.data.abbreviation}  ${entry.data.title || entry.data.body.split("\n")[0]}`
      : isMeme && entry.kind === "meme"
        ? entry.data.name
        : isSocial && entry.kind === "social"
          ? `Download from ${platformLabel(entry.data.platform)}`
          : isCalc || isColor || isCommand || isSuggestion || isKillTarget || isOpener || isBruno || isHelp || isApp || isPwgen || isBpm || isEqualizer || isTotpManage || isTotp || isFinderFile || isFiglet || isXhype
            ? ""
            : isClown && entry.kind === "clown"
              ? truncateOneLine(entry.data.output, 80)
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
          ? "bg-white/20 text-white/90"
          : "bg-rose-500/15 text-rose-500")
      }
    >
      cmd
    </span>
  ) : isSuggestion ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/90"
          : "bg-rose-500/15 text-rose-500")
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
  ) : isMeme && entry.kind === "meme" ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/90"
          : "bg-rose-500/15 text-rose-500")
      }
      title="Meme — ⏎ copies it to the clipboard"
    >
      {entry.data.category || "meme"}
    </span>
  ) : isClown && entry.kind === "clown" ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/90"
          : "bg-rose-500/15 text-rose-500")
      }
      title={`${entry.data.name} — ⏎ fügt den Text ein`}
    >
      {entry.data.name}
    </span>
  ) : isBruno ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/90"
          : "bg-rose-500/15 text-rose-500")
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
          ? "bg-white/20 text-white/90"
          : "bg-rose-500/15 text-rose-500")
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
          ? "bg-white/20 text-white/90"
          : "bg-rose-500/15 text-rose-500")
      }
      title="Live BPM detector — listens to the microphone"
    >
      bpm
    </span>
  ) : isEqualizer ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/90"
          : "bg-rose-500/15 text-rose-500")
      }
      title="Live spectrum equalizer — listens to the microphone"
    >
      equalizer
    </span>
  ) : isTotpManage ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/90"
          : "bg-rose-500/15 text-rose-500")
      }
      title="2FA / TOTP management overlay"
    >
      2fa
    </span>
  ) : isTotp ? (
    <span
      className={
        "shrink-0 rounded px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide " +
        (selected
          ? "bg-white/20 text-white/90"
          : "bg-rose-500/15 text-rose-500")
      }
      title="TOTP code — ⏎ copies"
    >
      otp
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
      style={railGutter ? { ...style, paddingLeft: 12 + railGutter } : style}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      className={
        "group relative flex cursor-pointer items-center gap-2 px-3 py-2 text-[13px] " +
        // Custom-command rows fade in when they surface (opacity-only — the row
        // root holds the virtualizer's translateY transform).
        (isCustomCommand ? "md3-cmd-enter " : "") +
        (isCustomCommand
          ? selected
            ? "bg-rose-600 text-white"
            : "bg-rose-500/10 hover:bg-rose-500/20"
          : selected
            ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
            : isNotedClip
              ? "bg-amber-400/15 hover:bg-amber-400/25"
              : "hover:bg-[var(--color-surface)]")
      }
    >
      <LineageRails rails={rails} kind={lineageKind} />
      <span
        className={
          "shrink-0 " +
          // When a command row is selected, its icon does a one-shot spring pop
          // — a small "ready to run" affordance (re-fires each time it's
          // re-selected; the key for `command` rows is stable so typing args
          // doesn't replay it).
          (isCustomCommand && selected ? "md3-cmd-icon " : "") +
          (selected
            ? "text-white/80"
            : isCustomCommand
              ? "text-rose-500"
              : "text-[var(--color-muted)]")
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
              <InlineMd text={entry.data.hint} />
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
              <InlineMd text={entry.data.description} />
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
        ) : isHelp && entry.kind === "help" ? (
          <span className="flex min-w-0 items-baseline gap-2">
            <span className="shrink-0 font-[var(--font-mono)] text-[13px] font-semibold">
              {entry.data.command}
            </span>
            <span
              className={
                "truncate text-[11px] " +
                (selected ? "text-white/80" : "text-[var(--color-muted)]")
              }
            >
              <InlineMd text={entry.data.tagline} />
            </span>
            <span
              className={
                "ml-auto shrink-0 rounded-full px-1.5 text-[10px] " +
                (selected
                  ? "bg-white/20 text-white/90"
                  : "bg-[var(--color-surface)] text-[var(--color-muted)]")
              }
            >
              {entry.data.category}
            </span>
          </span>
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
                // Name the ACTIVE mode and the way to the other one — the
                // switch existed but was invisible (only the `f` suffix).
                const kind = entry.data.self ? "Unternehmer · Gewinn" : "Angestellt · Brutto";
                return `${kind} ${fmt.format(entry.data.yearlyGross)} / Jahr · Abgaben ${pct.format(entry.data.deductionRate)} · ⇥ Modus`;
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
        ) : isXhype && entry.kind === "xhype" ? (
          <span className="flex flex-col">
            <span className="truncate font-semibold">{entry.data.label}</span>
            <span
              className={
                "truncate text-[11px] " +
                (selected ? "text-white/70" : "text-[var(--color-muted)]")
              }
            >
              ⏎ Vollbild · 30 s · sechs Akte ·{" "}
              {entry.data.mode === "news" ? "tagesschau-Schlagzeilen" : "x!! zeigt die News"} ·
              Klick/Taste bricht ab
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
        ) : isEqualizer && entry.kind === "equalizer" ? (
          <span className="flex flex-col">
            <span className="truncate font-semibold">{entry.data.label}</span>
            <span
              className={
                "truncate text-[11px] " +
                (selected ? "text-white/70" : "text-[var(--color-muted)]")
              }
            >
              ⏎ Live mic spectrum · Enter pins · Esc to exit
            </span>
          </span>
        ) : isTotpManage && entry.kind === "totp-manage" ? (
          <span className="flex flex-col">
            <span className="truncate font-semibold">{entry.data.label}</span>
            <span
              className={
                "truncate text-[11px] " +
                (selected ? "text-white/70" : "text-[var(--color-muted)]")
              }
            >
              {entry.data.mode === "add"
                ? "⏎ Opens the add form (Issuer · Login · Secret) · Esc to exit"
                : "⏎ List, Add, Import, Export · Esc to exit"}
            </span>
          </span>
        ) : isTotp && entry.kind === "totp" ? (
          <span className="flex flex-1 items-center gap-3">
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="truncate font-semibold">
                {entry.data.issuer || "(no issuer)"}
              </span>
              {entry.data.account && (
                <span
                  className={
                    "truncate text-[11px] " +
                    (selected ? "text-white/70" : "text-[var(--color-muted)]")
                  }
                >
                  {entry.data.account} · {entry.data.seconds_remaining}s remaining · ⏎ copies code
                </span>
              )}
            </span>
            <span className="shrink-0 font-[var(--font-mono)] text-[16px] font-semibold tabular-nums tracking-[0.1em]">
              {entry.data.code}
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
        ) : isFiglet && entry.kind === "figlet-font" ? (
          // Font name + category, then a compact monospace sample of the text
          // in this font (empty until the batched sample fetch fills it).
          <span className="flex min-w-0 flex-col">
            <span className="flex items-baseline gap-1.5">
              <span className="truncate font-semibold">{entry.data.name}</span>
              <span className={selected ? "text-white/60" : "text-[var(--color-muted)]"}>
                {entry.data.category}
              </span>
              {entry.data.pinned && <span title="Pinned">★</span>}
            </span>
            {entry.data.sample && (
              <pre
                className={
                  "mt-0.5 overflow-hidden whitespace-pre font-[var(--font-mono)] text-[8px] leading-[1.05] " +
                  (selected ? "text-white/80" : "text-[var(--color-muted)]")
                }
              >
                {entry.data.sample}
              </pre>
            )}
          </span>
        ) : (
          label
        )}
      </span>
      {(isCommand || isSuggestion) && selected && (
        // Discoverability: a selected command row hints that `?` opens its
        // full inline help. Subtle, right-aligned, keyboard-cap styled.
        <span
          className="ml-auto shrink-0 rounded border border-white/30 px-1 py-px text-[9px] font-semibold text-white/80"
          aria-label="Press ? for help"
        >
          ? help
        </span>
      )}
      {styledFormat && (
        <span
          title={`Styled ${styledFormat} content`}
          className={
            "shrink-0 rounded px-1 py-px text-[9px] font-semibold uppercase tracking-wider " +
            (selected ? "bg-white/20 text-white/80" : "bg-[var(--color-border)]/60 text-[var(--color-muted)]")
          }
        >
          {styledFormat}
        </span>
      )}
      {isNotedClip && entry.kind === "clip" && entry.data.note && (
        <span
          title={entry.data.note}
          className={"shrink-0 " + (selected ? "text-white/90" : "text-amber-500")}
          aria-label="Has a note"
        >
          <StickyNote size={12} fill="currentColor" />
        </span>
      )}
      {entry.kind === "clip" && onTogglePin && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onTogglePin(!entry.data.pinned);
          }}
          title={entry.data.pinned ? "Unpin" : "Pin to top"}
          className={
            "shrink-0 rounded p-0.5 " +
            (entry.data.pinned
              ? "opacity-100 text-[var(--color-accent)]"
              : "opacity-0 group-hover:opacity-100 " +
                (selected
                  ? "text-white/80 hover:bg-white/20"
                  : "text-[var(--color-muted)] hover:bg-[var(--color-border)] hover:text-[var(--color-accent)]"))
          }
        >
          <Pin size={12} fill={entry.data.pinned ? "currentColor" : "none"} />
        </button>
      )}
      {entry.kind === "clip" && onSaveAsNote && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onSaveAsNote();
            setBookmarkSaved(true);
            if (bookmarkTimerRef.current !== null) {
              window.clearTimeout(bookmarkTimerRef.current);
            }
            bookmarkTimerRef.current = window.setTimeout(
              () => setBookmarkSaved(false),
              1500,
            );
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
