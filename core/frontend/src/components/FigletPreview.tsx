/**
 * Big banner preview for the `figlet` command — renders in the right preview
 * column when a `figlet-font` row is selected. Shows the SELECTED font's full
 * banner of the user's text in monospace (the copy payload), plus option chips
 * (align / width / trim / comment / box) that update without re-typing, and a
 * hint when the font can't render some characters.
 *
 * Rendering is Rust-side (the font engine); this debounces + caches per
 * (text, font, opts) so re-selecting or re-rendering never flickers.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { Type, AlignLeft, AlignCenter, AlignRight, Square, AlertTriangle } from "lucide-react";
import { figletRender } from "../lib/ipc";
import type { FigletBanner, FigletComment, FigletOpts } from "../lib/figlet";

const COMMENT_LABELS: Record<FigletComment, string> = {
  none: "plain",
  slashes: "//",
  hash: "#",
  block: "/* */",
  html: "<!--",
};

export function FigletPreview({
  text,
  font,
  category,
  opts,
  onOptsChange,
}: {
  text: string;
  font: string;
  category?: string;
  opts: FigletOpts;
  onOptsChange: (patch: Partial<FigletOpts>) => void;
}) {
  const [banner, setBanner] = useState<FigletBanner | null>(null);
  const cache = useRef<Map<string, FigletBanner>>(new Map());

  const key = useMemo(() => JSON.stringify({ text, font, opts }), [text, font, opts]);

  useEffect(() => {
    if (!text.trim()) {
      setBanner(null);
      return;
    }
    const cached = cache.current.get(key);
    if (cached) {
      setBanner(cached);
      return;
    }
    let alive = true;
    const t = window.setTimeout(() => {
      figletRender(text, font, opts)
        .then((b) => {
          if (!alive) return;
          cache.current.set(key, b);
          setBanner(b);
        })
        .catch(() => {
          if (alive) setBanner(null);
        });
    }, 120);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [key, text, font, opts]);

  const chip = (active: boolean) =>
    "rounded px-2 py-0.5 text-[11px] font-medium transition-colors " +
    (active
      ? "bg-rose-600 text-white"
      : "bg-[var(--color-surface)] text-[var(--color-muted)] hover:text-rose-500");

  return (
    <div className="flex h-full flex-col p-3 text-sm">
      {/* Header: font name + category */}
      <div className="mb-2 flex items-center gap-2">
        <Type size={16} className="text-rose-500" />
        <span className="font-semibold text-[var(--color-fg)]">{font}</span>
        {category && <span className="text-[11px] text-[var(--color-muted)]">· {category}</span>}
        <span
          className="ml-auto text-[11px] text-[var(--color-muted)]"
          title="Enter copies the banner as text · Shift+Enter copies it as a cropped PNG image · Cmd/Ctrl+Shift+Enter saves the PNG to Downloads"
        >
          Enter copies · ⇧⏎ PNG · ⌘⇧⏎ save
        </span>
      </div>

      {/* Option chips */}
      <div className="mb-2 flex flex-wrap items-center gap-1">
        <button className={chip(opts.align === "left")} title="Align left" onClick={() => onOptsChange({ align: "left" })}>
          <AlignLeft size={13} />
        </button>
        <button className={chip(opts.align === "center")} title="Center" onClick={() => onOptsChange({ align: "center" })}>
          <AlignCenter size={13} />
        </button>
        <button className={chip(opts.align === "right")} title="Align right" onClick={() => onOptsChange({ align: "right" })}>
          <AlignRight size={13} />
        </button>
        <button className={chip(opts.boxed)} title="Box border" onClick={() => onOptsChange({ boxed: !opts.boxed })}>
          <Square size={13} />
        </button>
        <button className={chip(opts.trim)} title="Trim trailing whitespace" onClick={() => onOptsChange({ trim: !opts.trim })}>
          trim
        </button>
        {/* Comment style cycles through the options */}
        <button
          className={chip(opts.comment !== "none")}
          title="Comment wrap"
          onClick={() => {
            const order: FigletComment[] = ["none", "slashes", "hash", "block", "html"];
            const next = order[(order.indexOf(opts.comment) + 1) % order.length];
            onOptsChange({ comment: next });
          }}
        >
          {COMMENT_LABELS[opts.comment]}
        </button>
        <label className="ml-1 flex items-center gap-1 text-[11px] text-[var(--color-muted)]">
          w
          <input
            type="number"
            min={0}
            max={400}
            value={opts.width}
            onChange={(e) => {
              const n = parseInt(e.target.value, 10);
              onOptsChange({ width: Number.isFinite(n) ? Math.max(0, Math.min(400, n)) : 0 });
            }}
            className="w-14 rounded bg-[var(--color-surface)] px-1 py-0.5 text-[11px] text-[var(--color-fg)]"
          />
        </label>
      </div>

      {/* Unsupported-char hint — never a silent loss */}
      {banner && banner.unsupported.length > 0 && (
        <p className="mb-2 flex items-center gap-1 rounded bg-amber-500/10 px-2 py-1 text-[11px] text-amber-600 dark:text-amber-400">
          <AlertTriangle size={12} />
          {banner.unsupported.length} character{banner.unsupported.length === 1 ? "" : "s"} not in this
          font: <span className="font-mono">{banner.unsupported.join(" ")}</span>
        </p>
      )}

      {/* The banner — monospace, pre, horizontal scroll (never wrap the art) */}
      <div className="min-h-0 flex-1 overflow-auto rounded bg-[var(--color-surface)] p-2">
        {text.trim() ? (
          <pre className="whitespace-pre font-[var(--font-mono)] text-[11px] leading-[1.1] text-[var(--color-fg)]">
            {banner ? banner.text : "…"}
          </pre>
        ) : (
          <p className="p-2 text-[var(--color-muted)]">
            Type text to see it as a banner. ↑/↓ browse fonts · Tab fills the font · Enter copies.
          </p>
        )}
      </div>
    </div>
  );
}
