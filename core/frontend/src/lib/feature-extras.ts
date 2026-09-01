/**
 * The hand-maintained rows of the in-app **Features tab** that are NOT derived
 * from the `CommandDoc` registry: non-command search-bar features, the hidden
 * triggers (games / bpm / equalizer / 2fa), and the in-popup / preview actions.
 *
 * Extracted from `FeaturesPanel.tsx` into data so a test can **guarantee every
 * hidden trigger is documented** (`feature-extras.test.ts`) — the class of
 * feature that has no automatic gate (real commands are covered by
 * `commandDocs.test.ts` + `gen-docs`). When you add a hidden trigger (a new
 * game, visualizer, …) add it to `HIDDEN_TRIGGERS` **and** to the matching row
 * list, or the test fails by name.
 */
import { IS_MAC, formatHotkey } from "./platform";

const MOD = IS_MAC ? "Meta" : "Ctrl"; // Cmd on macOS, Ctrl elsewhere

export interface FeatureRow {
  /** Human name of the feature. */
  name: string;
  /** Pre-formatted shortcut, or the literal text the user types. */
  trigger: string;
  /** True → render `trigger` as a typed-command chip, not a key chip. */
  typed?: boolean;
  /** Short "how to use" note; omitted when the trigger speaks for itself. */
  note?: string;
  /** For hidden triggers / games: the exact word the completeness test checks. */
  keyword?: string;
}

/** Search-bar features that aren't power commands (no `CommandDoc`). */
export const NON_COMMAND_FEATURES: FeatureRow[] = [
  {
    name: "AI prompt templates",
    trigger: "ai… (aiplan · aireview · aifrontend · aibanana · …)",
    typed: true,
    note: '27 curated prompt snippets — type the abbreviation to expand a ready-to-use AI prompt; Enter pastes. New: aifrontend (AAA Material 3 frontend) · aibanana (Nano-Banana OG thumbnail). Grouped under "AI Prompts" in the Snippets tab.',
  },
  {
    name: "Snippet groups",
    trigger: "Snippets tab",
    typed: false,
    note: "Organise snippets into groups — filter the list by group chip, assign a group in the editor, and create/rename/reorder/delete groups via the folder button. Ships pre-grouped (AI Prompts · Colors). Groups are carried in snippet + full-settings backups by name.",
  },
  {
    name: "Snippet storage",
    trigger: "Settings → Snippets",
    typed: false,
    note: "Shows how many snippets you have and their on-disk footprint. There's no cap on how many you can store (the 1000-item limit is clipboard history only); the list is virtualised so thousands stay smooth.",
  },
  {
    name: "Download social media",
    trigger: "paste a YouTube / Instagram / TikTok / Facebook URL",
    typed: true,
    note: "Auto-detected in a clip or the search bar → the preview offers Download video (all) + Download audio (YouTube only) → Downloads. Prefers H.264 (QuickTime-playable); retries with browser cookies on YouTube's bot check. Needs yt-dlp.",
  },
  {
    name: "Ausschnitt (trim & download)",
    trigger: "„Ausschnitt“ under the download buttons",
    typed: false,
    note: "QuickTime-style trim bar: yellow handles over the timeline, everything outside is discarded; drag the range, set the playhead, type start/end times, check Anfang · Bereich · Ende by ear (a small audio proxy, ~5 s to fetch). The download then fetches ONLY the section (yt-dlp --download-sections, frame-accurate) — 20 s out of an 83-minute set arrive as ~1.2 MiB. Collapsed = the download is byte-identical to before.",
  },
  {
    name: "Link grabber (batch download)",
    trigger: "paste many links into the box under the download bar",
    typed: true,
    note: "Paste anything — a list, a chat log, an e-mail — and every YouTube / Instagram / TikTok / Facebook / Dailymotion link in it is picked out, deduplicated and downloaded one after another. A failing link is recorded on its row and the queue continues; Retry re-runs only the failures. Audio is offered when every link is YouTube. The popup stays pinned for the run and Finder opens once at the end, not once per file. Each row shows a small thumbnail, the title, channel and duration before anything is downloaded — cached per URL, debounced, at most three lookups at once (each costs ~4 s).",
  },
  { name: "Calculator", trigger: "2+2 · sqrt(144) · 0xff & 1", typed: true, note: "Inline calculator — Enter pastes the result." },
  {
    name: "Unit / base / time converter",
    trigger: "5 km in mi · 0xff in dec · 1700000000 as date",
    typed: true,
    note: "Conversions right in the search box (length/mass/data/temp · number base · epoch→date).",
  },
  { name: "Colour converter", trigger: "#hex · rgb(…) · hsl(…)", typed: true, note: "Parse any colour format; Enter pastes the canonical hex." },
];

/** Hidden triggers — an exact word, deliberately NOT in autocomplete/`COMMANDS`. */
export const HIDDEN_TRIGGER_FEATURES: FeatureRow[] = [
  { name: "2FA manager", trigger: "2fa", keyword: "2fa", typed: true, note: "Full TOTP overlay — list / add / import / export, each entry with its brand icon (Simple Icons; unknown issuers get a monogram, never a guessed look-alike). Just type to filter the list; Enter copies the top match's code. `2fa add [issuer]` (or the preview's ＋ button) jumps straight to the add form: Issuer · Login · Base32 secret." },
  { name: "TOTP code", trigger: "otp <issuer> · 2fa <issuer>", keyword: "otp", typed: true, note: "e.g. otp ama or 2fa hosti → live code for the matching provider, Enter copies it." },
  { name: "BPM detector", trigger: "bpm", keyword: "bpm", typed: true, note: "Press Enter — taps your mic, shows live BPM. Enter again pins it (click-outside won't close; visualizer turns red)." },
  {
    name: "Equalizer",
    trigger: "equalizer · eq",
    keyword: "equalizer",
    typed: true,
    note: "Press Enter — a live 28-band mic spectrum analyzer with peak-hold, plus a live BPM + dB readout and beat-reactive effects. Enter pins it (turns red, click-outside won't close); shares the native mic with bpm/disco.",
  },
  { name: "App launcher", trigger: "<app name>", typed: true, note: "Type an app's name → Enter launches it." },
];

/** In-popup & preview actions (mouse / keyboard on the popup itself). */
export const IN_POPUP_ACTIONS: FeatureRow[] = [
  { name: "Paste selected entry", trigger: formatHotkey("Enter") },
  { name: "Paste with formatting", trigger: formatHotkey("Shift+Enter"), note: "Paste a clip keeping its original HTML/RTF formatting." },
  { name: "Navigate / close", trigger: "↑ ↓ · Esc", note: "Arrow keys move the selection; Esc hides the popup." },
  {
    name: "Adjust volume",
    trigger: formatHotkey("Shift+ArrowUp") + " / " + formatHotkey("Shift+ArrowDown"),
    note: "Raise / lower the system volume without leaving the popup.",
  },
  { name: "Pin clip to top", trigger: "★ list action", note: "Pin a clip — floats to the top and is never pruned." },
  {
    name: "System sleep status",
    trigger: "footer indicator (macOS)",
    note: "Shows whether something is holding the Mac awake: amber \"no-sleep\" when the active pmset profile has sleep 0, \"wach 4:12\" counting down until sleep is possible again (caffeinate & co), \"wach ∞\" for an indefinite holder — tooltip names the processes. Hidden when nothing prevents sleep. Distinct from the red \"wake\" LED, which is Inspector's own wakelock.",
  },
  {
    name: "Show only pinned clips",
    trigger: "📌 history toolbar",
    note: "Toggle the pin button in the history toolbar to collapse the list to just your pinned clips (search still filters within them). Resets when the popup closes.",
  },
  {
    name: "Lineage rails",
    trigger: "left of the list",
    note: "Coloured commit-graph paths connect a clip you copied in a different shape (Plain text / UPPER / base64 …) to its original. Toggle in Settings → Appearance.",
  },
  {
    name: "Formatting options",
    trigger: `hold ${IS_MAC ? "⌘" : "Ctrl"} in the preview`,
    note: "On a text/HTML/RTF clip, hold the modifier (or click the hint) to reveal the transform chips (UPPER / lower / base64 / url-encode …). Each makes a NEW entry; the original is untouched.",
  },
  { name: "Smart actions", trigger: "preview buttons", note: "On a text clip: Open link · Compose email · Call · Open in Maps · Make QR (auto-detected)." },
  { name: "Delete entry", trigger: "🗑 list action", note: "Remove a single clip from the history." },
  { name: "Cut out background", trigger: formatHotkey(`${MOD}+KeyB`), note: "On an image entry in the preview — U²-Net subject cut-out → Downloads." },
  { name: "Screenshot annotate", trigger: "preview → Edit", note: "Arrow/line/text/rect/ellipse/highlight/blur/redact/step-badge on a canvas." },
  { name: "Pin screenshot to screen", trigger: "preview → Pin to screen", note: "Float the capture as an always-on-top window; multiple pins; close per pin." },
  { name: "Text transforms", trigger: formatHotkey(`${MOD}+1`) + "…" + formatHotkey(`${MOD}+9`), note: "On a text entry — UPPER / lower / camel / snake / base64 / url-encode …" },
  { name: "Recolor", trigger: "preview toolbar", note: "Shown for logos / silhouettes (low-chroma images)." },
  { name: "Save entry as note", trigger: "list action", note: "Bookmark any clipboard entry into the Notes tab." },
];

/** Hidden games — exact word into the search field. */
export const HIDDEN_GAMES: FeatureRow[] = [
  {
    name: "X!",
    trigger: "x!",
    keyword: "x!",
    typed: true,
    note: "Vollbild-Spektakel in sechs Akten (30 s) — zeigt zufällig gezogene Befehle und Eigenschaften der App selbst. Klick oder Taste bricht ab.",
  },
  {
    name: "X!! (Schlagzeilen)",
    trigger: "x!!",
    keyword: "x!!",
    typed: true,
    note: "Dasselbe Stück, gefüttert mit den heutigen tagesschau-Schlagzeilen (öffentliche API, ohne Schlüssel). Ohne Netz spielt es die Eigenschaften-Fassung.",
  },
  { name: "Pong", trigger: "getshaky", keyword: "getshaky", typed: true },
  { name: "Snake — walls", trigger: "rockthebox", keyword: "rockthebox", typed: true },
  { name: "Snake — wrap edges", trigger: "rockthabox", keyword: "rockthabox", typed: true },
  { name: "Space Invaders", trigger: "spacer", keyword: "spacer", typed: true },
  { name: "Flappy Bird", trigger: "learningtofly", keyword: "learningtofly", typed: true },
];

/**
 * Every hidden trigger that MUST have a Features-tab row. `feature-extras.test.ts`
 * asserts each appears (by `keyword`) in `HIDDEN_TRIGGER_FEATURES` ∪ `HIDDEN_GAMES`.
 * `opener` is intentionally omitted (an undocumented easter egg); `shazam` is a
 * real `COMMANDS` entry, so it's covered by the `CommandDoc` gate instead.
 */
export const HIDDEN_TRIGGERS: readonly string[] = [
  "x!",
  "x!!",
  "getshaky",
  "rockthebox",
  "rockthabox",
  "spacer",
  "learningtofly",
  "bpm",
  "equalizer",
  "2fa",
  "otp",
];
