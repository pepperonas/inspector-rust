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
  { id: "appearance", label: "Appearance", names: ["appearance", "theme", "darstellung", "dark", "light", "size", "crt", "animation", "popup animation", "animationsdauer"] },
  { id: "adb", label: "Android (adb)", names: ["adb", "android", "handy", "smartphone", "adboss", "phone"] },
  { id: "clipboard-history", label: "Clipboard history", names: ["history", "clipboard history", "max entries", "limit", "cap", "verlauf"] },
  { id: "pagespeed", label: "PageSpeed", names: ["pagespeed", "lighthouse", "api key", "google", "seite", "performance"] },
  { id: "device-sync", label: "Device sync", names: ["device sync", "geraete-sync", "geräte-sync", "geraetesync", "macs", "icloud", "abgleich"] },
  { id: "clipboard-privacy", label: "Clipboard privacy", names: ["privacy", "clipboard privacy", "exclude", "auto-clear"] },
  { id: "cleaning", label: "Cleaning", names: ["cleaning", "clean", "cleaner", "aufräumen"] },
  { id: "timesheet", label: "Timesheet", names: ["timesheet", "tracking", "zeiterfassung", "track"] },
  { id: "bruno", label: "Bruno (Brutto/Netto)", names: ["bruno", "steuer", "netto", "brutto", "tax"] },
  { id: "faker", label: "Faker", names: ["faker", "fake data", "testdaten"] },
  { id: "figlet", label: "Figlet", names: ["figlet", "banner", "ascii"] },
  { id: "security", label: "Security builder", names: ["security", "sec", "nmap", "pentest"] },
  { id: "meme", label: "Meme library", names: ["meme", "memes", "gif"] },
  { id: "weather", label: "Weather", names: ["weather", "wetter", "openweather", "forecast"] },
  { id: "timer-alarm", label: "Timer alarm", names: ["timer", "alarm", "wecker"] },
  { id: "input-lock", label: "Input lock", names: ["input lock", "freeze", "lock chord"] },
  { id: "gestures", label: "Touchpad gestures", names: ["gestures", "gesten", "touchpad", "trackpad", "tip-tap"] },
  { id: "window", label: "Window snapping & palette", names: ["window", "snapping", "palette", "fenster", "snap"] },
  { id: "cloud-sync", label: "Cloud-Sync (cue)", names: ["cue", "sync", "cloud", "cloud-sync", "cue-sync", "token"] },
  { id: "backup", label: "Backup & restore", names: ["backup", "restore", "export", "import", "sicherung"] },
  { id: "startup", label: "Startup", names: ["startup", "autostart", "login", "keep running", "keepalive"] },
] as const;

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
