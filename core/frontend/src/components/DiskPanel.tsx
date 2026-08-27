import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  HardDrive,
  RefreshCw,
  ChevronRight,
  Trash2,
  Folder,
  FileIcon,
  CornerLeftUp,
} from "lucide-react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { diskScan, diskTrash, type DiskScan, type DiskNode, type DiskScanProgress } from "../lib/ipc";
import {
  sunburstArcs,
  arcPath,
  nodeAt,
  formatBytes,
  formatPct,
  baseName,
  parentPath,
  joinPath,
  pathCrumbs,
  childRows,
  type Arc,
  type ChildRow,
} from "../lib/disk";
import { confirmDialog } from "../lib/confirm";
import { prefersReducedMotion } from "../lib/md3-motion";

/**
 * `disk` / `daisy` — a DaisyDisk-style disk-usage sunburst in the preview
 * column (v0.120.0). Concentric rings (each a directory level, each segment
 * sized by on-disk space), a centre hub with the volume free/used readout,
 * click-to-drill with a breadcrumb, hover details, and a largest-files list.
 * Enter-activated — a full `~` walk is heavy IO, never per keystroke.
 */
const SIZE = 320; // svg viewbox (square)
const CX = SIZE / 2;
const CY = SIZE / 2;
const HUB_R = 58;
const RING = 26;
const RINGS = 5;

export function DiskPanel({
  arg,
  focused,
  onExit,
}: {
  arg: string;
  focused: boolean;
  onExit: () => void;
}) {
  const [scan, setScan] = useState<DiskScan | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [progress, setProgress] = useState<DiskScanProgress | null>(null);
  const [scanning, setScanning] = useState(false);
  // Drill path (index chain from the scan root); [] = the root itself.
  const [drill, setDrill] = useState<number[]>([]);
  // The folder currently being scanned. Starts at the typed argument and moves
  // as the user navigates OUT of the scanned tree — walking up past the root,
  // or down past where the walk stopped pruning.
  const [target, setTarget] = useState<string | null>(argPath(arg));
  const [hover, setHover] = useState<Arc | null>(null);
  /// Selected row in the child list (keyboard navigation).
  const [sel, setSel] = useState(0);
  // The key handler reads the rows through a ref so it doesn't re-subscribe on
  // every scan.
  const rowsRef = useRef<ChildRow[]>([]);
  // Set once the user actually drives the list with the keyboard — the list
  // sits below the chart, so it has to come into view THEN, but never on
  // mount (that dragged the header and path bar off-screen).
  const navigatedRef = useRef(false);
  const [note, setNote] = useState<string | null>(null);
  const aliveRef = useRef(true);
  const scrollRef = useRef<HTMLDivElement>(null);
  const seqRef = useRef(0);

  const run = useCallback((path: string | null) => {
    const seq = ++seqRef.current;
    setScanning(true);
    setErr(null);
    setProgress({ items: 0, bytes: 0 });
    diskScan(path && path.trim() ? path.trim() : null)
      .then((s) => {
        if (!aliveRef.current || seq !== seqRef.current) return;
        setScan(s);
        setDrill([]);
        setHover(null);
        setScanning(false);
      })
      .catch((e) => {
        if (!aliveRef.current || seq !== seqRef.current) return;
        setErr(String(e));
        setScanning(false);
      });
  }, []);

  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  // One scan per target — mount included. Navigation is just `setTarget`.
  useEffect(() => {
    run(target);
  }, [target, run]);

  // A newly typed argument re-targets. ⚠️ It must go through `setTarget`, NOT
  // call `run` itself: on mount this fires with the value `target` was already
  // seeded from, and React bails out of an identical state write, so there is
  // exactly ONE walk. A direct `run(arg)` here (the shape this effect used to
  // have) would scan the home folder twice on every open.
  useEffect(() => {
    setTarget(argPath(arg));
  }, [arg]);

  /**
   * Up one level. Inside the scanned tree that's instant (the sizes are
   * already known); at the scan root it re-scans the parent folder, which is
   * what lets you browse the whole disk without retyping a path.
   */
  const goUp = useCallback(() => {
    if (drill.length > 0) {
      setDrill((d) => d.slice(0, -1));
      setHover(null);
      return;
    }
    const p = scan ? parentPath(scan.root_path) : null;
    if (p) setTarget(p);
  }, [drill, scan]);

  /** Open a child of the current focus: instant while the tree still has its
   *  children, a fresh scan at the walk's pruning boundary. Shared by the arc
   *  click and the list so both behave identically. */
  const openChild = useCallback(
    (index: number, node: DiskNode) => {
      if (!scan || !node.is_dir || node.other) return;
      if ((node.children?.length ?? 0) > 0) {
        setDrill((d) => [...d, index]);
        setHover(null);
      } else {
        const abs = absPath(scan, drill, [index]);
        if (abs) setTarget(abs);
      }
    },
    [scan, drill],
  );

  /** Navigate to an absolute path — instantly if it's inside the current
   *  tree, otherwise by re-scanning there. */
  const goTo = useCallback((crumbSteps: number | null, path: string) => {
    if (crumbSteps === null) setTarget(path);
    else {
      setDrill((d) => d.slice(0, crumbSteps));
      setHover(null);
    }
  }, []);

  // Live progress while a scan is in flight.
  useEffect(() => {
    let un: UnlistenFn | undefined;
    let cancelled = false;
    void listen<DiskScanProgress>("disk-scan-progress", (e) => {
      if (!cancelled && scanning) setProgress(e.payload);
    }).then((u) => {
      if (cancelled) u();
      else un = u;
    });
    return () => {
      cancelled = true;
      un?.();
    };
  }, [scanning]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      // The path is typed in the search field, so a shortcut must never eat a
      // keystroke meant for it (the weather lesson). Esc still exits from
      // anywhere.
      const tgt = e.target as HTMLElement | null;
      const typing =
        !!tgt && (tgt.tagName === "INPUT" || tgt.tagName === "TEXTAREA" || tgt.isContentEditable);

      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        // Esc drills UP one level first, then exits at the root (DaisyDisk's
        // back gesture).
        if (!typing && drill.length > 0) setDrill((d) => d.slice(0, -1));
        else onExit();
        return;
      }
      if (typing || e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === "Backspace" || e.key === "ArrowLeft") {
        e.preventDefault();
        goUp();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        // The list is the keyboard path into small folders; the chart can't
        // offer one because its slivers aren't addressable.
        e.preventDefault();
        navigatedRef.current = true;
        setSel((i) => {
          const n = rowsRef.current.length;
          if (n === 0) return 0;
          return (i + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
        });
      } else if (e.key === "Enter") {
        e.preventDefault();
        const row = rowsRef.current[sel];
        if (row) openChild(row.index, row.node);
      } else if (e.key === "r" || e.key === "R") {
        e.preventDefault();
        run(target);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit, drill, goUp, run, target, sel, openChild]);


  const flash = (m: string) => {
    setNote(m);
    window.setTimeout(() => setNote((n) => (n === m ? null : n)), 2600);
  };

  // The node the rings are drawn FROM (the drill focus).
  const focusNode: DiskNode | null = useMemo(() => {
    if (!scan) return null;
    return nodeAt(scan.tree, drill) ?? scan.tree;
  }, [scan, drill]);

  const arcs = useMemo(
    () =>
      focusNode
        ? sunburstArcs(focusNode, { hubR: HUB_R, ring: RING, rings: RINGS })
        : [],
    [focusNode],
  );

  // Every child of the current focus — the ONLY way into a folder whose arc
  // is a sub-pixel sliver (see `childRows`).
  const rows: ChildRow[] = useMemo(() => (focusNode ? childRows(focusNode) : []), [focusNode]);

  useEffect(() => {
    rowsRef.current = rows;
  }, [rows]);
  // A new folder starts at its first entry.
  useEffect(() => {
    setSel(0);
    navigatedRef.current = false;
  }, [focusNode]);

  // The folder names along the drill, for the path bar.
  const drillNames = useMemo(() => {
    if (!scan) return [];
    const names: string[] = [];
    let cur: DiskNode = scan.tree;
    for (const i of drill) {
      const next = cur.children?.[i];
      if (!next) break;
      names.push(next.name);
      cur = next;
    }
    return names;
  }, [scan, drill]);

  if (err) {
    return (
      <Shell focused={focused}>
        <div className="rounded-xl border border-[var(--color-border)] p-4">
          <p className="text-[12px] font-medium">Scan fehlgeschlagen</p>
          <p className="mt-1 text-[11px] leading-snug text-[var(--color-muted)]">{err}</p>
          <p className="mt-2 text-[11px] text-[var(--color-muted)]">
            Für geschützte Ordner (z. B. weite Teile von <code>/</code>) braucht IR ggf. „Full Disk
            Access“ in den Systemeinstellungen.
          </p>
        </div>
      </Shell>
    );
  }

  if (!scan) {
    return (
      <Shell focused={focused}>
        <ScanningCard progress={progress} />
      </Shell>
    );
  }

  const hoverNode = hover?.node ?? focusNode!;
  const hoverIsFocus = !hover;
  const reduce = prefersReducedMotion();

  return (
    <div
      ref={scrollRef}
      className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]"
    >
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2 text-[13px] font-medium">
          <HardDrive size={15} className="shrink-0 text-[var(--color-accent)]" />
          <span className="truncate">Speicher</span>
        </div>
        <div className="flex shrink-0 items-center gap-0.5">
          <button
            type="button"
            onClick={goUp}
            disabled={drill.length === 0 && !parentPath(scan.root_path)}
            title="Eine Ebene höher (⌫)"
            className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)] disabled:opacity-30"
          >
            <CornerLeftUp size={13} />
          </button>
          <button
            type="button"
            onClick={() => run(target)}
            title="Neu scannen (R)"
            className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          >
            <RefreshCw size={13} className={scanning ? "animate-spin" : undefined} />
          </button>
        </div>
      </div>

      <PathBar rootPath={scan.root_path} drillNames={drillNames} onGo={goTo} />

      {/* The sunburst. */}
      <div className="relative mx-auto" style={{ width: SIZE, maxWidth: "100%" }}>
        <svg
          viewBox={`0 0 ${SIZE} ${SIZE}`}
          className="w-full"
          onMouseLeave={() => setHover(null)}
        >
          {arcs.map((a) => {
            const d = arcPath(a, CX, CY);
            if (!d) return null;
            const isHover = hover?.path.join(",") === a.path.join(",");
            const dimmed = hover && !isHover && !isAncestor(a.path, hover.path);
            return (
              <path
                key={a.path.join("-")}
                d={d}
                fill={a.color}
                stroke="var(--color-bg)"
                strokeWidth={0.5}
                className={reduce ? "" : "disk-arc"}
                style={{
                  opacity: dimmed ? 0.32 : 1,
                  cursor: a.node.is_dir && !a.node.other ? "pointer" : "default",
                  transition: reduce ? undefined : "opacity 140ms ease",
                  // Staggered sweep-in on (re)scan/drill: outer rings slightly later.
                  animationDelay: reduce ? undefined : `${a.depth * 55}ms`,
                }}
                onMouseEnter={() => setHover(a)}
                onClick={() => {
                  if (!a.node.is_dir || a.node.other) return;
                  // Deeper rings carry a multi-step path, so this can't go
                  // through `openChild` (which takes one child index).
                  if ((a.node.children?.length ?? 0) > 0) {
                    setDrill([...drill, ...a.path]);
                    setHover(null);
                  } else {
                    const abs = absPath(scan, drill, a.path);
                    if (abs) setTarget(abs);
                  }
                }}
              />
            );
          })}
          {/* Centre hub — free/used of the volume, or the focus node's size. */}
          <circle cx={CX} cy={CY} r={HUB_R - 3} fill="var(--color-surface)" stroke="var(--color-border)" strokeWidth={1} />
        </svg>
        {/* Hub label (HTML overlay for crisp text). */}
        <div
          className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center text-center"
          style={{ padding: HUB_R }}
        >
          <span className="max-w-[120px] truncate text-[11px] font-medium">
            {hoverIsFocus ? focusNode!.name : hoverNode.other ? "Sonstiges" : hoverNode.name}
          </span>
          <span className="text-[15px] font-semibold tabular-nums">{formatBytes(hoverNode.size)}</span>
          <span className="text-[10px] text-[var(--color-muted)] tabular-nums">
            {formatPct(hoverNode.size, focusNode!.size)}
            {hoverNode.is_dir && hoverNode.child_count > 0 ? ` · ${hoverNode.child_count} Einträge` : ""}
          </span>
        </div>
      </div>

      {/* Volume readout (DaisyDisk's centre free-space, as a bar below). */}
      {scan.volume_total > 0 && (
        <VolumeBar scan={scan} />
      )}

      {note && <p className="text-[11px] text-emerald-500">{note}</p>}

      {/* Hover / selection detail + trash action. */}
      {hover && !hover.node.other && (
        <DetailRow arc={hover} whole={focusNode!.size} onTrash={async () => {
          const abs = absPath(scan, drill, hover.path);
          if (!abs) return;
          if (!(await confirmDialog(`„${hover.node.name}“ in den Papierkorb verschieben?`, "Speicher"))) return;
          try {
            await diskTrash(abs);
            flash(`In den Papierkorb: ${hover.node.name}`);
            run(target); // re-scan to reflect the freed space — stay where we are
          } catch (e) {
            flash(String(e));
          }
        }} />
      )}

      <ChildList
        rows={rows}
        selected={sel}
        onSelect={setSel}
        onOpen={(r) => openChild(r.index, r.node)}
        reveal={navigatedRef}
      />

      <TopFiles scan={scan} onTrash={async (path) => {
        if (!(await confirmDialog(`„${baseName(path)}“ in den Papierkorb verschieben?`, "Speicher"))) return;
        try {
          await diskTrash(path);
          flash(`In den Papierkorb: ${baseName(path)}`);
          run(target);
        } catch (e) {
          flash(String(e));
        }
      }} />

      <p className="text-[10px] text-[var(--color-muted)]">
        {scan.items.toLocaleString("de-DE")} Einträge gescannt · Klick = reinzoomen
      </p>
      {focused && (
        <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">
          ⌫ eine Ebene höher · R neu scannen · Esc zurück/schließen
        </p>
      )}
    </div>
  );
}

function Shell({ focused, children }: { focused: boolean; children: React.ReactNode }) {
  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]">
      <div className="flex items-center gap-2 text-[13px] font-medium">
        <HardDrive size={15} className="text-[var(--color-accent)]" /> Speicher
      </div>
      {children}
      {focused && <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">Esc schließen</p>}
    </div>
  );
}

function ScanningCard({ progress }: { progress: DiskScanProgress | null }) {
  return (
    <div className="flex flex-col items-center gap-3 rounded-xl border border-[var(--color-border)] p-6">
      <div className="disk-scan-orb" aria-hidden />
      <p className="text-[12px] font-medium">Scanne…</p>
      {progress && (
        <p className="text-[11px] text-[var(--color-muted)] tabular-nums">
          {progress.items.toLocaleString("de-DE")} Einträge · {formatBytes(progress.bytes)}
        </p>
      )}
      <p className="text-[11px] text-[var(--color-muted)]">
        Ein voller Home-Scan kann einen Moment dauern.
      </p>
    </div>
  );
}

/**
 * The absolute path of what's on screen, always visible and always clickable.
 * Segments inside the scanned tree jump instantly; those above the scan root
 * re-scan there, which is how you browse out of the folder you started in.
 */
function PathBar({
  rootPath,
  drillNames,
  onGo,
}: {
  rootPath: string;
  drillNames: string[];
  onGo: (steps: number | null, path: string) => void;
}) {
  const crumbs = pathCrumbs(rootPath, drillNames);
  return (
    <div
      className="flex flex-wrap items-center gap-0.5 text-[11px]"
      title={crumbs[crumbs.length - 1]?.path}
    >
      {crumbs.map((c, i) => (
        <span key={c.path} className="flex items-center gap-0.5">
          {i > 0 && <ChevronRight size={11} className="shrink-0 text-[var(--color-muted)]" />}
          <button
            type="button"
            onClick={() => onGo(c.steps, c.path)}
            className={
              "max-w-[140px] truncate rounded px-1 py-0.5 font-[var(--font-mono)] " +
              (i === crumbs.length - 1
                ? "font-medium text-[var(--color-fg)]"
                : "text-[var(--color-muted)] hover:text-[var(--color-accent)]")
            }
            title={c.path}
          >
            {c.name}
          </button>
        </span>
      ))}
    </div>
  );
}

/**
 * Every child of the current folder as a row — the way into folders the chart
 * cannot show. A 2 MB `src` beside a 20 GB `target` is a sub-pixel arc; here
 * it is a full-width row like any other.
 */
function ChildList({
  rows,
  selected,
  onSelect,
  onOpen,
  reveal,
}: {
  rows: ChildRow[];
  selected: number;
  onSelect: (i: number) => void;
  onOpen: (r: ChildRow) => void;
  /** True once the user drove the list from the keyboard. */
  reveal: React.RefObject<boolean>;
}) {
  const selRef = useRef<HTMLButtonElement>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  // ⚠️ Scroll the LIST, never `scrollIntoView`. On mount the first row sits
  // below the preview's fold, so `scrollIntoView` scrolled the whole column
  // and pushed the header + path bar out of sight (seen in a live capture).
  // Adjusting the list's own scrollTop cannot move anything else.
  useEffect(() => {
    const box = boxRef.current;
    const el = selRef.current;
    if (!box || !el) return;
    // Only once the keyboard is in play: bring the list itself into view.
    if (reveal.current) el.scrollIntoView({ block: "nearest" });
    const top = el.offsetTop - box.offsetTop;
    if (top < box.scrollTop) box.scrollTop = top;
    else if (top + el.offsetHeight > box.scrollTop + box.clientHeight) {
      box.scrollTop = top + el.offsetHeight - box.clientHeight;
    }
  }, [selected, reveal]);
  if (rows.length === 0) return null;
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
      <p className="mb-2 text-[11px] font-medium">
        Inhalt <span className="text-[var(--color-muted)]">· ↑↓ wählen · Enter öffnen</span>
      </p>
      <div ref={boxRef} className="flex max-h-[220px] flex-col gap-0.5 overflow-y-auto">
        {rows.map((r, i) => {
          const openable = r.node.is_dir && !r.node.other;
          return (
            <button
              key={`${r.index}-${r.node.name}`}
              ref={i === selected ? selRef : undefined}
              type="button"
              onClick={() => (openable ? onOpen(r) : onSelect(i))}
              onMouseEnter={() => onSelect(i)}
              className={
                "flex items-center gap-2 rounded px-1.5 py-1 text-left text-[11px] " +
                (i === selected ? "bg-[var(--color-accent)]/15" : "hover:bg-[var(--color-border)]/40") +
                (openable ? " cursor-pointer" : " cursor-default")
              }
            >
              <span className="shrink-0 text-[var(--color-muted)]">
                {r.node.is_dir ? <Folder size={12} /> : <FileIcon size={12} />}
              </span>
              <span className="min-w-0 flex-1 truncate" title={r.node.name}>
                {r.node.other ? "Sonstiges" : r.node.name}
              </span>
              {/* A share bar, so the proportion the chart shows survives here. */}
              <span className="h-1 w-10 shrink-0 overflow-hidden rounded-full bg-[var(--color-border)]">
                <span
                  className="block h-full rounded-full bg-[var(--color-accent)]"
                  style={{ width: `${Math.max(2, r.share * 100)}%` }}
                />
              </span>
              <span className="w-16 shrink-0 text-right tabular-nums text-[var(--color-muted)]">
                {formatBytes(r.node.size)}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function VolumeBar({ scan }: { scan: DiskScan }) {
  const used = scan.volume_total - scan.volume_free;
  const usedPct = (used / scan.volume_total) * 100;
  // The scanned folder's share of the whole volume (DaisyDisk highlights how
  // much of the disk this subtree accounts for).
  const scanPct = (scan.total / scan.volume_total) * 100;
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
      <div className="mb-1 flex items-center justify-between text-[11px]">
        <span className="text-[var(--color-muted)]">{scan.volume_mount || "Volume"}</span>
        <span className="tabular-nums">
          {formatBytes(scan.volume_free)} frei von {formatBytes(scan.volume_total)}
        </span>
      </div>
      <div className="relative h-2 w-full overflow-hidden rounded-full bg-[var(--color-border)]">
        {/* used (muted) */}
        <div
          className="absolute inset-y-0 left-0 rounded-full bg-[var(--color-muted)] opacity-50"
          style={{ width: `${Math.min(100, usedPct)}%` }}
        />
        {/* this scan's slice, in accent, overlaid at the left */}
        <div
          className="absolute inset-y-0 left-0 rounded-full bg-[var(--color-accent)]"
          style={{ width: `${Math.min(100, scanPct)}%` }}
        />
      </div>
      <p className="mt-1 text-[10px] text-[var(--color-muted)]">
        Dieser Ordner: {formatBytes(scan.total)} · {formatPct(scan.total, scan.volume_total)} des Volumes
      </p>
    </div>
  );
}

function DetailRow({ arc, whole, onTrash }: { arc: Arc; whole: number; onTrash: () => void }) {
  return (
    <div className="flex items-center gap-2 rounded-lg border border-[var(--color-border)] px-2.5 py-1.5 text-[11px]">
      <span className="shrink-0" style={{ color: arc.color }}>
        {arc.node.is_dir ? <Folder size={13} /> : <FileIcon size={13} />}
      </span>
      <span className="min-w-0 flex-1 truncate" title={arc.node.name}>
        {arc.node.name}
      </span>
      <span className="shrink-0 tabular-nums text-[var(--color-muted)]">
        {formatBytes(arc.node.size)} · {formatPct(arc.node.size, whole)}
      </span>
      <button
        type="button"
        onClick={onTrash}
        title="In den Papierkorb"
        className="shrink-0 rounded p-1 text-[var(--color-muted)] hover:text-red-500"
      >
        <Trash2 size={12} />
      </button>
    </div>
  );
}

function TopFiles({ scan, onTrash }: { scan: DiskScan; onTrash: (path: string) => void }) {
  if (scan.top_files.length === 0) return null;
  const max = scan.top_files[0].size || 1;
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
      <p className="mb-2 text-[11px] font-medium">Größte Dateien</p>
      <div className="flex flex-col gap-1">
        {scan.top_files.slice(0, 12).map((f) => (
          <div key={f.path} className="group flex items-center gap-2 text-[11px]">
            <div className="relative min-w-0 flex-1">
              <div
                className="absolute inset-y-0 left-0 rounded bg-[var(--color-accent)] opacity-15"
                style={{ width: `${(f.size / max) * 100}%` }}
              />
              <span className="relative block truncate px-1 py-0.5 font-[var(--font-mono)]" title={f.path}>
                {baseName(f.path)}
              </span>
            </div>
            <span className="shrink-0 tabular-nums text-[var(--color-muted)]">{formatBytes(f.size)}</span>
            <button
              type="button"
              onClick={() => onTrash(f.path)}
              title="In den Papierkorb"
              className="shrink-0 rounded p-0.5 text-[var(--color-muted)] opacity-0 transition-opacity hover:text-red-500 group-hover:opacity-100"
            >
              <Trash2 size={11} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

/** Is `path` an ancestor (prefix) of `of`? Used to keep the hovered segment's
 *  parents un-dimmed (DaisyDisk highlights the whole radial slice). */
function isAncestor(path: number[], of: number[]): boolean {
  if (path.length >= of.length) return false;
  return path.every((v, i) => v === of[i]);
}

/** The typed argument as a scan target — blank means "let the backend decide"
 *  (the Finder selection, else the home folder). */
function argPath(arg: string): string | null {
  const t = arg.trim();
  return t ? t : null;
}

/** Absolute filesystem path of an arc, from the scan root + drill + arc path.
 *  Returns null if any node has no resolvable name (the synthetic "Other"). */
function absPath(scan: DiskScan, drill: number[], arcPathIdx: number[]): string | null {
  let cur: DiskNode = scan.tree;
  const parts: string[] = [];
  for (const i of [...drill, ...arcPathIdx]) {
    const next = cur.children?.[i];
    if (!next || next.other) return null;
    parts.push(next.name);
    cur = next;
  }
  return joinPath(scan.root_path, parts);
}
