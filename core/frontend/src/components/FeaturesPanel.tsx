import { useEffect, useState } from "react";
import { Gamepad2, Keyboard, MousePointerClick, Search, Terminal } from "lucide-react";
import {
  getAutoExpandConfig,
  getDirectSlots,
  getExpanderConfig,
  getInputLockChord,
  getPopupHotkey,
  getHistoryHotkey,
  type AutoExpandConfig,
  type DirectSlot,
} from "../lib/ipc";
import { IS_MAC, formatHotkey } from "../lib/platform";

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

const MOD = IS_MAC ? "Meta" : "Ctrl"; // Cmd on macOS, Ctrl elsewhere

export function FeaturesPanel() {
  const [popupHotkey, setPopupHotkey] = useState("Ctrl+Space");
  const [historyHotkey, setHistoryHotkey] = useState("");
  const [expander, setExpander] = useState<{ enabled: boolean; hotkey: string }>({
    enabled: false,
    hotkey: "Alt+Digit1",
  });
  const [slots, setSlots] = useState<DirectSlot[]>([]);
  const [chord, setChord] = useState<string[]>(["i", "r"]);
  const [autoExpand, setAutoExpand] = useState<AutoExpandConfig | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const [p, e, s, c, ae, h] = await Promise.all([
          getPopupHotkey(),
          getExpanderConfig(),
          getDirectSlots(),
          getInputLockChord(),
          getAutoExpandConfig(),
          getHistoryHotkey(),
        ]);
        if (!alive) return;
        setPopupHotkey(p);
        setHistoryHotkey(h);
        setExpander({ enabled: e.enabled, hotkey: e.hotkey });
        setSlots(s);
        setChord(c);
        setAutoExpand(ae);
      } catch {
        /* leave defaults — backend unreachable (e.g. browser-only test). */
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

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
      blurb: "Type these into the popup's search field.",
      rows: [
        { name: "AI prompt templates", trigger: "ai… (aiplan · aireview · aifrontend · aibanana · …)", typed: true, note: "27 curated prompt snippets — type the abbreviation to expand a ready-to-use AI prompt; Enter pastes. New: aifrontend (AAA Material 3 frontend) · aibanana (Nano-Banana OG thumbnail). Manage in the Snippets tab." },
        { name: "Download social media", trigger: "paste a YouTube / Instagram / TikTok / Facebook URL", typed: true, note: "Auto-detected in a clip or the search bar → the preview offers Download video (all) + Download audio (YouTube only) → Downloads. Prefers H.264 (QuickTime-playable); retries with browser cookies on YouTube's bot check. Needs yt-dlp." },
        { name: "Trim audio / video", trigger: "trim", typed: true, note: "Pick a local file → set start/end → lossless-fast (keyframe) or frame-accurate cut → saves a -trim copy next to it. Needs ffmpeg." },
        { name: "Calculator", trigger: "2+2 · sqrt(144) · 0xff & 1", typed: true, note: "Inline calculator — Enter pastes the result." },
        { name: "Unit / base / time converter", trigger: "5 km in mi · 0xff in dec · 1700000000 as date", typed: true, note: "Conversions right in the search box (length/mass/data/temp · number base · epoch→date)." },
        { name: "Colour converter", trigger: "#hex · rgb(…) · hsl(…)", typed: true, note: "Parse any colour format; Enter pastes the canonical hex." },
        { name: "Translate", trigger: "tr / tren / trde <text>", typed: true, note: "Google Translate. Also DE↔IT/ES/PL: trde2it · trit2de · trde2sp · trsp2de · trde2pl · trpl2de." },
        { name: "Web search", trigger: "g · ddg · gh · yt · npm · crates · so · mdn · wiki <query>", typed: true, note: "Open a site's search in your browser." },
        { name: "Dev tools", trigger: "uuid [n] · slug <t> · hash <t> · json · jwt", typed: true, note: "Generate UUIDs · slugify · SHA-256 · pretty-print clipboard JSON · decode clipboard JWT → clipboard." },
        { name: "QR code", trigger: "qr <text>", typed: true, note: "Live preview in the panel; Enter copies the PNG to the clipboard." },
        { name: "Resize clipboard image", trigger: "rz <W>x<H>", typed: true, note: "Lanczos resize of the image on the clipboard." },
        { name: "Optimise PNG", trigger: "optim", typed: true, note: "Lossless oxipng of the clipboard PNG → Downloads." },
        { name: "Remove vowels", trigger: "rmvvls <text>", typed: true },
        { name: "Kill process", trigger: "kill [-9] [name | pid]", typed: true, note: "Live picker by name or PID (kill 1234); Enter kills (confirm first)." },
        { name: "Reboot / Shut down / Lock", trigger: "reboot · shutdown · lock", typed: true, note: "macOS only. reboot/shutdown confirm first." },
        { name: "Mute system", trigger: "mute", typed: true },
        { name: "Input lock", trigger: "freeze", typed: true, note: `Blocks all input until the unlock chord (${chord.map((k) => k.toUpperCase()).join(" + ") || "i + r"}). ⌥⌘Esc always frees.` },
        { name: "Keep awake", trigger: "wakelock on / off", typed: true, note: "Alias: caffeine on/off. Pauses sleep + screen lock; a status toast confirms." },
        { name: "Create file / folder", trigger: "touch <name> / mkdir <name>", typed: true, note: "In the frontmost Finder window's folder (needs Automation → Finder)." },
        { name: "Open terminal here", trigger: "terminal", typed: true, note: "iTerm2 / Terminal at the frontmost Finder folder." },
        { name: "Net-pay calculator", trigger: "bruno <€>", typed: true, note: "German brutto→netto (Steuerjahr 2025)." },
        { name: "Timer", trigger: "timer <n>[s|min]", typed: true, note: "Notification + sound on expiry; status toast on set." },
        { name: "Alarm", trigger: "alarm <HH:MM>", typed: true, note: "Fires at a clock time (next occurrence), e.g. 3:00 / 15:15." },
        { name: "Markdown → PDF", trigger: "md2pdf [path]", typed: true, note: "Same as ⌃⇧M — selection or a path. PDF lands next to the source." },
        { name: "Screenshot (modes)", trigger: "shot [n] · shotfull · shotwin · shotlast", typed: true, note: "Region (opt. self-timer n) · full screen · active window · repeat last → same floating preview." },
        { name: "Clean caches/logs", trigger: "clean", typed: true, note: "Dry-run preview + confirm before deleting. Level/categories in Settings. Allowlist-only, no symlinks." },
        { name: "Monitor brightness", trigger: "brightness · bri", typed: true, note: "Enter → sliders in the preview; ↑↓ pick monitor, ←→ adjust, Enter back to list. Built-in + external. Software dimming on macOS/Windows, DDC on Linux." },
        { name: "Audio output device", trigger: "sound", typed: true, note: "Enter → device list in the preview; ↑↓ select, Enter switches the system output. macOS + Windows." },
        { name: "Random number", trigger: "rnd · random [max] [min max]", typed: true, note: "Rolls a number (default 1–6; `rnd 100` = 1–100, `rnd 5 500` = 5–500), shown big on screen." },
        { name: "Password generator", trigger: "pwgen [N]", typed: true, note: "Bare pwgen = default length; pwgen 16 sets it. Modes in the preview." },
        { name: "2FA manager", trigger: "2fa", typed: true, note: "Full TOTP overlay — list / add / import / export." },
        { name: "TOTP code", trigger: "otp <issuer>", typed: true, note: "e.g. otp ama → live Amazon code, Enter copies." },
        { name: "BPM detector", trigger: "bpm", typed: true, note: "Press Enter — taps your mic, shows live BPM." },
        { name: "Meme picker", trigger: "meme [query]", typed: true, note: "Fuzzy-browse your meme folder; animated preview; Enter copies to the clipboard." },
        { name: "App launcher", trigger: "<app name>", typed: true, note: "Type an app's name → Enter launches it." },
      ],
    },
    {
      title: "In-popup & preview actions",
      icon: <MousePointerClick size={14} />,
      rows: [
        { name: "Paste selected entry", trigger: formatHotkey("Enter") },
        { name: "Paste with formatting", trigger: formatHotkey("Shift+Enter"), note: "Paste a clip keeping its original HTML/RTF formatting." },
        { name: "Navigate / close", trigger: "↑ ↓ · Esc", note: "Arrow keys move the selection; Esc hides the popup." },
        { name: "Adjust volume", trigger: formatHotkey("Shift+ArrowUp") + " / " + formatHotkey("Shift+ArrowDown"), note: "Raise / lower the system volume without leaving the popup." },
        { name: "Pin clip to top", trigger: "★ list action", note: "Pin a clip — floats to the top and is never pruned." },
        { name: "Smart actions", trigger: "preview buttons", note: "On a text clip: Open link · Compose email · Call · Open in Maps · Make QR (auto-detected)." },
        { name: "Delete entry", trigger: "🗑 list action", note: "Remove a single clip from the history." },
        { name: "Cut out background", trigger: formatHotkey(`${MOD}+KeyB`), note: "On an image entry in the preview — U²-Net subject cut-out → Downloads." },
        { name: "Screenshot annotate", trigger: "preview → Edit", note: "Arrow/line/text/rect/ellipse/highlight/blur/redact/step-badge on a canvas." },
        { name: "Pin screenshot to screen", trigger: "preview → Pin to screen", note: "Float the capture as an always-on-top window; multiple pins; close per pin." },
        { name: "Text transforms", trigger: formatHotkey(`${MOD}+1`) + "…" + formatHotkey(`${MOD}+9`), note: "On a text entry — UPPER / lower / camel / snake / base64 / url-encode …" },
        { name: "Recolor", trigger: "preview toolbar", note: "Shown for logos / silhouettes (low-chroma images)." },
        { name: "Save entry as note", trigger: "list action", note: "Bookmark any clipboard entry into the Notes tab." },
      ],
    },
    {
      title: "Hidden games",
      icon: <Gamepad2 size={14} />,
      blurb: "Type the exact word into the search field. Esc suspends & resumes; each keeps its own high score.",
      rows: [
        { name: "Pong", trigger: "getshaky", typed: true },
        { name: "Snake — walls", trigger: "rockthebox", typed: true },
        { name: "Snake — wrap edges", trigger: "rockthabox", typed: true },
        { name: "Space Invaders", trigger: "spacer", typed: true },
        { name: "Flappy Bird", trigger: "learningtofly", typed: true },
      ],
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
