import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Cloud, Copy, Pencil, Pin, Save, X } from "lucide-react";
import {
  getPendingScreenshotInfo,
  repositionPreviewToCursor,
  screenshotPreviewCopy,
  screenshotPreviewDiscard,
  screenshotPreviewEdit,
  screenshotPreviewSave,
  setScreenshotPinned,
  type PendingScreenshotInfo,
} from "../lib/ipc";

/**
 * CleanShot-X-style floating screenshot preview.
 *
 * Mounted only in the `screenshot-preview` Tauri window (routed in
 * `main.tsx`). Layout matches the user-provided mockup:
 *
 *   ┌──────────────────────────────┐
 *   │ [X]                     [📌] │   ← top corners
 *   │                              │
 *   │         ╭─Copy─╮             │   ← centred pill buttons
 *   │         ╰──────╯             │
 *   │         ╭─Save─╮             │
 *   │         ╰──────╯             │
 *   │                              │
 *   │ [✏]                     [☁]  │   ← bottom corners
 *   └──────────────────────────────┘
 *
 * The screenshot itself is the **background** of the card (object-fit
 * cover, slightly darkened by an overlay so the buttons stay
 * readable). All six controls float on top.
 *
 * Pin (top-right) toggles a backend flag — while pinned, the next
 * screenshot leaves this preview alone (the new PNG still goes to
 * clipboard + history). Cloud (bottom-right) is a deliberate no-op
 * placeholder for a future "upload to host" feature.
 *
 * Cursor-follow + window positioning are owned by Rust
 * (`screenshot_preview::show_preview`); React just polls
 * `reposition_preview_to_cursor` every 200 ms.
 */
export function ScreenshotPreview() {
  const [info, setInfo] = useState<PendingScreenshotInfo | null>(null);
  // Local mirror of the pinned state — kept in sync with backend on
  // mount + on toggle. Backend is the source of truth (it gates the
  // pipeline's "skip preview replacement" branch), but the UI needs
  // a snappy optimistic update.
  const [pinned, setPinned] = useState(false);
  const [copied, setCopied] = useState(false);
  // v0.35.2 — track the 1.4 s "Copied" toast timer so we can clear it on
  // unmount; otherwise the timeout fires its setCopied(false) on a
  // stale component instance and React logs a warning.
  const copyTimerRef = useRef<number | null>(null);

  // Initial load + listen for subsequent screenshots.
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    const refresh = () =>
      getPendingScreenshotInfo()
        .then((i) => {
          setInfo(i);
          setPinned(i?.pinned ?? false);
        })
        .catch(() => undefined);
    void refresh();
    void listen("screenshot-pending", refresh).then((u) => {
      unlisten = u;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  // Cursor-follow polling — same shape as v0.28.7+. Re-positions the
  // preview when the cursor crosses to a different monitor.
  useEffect(() => {
    const id = window.setInterval(() => {
      void repositionPreviewToCursor().catch(() => undefined);
    }, 200);
    return () => window.clearInterval(id);
  }, []);

  const onSave = () => {
    void screenshotPreviewSave().catch(() => undefined);
  };
  // Cleanup the "Copied" toast timer on unmount to avoid a state
  // update on an unmounted component. Mirror pattern used by the
  // expander-blocked banner in App.tsx.
  useEffect(() => () => {
    if (copyTimerRef.current !== null) {
      window.clearTimeout(copyTimerRef.current);
      copyTimerRef.current = null;
    }
  }, []);

  const onCopy = () => {
    void screenshotPreviewCopy()
      .then(() => {
        // 1.4 s visual confirmation — the action is otherwise invisible
        // (it just updates the clipboard).
        setCopied(true);
        if (copyTimerRef.current !== null) {
          window.clearTimeout(copyTimerRef.current);
        }
        copyTimerRef.current = window.setTimeout(() => {
          setCopied(false);
          copyTimerRef.current = null;
        }, 1400);
      })
      .catch(() => undefined);
  };
  const onDiscard = () => {
    void screenshotPreviewDiscard().catch(() => undefined);
  };
  const onEdit = () => {
    void screenshotPreviewEdit().catch(() => undefined);
  };
  const onTogglePin = () => {
    const next = !pinned;
    setPinned(next); // optimistic — backend confirms on next render
    void setScreenshotPinned(next).catch(() => {
      setPinned(!next); // roll back on failure
    });
  };

  const imgSrc = info ? convertFileSrc(info.path) : null;

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-transparent p-1">
      <div
        className="relative flex h-full w-full overflow-hidden rounded-2xl border border-white/10 shadow-2xl"
        style={{
          backgroundImage: imgSrc ? `url(${imgSrc})` : undefined,
          backgroundSize: "cover",
          backgroundPosition: "center",
          backgroundColor: "#1a1a1a",
        }}
      >
        {/* Darkening overlay — keeps the buttons readable on top of any
            background (light/dark/colourful screenshots). */}
        <div className="absolute inset-0 bg-black/40 backdrop-blur-[1px]" />

        {/* Top-left: Close (discard) */}
        <CornerButton
          onClick={onDiscard}
          title="Close (discard the capture)"
          ariaLabel="Close preview"
          className="left-2 top-2"
        >
          <X size={14} />
        </CornerButton>

        {/* Top-right: Pin toggle */}
        <CornerButton
          onClick={onTogglePin}
          title={
            pinned
              ? "Unpin — let the next screenshot replace this preview"
              : "Pin — keep this preview open across the next screenshot"
          }
          ariaLabel={pinned ? "Unpin preview" : "Pin preview"}
          className={"right-2 top-2 " + (pinned ? "ring-1 ring-white/60" : "")}
        >
          <Pin
            size={14}
            className={pinned ? "fill-white text-white" : ""}
          />
        </CornerButton>

        {/* Centre: Copy + Save pills, stacked vertically. */}
        <div className="relative z-10 m-auto flex flex-col gap-2">
          <PillButton onClick={onCopy} title="Copy image to clipboard">
            {copied ? (
              <span className="flex items-center gap-1">
                <Copy size={13} /> Copied
              </span>
            ) : (
              <span className="flex items-center gap-1.5">
                <Copy size={13} /> Copy
              </span>
            )}
          </PillButton>
          <PillButton
            onClick={onSave}
            title="Save to ~/Downloads (with the source-app name) + clipboard + history"
          >
            <span className="flex items-center gap-1.5">
              <Save size={13} /> Save
            </span>
          </PillButton>
        </div>

        {/* Bottom-left: Edit (open annotation editor) */}
        <CornerButton
          onClick={onEdit}
          title="Edit — open the annotation editor (arrows, text, blur, …)"
          ariaLabel="Edit screenshot"
          className="bottom-2 left-2"
        >
          <Pencil size={14} />
        </CornerButton>

        {/* Bottom-right: Cloud upload — placeholder. Tooltip is honest
            about the missing implementation so a click doesn't feel
            broken — it's a clearly-marked future feature. */}
        <CornerButton
          onClick={() => {
            /* TODO v0.33.x — pick a host (VPS / imgur / …) */
          }}
          title="Cloud upload — coming soon"
          ariaLabel="Cloud upload (coming soon)"
          className="bottom-2 right-2 opacity-50 cursor-not-allowed"
        >
          <Cloud size={14} />
        </CornerButton>
      </div>
    </div>
  );
}

/**
 * Small circular button anchored to one of the four corners. Used for
 * X / Pin / Pencil / Cloud. Solid semi-transparent background so it
 * reads on top of any screenshot.
 */
function CornerButton({
  onClick,
  title,
  ariaLabel,
  className = "",
  children,
}: {
  onClick: () => void;
  title: string;
  ariaLabel: string;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-label={ariaLabel}
      className={
        "absolute z-10 flex h-7 w-7 items-center justify-center rounded-full bg-white/85 text-black shadow backdrop-blur transition-colors hover:bg-white " +
        className
      }
    >
      {children}
    </button>
  );
}

/**
 * Centre pill button used for Copy + Save. Larger and more prominent
 * than corner buttons. White-on-dark to match the mockup.
 */
function PillButton({
  onClick,
  title,
  children,
}: {
  onClick: () => void;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className="min-w-[96px] rounded-full bg-white/90 px-4 py-1.5 text-[13px] font-semibold text-black shadow-md backdrop-blur transition-colors hover:bg-white"
    >
      {children}
    </button>
  );
}
