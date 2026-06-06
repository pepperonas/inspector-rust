import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { X } from "lucide-react";
import { closePin, getPinImage } from "../lib/ipc";

/**
 * A pinned screenshot — a frameless, always-on-top floating window that
 * holds a captured image on screen until the user closes it (CleanShot-X
 * "Pin to screen"). Multiple pins coexist as separate windows, each routed
 * here by a `screenshot-pin-<seq>` label (see `main.tsx`).
 *
 * The whole surface is a drag region (move the pin anywhere); a small close
 * button in the corner removes it and deletes its cached PNG. The window is
 * resizable from its edges (set on the Rust side).
 */
export function ScreenshotPin() {
  const label = getCurrentWebviewWindow().label;
  const [src, setSrc] = useState<string | null>(null);
  const [hover, setHover] = useState(false);

  useEffect(() => {
    let alive = true;
    getPinImage(label)
      .then((path) => {
        if (alive && path) setSrc(convertFileSrc(path));
      })
      .catch(() => {
        /* leave blank; the window can still be closed */
      });
    return () => {
      alive = false;
    };
  }, [label]);

  return (
    <div
      className="relative h-screen w-screen overflow-hidden rounded-lg bg-transparent"
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
    >
      {/* Drag region covers the whole image. */}
      <div data-tauri-drag-region className="h-full w-full">
        {src ? (
          <img
            src={src}
            alt="Pinned screenshot"
            draggable={false}
            data-tauri-drag-region
            className="pointer-events-none h-full w-full object-contain"
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center text-[12px] text-white/60">
            Loading…
          </div>
        )}
      </div>

      <button
        onClick={() => void closePin(label)}
        title="Close pin"
        className={
          "absolute right-1.5 top-1.5 flex h-6 w-6 items-center justify-center rounded-full bg-black/60 text-white transition-opacity hover:bg-black/80 " +
          (hover ? "opacity-100" : "opacity-0")
        }
      >
        <X size={14} />
      </button>
    </div>
  );
}
