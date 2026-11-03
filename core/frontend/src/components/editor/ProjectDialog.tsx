import { useEffect, useMemo, useRef, useState } from "react";
import { FolderOpen, X } from "lucide-react";
import { recordListProjects, recordOpenDialog, type RecordProjectItem } from "../../lib/ipc";
import { formatTimecode } from "../../lib/editor";
import { locale, useT } from "../../lib/i18n";
import { filterProjects, isOpenProject, lastChange } from "../../lib/record-projects";
import { ProjectThumb } from "../ProjectThumb";

/** A project the editor can switch to: finished (or incomplete with video), not recording. */
export const openable = (p: RecordProjectItem) => !p.active && p.status !== "failed";

/**
 * ⌘O in the editor: the same project list as the `record` panel (most recently edited first,
 * thumbnails, search by name or date) as a dialog. Picking one switches the editor to it —
 * the caller saves pending edits first. "Open other folder…" takes any project folder.
 */
export function ProjectDialog({
  currentDir,
  onOpen,
  onClose,
  onError,
}: {
  currentDir: string;
  onOpen: (dir: string) => void;
  onClose: () => void;
  onError: (e: unknown) => void;
}) {
  const t = useT();
  const [items, setItems] = useState<RecordProjectItem[] | null>(null);
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => {
    let dead = false;
    recordListProjects()
      .then((l) => !dead && setItems(l))
      .catch((e) => {
        if (dead) return;
        setItems([]);
        onError(e);
      });
    return () => {
      dead = true;
    };
  }, [onError]);

  const list = useMemo(() => filterProjects(items ?? [], query, locale()), [items, query]);
  const selIdx = Math.min(sel, Math.max(0, list.length - 1));

  const choose = (p: RecordProjectItem | undefined) => {
    if (!p || !openable(p)) return;
    if (isOpenProject(p, currentDir)) onClose();
    else onOpen(p.dir);
  };
  const other = () => {
    recordOpenDialog()
      .then((dir) => {
        if (dir) onOpen(dir);
      })
      .catch(onError);
  };

  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-idx="${selIdx}"]`)?.scrollIntoView?.({ block: "nearest" });
  }, [selIdx]);

  // The dialog owns the keys (capture phase): neither the editor's shortcuts nor the
  // popup's Esc fallback see them.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        e.stopPropagation();
        setSel(Math.max(0, Math.min(list.length - 1, selIdx + (e.key === "ArrowDown" ? 1 : -1))));
      } else if (e.key === "Enter") {
        e.preventDefault();
        e.stopPropagation();
        choose(list[selIdx]);
      } else {
        // Typing goes to the search field, nothing else in the editor reacts.
        e.stopPropagation();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  return (
    <div className="md3-overlay-in fixed inset-0 z-30 flex items-center justify-center bg-black/50" onPointerDown={onClose}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("rec4.projects.title")}
        data-testid="ed-projects"
        className="md3-pop-in flex max-h-[80vh] w-[460px] flex-col rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 text-[var(--color-fg)] shadow-2xl"
        onPointerDown={(e) => e.stopPropagation()}
      >
        <div className="mb-2 flex items-center justify-between">
          <h2 className="text-[14px] font-semibold">{t("rec4.projects.title")}</h2>
          <button onClick={onClose} aria-label={t("rec2.ed.close")} className="rounded p-1 hover:bg-[var(--color-bg)]">
            <X size={14} />
          </button>
        </div>
        <input
          autoFocus
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setSel(0);
          }}
          placeholder={t("rec4.projects.search")}
          aria-label={t("rec4.projects.search")}
          className="mb-2 rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px] outline-none focus:border-[var(--color-accent)]"
        />
        {items === null ? (
          <p className="px-2 py-2 text-[12px] text-[var(--color-muted)]">{t("rec4.take.loading")}</p>
        ) : list.length === 0 ? (
          <p className="px-2 py-2 text-[12px] text-[var(--color-muted)]">
            {items.length > 0 && query.trim() ? t("rec4.projects.no_match", { query: query.trim() }) : t("rec2.panel.empty")}
          </p>
        ) : (
          <ul ref={listRef} className="min-h-0 flex-1 overflow-y-auto">
            {list.map((p, i) => {
              const current = isOpenProject(p, currentDir);
              return (
                <li key={p.dir}>
                  <button
                    data-idx={i}
                    disabled={!openable(p)}
                    onClick={() => choose(p)}
                    onPointerEnter={() => setSel(i)}
                    className={`flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-[12px] disabled:opacity-40 ${
                      i === selIdx ? "bg-[var(--color-accent)]/10" : ""
                    }`}
                  >
                    <ProjectThumb src={p.thumb} />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-medium">{p.title || t("rec2.default_title")}</span>
                      <span className="block text-[11px] text-[var(--color-muted)]">
                        {new Date(lastChange(p)).toLocaleString(locale(), { dateStyle: "short", timeStyle: "short" })}
                        {current ? ` · ${t("rec4.projects.current")}` : ""}
                      </span>
                    </span>
                    <span className="w-12 shrink-0 text-right text-[11px] tabular-nums text-[var(--color-muted)]">
                      {formatTimecode(p.durationMs)}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
        <div className="mt-2 flex justify-end">
          <button
            onClick={other}
            className="flex items-center gap-1.5 rounded-md border border-[var(--color-border)] px-2 py-1 text-[12px] hover:border-[var(--color-accent)]"
          >
            <FolderOpen size={13} /> {t("rec4.projects.open")}
          </button>
        </div>
      </div>
    </div>
  );
}
