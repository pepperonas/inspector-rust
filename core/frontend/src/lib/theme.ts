/**
 * Theme application — the bridge between the persisted preference
 * (SQLite `appearance.theme` setting) and the CSS in `styles.css`.
 *
 * `styles.css` resolves the actual palette from the `data-theme`
 * attribute on `<html>`:
 *
 *   data-theme="dark"    → forced dark
 *   data-theme="light"   → forced light
 *   data-theme="system"  → follow the OS (prefers-color-scheme)
 *
 * This module is the *only* place that writes that attribute.
 */

/** The three valid theme preferences. Mirrors the Rust-side
 *  `normalise_theme` whitelist. */
export type ThemePreference = "light" | "dark" | "system";

/** Coerce an arbitrary string to a valid ThemePreference. Anything
 *  unrecognised falls back to `"system"` — defends against a
 *  hand-edited settings DB or a future value this build predates. */
export function normaliseTheme(value: string | null | undefined): ThemePreference {
  return value === "light" || value === "dark" ? value : "system";
}

/** Apply a theme preference by writing the `data-theme` attribute on
 *  the document root. Pure DOM side-effect; no persistence (the caller
 *  persists separately via the IPC layer). Safe to call repeatedly. */
export function applyTheme(theme: ThemePreference): void {
  document.documentElement.setAttribute("data-theme", theme);
}

/** Human-readable label for a theme preference — used by the Settings
 *  segmented control + any toast copy. */
export function themeLabel(theme: ThemePreference): string {
  switch (theme) {
    case "light":
      return "Light";
    case "dark":
      return "Dark";
    case "system":
      return "System";
  }
}

/** Duration the `theme-switching` class stays on <html> — slightly past
 *  `--duration-base` so the colour transitions complete before the blanket
 *  rule is lifted. */
export const THEME_SWITCH_MS = 200;

/**
 * Interactive theme change (Settings picker): briefly arms a blanket
 * `transition-colors` via `html.theme-switching` (styles.css) so background
 * and border colours glide instead of jumping — then applies the theme.
 * Deliberately NOT part of `applyTheme`: the mount-time apply must stay
 * instant (nothing is painted yet, and a launch-time blanket transition on
 * `*` would tax the first popup open). One-shot per explicit user action.
 */
export function applyThemeAnimated(theme: ThemePreference): void {
  const root = document.documentElement;
  root.classList.add("theme-switching");
  applyTheme(theme);
  window.setTimeout(() => root.classList.remove("theme-switching"), THEME_SWITCH_MS);
}
