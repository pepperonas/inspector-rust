import { useEffect, useMemo, useState } from "react";
import { Calculator, Check, Copy, Download, Palette, Scissors, Type, Wand2, Zap } from "lucide-react";
import type { ListEntry } from "../lib/types";
import { formatBytes } from "../lib/format";
import { readableForeground, tryParseColor } from "../lib/colors";
import { IS_MAC } from "../lib/platform";
import { TRANSFORMS, applyTransform, type TransformKind } from "../lib/text-transform";
import {
  commitTransformedText,
  cutOutImageEntry,
  cutOutImageFile,
  imageChromaticity,
  recolorImageEntry,
  saveImageEntryToDownloads,
} from "../lib/ipc";

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

  // ── Command preview (power-command palette) ───────────────────────────────
  if (entry.kind === "command") {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          Power command
        </div>
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div className="text-[14px] font-semibold leading-snug">{entry.data.label}</div>
          <div className="mt-2 text-[12px] text-[var(--color-muted)] leading-snug">
            {entry.data.hint}
          </div>
          <div className="mt-3 font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            ⏎ Enter to run
          </div>
        </div>
      </div>
    );
  }

  if (entry.kind === "kill-target") {
    const sig = entry.data.force ? "SIGKILL (force quit)" : "SIGTERM (graceful)";
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          Kill process
        </div>
        <div className="rounded-xl border border-red-500/40 bg-red-500/5 p-4">
          <div className="text-[14px] font-semibold leading-snug">{entry.data.name}</div>
          <div className="mt-2 grid grid-cols-[80px_1fr] gap-x-3 gap-y-1 text-[12px]">
            <span className="text-[var(--color-muted)]">PID</span>
            <span className="font-[var(--font-mono)] tabular-nums">{entry.data.pid}</span>
            <span className="text-[var(--color-muted)]">Memory</span>
            <span className="font-[var(--font-mono)] tabular-nums">{entry.data.memory_mb.toFixed(2)} MB</span>
            <span className="text-[var(--color-muted)]">Signal</span>
            <span className="font-[var(--font-mono)]">{sig}</span>
            {entry.data.exe && (
              <>
                <span className="text-[var(--color-muted)]">Path</span>
                <span className="break-all font-[var(--font-mono)] text-[11px]">{entry.data.exe}</span>
              </>
            )}
          </div>
          <div className="mt-3 font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            ⏎ Enter to confirm and kill
          </div>
        </div>
      </div>
    );
  }

  if (entry.kind === "command-suggestion") {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          Command suggestion
        </div>
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div className="font-[var(--font-mono)] text-[14px] font-semibold leading-snug">
            {entry.data.syntax}
          </div>
          <div className="mt-2 text-[12px] text-[var(--color-muted)] leading-snug">
            {entry.data.description}
          </div>
          <div className="mt-3 font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            ⏎ Enter completes into the search bar
          </div>
        </div>
      </div>
    );
  }

  if (entry.kind === "opener") {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          Random opener · ← → switches
        </div>
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div className="text-[14px] italic leading-snug">{entry.data.text}</div>
          <div className="mt-3 font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            ⏎ Enter pastes into the focused app &nbsp;·&nbsp; ← → cycles to the
            previous / next opener
          </div>
        </div>
      </div>
    );
  }

  if (entry.kind === "bruno") {
    const eur = new Intl.NumberFormat("de-DE", {
      style: "currency",
      currency: "EUR",
      maximumFractionDigits: 0,
    });
    const eurExact = new Intl.NumberFormat("de-DE", {
      style: "currency",
      currency: "EUR",
      maximumFractionDigits: 2,
    });
    const pct = new Intl.NumberFormat("de-DE", {
      style: "percent",
      maximumFractionDigits: 1,
    });
    const d = entry.data;
    const Row = ({ k, v, accent }: { k: string; v: string; accent?: boolean }) => (
      <div
        className={
          "flex items-baseline justify-between border-b border-[var(--color-border)] py-1.5 last:border-b-0 " +
          (accent ? "font-semibold text-[var(--color-fg)]" : "text-[var(--color-muted)]")
        }
      >
        <span>{k}</span>
        <span className="font-[var(--font-mono)] tabular-nums">{v}</span>
      </div>
    );
    const STATE_LABELS: Record<string, string> = {
      bw: "Baden-Württemberg", by: "Bayern", be: "Berlin", bb: "Brandenburg",
      hb: "Bremen", hh: "Hamburg", he: "Hessen", mv: "Mecklenburg-Vorp.",
      ni: "Niedersachsen", nw: "Nordrhein-Westfalen", rp: "Rheinland-Pfalz",
      sl: "Saarland", sn: "Sachsen", st: "Sachsen-Anhalt",
      sh: "Schleswig-Holstein", th: "Thüringen",
    };
    return (
      <div className="flex h-full flex-col gap-3 overflow-auto p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          Brutto → Netto · Steuerjahr 2025 (vereinfacht)
        </div>
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div className="mb-2 text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
            Annahmen
          </div>
          <div className="text-[12px] leading-tight text-[var(--color-fg)]">
            Klasse {d.taxClass} · {STATE_LABELS[d.state] ?? d.state.toUpperCase()} ·{" "}
            {d.children === 0 ? "kinderlos" : `${d.children} Kind${d.children === 1 ? "" : "er"}`}{" "}
            · {d.isChurchMember ? "kirchensteuerpflichtig" : "keine Kirchensteuer"}
          </div>
          <div className="mt-1 text-[11px] text-[var(--color-muted)]">
            Über Settings → Bruno anpassen.
          </div>
        </div>

        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <Row k="Brutto / Jahr" v={eur.format(d.yearlyGross)} />
          <Row k="Brutto / Monat" v={eur.format(d.yearlyGross / 12)} />
          <Row k="Krankenversicherung" v={"− " + eurExact.format(d.social.health)} />
          <Row k="Pflegeversicherung" v={"− " + eurExact.format(d.social.care)} />
          <Row k="Rentenversicherung" v={"− " + eurExact.format(d.social.pension)} />
          <Row k="Arbeitslosenversicherung" v={"− " + eurExact.format(d.social.unemployment)} />
          <Row k="Einkommensteuer" v={"− " + eurExact.format(d.incomeTax)} />
          {d.soli > 0 && <Row k="Solidaritätszuschlag" v={"− " + eurExact.format(d.soli)} />}
          {d.churchTax > 0 && <Row k="Kirchensteuer" v={"− " + eurExact.format(d.churchTax)} />}
          <Row k="Summe Abgaben" v={eur.format(d.totalDeductions)} />
          <Row k="Abgabenquote" v={pct.format(d.deductionRate)} />
          <Row k="Grenzsteuersatz" v={pct.format(d.marginalRate)} />
        </div>

        <div className="rounded-xl border border-[var(--color-accent)]/40 bg-[var(--color-accent)]/5 p-4">
          <Row k="Netto / Monat" v={eurExact.format(d.netMonth)} accent />
          <Row k="Netto / Jahr" v={eurExact.format(d.netYear)} accent />
        </div>

        <div className="font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
          ⏎ Enter kopiert {d.period === "monthly" ? "Monats-Netto" : "Jahres-Netto"} ins Clipboard
          {" "}·{" "}
          ⚠ Vereinfacht: keine Faktorverfahren / Freibeträge / Lohnsteuer-Ermäßigungen.
        </div>
      </div>
    );
  }

  if (entry.kind === "finder-file") {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          Finder selection
        </div>
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div className="truncate text-[14px] font-semibold leading-snug">
            {entry.data.name}
          </div>
          <div className="mt-2 break-all font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            {entry.data.path}
          </div>
          {entry.data.size_bytes != null && (
            <div className="mt-1 text-[11px] text-[var(--color-muted)]">
              {formatBytes(entry.data.size_bytes)}
            </div>
          )}
          <div className="mt-3 font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            ⏎ Enter opens the file
            {entry.data.is_image && (
              <>
                {" "}&nbsp;·&nbsp; type{" "}
                <span className="font-semibold text-[var(--color-fg)]">
                  rz 1200x800
                </span>{" "}
                to resize all selected images
              </>
            )}
          </div>
        </div>
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
        <CutoutButton source={{ kind: "entry", entryId: clip.id }} />
        <SaveImageButton entryId={clip.id} />
        <RecolorToolbar entryId={clip.id} />
      </div>
    );
  }

  if (clip.content_type === "files" && parsedFiles) {
    // Single image file → enable cutout. Detection is purely
    // extension-based; the backend re-validates by trying to decode
    // the bytes, so a misleading extension (".png" file that's
    // actually a text file) just surfaces a clear error instead of
    // crashing. HEIC isn't in the `image` crate's decoder set, so
    // it's deliberately omitted from the trigger list.
    const imageExt = /\.(png|jpe?g|webp|gif|bmp)$/i;
    const lone = parsedFiles.length === 1 ? parsedFiles[0] : null;
    const cutoutTarget = lone && imageExt.test(lone) ? lone : null;
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
        {cutoutTarget && <CutoutButton source={{ kind: "file", path: cutoutTarget }} />}
      </div>
    );
  }

  if (clip.content_type === "html") {
    // Clipboard HTML usually arrives with the source page's own
    // colours / fonts baked in as inline styles — copy from a dark-mode
    // site, you get black backgrounds; copy from a light-mode site,
    // you get a glaring white sheet on top of the app's dark theme.
    // Neither matches Inspector Rust's UI. Inject a theme-aware base
    // style into the iframe + override the source's background and
    // text colours so the preview reads in the app's theme. Inline
    // styles that aren't background / text colour (layout, sizing,
    // borders' radius, image styling) survive — only the colour war
    // is suppressed.
    const cs = getComputedStyle(document.documentElement);
    const v = (n: string, fb: string) => cs.getPropertyValue(n).trim() || fb;
    const fg = v("--color-fg", "#e0e0e0");
    const bg = v("--color-surface", "#15171d");
    const muted = v("--color-muted", "#9a9fac");
    const accent = v("--color-accent", "#6366f1");
    const border = v("--color-border", "#2b2e38");
    const themedSrcDoc = `<!doctype html><html><head><meta charset="utf-8"><style>
      :root { color-scheme: dark; }
      html, body {
        margin: 0;
        padding: 12px;
        background: ${bg};
        color: ${fg};
        font: 13px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
      }
      /* Override the pasted HTML's colour decisions so the preview
         matches the app theme. Layout / sizing / images are left
         alone — we only suppress colour clashes. */
      body, body * {
        background-color: transparent !important;
        color: ${fg} !important;
        border-color: ${border} !important;
      }
      a, body a * { color: ${accent} !important; }
      code, pre {
        background: rgba(127, 127, 127, 0.12) !important;
        font-family: ui-monospace, SFMono-Regular, Menlo, monospace !important;
      }
      img { max-width: 100%; height: auto; }
      table { border-collapse: collapse; }
      td, th { border: 1px solid ${border} !important; padding: 4px 8px; }
      blockquote {
        margin: 8px 0 8px 0;
        padding-left: 12px;
        border-left: 3px solid ${accent} !important;
        color: ${muted} !important;
      }
    </style></head><body>${clip.content_data}</body></html>`;
    return (
      <div className="flex h-full flex-col p-4">
        {meta}
        <iframe
          sandbox=""
          srcDoc={themedSrcDoc}
          className="flex-1 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]"
          title="html preview"
        />
        {/* HTML clips have a text representation in `content_text` —
            offer the same string transforms as plain-text entries. */}
        {clip.content_text && <TransformBar text={clip.content_text} />}
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
        {/* RTF clips: same string transforms apply to the plain-text
            representation, like OCR / HTML / plain text. */}
        {clip.content_text && <TransformBar text={clip.content_text} />}
      </div>
    );
  }

  // Default text branch — also where OCR-recognised text (saved as
  // content_type=Text) lands, so the transform bar shows there too.
  return (
    <div className="flex h-full flex-col p-4">
      {meta}
      <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 font-[var(--font-mono)] text-[12px] leading-5">
        {clip.content_data}
      </pre>
      <TransformBar text={clip.content_text} />
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

// ── Cut-out background button (image entries only) ──────────────────────────

/** Source the cutout reads from. `entry` mode pulls the PNG bytes
 *  from a clipboard image entry in the DB; `file` mode reads from disk
 *  (so JPG / WebP / GIF / BMP files dragged from Finder also work). */
type CutoutSource =
  | { kind: "entry"; entryId: number }
  | { kind: "file"; path: string };

/** "Freistellen" / background-removal action. Chroma-keys the image
 *  using the four corner colours, writes the transparent PNG to
 *  `~/Downloads/`, and shows the saved filename for a few seconds.
 *
 *  Works on any clipboard image entry (PNG-encoded by the watcher) and
 *  on any single-file Files entry pointing at an image. The last-saved
 *  filename remains visible so the user knows where the file went;
 *  cleared whenever the source changes. */
function CutoutButton({ source }: { source: CutoutSource }) {
  const [busy, setBusy] = useState(false);
  const [savedTo, setSavedTo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Re-key on the source identity so feedback resets when the user
  // moves between entries (different ids OR different file paths).
  const sourceKey = source.kind === "entry" ? `e:${source.entryId}` : `f:${source.path}`;
  useEffect(() => {
    setSavedTo(null);
    setError(null);
  }, [sourceKey]);

  const run = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const path =
        source.kind === "entry"
          ? await cutOutImageEntry(source.entryId)
          : await cutOutImageFile(source.path);
      setSavedTo(path);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  // Cmd/Ctrl+B is the documented shortcut. Registered at the window
  // level so it fires regardless of focus, but scoped to the lifetime
  // of this component — i.e. only while a cutout-eligible entry is
  // selected. Non-eligible entries don't render the button, so the
  // shortcut is implicitly gated.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "b") {
        e.preventDefault();
        void run();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
    // `run` closes over `busy` and the source; re-binding on each
    // change is cheap and keeps the closure current.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sourceKey, busy]);

  const filename = savedTo ? savedTo.split("/").pop() : null;

  return (
    <div className="mt-3 flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5">
      <button
        onClick={() => void run()}
        disabled={busy}
        title="Remove background and save to Downloads (Cmd/Ctrl+B)"
        className={
          "flex items-center gap-1.5 rounded px-2 py-1 text-[11px] font-medium " +
          (busy
            ? "cursor-wait bg-[var(--color-bg)] text-[var(--color-muted)]"
            : "bg-[var(--color-accent)] text-[var(--color-accent-fg)] hover:opacity-90")
        }
      >
        <Scissors size={12} />
        {busy ? "Cutting…" : "Cut out background"}
      </button>
      <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-muted)]">
        ⌘B
      </kbd>
      {savedTo && (
        <span className="ml-auto flex items-center gap-1 truncate text-[11px] text-emerald-400" title={savedTo}>
          <Check size={11} />
          Saved <span className="font-[var(--font-mono)]">{filename}</span>
        </span>
      )}
      {error && (
        <span className="ml-auto text-[11px] text-red-400" title={error}>
          failed
        </span>
      )}
    </div>
  );
}

// ── Save-image-to-Downloads button (image entries only) ─────────────────────

/** "Save to Downloads" — writes the selected image entry's PNG bytes
 *  unchanged to `~/Downloads/inspector-rust-image-<ts>.png`. The companion
 *  to the recolor flow: clicking a recolor swatch creates a new
 *  history entry, and this button takes that new entry off the
 *  in-app DB and onto disk. Same UX shape as `CutoutButton`. */
function SaveImageButton({ entryId }: { entryId: number }) {
  const [busy, setBusy] = useState(false);
  const [savedTo, setSavedTo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setSavedTo(null);
    setError(null);
  }, [entryId]);

  const run = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const path = await saveImageEntryToDownloads(entryId);
      setSavedTo(path);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  // Cmd/Ctrl+S — handler is scoped to the lifetime of this component,
  // so it only fires when an image entry is the current selection.
  // Non-image entries don't render the button → handler isn't bound.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void run();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entryId, busy]);

  const filename = savedTo ? savedTo.split("/").pop() : null;

  return (
    <div className="mt-3 flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5">
      <button
        onClick={() => void run()}
        disabled={busy}
        title="Save current image to Downloads (Cmd/Ctrl+S)"
        className={
          "flex items-center gap-1.5 rounded px-2 py-1 text-[11px] font-medium " +
          (busy
            ? "cursor-wait bg-[var(--color-bg)] text-[var(--color-muted)]"
            : "bg-[var(--color-accent)] text-[var(--color-accent-fg)] hover:opacity-90")
        }
      >
        <Download size={12} />
        {busy ? "Saving…" : "Save to Downloads"}
      </button>
      <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-muted)]">
        ⌘S
      </kbd>
      {savedTo && (
        <span className="ml-auto flex items-center gap-1 truncate text-[11px] text-emerald-400" title={savedTo}>
          <Check size={11} />
          Saved <span className="font-[var(--font-mono)]">{filename}</span>
        </span>
      )}
      {error && (
        <span className="ml-auto text-[11px] text-red-400" title={error}>
          failed
        </span>
      )}
    </div>
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

/** String-manipulation toolbar shown under a selected *text* entry.
 *  Each chip applies a transform from `lib/text-transform.ts`; the
 *  result is committed to the clipboard + a new History entry via
 *  `commit_transformed_text`. The first nine transforms also bind to
 *  `Cmd/Ctrl+1…9` while a text entry is selected. */
function TransformBar({ text }: { text: string }) {
  const run = async (kind: TransformKind) => {
    try {
      await commitTransformedText(applyTransform(kind, text));
    } catch (e) {
      console.error("transform commit failed", e);
    }
  };

  // Cmd/Ctrl+1…9 → the digit-bound transforms. Digits alone can't be
  // used (they'd type into the search bar); Cmd/Ctrl+digit is the same
  // CmdOrCtrl pattern as ⌘B / ⌘S.
  //
  // Cmd/Ctrl+^ → "Plain text" (strip HTML / RTF styling). Accepts
  // either Shift state since `^` requires Shift on US layouts
  // (Shift+6) but is a bare keypress on German ISO. Only Alt is
  // rejected to leave the German Alt+^ free for whatever the OS
  // might map it to.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      if (e.key === "^") {
        e.preventDefault();
        void run("plain-text");
        return;
      }
      // Digit shortcut path: reject shift so Shift+digit (which types
      // !@#$… on US) doesn't trigger a transform.
      if (e.shiftKey) return;
      if (!/^[1-9]$/.test(e.key)) return;
      const spec = TRANSFORMS.find((t) => t.digit === Number(e.key));
      if (!spec) return;
      e.preventDefault();
      void run(spec.kind);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // `text` is the only thing `run` closes over that changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text]);

  const mod = IS_MAC ? "⌘" : "Ctrl+";

  return (
    <div className="mt-2 shrink-0 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2">
      <div className="mb-1.5 flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">
        <Type size={12} className="text-[var(--color-accent)]" />
        Transform → new entry + clipboard
      </div>
      <div className="flex flex-wrap gap-1.5">
        {TRANSFORMS.map((t) => {
          // Special-case the plain-text transform — it carries no digit
          // but is bound to Cmd/Ctrl+^ via the keyboard handler above.
          const badge =
            t.digit != null
              ? `${mod}${t.digit}`
              : t.kind === "plain-text"
                ? `${mod}^`
                : null;
          return (
            <button
              key={t.kind}
              onClick={() => void run(t.kind)}
              title={badge ?? undefined}
              className="flex items-center gap-1 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[11px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
            >
              {badge && (
                <kbd className="rounded bg-[var(--color-surface)] px-1 font-[var(--font-mono)] text-[9px] text-[var(--color-muted)]">
                  {badge}
                </kbd>
              )}
              {t.label}
            </button>
          );
        })}
      </div>
    </div>
  );
}
