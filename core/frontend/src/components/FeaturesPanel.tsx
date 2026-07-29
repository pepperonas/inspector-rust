import { useEffect, useState } from "react";
import { Gamepad2, Keyboard, MousePointerClick, Search, Terminal } from "lucide-react";
import {
  getAutoExpandConfig,
  getDirectSlots,
  getExpanderConfig,
  getPopupHotkey,
  getHistoryHotkey,
  type AutoExpandConfig,
  type DirectSlot,
} from "../lib/ipc";
import { formatHotkey } from "../lib/platform";
import { COMMAND_DOCS } from "../lib/commandDocs";
import {
  NON_COMMAND_FEATURES,
  HIDDEN_TRIGGER_FEATURES,
  IN_POPUP_ACTIONS,
  HIDDEN_GAMES,
} from "../lib/feature-extras";

/**
 * Features tab — a read-only, tabular catalogue of everything the app can
 * do, with each function's *currently configured* shortcut / trigger and a
 * short note on how to invoke it (only where that isn't self-evident).
 *
 * Configurable hotkeys (popup, abbreviation expander, direct snippet slots,
 * input-lock chord) are fetched live on mount — the panel remounts every
 * time the user switches to this tab, so the values are always current.
 * The fixed global hotkeys (OCR / screenshot / colour / Finder / md→PDF)
 * are literal constants that mirror their `hotkey.rs` registration.
 */

interface Row {
  /** Human name of the feature. */
  name: string;
  /** Pre-formatted shortcut or the literal text the user types. */
  trigger: string;
  /** True → render `trigger` as a typed-command chip, not a key chip. */
  typed?: boolean;
  /** Short "how to use" note; omitted when the trigger speaks for itself. */
  note?: string;
}

interface Section {
  title: string;
  icon: React.ReactNode;
  /** Optional one-line intro shown under the heading. */
  blurb?: string;
  rows: Row[];
}


export function FeaturesPanel() {
  const [popupHotkey, setPopupHotkey] = useState("Ctrl+Space");
  const [historyHotkey, setHistoryHotkey] = useState("");
  const [expander, setExpander] = useState<{ enabled: boolean; hotkey: string }>({
    enabled: false,
    hotkey: "Alt+Digit1",
  });
  const [slots, setSlots] = useState<DirectSlot[]>([]);
  const [autoExpand, setAutoExpand] = useState<AutoExpandConfig | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const [p, e, s, ae, h] = await Promise.all([
          getPopupHotkey(),
          getExpanderConfig(),
          getDirectSlots(),
          getAutoExpandConfig(),
          getHistoryHotkey(),
        ]);
        if (!alive) return;
        setPopupHotkey(p);
        setHistoryHotkey(h);
        setExpander({ enabled: e.enabled, hotkey: e.hotkey });
        setSlots(s);
        setAutoExpand(ae);
      } catch {
        /* leave defaults — backend unreachable (e.g. browser-only test). */
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  // Every registered power command, straight from the canonical CommandDoc
  // registry — so this catalogue can never drift from what actually exists
  // (the same source generates the README command matrix). Deep detail lives in
  // the inline `?` help; here we show the grammar + one-line tagline.
  const commandDocRows: Row[] = COMMAND_DOCS.map((d) => ({
    name: d.command,
    trigger: d.synopsis,
    typed: true,
    note: d.aliases.length > 0 ? `${d.tagline} (alias: ${d.aliases.join(", ")})` : d.tagline,
  }));

  const sections: Section[] = [
    {
      title: "Global hotkeys",
      icon: <Keyboard size={14} />,
      blurb: "Work in any app, even when the popup is closed.",
      rows: [
        {
          name: "Open app / clipboard history",
          trigger: formatHotkey(popupHotkey),
          note: "Toggle the main popup (clipboard history, search & commands). Configurable in Settings → Popup hotkey.",
        },
        ...(historyHotkey.trim()
          ? [
              {
                name: "Clipboard history (2nd hotkey)",
                trigger: formatHotkey(historyHotkey),
                note: "Second, optional shortcut that also opens the clipboard history. Configurable in Settings → Clipboard-history hotkey.",
              },
            ]
          : []),
        {
          name: "OCR region",
          trigger: formatHotkey("Ctrl+Shift+O"),
          note: "Drag a marquee → recognised text lands on the clipboard.",
        },
        {
          name: "Screenshot region",
          trigger: formatHotkey("Ctrl+Shift+S"),
          note: "Drag a marquee → PNG to clipboard + a floating preview to save/edit.",
        },
        {
          name: "Pick colour (eyedropper)",
          trigger: formatHotkey("Ctrl+Shift+C"),
          note: "Loupe to sample any on-screen pixel; hex → clipboard.",
        },
        {
          name: "Screen recording",
          trigger: formatHotkey("Ctrl+Shift+Alt+S"),
          note: "Drag a region → pick audio (system / mic / none) → 3 s countdown → MP4 to Downloads. Floating bar with pause/resume + stop; needs ffmpeg.",
        },
        {
          name: "Replace / overlay audio",
          trigger: formatHotkey("Ctrl+Shift+Alt+M"),
          note: "Select a video in Finder → overlay opens. Replace or mix in a local audio file or a yt-dlp'd YouTube track at a chosen start position; saves a sibling -audioswap.mp4. Needs ffmpeg (+ yt-dlp for YouTube).",
        },
        {
          name: "Finder selection",
          trigger: formatHotkey("Ctrl+Shift+F"),
          note: "Pull the files selected in Finder into the popup (resize / optimise / cut-out).",
        },
        {
          name: "Markdown → PDF",
          trigger: formatHotkey("Ctrl+Shift+M"),
          note: "Convert the .md files selected in Finder to PDF beside the source.",
        },
        {
          name: "Timesheet",
          trigger: formatHotkey("Ctrl+Shift+T"),
          note: "Open the time-tracking overview (the Timesheet tab). Doesn't start/stop tracking — use `track on` / `track off` for that. macOS.",
        },
        {
          name: "Abbreviation expander",
          trigger: formatHotkey(expander.hotkey),
          note: expander.enabled
            ? "Type an abbreviation anywhere (incl. terminals), press the hotkey → it expands. Backed by the keystroke buffer."
            : "Disabled — enable it in Settings. Then type an abbreviation and press the hotkey (works everywhere, incl. terminals).",
        },
        {
          name: "Auto-Expansion (aText-Stil)",
          trigger: "automatic",
          note: autoExpand?.enabled
            ? `On — snippets expand while you type (${autoExpand.trigger === "immediate" ? "immediate" : "after a delimiter"}), no hotkey. Configure in Settings.`
            : "Off — enable in Settings to expand snippets automatically as you type, in any app.",
        },
        ...(slots.length
          ? slots.map((s) => ({
              name: `Direct snippet — ${s.title ?? s.abbreviation ?? `#${s.snippet_id}`}`,
              trigger: formatHotkey(s.hotkey),
              note: "Press to paste the snippet's body anywhere (works in terminals too).",
            }))
          : [
              {
                name: "Direct snippet slots",
                trigger: "—",
                note: "None bound. Add hotkey → snippet bindings in Settings.",
              },
            ]),
      ],
    },
    {
      title: "Search-bar commands",
      icon: <Terminal size={14} />,
      blurb: "Type these into the popup's search field. Add ? to any command (or type ? alone) for full inline help — arguments, examples, tips.",
      rows: [
        // Non-command features (calculator / converters / clip detection) — not
        // power commands, so not in the CommandDoc registry (see feature-extras).
        ...NON_COMMAND_FEATURES,
        // Every power command, from the canonical registry (see commandDocRows).
        ...commandDocRows,
        // Hidden triggers — intentionally NOT in the registry/autocomplete.
        ...HIDDEN_TRIGGER_FEATURES,
      ],
    },
    {
      title: "In-popup & preview actions",
      icon: <MousePointerClick size={14} />,
      rows: IN_POPUP_ACTIONS,
    },
    {
      title: "Hidden games",
      icon: <Gamepad2 size={14} />,
      blurb: "Type the exact word into the search field. Esc suspends & resumes; each keeps its own high score.",
      rows: HIDDEN_GAMES,
    },
  ];

  const q = query.trim().toLowerCase();
  const filteredSections = q
    ? sections
        .map((sec) => ({
          ...sec,
          rows: sec.rows.filter(
            (r) =>
              r.name.toLowerCase().includes(q) ||
              r.trigger.toLowerCase().includes(q) ||
              (r.note ?? "").toLowerCase().includes(q),
          ),
        }))
        .filter((sec) => sec.rows.length > 0)
    : sections;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* Fixed search header — kept OUT of the scroll area so it can never
          overlap content while scrolling. */}
      <div className="border-b border-[var(--color-border)] px-6 pb-3 pt-6">
        <div className="mx-auto w-full max-w-3xl">
          <div className="relative">
            <Search
              size={14}
              className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-[var(--color-muted)]"
            />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search features…"
              className="w-full rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] py-1.5 pl-8 pr-3 text-[12px] outline-none focus:border-[var(--color-accent)]"
            />
          </div>
        </div>
      </div>

      {/* Scrolling content */}
      <div className="min-h-0 flex-1 overflow-auto px-6 py-6">
        <div className="mx-auto w-full max-w-3xl space-y-7">
        {filteredSections.length === 0 && (
          <p className="text-center text-[12px] text-[var(--color-muted)]">
            No features match “{query}”.
          </p>
        )}
        {filteredSections.map((sec) => (
          <section key={sec.title}>
            <div className="mb-2 flex items-center gap-2 text-[var(--color-fg)]">
              <span className="text-[var(--color-accent)]">{sec.icon}</span>
              <h2 className="text-[13px] font-semibold uppercase tracking-[0.12em]">
                {sec.title}
              </h2>
            </div>
            {sec.blurb && (
              <p className="mb-2 text-[11px] text-[var(--color-muted)]">{sec.blurb}</p>
            )}
            <div className="overflow-hidden rounded border border-[var(--color-border)]">
              <table className="w-full border-collapse text-[12px]">
                <tbody>
                  {sec.rows.map((row, i) => (
                    <tr
                      key={row.name + i}
                      className={
                        "align-top " +
                        (i % 2 === 0
                          ? "bg-[var(--color-bg)]"
                          : "bg-[var(--color-surface)]")
                      }
                    >
                      <td className="w-[30%] whitespace-nowrap px-3 py-2 font-medium text-[var(--color-fg)]">
                        {row.name}
                      </td>
                      <td className="w-[22%] px-3 py-2">
                        <Chip typed={row.typed}>{row.trigger}</Chip>
                      </td>
                      <td className="px-3 py-2 text-[11px] leading-relaxed text-[var(--color-muted)]">
                        {row.note ?? ""}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
        ))}

        <p className="pb-2 text-center text-[10px] text-[var(--color-muted)]">
          Configurable shortcuts (popup, expander, snippet slots) are set in the Settings tab.
        </p>
        </div>
      </div>
    </div>
  );
}

/** A monospace chip — keys use the accent tint, typed commands a neutral box. */
function Chip({ children, typed }: { children: React.ReactNode; typed?: boolean }) {
  return (
    <span
      className={
        "inline-block rounded border px-1.5 py-0.5 font-[var(--font-mono)] text-[11px] whitespace-nowrap " +
        (typed
          ? "border-[var(--color-border)] bg-[var(--color-bg)] text-[var(--color-fg)]"
          : "border-[var(--color-accent)]/40 bg-[var(--color-accent)]/10 text-[var(--color-accent)]")
      }
    >
      {children}
    </span>
  );
}
