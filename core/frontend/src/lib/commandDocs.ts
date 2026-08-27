// Inline-help registry — the CANONICAL source of truth for every search-bar
// power command (v0.84.273). One structured CommandDoc per command; `?` after
// a command renders this in the preview pane. The README command matrix and the
// Features tab are GENERATED from this file (scripts/gen-docs.mjs) — do not
// hand-edit those. Only `import type` here so the Node generator can load it.
//
// Coverage is enforced: commandDocs.test.ts asserts every non-hidden COMMANDS
// keyword has a doc (via `command`/`aliases`), each with a non-empty synopsis +
// description, ≥3 examples and ≥1 tip or caveat. A missing doc fails that test
// by name.

export interface Arg {
  name: string;
  required: boolean;
  description: string;
  default?: string;
}

export interface Flag {
  flag: string;
  value_type?: string;
  description: string;
  default?: string;
}

export interface Example {
  /** Tab-fillable into the search bar. */
  input: string;
  /** What it produces / does. */
  result: string;
  note?: string;
}

export interface CommandDoc {
  command: string;
  aliases: string[];
  category: string;
  version_added: string;
  /** One line — inline help + README.md matrix. */
  tagline: string;
  /** German matrix line for README.de.md (falls back to `tagline`). */
  tagline_de?: string;
  /** Grammar, monospace. */
  synopsis: string;
  description: string;
  arguments: Arg[];
  flags: Flag[];
  examples: Example[];
  tips: string[];
  caveats: string[];
  related: string[];
  /** Path to a docs/*.md with more detail, if any. */
  see_also?: string;
}

const CAT_WEB = "Translate & Search";
const CAT_IMG = "Images & Files";
const CAT_TEXT = "Text & Dev tools";
const CAT_SYS = "System";
const CAT_MEDIA = "Media & Screenshots";
const CAT_PROD = "Productivity";
const CAT_SEC = "Testing & Security";
const CAT_AV = "Audio & Smart home";
const CAT_INFO = "Info & Monitoring";
const CAT_FUN = "Fun";

export const COMMAND_DOCS: CommandDoc[] = [
  // ── Translate & Search ────────────────────────────────────────────────
  {
    command: "tr",
    aliases: ["tren", "trde", "trde2it", "trit2de", "trde2sp", "trsp2de", "trde2pl", "trpl2de"],
    category: CAT_WEB,
    version_added: "0.18.0",
    tagline: "Live translate in the preview — Enter copies, ⇧Enter opens Google Translate.",
    tagline_de: "Live-Übersetzung in der Preview — Enter kopiert, ⇧Enter öffnet Google Translate.",
    synopsis: "tr <text>   ·   tren|trde <text>   ·   trde2it|trit2de|trde2sp|trsp2de|trde2pl|trpl2de <text>",
    description:
      "Shows a live translation in the preview while you type (keyless Google gtx → MyMemory). Enter copies the result to the clipboard; Shift+Enter opens Google Translate in the browser with the text pre-filled. Click the source or target box to copy that side. The keyword picks the language pair: `tr` auto-detects → German; `tren` English→German, `trde` German→English; the `tr<a>2<b>` forms are fixed pairs between German and Italian/Spanish/Polish.",
    arguments: [{ name: "text", required: true, description: "The text to translate (rest of the line).", default: undefined }],
    flags: [],
    examples: [
      { input: "tr Feierabend", result: "Live DE→EN (or auto); Enter copies the English result." },
      { input: "tren cheerful", result: "English → German; ⇧Enter opens Google Translate." },
      { input: "trde2it Guten Morgen", result: "German → Italian (buongiorno)." },
    ],
    tips: [
      "`tr` auto-detects the source language, so it's the one to reach for when unsure.",
      "Click either language box in the preview to copy that text without leaving the popup.",
    ],
    caveats: [
      "Live translate and ⇧Enter both send the text to an external provider (Google / MyMemory / Google Translate in the browser).",
    ],
    related: ["g"],
  },
  {
    command: "g",
    aliases: ["ddg", "gh", "yt", "npm", "crates", "so", "mdn", "wiki"],
    category: CAT_WEB,
    version_added: "0.76.0",
    tagline: "Web-search bangs — open a site's search for the query.",
    tagline_de: "Web-Such-Bangs — die Suche einer Seite für die Anfrage öffnen.",
    synopsis: "g|ddg|gh|yt|npm|crates|so|mdn|wiki <query>",
    description:
      "A DuckDuckGo-style 'bang' opens a specific site's search results for your query in the browser: `g` Google, `ddg` DuckDuckGo, `gh` GitHub, `yt` YouTube, `npm` npm, `crates` crates.io, `so` Stack Overflow, `mdn` MDN, `wiki` Wikipedia. Data-driven — adding a bang is one map entry.",
    arguments: [{ name: "query", required: true, description: "Search terms (rest of the line).", default: undefined }],
    flags: [],
    examples: [
      { input: "gh tauri clipboard", result: "Opens GitHub search for 'tauri clipboard'." },
      { input: "crates serde", result: "Opens crates.io search for 'serde'." },
      { input: "mdn addEventListener", result: "Opens MDN search." },
    ],
    tips: ["The query is URL-encoded, so spaces and symbols are safe."],
    caveats: ["Opens your browser (the search request happens there)."],
    related: ["tr"],
  },

  // ── Images & Files ────────────────────────────────────────────────────
  {
    command: "rz",
    aliases: ["resize"],
    category: CAT_IMG,
    version_added: "0.84.72",
    tagline: "Resize the selected Finder image(s) (Lanczos3) → sibling files.",
    tagline_de: "Ausgewählte Finder-Bild(er) skalieren (Lanczos3) → Nachbardateien.",
    synopsis: "rz <W>x<H>   ·   rz <W> <H>",
    description:
      "Resizes the image(s) currently selected in Finder to W×H (Lanczos3), writing `<name>-WxH.<ext>` next to each (PNG/JPEG/WebP/GIF/BMP). Reads the live selection — you don't have to be in finder-mode first. Falls back to the clipboard image (16 MP cap) when nothing usable is selected or Automation isn't granted. Preset rows appear as you type `rz` (Tab fills one).",
    arguments: [{ name: "WxH", required: true, description: "Target dimensions — `200x200`, `200 x 200`, or a plain space `200 200`.", default: undefined }],
    flags: [],
    examples: [
      { input: "rz 1200x800", result: "Each selected image → a 1200×800 sibling copy." },
      { input: "rz 512 512", result: "Space-separated dimensions also work." },
      { input: "rz 64x64", result: "Great for favicons/app icons from a selected PNG." },
    ],
    tips: ["Type `rz` bare to see the size presets; Tab fills one into the bar."],
    caveats: [
      "Needs the Finder Automation permission (macOS) for the selection; without it, falls back to the clipboard image.",
      "Aspect ratio is not preserved — W and H are applied as given.",
    ],
    related: ["optim"],
  },
  {
    command: "optim",
    aliases: ["optimize"],
    category: CAT_IMG,
    version_added: "0.84.71",
    tagline: "Compress the selected Finder image(s) → sibling files.",
    tagline_de: "Ausgewählte Finder-Bild(er) komprimieren → Nachbardateien.",
    synopsis: "optim",
    description:
      "Compresses the image(s) selected in Finder, writing `<name>-optim.<ext>` next to each — PNG losslessly via oxipng, JPEG re-encoded at q85 (kept only if smaller). Reads the live selection; falls back to the clipboard PNG → Downloads when nothing usable is selected or Automation isn't granted.",
    arguments: [],
    flags: [],
    examples: [
      { input: "optim", result: "Each selected PNG/JPEG → a smaller `-optim` sibling." },
      { input: "optim", result: "With no selection but a PNG on the clipboard → compressed PNG in ~/Downloads." },
      { input: "optim", result: "JPEGs are re-encoded at q85 and only kept if the result is actually smaller." },
    ],
    tips: ["PNG compression is lossless (oxipng) — safe for screenshots and logos."],
    caveats: ["Needs the Finder Automation permission for the selection path."],
    related: ["rz"],
  },
  {
    command: "touch",
    aliases: [],
    category: CAT_IMG,
    version_added: "0.53.0",
    tagline: "Create a file in the front Finder/Explorer folder (optional content).",
    tagline_de: "Datei im vordersten Finder/Explorer-Ordner anlegen (opt. Inhalt).",
    synopsis: "touch <name> [> text]",
    description:
      "Creates an empty file named `<name>` in the folder of the frontmost Finder window (macOS) / Explorer window (Windows), then reveals it. Inline content after `>` is written into the file. Nested relative paths create intermediate directories; absolute paths and `..` traversal are rejected so it can't escape the folder.",
    arguments: [
      { name: "name", required: true, description: "File name; may be a nested relative path (`a/b/c.txt`).", default: undefined },
      { name: "> text", required: false, description: "Everything after the first `>` becomes the file's content.", default: "(empty file)" },
    ],
    flags: [],
    examples: [
      { input: "touch notes.md", result: "Empty notes.md in the front Finder folder, selected." },
      { input: "touch hello.txt > das ist ein test", result: "hello.txt containing 'das ist ein test'." },
      { input: "touch src/app/index.ts", result: "Creates src/app/ then index.ts (intermediate dirs made)." },
    ],
    tips: ["Falls back to the Desktop when no Finder/Explorer window is open."],
    caveats: ["macOS needs the Finder Automation grant. Windows paths are runtime-unverified."],
    related: ["mkdir", "terminal"],
  },
  {
    command: "mkdir",
    aliases: [],
    category: CAT_IMG,
    version_added: "0.53.0",
    tagline: "Create a folder in the front Finder/Explorer folder.",
    tagline_de: "Ordner im vordersten Finder/Explorer-Ordner anlegen.",
    synopsis: "mkdir <name>",
    description:
      "Creates a folder named `<name>` in the frontmost Finder/Explorer folder and reveals it. Nested relative paths (`a/b/c`) create the intermediate directories; absolute paths and `..` are rejected.",
    arguments: [{ name: "name", required: true, description: "Folder name; may be a nested relative path.", default: undefined }],
    flags: [],
    examples: [
      { input: "mkdir assets", result: "New 'assets' folder in the front Finder folder." },
      { input: "mkdir src/components", result: "Creates src/ then components/." },
      { input: "mkdir 2026-07-invoices", result: "A dated folder for filing." },
    ],
    tips: ["Falls back to the Desktop when no file-manager window is open."],
    caveats: ["macOS needs the Finder Automation grant."],
    related: ["touch", "terminal"],
  },
  {
    command: "terminal",
    aliases: [],
    category: CAT_IMG,
    version_added: "0.53.0",
    tagline: "Open a terminal at the front Finder/Explorer folder.",
    tagline_de: "Terminal im vordersten Finder/Explorer-Ordner öffnen.",
    synopsis: "terminal",
    description:
      "Opens a terminal already `cd`'d into the frontmost Finder/Explorer folder. macOS prefers iTerm2, falling back to Terminal.app; Windows prefers Windows Terminal, then PowerShell, then cmd.exe.",
    arguments: [],
    flags: [],
    examples: [
      { input: "terminal", result: "iTerm2 (or Terminal.app) opens in the current Finder folder." },
      { input: "terminal", result: "With no Finder window open → opens at the Desktop." },
      { input: "terminal", result: "Windows: Windows Terminal at the current Explorer folder." },
    ],
    tips: ["Pairs well with `touch`/`mkdir` to scaffold then jump into a shell."],
    caveats: ["macOS needs the Finder Automation grant. Windows paths are runtime-unverified."],
    related: ["touch", "mkdir", "sec"],
  },

  // ── Text & Dev tools ──────────────────────────────────────────────────
  {
    command: "rmvvls",
    aliases: [],
    category: CAT_TEXT,
    version_added: "0.18.0",
    tagline: "Strip vowels from the text → clipboard.",
    tagline_de: "Vokale aus dem Text entfernen → Clipboard.",
    synopsis: "rmvvls <text>",
    description: "Removes all vowels (aeiou + AEIOU + ä/ö/ü) from the text and puts the result on the clipboard — a compact-writing / obfuscation gag.",
    arguments: [{ name: "text", required: true, description: "The text to strip.", default: undefined }],
    flags: [],
    examples: [
      { input: "rmvvls Donaudampfschiff", result: "Dndmpfschff → clipboard." },
      { input: "rmvvls hello world", result: "hll wrld." },
      { input: "rmvvls Über Öl", result: "br l (umlauts count as vowels)." },
    ],
    tips: ["Handy for terse variable names or a quick cipher."],
    caveats: [],
    related: ["slug", "hash"],
  },
  {
    command: "uuid",
    aliases: [],
    category: CAT_TEXT,
    version_added: "0.76.0",
    tagline: "Generate random v4 UUID(s) → clipboard.",
    tagline_de: "Zufällige v4-UUID(s) erzeugen → Clipboard.",
    synopsis: "uuid [n]",
    description: "Generates one (or `n`) random version-4 UUIDs (via the Web Crypto CSPRNG) onto the clipboard, newline-separated for n>1.",
    arguments: [{ name: "n", required: false, description: "How many UUIDs.", default: "1" }],
    flags: [],
    examples: [
      { input: "uuid", result: "One UUID, e.g. 3f2504e0-4f89-41d3-9a0c-0305e82c3301." },
      { input: "uuid 5", result: "Five UUIDs, one per line." },
      { input: "uuid 100", result: "A bulk batch for seeding a table." },
    ],
    tips: ["For structured fake records use `faker uuid` / `faker user`."],
    caveats: [],
    related: ["faker", "hash", "slug"],
  },
  {
    command: "slug",
    aliases: [],
    category: CAT_TEXT,
    version_added: "0.76.0",
    tagline: "Slugify text (URL-safe, lowercase, hyphenated) → clipboard.",
    tagline_de: "Text sluggen (URL-sicher, klein, Bindestriche) → Clipboard.",
    synopsis: "slug <text>",
    description: "Turns the text into a URL-safe slug: lowercased, spaces → hyphens, non-alphanumerics stripped.",
    arguments: [{ name: "text", required: true, description: "The text to slugify.", default: undefined }],
    flags: [],
    examples: [
      { input: "slug Hello, World!", result: "hello-world" },
      { input: "slug My New Blog Post", result: "my-new-blog-post" },
      { input: "slug Über uns", result: "uber-uns" },
    ],
    tips: ["Turn a title into a filename- or URL-safe stem in one keystroke."],
    caveats: [],
    related: ["uuid", "hash"],
  },
  {
    command: "hash",
    aliases: [],
    category: CAT_TEXT,
    version_added: "0.76.0",
    tagline: "SHA-256 the text → clipboard (hex).",
    tagline_de: "Text als SHA-256 hashen → Clipboard (Hex).",
    synopsis: "hash <text>",
    description: "Computes the SHA-256 of the text (via Web Crypto) and puts the hex digest on the clipboard.",
    arguments: [{ name: "text", required: true, description: "The text to hash.", default: undefined }],
    flags: [],
    examples: [
      { input: "hash password123", result: "The 64-char hex SHA-256 digest." },
      { input: "hash ", result: "Hash of an empty string (e3b0c44…)." },
      { input: "hash contract-v2.pdf", result: "Hash of the literal text (not the file)." },
    ],
    tips: ["This hashes the typed text, not a file."],
    caveats: ["SHA-256 is a checksum, not password storage — don't treat it as such."],
    related: ["uuid", "slug"],
  },
  {
    command: "json",
    aliases: [],
    category: CAT_TEXT,
    version_added: "0.76.0",
    tagline: "Pretty-print the clipboard JSON → clipboard.",
    tagline_de: "Clipboard-JSON formatieren → Clipboard.",
    synopsis: "json",
    description: "Reads the clipboard, parses it as JSON, and writes back a 2-space pretty-printed version. Invalid JSON is left untouched with an error.",
    arguments: [],
    flags: [],
    examples: [
      { input: "json", result: 'Clipboard {"a":1,"b":[2,3]} → indented, readable JSON.' },
      { input: "json", result: "A minified API response becomes browsable." },
      { input: "json", result: "Invalid JSON on the clipboard → an error, clipboard unchanged." },
    ],
    tips: ["Copy a blob, run `json`, paste the formatted result."],
    caveats: ["Operates on the clipboard, not on typed input."],
    related: ["jwt"],
  },
  {
    command: "jwt",
    aliases: [],
    category: CAT_TEXT,
    version_added: "0.76.0",
    tagline: "Decode the clipboard JWT (header + payload) → clipboard.",
    tagline_de: "Clipboard-JWT dekodieren (Header + Payload) → Clipboard.",
    synopsis: "jwt",
    description: "Base64url-decodes the header and payload of a JWT on the clipboard and writes them back as readable JSON. Does NOT verify the signature.",
    arguments: [],
    flags: [],
    examples: [
      { input: "jwt", result: "Clipboard eyJhbGc… → decoded {header, payload} JSON." },
      { input: "jwt", result: "Inspect token claims (exp, sub, scopes) at a glance." },
      { input: "jwt", result: "A malformed token → error, clipboard unchanged." },
    ],
    tips: ["Great for eyeballing token expiry/claims during debugging."],
    caveats: ["Signature is NOT checked — decode only, never trust the contents as verified."],
    related: ["json"],
  },
  {
    command: "qr",
    aliases: [],
    category: CAT_TEXT,
    version_added: "0.76.0",
    tagline: "Generate a QR code — preview live, Enter copies the PNG.",
    tagline_de: "QR-Code erzeugen — Live-Vorschau, Enter kopiert das PNG.",
    synopsis: "qr <text>",
    description:
      "Renders a QR code of the text live in the preview pane (black-on-white so it always scans). Enter copies the PNG to the clipboard (and history). Dependency-free, offline.",
    arguments: [{ name: "text", required: true, description: "The text/URL to encode.", default: undefined }],
    flags: [],
    examples: [
      { input: "qr https://celox.io", result: "A scannable QR of the URL; Enter copies the PNG." },
      { input: "qr WIFI:T:WPA;S:MyNet;P:secret;;", result: "A Wi-Fi join QR." },
      { input: "qr +49 170 1234567", result: "Encode a phone number." },
    ],
    tips: ["The preview updates as you type — check it scans before copying."],
    caveats: ["Very long inputs make dense codes that scan poorly."],
    related: ["uuid"],
  },
  {
    command: "figlet",
    aliases: ["banner", "ascii"],
    category: CAT_TEXT,
    version_added: "0.85.0",
    tagline: "ASCII-art banners — live preview, browse hundreds of fonts, Enter copies.",
    tagline_de: "ASCII-Art-Banner — Live-Vorschau, Hunderte Fonts durchblättern, Enter kopiert.",
    synopsis: "figlet <text> [@font] [--width=n] [--center|--right]   ·   banner · ascii",
    description:
      "Turns text into a FIGlet ASCII-art banner. As you type, the selected font's banner renders live in the preview (monospace); the left list is a gallery where every row previews YOUR text in that font — arrow through it, or fuzzy-filter with `@name`. Enter copies the selected font's full banner to the clipboard (exactly the rendered text, newlines and all); Shift+Enter copies it as a tightly-cropped PNG image instead (theme-coloured, lands in history like any image clip). Hundreds of fonts are bundled and inflated lazily. Option chips (align / width / trim / comment-wrap / box border) apply without re-typing. Unrenderable characters are reported, never silently dropped.",
    arguments: [
      { name: "text", required: false, description: "The banner text. Bare `figlet` opens the gallery with a placeholder.", default: "(gallery)" },
    ],
    flags: [
      { flag: "@<font>", value_type: "name", description: "Jump to / fuzzy-filter a font (e.g. `@slant`, `@sla`).", default: "Settings default font" },
      { flag: "--width", value_type: "n", description: "Wrap width in columns (0 = no wrap).", default: "Settings default (80)" },
      { flag: "--center / --right / --left", value_type: undefined, description: "Alignment.", default: "left" },
      { flag: "--box", value_type: undefined, description: "Draw a box-drawing border around the banner.", default: "off" },
      { flag: "--comment", value_type: "slashes|hash|block|html", description: "Wrap the banner as a source comment.", default: "none" },
    ],
    examples: [
      { input: "figlet Hello", result: "A live banner in the default font + the font gallery." },
      { input: "figlet Deploy @slant", result: "Jumps straight to the 'slant' font." },
      { input: "figlet TODO @doom --box --comment=slashes", result: "A boxed, //-commented banner header for source." },
    ],
    tips: [
      "Tab fills the selected font as `@font` into the search bar so you can keep tweaking; Enter copies.",
      "Shift+Enter copies the banner as a PNG image, cropped tight to the glyphs and coloured like the preview — for chats/mails that mangle monospace text.",
      "Cmd/Ctrl+Shift+Enter renders the PNG with a TRANSPARENT background (glyphs in the theme text colour) and does both: saves it to ~/Downloads (`figlet-<text>-<ts>.png`, revealed) AND copies it to the clipboard. Any image already in history can also be saved via its Save button / Cmd+S in the preview.",
      "With hundreds of fonts, `@name` fuzzy-search is the fastest way to find one.",
    ],
    caveats: [
      "Most FIGlet fonts are ASCII-only — accented/emoji characters are skipped with a visible hint (German umlauts do work).",
      "Very wide banners scroll horizontally in the preview rather than wrapping (use --width to wrap on word boundaries).",
    ],
    related: ["qr"],
    see_also: "docs/figlet.md",
  },

  // ── System ────────────────────────────────────────────────────────────
  {
    command: "settings",
    aliases: ["config"],
    category: CAT_SYS,
    version_added: "0.87.1",
    tagline: "Open the Settings tab — optionally jump straight to a section.",
    tagline_de: "Öffnet den Settings-Tab — optional direkt zu einer Sektion springen.",
    synopsis: "settings [section]   ·   config [section]",
    description:
      "Opens Inspector Rust's Settings tab from the search bar. With an argument it deep-links to a section: the name is fuzzy-matched (exact > prefix > subsequence) against German AND English section names — `settings cue`, `settings sync`, `settings hotkeys`, `settings gesten` all work. The target section scrolls into view and flashes a highlight ring so you see exactly where you landed.",
    arguments: [
      { name: "section", required: false, description: "Section to jump to (fuzzy; DE/EN names) — e.g. cue · bruno · hotkeys · backup · gestures · timesheet.", default: "top of the Settings tab" },
    ],
    flags: [],
    examples: [
      { input: "settings", result: "Opens the Settings tab." },
      { input: "settings cue", result: "Jumps to Cloud-Sync (cue) — token, connection checkmarks, sync now." },
      { input: "settings hotkeys", result: "Jumps to Global shortcuts (rebindable action hotkeys)." },
    ],
    tips: [
      "German names work too: `settings gesten`, `settings zeiterfassung`, `settings aufräumen`.",
      "`config` is a drop-in alias if that's your muscle memory.",
    ],
    caveats: [],
    related: ["kill"],
  },
  {
    command: "kill",
    aliases: [],
    category: CAT_SYS,
    version_added: "0.19.0",
    tagline: "Live process picker — filter by name/PID, confirm, terminate.",
    tagline_de: "Live-Prozess-Picker — nach Name/PID filtern, bestätigen, beenden.",
    synopsis: "kill [-9] [pattern | pid]",
    description:
      "Lists running processes (sorted by memory, including Inspector Rust itself) as a live picker; the argument filters by name/exe substring (every whitespace token must match) or, when all-digits, by exact PID. Enter asks a native confirmation, then sends SIGTERM (or SIGKILL with `-9`; TerminateProcess on Windows).",
    arguments: [{ name: "pattern | pid", required: false, description: "Name/exe substring(s), or an exact numeric PID.", default: "(show all)" }],
    flags: [{ flag: "-9", value_type: undefined, description: "Force kill (SIGKILL) instead of SIGTERM.", default: "SIGTERM" }],
    examples: [
      { input: "kill", result: "The full process list, biggest memory first." },
      { input: "kill inspector", result: "Inspector Rust (and any other matching process)." },
      { input: "kill -9 1234", result: "Force-kill PID 1234 (after confirming)." },
    ],
    tips: ["Type a memory hog's name to find it fast; the list is sorted by RAM.", "`kill inspector rust` matches InspectorRust — every word is required, space optional in the process name."],
    caveats: ["Destructive — a native confirmation is required before the kill.", "Windows has no signals; `-9` and plain both map to a forced TerminateProcess.", "Unfiltered list is capped at 50 (highest memory); type a pattern to find quieter processes."],
    related: ["reboot", "shutdown", "lock"],
  },
  {
    command: "reboot",
    aliases: [],
    category: CAT_SYS,
    version_added: "0.19.0",
    tagline: "Reboot the machine (with confirmation).",
    tagline_de: "Rechner neu starten (mit Bestätigung).",
    synopsis: "reboot",
    description: "Restarts the computer after a native confirmation. macOS via loginwindow Apple Events; Windows `shutdown /r`; Linux `systemctl reboot`. No sudo.",
    arguments: [],
    flags: [],
    examples: [
      { input: "reboot", result: "Confirmation dialog → the machine reboots." },
      { input: "reboot", result: "Cancel the dialog → nothing happens." },
      { input: "reboot", result: "Same flow on Windows/Linux (logind)." },
    ],
    tips: [],
    caveats: ["Destructive to unsaved work — a confirmation is always shown."],
    related: ["shutdown", "lock"],
  },
  {
    command: "shutdown",
    aliases: [],
    category: CAT_SYS,
    version_added: "0.19.0",
    tagline: "Shut the machine down (with confirmation).",
    tagline_de: "Rechner herunterfahren (mit Bestätigung).",
    synopsis: "shutdown",
    description: "Powers off the computer after a native confirmation. macOS via loginwindow Apple Events; Windows `shutdown /s`; Linux `systemctl poweroff`. No sudo.",
    arguments: [],
    flags: [],
    examples: [
      { input: "shutdown", result: "Confirmation dialog → the machine powers off." },
      { input: "shutdown", result: "Cancel → nothing happens." },
      { input: "shutdown", result: "Same flow on Windows/Linux." },
    ],
    tips: [],
    caveats: ["Destructive to unsaved work — a confirmation is always shown."],
    related: ["reboot", "lock"],
  },
  {
    command: "lock",
    aliases: [],
    category: CAT_SYS,
    version_added: "0.19.0",
    tagline: "Lock the screen immediately.",
    tagline_de: "Bildschirm sofort sperren.",
    synopsis: "lock",
    description: "Locks the screen right away (no confirmation — it's cheap to undo). macOS `pmset displaysleepnow`; Windows LockWorkStation; Linux loginctl/xdg-screensaver.",
    arguments: [],
    flags: [],
    examples: [
      { input: "lock", result: "Screen locks instantly." },
      { input: "lock", result: "Quick way to step away securely." },
      { input: "lock", result: "Same on Windows/Linux." },
    ],
    tips: ["No confirmation — it's the one system command that's trivially reversible."],
    caveats: [],
    related: ["reboot", "shutdown", "freeze"],
  },
  {
    command: "mute",
    aliases: [],
    category: CAT_SYS,
    version_added: "0.19.0",
    tagline: "Toggle system output mute.",
    tagline_de: "System-Ausgabe stumm schalten (Toggle).",
    synopsis: "mute",
    description: "Toggles the system output mute state. macOS via osascript; Windows the multimedia mute key; Linux wpctl/pactl.",
    arguments: [],
    flags: [],
    examples: [
      { input: "mute", result: "Mutes if unmuted, unmutes if muted." },
      { input: "mute", result: "Silence a call notification fast." },
      { input: "mute", result: "Toggle back with the same command." },
    ],
    tips: ["Shift+↑/↓ in the popup nudges the volume by 5%; `sound` opens the full slider."],
    caveats: [],
    related: ["sound", "boom"],
  },
  {
    command: "freeze",
    aliases: [],
    category: CAT_SYS,
    version_added: "0.35.0",
    tagline: "Input lock — block keyboard + mouse until an unlock chord.",
    tagline_de: "Eingabesperre — Tastatur + Maus bis zum Entsperr-Chord blockieren.",
    synopsis: "freeze",
    description:
      "Blocks all keyboard/mouse/trackpad input until you enter the unlock chord (default: hold `i`, press `r`; configurable in Settings → Input Lock). Handy for wiping the keyboard/screen with a toddler or cat around. ⌥⌘Esc (Force Quit) always works as a safety hatch.",
    arguments: [],
    flags: [],
    examples: [
      { input: "freeze", result: "Input is blocked; the unlock chord releases it." },
      { input: "freeze", result: "Wipe the keyboard without triggering anything." },
      { input: "freeze", result: "⌥⌘Esc still works if you need out." },
    ],
    tips: ["Set your own unlock chord in Settings → Input Lock."],
    caveats: ["macOS requires the Accessibility permission (shared with the expander)."],
    related: ["lock"],
  },
  {
    command: "wakelock",
    aliases: ["caffeine"],
    category: CAT_SYS,
    version_added: "0.52.0",
    tagline: "Keep the Mac awake — full (screen on) or dark (screen may sleep).",
    tagline_de: "Mac wachhalten — voll (Bildschirm an) oder dark (Bildschirm darf schlafen).",
    synopsis: "wakelock on|off|dark [on|off]   ·   caffeine on|off|dark",
    description:
      "Prevents sleep, in two modes. **Full** (`wakelock on`): macOS spawns `caffeinate -disu` (real IOPM assertions) — system awake AND display forced on; Windows uses SetThreadExecutionState plus a periodic invisible F15 keypress (so the screensaver/lock don't engage); Linux a logind idle+sleep inhibitor. **Dark** (`wakelock dark`, v0.116.0): the system stays awake while the DISPLAY may sleep (`caffeinate -is` / system-only execution state / sleep-only inhibitor) — remote connections (SSH, Claude Code) stay reachable with the screen dark. The footer shows a red LED for full and a violet ☾ `srv` badge for dark; the ☾ is CLICKABLE and toggles dark mode directly.",
    arguments: [{ name: "on|off|dark", required: true, description: "`on`/`off` = full mode (also accepts 1/0/true/false). `dark` toggles the screen-may-sleep mode; `dark on` / `dark off` set it explicitly.", default: undefined }],
    flags: [],
    examples: [
      { input: "wakelock on", result: "Full keep-awake: screen stays on; red footer LED." },
      { input: "wakelock dark", result: "Dark wake toggles: screen may sleep, system stays awake — ☾ srv in the footer." },
      { input: "wakelock off", result: "Back to normal sleep behaviour (either mode)." },
      { input: "caffeine dark on", result: "Same dark mode via the alias." },
    ],
    tips: [
      "`caffeine` is a drop-in alias if that's the muscle memory you have.",
      "Dark wake is the server mode: walk away, let the screen sleep, stay reachable over SSH/Claude Code.",
      "The ☾ button in the footer toggles dark wake with one click — no typing needed.",
    ],
    caveats: [
      "Dark wake does NOT survive closing the lid — clamshell sleep is OS-forced.",
      "The old `=1`/`=0` syntax was removed in v0.52.0 — use on/off.",
    ],
    related: ["stats", "uptime"],
  },

  // ── Productivity ──────────────────────────────────────────────────────
  {
    command: "bruno",
    aliases: [],
    category: CAT_PROD,
    version_added: "0.33.0",
    tagline: "German net-pay calculator — employees AND freelancers (tax year 2025).",
    tagline_de: "Netto-Rechner Deutschland — Angestellte UND Selbständige (Steuerjahr 2025).",
    synopsis: "bruno <€>[m|j]   ·   bruno <gewinn>[m]f   ·   bruno <einnahmen>-<ausgaben>f",
    description:
      "Computes German net pay (simplified §32a tariff, tax year 2025). Employee mode: gross salary with the employee share of social insurance; suffix `m` = monthly, `j` = yearly. Freelancer/self-employed mode (`f` suffix): yearly PROFIT — or `income-expenses` — with voluntary GKV (min/max assessment bounds) or a fixed PKV premium, full care-insurance rate, no pension/unemployment contributions, Grund- or Splittingtarif, and (for Gewerbebetriebe) Gewerbesteuer incl. the §35 income-tax credit. Personal parameters come from Settings → Bruno. VAT is a pass-through (§19 small-business hint only). Not tax advice.",
    arguments: [{ name: "€", required: true, description: "Gross amount (employee) or profit (self-employed with `f`); `einnahmen-ausgaben` computes the profit. Optional suffix `m` (monthly) or `j` (yearly).", default: undefined }],
    flags: [
      { flag: "f", value_type: undefined, description: "Self-employed calculation (freelancer/Gewerbe — set the Rechtsform, KV type, Hebesatz etc. in Settings → Bruno).", default: "employee mode" },
    ],
    examples: [
      { input: "bruno 4500", result: "Employee net pay for €4500 gross with your Settings defaults." },
      { input: "bruno 80000f", result: "Freelancer: net from €80k yearly profit (GKV/PKV per Settings)." },
      { input: "bruno 90000-15000f", result: "Self-employed: €90k income − €15k expenses → profit → net." },
    ],
    tips: [
      "Set tax class / Bundesland / children / church / health surcharge — and the freelancer options (Rechtsform, GewSt-Hebesatz, GKV/PKV, Splitting) — in Settings → Bruno once.",
      "Enter pastes the period-matched net amount; Shift+Enter copies the COMPLETE breakdown (assumptions + every deduction row + net) as aligned plain text — ready for a mail or note. Works in both modes.",
      "`bruno 7000mf` reads the €7 000 as MONTHLY profit (m and f combine).",
    ],
    caveats: [
      "A simplified model — not tax advice.",
      "Self-employed mode assumes no compulsory pension/unemployment insurance (exceptions like Künstlersozialkasse aren't modelled) and treats VAT as a pass-through.",
    ],
    related: ["faker"],
  },
  {
    command: "timer",
    aliases: [],
    category: CAT_PROD,
    version_added: "0.42.0",
    tagline: "Countdown timer — fires an alarm on expiry.",
    tagline_de: "Countdown-Timer — löst bei Ablauf einen Alarm aus.",
    synopsis: "timer <N>[s|min|h]",
    description:
      "Starts a countdown; on expiry it raises the alarm (a full-screen overlay you dismiss, or a native notification — Settings → Timer alarm). A footer LED shows the live count. Cancellable per timer.",
    arguments: [{ name: "N[s|min|h]", required: true, description: "Duration — seconds (default), `min`, or `h`.", default: undefined }],
    flags: [],
    examples: [
      { input: "timer 10min", result: "A 10-minute countdown; alarm at the end." },
      { input: "timer 90s", result: "90 seconds." },
      { input: "timer 2h", result: "Two hours (e.g. a parking meter)." },
    ],
    tips: ["Use `alarm <HH:MM>` for a wall-clock time instead of a duration."],
    caveats: [],
    related: ["alarm"],
  },
  {
    command: "alarm",
    aliases: [],
    category: CAT_PROD,
    version_added: "0.42.0",
    tagline: "Alarm at a clock time (next occurrence).",
    tagline_de: "Alarm zu einer Uhrzeit (nächstes Auftreten).",
    synopsis: "alarm <HH:MM>",
    description: "Schedules the alarm for the next occurrence of the given wall-clock time (reuses the timer scheduler). A status toast confirms.",
    arguments: [{ name: "HH:MM", required: true, description: "24-hour clock time (12-hour also parsed).", default: undefined }],
    flags: [],
    examples: [
      { input: "alarm 15:15", result: "Alarm at 15:15 today (or tomorrow if past)." },
      { input: "alarm 3:00", result: "3 AM." },
      { input: "alarm 06:30", result: "A wake-up alarm." },
    ],
    tips: ["For a duration instead of a clock time, use `timer`."],
    caveats: [],
    related: ["timer"],
  },
  {
    command: "pwgen",
    aliases: [],
    category: CAT_SEC,
    version_added: "0.40.0",
    tagline: "Password generator — CSPRNG, 4 modes.",
    tagline_de: "Passwort-Generator — CSPRNG, 4 Modi.",
    synopsis: "pwgen [N]",
    description:
      "Generates a strong password (Web Crypto CSPRNG, rejection-sampled to avoid modulo bias). Bare `pwgen` uses the default length; `pwgen 16` sets it. Four modes via preview buttons or ⌘/Ctrl+1…4: all (alnum+symbols), alnum, dict (CapitalisedWords + digits), leet (dict + vowel leet). Enter copies; Alt+Enter switches to alphanumeric + copies.",
    arguments: [{ name: "N", required: false, description: "Password length.", default: "12" }],
    flags: [],
    examples: [
      { input: "pwgen", result: "A 12-char password (all charset)." },
      { input: "pwgen 24", result: "A 24-char password." },
      { input: "pwgen 4", result: "Switch to dict/leet mode in the preview for a memorable one." },
    ],
    tips: ["This is the real (CSPRNG) generator — prefer it over `faker password`, which is a seedable toy."],
    caveats: [],
    related: ["faker", "uuid"],
  },

  // ── Testing & Security ────────────────────────────────────────────────
  {
    command: "faker",
    aliases: ["fake"],
    category: CAT_SEC,
    version_added: "0.84.270",
    tagline: "Realistic fake test data — 70+ generators, 14 locales, many formats.",
    tagline_de: "Realistische Fake-Testdaten — 70+ Generatoren, 14 Locales, viele Formate.",
    synopsis: "faker [gen] [n] [@locale] [--json|csv|sql[=table]|ts] [--seed=N]   ·   faker tpl \"<template>\" [n]",
    description:
      "Generates realistic test data. Bare `faker` lists 70+ generators with a live sample each; `faker <gen>` produces the default count, `faker <gen> <n>` n values. Composite records (person/user/address_full/company_full/order) plus scalars (names, emails, addresses, finance, lorem, dates, numbers, uuid…). Argument order is irrelevant; the default locale is DE_DE with an honest EN fallback shown when a generator isn't localised. `faker tpl \"…\"` renders a free template. ⌘/Ctrl+R rerolls. Also usable in snippets as `{faker:first_name}`.",
    arguments: [
      { name: "gen", required: false, description: "Generator name/alias (email, person, uuid, int…). Bare = catalogue.", default: "(catalogue)" },
      { name: "n", required: false, description: "How many values (1…10000).", default: "Settings default (1)" },
      { name: "range", required: false, description: "For numeric generators, e.g. `faker int 1..100`.", default: undefined },
    ],
    flags: [
      { flag: "@<locale>", value_type: "de|en|fr|it|pt|ja|zh|…", description: "Locale override for this call.", default: "Settings default (DE_DE)" },
      { flag: "--json", value_type: undefined, description: "JSON array output.", default: "plain" },
      { flag: "--csv", value_type: undefined, description: "CSV with header.", default: "plain" },
      { flag: "--sql", value_type: "[=table]", description: "INSERT statements (table defaults to the generator name).", default: undefined },
      { flag: "--ts", value_type: undefined, description: "TS object-literal array (test fixtures).", default: undefined },
      { flag: "--seed", value_type: "u64", description: "Reproducible output (same seed → byte-identical).", default: "(random, shown in preview)" },
    ],
    examples: [
      { input: "faker person 50 --csv @de", result: "50 German person records as valid CSV on the clipboard." },
      { input: "faker int 1..100", result: "A random integer in [1,100]." },
      { input: 'faker tpl "{name} <{email}>" 10', result: "10 rendered 'Name <email>' lines." },
    ],
    tips: [
      "⌘/Ctrl+R rerolls the preview with a fresh seed; the seed is shown so you can pin it with `--seed=`.",
      "`faker password` is a seedable toy — use `pwgen` for real passwords.",
    ],
    caveats: ["fake doesn't localise every generator in every locale; unsupported → EN, shown as a fallback chip."],
    related: ["pwgen", "uuid", "sec"],
    see_also: "docs/faker.md",
  },
  {
    command: "sec",
    aliases: ["nmap", "sqlmap", "feroxbuster", "ferox", "john"],
    category: CAT_SEC,
    version_added: "0.84.271",
    tagline: "Guided pentest-command builders — nmap · sqlmap · ferox · John.",
    tagline_de: "Geführte Pentest-Command-Builder — nmap · sqlmap · ferox · John.",
    synopsis: "sec [nmap|sqlmap|ferox|john] [preset] [target]   ·   <tool> [preset] [target]",
    description:
      "Assembles syntactically correct command lines for four standard tools from presets, with a plain-English cheat-sheet for every flag. Enter copies the (shell-quoted) command + pastes it — it never runs anything. ⌘/Ctrl+Enter opens your terminal (macOS) with the command inserted, un-submitted by default; sharp presets confirm first. Inspector Rust NEVER runs the tools, opens no sockets, spawns no tool subprocess. Authorized targets only. `sec john prepare` lists the *2john helpers.",
    arguments: [
      { name: "tool", required: false, description: "nmap · sqlmap · ferox · john (also directly, e.g. `nmap …`).", default: "(4-tool overview)" },
      { name: "preset", required: false, description: "A preset name (e.g. service, dump, dir, wordlist).", default: "(preset list)" },
      { name: "target", required: false, description: "Host/CIDR/URL/hash-file, shown in the command.", default: "‹placeholder›" },
    ],
    flags: [{ flag: "--key value", value_type: undefined, description: "Set an optional field by key (e.g. `--ports 1-1000`).", default: undefined }],
    examples: [
      { input: "nmap service 10.0.0.5", result: "Builds `nmap -sV -sC 10.0.0.5` + a flag cheat-sheet." },
      { input: "sec ferox dir http://host /usr/share/wordlists/common.txt", result: "A feroxbuster content-discovery command." },
      { input: "john wordlist hashes.txt", result: "`john --wordlist=<default> hashes.txt` (uses your Settings wordlist)." },
    ],
    tips: [
      "Enter copies; ⌘/Ctrl+Enter runs it in your terminal (opt-in, macOS).",
      "Set a scope note + default wordlist in Settings → Security.",
    ],
    caveats: [
      "The app builds text — it does not scan. Use only against systems you're authorized to test.",
      "The terminal hand-off is macOS-only (sh/bash quoting); elsewhere the command is clipboard-only.",
    ],
    related: ["faker", "terminal"],
    see_also: "docs/security-builder.md",
  },

  // ── Media & Screenshots ───────────────────────────────────────────────
  {
    command: "shot",
    aliases: ["shotfull", "shotwin", "shotlast"],
    category: CAT_MEDIA,
    version_added: "0.57.0",
    tagline: "Screenshot — region / full-screen / window / repeat, with a self-timer.",
    tagline_de: "Screenshot — Region / Vollbild / Fenster / Wiederholen, mit Selbstauslöser.",
    synopsis: "shot [seconds]   ·   shotfull [seconds]   ·   shotwin [seconds]   ·   shotlast",
    description:
      "Captures a screenshot to the clipboard + a floating preview (Save/Copy/Edit/Pin). `shot` = drag a region, `shotfull` = full screen, `shotwin` = the active window, `shotlast` = repeat the last mode. An optional self-timer delays the capture by N seconds.",
    arguments: [{ name: "seconds", required: false, description: "Self-timer delay before capture.", default: "0" }],
    flags: [],
    examples: [
      { input: "shot", result: "Drag a region; the capture lands on the clipboard with a preview." },
      { input: "shotfull 3", result: "Full-screen capture after a 3-second timer." },
      { input: "shotwin", result: "Capture the currently active window." },
    ],
    tips: ["The floating preview lets you annotate (Edit) or Pin the shot to the screen.", "`Ctrl+Shift+S` is the global region-screenshot hotkey."],
    caveats: ["macOS needs the Screen Recording permission."],
    related: ["trim", "md2pdf"],
  },
  {
    command: "trim",
    aliases: [],
    category: CAT_MEDIA,
    version_added: "0.84.28",
    tagline: "Trim a video/audio file — lossless-fast or frame-accurate.",
    tagline_de: "Video/Audio-Datei trimmen — verlustfrei-schnell oder frame-genau.",
    synopsis: "trim",
    description:
      "Opens a file picker, then a trim overlay with start/end sliders and a mode toggle: lossless (keyframe-snapped, `-c copy`, fast) or frame-accurate (re-encode). Saves a `<stem>-trim.<ext>` next to the source. Needs ffmpeg on PATH.",
    arguments: [],
    flags: [],
    examples: [
      { input: "trim", result: "Pick a file → set in/out → a trimmed sibling is written." },
      { input: "trim", result: "Lossless mode: instant, keyframe-snapped cut." },
      { input: "trim", result: "Frame-accurate mode: exact cut via re-encode." },
    ],
    tips: ["Lossless is fastest; switch to frame-accurate only when you need an exact boundary."],
    caveats: ["Requires ffmpeg installed on PATH."],
    related: ["shot", "md2pdf"],
  },
  {
    command: "md2pdf",
    aliases: [],
    category: CAT_MEDIA,
    version_added: "0.46.0",
    tagline: "Markdown → PDF (GitHub CSS), sibling file.",
    tagline_de: "Markdown → PDF (GitHub-CSS), Nachbardatei.",
    synopsis: "md2pdf [path]",
    description:
      "Converts Markdown (CommonMark + GFM) to a PDF with embedded GitHub CSS, written next to the source (`foo.md` → `foo.pdf`). Bare `md2pdf` uses the Finder selection (macOS); or pass an explicit path. Same as the `Ctrl+Shift+M` hotkey.",
    arguments: [{ name: "path", required: false, description: "Path to a .md file. Omit to use the Finder selection.", default: "(Finder selection)" }],
    flags: [],
    examples: [
      { input: "md2pdf", result: "Converts the Markdown file selected in Finder → PDF beside it." },
      { input: "md2pdf ~/notes/README.md", result: "Converts an explicit path." },
      { input: "md2pdf", result: "GFM tables/code fences render with GitHub styling." },
    ],
    tips: ["The macOS backend is WKWebView `createPDF` — no external tool needed."],
    caveats: ["The selection path + Ctrl+Shift+M are macOS-only; on Windows pass an explicit path. No Linux backend yet."],
    related: ["shot"],
  },
  {
    command: "meme",
    aliases: [],
    category: CAT_FUN,
    version_added: "0.70.0",
    tagline: "Browse your meme folder, copy the picked GIF/image.",
    tagline_de: "Meme-Ordner durchsuchen, gewähltes GIF/Bild kopieren.",
    synopsis: "meme [query]",
    description:
      "Fuzzy-browses a folder of GIFs/images (Settings → Meme library; default `~/My Drive/media/memes`) and copies the selected one on Enter — as a file-URL on macOS so animation is preserved when pasting into chat apps. The selected meme previews animated in the preview pane.",
    arguments: [{ name: "query", required: false, description: "Fuzzy filter over name + category folder.", default: "(all)" }],
    flags: [],
    examples: [
      { input: "meme", result: "The whole library; arrow through, Enter copies." },
      { input: "meme cat", result: "Only memes matching 'cat'." },
      { input: "meme facepalm", result: "Fuzzy-find a reaction GIF." },
    ],
    tips: ["Set the folder in Settings → Meme library (needed on Windows with a Drive letter path)."],
    caveats: ["A meme-less build (`build:*:nomeme`) omits this command entirely."],
    related: [],
  },
  {
    command: "shazam",
    aliases: [],
    category: CAT_MEDIA,
    version_added: "0.84.250",
    tagline: "Recognise the song playing from the mic.",
    tagline_de: "Den laufenden Song über das Mikro erkennen.",
    synopsis: "shazam [history]",
    description:
      "Records ~10 s from the microphone, generates a Shazam audio-signature, queries Shazam's public API, and shows the matched track (cover · title · artist · album · year) with Shazam/Spotify/YouTube links + lyrics. `shazam history` opens past recognitions. Native mic capture (no webview) so playback isn't disturbed. The recognition runs in the backend, so it keeps going even if you close the overlay — the match is saved to history and (if the panel is closed) pops a toast.",
    arguments: [{ name: "history", required: false, description: "Open the recognition history instead of listening.", default: "(listen)" }],
    flags: [],
    examples: [
      { input: "shazam", result: "Listens ~10 s, then shows the matched song + links." },
      { input: "shazam history", result: "Your past recognitions with links + per-row delete." },
      { input: "shazam", result: "R re-records, Esc exits." },
    ],
    tips: [
      "R re-records, L opens the LYRICS of the match (in-app via lrclib.net, browser-search fallback; a +DE toggle shows a German translation under each line), Enter copies “Title – Artist”.",
      "The Spotify button opens the song in the Spotify desktop app when it's installed, else the web player; Shazam (blue) / Spotify (green) / YouTube (red) are brand icons — hover for the tooltip.",
      "Close the overlay mid-listen and the recognition keeps running in the backend — the match still lands in history and a toast shows the song.",
      "`shazam history` opens your recognized-songs list directly — with a search field over title · artist · album · genre.",
    ],
    caveats: ["Needs a microphone + network (the recognition query). No match → a clear 'no match' state."],
    related: [],
  },

  // ── Cleanup / Display / Info ──────────────────────────────────────────
  {
    command: "clean",
    aliases: ["cleanup"],
    category: CAT_SYS,
    version_added: "0.60.0",
    tagline: "Reclaim disk space — cache/log/temp + developer junk, folder picker.",
    tagline_de: "Speicher freimachen — Cache/Log/Temp + Dev-Müll, Ordner-Picker.",
    synopsis: "clean",
    description:
      "A safety-first disk cleaner: a dry-run scan renders as a checkbox list of directories (size + largest files) grouped by category. Tick what to delete, press Enter twice (arm → confirm) to sweep only the checked categories. Strict allowlist, symlinks never followed, deletion is always file-by-file with re-validation. Levels Safe/Standard/Aggressive + developer targets (stale node_modules/target, JetBrains/Xcode leftovers, Docker/brew/pnpm) in Settings → Cleaning. Scan and delete keep running after Esc — reopen with `clean` to review a finished scan, or wait for the Cleaned toast.",
    arguments: [],
    flags: [],
    examples: [
      { input: "clean", result: "Scan → tick categories → Enter twice → space reclaimed." },
      { input: "clean", result: "Space toggles a row; A toggles all; the selected row shows its 3 largest files." },
      { input: "clean", result: "Downloads dupes / old installers are offered but pre-deselected." },
    ],
    tips: [
      "Configure the level + developer roots + per-category toggles in Settings → Cleaning.",
      "Esc mid-scan or mid-delete closes the overlay only — the job finishes in the background (toast when done).",
    ],
    caveats: ["It deletes user files — always dry-runs first and re-validates every path against a strict allowlist before deleting."],
    related: ["stats"],
    see_also: "docs/cleanup.md",
  },
  {
    command: "brightness",
    aliases: ["bri"],
    category: CAT_AV,
    version_added: "0.62.0",
    tagline: "Per-monitor brightness sliders in the preview.",
    tagline_de: "Helligkeits-Slider pro Monitor im Preview.",
    synopsis: "brightness   ·   bri",
    description:
      "Opens a brightness slider per monitor (+ an 'all' master) in the preview pane. macOS/Windows use software gamma dimming (works on the built-in panel AND external/adapter monitors); Linux uses DDC/CI. On EDR-capable Macs the slider runs past 100% into the display's HDR headroom. ↑/↓ pick a monitor, ←/→ adjust ±5, Enter hands the arrows back, Esc exits.",
    arguments: [],
    flags: [],
    examples: [
      { input: "brightness", result: "Sliders for each monitor; arrow keys adjust." },
      { input: "bri", result: "Same thing, shorter." },
      { input: "brightness", result: "On an XDR Mac, push past 100% for extra HDR brightness." },
    ],
    tips: ["Software dimming can only go darker than native — it reduces emitted light, not backlight."],
    caveats: ["Gamma dimming resets to 100% at logout (there's no 'read current backlight')."],
    related: ["sound", "stats"],
  },
  {
    command: "sound",
    aliases: ["audio"],
    category: CAT_AV,
    version_added: "0.80.0",
    tagline: "Audio output picker + a system volume slider.",
    tagline_de: "Audio-Ausgabe-Wähler + System-Lautstärke-Slider.",
    synopsis: "sound   ·   audio",
    description:
      "Opens an audio panel in the preview: a volume slider at the top (←/→ ∓5, or click/drag) and the list of output devices below (↑/↓ select, Enter switches the system default output, Esc exits). Shows directly on typing `sound`/`audio` — Enter only hands it keyboard focus.",
    arguments: [],
    flags: [],
    examples: [
      { input: "sound", result: "Volume slider + output device list." },
      { input: "audio", result: "Same panel (alias)." },
      { input: "sound", result: "Switch output from speakers to headphones." },
    ],
    tips: ["`mute` toggles mute; Shift+↑/↓ in the popup nudges volume without opening the panel."],
    caveats: ["Windows has no cheap volume read-back, so the slider hides there (device switching still works)."],
    related: ["mute", "boom"],
  },
  {
    command: "boom",
    aliases: [],
    category: CAT_AV,
    version_added: "0.84.143",
    tagline: "System-wide audio EQ + presets + volume boost.",
    tagline_de: "Systemweiter Audio-EQ + Presets + Lautstärke-Boost.",
    synopsis: "boom",
    description:
      "An inline audio-enhancement controller: a 10-band graphic EQ, 20 presets, pre-amp, volume boost, and 5 enhancement effects (bass/clarity/fidelity/ambience/night) applied to ALL system audio, with live in/out level meters. macOS installs a small virtual audio driver from the panel (one click); Windows drives Equalizer APO. Battery-aware: the bridge suspends after 60 s of silence.",
    arguments: [],
    flags: [],
    examples: [
      { input: "boom", result: "The EQ/preset panel; toggle on to enhance system audio." },
      { input: "boom", result: "Pick a genre preset, tweak the 10 bands." },
      { input: "boom", result: "Off = audio passes through untouched." },
    ],
    tips: ["macOS needs the one-time driver install (button in the panel)."],
    caveats: ["Distribution needs a signed driver; the bundled one is ad-hoc (loads locally)."],
    related: ["sound", "mute", "disco"],
  },
  {
    command: "hue",
    aliases: [],
    category: CAT_AV,
    version_added: "0.84.40",
    tagline: "Philips Hue lamp controller (local, LAN-only).",
    tagline_de: "Philips-Hue-Lampensteuerung (lokal, nur LAN).",
    synopsis: "hue",
    description:
      "An inline Hue controller in the preview: an 'All lamps' master + a row per lamp (Tab/↑↓ select, ←→ brightness, Enter/Space on/off, 1–8 colour presets). Local-first — all bridge traffic is plain HTTP on the LAN, discovery via SSDP; no Philips cloud. First use pairs by pressing the bridge's link button.",
    arguments: [],
    flags: [],
    examples: [
      { input: "hue", result: "Lamp list; arrow/number keys control brightness + colour." },
      { input: "hue", result: "First run: a connect card → press the bridge button to pair." },
      { input: "hue", result: "1–8 pick colour presets; the selected row auto-centres." },
    ],
    tips: ["`disco` beat-syncs the same lamps to your microphone."],
    caveats: ["Needs a Hue Bridge on your LAN; group commands are rate-limited (~1/s)."],
    related: ["disco", "boom"],
  },
  {
    command: "iris",
    aliases: [],
    category: CAT_AV,
    version_added: "0.102.0",
    tagline: "Red screen-edge glow whenever the microphone gets too loud.",
    tagline_de: "Rotes Glimmen an den Bildschirmrändern, sobald das Mikrofon zu laut wird.",
    synopsis: "iris [dB]   ·   iris 0",
    description:
      "Watches the default microphone in the background and lights a field of drifting red blobs around the edges of every screen while the level exceeds your threshold — a peripheral 'you are being loud' signal that needs no glance at a window. The overlays are click-through and never take focus, and no text is drawn, so the machine stays fully usable. Soft, punchy light flares fire along the edges of the screen on an irregular beat over a muted drifting field — strobing attack, shimmering hold, fast progressive fade, in the raspi5 dB-analysis page's warm red-to-amber palette; the middle of the screen always stays clear. Typing a number arms it immediately — no Enter — and editing the number retunes the running session live, marker included. When music plays, a built-in beat detector locks the strobe to the kicks — every monitor flashes on the same beat, harder kicks hit harder; without a detectable beat the impulses fall back to an irregular cadence that tightens with loudness. The threshold is SPL on the disco-controller convention (dBFS + 90), so a value that works on raspi5 means the same here.",
    arguments: [
      {
        name: "dB",
        required: false,
        description:
          "Threshold in SPL (30-100). Omit to toggle using the last saved value; 0 switches monitoring off.",
        default: "55 (the raspi5 warn_thr default)",
      },
    ],
    flags: [],
    examples: [
      { input: "iris 55", result: "Arms at 55 dB the moment you stop typing — no Enter — and shows the live meter." },
      { input: "iris", result: "Toggles: arms with the last threshold, or disarms a running session." },
      { input: "iris 0", result: "Switches monitoring off explicitly." },
      { input: "iris 72,5", result: "Arms at 72.5 dB — a comma decimal works too." },
    ],
    tips: [
      "Edit the number in the search bar to retune live — type `iris 50`, watch the meter, nudge to `iris 55`; every change applies without Enter and the threshold marker follows.",
      "Leaving the panel with Esc does NOT disarm: monitoring keeps running with the popup closed, which is the whole point. Use `iris` again or `iris 0` to stop it.",
      "The threshold is persisted, so a bare `iris` re-arms at the value you last tuned.",
    ],
    caveats: [
      "Monitoring never auto-starts after an app restart — opening the microphone silently at login would be a nasty surprise.",
      "A hysteresis of 2 dB plus a 400 ms minimum hold keeps the vignette from flickering at the threshold, so it stays lit slightly longer than the raw level would suggest.",
      "The vignette may not draw over an app running in native macOS fullscreen.",
    ],
    related: ["sound", "boom", "disco"],
  },
  {
    command: "disco",
    aliases: [],
    category: CAT_AV,
    version_added: "0.84.43",
    tagline: "Beat-sync Hue lamps to the mic — keeps running after close.",
    tagline_de: "Hue-Lampen zum Mikro beat-syncen — läuft nach dem Schließen weiter.",
    synopsis: "disco 1|0",
    description:
      "A mic-driven 'disco': on each confident beat it drives the lamps as a round-robin colour chase. `disco 1` = on, `disco 0` = off, bare `disco` = toggle. Modes rainbow/pulse/strobe + a sensitivity slider. The engine is a persistent singleton, so it keeps running after the popup closes.",
    arguments: [{ name: "1|0", required: false, description: "1 = on, 0 = off; omit to toggle.", default: "(toggle)" }],
    flags: [],
    examples: [
      { input: "disco 1", result: "Lamps start beat-syncing to the microphone." },
      { input: "disco 0", result: "Stops; lamps restored to warm white." },
      { input: "disco", result: "Toggles on/off." },
    ],
    tips: ["It reuses the same BpmAnalyzer as the `bpm` detector; tune sensitivity in the panel."],
    caveats: ["rAF is throttled while the window is hidden, so detection currently pauses while the popup is closed."],
    related: ["hue", "boom"],
  },

  // ── Info & Monitoring ─────────────────────────────────────────────────
  {
    command: "adb",
    aliases: [],
    category: CAT_SYS,
    version_added: "0.119.0",
    tagline: "Control your Android phone — dashboard, remote, screenshot, apps, WiFi-ADB.",
    tagline_de: "Android-Handy steuern — Dashboard, Fernbedienung, Screenshot, Apps, WLAN-ADB.",
    synopsis: "adb [remote|apps|wifi]",
    description:
      "The popup-sized companion to the ADBOSS desktop app: five views over a USB- or WiFi-connected Android device. **Info** — live dashboard (model, Android version, battery with temperature, RAM, storage, WLAN/IP/RSSI, resolution) polling every 5 s, plus device screenshot (→ Mac clipboard + history) and screen recording (→ ~/Downloads). **Steuern** — WiFi/Bluetooth/airplane/DND toggles, brightness + media-volume sliders, screen wake/sleep/lock. **Remote** — navigation keys, D-pad, send text (ASCII), tap/swipe at coordinates. **Apps** — search installed packages, launch / force-stop / clear data / uninstall (with confirmation). **WLAN** — switch a USB device to TCP/IP mode and connect wirelessly (cable off afterwards), or connect a known ip:port directly. Every device command is the battle-tested ADBOSS form; values are strictly validated before touching a shell.",
    arguments: [
      { name: "remote|apps|wifi", required: false, description: "Open the panel directly on that view; bare `adb` starts on the dashboard.", default: "Info" },
    ],
    flags: [],
    examples: [
      { input: "adb", result: "Dashboard + quick controls for the connected device." },
      { input: "adb remote", result: "Remote control: keys, D-pad, text, tap/swipe." },
      { input: "adb apps", result: "Search apps, launch/stop/uninstall." },
      { input: "adb wifi", result: "Enable WiFi-ADB on the USB device or connect an ip:port." },
    ],
    tips: [
      "One-time phone setup: Developer options → USB debugging, then confirm the RSA dialog on the phone (details: Settings → Android (adb)).",
      "Screenshot lands straight in the Mac clipboard AND the history — paste it anywhere immediately.",
      "WiFi flow: plug in USB once → adb wifi → \"WLAN-ADB aktivieren\" — the cable can come off.",
    ],
    caveats: [
      "Needs the adb binary (brew install android-platform-tools) — the panel shows an install card if it's missing.",
      "`input text` can only deliver ASCII — umlauts/emoji are rejected with a hint, not silently garbled.",
      "Logcat, Bluetooth-HCI analysis and file transfer deliberately stay in ADBOSS (they need a full window).",
    ],
    related: ["stats", "loc"],
    see_also: "docs/adb.md",
  },
  {
    command: "repo",
    aliases: ["export"],
    category: CAT_INFO,
    version_added: "0.123.0",
    tagline: "Git repository activity stats — commits, contributors, hotspots.",
    tagline_de: "Git-Repo-Statistik — Commits, Mitwirkende, Hotspots.",
    synopsis: "repo [url|pfad]   ·   export [url|pfad]",
    description:
      "Analyses a git repository's history and shows it in the preview, oriented on the repo2viz project: KPI tiles (commits, contributors, active days, longest streak, lines added/removed), a month-activity sparkline, weekday and hour-of-day charts, commit-category bars (conventional commits), and the most-active files, file types and contributors. Give a GitHub URL (read-only bare clone), a local path, or nothing — then it uses the folder selected in Finder if it's a git repo. The `export` command (or the ⬇ button / `E`) writes the same analysis as a single self-contained HTML file to ~/Downloads, named <owner>-<repo>-activity.html.",
    arguments: [
      { name: "url|pfad", required: false, description: "A git URL or local path. Omit to analyse the Finder-selected .git folder.", default: "Finder selection" },
    ],
    flags: [],
    examples: [
      { input: "repo https://github.com/pepperonas/inspector-rust", result: "Clones read-only and shows the activity stats." },
      { input: "repo", result: "Analyses the folder selected in Finder (if it's a git repo)." },
      { input: "export https://github.com/user/projekt", result: "Writes user-projekt-activity.html to ~/Downloads." },
    ],
    tips: [
      "In the panel, E (or ⬇) exports the current analysis as a self-contained HTML report.",
      "Local repos are analysed in place (no clone) — instant for your own projects.",
      "Churn = lines added + removed; the hotspots are ranked by how often a file changed.",
    ],
    caveats: [
      "A URL does a full bare clone (history + blobs for exact churn) — a very large repo takes a while.",
      "Needs git on PATH.",
    ],
    related: ["loc", "disk"],
    see_also: "docs/repo.md",
  },
  {
    command: "clown",
    aliases: [],
    category: CAT_FUN,
    version_added: "0.132.0",
    tagline: "tExT sO sChReIbEn — a gallery of silly text styles.",
    tagline_de: "TeXt sO sChrEibEn — Galerie alberner Textstile.",
    synopsis: "clown [text] [@style]",
    description:
      "Turns text into something sillier and lists EVERY style at once, each row showing your text already transformed — pick with the arrow keys, Enter pastes it. Styles: **Clown** (alternating case + occasional leet, the default), **Mock**, **Leet**, **Vaporwave** (fullwidth), **Double-struck**, **Fett**, **Schreibschrift**, **Kapitälchen**, **Kopfüber**, **Durchgestrichen**, **Klatschen** (👏 between words) and **Gesperrt**. Without an argument it takes the current clipboard text, so you don't have to retype a long passage. Every transform is deterministic — the same text always gives the same result, so the preview never flickers while you type.",
    arguments: [
      { name: "text", required: false, description: "The text to mangle. Omitted → the clipboard's text.", default: "clipboard" },
      { name: "@style", required: false, description: "Jump straight to one style, e.g. `@leet` — it floats to the top of the list.", default: "all styles" },
    ],
    flags: [],
    examples: [
      { input: "clown so schreiben kann", result: "sO sChrEibEn K4nN — plus every other style below it." },
      { input: "clown", result: "Takes the clipboard text and shows it in all styles." },
      { input: "clown hallo @upside", result: "ollɐɥ — the upside-down style first." },
    ],
    tips: [
      "Bare `clown` reads the clipboard — copy a paragraph, then just type `clown`.",
      "The list shows your text already transformed, so you pick by looking, not by guessing.",
    ],
    caveats: [
      "The Unicode styles (double-struck, script, small caps, …) are look-alike letters, not formatting: screen readers announce them character by character, some apps and fonts show boxes instead, and search/autocorrect no longer recognise the words. Fine for chats and toots — not for anything that matters.",
      "Small caps have no glyph for `q` and `x`, so those two stay plain; upside-down leaves unmapped characters as they are.",
    ],
    related: ["figlet", "faker"],
  },
  {
    command: "rickroll",
    aliases: [],
    category: CAT_FUN,
    version_added: "0.122.0",
    tagline: "You know the rules — and so do I.",
    tagline_de: "Du kennst die Regeln — und ich auch.",
    synopsis: "rickroll",
    description:
      "Plays Rick Astley's \"Never Gonna Give You Up\" — with sound — right in the preview column (from a BUNDLED, heavily compressed clip — 480p: zero network, and no YouTube-embed failures (the iframe died in the webview with Error 153). A scrolling lyric marquee runs beneath it; native player controls, a restart button and an open-in-browser fallback round it out.",
    arguments: [],
    flags: [],
    examples: [
      { input: "rickroll", result: "The video autoplays (with sound) in the preview." },
      { input: "rickroll → ↻ Nochmal", result: "Restarts the clip from the top." },
      { input: "rickroll → Browser", result: "Opens the video on YouTube instead." },
    ],
    tips: [
      "No sound? Some webviews block autoplay-with-sound — hit \"Im Browser abspielen\".",
      "\"↻ Nochmal\" reloads the clip from the top.",
    ],
    caveats: ["Needs a network connection — the video streams from YouTube."],
    related: [],
  },
  {
    command: "alias",
    aliases: [],
    category: CAT_SYS,
    version_added: "0.127.0",
    tagline: "Guided shell-alias builder — per-OS one-liners + direct create.",
    tagline_de: "Shell-Alias geführt anlegen — Befehl je OS + direkt anlegen.",
    synopsis: "alias [name]",
    description:
      "Opens a guided alias builder in the preview: enter the terminal command and the alias name, and the panel shows the exact one-liner that creates the alias on each OS — macOS (zsh, ~/.zshrc), Linux (bash, ~/.bashrc) and Windows (PowerShell $PROFILE, as a function so arguments are forwarded). Every row has a copy button; the current OS's row has an extra create button that appends the alias to your shell config directly. Below the builder, the aliases already defined in your rc file are listed — searchable, alphabetical — with edit (fills the builder; the button flips to update) and delete. Quoting is handled for you — commands containing quotes survive verbatim.",
    arguments: [
      { name: "name", required: false, description: "Pre-fills the alias-name field.", default: "—" },
    ],
    flags: [],
    examples: [
      { input: "alias", result: "Opens the builder with empty fields." },
      { input: "alias gs", result: "Builder with the alias name pre-filled as gs." },
      { input: "alias kgc", result: "Type `git gc --aggressive` as the command → per-OS create one-liners." },
    ],
    tips: [
      "The create button writes to THIS machine's shell config (~/.zshrc here); copy the other rows for your other machines.",
      "A new alias applies in the next terminal — or after `source ~/.zshrc`.",
      "Editing an existing alias: click its pencil in the list — the create button becomes an update.",
    ],
    caveats: [
      "Fish is refused honestly (different alias syntax) — the panel's zsh/bash lines don't apply there.",
      "Windows creates a PowerShell *function* (Set-Alias can't carry arguments); runtime-unverified per house convention.",
    ],
    related: ["terminal", "touch"],
  },
  {
    command: "nosleep",
    aliases: [],
    category: CAT_SYS,
    version_added: "0.124.0",
    tagline: "Keep the Mac awake on AC — persistently (pmset profile).",
    tagline_de: "Mac am Netzteil dauerhaft wachhalten (pmset-Profil).",
    synopsis: "nosleep [on|off]",
    description:
      "Toggles the PERSISTENT AC idle-sleep profile: `pmset -c sleep 0` so the Mac never idle-sleeps on wall power, surviving reboots until you turn it off. The write goes through one admin prompt (no terminal sudo). The panel shows the live AC + battery sleep timeouts and a switch; `nosleep on` / `nosleep off` act straight away. Turning it off restores the timeout it overwrote. Different from `wakelock dark`, which is a session-only assertion that ends on the next reboot; nosleep changes the stored profile. Only the SYSTEM sleep on AC is locked — the display still sleeps normally.",
    arguments: [
      { name: "on|off", required: false, description: "on = never sleep on AC; off = restore the previous timeout. Omit to open the panel.", default: "panel" },
    ],
    flags: [],
    examples: [
      { input: "nosleep", result: "Opens the panel with the live AC/battery profile + a switch." },
      { input: "nosleep on", result: "pmset -c sleep 0 (admin prompt) — never idle-sleep on AC." },
      { input: "nosleep off", result: "Restores the previous AC sleep timeout." },
    ],
    tips: [
      "For just this session without admin rights, use `wakelock dark` instead.",
      "The footer's sleep indicator shows \"no-sleep\" while this is active.",
      "Only system sleep is locked — your display still sleeps on its own timer.",
    ],
    caveats: [
      "macOS only; writes the stored pmset profile and needs an admin prompt.",
      "`off` restores the timeout IR overwrote, or 1 minute if none was remembered.",
    ],
    related: ["wakelock", "stats"],
  },
  {
    command: "clock",
    aliases: [],
    category: CAT_INFO,
    version_added: "0.121.0",
    tagline: "World clock — live times for the world's major cities.",
    tagline_de: "Weltzeituhr — Live-Zeiten der wichtigsten Städte.",
    synopsis: "clock",
    description:
      "A live world clock in the preview: a card per timezone with the current time (seconds ticking), the local date, a ±1-day chip when it's another calendar day there, the UTC offset, and a sun/moon wash for day vs night. Ships with a spread of major capitals; add any city via the autocomplete search (matches city, country or IANA timezone id) and remove one with the × on its card. Times come from the operating system's timezone database, so daylight-saving is always correct. Your set of clocks is saved and restored across sessions.",
    arguments: [],
    flags: [],
    examples: [
      { input: "clock", result: "The world-clock grid; type a city to add one." },
      { input: "clock → \"tokio\"", result: "Autocomplete → Enter adds Tokyo." },
      { input: "clock → \"pacific/auckland\"", result: "Any IANA zone works by typing its id." },
    ],
    tips: [
      "Type a country (\"japan\") or a raw timezone id (\"asia/dubai\") — not just city names.",
      "The ±1-day chip and UTC offset make scheduling across zones a glance, not mental math.",
      "Your clocks persist — the set you build is there next time.",
    ],
    caveats: [
      "Offsets and DST come from the OS timezone database; a very outdated macOS could lag a recent DST rule change.",
    ],
    related: ["calendar", "weather"],
  },
  {
    command: "disk",
    aliases: ["daisy"],
    category: CAT_INFO,
    version_added: "0.120.0",
    tagline: "DaisyDisk-style disk usage — a sunburst of what's eating your space.",
    tagline_de: "Speicher-Analyse à la DaisyDisk — Sunburst, was den Platz frisst.",
    synopsis: "disk [pfad]   ·   daisy [pfad]",
    description:
      "Scans a folder and draws its disk usage as a concentric sunburst (like the DaisyDisk app): each ring is a directory level, each segment a folder/file sized by the space it actually occupies on disk. The centre hub shows the hovered item's size + share; a volume bar shows free space and how much of the whole disk this folder accounts for. A **path bar** always names the folder on screen and every segment of it is clickable, so you can browse the whole disk without retyping — click a ring segment to zoom in, `⌫` or Esc to walk back out, past the folder you started in. A largest-files list sits below, and any item can be moved to the Trash (the DaisyDisk collector) with confirmation. Bare `disk` scans the folder **selected in Finder**, else your home folder; `disk <pfad>` an explicit folder; `disk /` the whole volume (then free space shows too). On-disk size (allocated blocks), symlinks not followed, stays on one filesystem.",
    arguments: [
      { name: "pfad", required: false, description: "Folder to scan. Omit to use the Finder selection, else the home folder; `/` for the whole volume.", default: "Finder selection, else home" },
    ],
    flags: [],
    examples: [
      { input: "disk", result: "Sunburst of the folder selected in Finder — or your home folder." },
      { input: "disk /", result: "The whole boot volume, free space included." },
      { input: "daisy ~/Downloads", result: "Same view (alias) for a specific folder." },
    ],
    tips: [
      "Click a ring segment to zoom into that folder; `⌫` (or Esc) walks back out — past the scan root, so you can browse anywhere.",
      "**The list under the chart is the way into small folders.** The sunburst is area-proportional, so a 2 MB `src` next to a 20 GB `target` is a sub-pixel sliver you cannot click — the list has every child regardless of size (`↑↓` select, Enter opens).",
      "The path bar shows exactly which folder you are looking at; click any segment of it to jump straight there.",
      "The largest-files list and any segment have a trash button — it moves to the Trash (recoverable), then re-scans.",
      "Sizes are on-disk (allocated blocks), so they match what the volume readout says — not apparent size.",
    ],
    caveats: [
      "A full home/volume scan walks 10⁵–10⁶ files — it takes a few seconds (a live count shows progress).",
      "Scanning protected system paths under `/` may need Full Disk Access in System Settings; unreadable folders are skipped, never fatal.",
      "The chart is bounded (top folders per ring, ~5 rings) so it stays legible — the largest-files list is computed over everything.",
      "A folder far smaller than its siblings gets no visible arc at all. That is honest, not a bug — the chart shows proportion; use the list below it to get in.",
    ],
    related: ["loc", "stats"],
    see_also: "docs/disk.md",
  },
  {
    command: "loc",
    aliases: [],
    category: CAT_INFO,
    version_added: "0.117.0",
    tagline: "Lines-of-code statistics for the Finder selection — per language, with charts.",
    tagline_de: "Lines-of-Code-Statistik für die Finder-Auswahl — pro Sprache, mit Charts.",
    synopsis: "loc [pfad]",
    description:
      "Counts lines of code with tokei (~200 languages, real per-language syntax knowledge — block comments, strings, nested comments, embedded languages). Bare `loc` counts the folder(s) selected in Finder; `loc <pfad>` an explicit path. The preview shows totals (files / lines / code / comments / blanks), a stacked language bar, a donut chart of language shares, and a per-language table. Comments INCLUDE documentation (doc comments, Python docstrings). By default `.gitignore` is respected (inside git repos) and hidden files are skipped — a toggle counts everything, node_modules included.",
    arguments: [
      { name: "pfad", required: false, description: "Explicit folder/file to count. Omit it to use the live Finder selection (macOS).", default: "Finder selection" },
    ],
    flags: [],
    examples: [
      { input: "loc", result: "Counts the folder selected in Finder — language table + charts in the preview." },
      { input: "loc ~/claude/inspector-rust", result: "Counts an explicit path, no Finder needed." },
      { input: "loc src", result: "Relative paths resolve against the app's working dir — prefer absolute paths." },
    ],
    tips: [
      "Export als HTML, PDF oder PNG über die drei Knöpfe — die Datei landet in `~/Downloads` und wird im Finder gezeigt.",
      "Der Pfad über der Statistik ist klickbar, und die Unterordner-Reihe darunter zählt einen Ordner tiefer — `⌫` geht wieder hoch.",
      "R re-counts (e.g. after edits); the checkbox at the bottom includes ignored + hidden files.",
      "A folder's .gitignore only filters inside a git repo — a plain folder with a .gitignore counts everything.",
    ],
    caveats: [
      "Bare `loc` needs the Finder Automation permission (like Ctrl+Shift+F); `loc <pfad>` works without it.",
      "A separate docs-vs-comments split is not reliably possible across languages — documentation counts as comments (same as IntelliJ's Statistic plugin).",
    ],
    related: ["stats", "uptime"],
  },
  {
    command: "stats",
    aliases: [],
    category: CAT_INFO,
    version_added: "0.84.59",
    tagline: "Live system dashboard — CPU/mem/battery/sensors/disks/net + history.",
    tagline_de: "Live-System-Dashboard — CPU/RAM/Akku/Sensoren/Disks/Netz + Verlauf.",
    synopsis: "stats",
    description:
      "A read-only, auto-refreshing dashboard in the preview: CPU (overall + per-core), memory/swap, battery & instantaneous watts, temps/fans, disks, live network throughput, host/uptime. A Live/History toggle plots the last 1h/6h/24h/7d per metric (an always-on background collector samples every 60 s). ↑/↓ scroll, Esc exits.",
    arguments: [],
    flags: [],
    examples: [
      { input: "stats", result: "The live dashboard, refreshing every 1.5 s." },
      { input: "stats", result: "Toggle to History for per-metric line charts." },
      { input: "stats", result: "Watch live power draw (watts) + fan RPM." },
    ],
    tips: ["Sources degrade gracefully — a missing sensor is omitted, never faked."],
    caveats: ["Fan RPM: macOS SMC / Linux hwmon only; Windows has no rootless fan API."],
    related: ["uptime", "clean", "snitch"],
  },
  {
    command: "tokens",
    aliases: ["usage"],
    category: CAT_INFO,
    version_added: "0.101.0",
    tagline: "Claude Code token usage — cost, projects, sessions & models.",
    tagline_de: "Claude-Code-Tokenverbrauch — Kosten, Projekte, Sessions & Modelle.",
    synopsis: "tokens",
    description:
      "Shows your Claude Code usage from the local Token Tracker dashboard (port 5010) in the preview: totals & API-equivalent cost, projects/sessions, and model breakdown. Opens on Today (fast parallel fetch); period chips Today / 7d / 30d / All; sessions load only when that list is opened; toggle cache tokens on/off. Needs the Token Tracker running — otherwise a start-hint card is shown. ↑/↓ scroll, R refresh, Esc exits.",
    arguments: [],
    flags: [],
    examples: [
      { input: "tokens", result: "Open the usage panel on Today (fast)." },
      { input: "usage", result: "Same panel via the alias." },
      { input: "tokens", result: "Switch to Models to see Opus vs Sonnet spend." },
    ],
    tips: [
      "Costs are API-equivalent estimates (cache tokens dominate) — not your Claude subscription bill.",
      "Start Token Tracker first (LaunchAgent io.celox.token-tracker or `node server.js` in that repo).",
      "A zero Today with yesterday’s totals shown means no Claude Code JSONL yet — Cursor/Composer isn’t tracked.",
    ],
    caveats: [
      "Requires the local Token Tracker on http://127.0.0.1:5010 — Inspector Rust does not parse ~/.claude JSONL itself.",
    ],
    related: ["stats", "track", "shazam"],
  },
  {
    command: "uptime",
    aliases: [],
    category: CAT_INFO,
    version_added: "0.84.64",
    tagline: "Live, animated uptime readout.",
    tagline_de: "Live-animierte Uptime-Anzeige.",
    synopsis: "uptime",
    description: "Shows the system uptime in a readable Dd HH:MM:SS.mmm clock (with a shimmering millisecond tail) plus the total seconds down to microseconds, both animated. Cheap — one lightweight call anchored to a rAF loop, no per-frame IPC.",
    arguments: [],
    flags: [],
    examples: [
      { input: "uptime", result: "e.g. 3d 04:12:07.318 — ticking live." },
      { input: "uptime", result: "The boot timestamp is shown underneath." },
      { input: "uptime", result: "Esc closes it." },
    ],
    tips: ["A cheap, pretty way to check how long the machine's been up; `stats` has the full picture."],
    caveats: [],
    related: ["stats"],
  },
  {
    command: "snitch",
    aliases: [],
    category: CAT_INFO,
    version_added: "0.84.246",
    tagline: "Network monitor + best-effort per-app blocker + world map (macOS).",
    tagline_de: "Netzwerk-Monitor + Best-Effort-Per-App-Blocker + Weltkarte (macOS).",
    synopsis: "snitch [map]",
    description:
      "`snitch` lists apps with live outbound connections and lets you best-effort-block an app (a root pf daemon, one prompt). `snitch map` shows a world map of the servers your machine talks to (offline dotted basemap, ip-api geolocation for public IPs only, live per-app throughput arcs). Honest scope: a real per-app firewall needs a NetworkExtension entitlement this app can't have, so blocking is best-effort, never a hard firewall.",
    arguments: [{ name: "map", required: false, description: "Open the world-map connections view (also `conn`/`show`/`world`).", default: "(app blocker)" }],
    flags: [],
    examples: [
      { input: "snitch", result: "Apps with live connections; toggle a block (first block prompts once for admin)." },
      { input: "snitch map", result: "A world map of the servers you're connected to." },
      { input: "snitch map", result: "Active servers glow with animated packet arcs from 'home'." },
    ],
    tips: ["Private/LAN IPs are never sent out — only public IPs are geolocated (ip-api)."],
    caveats: ["macOS only. Blocking is best-effort (fails open if the daemon dies); it can't scope to a single app the way Little Snitch does."],
    related: ["stats"],
  },
  {
    command: "weather",
    aliases: ["wetter"],
    category: CAT_INFO,
    version_added: "0.97.0",
    tagline: "Weather for your location — current, next 12 h + 5-day forecast, animated.",
    tagline_de: "Wetter für deinen Standort — aktuell, nächste 12 h + 5-Tage-Vorschau, animiert.",
    synopsis: "weather [city]",
    description:
      "Shows the current conditions, the next 12 hours (3-hour slots with temperature, condition and rain probability, labelled in the searched city's local time) and a 5-day forecast in the preview, with a weather-appropriate animation (sun, drifting clouds, falling rain/snow, lightning, mist). With no argument it uses your IP-geolocated location; `weather berlin` overrides it with a city. Powered by OpenWeatherMap — set a free API key in Settings → Weather (or paste it into the connect card the first time). Refreshes every 10 min; press R to refresh now, ↑/↓ scroll, Esc exits.",
    arguments: [
      {
        name: "city",
        required: false,
        description: "A city name to look up instead of your location, e.g. `weather berlin`.",
        default: "(your IP location)",
      },
    ],
    flags: [],
    examples: [
      { input: "weather", result: "Current conditions, next 12 h + 5-day forecast for where you are." },
      { input: "weather berlin", result: "Weather for Berlin." },
      { input: "weather new york", result: "Multi-word city names work too." },
    ],
    tips: [
      "Needs a free OpenWeatherMap API key — add it in Settings → Weather, or paste it into the connect card the panel shows the first time.",
      "Your location is resolved by IP (no GPS/permission); pass a city to override it.",
      "As you type a city (`weather darm`), matching major cities autocomplete — press Tab (or →) to fill it, or Enter to complete + show it.",
    ],
    caveats: ["Requires a network connection and a valid OpenWeather key."],
    related: ["stats", "uptime"],
  },
  {
    command: "calendar",
    aliases: ["cal"],
    category: CAT_PROD,
    version_added: "0.84.234",
    tagline: "Month-view calendar in the preview — which weekday was that date?",
    tagline_de: "Monatskalender im Preview — welcher Wochentag war das Datum?",
    synopsis: "calendar [month year]   ·   cal [month year]",
    description:
      "Shows a month calendar in the preview to research which weekday a date fell on. ←/→ month, ↑/↓ year, PgUp/PgDn month, T/Home today, click a day → a full-date readout + distance from today. The argument jumps the view: `cal märz 1990`, `cal 3.2024`, `cal 2024-03`, or a bare year. Monday-start weeks + ISO week numbers.",
    arguments: [{ name: "month year", required: false, description: "Jump target — DE/EN month names, `3.2024`, `2024-03`, or a bare year.", default: "(current month)" }],
    flags: [],
    examples: [
      { input: "cal märz 1990", result: "March 1990 — see which weekday any date was." },
      { input: "calendar 2024-03", result: "March 2024." },
      { input: "cal 1999", result: "Jumps to the year." },
    ],
    tips: ["Pure client-side date math — no network, works offline."],
    caveats: [],
    related: ["alarm", "timer"],
  },
  {
    command: "track",
    aliases: [],
    category: CAT_PROD,
    version_added: "0.84.77",
    tagline: "Time tracking — start/stop, opt-in, encrypted at rest (macOS).",
    tagline_de: "Zeiterfassung — Start/Stopp, opt-in, verschlüsselt (macOS).",
    synopsis: "track on|off",
    description:
      "Opt-in, offline, encrypted-at-rest time tracking. `track on`/`track off` starts/stops a session (a footer REC LED shows recording/idle); bare `track` (or Ctrl+Shift+T) opens the Timesheet tab. Tracks the frontmost app/window with retroactive idle auto-pause and an optional browser-tab bridge; a per-app breakdown, categories, and CSV/HTML export.",
    arguments: [{ name: "on|off", required: true, description: "Start or stop a tracking session.", default: undefined }],
    flags: [],
    examples: [
      { input: "track on", result: "Starts recording; the footer REC LED lights." },
      { input: "track off", result: "Stops the session." },
      { input: "track", result: "Opens the Timesheet tab (day/week view, export)." },
    ],
    tips: ["Window titles/URLs are AES-encrypted at rest; a denylist strips sensitive titles."],
    caveats: ["macOS is verified end-to-end; Windows/Linux active-window+idle are compile-validated but runtime-unverified."],
    related: ["stats"],
    see_also: "docs/timesheet.md",
  },
  {
    command: "rnd",
    aliases: ["random"],
    category: CAT_FUN,
    version_added: "0.68.0",
    tagline: "Roll a random number — shown in a status toast.",
    tagline_de: "Zufallszahl würfeln — in einem Status-Toast angezeigt.",
    synopsis: "rnd [max]   ·   rnd [min] [max]   ·   random …",
    description: "Rolls a random integer (CSPRNG, rejection-sampled) and shows it in a big status toast. No args = 1–6 (a die); one number = 1–N; two numbers = min–max (swapped if reversed).",
    arguments: [
      { name: "max", required: false, description: "Upper bound when given alone (range 1…max).", default: "6" },
      { name: "min max", required: false, description: "Two numbers = inclusive [min, max].", default: undefined },
    ],
    flags: [],
    examples: [
      { input: "rnd", result: "A die roll, 1–6." },
      { input: "rnd 100", result: "1–100." },
      { input: "rnd 10 20", result: "10–20 inclusive." },
    ],
    tips: ["`random` is a drop-in alias."],
    caveats: [],
    related: ["faker"],
  },
];

/** Look up a doc by command name or alias (case-insensitive). */
export function lookupDoc(name: string): CommandDoc | undefined {
  const n = name.trim().toLowerCase();
  return (
    COMMAND_DOCS.find((d) => d.command === n) ??
    COMMAND_DOCS.find((d) => d.aliases.some((a) => a.toLowerCase() === n))
  );
}

/** Docs grouped by category, in first-seen order — for the `?` index. */
export function groupedIndex(): { category: string; docs: CommandDoc[] }[] {
  const groups: { category: string; docs: CommandDoc[] }[] = [];
  for (const d of COMMAND_DOCS) {
    let g = groups.find((x) => x.category === d.category);
    if (!g) {
      g = { category: d.category, docs: [] };
      groups.push(g);
    }
    g.docs.push(d);
  }
  // Entries within each category read alphabetically by command name.
  for (const g of groups) g.docs.sort((a, b) => a.command.localeCompare(b.command));
  return groups;
}
