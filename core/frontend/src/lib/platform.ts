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

export const IS_LINUX: boolean =
  typeof navigator !== "undefined" && /Linux/i.test(navigator.userAgent || "");

/**
 * Pretty-print a stored Tauri/global-shortcut spec — the `<Mod>+<Mod>+<Code>`
 * strings produced by `eventToShortcut` / persisted by the backend, e.g.
 * `"Alt+Digit1"`, `"Ctrl+Shift+V"`, `"Meta+KeyB"`. Maps W3C key codes
 * (`Digit1` → `1`, `KeyB` → `B`, `Backquote` → `` ` ``) and renders modifier
 * glyphs on macOS. Returns an em-dash for an empty / unset spec.
 */
export function formatHotkey(spec: string): string {
  if (!spec || !spec.trim()) return "—";
  return spec
    .split("+")
    .map((raw) => {
      const k = raw.trim();
      const lower = k.toLowerCase();
      // Modifiers.
      if (lower === "ctrl" || lower === "control") return IS_MAC ? "⌃" : "Ctrl";
      if (lower === "shift") return IS_MAC ? "⇧" : "Shift";
      if (lower === "alt" || lower === "option") return IS_MAC ? "⌥" : "Alt";
      if (lower === "meta" || lower === "cmd" || lower === "command" || lower === "super")
        return IS_MAC ? "⌘" : "Win";
      // Key codes.
      if (/^digit[0-9]$/.test(lower)) return k.slice(-1);
      if (/^key[a-z]$/.test(lower)) return k.slice(-1).toUpperCase();
      if (/^numpad[0-9]$/.test(lower)) return "Num" + k.slice(-1);
      if (/^f([1-9]|1[0-9]|2[0-4])$/.test(lower)) return k.toUpperCase();
      if (lower === "backquote") return "`";
      if (lower === "minus") return "-";
      if (lower === "equal") return "=";
      if (lower === "space") return "Space";
      if (lower === "arrowup") return "↑";
      if (lower === "arrowdown") return "↓";
      if (lower === "arrowleft") return "←";
      if (lower === "arrowright") return "→";
      return k;
    })
    .join(IS_MAC ? "" : "+");
}

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
