import { useEffect, useState } from "react";
import { Keyboard } from "lucide-react";
import { useT } from "../lib/i18n";

/**
 * Map WebKit-on-macOS quirks back to the layout-stable W3C code.
 *
 * On a German ISO MacBook keyboard, the **top-left** key (under Esc,
 * labeled `^` `°`) is in the same physical position — and registers with
 * the same Carbon virtual keycode (`kVK_ANSI_Grave` = 0x32) — as the US
 * `` ` ` `` key. WebKit nonetheless reports `event.code = "IntlBackslash"`
 * for it on DE-ISO macOS. That value is technically a valid W3C code,
 * but the OS-level global-hotkey crate has no `IntlBackslash` →
 * macOS-keycode mapping, so registration fails. Mapping it to `Backquote`
 * lines the captured code up with what the OS will actually see when the
 * hotkey fires.
 */
function normalizeCode(code: string): string {
  if (code === "IntlBackslash") return "Backquote";
  return code;
}

/**
 * Convert a `KeyboardEvent` into the string format expected by the
 * shortcut parser in `core/rust-lib/src/hotkey.rs`:
 *
 *   `<Modifier>+<Modifier>+<Code>`
 *
 * - Modifiers from `event.<modifier>Key` flags: `Ctrl`, `Shift`, `Alt`,
 *   `Meta` (becomes Cmd/Super on the Rust side).
 * - The non-modifier key uses `event.code` (W3C `KeyboardEvent.code`
 *   spec — `Backquote`, `KeyA`, `Digit1`, `F5`, `ArrowLeft`, …) after
 *   running it through `normalizeCode` to handle WebKit-on-macOS quirks.
 *
 * Returns `null` if the only key pressed was a bare modifier (Ctrl alone,
 * Shift alone, …) — those aren't valid hotkeys on their own.
 */
export function eventToShortcut(e: KeyboardEvent): string | null {
  const modifiers: string[] = [];
  if (e.ctrlKey) modifiers.push("Ctrl");
  if (e.shiftKey) modifiers.push("Shift");
  if (e.altKey) modifiers.push("Alt");
  if (e.metaKey) modifiers.push("Meta");

  const code = e.code;
  if (
    code === "" ||
    code === "ControlLeft" ||
    code === "ControlRight" ||
    code === "ShiftLeft" ||
    code === "ShiftRight" ||
    code === "AltLeft" ||
    code === "AltRight" ||
    code === "MetaLeft" ||
    code === "MetaRight"
  ) {
    return null;
  }
  return [...modifiers, normalizeCode(code)].join("+");
}

interface Props {
  value: string;
  onChange: (next: string) => void;
  /** When set, replace the displayed string with this — useful while a
   *  network round-trip is pending. */
  pending?: boolean;
  disabled?: boolean;
}

/**
 * Click-to-record hotkey field.
 *
 * Implementation note: Safari / WebKit (the Tauri renderer on macOS) does
 * **not** focus a `<button>` element on click. So an `onKeyDown` handler
 * on the button never fires during capture. The reliable fix is to listen
 * on `window` while capturing, then tear the listener down once a valid
 * shortcut is recorded.
 */
export function HotkeyCapture({ value, onChange, pending, disabled }: Props) {
  const t = useT();
  const [capturing, setCapturing] = useState(false);

  useEffect(() => {
    if (!capturing) return;

    const onKey = (e: KeyboardEvent) => {
      // Block every key while we're recording — including Esc, which the
      // global keyboard-nav hook would otherwise consume to hide the popup.
      e.preventDefault();
      e.stopPropagation();

      if (e.key === "Escape") {
        setCapturing(false);
        return;
      }
      if (e.key === "Backspace") {
        onChange("");
        setCapturing(false);
        return;
      }
      const sc = eventToShortcut(e);
      if (sc) {
        onChange(sc);
        setCapturing(false);
      }
    };

    // Capture phase so we win over the global useKeyboardNav listener (and
    // anything else that listens at window level for navigation keys).
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capturing, onChange]);

  const label = capturing
    ? t("hotkey.capturing")
    : pending
      ? t("common.saving")
      : value || t("hotkey.none");

  return (
    <button
      type="button"
      disabled={disabled}
      onClick={() => setCapturing((c) => !c)}
      className={
        "flex items-center gap-2 rounded border px-2 py-1 text-left font-[var(--font-mono)] text-[12px] outline-none transition-colors disabled:opacity-50 " +
        (capturing
          ? "border-[var(--color-accent)] bg-[var(--color-accent)]/10 text-[var(--color-accent)]"
          : "border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-fg)] hover:border-[var(--color-accent)]/60")
      }
      title={t("hotkey.capture_title")}
    >
      <Keyboard size={12} />
      <span className="truncate">{label}</span>
    </button>
  );
}
