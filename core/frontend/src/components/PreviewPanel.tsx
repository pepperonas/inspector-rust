import { Suspense, lazy, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { QrPreview } from "./QrPreview";
import { STATE_LABELS, brunoSelfAssumptions, buildBrunoExport } from "../lib/bruno";
import {
  AudioLines, Calculator, Check, Copy, Download, ExternalLink, GitBranch, Loader2, Mail, MapPin, Music, Palette,
  Pencil, Phone, Plus, QrCode, Scissors, StickyNote, Wand2, Zap,
} from "lucide-react";
import { SnippetEditor, type SnippetDraft } from "./SnippetEditor";
import { TotpBrandIcon } from "./TotpBrandIcon";
import { FakerPreview } from "./FakerPreview";
import { SecPreview } from "./SecPreview";
import { FigletPreview } from "./FigletPreview";
import type { FigletOpts } from "../lib/figlet";
import type { CatalogEntry, FakerDefaults } from "../lib/faker";
import type { SecCatalog, SecDefaults } from "../lib/sec";
import type { SnippetCategory } from "../lib/ipc";
import { openUrl } from "@tauri-apps/plugin-opener";
import { writeText as clipboardWriteText } from "@tauri-apps/plugin-clipboard-manager";
import type { ClipEntry, ListEntry } from "../lib/types";
import { detectSmartActions, type SmartActionKind } from "../lib/smart-actions";
import { LinkGrabber } from "./LinkGrabber";
import { MetaCard, useMeta } from "./SocialMeta";
import { ExportRow } from "./ExportRow";
import { TrimBar } from "./TrimBar";
import { fmtClock, fullRange, isFullRange, sectionFor, type Range } from "../lib/trim-range";
import { detectSocial, platformLabel, type SocialTarget } from "../lib/social";
import { findRepoUrl } from "../lib/repo-url";
import { needsFullText } from "../lib/clip-text";
import { TotpSecondsLeft } from "./TotpSecondsLeft";
import { AnimatedNumber } from "./AnimatedNumber";
import { qrPngBase64 } from "../lib/qr";
import { formatBytes } from "../lib/format";
import { resolveFileWrite } from "../lib/touch";
import { readableForeground, tryParseColor } from "../lib/colors";
import { IS_MAC } from "../lib/platform";
import { TRANSLATE_LANGS, isTranslateKind } from "../lib/commands";
import type { CommandKind } from "../lib/commands";
import {
  commitTransformedText,
  cutOutImageEntry,
  cutOutImageFile,
  getClip,
  imageChromaticity,
  qrCopyPng,
  recolorImageEntry,
  saveImageEntryToDownloads,
  setClipNote,
  socialDownload,
  socialYtdlpAvailable,
  brunoExport,
} from "../lib/ipc";
import { InlineMd } from "./InlineMd";
import { locale, t, tn, tSplit, useT, type MessageKey } from "../lib/i18n";

interface Props {
  entry: ListEntry | null;
  /** Active pwgen mode (v0.40.0+). Used to drive the radio-style
   *  mode picker in the preview pane when the selected row is a
   *  `pwgen` entry. */
  pwgenMode?: "all" | "alnum" | "dict" | "leet";
  /** Setter — App.tsx owns the state; PreviewPanel just dispatches
   *  user clicks on the mode buttons. */
  onPwgenModeChange?: (m: "all" | "alnum" | "dict" | "leet") => void;
  /** Bump to force a fresh password generation without changing mode
   *  (the "regenerate" button calls this). */
  onPwgenReroll?: () => void;
  /** The user edited the generated password in the field (v0.65.0). */
  onPwgenEdit?: (value: string) => void;
  /** The password field gained / lost focus — App disables the global
   *  keyboard nav while it's focused so typing edits the password. */
  onPwgenEditingChange?: (editing: boolean) => void;
  /** Enter in the password field — copy the (possibly edited) password. */
  onPwgenCommit?: () => void;
  /** Tab-selected download mode for the selected `social` (YouTube) row. */
  socialMode?: "video" | "audio";
  /** A social download button was clicked — sync the Tab selection. */
  onSocialModeChange?: (m: "video" | "audio") => void;
  /** Bumped by the global Enter key to trigger a download of `socialMode`. */
  socialRunSignal?: number;
  /** A GitHub repo URL (row or inside a clip) should open the `repo` panel. */
  onAnalyzeRepo?: (webUrl: string) => void;
  /** Faker catalogue + defaults (for the `faker` command preview) + a reroll
   *  signal bumped by ⌘/Ctrl+R. */
  fakerCatalog?: CatalogEntry[];
  fakerDefaults?: FakerDefaults;
  fakerReroll?: number;
  /** Security-builder catalogue + defaults (for the `sec` command preview). */
  secCatalog?: SecCatalog;
  secDefaults?: SecDefaults;
  /** Snippet groups — needed by the inline snippet editor's group picker. */
  snippetCategories?: SnippetCategory[];
  /** True while the selected snippet is being edited in place (v0.84.265):
   *  the preview renders the editor instead of the read-only body. App.tsx owns
   *  the flag because it must also disable the global list navigation. */
  snippetEditing?: boolean;
  /** The ✏️ button was clicked — App.tsx opens the editor (same as Cmd+E). */
  onSnippetEdit?: () => void;
  /** Saved — App.tsx refreshes the snippet list and closes the editor. */
  onSnippetSaved?: () => void | Promise<void>;
  /** Esc / Cancel in the editor. */
  onSnippetEditCancel?: () => void;
  /** Figlet command: the parsed banner text + current render opts + a setter
   *  for the option chips (App.tsx owns the state so Enter copies with them). */
  figletText?: string;
  figletOpts?: FigletOpts;
  onFigletOptsChange?: (patch: Partial<FigletOpts>) => void;
  /** Live translation for the `tr*` commands (v0.93.0): the debounced result of
   *  the keyless Google/MyMemory lookup, rendered directly in the preview. The
   *  "Enter opens Google Translate" browser fallback stays regardless. */
  liveTranslation?: PreviewLiveTranslation | null;
  /** The 2fa preview's subtle "＋ Add account" button (v0.104.0) — App.tsx
   *  opens the TOTP overlay straight on the Add form (issuer pre-filled when
   *  the selected row carries one). */
  onTotpAdd?: (issuer?: string) => void;
}

/** The live-translation state passed down from App.tsx. */
export type PreviewLiveTranslation =
  | { status: "loading" }
  | { status: "ok"; text: string; provider: string; detectedSource: string }
  | { status: "error" };

/** One label/value line in the Bruno (net-pay) breakdown. Module-level so its
 *  identity is stable across PreviewPanel re-renders — otherwise the animated
 *  Netto rows would remount (and restart their slot-machine roll) on every
 *  unrelated render. `animate` rolls the digits like the calculator result. */
/**
 * Export chips for the Bruno breakdown — ONE component for both modes so the
 * affordance cannot drift, wired to the shared `ExportRow` like every report
 * panel. The payload is built by the pure `buildBrunoExport`, the SAME mapping
 * for preview and PDF.
 */
function BrunoExportRow({ data }: { data: Parameters<typeof buildBrunoExport>[0] }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const run = (fmt: "html" | "pdf") => {
    setBusy(fmt);
    setDone(null);
    void brunoExport(buildBrunoExport(data), fmt)
      .then((p) => setDone(p.split(/[/\\]/).pop() ?? p))
      .catch((e) => setDone(t("common.error_detail", { detail: String(e) })))
      .finally(() => setBusy(null));
  };
  return <ExportRow formats={["html", "pdf"]} busy={busy} done={done} onExport={run} />;
}

function BrunoRow({
  k,
  v,
  accent,
  animate,
}: {
  k: string;
  v: string;
  accent?: boolean;
  animate?: boolean;
}) {
  return (
    <div
      className={
        "flex items-baseline justify-between border-b border-[var(--color-border)] py-1.5 last:border-b-0 " +
        (accent ? "font-semibold text-[var(--color-fg)]" : "text-[var(--color-muted)]")
      }
    >
      <span>{k}</span>
      <span className="font-[var(--font-mono)] tabular-nums">
        {animate ? <AnimatedNumber value={v} /> : v}
      </span>
    </div>
  );
}

/** Human-readable names for the language codes used by the `tr*` commands. */
const LANG_NAME_KEYS: Record<string, MessageKey> = {
  auto: "tr.lang.auto",
  en: "tr.lang.en",
  de: "tr.lang.de",
  it: "tr.lang.it",
  es: "tr.lang.es",
  pl: "tr.lang.pl",
};
function langName(code: string): string {
  const key = LANG_NAME_KEYS[code];
  return key ? t(key) : code;
}

/**
 * Live-translation preview for the `tr*` commands (v0.93.0; Enter=copy v0.101.5).
 * Enter copies the translation; ⇧Enter opens Google Translate. Click either
 * language box to copy that side's text.
 */
function TranslatePreview({
  kind,
  text,
  live,
}: {
  kind: CommandKind;
  text: string;
  live: PreviewLiveTranslation | null | undefined;
}) {
  const pair = TRANSLATE_LANGS[kind];
  const srcName = pair ? langName(pair.sl) : "";
  const tgtName = pair ? langName(pair.tl) : "";
  const detected =
    live?.status === "ok" && live.detectedSource
      ? langName(live.detectedSource)
      : null;
  const [copiedSide, setCopiedSide] = useState<"src" | "tgt" | null>(null);

  const copySide = async (side: "src" | "tgt", value: string) => {
    if (!value) return;
    try {
      await clipboardWriteText(value);
      setCopiedSide(side);
      window.setTimeout(() => setCopiedSide(null), 1200);
    } catch {
      /* ignore */
    }
  };

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4">
      <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
        {t("tr.header", { src: detected ?? srcName, tgt: tgtName })}
      </div>

      {text.length === 0 ? (
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 text-[12px] text-[var(--color-muted)]">
          {t("tr.empty")}
        </div>
      ) : (
        <>
          <button
            type="button"
            onClick={() => void copySide("src", text)}
            title={t("tr.copy_source")}
            className="group w-full rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 text-left transition-colors hover:border-[var(--color-accent)]/50"
          >
            <div className="flex items-center justify-between">
              <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
                {detected ?? srcName}
              </div>
              <span className="flex items-center gap-1 text-[10px] text-[var(--color-muted)] opacity-0 transition-opacity group-hover:opacity-100">
                {copiedSide === "src" ? (
                  <span className="md3-success-pop items-center gap-1"><Check size={11} /> {t("common.copied")}</span>
                ) : (
                  <><Copy size={11} /> {t("common.copy")}</>
                )}
              </span>
            </div>
            <div className="mt-1 text-[13px] leading-snug">{text}</div>
          </button>

          <button
            type="button"
            onClick={() => {
              if (live?.status === "ok") void copySide("tgt", live.text);
            }}
            disabled={live?.status !== "ok"}
            title={
              live?.status === "ok"
                ? t("tr.copy_target")
                : t("tr.not_ready")
            }
            className="group w-full rounded-xl border border-[var(--color-accent)]/40 bg-[var(--color-accent)]/5 p-4 text-left transition-colors hover:border-[var(--color-accent)] disabled:cursor-default disabled:hover:border-[var(--color-accent)]/40"
          >
            <div className="flex items-center justify-between">
              <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
                {tgtName}
              </div>
              {live?.status === "loading" ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin text-[var(--color-muted)]" />
              ) : live?.status === "ok" ? (
                <span className="flex items-center gap-1 text-[10px] text-[var(--color-muted)] opacity-0 transition-opacity group-hover:opacity-100">
                  {copiedSide === "tgt" ? (
                    <span className="md3-success-pop items-center gap-1"><Check size={11} /> {t("common.copied")}</span>
                  ) : (
                    <><Copy size={11} /> {t("common.copy")}</>
                  )}
                </span>
              ) : null}
            </div>
            {live?.status === "ok" ? (
              <>
                <div className="mt-1 text-[15px] font-semibold leading-snug">{live.text}</div>
                <div className="mt-2 text-[10px] uppercase tracking-wide text-[var(--color-muted)]">
                  {t("tr.via", { provider: live.provider })}
                </div>
              </>
            ) : live?.status === "error" ? (
              <div className="mt-1 text-[12px] text-[var(--color-muted)]">
                {t("tr.unavailable")}
              </div>
            ) : (
              <div className="mt-1 text-[13px] italic text-[var(--color-muted)]">
                {live?.status === "loading" ? t("tr.translating") : "…"}
              </div>
            )}
          </button>
        </>
      )}

      <div className="mt-auto font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
        {t("tr.footer")}
      </div>
    </div>
  );
}


export function PreviewPanel({
  entry,
  pwgenMode,
  onPwgenModeChange,
  onPwgenReroll,
  onPwgenEdit,
  onPwgenEditingChange,
  onPwgenCommit,
  socialMode,
  onSocialModeChange,
  socialRunSignal,
  onAnalyzeRepo,
  fakerCatalog = [],
  fakerDefaults,
  fakerReroll = 0,
  secCatalog,
  secDefaults,
  snippetCategories = [],
  snippetEditing = false,
  onSnippetEdit,
  onSnippetSaved,
  onSnippetEditCancel,
  figletText = "",
  figletOpts,
  onFigletOptsChange,
  liveTranslation,
  onTotpAdd,
}: Props) {
  // Subscribes the panel (and its non-memo children) to language switches.
  useT();
  // Stable identity of the selected CLIP for the memos below. `[entry]` deps
  // were an identity trap: App rebuilds `combined` per keystroke, so the SAME
  // selected clip arrives in a fresh `{kind, data}` wrapper every key — the
  // memos re-ran each time (a forced-style-recalc getComputedStyle + multi-KB
  // template for html clips, a JSON.parse for files clips) despite their
  // comment claiming otherwise. Clip content is immutable per id (rows are
  // hash-keyed), so (kind-narrowed) id + type is the true dependency.
  const clipId = entry?.kind === "clip" ? entry.data.id : null;
  const clipType = entry?.kind === "clip" ? entry.data.content_type : null;

  const parsedFiles = useMemo<string[] | null>(() => {
    if (!entry || entry.kind !== "clip" || entry.data.content_type !== "files") return null;
    try {
      return JSON.parse(entry.data.content_data) as string[];
    } catch {
      return null;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clipId, clipType]);

  // The themed HTML `srcDoc` is expensive to build (a `getComputedStyle` +
  // template assembly). Memoised on the clip's stable identity (see above),
  // so it truly recomputes only when the selection changes. Hooks must run
  // unconditionally, so compute here (top level) and consume in the branches
  // below. (The image `src` is fetched on demand by <ImagePreview> — the slim
  // history list omits image blobs.)
  const htmlSrcDoc = useMemo<string | null>(() => {
    if (entry?.kind !== "clip" || entry.data.content_type !== "html") return null;
    const cs = getComputedStyle(document.documentElement);
    const v = (n: string, fb: string) => cs.getPropertyValue(n).trim() || fb;
    const fg = v("--color-fg", "#e0e0e0");
    const bg = v("--color-surface", "#15171d");
    const muted = v("--color-muted", "#9a9fac");
    const accent = v("--color-accent", "#6366f1");
    const border = v("--color-border", "#2b2e38");
    return `<!doctype html><html><head><meta charset="utf-8"><style>
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
    </style></head><body>${entry.data.content_data}</body></html>`;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clipId, clipType]);

  if (!entry) {
    return (
      <div className="flex h-full items-center justify-center text-[13px] text-[var(--color-muted)]">
        {t("preview.select")}
      </div>
    );
  }

  // ── Color preview ──────────────────────────────────────────────────────────
  // ── Social download (URL typed/pasted into the search bar) ──────────────────
  if (entry.kind === "repo-url") {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="flex items-center gap-2 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          <GitBranch size={12} className="text-rose-500" />
          <span>{t("preview.repo.title")}</span>
        </div>
        <p className="text-[15px] font-medium">{entry.data.owner}/{entry.data.repo}</p>
        <p className="break-all font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">{entry.data.web_url}</p>
        <p className="text-[12px] leading-snug text-[var(--color-muted)]">
          {t("preview.repo.hint")}
        </p>
      </div>
    );
  }

  if (entry.kind === "social") {
    return (
      <div className="flex h-full flex-col p-4">
        <div className="mb-3 flex items-center gap-2 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          <Download size={12} className="text-rose-500" />
          <span>{t("preview.social.title", { platform: platformLabel(entry.data.platform) })}</span>
        </div>
        <div className="break-all rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
          {entry.data.url}
        </div>
        <MetaCard url={entry.data.url} />
        <SocialDownloadBar
          target={entry.data}
          mode={socialMode}
          onModeChange={onSocialModeChange}
          runSignal={socialRunSignal}
        />
        <LinkGrabber seedUrl={entry.data.url} />
      </div>
    );
  }

  if (entry.kind === "color") {
    const c = entry.data;
    const fg = readableForeground(c.r, c.g, c.b);
    // Use the Tauri clipboard plugin (not the browser `navigator.clipboard`,
    // which can fail silently in the WKWebView when the popup was just reshown
    // without a fresh user gesture).
    const copy = (text: string) => {
      void clipboardWriteText(text).catch(() => {});
    };
    return (
      <div className="flex h-full flex-col p-4">
        <div className="mb-3 flex items-center gap-2 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          <Palette size={12} className="text-[var(--color-accent)]" />
          <span>{t("preview.color.title")}</span>
          <span>·</span>
          <span>{t("preview.color.hint")}</span>
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
          <span>{t("preview.calc.title")}</span>
          <span>·</span>
          <span>{t("preview.calc.hint")}</span>
        </div>
        <div className="flex flex-1 flex-col items-stretch justify-center gap-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-6">
          <div className="text-center font-[var(--font-mono)] text-[14px] text-[var(--color-muted)]">
            {entry.data.expression}
          </div>
          <div className="text-center font-[var(--font-mono)] text-[28px] font-semibold leading-tight">
            = <AnimatedNumber value={entry.data.display} />
          </div>
        </div>
      </div>
    );
  }

  // ── Snippet preview (+ inline editor, v0.84.265) ───────────────────────────
  if (entry.kind === "snippet") {
    const { id, abbreviation, title, body, category, version } = entry.data;

    // Editing in place: the prompt you just searched for is the one you want to
    // fix, so ✏️ / Cmd+E swaps this pane for the real editor — no tab switch,
    // the search stays, and Enter pastes the edited version right after saving.
    if (snippetEditing) {
      const draft: SnippetDraft = {
        id,
        abbreviation,
        title,
        body,
        categoryId: category
          ? snippetCategories.find((c) => c.name === category)?.id ?? null
          : null,
        version,
      };
      return (
        <div className="flex h-full flex-col p-4">
          <SnippetEditor
            key={id}
            initial={draft}
            categories={snippetCategories}
            // The body is what you came to change — start there.
            autoFocus="body"
            onSaved={() => onSnippetSaved?.()}
            onCancel={() => onSnippetEditCancel?.()}
          />
        </div>
      );
    }

    return (
      <div className="flex h-full flex-col p-4">
        <div className="mb-3 flex items-center gap-2 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          <Zap size={12} className="text-[var(--color-accent)]" />
          <span>{t("preview.snippet.title")}</span>
          <span>·</span>
          <span>{formatBytes(body.length)}</span>
          {category && (
            <>
              <span>·</span>
              <span>{category}</span>
            </>
          )}
          <span>·</span>
          <span className="font-[var(--font-mono)]" title={t("preview.snippet.revision", { n: version ?? 1 })}>
            v{version ?? 1}
          </span>
        </div>
        <div className="mb-3 flex items-center gap-3">
          <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-0.5 font-[var(--font-mono)] text-[13px] font-semibold">
            {abbreviation}
          </kbd>
          {title && (
            <span className="truncate text-[13px] text-[var(--color-muted)]">{title}</span>
          )}
          <button
            onClick={() => onSnippetEdit?.()}
            title={t("preview.snippet.edit_title")}
            aria-label={t("preview.snippet.edit_aria")}
            className="md3-press ml-auto flex shrink-0 items-center gap-1 rounded border border-[var(--color-border)] px-2 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
          >
            <Pencil size={12} />
            {t("common.edit")}
          </button>
        </div>
        <pre className="flex-1 overflow-auto whitespace-pre-wrap rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 font-[var(--font-mono)] text-[12px] leading-5">
          {body}
        </pre>
      </div>
    );
  }

  // ── Figlet gallery: the selected font's big banner ────────────────────────
  if (entry.kind === "figlet-font") {
    return (
      <FigletPreview
        text={figletText}
        font={entry.data.name}
        category={entry.data.category}
        opts={figletOpts ?? { width: 80, align: "left", trim: true, comment: "none", boxed: false }}
        onOptsChange={onFigletOptsChange ?? (() => undefined)}
      />
    );
  }

  // ── Command preview (power-command palette) ───────────────────────────────
  if (entry.kind === "command") {
    if (entry.data.commandKind === "sec") {
      return (
        <SecPreview
          keyword={entry.data.rawInput.split(/\s+/)[0] || "sec"}
          arg={entry.data.arg}
          catalog={secCatalog ?? { tools: [], common_wordlists: [], john_formats: [] }}
          defaults={
            secDefaults ?? {
              wordlist: "",
              output_dir: "",
              timing: "",
              threads: 0,
              rate: 0,
              john_line: "jumbo",
              terminal: "iterm",
              auto_enter: false,
              scope_note: "",
              save_history: true,
            }
          }
        />
      );
    }
    if (entry.data.commandKind === "faker") {
      return (
        <FakerPreview
          arg={entry.data.arg}
          catalog={fakerCatalog}
          defaults={
            fakerDefaults ?? {
              locale: "DE_DE",
              count: 1,
              format: "plain",
              pinned: [],
              save_history: true,
            }
          }
          rerollSignal={fakerReroll}
        />
      );
    }
    if (isTranslateKind(entry.data.commandKind as CommandKind)) {
      return (
        <TranslatePreview
          kind={entry.data.commandKind as CommandKind}
          text={entry.data.arg.trim()}
          live={liveTranslation}
        />
      );
    }
    const isQr = entry.data.commandKind === "qr" && entry.data.arg.trim().length > 0;
    const fileWrite =
      entry.data.commandKind === "touch" || entry.data.commandKind === "echo"
        ? resolveFileWrite(entry.data.commandKind, entry.data.arg)
        : null;
    return (
      <div className="flex h-full flex-col gap-3 overflow-y-auto p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("preview.command.title")}
        </div>
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div className="text-[14px] font-semibold leading-snug">{entry.data.label}</div>
          <div className="mt-2 text-[12px] text-[var(--color-muted)] leading-snug">
            <InlineMd text={entry.data.hint} />
          </div>
          <div className="mt-3 font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            {isQr ? t("preview.command.enter_qr") : t("preview.command.enter_run")}
          </div>
        </div>
        {isQr && <QrPreview key={entry.data.arg} text={entry.data.arg} />}
        {fileWrite?.content && (
          <div className="flex min-h-0 flex-col gap-1.5">
            <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
              {fileWrite.append ? t("preview.file.append") : t("preview.file.content")} · {fileWrite.name}
            </div>
            <pre
              data-testid="file-write-preview"
              className="overflow-auto whitespace-pre rounded-xl border border-[var(--color-border)] bg-[var(--color-bg)] p-3 font-[var(--font-mono)] text-[12px] leading-relaxed"
              style={{ tabSize: 4 }}
            >
              {fileWrite.content}
            </pre>
          </div>
        )}
      </div>
    );
  }

  if (entry.kind === "kill-target") {
    const sig = entry.data.force ? t("preview.kill.sigkill") : t("preview.kill.sigterm");
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("preview.kill.title")}
        </div>
        <div className="rounded-xl border border-red-500/40 bg-red-500/5 p-4">
          <div className="text-[14px] font-semibold leading-snug">{entry.data.name}</div>
          <div className="mt-2 grid grid-cols-[80px_1fr] gap-x-3 gap-y-1 text-[12px]">
            <span className="text-[var(--color-muted)]">PID</span>
            <span className="font-[var(--font-mono)] tabular-nums">{entry.data.pid}</span>
            <span className="text-[var(--color-muted)]">{t("preview.kill.memory")}</span>
            <span className="font-[var(--font-mono)] tabular-nums">{entry.data.memory_mb.toFixed(2)} MB</span>
            <span className="text-[var(--color-muted)]">Signal</span>
            <span className="font-[var(--font-mono)]">{sig}</span>
            {entry.data.exe && (
              <>
                <span className="text-[var(--color-muted)]">{t("preview.kill.path")}</span>
                <span className="break-all font-[var(--font-mono)] text-[11px]">{entry.data.exe}</span>
              </>
            )}
          </div>
          <div className="mt-3 font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            {t("preview.kill.enter")}
          </div>
        </div>
      </div>
    );
  }

  if (entry.kind === "command-suggestion") {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("preview.suggestion.title")}
        </div>
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div className="font-[var(--font-mono)] text-[14px] font-semibold leading-snug">
            {entry.data.syntax}
          </div>
          <div className="mt-2 text-[12px] text-[var(--color-muted)] leading-snug">
            <InlineMd text={entry.data.description} />
          </div>
          <div className="mt-3 font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            {t("preview.suggestion.enter")}
          </div>
        </div>
      </div>
    );
  }

  if (entry.kind === "opener") {
    // Keyed on the text so each ←/→ switch remounts → replays the directional
    // slide-in (from the right for "next", from the left for "prev").
    const slide = entry.data.dir === -1 ? "md3-opener-in-left" : "md3-opener-in-right";
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("preview.opener.title")}
        </div>
        <div className="overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div key={entry.data.text} className={`text-[14px] italic leading-snug ${slide}`}>
            {entry.data.text}
          </div>
          <div className="mt-3 font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            {t("preview.opener.hint")}
          </div>
        </div>
      </div>
    );
  }

  if (entry.kind === "bruno" && entry.data.self) {
    // Selbständigen-Variante (`f`-Suffix) — eigene Aufstellung.
    const d = entry.data.self;
    const eur = new Intl.NumberFormat(locale(), {
      style: "currency", currency: "EUR", maximumFractionDigits: 0,
    });
    const eurExact = new Intl.NumberFormat(locale(), {
      style: "currency", currency: "EUR", maximumFractionDigits: 2,
    });
    const pct = new Intl.NumberFormat(locale(), {
      style: "percent", maximumFractionDigits: 1,
    });
    return (
      <div className="flex h-full flex-col gap-3 overflow-auto p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("bruno.self.title")}
        </div>
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div className="mb-2 text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
            {t("bruno.assumptions")}
          </div>
          <div className="text-[12px] leading-tight text-[var(--color-fg)]">
            {brunoSelfAssumptions(d)}
          </div>
          <div className="mt-1 text-[11px] text-[var(--color-muted)]">
            {t("bruno.adjust")}
          </div>
        </div>

        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          {d.expenses !== undefined && (
            <>
              <BrunoRow k={t("bruno.row.revenue_year")} v={eur.format(d.yearlyProfit + d.expenses)} />
              <BrunoRow k={t("bruno.row.expenses")} v={"− " + eur.format(d.expenses)} />
            </>
          )}
          <BrunoRow k={t("bruno.row.profit_year")} v={eur.format(d.yearlyProfit)} />
          <BrunoRow k={t("bruno.row.profit_month")} v={eur.format(d.yearlyProfit / 12)} />
          <BrunoRow k={t("bruno.row.health")} v={"− " + eurExact.format(d.health)} />
          {d.care > 0 && <BrunoRow k={t("bruno.row.care")} v={"− " + eurExact.format(d.care)} />}
          {d.gewerbesteuer > 0 && (
            <>
              <BrunoRow k={t("bruno.row.trade_tax")} v={"− " + eurExact.format(d.gewerbesteuer)} />
              <BrunoRow k={t("bruno.row.trade_credit")} v={"+ " + eurExact.format(d.gewerbeAnrechnung)} />
            </>
          )}
          <BrunoRow k={t("bruno.row.income_tax")} v={"− " + eurExact.format(d.incomeTax)} />
          {d.soli > 0 && <BrunoRow k={t("bruno.row.soli")} v={"− " + eurExact.format(d.soli)} />}
          {d.churchTax > 0 && <BrunoRow k={t("bruno.row.church")} v={"− " + eurExact.format(d.churchTax)} />}
          <BrunoRow k={t("bruno.row.total")} v={eur.format(d.totalDeductions)} />
          <BrunoRow k={t("bruno.row.rate")} v={pct.format(d.deductionRate)} />
          <BrunoRow k={t("bruno.row.marginal")} v={pct.format(d.marginalRate)} />
        </div>

        <div className="rounded-xl border border-[var(--color-accent)]/40 bg-[var(--color-accent)]/5 p-4">
          <BrunoRow k={t("bruno.row.net_month")} v={eurExact.format(d.netMonth)} accent animate />
          <BrunoRow k={t("bruno.row.net_year")} v={eurExact.format(d.netYear)} accent animate />
        </div>

        <BrunoExportRow data={entry.data} />

        <div className="font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
          {entry.data.period === "monthly" ? t("bruno.enter_month") : t("bruno.enter_year")}
          {" "}· {t("bruno.shift_enter")} ·{" "}
          {t("bruno.self.caveat")}
        </div>
      </div>
    );
  }

  if (entry.kind === "bruno") {
    const eur = new Intl.NumberFormat(locale(), {
      style: "currency",
      currency: "EUR",
      maximumFractionDigits: 0,
    });
    const eurExact = new Intl.NumberFormat(locale(), {
      style: "currency",
      currency: "EUR",
      maximumFractionDigits: 2,
    });
    const pct = new Intl.NumberFormat(locale(), {
      style: "percent",
      maximumFractionDigits: 1,
    });
    const d = entry.data;
    return (
      <div className="flex h-full flex-col gap-3 overflow-auto p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("bruno.employee.title")}
        </div>
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div className="mb-2 text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
            {t("bruno.assumptions")}
          </div>
          <div className="text-[12px] leading-tight text-[var(--color-fg)]">
            {t("bruno.class", { n: d.taxClass })} · {STATE_LABELS[d.state] ?? d.state.toUpperCase()} ·{" "}
            {d.children === 0 ? t("bruno.childless") : tn("bruno.children", d.children)}{" "}
            · {d.isChurchMember ? t("bruno.church_yes") : t("bruno.church_no")}
          </div>
          <div className="mt-1 text-[11px] text-[var(--color-muted)]">
            {t("bruno.adjust")}
          </div>
        </div>

        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <BrunoRow k={t("bruno.row.gross_year")} v={eur.format(d.yearlyGross)} />
          <BrunoRow k={t("bruno.row.gross_month")} v={eur.format(d.yearlyGross / 12)} />
          <BrunoRow k={t("bruno.row.health")} v={"− " + eurExact.format(d.social.health)} />
          <BrunoRow k={t("bruno.row.care")} v={"− " + eurExact.format(d.social.care)} />
          <BrunoRow k={t("bruno.row.pension")} v={"− " + eurExact.format(d.social.pension)} />
          <BrunoRow k={t("bruno.row.unemployment")} v={"− " + eurExact.format(d.social.unemployment)} />
          <BrunoRow k={t("bruno.row.income_tax")} v={"− " + eurExact.format(d.incomeTax)} />
          {d.soli > 0 && <BrunoRow k={t("bruno.row.soli")} v={"− " + eurExact.format(d.soli)} />}
          {d.churchTax > 0 && <BrunoRow k={t("bruno.row.church")} v={"− " + eurExact.format(d.churchTax)} />}
          <BrunoRow k={t("bruno.row.total")} v={eur.format(d.totalDeductions)} />
          <BrunoRow k={t("bruno.row.rate")} v={pct.format(d.deductionRate)} />
          <BrunoRow k={t("bruno.row.marginal")} v={pct.format(d.marginalRate)} />
        </div>

        <div className="rounded-xl border border-[var(--color-accent)]/40 bg-[var(--color-accent)]/5 p-4">
          <BrunoRow k={t("bruno.row.net_month")} v={eurExact.format(d.netMonth)} accent animate />
          <BrunoRow k={t("bruno.row.net_year")} v={eurExact.format(d.netYear)} accent animate />
        </div>

        <BrunoExportRow data={d} />

        <div className="font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
          {d.period === "monthly" ? t("bruno.enter_month") : t("bruno.enter_year")}
          {" "}· {t("bruno.shift_enter")} ·{" "}
          {t("bruno.employee.caveat")}
        </div>
      </div>
    );
  }

  if (entry.kind === "totp-manage") {
    const addMode = entry.data.mode === "add";
    return (
      <div className="flex h-full flex-col gap-4 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("preview.totp.title")}
        </div>
        <div className="flex flex-1 flex-col items-center justify-center gap-4 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-6 text-center">
          <Zap size={36} className="text-[var(--color-accent)] opacity-80" />
          <div className="text-[14px] font-semibold">{entry.data.label}</div>
          <div className="max-w-sm text-[12px] text-[var(--color-muted)]">
            {addMode ? (
              <>
                {tSplit("preview.totp.add_open", "key")[0]}<kbd className="rounded bg-[var(--color-bg)] px-1 font-[var(--font-mono)] text-[11px]">↩</kbd>{tSplit("preview.totp.add_open", "key")[1]}
                {entry.data.issuer ? <> ({t("preview.totp.prefilled")} <b>{entry.data.issuer}</b>)</> : null}
                {t("preview.totp.add_rest")}
              </>
            ) : (
              <>
                {tSplit("preview.totp.manage", "key")[0]}<kbd className="rounded bg-[var(--color-bg)] px-1 font-[var(--font-mono)] text-[11px]">↩</kbd>{tSplit("preview.totp.manage", "key")[1]}
              </>
            )}
          </div>
          {onTotpAdd && !addMode && (
            // Subtle secondary path to the add form — the discoverable
            // sibling of the `2fa add` sub-command (v0.104.0).
            <button
              onClick={() => onTotpAdd(entry.data.issuer)}
              className="flex items-center gap-1.5 rounded border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
            >
              <Plus size={12} />
              {t("preview.totp.add_account")}
            </button>
          )}
          <div className="text-[10px] text-[var(--color-muted)]">
            {t("preview.totp.secrets_note")}
          </div>
        </div>
      </div>
    );
  }

  if (entry.kind === "totp") {
    const e = entry.data;
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("preview.totp.meta", { digits: e.digits, period: e.period })}
        </div>
        <div className="rounded-xl border border-[var(--color-accent)]/40 bg-[var(--color-accent)]/5 p-4">
          <div className="break-all font-[var(--font-mono)] text-[36px] font-bold tracking-[0.15em] tabular-nums">
            {e.code}
          </div>
        </div>
        <div className="flex flex-col gap-1 text-[13px]">
          <div className="flex items-center gap-2">
            <TotpBrandIcon issuer={e.issuer} size={24} />
            <b>{e.issuer || t("preview.totp.no_issuer")}</b>
          </div>
          {e.account && <div className="text-[var(--color-muted)]">{e.account}</div>}
        </div>
        <div className="mt-auto text-[11px] text-[var(--color-muted)]">
          {tSplit("preview.totp.countdown", "secs")[0]}<TotpSecondsLeft period={e.period} />{tSplit("preview.totp.countdown", "secs")[1]}
        </div>
      </div>
    );
  }

  if (entry.kind === "xhype") {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-5 bg-black p-6 text-center">
        <div
          className="text-[64px] font-black leading-none"
          style={{ color: "#ff3b0f", textShadow: "0 0 34px rgba(255,59,15,.75), 0 0 90px rgba(124,58,237,.5)" }}
        >
          X!
        </div>
        <div className="text-[12px] font-semibold uppercase tracking-[0.3em] text-[var(--color-muted)]">
          {t("preview.xhype.acts")}
        </div>
        <div className="max-w-sm text-[12px] text-[var(--color-muted)]">
          {t("preview.xhype.hint")}
        </div>
      </div>
    );
  }

  if (entry.kind === "bpm") {
    return (
      <div className="flex h-full flex-col gap-4 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("preview.bpm.title")}
        </div>
        <div className="flex flex-1 flex-col items-center justify-center gap-4 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-6 text-center">
          <Zap size={36} className="text-[var(--color-accent)] opacity-80" />
          <div className="text-[14px] font-semibold">{entry.data.label}</div>
          <div className="max-w-sm text-[12px] text-[var(--color-muted)]">
            {tSplit("preview.bpm.hint", "key")[0]}<kbd className="rounded bg-[var(--color-bg)] px-1 font-[var(--font-mono)] text-[11px]">↩</kbd>{tSplit("preview.bpm.hint", "key")[1]}
          </div>
          <div className="text-[10px] text-[var(--color-muted)]">
            {t("preview.bpm.algo")}
          </div>
        </div>
      </div>
    );
  }

  if (entry.kind === "equalizer") {
    return (
      <div className="flex h-full flex-col gap-4 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("preview.eq.title")}
        </div>
        <div className="flex flex-1 flex-col items-center justify-center gap-4 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-6 text-center">
          <AudioLines size={36} className="text-[var(--color-accent)] opacity-80" />
          <div className="text-[14px] font-semibold">{entry.data.label}</div>
          <div className="max-w-sm text-[12px] text-[var(--color-muted)]">
            {tSplit("preview.eq.hint", "key")[0]}<kbd className="rounded bg-[var(--color-bg)] px-1 font-[var(--font-mono)] text-[11px]">↩</kbd>{tSplit("preview.eq.hint", "key")[1]}
          </div>
          <div className="text-[10px] text-[var(--color-muted)]">
            {t("preview.eq.keys")}
          </div>
        </div>
      </div>
    );
  }

  if (entry.kind === "pwgen") {
    const d = entry.data;
    // `digit` matches the global Cmd/Ctrl+1…4 listener in App.tsx
    // — keep the two in sync if reordering or adding modes.
    const MODES: Array<{
      key: "all" | "alnum" | "dict" | "leet";
      label: string;
      hint: string;
      digit: 1 | 2 | 3 | 4;
    }> = [
      { key: "all", label: t("pwgen.mode.all"), hint: t("pwgen.mode.all_hint"), digit: 1 },
      { key: "alnum", label: t("pwgen.mode.alnum"), hint: t("pwgen.mode.alnum_hint"), digit: 2 },
      { key: "dict", label: t("pwgen.mode.dict"), hint: t("pwgen.mode.dict_hint"), digit: 3 },
      { key: "leet", label: t("pwgen.mode.leet"), hint: t("pwgen.mode.leet_hint"), digit: 4 },
    ];
    const modPretty = IS_MAC ? "⌘" : "Ctrl+";
    const modWord = IS_MAC ? "Cmd" : "Ctrl";
    return (
      <div className="flex h-full flex-col gap-3 overflow-auto p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("pwgen.meta", { n: d.length })}
        </div>
        {/* The password itself — large, mono, and editable. Type to tweak
            it; Enter copies the edited value, Esc blurs back. */}
        <div className="rounded-xl border border-[var(--color-accent)]/40 bg-[var(--color-accent)]/5 p-3">
          <input
            type="text"
            value={d.password}
            spellCheck={false}
            autoComplete="off"
            autoCapitalize="off"
            autoCorrect="off"
            aria-label={t("pwgen.aria")}
            onChange={(e) => onPwgenEdit?.(e.target.value)}
            onFocus={() => onPwgenEditingChange?.(true)}
            onBlur={() => onPwgenEditingChange?.(false)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                onPwgenCommit?.();
              } else if (e.key === "Escape") {
                e.preventDefault();
                (e.target as HTMLInputElement).blur();
              }
            }}
            className="w-full break-all border-0 bg-transparent p-1 font-[var(--font-mono)] text-[16px] font-semibold leading-snug text-[var(--color-fg)] outline-none focus:ring-1 focus:ring-[var(--color-accent)]/50"
          />
          <div className="mt-0.5 px-1 text-[10px] text-[var(--color-muted)]">
            {t("pwgen.editable")}
          </div>
        </div>
        {/* Mode picker — radio style, currently-active highlighted.
            Each button shows its Cmd/Ctrl+digit shortcut (handled
            globally in App.tsx → "selectedPwgen" effect: switches
            mode + regenerates. Enter then copies + hides). */}
        <div className="flex flex-col gap-1.5">
          <div className="text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
            {t("pwgen.mode")}
          </div>
          <div className="grid grid-cols-2 gap-1.5">
            {MODES.map((m) => {
              const active = (pwgenMode ?? d.mode) === m.key;
              return (
                <button
                  key={m.key}
                  onClick={() => onPwgenModeChange?.(m.key)}
                  title={t("pwgen.mode_title", { key: `${modWord}+${m.digit}` })}
                  className={
                    "flex flex-col items-start rounded-md border px-2.5 py-1.5 text-left text-[12px] " +
                    (active
                      ? "border-[var(--color-accent)] bg-[var(--color-accent)]/10 text-[var(--color-fg)]"
                      : "border-[var(--color-border)] text-[var(--color-muted)] hover:bg-[var(--color-surface)]")
                  }
                >
                  <div className="flex w-full items-center justify-between gap-2">
                    <span className="font-medium">{m.label}</span>
                    <kbd className="rounded bg-[var(--color-bg)] px-1 py-px font-[var(--font-mono)] text-[9px] text-[var(--color-muted)]">
                      {modPretty}{m.digit}
                    </kbd>
                  </div>
                  <span className="text-[10px] opacity-80">{m.hint}</span>
                </button>
              );
            })}
          </div>
        </div>
        {/* Regenerate + hotkey hint */}
        <div className="flex items-center justify-between">
          <button
            onClick={() => onPwgenReroll?.()}
            className="md3-press rounded-md border border-[var(--color-border)] px-2.5 py-1 text-[12px] hover:bg-[var(--color-surface)]"
          >
            ↻ {t("pwgen.regenerate")}
          </button>
          <span className="font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            {t("pwgen.footer", { mod: modPretty })}
          </span>
        </div>
      </div>
    );
  }

  if (entry.kind === "typed-text") {
    return (
      <div className="flex h-full flex-col p-4">
        <div className="mb-2 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("preview.typed.title")}
        </div>
        <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 font-[var(--font-mono)] text-[13px] leading-5">
          {entry.data.text}
        </pre>
        <div className="mt-2 text-[11px] text-[var(--color-muted)]">
          {t("preview.typed.hint")}
        </div>
        <LazyTransformBar text={entry.data.text} initiallyOpen />
      </div>
    );
  }

  if (entry.kind === "app") {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("preview.app.title")}
        </div>
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div className="truncate text-[14px] font-semibold leading-snug">
            {entry.data.name}
          </div>
          <div className="mt-2 break-all font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            {entry.data.path}
          </div>
          <div className="mt-3 font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            {t("preview.app.hint")}
          </div>
        </div>
      </div>
    );
  }

  if (entry.kind === "settings-section") {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("preview.setting.title")}
        </div>
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
          <div className="truncate text-[14px] font-semibold leading-snug">
            {entry.data.label}
          </div>
          <div className="mt-3 font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
            {t("preview.setting.hint")}
          </div>
        </div>
      </div>
    );
  }

  if (entry.kind === "meme") {
    // Animated preview straight from disk (a GIF plays in an <img>).
    const src = convertFileSrc(entry.data.path);
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="flex items-center gap-2 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          <span>Meme</span>
          {entry.data.category && (
            <>
              <span>·</span>
              <span className="text-[var(--color-accent)]">{entry.data.category}</span>
            </>
          )}
        </div>
        <div className="flex flex-1 items-center justify-center overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]">
          <img
            src={src}
            alt={entry.data.name}
            className="max-h-full max-w-full object-contain"
          />
        </div>
        <div className="truncate text-center text-[13px] font-semibold">
          {entry.data.name}
        </div>
        <div className="text-center font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
          {t("preview.meme.hint")}
        </div>
      </div>
    );
  }

  if (entry.kind === "finder-file") {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {t("preview.finder.title")}
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
            {t("preview.finder.open")}
            {entry.data.is_image && (
              <>
                {" "}&nbsp;·&nbsp; {tSplit("preview.finder.resize", "cmd")[0]}
                <span className="font-semibold text-[var(--color-fg)]">
                  rz 1200x800
                </span>
                {tSplit("preview.finder.resize", "cmd")[1]}
              </>
            )}
          </div>
        </div>
      </div>
    );
  }

  // `?`-index rows never reach this panel (App renders CommandHelp for them),
  // but the union includes them — narrow explicitly for the clip fallthrough.
  if (entry.kind === "help") return null;

  // ── Clown style preview ────────────────────────────────────────────────────
  if (entry.kind === "clown") {
    return (
      <div className="flex h-full flex-col gap-3 p-4 text-[var(--color-fg)]">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          {entry.data.name}
        </div>
        {/* The output can be long and full of astral glyphs — let it wrap and
            stay selectable, but never let it push the panel wide. */}
        <div className="max-h-[60%] overflow-y-auto whitespace-pre-wrap break-words rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 text-[16px] leading-relaxed">
          {entry.data.output}
        </div>
        <div className="text-[12px] text-[var(--color-muted)]">{entry.data.hint}</div>
        <div className="mt-auto font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
          {t("preview.clown.hint")}
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
      <div className="ml-auto flex items-center gap-2">
        {(clip.content_type === "html" || clip.content_type === "rtf") &&
          clip.content_text && <CopyPlainButton text={clip.content_text} sourceId={clip.id} />}
        <NoteButton entry={clip} />
      </div>
    </div>
  );

  if (clip.content_type === "image") {
    return (
      <div className="flex h-full flex-col p-4">
        {meta}
        <div className="flex flex-1 items-center justify-center overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]">
          <ImagePreview entryId={clip.id} inline={clip.content_data} />
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
    // Clipboard HTML usually arrives with the source page's own colours / fonts
    // baked in as inline styles, which clash with Inspector Rust's theme. The
    // theme-aware iframe `srcDoc` (built in `htmlSrcDoc` above, memoised on the
    // entry) injects a base style + suppresses the source's colour decisions.
    return (
      <div className="flex h-full flex-col p-4">
        {meta}
        <iframe
          sandbox=""
          srcDoc={htmlSrcDoc ?? ""}
          className="flex-1 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]"
          title={t("preview.html_title")}
        />
        {/* HTML clips have a text representation in `content_text` —
            offer the same string transforms as plain-text entries. */}
        {clip.content_text && <LazyTransformBar text={clip.content_text} sourceId={clip.id} />}
      </div>
    );
  }

  if (clip.content_type === "rtf") {
    return (
      <div className="flex h-full flex-col p-4">
        {meta}
        <CappedPre
          key={clip.id}
          text={clip.content_text}
          className="flex-1 overflow-auto whitespace-pre-wrap rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 font-[var(--font-mono)] text-[12px] leading-5"
        />
        <div className="mt-2 text-[11px] text-[var(--color-muted)]">
          {t("preview.rtf_note")}
        </div>
        {/* RTF clips: same string transforms apply to the plain-text
            representation, like OCR / HTML / plain text. */}
        {clip.content_text && <LazyTransformBar text={clip.content_text} sourceId={clip.id} />}
      </div>
    );
  }

  // Default text branch — also where OCR-recognised text (saved as
  // content_type=Text) lands, so the transform bar shows there too.
  return (
    <div className="flex h-full flex-col p-4">
      {meta}
      <ClipTextBody key={clip.id} clip={clip} />
      <SmartActionsBar text={clip.content_text} />
      <ClipLinkActions text={clip.content_text} onAnalyzeRepo={onAnalyzeRepo} />
      <LazyTransformBar text={clip.content_text} sourceId={clip.id} />
    </div>
  );
}

/** Social-download + repo-analysis affordances of a text clip. The two scans
 *  (`findRepoUrl` splits the WHOLE text into tokens) are memoised on the text:
 *  run inline they repeated on every PreviewPanel render — each keystroke and
 *  arrow press — which for a multi-MB clip meant a full re-tokenisation. */
function ClipLinkActions({
  text,
  onAnalyzeRepo,
}: {
  text: string;
  onAnalyzeRepo?: (url: string) => void;
}) {
  const social = useMemo(() => detectSocial(text), [text]);
  const gh = useMemo(() => findRepoUrl(text), [text]);
  return (
    <>
      {social && <SocialDownloadBar target={social} />}
      {gh && onAnalyzeRepo && (
        <button
          type="button"
          onClick={() => onAnalyzeRepo(gh.web_url)}
          className="mt-2 flex items-center gap-1.5 self-start rounded-lg border border-[var(--color-border)] px-2 py-1 text-[12px] hover:border-[var(--color-accent)]"
        >
          <GitBranch size={12} /> {t("preview.repo.analyze", { repo: `${gh.owner}/${gh.repo}` })}
        </button>
      )}
    </>
  );
}

/** Above this many characters the text preview renders a capped slice with an
 *  explicit "show all" opt-in — WebKit lays out the WHOLE wrapped text node,
 *  so arrowing onto a multi-MB log clip froze the popup for hundreds of ms.
 *  Callers key this on the clip id, so the opt-in resets per clip. Paste and
 *  the transforms are untouched — they always use the full content. */
const PREVIEW_TEXT_CAP = 200_000;

function CappedPre({ text, className }: { text: string; className: string }) {
  const [showAll, setShowAll] = useState(false);
  if (showAll || text.length <= PREVIEW_TEXT_CAP) {
    return <pre className={className}>{text}</pre>;
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <pre className={className}>{text.slice(0, PREVIEW_TEXT_CAP)}</pre>
      <button
        onClick={() => setShowAll(true)}
        className="mt-1 self-start rounded border border-[var(--color-border)] px-2 py-0.5 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
      >
        {t("preview.show_all", { n: text.length.toLocaleString(locale()) })}
      </button>
    </div>
  );
}

/**
 * Text body of a clip. The slim list omits `content_data` for text rows (it
 * duplicates `content_text`), so this renders `content_text` directly and only
 * fetches the full row by id when `content_text` is a truncated preview
 * (`needsFullText`).
 */
function ClipTextBody({ clip }: { clip: ClipEntry }) {
  const inline = clip.content_data || clip.content_text;
  const [full, setFull] = useState<string | null>(null);
  useEffect(() => {
    if (!needsFullText(clip.content_data, clip.content_text, clip.byte_size)) return;
    let cancelled = false;
    void getClip(clip.id)
      .then((e) => {
        if (!cancelled && e && e.content_data) setFull(e.content_data);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [clip.id, clip.content_data, clip.content_text, clip.byte_size]);
  return (
    <CappedPre
      text={full ?? inline}
      className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 font-[var(--font-mono)] text-[12px] leading-5"
    />
  );
}

/**
 * Image preview that fetches the bitmap on demand. The slim history list omits
 * image blobs (they're multi-MB and the list never renders them), so a selected
 * image clip's `content_data` is empty — we pull the full payload by id via
 * `get_clip`. If `inline` data is already present (a freshly-captured entry that
 * still carries it), we use it directly and skip the round-trip. A tiny spinner
 * shows during the (single-row, fast) fetch.
 */
function ImagePreview({ entryId, inline }: { entryId: number; inline: string }) {
  const [src, setSrc] = useState<string | null>(
    inline ? `data:image/png;base64,${inline}` : null,
  );
  useEffect(() => {
    if (inline) {
      setSrc(`data:image/png;base64,${inline}`);
      return;
    }
    let cancelled = false;
    setSrc(null);
    void getClip(entryId)
      .then((e) => {
        if (!cancelled && e && e.content_data) {
          setSrc(`data:image/png;base64,${e.content_data}`);
        }
      })
      .catch(() => {
        // Row vanished / decode failed — leave the placeholder.
      });
    return () => {
      cancelled = true;
    };
  }, [entryId, inline]);

  if (!src) {
    return (
      <div className="flex items-center gap-2 text-[12px] text-[var(--color-muted)]">
        <Loader2 size={14} className="animate-spin" />
        {t("preview.loading_image")}
      </div>
    );
  }
  return (
    <img src={src} alt={t("preview.image_alt")} className="max-h-full max-w-full object-contain" />
  );
}

/** Download buttons for a detected social-media URL — video (all platforms) +
 *  audio (YouTube only). Used by the `social` preview and by clip URLs. */
/** A "schöne Animation" shown while a social-media download runs: bobbing
 *  download glyph inside two expanding MD3 accent rings, over a scrolling
 *  wavy indeterminate-progress line. Respects prefers-reduced-motion (CSS). */
function DownloadAnimation({ label }: { label: string }) {
  return (
    <div className="md3-banner-in flex flex-col items-center justify-center gap-5 py-9">
      <div className="relative flex h-16 w-16 items-center justify-center">
        <span className="md3-dl-ring" />
        <span className="md3-dl-ring" style={{ animationDelay: "0.9s" }} />
        <Download size={30} className="md3-dl-icon text-[var(--color-accent)]" />
      </div>
      <div className="text-[12px] font-medium text-[var(--color-fg)]">{label}</div>
      <div className="h-4 w-48 overflow-hidden">
        <svg viewBox="0 0 96 16" className="md3-dl-wave anim-keep h-full w-[200%]" preserveAspectRatio="none">
          <path
            d="M0 8 Q 6 2, 12 8 T 24 8 T 36 8 T 48 8 T 60 8 T 72 8 T 84 8 T 96 8"
            fill="none"
            stroke="var(--color-accent)"
            strokeWidth="2"
            strokeLinecap="round"
          />
        </svg>
      </div>
    </div>
  );
}

/**
 * Download bar for a detected social URL.
 *
 * When the row is the selected `social` entry, App.tsx drives it as a
 * **controlled** component: `mode` is the Tab-selected target (video/audio,
 * highlighted), `onModeChange` reports button clicks back up, and bumping
 * `runSignal` triggers a download of the current `mode` from the global Enter
 * key — so the keyboard path shows the same in-bar progress animation as a
 * click. In the clip-preview path the controlled props are omitted and the bar
 * works standalone (video is the visual default; YouTube also offers audio).
 */
function SocialDownloadBar({
  target,
  mode,
  onModeChange,
  runSignal,
}: {
  target: SocialTarget;
  mode?: "video" | "audio";
  onModeChange?: (m: "video" | "audio") => void;
  runSignal?: number;
}) {
  const [available, setAvailable] = useState(true);
  // ⚠️ Trim is OPT-IN. While the section is closed, `sectionFor` returns
  // undefined and the download is exactly what it was before this existed.
  const [trimOpen, setTrimOpen] = useState(false);
  const meta = useMeta(target.url);
  const durationS = meta?.state === "ok" ? (meta.meta.duration_s ?? 0) : 0;
  const [range, setRange] = useState<Range>({ start: 0, end: 0 });
  useEffect(() => {
    setRange(fullRange(durationS));
  }, [target.url, durationS]);
  useEffect(() => {
    setTrimOpen(false);
  }, [target.url]);
  const [busy, setBusy] = useState<null | "video" | "audio">(null);
  const [done, setDone] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void socialYtdlpAvailable().then(setAvailable).catch(() => setAvailable(false));
  }, []);

  const run = async (m: "video" | "audio") => {
    if (busy) return;
    setBusy(m);
    setError(null);
    setDone(null);
    try {
      const out = await socialDownload(
        target.url,
        m,
        true,
        sectionFor(range, durationS, trimOpen),
      );
      setDone(out.split(/[/\\]/).pop() || out);
    } catch (e) {
      const msg = String(e);
      setError(msg.includes("no_ytdlp") ? t("social.no_ytdlp") : t("common.failed_detail", { detail: msg }));
    } finally {
      setBusy(null);
    }
  };

  // Global Enter (handled in App.tsx) bumps `runSignal` → download the
  // Tab-selected mode here, so the progress animation shows.
  const prevSignal = useRef(runSignal ?? 0);
  useEffect(() => {
    if (runSignal === undefined || runSignal === prevSignal.current) return;
    prevSignal.current = runSignal;
    void run(mode ?? "video");
    // `run` is stable enough for this fire-on-signal; mode read at call time.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runSignal]);

  if (!available) {
    return (
      <div className="mt-3 text-[12px] text-[var(--color-muted)]">
        {tSplit("social.install_hint", "cmd", { platform: platformLabel(target.platform) })[0]}<code className="rounded bg-[var(--color-surface)] px-1">brew install yt-dlp</code>{tSplit("social.install_hint", "cmd", { platform: platformLabel(target.platform) })[1]}
      </div>
    );
  }
  const isYt = target.platform === "youtube";
  if (busy) {
    return (
      <DownloadAnimation
        label={t(busy === "audio" ? "social.downloading_audio" : "social.downloading_video", { platform: platformLabel(target.platform) })}
      />
    );
  }
  // Highlight the active (Tab-selected) button when controlled; otherwise video
  // keeps the standalone "primary" look.
  const controlled = mode !== undefined;
  const videoActive = controlled ? mode === "video" : true;
  const audioActive = controlled ? mode === "audio" : false;
  const accentBtn =
    "md3-press flex flex-1 items-center justify-center gap-1.5 rounded bg-[var(--color-accent)] px-3 py-2 text-[12px] font-medium text-[var(--color-accent-fg)]";
  const outlineBtn =
    "md3-press flex flex-1 items-center justify-center gap-1.5 rounded border border-[var(--color-border)] px-3 py-2 text-[12px] hover:border-[var(--color-accent)]";
  return (
    <div className="mt-3 flex flex-col gap-2">
      <div className="flex gap-2">
        <button
          onClick={() => {
            onModeChange?.("video");
            void run("video");
          }}
          className={videoActive ? accentBtn : outlineBtn}
        >
          <Download size={13} />
          {t("social.download_video")}
        </button>
        {isYt && (
          <button
            onClick={() => {
              onModeChange?.("audio");
              void run("audio");
            }}
            className={audioActive ? accentBtn : outlineBtn}
          >
            <Music size={13} />
            {t("social.download_audio")}
          </button>
        )}
      </div>
      {isYt && controlled && (
        <div className="font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
          {t("social.tab_hint")}
        </div>
      )}

      {/* Trim — optional, below the buttons. Closed, nothing about the download
          changes; the duration comes from the metadata already fetched above. */}
      {durationS > 0 && (
        <div className="mt-1 border-t border-[var(--color-border)] pt-2">
          <button
            type="button"
            onClick={() => setTrimOpen((v) => !v)}
            className="md3-press flex w-full items-center gap-1.5 text-[11px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            aria-expanded={trimOpen}
          >
            <Scissors size={11} />
            {t("social.trim")}
            {!trimOpen && <span className="opacity-70">{t("social.trim_hint")}</span>}
            {trimOpen && !isFullRange(range, durationS) && (
              <span className="font-[var(--font-mono)] text-[var(--color-accent)]">
                {fmtClock(range.start, durationS >= 3600)}–{fmtClock(range.end, durationS >= 3600)}
              </span>
            )}
            <span className="ml-auto">{trimOpen ? "▾" : "▸"}</span>
          </button>
          {trimOpen && (
            <div className="mt-2">
              <TrimBar
                url={target.url}
                duration={durationS}
                range={range}
                onRange={setRange}
              />
            </div>
          )}
        </div>
      )}
      {error && <div className="text-[11px] text-rose-400">{error}</div>}
      {done && (
        <div className="md3-banner-in flex items-center gap-1 text-[11px] text-emerald-400">
          <Check size={12} /> {tSplit("social.saved", "file")[0]}<b>{done}</b>{tSplit("social.saved", "file")[1]}
        </div>
      )}
    </div>
  );
}

const SMART_ICON: Record<SmartActionKind, typeof ExternalLink> = {
  "open-url": ExternalLink,
  email: Mail,
  call: Phone,
  maps: MapPin,
  qr: QrCode,
};

/** Button text per action — `SmartAction.label` is the English fallback the
 *  pure detector carries; the UI shows the translation. */
const SMART_LABEL: Record<SmartActionKind, MessageKey> = {
  "open-url": "smart.open_url",
  email: "smart.email",
  call: "smart.call",
  maps: "smart.maps",
  qr: "smart.qr",
};

/** Copy the clip's plain-text representation to the clipboard (for rich clips
 *  like HTML/RTF). Goes through `commit_transformed_text` so the write is
 *  marked self-write + lands in history as a plain-text entry. */
function CopyPlainButton({ text, sourceId }: { text: string; sourceId?: number }) {
  const [done, setDone] = useState(false);
  const doneTimer = useRef<number | null>(null);
  const copy = () => {
    void commitTransformedText(text, sourceId, "plain-text").catch(() => undefined);
    setDone(true);
    if (doneTimer.current !== null) window.clearTimeout(doneTimer.current);
    doneTimer.current = window.setTimeout(() => setDone(false), 1500);
  };
  return (
    <button
      type="button"
      onClick={copy}
      title={t("preview.copy_plain_title")}
      className="flex items-center gap-1 rounded-full border border-[var(--color-border)] px-2 py-0.5 text-[11px] font-medium text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
    >
      {done ? (
        <span className="md3-success-pop items-center gap-1">
          <Check size={12} /> {t("common.copied")}
        </span>
      ) : (
        <>
          <Copy size={12} /> {t("preview.copy_text")}
        </>
      )}
    </button>
  );
}

/** Note button (top-right of the preview): add a note, or — when one exists —
 *  show it highlighted with Edit / Delete. The list row turns yellow whenever a
 *  note is set; clearing it removes the highlight. The note text round-trips via
 *  `set_clip_note`; the backend's `clipboard-changed` event refreshes the entry. */
function NoteButton({ entry }: { entry: ClipEntry }) {
  const note = entry.note ?? "";
  const has = note.length > 0;
  const [open, setOpen] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  const openMenu = () => {
    setDraft(note);
    setEditing(!has); // no note yet → straight to the editor
    setOpen((o) => !o);
  };
  const save = () => {
    void setClipNote(entry.id, draft.trim()).catch(() => undefined);
    setOpen(false);
  };
  const remove = () => {
    void setClipNote(entry.id, "").catch(() => undefined);
    setOpen(false);
  };

  return (
    <span className="relative normal-case">
      <button
        type="button"
        onClick={openMenu}
        title={has ? t("note.title") : t("note.add")}
        className={
          "flex items-center gap-1 rounded-full border px-2 py-0.5 text-[11px] font-medium " +
          (has
            ? "border-amber-400/50 bg-amber-400/15 text-amber-500"
            : "border-[var(--color-border)] text-[var(--color-muted)] hover:bg-[var(--color-surface)]")
        }
      >
        <StickyNote size={12} fill={has ? "currentColor" : "none"} />
        {has ? t("note.title") : t("note.add")}
      </button>
      {open && (
        <div className="pop-enter absolute right-0 top-full z-20 mt-1 w-64 rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] p-2 text-[12px] shadow-lg">
          {has && !editing ? (
            <>
              <p className="mb-2 max-h-32 overflow-auto whitespace-pre-wrap break-words text-[var(--color-fg)]">
                {note}
              </p>
              <div className="flex items-center justify-end gap-1.5">
                <button
                  type="button"
                  onClick={remove}
                  className="md3-press rounded-full border border-rose-500/40 px-2.5 py-1 text-rose-400 hover:bg-rose-500/10"
                >
                  {t("common.delete")}
                </button>
                <button
                  type="button"
                  onClick={() => setEditing(true)}
                  className="md3-press rounded-full bg-[var(--color-accent)] px-3 py-1 font-semibold text-[var(--color-accent-fg)]"
                >
                  {t("common.edit")}
                </button>
              </div>
            </>
          ) : (
            <>
              <textarea
                autoFocus
                value={draft}
                placeholder={t("note.placeholder")}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) save();
                  else if (e.key === "Escape") setOpen(false);
                }}
                rows={3}
                className="mb-2 w-full rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1.5 py-1"
              />
              <div className="flex items-center justify-end gap-1.5">
                <button
                  type="button"
                  onClick={() => setOpen(false)}
                  className="md3-press rounded-full border border-[var(--color-border)] px-2.5 py-1"
                >
                  {t("common.cancel")}
                </button>
                <button
                  type="button"
                  onClick={save}
                  disabled={draft.trim() === ""}
                  className="md3-press rounded-full bg-[var(--color-accent)] px-3 py-1 font-semibold text-[var(--color-accent-fg)] disabled:opacity-40"
                >
                  {t("common.save")}
                </button>
              </div>
            </>
          )}
        </div>
      )}
    </span>
  );
}

/** Row of one-tap actions detected from a text clip (open link, compose email,
 *  call, open Maps, make QR). Only renders when something is detected. */
function SmartActionsBar({ text }: { text: string }) {
  const actions = useMemo(() => detectSmartActions(text), [text]);
  if (actions.length === 0) return null;
  const run = (kind: SmartActionKind, href: string) => {
    if (kind === "qr") {
      try {
        void qrCopyPng(qrPngBase64(href), href.length > 48 ? `${href.slice(0, 48)}…` : href);
      } catch {
        /* unencodable — ignore */
      }
      return;
    }
    void openUrl(href).catch(() => {});
  };
  return (
    <div className="mt-2 flex flex-wrap gap-1.5">
      {actions.map((a) => {
        const Icon = SMART_ICON[a.kind];
        return (
          <button
            key={`${a.kind}-${a.href}`}
            onClick={() => run(a.kind, a.href)}
            className="md3-press flex items-center gap-1.5 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[11px] text-[var(--color-fg)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
          >
            <Icon size={12} />
            {t(SMART_LABEL[a.kind])}
          </button>
        );
      })}
    </div>
  );
}

function CopyButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      title={t("common.copy")}
      className="md3-press rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]"
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
        title={t("cutout.title")}
        className={
          "md3-press flex items-center gap-1.5 rounded px-2 py-1 text-[11px] font-medium " +
          (busy
            ? "cursor-wait bg-[var(--color-bg)] text-[var(--color-muted)]"
            : "bg-[var(--color-accent)] text-[var(--color-accent-fg)] hover:opacity-90")
        }
      >
        <Scissors size={12} />
        {busy ? t("cutout.busy") : t("cutout.label")}
      </button>
      <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-muted)]">
        ⌘B
      </kbd>
      {savedTo && (
        <span className="md3-success-pop ml-auto flex items-center gap-1 truncate text-[11px] text-emerald-400" title={savedTo}>
          <Check size={11} />
          {t("common.saved")} <span className="font-[var(--font-mono)]">{filename}</span>
        </span>
      )}
      {error && (
        <span className="ml-auto text-[11px] text-red-400" title={error}>
          {t("common.failed_short")}
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
        title={t("saveimg.title")}
        className={
          "md3-press flex items-center gap-1.5 rounded px-2 py-1 text-[11px] font-medium " +
          (busy
            ? "cursor-wait bg-[var(--color-bg)] text-[var(--color-muted)]"
            : "bg-[var(--color-accent)] text-[var(--color-accent-fg)] hover:opacity-90")
        }
      >
        <Download size={12} />
        {busy ? t("common.saving") : t("saveimg.label")}
      </button>
      <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-muted)]">
        ⌘S
      </kbd>
      {savedTo && (
        <span className="md3-success-pop ml-auto flex items-center gap-1 truncate text-[11px] text-emerald-400" title={savedTo}>
          <Check size={11} />
          {t("common.saved")} <span className="font-[var(--font-mono)]">{filename}</span>
        </span>
      )}
      {error && (
        <span className="ml-auto text-[11px] text-red-400" title={error}>
          {t("common.failed_short")}
        </span>
      )}
    </div>
  );
}

// ── Recolor toolbar (image entries only) ────────────────────────────────────

const PRESETS: Array<{ hex: string; labelKey: MessageKey }> = [
  { hex: "#CE422B", labelKey: "recolor.rust" },
  { hex: "#DC2626", labelKey: "recolor.red" },
  { hex: "#16A34A", labelKey: "recolor.green" },
  { hex: "#2563EB", labelKey: "recolor.blue" },
  { hex: "#9333EA", labelKey: "recolor.purple" },
  { hex: "#F59E0B", labelKey: "recolor.amber" },
  { hex: "#0891B2", labelKey: "recolor.cyan" },
  { hex: "#6B7280", labelKey: "recolor.gray" },
  { hex: "#000000", labelKey: "recolor.black" },
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
        {t("recolor.title")}
      </span>
      <div className="flex flex-wrap items-center gap-1">
        {PRESETS.map((p) => (
          <button
            key={p.hex}
            onClick={() => void apply(p.hex)}
            disabled={busy}
            title={`${t(p.labelKey)} (${p.hex})`}
            className="md3-press h-5 w-5 shrink-0 rounded border border-[var(--color-border)] disabled:opacity-50"
            style={{ backgroundColor: p.hex }}
            aria-label={t("recolor.aria", { color: t(p.labelKey) })}
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
          {t("common.failed_short")}
        </span>
      )}
    </div>
  );
}

const TransformBarLazy = lazy(() => import("./TransformBar").then((m) => ({ default: m.TransformBar })));

/** The transform chips load on first use — they only exist for text entries,
 *  and this file is part of the start-up bundle. */
function LazyTransformBar(props: { text: string; sourceId?: number; initiallyOpen?: boolean }) {
  return (
    <Suspense fallback={null}>
      <TransformBarLazy {...props} />
    </Suspense>
  );
}
