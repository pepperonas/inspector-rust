/**
 * `clean` — interactive category picker in the right preview column
 * (v0.84.242; same inline family as brightness/sound/stats/calendar).
 * Replaces the old all-or-nothing `window.confirm`: the dry-run scan renders
 * as a checkbox list of categories (size + file count + the 3 largest files of
 * the selected row), and only the checked categories are handed to
 * `cleaner_execute`. Keyboard-first: ↑/↓ select · Space toggle · A all/none ·
 * Enter twice (arm → confirm) deletes · Esc disarms, then exits. All selection
 * math is the pure, unit-tested `lib/clean-select.ts`.
 */
import { useEffect, useRef, useState } from "react";
import { AlertTriangle, CheckSquare, Loader2, Sparkles, Square, Trash2 } from "lucide-react";
import { cleanerExecute, cleanerScan, showStatusToast, type CleanPlan } from "../lib/ipc";
import { formatBytes } from "../lib/commands";
import {
  basename,
  categorySummaries,
  filterPlan,
  selectionTotals,
  topItems,
  type CleanCategorySummary,
} from "../lib/clean-select";

type Phase = "scanning" | "pick" | "executing" | "error";

/** Categories that appear in the picker but start UNCHECKED — they touch user
 * files (Downloads installers, duplicate downloads, Trash) or have a real
 * re-build cost (Docker build cache), so deleting them must be an explicit
 * tick, never a default. */
const PRESELECT_OFF = new Set(["installers", "dupes", "trash", "docker"]);

export function CleanPanel({
  focused,
  onInteract,
  onExit,
}: {
  focused: boolean;
  /** Called after a mouse interaction so the parent keeps the search field focused. */
  onInteract?: () => void;
  onExit: () => void;
}) {
  const [phase, setPhase] = useState<Phase>("scanning");
  const [error, setError] = useState("");
  const [plan, setPlan] = useState<CleanPlan | null>(null);
  const [summaries, setSummaries] = useState<CleanCategorySummary[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [row, setRow] = useState(0);
  const [armed, setArmed] = useState(false);
  const rowRefs = useRef<(HTMLDivElement | null)[]>([]);

  // Dry-run scan on mount. Read-only — nothing is deleted here.
  useEffect(() => {
    let cancelled = false;
    cleanerScan()
      .then((p) => {
        if (cancelled) return;
        const sums = categorySummaries(p);
        setPlan(p);
        setSummaries(sums);
        setSelected(new Set(sums.map((s) => s.key).filter((k) => !PRESELECT_OFF.has(k))));
        setPhase("pick");
      })
      .catch((e) => {
        if (cancelled) return;
        setError(String(e));
        setPhase("error");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const toggle = (key: string) => {
    setArmed(false);
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const toggleAll = () => {
    setArmed(false);
    setSelected((prev) =>
      prev.size === summaries.length ? new Set() : new Set(summaries.map((s) => s.key)),
    );
  };

  const totals = selectionTotals(summaries, selected);

  const execute = async () => {
    if (!plan || totals.files === 0) return;
    setPhase("executing");
    try {
      const res = await cleanerExecute(filterPlan(plan, selected));
      // The toast hides the popup itself and plays the flourish.
      await showStatusToast(
        "clean",
        true,
        "Cleaned",
        `${formatBytes(res.freed_bytes)} freed${res.errors.length ? ` · ${res.errors.length} skipped` : ""}`,
      );
      onExit();
    } catch (e) {
      setError(String(e));
      setPhase("error");
    }
  };

  // Keyboard while focused: ↑/↓ row, Space toggle, A all/none, Enter twice
  // deletes, Esc disarms then exits.
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (phase !== "pick") {
        if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          onExit();
        }
        return;
      }
      switch (e.key) {
        case "ArrowUp":
          setRow((r) => Math.max(0, r - 1));
          break;
        case "ArrowDown":
          setRow((r) => Math.min(summaries.length - 1, r + 1));
          break;
        case " ":
          if (summaries[row]) toggle(summaries[row].key);
          break;
        case "a":
        case "A":
          toggleAll();
          break;
        case "Enter":
          if (totals.files === 0) break;
          if (armed) void execute();
          else setArmed(true);
          break;
        case "Escape":
          if (armed) {
            setArmed(false);
          } else {
            onExit();
          }
          break;
        default:
          return;
      }
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focused, phase, summaries, row, armed, totals.files, selected, plan]);

  // Keep the selected row in view in long category lists.
  useEffect(() => {
    rowRefs.current[row]?.scrollIntoView({ block: "nearest" });
  }, [row]);

  const currentKey = summaries[row]?.key;
  const preview = plan && currentKey ? topItems(plan, currentKey, 3) : [];

  return (
    <div className="flex h-full flex-col gap-2 overflow-hidden p-3 text-sm">
      <div className="flex items-center gap-2 text-[var(--color-fg)]">
        <Sparkles size={16} className="text-rose-400" />
        <span className="font-semibold">Clean</span>
        {phase === "pick" && plan && (
          <span className="ml-auto text-xs text-[var(--color-muted)]">
            found {formatBytes(plan.total_bytes)} in {plan.items.length} files
          </span>
        )}
      </div>

      {phase === "scanning" && (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 text-[var(--color-muted)]">
          <Loader2 size={28} className="animate-spin" />
          <div>Scanning caches…</div>
        </div>
      )}

      {phase === "executing" && (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 text-[var(--color-muted)]">
          <Loader2 size={28} className="animate-spin" />
          <div>Deleting {totals.files.toLocaleString()} files…</div>
        </div>
      )}

      {phase === "error" && (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 px-4 text-center">
          <AlertTriangle size={24} className="text-amber-400" />
          <div className="text-[var(--color-fg)]">Cleaning failed</div>
          <div className="break-all text-xs text-[var(--color-muted)]">{error}</div>
          <div className="text-xs text-[var(--color-muted)]">Esc to close</div>
        </div>
      )}

      {phase === "pick" && summaries.length === 0 && (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 text-[var(--color-muted)]">
          <Sparkles size={24} />
          <div>Nothing to clean — all tidy.</div>
          <div className="text-xs">Esc to close</div>
        </div>
      )}

      {phase === "pick" && summaries.length > 0 && (
        <>
          <div className="min-h-0 flex-1 space-y-1 overflow-y-auto pr-1">
            {summaries.map((s, i) => {
              const isSel = i === row;
              const isChecked = selected.has(s.key);
              return (
                <div
                  key={s.key}
                  ref={(el) => {
                    rowRefs.current[i] = el;
                  }}
                  className={
                    "cursor-pointer rounded-lg border px-2.5 py-2 transition-colors " +
                    (isSel
                      ? "border-rose-500/60 bg-rose-500/10"
                      : "border-[var(--color-border)] bg-[var(--color-surface)]")
                  }
                  onClick={() => {
                    setRow(i);
                    toggle(s.key);
                    onInteract?.();
                  }}
                >
                  <div className="flex items-center gap-2">
                    {isChecked ? (
                      <CheckSquare size={16} className="shrink-0 text-rose-400" />
                    ) : (
                      <Square size={16} className="shrink-0 text-[var(--color-muted)]" />
                    )}
                    <span
                      className={
                        "min-w-0 flex-1 truncate " +
                        (isChecked ? "text-[var(--color-fg)]" : "text-[var(--color-muted)]")
                      }
                    >
                      {s.label}
                    </span>
                    <span className="shrink-0 text-xs tabular-nums text-[var(--color-muted)]">
                      {s.count.toLocaleString()} files
                    </span>
                    <span className="w-16 shrink-0 text-right text-xs font-semibold tabular-nums text-[var(--color-fg)]">
                      {formatBytes(s.bytes)}
                    </span>
                  </div>
                  {isSel && preview.length > 0 && (
                    <div className="mt-1.5 space-y-0.5 border-t border-[var(--color-border)] pt-1.5">
                      {preview.map((it) => (
                        <div
                          key={it.path}
                          className="flex items-center gap-2 text-xs text-[var(--color-muted)]"
                          title={it.path}
                        >
                          <span className="min-w-0 flex-1 truncate">{basename(it.path)}</span>
                          <span className="shrink-0 tabular-nums">{formatBytes(it.size)}</span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              );
            })}
          </div>

          <button
            type="button"
            disabled={totals.files === 0}
            className={
              "flex items-center justify-center gap-2 rounded-lg px-3 py-2 font-semibold transition-colors " +
              (totals.files === 0
                ? "cursor-default bg-[var(--color-surface)] text-[var(--color-muted)]"
                : armed
                  ? "bg-red-600 text-white"
                  : "bg-rose-600/90 text-white hover:bg-rose-600")
            }
            onClick={() => {
              if (totals.files === 0) return;
              if (armed) void execute();
              else setArmed(true);
              onInteract?.();
            }}
          >
            <Trash2 size={15} />
            {totals.files === 0
              ? "Nothing selected"
              : armed
                ? `Press Enter again — delete ${formatBytes(totals.bytes)} for good`
                : `Delete ${totals.files.toLocaleString()} files · free ${formatBytes(totals.bytes)}`}
          </button>

          <div className="text-center text-[11px] text-[var(--color-muted)]">
            ↑↓ select · Space toggle · A all/none · Enter ×2 delete · Esc cancel — more
            categories in Settings → Cleaning
          </div>
        </>
      )}
    </div>
  );
}
