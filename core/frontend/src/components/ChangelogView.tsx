import { useEffect, useMemo, useRef, useState } from "react";
import { InlineMd } from "./InlineMd";
import { useT } from "../lib/i18n";
import {
  CHANGELOG_FIRST,
  CHANGELOG_MORE,
  filterReleases,
  parseChangelog,
  type ChangelogBlock,
  type ChangelogRelease,
} from "../lib/changelog";

/**
 * The full CHANGELOG.md, in English only (the file is English; UI chrome
 * around it follows the app language). The ~700 KB text is its own chunk and
 * loads when the section scrolls into view, so opening Settings costs nothing.
 */
let loaded: Promise<ChangelogRelease[]> | null = null;
function loadChangelog(): Promise<ChangelogRelease[]> {
  loaded ??= import("../../../../CHANGELOG.md?raw").then((m) => parseChangelog(m.default));
  return loaded;
}

export function ChangelogView({ currentVersion }: { currentVersion?: string | null }) {
  const t = useT();
  const root = useRef<HTMLDivElement | null>(null);
  const [releases, setReleases] = useState<ChangelogRelease[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [shown, setShown] = useState(CHANGELOG_FIRST);
  const [query, setQuery] = useState("");

  useEffect(() => {
    const el = root.current;
    let cancelled = false;
    const start = () => {
      loadChangelog().then(
        (r) => !cancelled && setReleases(r),
        () => !cancelled && setFailed(true),
      );
    };
    if (!el || typeof IntersectionObserver === "undefined") {
      start();
      return () => {
        cancelled = true;
      };
    }
    const io = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        io.disconnect();
        start();
      }
    });
    io.observe(el);
    return () => {
      cancelled = true;
      io.disconnect();
    };
  }, []);

  const filtered = useMemo(() => (releases ? filterReleases(releases, query) : []), [releases, query]);
  const visible = filtered.slice(0, shown);

  return (
    <div ref={root} className="space-y-3" data-testid="changelog">
      {releases === null && !failed && (
        <p className="text-xs text-[var(--color-muted)]">{t("set.changelog.loading")}</p>
      )}
      {failed && <p className="text-xs text-red-500">{t("set.changelog.failed")}</p>}
      {releases && (
        <>
          <input
            type="search"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setShown(CHANGELOG_FIRST);
            }}
            placeholder={t("set.changelog.search")}
            aria-label={t("set.changelog.search")}
            className="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-xs"
          />
          <p className="text-[11px] text-[var(--color-muted)]">
            {t("set.changelog.count", { shown: Math.min(shown, filtered.length), total: filtered.length })}
          </p>
          {visible.map((r) => (
            <Release key={r.version} release={r} current={r.version === currentVersion} />
          ))}
          {filtered.length > shown && (
            <button
              type="button"
              onClick={() => setShown((n) => n + CHANGELOG_MORE)}
              className="rounded border border-[var(--color-border)] px-2.5 py-1 text-xs hover:bg-[var(--color-surface)]"
            >
              {t("set.changelog.older")}
            </button>
          )}
        </>
      )}
    </div>
  );
}

function Release({ release, current }: { release: ChangelogRelease; current: boolean }) {
  const t = useT();
  return (
    <article lang="en" className="rounded border border-[var(--color-border)] p-3" data-testid="changelog-release">
      <header className="mb-2 flex items-baseline gap-2">
        <h3 className="text-sm font-semibold">{release.version}</h3>
        {release.date && <span className="text-[11px] text-[var(--color-muted)]">{release.date}</span>}
        {current && (
          <span className="rounded bg-[var(--color-accent)] px-1.5 py-0.5 text-[10px] font-semibold text-[var(--color-accent-fg)]">
            {t("set.changelog.installed")}
          </span>
        )}
      </header>
      <div className="space-y-1.5 text-xs leading-relaxed">
        {release.blocks.map((b, i) => (
          <Block key={i} block={b} />
        ))}
      </div>
    </article>
  );
}

function Block({ block }: { block: ChangelogBlock }) {
  switch (block.kind) {
    case "heading":
      return <h4 className="pt-1 text-[11px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">{block.text}</h4>;
    case "item":
      return (
        <div className="flex gap-1.5" style={{ paddingLeft: `${block.depth * 14}px` }}>
          <span aria-hidden className="text-[var(--color-muted)]">{block.ordered ? "›" : "•"}</span>
          {/* One flow box: InlineMd yields several nodes, which would each become a flex column. */}
          <span className="min-w-0">
            <InlineMd text={block.text} />
          </span>
        </div>
      );
    case "para":
      return (
        <p>
          <InlineMd text={block.text} />
        </p>
      );
    case "code":
      return (
        <pre className="overflow-x-auto rounded bg-[var(--color-surface)] p-2 font-[var(--font-mono)] text-[11px]">{block.text}</pre>
      );
    case "table":
      return (
        <div className="overflow-x-auto">
          <table className="text-[11px]">
            <tbody>
              {block.rows.map((row, r) => (
                <tr key={r} className={r === 0 ? "font-semibold" : undefined}>
                  {row.map((cell, c) => (
                    <td key={c} className="border-b border-[var(--color-border)] px-2 py-0.5">
                      <InlineMd text={cell} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
  }
}
