import { useEffect, useMemo, useState } from "react";
import { ExternalLink, Music } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { embedUrl, watchUrl, RICK_LINES } from "../lib/rickroll";
import { prefersReducedMotion } from "../lib/md3-motion";

/**
 * `rickroll` — plays Rick Astley's "Never Gonna Give You Up" with sound right
 * in the preview column (v0.122.0). The YouTube embed autoplays (the Enter/
 * click that opened the panel is the required user gesture; CSP is disabled so
 * the iframe loads). A prominent "open in browser" fallback covers a webview
 * that refuses autoplay-with-sound. A scrolling lyric marquee + gradient wash
 * make it a proper gag.
 */
export function RickrollPanel({ focused, onExit }: { focused: boolean; onExit: () => void }) {
  // Remount the iframe to (re)start playback; bumping the key reloads it.
  const [playKey, setPlayKey] = useState(0);
  const src = useMemo(() => embedUrl({ autoplay: true }), []);
  const reduce = prefersReducedMotion();

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

      {/* The video, 16:9, with a playful animated glow frame. */}
      <div
        className="relative overflow-hidden rounded-xl border border-[var(--color-border)]"
        style={{ aspectRatio: "16 / 9" }}
      >
        <iframe
          key={playKey}
          src={src}
          title="Rick Astley — Never Gonna Give You Up"
          className="absolute inset-0 h-full w-full"
          style={{ border: 0 }}
          allow="autoplay; encrypted-media; fullscreen"
          allowFullScreen
        />
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
          onClick={() => setPlayKey((k) => k + 1)}
          className="rounded-md bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)]"
        >
          ↻ Nochmal
        </button>
        <button
          type="button"
          onClick={() => void openUrl(watchUrl())}
          className="rounded-md border border-[var(--color-border)] px-3 py-1.5 text-[12px] hover:text-[var(--color-accent)]"
        >
          Im Browser abspielen
        </button>
      </div>
      <p className="text-[11px] text-[var(--color-muted)]">
        Kein Ton? Manche Webviews blocken Autoplay mit Ton — dann „Im Browser abspielen“.
      </p>
      {focused && (
        <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">Esc schließen</p>
      )}
    </div>
  );
}
