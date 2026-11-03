import { useEffect, useRef, useState } from "react";
import { recordListProjects, type RecordProjectItem } from "../../lib/ipc";
import { formatTimecode } from "../../lib/editor";
import { locale, useT } from "../../lib/i18n";
import { isOpenProject } from "../../lib/record-projects";

/** The projects that can be inserted: finished, not the open one, not the one recording. */
export function insertableTakes(items: RecordProjectItem[], currentDir: string): RecordProjectItem[] {
  return items.filter((p) => p.status === "ready" && !p.active && !isOpenProject(p, currentDir) && p.durationMs > 0);
}

/**
 * "Insert recording…": the other finished projects of the recordings folder. Picking one
 * copies it into the open project (Rust, `editor_import_take`) and appends it as a clip.
 */
export function TakePicker({
  currentDir,
  onPick,
  onClose,
  onError,
}: {
  currentDir: string;
  onPick: (dir: string) => void;
  onClose: () => void;
  onError: (e: unknown) => void;
}) {
  const t = useT();
  const [items, setItems] = useState<RecordProjectItem[] | null>(null);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let dead = false;
    recordListProjects()
      .then((list) => !dead && setItems(insertableTakes(list, currentDir)))
      .catch((e) => {
        if (dead) return;
        setItems([]);
        onError(e);
      });
    return () => {
      dead = true;
    };
  }, [currentDir, onError]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    const onDown = (e: PointerEvent) => {
      const el = box.current;
      if (el && e.target instanceof Node && !el.contains(e.target) && !el.parentElement?.contains(e.target)) onClose();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onDown, true);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onDown, true);
    };
  }, [onClose]);

  const date = (ms: number) =>
    ms > 0 ? new Date(ms).toLocaleString(locale(), { dateStyle: "short", timeStyle: "short" }) : "";

  return (
    <div
      ref={box}
      role="dialog"
      aria-label={t("rec4.take.insert")}
      data-testid="ed-take-picker"
      className="pop-enter absolute bottom-10 left-0 z-30 w-80 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-1.5 shadow-xl"
    >
      <p className="px-2 pb-1.5 pt-1 text-[11px] text-[var(--color-muted)]">{t("rec4.take.picker_hint")}</p>
      {items === null ? (
        <p className="px-2 py-2 text-[12px] text-[var(--color-muted)]">{t("rec4.take.loading")}</p>
      ) : items.length === 0 ? (
        <p className="px-2 py-2 text-[12px] text-[var(--color-muted)]">{t("rec4.take.none")}</p>
      ) : (
        <ul className="max-h-64 overflow-y-auto">
          {items.map((p) => (
            <li key={p.dir}>
              <button
                className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] hover:bg-[var(--color-bg)]"
                onClick={() => onPick(p.dir)}
              >
                <span className="min-w-0 flex-1 truncate">{p.title || t("rec2.default_title")}</span>
                <span className="shrink-0 text-[11px] text-[var(--color-muted)]">{date(p.createdAt)}</span>
                <span className="w-12 shrink-0 text-right text-[11px] tabular-nums text-[var(--color-muted)]">
                  {formatTimecode(p.durationMs)}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
