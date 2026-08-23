import { useCallback, useEffect, useRef, useState } from "react";
import { Code2, FileCode2, RefreshCw } from "lucide-react";
import { locCount, type LocReport } from "../lib/ipc";
import { languageColor, donutSegments, formatCount, formatPct } from "../lib/loc";

/**
 * `loc` — lines-of-code statistics in the right preview column (v0.117.0).
 * Enter-activated (tokei walks a whole directory tree — never while typing).
 * Bare `loc` counts the live Finder selection; `loc <pfad>` an explicit path.
 * Charts: a GitHub-style stacked share bar, a donut with legend, and a
 * per-language table (files / code / comments / blanks — comments include
 * documentation, Python docstrings deliberately count as comments).
 *
 * The ignore toggle re-runs the count: default respects `.gitignore` (inside
 * git repos — a plain folder's .gitignore is inert, see loc.rs) and skips
 * hidden files; off counts EVERYTHING (vendored trees, node_modules).
 */
export function LocPanel({
  arg,
  focused,
  onExit,
}: {
  /** Optional explicit path typed after `loc `. Blank = Finder selection. */
  arg: string;
  focused: boolean;
  onExit: () => void;
}) {
  const [report, setReport] = useState<LocReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [respectIgnores, setRespectIgnores] = useState(true);
  const aliveRef = useRef(true);
  const seqRef = useRef(0);
  const scrollRef = useRef<HTMLDivElement>(null);

  const run = useCallback(
    (ignores: boolean) => {
      const seq = ++seqRef.current;
      setBusy(true);
      setError(null);
      const trimmed = arg.trim();
      const paths = trimmed
        ? [trimmed.startsWith("~/") ? trimmed : trimmed] // backend rejects missing paths clearly
        : null;
      locCount(paths, ignores)
        .then((r) => {
          if (!aliveRef.current || seq !== seqRef.current) return;
          setReport(r);
          setBusy(false);
        })
        .catch((e) => {
          if (!aliveRef.current || seq !== seqRef.current) return;
          setError(String(e));
          setBusy(false);
        });
    },
    [arg],
  );

  useEffect(() => {
    aliveRef.current = true;
    run(respectIgnores);
    return () => {
      aliveRef.current = false;
    };
    // Re-run on arg change only — the toggle triggers its own run.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [run]);

  // Keyboard: Esc exits, R refreshes, ↑/↓ scroll — but never while the user
  // is typing in the search field (the weather lesson: a window-capture
  // handler must not eat keys destined for a text input).
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      const tgt = e.target as HTMLElement | null;
      const typing =
        tgt && (tgt.tagName === "INPUT" || tgt.tagName === "TEXTAREA" || tgt.isContentEditable);
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
        return;
      }
      if (typing) return;
      const el = scrollRef.current;
      if (e.key === "r" || e.key === "R") {
        e.preventDefault();
        run(respectIgnores);
      } else if (e.key === "ArrowDown" && el) {
        e.preventDefault();
        el.scrollBy({ top: 64 });
      } else if (e.key === "ArrowUp" && el) {
        e.preventDefault();
        el.scrollBy({ top: -64 });
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit, run, respectIgnores]);

  const toggleIgnores = () => {
    const next = !respectIgnores;
    setRespectIgnores(next);
    run(next);
  };

  return (
    <div
      ref={scrollRef}
      className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]"
    >
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <Code2 size={15} className="text-[var(--color-accent)]" />
          Lines of code
          {report && (
            <span className="truncate text-[var(--color-muted)]">· {report.root_label}</span>
          )}
        </div>
        <button
          type="button"
          onClick={() => run(respectIgnores)}
          title="Neu zählen (R)"
          className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
        >
          <RefreshCw size={13} className={busy ? "animate-spin" : undefined} />
        </button>
      </div>

      {error ? (
        <ErrorCard error={error} />
      ) : report === null ? (
        <p className="text-[12px] text-[var(--color-muted)]">Zähle…</p>
      ) : (
        <>
          <TotalsRow r={report} />
          <ShareBar r={report} />
          <DonutCard r={report} />
          <LanguageTable r={report} />
          {report.inaccurate && (
            <p className="text-[11px] text-amber-500">
              ⚠️ Einzelne Dateien konnten nicht sauber geparst werden — Zahlen dort geschätzt.
            </p>
          )}
          <label className="flex cursor-pointer items-center gap-2 text-[11px] text-[var(--color-muted)]">
            <input
              type="checkbox"
              checked={respectIgnores}
              onChange={toggleIgnores}
              className="accent-[var(--color-accent)]"
            />
            .gitignore beachten &amp; versteckte Dateien überspringen
          </label>
        </>
      )}
      {focused && (
        <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">
          R neu zählen · ↑ ↓ scrollen · Esc schließen
        </p>
      )}
    </div>
  );
}

function ErrorCard({ error }: { error: string }) {
  const noSelection = error.includes("loc.no_selection");
  const automation = error.includes("finder.automation_denied");
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-4">
      <p className="flex items-center gap-2 text-[12px]">
        <FileCode2 size={14} className="text-[var(--color-accent)]" />
        {noSelection
          ? "Nichts ausgewählt."
          : automation
            ? "Kein Zugriff auf die Finder-Auswahl."
            : "Zählen fehlgeschlagen."}
      </p>
      <p className="mt-1 text-[11px] leading-snug text-[var(--color-muted)]">
        {noSelection
          ? "Markiere einen Ordner im Finder und drücke Enter erneut — oder tippe den Pfad direkt: loc ~/claude/projekt"
          : automation
            ? "macOS braucht die Automation-Berechtigung für den Finder (System Settings → Privacy → Automation) — oder nutze loc <pfad>."
            : error}
      </p>
    </div>
  );
}

function TotalsRow({ r }: { r: LocReport }) {
  const cells: Array<[string, string]> = [
    ["Dateien", formatCount(r.total_files)],
    ["Zeilen", formatCount(r.total_lines)],
    ["Code", formatCount(r.total_code)],
    ["Kommentare", formatCount(r.total_comments)],
    ["Leer", formatCount(r.total_blanks)],
  ];
  return (
    <div className="grid grid-cols-5 gap-1.5">
      {cells.map(([label, value]) => (
        <div
          key={label}
          className="rounded-lg border border-[var(--color-border)] px-1.5 py-1.5 text-center"
        >
          <div className="text-[13px] font-semibold tabular-nums">{value}</div>
          <div className="text-[10px] text-[var(--color-muted)]">{label}</div>
        </div>
      ))}
    </div>
  );
}

/** GitHub-style stacked language bar (share of code lines). */
function ShareBar({ r }: { r: LocReport }) {
  if (r.total_code === 0) return null;
  return (
    <div className="flex h-2.5 w-full overflow-hidden rounded-full">
      {r.languages
        .filter((l) => l.code_pct > 0.3)
        .map((l) => (
          <div
            key={l.name}
            title={`${l.name} · ${formatPct(l.code_pct)}`}
            style={{ width: `${l.code_pct}%`, backgroundColor: languageColor(l.name) }}
          />
        ))}
    </div>
  );
}

function DonutCard({ r }: { r: LocReport }) {
  const segs = donutSegments(
    r.languages.map((l) => ({ name: l.name, pct: l.code_pct })),
    { cx: 60, cy: 60, rOuter: 54, rInner: 34 },
  );
  if (segs.length === 0) return null;
  return (
    <div className="flex items-center gap-4 rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
      <svg viewBox="0 0 120 120" className="h-28 w-28 shrink-0">
        {segs.map((s) => (
          <path key={s.name} d={s.d} fill={s.color}>
            <title>
              {s.name} · {formatPct(s.pct)}
            </title>
          </path>
        ))}
      </svg>
      <div className="flex min-w-0 flex-col gap-1">
        {segs.map((s) => (
          <div key={s.name} className="flex items-center gap-1.5 text-[11px]">
            <span
              className="h-2.5 w-2.5 shrink-0 rounded-sm"
              style={{ backgroundColor: s.color }}
            />
            <span className="truncate">{s.name}</span>
            <span className="ml-auto tabular-nums text-[var(--color-muted)]">
              {formatPct(s.pct)}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function LanguageTable({ r }: { r: LocReport }) {
  if (r.languages.length === 0) {
    return (
      <p className="text-[12px] text-[var(--color-muted)]">
        Keine erkennbaren Quelldateien gefunden.
      </p>
    );
  }
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
      <div className="grid grid-cols-[minmax(0,1fr)_auto_auto_auto_auto] items-center gap-x-3 gap-y-1 text-[11px] tabular-nums">
        <span className="text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
          Sprache
        </span>
        <span className="text-right text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
          Dateien
        </span>
        <span className="text-right text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
          Code
        </span>
        <span className="text-right text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
          Komm.
        </span>
        <span className="text-right text-[10px] uppercase tracking-wider text-[var(--color-muted)]">
          Leer
        </span>
        {r.languages.map((l) => (
          <LanguageRow key={l.name} l={l} />
        ))}
      </div>
    </div>
  );
}

function LanguageRow({ l }: { l: import("../lib/ipc").LocLanguage }) {
  return (
    <>
      <span className="flex min-w-0 items-center gap-1.5">
        <span
          className="h-2.5 w-2.5 shrink-0 rounded-sm"
          style={{ backgroundColor: languageColor(l.name) }}
        />
        <span className="truncate">{l.name}</span>
        <span className="ml-1 text-[10px] text-[var(--color-muted)]">{formatPct(l.code_pct)}</span>
      </span>
      <span className="text-right text-[var(--color-muted)]">{formatCount(l.files)}</span>
      <span className="text-right">{formatCount(l.code)}</span>
      <span className="text-right text-[var(--color-muted)]">{formatCount(l.comments)}</span>
      <span className="text-right text-[var(--color-muted)]">{formatCount(l.blanks)}</span>
    </>
  );
}
