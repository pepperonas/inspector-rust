/**
 * Reliable destructive-action confirmation (v0.84.242).
 *
 * `window.confirm` is NOT trustworthy in the Tauri webview: on this WKWebView
 * it can return a result **without ever showing a dialog** (field report: the
 * `clean` command "deleted directly" — its confirm auto-passed; the TOTP
 * delete had the inverse failure earlier). Every destructive flow must use
 * this helper instead: the dialog plugin's `ask()` renders a real native
 * NSAlert through Rust, independent of the webview.
 *
 * The popup's hide-on-focus-loss is suppressed for the dialog's lifetime
 * (the alert steals focus), and any plugin failure returns `false` —
 * destructive actions fail CLOSED, never "assumed yes".
 */
import { ask } from "@tauri-apps/plugin-dialog";
import { setSuppressHide } from "./ipc";

export async function confirmDialog(message: string, title = "Are you sure?"): Promise<boolean> {
  try {
    await setSuppressHide(true).catch(() => undefined);
    return await ask(message, { title, kind: "warning" });
  } catch (e) {
    console.error("confirmDialog failed (failing closed)", e);
    return false;
  } finally {
    await setSuppressHide(false).catch(() => undefined);
  }
}
