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
import { t, type MessageKey } from "./i18n";

export interface SettingsSection {
  /** Stable DOM anchor id (`settings-<id>`) the panel scrolls to. */
  id: string;
  /** Display label (follows the app language — a getter over the catalogue). */
  readonly label: string;
  /** All names/synonyms the fuzzy matcher considers (lowercase). */
  names: string[];
}

/** A section entry whose label follows the app language (read at use time). */
function sec(id: string, names: string[]): SettingsSection {
  return {
    id,
    names,
    get label() {
      return t(`settings.section.${id}` as MessageKey);
    },
  };
}

function cat(id: string, sections: readonly string[]): SettingsCategory {
  return {
    id,
    sections,
    get label() {
      return t(`settings.category.${id}` as MessageKey);
    },
  };
}

export const SETTINGS_SECTIONS: readonly SettingsSection[] = [
  sec("behavior", ["behavior", "overlay", "click outside", "verhalten", "schließen", "blur"]),
  sec("sounds", ["sounds", "sound", "audio cues", "töne"]),
  sec("popup-hotkey", ["popup", "hotkey", "popup hotkey"]),
  sec("global-shortcuts", ["shortcuts", "global shortcuts", "hotkeys", "tastatur", "keyboard"]),
  sec("expander", ["expander", "text expander", "snippets expander", "abbreviation"]),
  sec("snippets", ["snippets", "snippet", "storage", "speicher", "count"]),
  sec("appearance", ["appearance", "theme", "darstellung", "dark", "light", "size", "crt", "animation", "animations", "animationen", "motion", "bewegung", "popup animation", "animationsdauer"]),
  sec("adb", ["adb", "android", "handy", "smartphone", "adboss", "phone"]),
  sec("clipboard-history", ["history", "clipboard history", "max entries", "limit", "cap", "verlauf"]),
  sec("pagespeed", ["pagespeed", "lighthouse", "api key", "google", "seite", "performance"]),
  sec("repos", ["repos", "repositories", "repository", "klonen", "clone", "git"]),
  sec("device-sync", ["device sync", "geraete-sync", "geräte-sync", "geraetesync", "macs", "icloud", "abgleich"]),
  sec("auto-backup", ["auto-backup", "auto backup", "automatisch", "sicherung", "geplant", "schedule", "google drive", "backup ordner", "backup-ordner", "intervall"]),
  sec("clipboard-privacy", ["privacy", "clipboard privacy", "exclude", "auto-clear", "privatsphäre", "datenschutz"]),
  sec("cleaning", ["cleaning", "clean", "cleaner", "aufräumen"]),
  sec("timesheet", ["timesheet", "tracking", "zeiterfassung", "track"]),
  sec("bruno", ["bruno", "steuer", "netto", "brutto", "tax"]),
  sec("faker", ["faker", "fake data", "testdaten"]),
  sec("figlet", ["figlet", "banner", "ascii"]),
  sec("security", ["security", "sec", "nmap", "pentest"]),
  sec("meme", ["meme", "memes", "gif"]),
  sec("ai", ["ki", "ki-anbieter", "ai", "ai providers", "api key", "api-schlüssel", "claude", "gemini", "chatgpt", "openai", "anthropic", "modell", "model"]),
  sec("weather", ["weather", "wetter", "openweather", "forecast"]),
  sec("timer-alarm", ["timer", "alarm", "wecker"]),
  sec("input-lock", ["input lock", "freeze", "lock chord"]),
  sec("gestures", ["gestures", "gesten", "touchpad", "trackpad", "tip-tap"]),
  sec("window", ["window", "snapping", "palette", "fenster", "snap"]),
  sec("window-palette", ["trigger", "tiling", "kachel", "titelleiste", "ausloeser"]),
  sec("cursor-wrap", ["cursor", "wrap", "wrap-around", "wraparound", "infinity monitor", "infinity", "mauszeiger", "zeiger", "rand", "edge", "pac-man", "pacman", "hot corners"]),
  sec("cloud-sync", ["cue", "sync", "cloud", "cloud-sync", "cue-sync", "token"]),
  sec("backup", ["backup", "restore", "export", "import", "sicherung"]),
  sec("startup", ["startup", "autostart", "login", "keep running", "keepalive"]),
  // Appended (v0.194.0) — new entries go LAST: ties keep registry order, so
  // inserting in the middle could change what an existing query resolves to.
  sec("history-hotkey", ["history hotkey", "clipboard hotkey", "verlauf hotkey", "ctrl+shift+v"]),
  sec("paste", ["paste", "einfügen", "plain text", "formatierung"]),
  sec("capture", ["capture", "ocr", "screenshot", "bildschirmfoto", "texterkennung"]),
  sec("keyboard-shortcuts", ["cheat sheet", "shortcut list", "kurzbefehle übersicht", "tastenübersicht"]),
  sec("auto-expand", ["auto-expansion", "autoexpand", "atext", "automatisch erweitern"]),
  sec("direct-slots", ["direct hotkey", "snippet hotkey", "direkt"]),
  sec("linux-shortcuts", ["linux", "gnome", "desktop shortcuts"]),
  sec("about", ["about", "version", "über", "info", "lizenz", "license"]),
  sec("pulse", ["pulse", "speicherdruck", "swap", "ssd", "tbw", "memory pressure"]),
  sec("spotify", ["spotify", "musik", "music", "verlauf", "history", "scrobble"]),
  sec("github", ["github", "github token", "integrations", "integration", "oauth", "followers", "stars", "sterne"]),
  sec("language", ["language", "sprache", "english", "englisch", "deutsch", "german", "locale"]),
  sec("changelog", ["changelog", "änderungen", "versionshinweise", "release notes", "what's new", "neuigkeiten", "history of changes"]),
  sec("record", ["record","aufnahme","recording","video"]),
];

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
  cat("general", ["behavior", "language", "appearance", "sounds", "startup"]),
  cat("keyboard", ["popup-hotkey", "history-hotkey", "global-shortcuts", "input-lock", "linux-shortcuts", "keyboard-shortcuts"]),
  cat("clipboard", ["clipboard-history", "clipboard-privacy", "paste", "capture"]),
  cat("snippets", ["snippets", "expander", "auto-expand", "direct-slots"]),
  cat("desktop", ["gestures", "window", "window-palette", "cursor-wrap"]),
  cat("commands", ["timer-alarm", "timesheet", "pulse", "cleaning", "bruno", "faker", "figlet", "security", "meme", "adb", "record"]),
  cat("services", ["ai", "weather", "spotify", "pagespeed", "github", "repos"]),
  cat("sync", ["cloud-sync", "device-sync", "auto-backup", "backup"]),
  cat("about", ["about", "changelog"]),
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
