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
    tagline: "Open Google Translate for the text — auto/EN↔DE and DE↔IT/ES/PL pairs.",
    tagline_de: "Google Translate für den Text öffnen — auto/EN↔DE und DE↔IT/ES/PL.",
    synopsis: "tr <text>   ·   tren|trde <text>   ·   trde2it|trit2de|trde2sp|trsp2de|trde2pl|trpl2de <text>",
    description:
      "Opens Google Translate in your browser with the text pre-filled. The keyword picks the language pair: `tr` auto-detects the source and translates to German; `tren` forces English→German, `trde` German→English; the `tr<a>2<b>` forms are fixed pairs between German and Italian/Spanish/Polish. Frontend-only — it builds a translate.google.com URL and opens it via the system browser; nothing is sent from the app itself.",
    arguments: [{ name: "text", required: true, description: "The text to translate (rest of the line).", default: undefined }],
    flags: [],
    examples: [
      { input: "tr Feierabend", result: "Google Translate opens, auto-detects German → English/target." },
      { input: "tren cheerful", result: "English → German (cheerful → fröhlich)." },
      { input: "trde2it Guten Morgen", result: "German → Italian (buongiorno)." },
    ],
    tips: [
      "`tr` auto-detects the source language, so it's the one to reach for when unsure.",
      "The whole rest of the line is the text — quotes aren't needed.",
    ],
    caveats: ["Opens the browser (a network request happens there, in your browser — not from Inspector Rust)."],
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
      "Lists running processes (sorted by memory, excluding our own PID) as a live picker; the argument filters by name/exe substring or, when all-digits, by exact PID. Enter asks a native confirmation, then sends SIGTERM (or SIGKILL with `-9`; TerminateProcess on Windows).",
    arguments: [{ name: "pattern | pid", required: false, description: "Name/exe substring, or an exact numeric PID.", default: "(show all)" }],
    flags: [{ flag: "-9", value_type: undefined, description: "Force kill (SIGKILL) instead of SIGTERM.", default: "SIGTERM" }],
    examples: [
      { input: "kill", result: "The full process list, biggest memory first." },
      { input: "kill slack", result: "Only processes matching 'slack'." },
      { input: "kill -9 1234", result: "Force-kill PID 1234 (after confirming)." },
    ],
    tips: ["Type a memory hog's name to find it fast; the list is sorted by RAM."],
    caveats: ["Destructive — a native confirmation is required before the kill.", "Windows has no signals; `-9` and plain both map to a forced TerminateProcess."],
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
    tagline: "Keep the Mac awake — wakelock on / off (alias caffeine).",
    tagline_de: "Mac wachhalten — wakelock on / off (Alias caffeine).",
    synopsis: "wakelock on|off   ·   caffeine on|off",
    description:
      "Prevents sleep. macOS spawns `caffeinate -disu` (real IOPM assertions); Windows uses SetThreadExecutionState plus a periodic invisible F15 keypress (so the screensaver/lock don't engage); Linux a logind idle+sleep inhibitor. A footer LED shows the state; a status toast confirms.",
    arguments: [{ name: "on|off", required: true, description: "Turn keep-awake on or off (also accepts 1/0/true/false).", default: undefined }],
    flags: [],
    examples: [
      { input: "wakelock on", result: "The Mac stays awake; footer LED lights." },
      { input: "wakelock off", result: "Back to normal sleep behaviour." },
      { input: "caffeine on", result: "Same effect (alias) — branded 'Caffeine' in the toast." },
    ],
    tips: ["`caffeine` is a drop-in alias if that's the muscle memory you have."],
    caveats: ["The old `=1`/`=0` syntax was removed in v0.52.0 — use on/off."],
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
      "Records ~10 s from the microphone, generates a Shazam audio-signature, queries Shazam's public API, and shows the matched track (cover · title · artist · album · year) with Shazam/Spotify/YouTube links. `shazam history` opens past recognitions. Native mic capture (no webview) so playback isn't disturbed.",
    arguments: [{ name: "history", required: false, description: "Open the recognition history instead of listening.", default: "(listen)" }],
    flags: [],
    examples: [
      { input: "shazam", result: "Listens ~10 s, then shows the matched song + links." },
      { input: "shazam history", result: "Your past recognitions with links + per-row delete." },
      { input: "shazam", result: "R re-records, Esc exits." },
    ],
    tips: [
      "R re-records, L opens the LYRICS of the match (in-app via lrclib.net, browser-search fallback), Enter copies “Title – Artist”.",
      "The platform buttons are brand icons now — Shazam (blue), Spotify (green), YouTube (red) — hover for the tooltip.",
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
      "A safety-first disk cleaner: a dry-run scan renders as a checkbox list of directories (size + largest files) grouped by category. Tick what to delete, press Enter twice (arm → confirm) to sweep only the checked categories. Strict allowlist, symlinks never followed, deletion is always file-by-file with re-validation. Levels Safe/Standard/Aggressive + developer targets (stale node_modules/target, JetBrains/Xcode leftovers, Docker/brew/pnpm) in Settings → Cleaning.",
    arguments: [],
    flags: [],
    examples: [
      { input: "clean", result: "Scan → tick categories → Enter twice → space reclaimed." },
      { input: "clean", result: "Space toggles a row; A toggles all; the selected row shows its 3 largest files." },
      { input: "clean", result: "Downloads dupes / old installers are offered but pre-deselected." },
    ],
    tips: ["Configure the level + developer roots + per-category toggles in Settings → Cleaning."],
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
  return groups;
}
