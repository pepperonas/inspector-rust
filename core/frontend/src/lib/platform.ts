/**
 * Tiny platform-detection helper for keyboard-shortcut labels.
 *
 * The Rust backend already cfg-gates per OS, but the React layer needs
 * to render the right modifier glyph (`⌘` on macOS, `Ctrl` elsewhere)
 * for shortcut hints. We detect via `navigator.platform` + UA fallback
 * — both Tauri WebViews (WKWebView on macOS, WebView2 on Windows)
 * report sensibly.
 */
export const IS_MAC: boolean =
  typeof navigator !== "undefined" &&
  (/Mac/i.test(navigator.platform || "") ||
    /Mac/i.test(navigator.userAgent || ""));

/** "⌘+Shift+O" on macOS, "Ctrl+Shift+O" elsewhere. Used in tooltips
 *  and shortcut tables — keep the input keys human-readable. */
export function shortcut(...keys: string[]): string {
  return keys
    .map((k) => {
      const lower = k.toLowerCase();
      if (lower === "cmdorctrl" || lower === "mod") {
        return IS_MAC ? "⌘" : "Ctrl";
      }
      if (lower === "shift") return IS_MAC ? "⇧" : "Shift";
      if (lower === "alt" || lower === "option") return IS_MAC ? "⌥" : "Alt";
      if (lower === "ctrl" || lower === "control") return "Ctrl";
      if (lower === "cmd" || lower === "meta") return IS_MAC ? "⌘" : "Win";
      if (lower === "enter") return "⏎";
      if (lower === "esc" || lower === "escape") return "Esc";
      if (lower === "up") return "↑";
      if (lower === "down") return "↓";
      if (lower === "backquote" || lower === "`") return "`";
      return k;
    })
    .join(IS_MAC ? "" : "+");
}
