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
import { COMMAND_DOCS, localizedTagline } from "../lib/commandDocs";
import {
  NON_COMMAND_FEATURES,
  HIDDEN_TRIGGER_FEATURES,
  IN_POPUP_ACTIONS,
  HIDDEN_GAMES,
} from "../lib/feature-extras";
import { InlineMd } from "./InlineMd";
import { stripInlineMarkup } from "../lib/inline-md";
import { t, useT } from "../lib/i18n";

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
  useT();
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
    note: d.aliases.length > 0 ? t("feat.with_alias", { text: localizedTagline(d), aliases: d.aliases.join(", ") }) : localizedTagline(d),
  }));

  const sections: Section[] = [
    {
      title: t("feat.global_hotkeys"),
      icon: <Keyboard size={14} />,
      blurb: t("feat.work_in_any_app_even"),
      rows: [
        {
          name: t("feat.open_app_clipboard_history"),
          trigger: formatHotkey(popupHotkey),
          note: t("feat.toggle_the_main_popup_clipboard"),
        },
        ...(historyHotkey.trim()
          ? [
              {
                name: t("feat.clipboard_history_2nd_hotkey"),
                trigger: formatHotkey(historyHotkey),
                note: t("feat.second_optional_shortcut_that_also"),
              },
            ]
          : []),
        {
          name: t("feat.ocr_region"),
          trigger: formatHotkey("Ctrl+Shift+O"),
          note: t("feat.drag_a_marquee_recognised_text"),
        },
        {
          name: t("feat.screenshot_region"),
          trigger: formatHotkey("Ctrl+Shift+S"),
          note: t("feat.drag_a_marquee_png_to"),
        },
        {
          name: t("feat.pick_colour_eyedropper"),
          trigger: formatHotkey("Ctrl+Shift+C"),
          note: t("feat.loupe_to_sample_any_on"),
        },
        {
          name: t("feat.screen_recording"),
          trigger: formatHotkey("Ctrl+Shift+Alt+S"),
          note: t("feat.record_hotkey_note"),
        },
        {
          name: t("feat.replace_overlay_audio"),
          trigger: formatHotkey("Ctrl+Shift+Alt+M"),
          note: t("feat.select_a_video_in_finder"),
        },
        {
          name: t("feat.finder_selection"),
          trigger: formatHotkey("Ctrl+Shift+F"),
          note: t("feat.pull_the_files_selected_in"),
        },
        {
          name: "Markdown → PDF",
          trigger: formatHotkey("Ctrl+Shift+M"),
          note: t("feat.convert_the_md_files_selected"),
        },
        {
          name: t("feat.timesheet"),
          trigger: formatHotkey("Ctrl+Shift+T"),
          note: t("feat.open_the_time_tracking_overview"),
        },
        {
          name: t("feat.abbreviation_expander"),
          trigger: formatHotkey(expander.hotkey),
          note: expander.enabled
            ? t("feat.type_an_abbreviation_anywhere_incl")
            : t("feat.disabled_enable_it_in_settings"),
        },
        {
          name: t("feat.auto_expansion_atext_style"),
          trigger: t("feat.automatic"),
          note: autoExpand?.enabled
            ? t("feat.auto_on", { mode: autoExpand.trigger === "immediate" ? t("feat.auto_immediate") : t("feat.auto_delimiter") })
            : t("feat.off_enable_in_settings_to"),
        },
        ...(slots.length
          ? slots.map((s) => ({
              name: t("feat.direct_snippet", { name: s.title ?? s.abbreviation ?? `#${s.snippet_id}` }),
              trigger: formatHotkey(s.hotkey),
              note: t("feat.press_to_paste_the_snippet"),
            }))
          : [
              {
                name: t("feat.direct_snippet_slots"),
                trigger: "—",
                note: t("feat.none_bound_add_hotkey_snippet"),
              },
            ]),
      ],
    },
    {
      title: t("feat.search_bar_commands"),
      icon: <Terminal size={14} />,
      blurb: t("feat.type_these_into_the_popup"),
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
      title: t("feat.in_popup_preview_actions"),
      icon: <MousePointerClick size={14} />,
      rows: IN_POPUP_ACTIONS,
    },
    {
      title: t("feat.hidden_games"),
      icon: <Gamepad2 size={14} />,
      blurb: t("feat.type_the_exact_word_into"),
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
              stripInlineMarkup(r.note ?? "")
                .toLowerCase()
                .includes(q),
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
              placeholder={t("feat.search_features")}
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
            {t("feat.no_match", { q: query })}
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
                        <InlineMd text={row.note ?? ""} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
        ))}

        <p className="pb-2 text-center text-[10px] text-[var(--color-muted)]">
          {t("feat.configurable_shortcuts_popup_expander_sn")}
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
