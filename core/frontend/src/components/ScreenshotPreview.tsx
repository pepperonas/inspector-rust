import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Check, Pencil, X } from "lucide-react";
import {
  getPendingScreenshotPath,
  repositionPreviewToCursor,
  screenshotPreviewDiscard,
  screenshotPreviewEdit,
  screenshotPreviewSave,
} from "../lib/ipc";

/**
 * CleanShot-X-style floating screenshot preview window.
 *
 * Mounted only in the `screenshot-preview` Tauri window (routed in
 * `main.tsx`). Shows the captured PNG thumbnail + three actions
 * (Save / Discard / Edit) and a top-right **X** that closes the
 * preview without saving (same as Discard — the screenshot is already
 * on the clipboard from the immediate-write step, so closing just
 * cleans up the temp file + the floating window). No auto-hide; the
 * preview stays put until the user picks an action.
 *
 * The window's position + size are owned by the Rust side
 * (`screenshot_preview::show_preview`). React only handles content +
 * actions. The pending file path comes from
 * `get_pending_screenshot_path` on mount, refreshed whenever the
 * `screenshot-pending` event fires (i.e. the user took another shot
 * while this preview is already up).
 */

export function ScreenshotPreview() {
  const [path, setPath] = useState<string | null>(null);

  // Initial load + subscribe to "another shot taken" events.
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    const refresh = () =>
      getPendingScreenshotPath()
        .then((p) => setPath(p))
        .catch(() => undefined);
    void refresh();
    void listen("screenshot-pending", refresh).then((u) => {
      unlisten = u;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  // Cursor-follow: every 200 ms, ask the backend to re-position the
  // preview window if the cursor has crossed to a different monitor.
  // Backend is idempotent and cheap when the cursor stays put — only
  // monitor changes trigger an actual set_position. Driving this from
  // React (rather than a Rust std::thread) goes through Tauri's IPC
  // layer which marshals set_position onto the main thread cleanly.
  useEffect(() => {
    const id = window.setInterval(() => {
      void repositionPreviewToCursor().catch(() => undefined);
    }, 200);
    return () => window.clearInterval(id);
  }, []);

  const onSave = () => {
    screenshotPreviewSave().catch(() => undefined);
  };
  const onDiscard = () => {
    screenshotPreviewDiscard().catch(() => undefined);
  };
  const onEdit = () => {
    screenshotPreviewEdit().catch(() => undefined);
  };

  // `convertFileSrc` turns an absolute filesystem path into a
  // `tauri://localhost/...`-style URL the webview can <img src>.
  const imgSrc = path ? convertFileSrc(path) : null;

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent p-2">
      <div className="relative flex h-full w-full flex-col gap-2 rounded-xl border border-[var(--color-border)] bg-[var(--color-bg)]/95 p-2 shadow-2xl backdrop-blur-md">
        {/* Top-right close — discards the capture (the screenshot is
            already on the clipboard from the immediate-write step, so
            X just cleans up the temp file + the floating window). */}
        <button
          type="button"
          onClick={onDiscard}
          title="Close (discard the capture)"
          aria-label="Close preview"
          className="absolute right-1.5 top-1.5 z-10 rounded-md bg-[var(--color-bg)]/80 p-1 text-[var(--color-muted)] backdrop-blur transition-colors hover:bg-red-500/10 hover:text-red-500"
        >
          <X size={13} />
        </button>
        {/* Thumbnail */}
        <div className="flex min-h-0 flex-1 items-center justify-center overflow-hidden rounded-lg bg-[var(--color-surface)]">
          {imgSrc ? (
            <img
              src={imgSrc}
              alt="screenshot preview"
              className="max-h-full max-w-full object-contain"
              draggable={false}
            />
          ) : (
            <span className="text-[11px] text-[var(--color-muted)]">No capture pending</span>
          )}
        </div>
        {/* Action row */}
        <div className="flex shrink-0 items-center justify-between gap-1">
          <button
            onClick={onDiscard}
            title="Discard — delete the capture"
            className="flex flex-1 items-center justify-center gap-1 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[11px] text-[var(--color-muted)] hover:border-red-500/40 hover:bg-red-500/10 hover:text-red-500"
          >
            <X size={13} /> Discard
          </button>
          <button
            onClick={onEdit}
            title="Edit — open in the system default image viewer"
            className="flex flex-1 items-center justify-center gap-1 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[11px] hover:border-[var(--color-accent)]/60 hover:bg-[var(--color-accent)]/10 hover:text-[var(--color-accent)]"
          >
            <Pencil size={13} /> Edit
          </button>
          <button
            onClick={onSave}
            title="Save — copy to clipboard + write to ~/Downloads + add to history"
            className="flex flex-1 items-center justify-center gap-1 rounded-lg bg-[var(--color-accent)] px-2 py-1.5 text-[11px] font-medium text-[var(--color-accent-fg)] hover:opacity-90"
          >
            <Check size={13} /> Save
          </button>
        </div>
      </div>
    </div>
  );
}
