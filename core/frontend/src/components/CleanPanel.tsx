/**
 * `clean` — interactive picker in the right preview column (v0.84.242; rows are
 * DIRECTORIES since v0.84.264; same inline family as brightness/sound/stats).
 *
 * The dry-run scan returns aggregated directory rows grouped under their
 * category — one line per directory with its total size, largest first — and
 * only the ticked directories are handed to `cleaner_execute` (the file-level
 * plan never leaves the backend). Keyboard-first: ↑/↓ move · Space toggle (on a
 * category header: the whole group) · A all/none · Enter twice (arm → confirm)
 * deletes · Esc disarms, then exits.
 *
 * **Background jobs (v0.101.1):** scan + execute run in the Rust backend and
 * survive Esc / overlay-hide (same pattern as `shazam`). Reopening reconnects
 * via `cleaner_status` + the `clean-done` event; a finished execute while the
 * panel is closed surfaces as an App-level status toast.
 *
 * All selection math is the pure, unit-tested `lib/clean-select.ts`.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, CheckSquare, Loader2, Sparkles, Square, Trash2 } from "lucide-react";
import {
  cleanerExecute,
  cleanerScan,
  cleanerStatus,
  showStatusToast,
  type CleanDone,
  type CleanPlanView,
} from "../lib/ipc";
import { formatBytes } from "../lib/commands";
import {
  basename,
  buildRows,
  dirsOfCategory,
  prettyPath,
  selectionTotals,
  type CleanRow,
} from "../lib/clean-select";

type Phase = "scanning" | "pick" | "executing" | "error";

/** Categories that appear in the picker but start UNCHECKED — they touch user
 * files (Downloads installers, duplicate downloads, Trash, stale projects) or
 * have a real re-build cost (Docker build cache, Xcode archives with their
 * dSYMs), so deleting them must be an explicit tick, never a default. */
const PRESELECT_OFF = new Set([
  "installers",
  "dupes",
  "trash",
  "docker",
  "xcode_archives",
  "stale_node_modules",
  "stale_rust_target",
  "simctl_unavailable",
]);

function applyView(
  v: CleanPlanView,
  setView: (v: CleanPlanView) => void,
  setRows: (r: CleanRow[]) => void,
  setSelected: (s: Set<string>) => void,
  setPhase: (p: Phase) => void,
) {
  setView(v);
  setRows(buildRows(v));
  setSelected(new Set(v.dirs.filter((d) => !PRESELECT_OFF.has(d.category)).map((d) => d.path)));
  setPhase("pick");
}

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
  const [view, setView] = useState<CleanPlanView | null>(null);
  const [rows, setRows] = useState<CleanRow[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [row, setRow] = useState(0);
  const [armed, setArmed] = useState(false);
  const rowRefs = useRef<(HTMLDivElement | null)[]>([]);
  /** Bumped on unmount so in-flight invoke returns don't toast/setState. */
  const runIdRef = useRef(0);
  /** True when we mounted into an already-running backend job (reconnect). */
  const reconnectedRef = useRef(false);

  // `~/…` display shortening — best-effort, purely cosmetic.
  const home = useMemo(() => {
    const first = view?.dirs.find((d) => d.path.startsWith("/Users/"));
    const m = first?.path.match(/^(\/Users\/[^/]+)/);
    return m?.[1];
  }, [view]);

  const startScan = useCallback(() => {
    const my = ++runIdRef.current;
    setPhase("scanning");
    setError("");
    cleanerScan()
      .then((v) => {
        if (runIdRef.current !== my) return;
        applyView(v, setView, setRows, setSelected, setPhase);
      })
      .catch((e) => {
        if (runIdRef.current !== my) return;
        // Concurrent start while reconnecting — ignore; clean-done will land.
        if (String(e).includes("clean.busy")) return;
        setError(String(e));
        setPhase("error");
      });
  }, []);

  // Mount: reconnect to an in-flight job, reuse a pending scan view, or scan.
  useEffect(() => {
    let cancelled = false;
    void cleanerStatus().then((st) => {
      if (cancelled) return;
      if (st.phase === "scanning") {
        reconnectedRef.current = true;
        setPhase("scanning");
        return;
      }
      if (st.phase === "executing") {
        reconnectedRef.current = true;
        setPhase("executing");
        return;
      }
      if (st.view) {
        applyView(st.view, setView, setRows, setSelected, setPhase);
        return;
      }
      startScan();
    });
    return () => {
      cancelled = true;
      runIdRef.current += 1;
    };
  }, [startScan]);

  // Reconnected path: apply the backend's `clean-done` (our own invoke also
  // resolves — guarded by runId / reconnectedRef so we don't double-apply).
  useEffect(() => {
    let gone = false;
    let un: (() => void) | null = null;
    void listen<CleanDone>("clean-done", (e) => {
      if (!reconnectedRef.current) return;
      reconnectedRef.current = false;
      const d = e.payload;
      if (d.kind === "scan") {
        if (d.view) applyView(d.view, setView, setRows, setSelected, setPhase);
        else {
          setError(d.error ?? "Scan failed");
          setPhase("error");
        }
      } else if (d.kind === "execute") {
        if (d.result) {
          // Panel open again mid-execute — toast + exit like a local finish.
          void showStatusToast(
            "clean",
            true,
            "Cleaned",
            `${formatBytes(d.result.freed_bytes)} freed${
              d.result.errors.length ? ` · ${d.result.errors.length} skipped` : ""
            }`,
          );
          onExit();
        } else {
          setError(d.error ?? "Cleaning failed");
          setPhase("error");
        }
      }
    }).then((u) => {
      if (gone) u();
      else un = u;
    });
    return () => {
      gone = true;
      if (un) un();
    };
  }, [onExit]);

  const setPaths = (paths: string[], on: boolean) => {
    setArmed(false);
    setSelected((prev) => {
      const next = new Set(prev);
      for (const p of paths) {
        if (on) next.add(p);
        else next.delete(p);
      }
      return next;
    });
  };

  /** Toggle a row: a directory toggles itself, a header its whole group. */
  const toggleRow = (r: CleanRow) => {
    if (r.kind === "dir") {
      setPaths([r.dir.path], !selected.has(r.dir.path));
      return;
    }
    const paths = view ? dirsOfCategory(view, r.key) : [];
    const allOn = paths.every((p) => selected.has(p));
    setPaths(paths, !allOn);
  };

  const toggleAll = () => {
    setArmed(false);
    const all = view?.dirs.map((d) => d.path) ?? [];
    setSelected((prev) => (prev.size === all.length ? new Set() : new Set(all)));
  };

  const totals = selectionTotals(view?.dirs ?? [], selected);

  const execute = async () => {
    if (!view || selected.size === 0) return;
    const my = ++runIdRef.current;
    setPhase("executing");
    try {
      const res = await cleanerExecute([...selected]);
      // Unmounted (Esc) while deleting — App's clean-done listener toasts.
      if (runIdRef.current !== my) return;
      await showStatusToast(
        "clean",
        true,
        "Cleaned",
        `${formatBytes(res.freed_bytes)} freed${res.errors.length ? ` · ${res.errors.length} skipped` : ""}`,
      );
      onExit();
    } catch (e) {
      if (runIdRef.current !== my) return;
      setError(String(e));
      setPhase("error");
    }
  };

  // Keyboard while focused: ↑/↓ row, Space toggle, A all/none, Enter twice
  // deletes, Esc disarms then exits. Esc during scan/execute closes the UI
  // only — the backend job keeps running.
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
          setRow((r) => Math.min(rows.length - 1, r + 1));
          break;
        case " ":
          if (rows[row]) toggleRow(rows[row]);
          break;
        case "a":
        case "A":
          toggleAll();
          break;
        case "Enter":
          if (selected.size === 0) break;
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
  }, [focused, phase, rows, row, armed, selected, view]);

  // Keep the selected row in view in long lists.
  useEffect(() => {
    rowRefs.current[row]?.scrollIntoView({ block: "nearest" });
  }, [row]);

  return (
    <div className="flex h-full flex-col gap-2 overflow-hidden p-3 text-sm">
      <div className="flex items-center gap-2 text-[var(--color-fg)]">
        <Sparkles size={16} className="text-rose-400" />
        <span className="font-semibold">Clean</span>
        {phase === "pick" && view && (
          <span className="ml-auto text-xs text-[var(--color-muted)]">
            found {formatBytes(view.total_bytes)} in {view.dirs.length} folders
          </span>
        )}
      </div>

      {phase === "scanning" && (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 text-[var(--color-muted)]">
          <Loader2 size={28} className="animate-spin" />
          <div>Scanning caches + projects…</div>
          <div className="text-xs">Esc closes the overlay — scan keeps running</div>
        </div>
      )}

      {phase === "executing" && (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 text-[var(--color-muted)]">
          <Loader2 size={28} className="animate-spin" />
          <div>
            {totals.files > 0
              ? `Deleting ${totals.files.toLocaleString()} files…`
              : "Deleting…"}
          </div>
          <div className="text-xs">Esc closes the overlay — delete keeps running</div>
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

      {phase === "pick" && rows.length === 0 && (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 text-[var(--color-muted)]">
          <Sparkles size={24} />
          <div>Nothing to clean — all tidy.</div>
          <div className="text-xs">Esc to close</div>
        </div>
      )}

      {phase === "pick" && rows.length > 0 && (
        <>
          <div className="min-h-0 flex-1 space-y-0.5 overflow-y-auto pr-1">
            {rows.map((r, i) => {
              const isSel = i === row;
              const setRef = (el: HTMLDivElement | null) => {
                rowRefs.current[i] = el;
              };

              if (r.kind === "header") {
                const paths = view ? dirsOfCategory(view, r.key) : [];
                const allOn = paths.length > 0 && paths.every((p) => selected.has(p));
                const someOn = paths.some((p) => selected.has(p));
                return (
                  <div
                    key={`h-${r.key}`}
                    ref={setRef}
                    className={
                      "mt-2 flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 first:mt-0 " +
                      (isSel ? "bg-rose-500/10 ring-1 ring-rose-500/60" : "")
                    }
                    onClick={() => {
                      setRow(i);
                      toggleRow(r);
                      onInteract?.();
                    }}
                    title={r.label}
                  >
                    {allOn ? (
                      <CheckSquare size={14} className="shrink-0 text-rose-400" />
                    ) : (
                      <Square
                        size={14}
                        className={someOn ? "shrink-0 text-rose-400/60" : "shrink-0 text-[var(--color-muted)]"}
                      />
                    )}
                    <span className="min-w-0 flex-1 truncate text-[11px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">
                      {r.label}
                    </span>
                    <span className="shrink-0 text-[11px] tabular-nums text-[var(--color-muted)]">
                      {r.dirs} {r.dirs === 1 ? "folder" : "folders"}
                    </span>
                    <span className="w-16 shrink-0 text-right text-[11px] font-semibold tabular-nums text-[var(--color-fg)]">
                      {formatBytes(r.bytes)}
                    </span>
                  </div>
                );
              }

              const d = r.dir;
              const isChecked = selected.has(d.path);
              return (
                <div
                  key={d.path}
                  ref={setRef}
                  className={
                    "ml-4 flex cursor-pointer items-center gap-2 rounded-lg border px-2.5 py-1.5 transition-colors " +
                    (isSel
                      ? "border-rose-500/60 bg-rose-500/10"
                      : "border-[var(--color-border)] bg-[var(--color-surface)]")
                  }
                  onClick={() => {
                    setRow(i);
                    toggleRow(r);
                    onInteract?.();
                  }}
                  title={d.path}
                >
                  {isChecked ? (
                    <CheckSquare size={15} className="shrink-0 text-rose-400" />
                  ) : (
                    <Square size={15} className="shrink-0 text-[var(--color-muted)]" />
                  )}
                  <span
                    className={
                      "min-w-0 flex-1 truncate " +
                      (isChecked ? "text-[var(--color-fg)]" : "text-[var(--color-muted)]")
                    }
                  >
                    {basename(d.path)}
                    <span className="ml-1.5 text-[10px] text-[var(--color-muted)]">
                      {prettyPath(d.path, home)}
                    </span>
                  </span>
                  <span className="shrink-0 text-xs tabular-nums text-[var(--color-muted)]">
                    {d.count.toLocaleString()} files
                  </span>
                  <span className="w-16 shrink-0 text-right text-xs font-semibold tabular-nums text-[var(--color-fg)]">
                    {formatBytes(d.size)}
                  </span>
                </div>
              );
            })}
          </div>

          <button
            type="button"
            disabled={selected.size === 0}
            className={
              "flex items-center justify-center gap-2 rounded-lg px-3 py-2 font-semibold transition-colors " +
              (selected.size === 0
                ? "cursor-default bg-[var(--color-surface)] text-[var(--color-muted)]"
                : armed
                  ? "bg-red-600 text-white"
                  : "bg-rose-600/90 text-white hover:bg-rose-600")
            }
            onClick={() => {
              if (selected.size === 0) return;
              if (armed) void execute();
              else setArmed(true);
              onInteract?.();
            }}
          >
            <Trash2 size={15} />
            {selected.size === 0
              ? "Nothing selected"
              : armed
                ? `Press Enter again — delete ${formatBytes(totals.bytes)} for good`
                : `Delete ${selected.size} ${selected.size === 1 ? "folder" : "folders"} · free ${formatBytes(totals.bytes)}`}
          </button>

          <div className="text-center text-[11px] text-[var(--color-muted)]">
            ↑↓ select · Space toggle · A all/none · Enter ×2 delete · Esc cancel — dev roots +
            categories in Settings → Cleaning
          </div>
        </>
      )}
    </div>
  );
}
