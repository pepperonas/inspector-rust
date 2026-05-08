import { useEffect, useMemo, useState } from "react";
import { Calculator, Copy, Palette, Wand2, Zap } from "lucide-react";
import type { ListEntry } from "../lib/types";
import { formatBytes } from "../lib/format";
import { readableForeground, tryParseColor } from "../lib/colors";
import { imageChromaticity, recolorImageEntry } from "../lib/ipc";

interface Props {
  entry: ListEntry | null;
}

export function PreviewPanel({ entry }: Props) {
  const parsedFiles = useMemo<string[] | null>(() => {
    if (!entry || entry.kind !== "clip" || entry.data.content_type !== "files") return null;
    try {
      return JSON.parse(entry.data.content_data) as string[];
    } catch {
      return null;
    }
  }, [entry]);

  if (!entry) {
    return (
      <div className="flex h-full items-center justify-center text-[13px] text-[var(--color-muted)]">
        Select an entry
      </div>
    );
  }

  // ── Color preview ──────────────────────────────────────────────────────────
  if (entry.kind === "color") {
    const c = entry.data;
    const fg = readableForeground(c.r, c.g, c.b);
    const copy = (text: string) => {
      void navigator.clipboard.writeText(text).catch(() => {});
    };
    return (
      <div className="flex h-full flex-col p-4">
        <div className="mb-3 flex items-center gap-2 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          <Palette size={12} className="text-[var(--color-accent)]" />
          <span>color</span>
          <span>·</span>
          <span>press Enter to paste hex</span>
        </div>
        <div
          className="flex h-32 items-center justify-center rounded-lg border border-[var(--color-border)] font-[var(--font-mono)] text-[24px] font-semibold tracking-wider"
          style={{ backgroundColor: c.hex, color: fg }}
        >
          {c.hex}
        </div>
        <div className="mt-3 grid grid-cols-[80px_1fr_auto] items-center gap-x-3 gap-y-1.5 text-[12px]">
          <span className="text-[var(--color-muted)]">Hex</span>
          <code className="rounded bg-[var(--color-surface)] px-1 py-0.5 font-[var(--font-mono)]">
            {c.hex}
          </code>
          <CopyButton onClick={() => copy(c.hex)} />

          <span className="text-[var(--color-muted)]">RGB</span>
          <code className="rounded bg-[var(--color-surface)] px-1 py-0.5 font-[var(--font-mono)]">
            {c.rgbString}
          </code>
          <CopyButton onClick={() => copy(c.rgbString)} />

          <span className="text-[var(--color-muted)]">HSL</span>
          <code className="rounded bg-[var(--color-surface)] px-1 py-0.5 font-[var(--font-mono)]">
            {c.hslString}
          </code>
          <CopyButton onClick={() => copy(c.hslString)} />
        </div>
      </div>
    );
  }

  // ── Calc preview ───────────────────────────────────────────────────────────
  if (entry.kind === "calc") {
    return (
      <div className="flex h-full flex-col p-4">
        <div className="mb-3 flex items-center gap-2 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          <Calculator size={12} className="text-[var(--color-accent)]" />
          <span>calculator</span>
          <span>·</span>
          <span>press Enter to paste result</span>
        </div>
        <div className="flex flex-1 flex-col items-stretch justify-center gap-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-6">
          <div className="text-center font-[var(--font-mono)] text-[14px] text-[var(--color-muted)]">
            {entry.data.expression}
          </div>
          <div className="text-center font-[var(--font-mono)] text-[28px] font-semibold leading-tight">
            = {entry.data.display}
          </div>
        </div>
      </div>
    );
  }

  // ── Snippet preview ────────────────────────────────────────────────────────
  if (entry.kind === "snippet") {
    const { abbreviation, title, body } = entry.data;
    return (
      <div className="flex h-full flex-col p-4">
        <div className="mb-3 flex items-center gap-2 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          <Zap size={12} className="text-[var(--color-accent)]" />
          <span>snippet</span>
          <span>·</span>
          <span>{formatBytes(body.length)}</span>
        </div>
        <div className="mb-3 flex items-baseline gap-3">
          <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-0.5 font-[var(--font-mono)] text-[13px] font-semibold">
            {abbreviation}
          </kbd>
          {title && (
            <span className="text-[13px] text-[var(--color-muted)]">{title}</span>
          )}
        </div>
        <pre className="flex-1 overflow-auto whitespace-pre-wrap rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 font-[var(--font-mono)] text-[12px] leading-5">
          {body}
        </pre>
      </div>
    );
  }

  // ── Clip preview ───────────────────────────────────────────────────────────
  const clip = entry.data;

  const meta = (
    <div className="mb-3 flex items-center gap-3 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
      <span>{clip.content_type}</span>
      <span>·</span>
      <span>{formatBytes(clip.byte_size)}</span>
    </div>
  );

  if (clip.content_type === "image") {
    const src = `data:image/png;base64,${clip.content_data}`;
    return (
      <div className="flex h-full flex-col p-4">
        {meta}
        <div className="flex flex-1 items-center justify-center overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]">
          <img
            src={src}
            alt="clipboard image"
            className="max-h-full max-w-full object-contain"
          />
        </div>
        <RecolorToolbar entryId={clip.id} />
      </div>
    );
  }

  if (clip.content_type === "files" && parsedFiles) {
    return (
      <div className="flex h-full flex-col p-4">
        {meta}
        <div className="flex-1 overflow-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 font-[var(--font-mono)] text-[12px]">
          {parsedFiles.map((p, i) => (
            <div key={i} className="truncate py-0.5">
              {p}
            </div>
          ))}
        </div>
      </div>
    );
  }

  if (clip.content_type === "html") {
    return (
      <div className="flex h-full flex-col p-4">
        {meta}
        <iframe
          sandbox=""
          srcDoc={clip.content_data}
          className="flex-1 rounded-lg border border-[var(--color-border)] bg-white"
          title="html preview"
        />
      </div>
    );
  }

  if (clip.content_type === "rtf") {
    return (
      <div className="flex h-full flex-col p-4">
        {meta}
        <pre className="flex-1 overflow-auto whitespace-pre-wrap rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 font-[var(--font-mono)] text-[12px] leading-5">
          {clip.content_text}
        </pre>
        <div className="mt-2 text-[11px] text-[var(--color-muted)]">
          RTF formatting will be preserved on paste.
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col p-4">
      {meta}
      <pre className="flex-1 overflow-auto whitespace-pre-wrap rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 font-[var(--font-mono)] text-[12px] leading-5">
        {clip.content_data}
      </pre>
    </div>
  );
}

function CopyButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      title="Copy"
      className="rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]"
    >
      <Copy size={11} />
    </button>
  );
}

// ── Recolor toolbar (image entries only) ────────────────────────────────────

const PRESETS: Array<{ hex: string; label: string }> = [
  { hex: "#CE422B", label: "Rust" },
  { hex: "#DC2626", label: "Red" },
  { hex: "#16A34A", label: "Green" },
  { hex: "#2563EB", label: "Blue" },
  { hex: "#9333EA", label: "Purple" },
  { hex: "#F59E0B", label: "Amber" },
  { hex: "#0891B2", label: "Cyan" },
  { hex: "#6B7280", label: "Gray" },
  { hex: "#000000", label: "Black" },
];

/** Inline recolor strip rendered below an image preview.
 *
 *  Renders only when the image looks "mostly grayscale" — saturated
 *  photos get hidden controls because tinting them produces the kind of
 *  Photoshop disaster nobody wants. Threshold is conservative (~12%
 *  chromaticity); silhouettes and logos pass, screenshots with a few
 *  coloured accents may not — by design. */
function RecolorToolbar({ entryId }: { entryId: number }) {
  const [eligible, setEligible] = useState<boolean | null>(null);
  const [hexInput, setHexInput] = useState("");
  const [hexValid, setHexValid] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reset and re-probe whenever the entry changes.
  useEffect(() => {
    let cancelled = false;
    setEligible(null);
    setError(null);
    imageChromaticity(entryId)
      .then((c) => {
        if (!cancelled) setEligible(c < 0.12);
      })
      .catch(() => {
        if (!cancelled) setEligible(false);
      });
    return () => {
      cancelled = true;
    };
  }, [entryId]);

  if (eligible !== true) return null;

  const apply = async (hex: string) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await recolorImageEntry(entryId, hex);
      // Frontend list refetches via "clipboard-changed" emitted by the
      // backend, so we don't need to do anything else here.
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const onHexChange = (value: string) => {
    setHexInput(value);
    setHexValid(value === "" || tryParseColor(value) !== null);
  };

  const onCustomApply = () => {
    const parsed = tryParseColor(hexInput);
    if (!parsed) {
      setHexValid(false);
      return;
    }
    void apply(`#${[parsed.r, parsed.g, parsed.b].map((n) => n.toString(16).padStart(2, "0")).join("")}`);
  };

  return (
    <div className="mt-3 flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5">
      <Wand2 size={13} className="shrink-0 text-[var(--color-muted)]" />
      <span className="shrink-0 text-[10px] uppercase tracking-wide text-[var(--color-muted)]">
        Recolor
      </span>
      <div className="flex flex-wrap items-center gap-1">
        {PRESETS.map((p) => (
          <button
            key={p.hex}
            onClick={() => void apply(p.hex)}
            disabled={busy}
            title={`${p.label} (${p.hex})`}
            className="h-5 w-5 shrink-0 rounded border border-[var(--color-border)] disabled:opacity-50"
            style={{ backgroundColor: p.hex }}
            aria-label={`Recolor to ${p.label}`}
          />
        ))}
      </div>
      <input
        type="text"
        value={hexInput}
        onChange={(e) => onHexChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            onCustomApply();
          }
        }}
        spellCheck={false}
        autoComplete="off"
        placeholder="#hex"
        disabled={busy}
        className={
          "ml-auto w-20 rounded border bg-[var(--color-bg)] px-1.5 py-0.5 font-[var(--font-mono)] text-[11px] outline-none disabled:opacity-50 " +
          (hexValid
            ? "border-[var(--color-border)] focus:border-[var(--color-accent)]"
            : "border-red-500 focus:border-red-500")
        }
      />
      {error && (
        <span className="text-[10px] text-red-400" title={error}>
          failed
        </span>
      )}
    </div>
  );
}
