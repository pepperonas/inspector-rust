import { useEffect, useRef, useState } from "react";
import { ExternalLink, Music } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { watchUrl, RICK_LINES } from "../lib/rickroll";
import { prefersReducedMotion } from "../lib/md3-motion";
import rickrollMp4 from "../assets/rickroll.mp4";

/**
 * `rickroll` — plays Rick Astley's "Never Gonna Give You Up" with sound right
 * in the preview column (v0.122.0). **Local clip since v0.128.2:** the YouTube
 * embed died in the Tauri WKWebView ("Error 153: Video player configuration
 * error" — embed/referer restrictions), so the panel plays a bundled, heavily
 * compressed MP4 (480p, ~5 MB) instead — zero network, no YouTube moods. The
 * effect tries unmuted autoplay (the Enter/click that opened the panel is the
 * user gesture); if the webview still refuses, a hint shows and the native
 * controls / "Abspielen" get it going. Browser fallback + lyric marquee stay.
 */
export function RickrollPanel({ focused, onExit }: { focused: boolean; onExit: () => void }) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [blocked, setBlocked] = useState(false);
  const [srcUrl, setSrcUrl] = useState<string | null>(null);
  const reduce = prefersReducedMotion();

  // ⚠️ Play from a BLOB, not the asset URL (v0.128.3 — "super laggy" field
  // report): WKWebView's media loader streams via Range requests, which the
  // embedded-asset custom protocol serves poorly → constant stutter. Fetching
  // the 5 MB once and handing the video an object URL gives the decoder full
  // random access from memory. Revoked on unmount (the media element is torn
  // down with it); a fetch failure falls back to the direct asset URL.
  useEffect(() => {
    let alive = true;
    let url: string | null = null;
    fetch(rickrollMp4)
      .then((r) => r.blob())
      .then((b) => {
        if (!alive) return;
        url = URL.createObjectURL(b.type ? b : new Blob([b], { type: "video/mp4" }));
        setSrcUrl(url);
      })
      .catch(() => {
        if (alive) setSrcUrl(rickrollMp4);
      });
    return () => {
      alive = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, []);

  // Unmuted autoplay attempt once the blob is ready — the opening Enter/click
  // is the user gesture.
  useEffect(() => {
    if (!srcUrl) return;
    const v = videoRef.current;
    if (!v) return;
    v.play().catch(() => setBlocked(true));
  }, [srcUrl]);

  // Show-while-typing (v0.130.0) means the mount attempt can run WITHOUT a
  // user gesture and get blocked — the Enter that hands focus over IS the
  // gesture, so try again then.
  useEffect(() => {
    if (!focused) return;
    const v = videoRef.current;
    if (v && v.paused) {
      v.play()
        .then(() => setBlocked(false))
        .catch(() => setBlocked(true));
    }
  }, [focused]);

  const restart = () => {
    const v = videoRef.current;
    if (!v) return;
    v.currentTime = 0;
    v.play()
      .then(() => setBlocked(false))
      .catch(() => setBlocked(true));
  };

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit]);

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <Music size={15} className="text-[var(--color-accent)]" /> Never Gonna Give You Up
        </div>
        <button
          type="button"
          onClick={() => void openUrl(watchUrl())}
          title="Auf YouTube öffnen"
          className="flex items-center gap-1 rounded-md px-1.5 py-1 text-[11px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
        >
          <ExternalLink size={12} /> Browser
        </button>
      </div>

      {/* The bundled clip, 16:9. */}
      <div
        className="relative overflow-hidden rounded-xl border border-[var(--color-border)] bg-black"
        style={{ aspectRatio: "16 / 9" }}
      >
        {srcUrl ? (
          <video
            ref={videoRef}
            src={srcUrl}
            controls
            playsInline
            className="absolute inset-0 h-full w-full"
          />
        ) : (
          <div className="absolute inset-0 flex items-center justify-center text-[12px] text-[var(--color-muted)]">
            Lade Clip…
          </div>
        )}
      </div>

      {/* Lyric marquee. */}
      <div className="overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] py-1">
        <div
          className={reduce ? "px-2 text-center" : "rickroll-marquee whitespace-nowrap"}
          style={reduce ? undefined : { willChange: "transform" }}
        >
          {reduce ? (
            <span className="text-[12px] text-[var(--color-accent)]">{RICK_LINES[0]} 🎵</span>
          ) : (
            <span className="inline-block text-[12px] text-[var(--color-accent)]">
              {[...RICK_LINES, ...RICK_LINES].map((l, i) => (
                <span key={i} className="mx-4">
                  🎶 {l}
                </span>
              ))}
            </span>
          )}
        </div>
      </div>

      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={restart}
          className="rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)]"
        >
          {blocked ? "▶ Abspielen" : "↻ Nochmal"}
        </button>
        <button
          type="button"
          onClick={() => void openUrl(watchUrl())}
          className="rounded-md border border-[var(--color-border)] px-3 py-1.5 text-[12px] hover:text-[var(--color-accent)]"
        >
          Im Browser abspielen
        </button>
      </div>
      {blocked && (
        <p className="text-[11px] text-amber-500">
          Autoplay wurde geblockt — „▶ Abspielen“ oder die Player-Steuerung starten den Clip.
        </p>
      )}
      {focused && (
        <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">Esc schließen</p>
      )}
    </div>
  );
}
