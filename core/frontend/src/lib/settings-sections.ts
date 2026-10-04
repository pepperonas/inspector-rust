/**
 * `settings [section]` — the deep-link registry for the Settings tab.
 *
 * Each entry maps a stable section id (the DOM anchor the panel scrolls to)
 * to the human names it can be found under, German AND English, so
 * `settings cue`, `settings sync`, `settings hotkeys` and `settings tastatur`
 * all resolve. Matching is exact > prefix > first-char-anchored subsequence
 * (the shared command `fuzzyScore`), best score wins. Pure + unit-tested.
 */
import { fuzzyScore } from "./commands";

export interface SettingsSection {
  /** Stable DOM anchor id (`settings-<id>`) the panel scrolls to. */
  id: string;
  /** Display label for the command row. */
  label: string;
  /** All names/synonyms the fuzzy matcher considers (lowercase). */
  names: string[];
}

export const SETTINGS_SECTIONS: readonly SettingsSection[] = [
  { id: "behavior", label: "Popup behavior", names: ["behavior", "overlay", "click outside", "verhalten", "schließen", "blur"] },
  { id: "sounds", label: "Sounds", names: ["sounds", "sound", "audio cues", "töne"] },
  { id: "popup-hotkey", label: "Popup hotkey", names: ["popup", "hotkey", "popup hotkey"] },
  { id: "global-shortcuts", label: "Global shortcuts", names: ["shortcuts", "global shortcuts", "hotkeys", "tastatur", "keyboard"] },
  { id: "expander", label: "Text expander", names: ["expander", "text expander", "snippets expander", "abbreviation"] },
  { id: "snippets", label: "Snippets", names: ["snippets", "snippet", "storage", "speicher", "count"] },
  { id: "appearance", label: "Appearance", names: ["appearance", "theme", "darstellung", "dark", "light", "size", "crt", "animation", "animations", "animationen", "motion", "bewegung", "popup animation", "animationsdauer"] },
  { id: "adb", label: "Android (adb)", names: ["adb", "android", "handy", "smartphone", "adboss", "phone"] },
  { id: "clipboard-history", label: "Clipboard history", names: ["history", "clipboard history", "max entries", "limit", "cap", "verlauf"] },
  { id: "pagespeed", label: "PageSpeed", names: ["pagespeed", "lighthouse", "api key", "google", "seite", "performance"] },
  { id: "repos", label: "Repositories", names: ["repos", "repositories", "repository", "klonen", "clone", "github", "git", "github token"] },
  { id: "device-sync", label: "Device sync", names: ["device sync", "geraete-sync", "geräte-sync", "geraetesync", "macs", "icloud", "abgleich"] },
  { id: "auto-backup", label: "Auto-backup", names: ["auto-backup", "auto backup", "automatisch", "sicherung", "geplant", "schedule", "google drive", "backup ordner", "backup-ordner", "intervall"] },
  { id: "clipboard-privacy", label: "Clipboard privacy", names: ["privacy", "clipboard privacy", "exclude", "auto-clear", "privatsphäre", "datenschutz"] },
  { id: "cleaning", label: "Cleaning", names: ["cleaning", "clean", "cleaner", "aufräumen"] },
  { id: "timesheet", label: "Timesheet", names: ["timesheet", "tracking", "zeiterfassung", "track"] },
  { id: "bruno", label: "Bruno (Brutto/Netto)", names: ["bruno", "steuer", "netto", "brutto", "tax"] },
  { id: "faker", label: "Faker", names: ["faker", "fake data", "testdaten"] },
  { id: "figlet", label: "Figlet", names: ["figlet", "banner", "ascii"] },
  { id: "security", label: "Security builder", names: ["security", "sec", "nmap", "pentest"] },
  { id: "meme", label: "Meme library", names: ["meme", "memes", "gif"] },
  { id: "ai", label: "KI-Anbieter", names: ["ki", "ki-anbieter", "ai", "ai providers", "api key", "api-schlüssel", "claude", "gemini", "chatgpt", "openai", "anthropic", "modell", "model"] },
  { id: "weather", label: "Weather", names: ["weather", "wetter", "openweather", "forecast"] },
  { id: "timer-alarm", label: "Timer alarm", names: ["timer", "alarm", "wecker"] },
  { id: "input-lock", label: "Input lock", names: ["input lock", "freeze", "lock chord"] },
  { id: "gestures", label: "Touchpad gestures", names: ["gestures", "gesten", "touchpad", "trackpad", "tip-tap"] },
  { id: "window", label: "Window snapping & palette", names: ["window", "snapping", "palette", "fenster", "snap"] },
  { id: "window-palette", label: "Window palette trigger", names: ["trigger", "tiling", "kachel", "titelleiste", "ausloeser"] },
  { id: "cursor-wrap", label: "Cursor wrap-around", names: ["cursor", "wrap", "wrap-around", "wraparound", "infinity monitor", "infinity", "mauszeiger", "zeiger", "rand", "edge", "pac-man", "pacman", "hot corners"] },
  { id: "cloud-sync", label: "Cloud-Sync (cue)", names: ["cue", "sync", "cloud", "cloud-sync", "cue-sync", "token"] },
  { id: "backup", label: "Backup & restore", names: ["backup", "restore", "export", "import", "sicherung"] },
  { id: "startup", label: "Startup", names: ["startup", "autostart", "login", "keep running", "keepalive"] },
  // Appended (v0.194.0) — new entries go LAST: ties keep registry order, so
  // inserting in the middle could change what an existing query resolves to.
  { id: "history-hotkey", label: "Clipboard-history hotkey", names: ["history hotkey", "clipboard hotkey", "verlauf hotkey", "ctrl+shift+v"] },
  { id: "paste", label: "Paste", names: ["paste", "einfügen", "plain text", "formatierung"] },
  { id: "capture", label: "Capture (OCR / screenshot)", names: ["capture", "ocr", "screenshot", "bildschirmfoto", "texterkennung"] },
  { id: "keyboard-shortcuts", label: "Keyboard shortcuts overview", names: ["cheat sheet", "shortcut list", "kurzbefehle übersicht", "tastenübersicht"] },
  { id: "auto-expand", label: "Auto-Expansion", names: ["auto-expansion", "autoexpand", "atext", "automatisch erweitern"] },
  { id: "direct-slots", label: "Direct hotkey → snippet", names: ["direct hotkey", "snippet hotkey", "direkt"] },
  { id: "linux-shortcuts", label: "Linux desktop shortcuts", names: ["linux", "gnome", "desktop shortcuts"] },
  { id: "about", label: "About", names: ["about", "version", "über", "info", "lizenz", "license"] },
] as const;

export interface SettingsCategory {
  /** DOM anchor `settings-cat-<id>`. */
  id: string;
  label: string;
  /** Section ids in display order. */
  sections: readonly string[];
}

/**
 * The order of the Settings tab: categories top to bottom, sections inside.
 * The panel renders from this list, so this is the ONE place the order lives
 * (a test pins that every registry id appears exactly once). Most-used and
 * most basic first; credentials, sync and backup towards the end; About last.
 */
export const SETTINGS_CATEGORIES: readonly SettingsCategory[] = [
  { id: "general", label: "Allgemein", sections: ["behavior", "appearance", "sounds", "startup"] },
  {
    id: "keyboard",
    label: "Tastatur & Kurzbefehle",
    sections: ["popup-hotkey", "history-hotkey", "global-shortcuts", "input-lock", "linux-shortcuts", "keyboard-shortcuts"],
  },
  {
    id: "clipboard",
    label: "Zwischenablage & Aufnahme",
    sections: ["clipboard-history", "clipboard-privacy", "paste", "capture"],
  },
  { id: "snippets", label: "Snippets & Textersetzung", sections: ["snippets", "expander", "auto-expand", "direct-slots"] },
  { id: "desktop", label: "Trackpad & Fenster", sections: ["gestures", "window", "window-palette", "cursor-wrap"] },
  {
    id: "commands",
    label: "Befehle",
    sections: ["timer-alarm", "timesheet", "cleaning", "bruno", "faker", "figlet", "security", "meme", "adb"],
  },
  { id: "services", label: "Dienste & Zugänge", sections: ["ai", "weather", "pagespeed", "repos"] },
  { id: "sync", label: "Sync & Sicherung", sections: ["cloud-sync", "device-sync", "auto-backup", "backup"] },
  { id: "about", label: "Info", sections: ["about"] },
];

/** The category a section is filed under (for the search row's hint). */
export function categoryOf(sectionId: string): SettingsCategory | null {
  return SETTINGS_CATEGORIES.find((c) => c.sections.includes(sectionId)) ?? null;
}

/**
 * Resolve the command argument to a section, or `null` (open Settings at the
 * top). Exact name > prefix > fuzzy subsequence across ALL names; the best
 * (lowest) score wins, ties break by registry order.
 */
export function matchSettingsSection(arg: string): SettingsSection | null {
  const q = arg.trim().toLowerCase();
  if (!q) return null;
  let best: { section: SettingsSection; score: number } | null = null;
  for (const section of SETTINGS_SECTIONS) {
    for (const name of section.names) {
      const s = fuzzyScore(name, q);
      if (s === null) continue;
      if (!best || s < best.score) best = { section, score: s };
    }
  }
  return best?.section ?? null;
}

/** Minimum typed length before sections surface in the MAIN search list. */
export const SETTINGS_SUGGEST_MIN_CHARS = 3;

/**
 * Settings suggestions for the MAIN search list (the macOS/Android
 * settings-search pattern, v0.164.0) — deliberately STRICTER than
 * `matchSettingsSection` (which serves the explicit `settings <arg>`
 * command): only exact / name-prefix / word-start hits, NEVER the fuzzy
 * subsequence. The clipboard search is the primary result set; a
 * subsequence match would push settings rows into ordinary clip queries
 * as noise. Ranking: exact (0) < name prefix (1) < word start (2); each
 * section counts once with its best score; ties keep registry order
 * (Array.sort is stable). The section LABEL joins the haystack so every
 * visible name is findable even where the synonym list is thin.
 */
export function suggestSettingsSections(query: string, limit = 2): SettingsSection[] {
  const q = query.trim().toLowerCase();
  if (q.length < SETTINGS_SUGGEST_MIN_CHARS) return [];
  const scored: { section: SettingsSection; score: number }[] = [];
  for (const section of SETTINGS_SECTIONS) {
    let best: number | null = null;
    for (const name of [...section.names, section.label.toLowerCase()]) {
      const s =
        name === q ? 0
        : name.startsWith(q) ? 1
        : name.split(/[\s-]+/).some((w) => w.startsWith(q)) ? 2
        : null;
      if (s !== null && (best === null || s < best)) best = s;
    }
    if (best !== null) scored.push({ section, score: best });
  }
  scored.sort((a, b) => a.score - b.score);
  return scored.slice(0, limit).map((e) => e.section);
}
