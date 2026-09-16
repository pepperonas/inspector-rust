import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  HardDrive,
  RefreshCw,
  ChevronRight,
  Trash2,
  Folder,
  FolderOpen,
  FileIcon,
  CornerLeftUp,
  Plus,
  Check,
  X,
  Home,
} from "lucide-react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { diskScan, diskTrashMany, type DiskScan, type DiskNode, type DiskScanProgress } from "../lib/ipc";
import {
  sunburstArcs,
  sunburstExtent,
  arcPath,
  nodeAt,
  formatBytes,
  formatPct,
  baseName,
  parentPath,
  pathCrumbs,
  childRows,
  absPathOf,
  pruneScan,
  drillByNames,
  collectorTotals,
  type Arc,
  type ChildRow,
  type CollectorItem,
} from "../lib/disk";
import { prefersReducedMotion } from "../lib/md3-motion";

/**
 * `disk` / `daisy` — a DaisyDisk-style disk-usage sunburst in the preview
 * column (v0.120.0). Concentric rings (each a directory level, each segment
 * sized by on-disk space), a centre hub with the volume free/used readout,
 * click-to-drill with a breadcrumb, hover details, and a largest-files list.
 * Enter-activated — a full `~` walk is heavy IO, never per keystroke.
 *
 * Deleting (v0.169.0) is DaisyDisk's collector, keyboard-first: Space (or ＋)
 * collects the selected row — folders and files alike, across drill levels —
 * a bar shows the running total, and ⌘⌫ (or 🗑 in the bar) moves the whole
 * collection to the Trash, two-stage (arm → confirm). With an empty collector
 * ⌘⌫ acts on the selected row directly. Nothing re-scans afterwards: the tree
 * is pruned locally (the bytes now live in the Trash) and the drill position
 * is kept by NAME, so deleting three things deep in a tree no longer costs
 * three round trips to the root.
 */
/** How long an armed delete stays armed before it quietly disarms. */
const ARM_MS = 4000;
const HUB_R = 58;
const RING = 26;
const RINGS = 5;
/** The ring geometry, shared with `sunburstArcs` so the viewBox and the arcs
 *  can never disagree about how far the drawing reaches. */
const RING_OPTS = { hubR: HUB_R, ring: RING, rings: RINGS };
/** Stroke width + antialiasing breathing room around the outermost ring. */
const PAD = 6;
/** Square viewBox, DERIVED from the geometry so it always encloses the arcs
 *  (the outer ring reaches `sunburstExtent` = 188px around the centre). The svg
 *  scales this to the preview width, so it fits at every panel size without
 *  clipping — the fix for the outer ring being cut off (was a fixed 320 viewBox
 *  drawn to radius 188). */
const VIEW = 2 * (sunburstExtent(RING_OPTS) + PAD);
const CX = VIEW / 2;
const CY = VIEW / 2;

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
  // The collector: absolute-path snapshots (they survive drilling elsewhere).
  const [collector, setCollector] = useState<CollectorItem[]>([]);
  // Two-stage delete: first ⌘⌫/click arms, the second within ARM_MS commits.
  const [armed, setArmed] = useState(false);
  const armTimer = useRef<number | null>(null);
  const [busy, setBusy] = useState(false);
  // Per-path failures of the last run, shown until the next action.
  const [failures, setFailures] = useState<{ path: string; error: string }[]>([]);
  // Right-click context menu on a ring segment: viewport coords + the target.
  const [menu, setMenu] = useState<{ x: number; y: number; item: CollectorItem } | null>(null);

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
        const abs = absPathOf(scan, [...drill, index]);
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

  /** Scan the real home folder — `~` is resolved by the backend's `expand_user`
   *  (`disk_scan`). The empty-state offers this when `/home` turns out to be the
   *  empty macOS autofs mount. */
  const goHome = useCallback(() => setTarget("~"), []);

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



  // The node the rings are drawn FROM (the drill focus).
  const focusNode: DiskNode | null = useMemo(() => {
    if (!scan) return null;
    return nodeAt(scan.tree, drill) ?? scan.tree;
  }, [scan, drill]);

  const arcs = useMemo(
    () =>
      focusNode ? sunburstArcs(focusNode, RING_OPTS) : [],
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

  const flash = useCallback((m: string) => {
    setNote(m);
    window.setTimeout(() => setNote((n) => (n === m ? null : n)), 3200);
  }, []);

  const disarm = useCallback(() => {
    if (armTimer.current) window.clearTimeout(armTimer.current);
    armTimer.current = null;
    setArmed(false);
  }, []);
  const arm = useCallback(() => {
    if (armTimer.current) window.clearTimeout(armTimer.current);
    setArmed(true);
    armTimer.current = window.setTimeout(() => setArmed(false), ARM_MS);
  }, []);
  useEffect(() => () => { if (armTimer.current) window.clearTimeout(armTimer.current); }, []);
  // Any change of what's selected or shown disarms — an armed delete must
  // never fire on something other than what the user was looking at.
  useEffect(() => { disarm(); }, [sel, focusNode, disarm]);

  /** The collectable identity of a list row — null for the synthetic
   *  "Sonstiges" bucket, which has no path and can't be trashed. */
  const itemOf = useCallback(
    (idxPath: number[], node: DiskNode): CollectorItem | null => {
      if (!scan || node.other) return null;
      const path = absPathOf(scan, [...drill, ...idxPath]);
      return path ? { path, name: node.name, size: node.size, is_dir: node.is_dir } : null;
    },
    [scan, drill],
  );

  const toggleCollect = useCallback((item: CollectorItem | null) => {
    if (!item) return;
    disarm();
    setCollector((c) =>
      c.some((i) => i.path === item.path) ? c.filter((i) => i.path !== item.path) : [...c, item],
    );
  }, [disarm]);

  /** Open the right-click menu for a ring segment. `null` (the synthetic
   *  "Sonstiges" bucket, which has no path) opens nothing. */
  const openMenu = useCallback(
    (e: React.MouseEvent, item: CollectorItem | null) => {
      e.preventDefault();
      e.stopPropagation();
      if (!item) return;
      disarm(); // a fresh menu supersedes any armed collector delete
      setMenu({ x: e.clientX, y: e.clientY, item });
    },
    [disarm],
  );

  /** Move `items` to the Trash, then apply the result LOCALLY: prune the
   *  tree, keep the drill by name, keep the volume readout (the Trash still
   *  holds the bytes), and surface per-path failures. No re-scan. */
  const execute = useCallback(async (items: CollectorItem[]) => {
    disarm();
    if (!scan || items.length === 0 || busy) return;
    setBusy(true);
    setFailures([]);
    const seq = seqRef.current;
    const names = drillNames;
    try {
      const rep = await diskTrashMany(items.map((i) => i.path));
      if (!aliveRef.current) return;
      const trashed = new Set(rep.trashed);
      // A scan that started meanwhile is fresher than any prune of the old tree.
      if (seq === seqRef.current) {
        const pruned = pruneScan(scan, rep.trashed);
        setScan(pruned);
        setDrill(drillByNames(pruned.tree, names));
        setHover(null);
      }
      setCollector((c) => c.filter((i) => !trashed.has(i.path)));
      setFailures(rep.failed);
      const done = items.filter((i) => trashed.has(i.path));
      const tot = collectorTotals(done);
      if (tot.count > 0) {
        flash(
          `${tot.count === 1 ? "1 Eintrag" : `${tot.count} Einträge`} in den Papierkorb (${formatBytes(tot.bytes)}) — Platz wird frei, sobald du den Papierkorb leerst`,
        );
      }
    } catch (e) {
      if (aliveRef.current) setFailures([{ path: "", error: String(e) }]);
    } finally {
      if (aliveRef.current) setBusy(false);
    }
  }, [scan, busy, drillNames, disarm, flash]);

  /** What ⌘⌫ / the bar button acts on: the collection when it has anything,
   *  else the selected row alone (the "just delete this" path). */
  const trashTargets = useCallback((): CollectorItem[] => {
    if (collector.length > 0) return collector;
    const row = rowsRef.current[sel];
    const item = row ? itemOf([row.index], row.node) : null;
    return item ? [item] : [];
  }, [collector, sel, itemOf]);

  /** First call arms (the row / bar turns red and says so), the second commits. */
  const requestTrash = useCallback(() => {
    const targets = trashTargets();
    if (targets.length === 0) return;
    if (!armed) {
      arm();
      return;
    }
    void execute(targets);
  }, [trashTargets, armed, arm, execute]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      // While the segment menu is open it owns the keyboard: Esc closes it,
      // every other key is swallowed so the list underneath doesn't act.
      if (menu) {
        if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          setMenu(null);
        }
        return;
      }
      // The path is typed in the search field, so a shortcut must never eat a
      // keystroke meant for it (the weather lesson). Esc still exits from
      // anywhere.
      const tgt = e.target as HTMLElement | null;
      const typing =
        !!tgt && (tgt.tagName === "INPUT" || tgt.tagName === "TEXTAREA" || tgt.isContentEditable);

      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        // An armed delete is cancelled first — Esc must always be "no".
        if (armed) {
          disarm();
          return;
        }
        // Then Esc drills UP one level, then exits at the root (DaisyDisk's
        // back gesture).
        if (!typing && drill.length > 0) setDrill((d) => d.slice(0, -1));
        else onExit();
        return;
      }
      if (typing) return;
      // ⌘⌫ is the Finder's "Move to Trash" and the one chord that survives
      // the modifier guard below; the forward Delete key does the same. Plain
      // ⌫ stays "one level up".
      if (((e.metaKey || e.ctrlKey) && e.key === "Backspace") || e.key === "Delete") {
        e.preventDefault();
        requestTrash();
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === "Backspace" || e.key === "ArrowLeft") {
        e.preventDefault();
        goUp();
      } else if (e.key === " ") {
        // Space collects the selected row (folders and files alike).
        e.preventDefault();
        const row = rowsRef.current[sel];
        if (row) toggleCollect(itemOf([row.index], row.node));
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
  }, [focused, onExit, drill, goUp, run, target, sel, openChild, armed, disarm, requestTrash, toggleCollect, itemOf, menu]);

  // A prune can shorten the list under the selection.
  useEffect(() => {
    if (sel >= rows.length && rows.length > 0) setSel(rows.length - 1);
  }, [rows, sel]);

  const collectedPaths = useMemo(() => new Set(collector.map((i) => i.path)), [collector]);

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
  // The folder has nothing to draw — no arcs, no rows, no collector/trash.
  const isEmpty = rows.length === 0;
  // A genuinely empty scan ROOT of `/home` is almost always the macOS autofs
  // auto_home mount (nobrowse, 0 entries). Rather than a dead-end message, the
  // empty state then offers a button straight to the real home folder. Matched
  // on `root_path` (canonicalised to …/home), not the folder NAME, so a real
  // empty folder literally named "home" gets no false redirect.
  const isHomeMount = drill.length === 0 && /(?:^|\/)home$/.test(scan.root_path);

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

      {/* The sunburst. Fills the preview width up to the viewBox size and
          scales the whole viewBox down on narrower panels, so the outer ring
          is never clipped (the viewBox encloses the arcs by construction).
          A folder with no children at all (e.g. a macOS autofs mount like
          /home, which is genuinely empty) draws no arcs — show a clear empty
          state instead of a lone "0 B" ring, which reads as broken. */}
      {isEmpty ? (
        <EmptyChart node={focusNode!} onGoHome={isHomeMount ? goHome : undefined} />
      ) : (
      <div className="relative mx-auto w-full" style={{ maxWidth: VIEW }}>
        <svg
          viewBox={`0 0 ${VIEW} ${VIEW}`}
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
                onContextMenu={(e) => openMenu(e, itemOf(a.path, a.node))}
                onClick={() => {
                  if (!a.node.is_dir || a.node.other) return;
                  // Deeper rings carry a multi-step path, so this can't go
                  // through `openChild` (which takes one child index).
                  if ((a.node.children?.length ?? 0) > 0) {
                    setDrill([...drill, ...a.path]);
                    setHover(null);
                  } else {
                    const abs = absPathOf(scan, [...drill, ...a.path]);
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
      )}

      {/* Volume readout (DaisyDisk's centre free-space, as a bar below). */}
      {scan.volume_total > 0 && (
        <VolumeBar scan={scan} />
      )}

      {note && <p className="text-[11px] text-emerald-500">{note}</p>}

      {/* Hover / selection detail — a segment goes into the collector from here. */}
      {hover && !hover.node.other && (
        <DetailRow
          arc={hover}
          whole={focusNode!.size}
          collected={collectedPaths.has(absPathOf(scan, [...drill, ...hover.path]) ?? "")}
          onCollect={() => toggleCollect(itemOf(hover.path, hover.node))}
        />
      )}

      <CollectorBar
        items={collector}
        armed={armed && collector.length > 0}
        busy={busy}
        failures={failures}
        onRemove={(path) => { disarm(); setCollector((c) => c.filter((i) => i.path !== path)); }}
        onClear={() => { disarm(); setCollector([]); }}
        onTrash={requestTrash}
        onDismissFailures={() => setFailures([])}
      />

      <ChildList
        rows={rows}
        selected={sel}
        armedRow={armed && collector.length === 0 ? sel : null}
        collected={collectedPaths}
        pathOf={(r) => absPathOf(scan, [...drill, r.index])}
        onSelect={setSel}
        onOpen={(r) => openChild(r.index, r.node)}
        onCollect={(r) => toggleCollect(itemOf([r.index], r.node))}
        reveal={navigatedRef}
      />

      <TopFiles
        scan={scan}
        collected={collectedPaths}
        onCollect={(f) => toggleCollect({ path: f.path, name: baseName(f.path), size: f.size, is_dir: false })}
      />

      <p className="text-[10px] text-[var(--color-muted)]">
        {scan.items.toLocaleString("de-DE")} Einträge gescannt{isEmpty ? "" : " · Klick = reinzoomen"}
      </p>
      {focused && (
        <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">
          {isEmpty
            ? "⌫ höher · R neu scannen · Esc zurück"
            : "⌫ höher · Leertaste sammeln · Rechtsklick: Menü · ⌘⌫ Papierkorb · R neu scannen · Esc zurück"}
        </p>
      )}
      {menu && (
        <SegmentMenu
          menu={menu}
          busy={busy}
          onTrash={() => void execute([menu.item])}
          onClose={() => setMenu(null)}
        />
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
 * it is a full-width row like any other. Each row also carries the collect
 * toggle (＋ → ✓): a row is a `div` holding TWO sibling buttons, never a
 * button inside a button (invalid HTML — the hue lesson).
 */
function ChildList({
  rows,
  selected,
  armedRow,
  collected,
  pathOf,
  onSelect,
  onOpen,
  onCollect,
  reveal,
}: {
  rows: ChildRow[];
  selected: number;
  /** The row an armed single delete would hit (null when the collector is in play). */
  armedRow: number | null;
  collected: Set<string>;
  pathOf: (r: ChildRow) => string | null;
  onSelect: (i: number) => void;
  onOpen: (r: ChildRow) => void;
  onCollect: (r: ChildRow) => void;
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
        Inhalt <span className="text-[var(--color-muted)]">· ↑↓ wählen · Enter öffnen · Leertaste sammeln</span>
      </p>
      <div ref={boxRef} className="flex max-h-[220px] flex-col gap-0.5 overflow-y-auto">
        {rows.map((r, i) => {
          const openable = r.node.is_dir && !r.node.other;
          const path = pathOf(r);
          const isCollected = !!path && collected.has(path);
          const isArmed = armedRow === i;
          return (
            <div
              key={`${r.index}-${r.node.name}`}
              onMouseEnter={() => onSelect(i)}
              className={
                "flex items-center gap-1 rounded text-[11px] " +
                (isArmed
                  ? "bg-red-500/15 ring-1 ring-red-500/60"
                  : i === selected
                    ? "bg-[var(--color-accent)]/15"
                    : "hover:bg-[var(--color-border)]/40")
              }
            >
              <button
                type="button"
                onClick={() => onCollect(r)}
                disabled={!path}
                title={isCollected ? "Aus dem Sammler nehmen (Leertaste)" : "In den Sammler (Leertaste)"}
                aria-pressed={isCollected}
                className={
                  "shrink-0 rounded p-1 disabled:opacity-20 " +
                  (isCollected
                    ? "text-[var(--color-accent)]"
                    : "text-[var(--color-muted)] hover:text-[var(--color-fg)]")
                }
              >
                {isCollected ? <Check size={12} /> : <Plus size={12} />}
              </button>
              <button
                ref={i === selected ? selRef : undefined}
                type="button"
                onClick={() => (openable ? onOpen(r) : onSelect(i))}
                className={
                  "flex min-w-0 flex-1 items-center gap-2 rounded py-1 pr-1.5 text-left " +
                  (openable ? "cursor-pointer" : "cursor-default")
                }
              >
                <span className="shrink-0 text-[var(--color-muted)]">
                  {r.node.is_dir ? <Folder size={12} /> : <FileIcon size={12} />}
                </span>
                <span className="min-w-0 flex-1 truncate" title={r.node.name}>
                  {r.node.other ? "Sonstiges" : r.node.name}
                </span>
                {isArmed ? (
                  <span className="shrink-0 text-[10px] font-medium text-red-500">
                    ⌘⌫ erneut = Papierkorb
                  </span>
                ) : (
                  <>
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
                  </>
                )}
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/**
 * DaisyDisk's collector, keyboard-first. Lists what has been gathered (across
 * drill levels — the items are absolute paths), the running total, and the
 * one button that actually trashes: two-stage, and it says so.
 */
function CollectorBar({
  items,
  armed,
  busy,
  failures,
  onRemove,
  onClear,
  onTrash,
  onDismissFailures,
}: {
  items: CollectorItem[];
  armed: boolean;
  busy: boolean;
  failures: { path: string; error: string }[];
  onRemove: (path: string) => void;
  onClear: () => void;
  onTrash: () => void;
  onDismissFailures: () => void;
}) {
  if (items.length === 0 && failures.length === 0) return null;
  const tot = collectorTotals(items);
  return (
    <div
      className={
        "rounded-xl border p-3 [contain:content] " +
        (armed ? "border-red-500/60 bg-red-500/5" : "border-[var(--color-accent)]/40")
      }
    >
      {items.length > 0 && (
        <>
          <div className="mb-2 flex items-center justify-between gap-2 text-[11px]">
            <span className="font-medium">
              Sammler{" "}
              <span className="text-[var(--color-muted)] tabular-nums">
                · {tot.count === 1 ? "1 Eintrag" : `${tot.count} Einträge`} · {formatBytes(tot.bytes)}
              </span>
            </span>
            <button
              type="button"
              onClick={onClear}
              className="rounded px-1 text-[10px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            >
              leeren
            </button>
          </div>
          <div className="mb-2 flex max-h-[120px] flex-col gap-0.5 overflow-y-auto">
            {items.map((i) => (
              <div key={i.path} className="flex items-center gap-2 text-[11px]">
                <span className="shrink-0 text-[var(--color-muted)]">
                  {i.is_dir ? <Folder size={12} /> : <FileIcon size={12} />}
                </span>
                <span className="min-w-0 flex-1 truncate" title={i.path}>{i.name}</span>
                <span className="shrink-0 tabular-nums text-[var(--color-muted)]">{formatBytes(i.size)}</span>
                <button
                  type="button"
                  onClick={() => onRemove(i.path)}
                  title="Aus dem Sammler nehmen"
                  className="shrink-0 rounded p-0.5 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                >
                  <X size={11} />
                </button>
              </div>
            ))}
          </div>
          <button
            type="button"
            onClick={onTrash}
            disabled={busy}
            className={
              "flex w-full items-center justify-center gap-1.5 rounded-lg px-2 py-1.5 text-[11px] font-medium disabled:opacity-50 " +
              (armed
                ? "bg-red-500 text-white"
                : "bg-[var(--color-accent)]/15 text-[var(--color-fg)] hover:bg-[var(--color-accent)]/25")
            }
          >
            <Trash2 size={12} />
            {busy
              ? "Verschiebe…"
              : armed
                ? `Nochmal ⌘⌫ oder klicken: ${tot.count === 1 ? "1 Eintrag" : `${tot.count} Einträge`} in den Papierkorb`
                : "In den Papierkorb (⌘⌫)"}
          </button>
        </>
      )}
      {failures.length > 0 && (
        <div className="mt-2 rounded-lg border border-amber-500/50 bg-amber-500/10 p-2 text-[11px]">
          <div className="mb-1 flex items-center justify-between">
            <span className="font-medium">Nicht verschoben</span>
            <button type="button" onClick={onDismissFailures} className="rounded p-0.5 text-[var(--color-muted)] hover:text-[var(--color-fg)]">
              <X size={11} />
            </button>
          </div>
          {failures.map((f, k) => (
            <p key={`${f.path}-${k}`} className="truncate text-[var(--color-muted)]" title={f.path}>
              {f.path ? `${baseName(f.path)}: ` : ""}{f.error}
            </p>
          ))}
        </div>
      )}
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

function DetailRow({
  arc,
  whole,
  collected,
  onCollect,
}: {
  arc: Arc;
  whole: number;
  collected: boolean;
  onCollect: () => void;
}) {
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
        onClick={onCollect}
        title={collected ? "Aus dem Sammler nehmen" : "In den Sammler"}
        aria-pressed={collected}
        className={
          "shrink-0 rounded p-1 " +
          (collected ? "text-[var(--color-accent)]" : "text-[var(--color-muted)] hover:text-[var(--color-fg)]")
        }
      >
        {collected ? <Check size={12} /> : <Plus size={12} />}
      </button>
    </div>
  );
}

function TopFiles({
  scan,
  collected,
  onCollect,
}: {
  scan: DiskScan;
  collected: Set<string>;
  onCollect: (f: { path: string; size: number }) => void;
}) {
  if (scan.top_files.length === 0) return null;
  const max = scan.top_files[0].size || 1;
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
      <p className="mb-2 text-[11px] font-medium">Größte Dateien</p>
      <div className="flex flex-col gap-1">
        {scan.top_files.slice(0, 12).map((f) => {
          const isCollected = collected.has(f.path);
          return (
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
                onClick={() => onCollect(f)}
                title={isCollected ? "Aus dem Sammler nehmen" : "In den Sammler"}
                aria-pressed={isCollected}
                className={
                  "shrink-0 rounded p-0.5 transition-opacity " +
                  (isCollected
                    ? "text-[var(--color-accent)] opacity-100"
                    : "text-[var(--color-muted)] opacity-0 hover:text-[var(--color-fg)] group-hover:opacity-100")
                }
              >
                {isCollected ? <Check size={11} /> : <Plus size={11} />}
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/**
 * Shown when the scanned/drilled folder has no children at all. An empty folder
 * (e.g. a macOS autofs mount like `/home`, which is genuinely 0 B) draws no
 * arcs, so the sunburst would otherwise be a lone "0 B" ring — which reads as
 * broken. The header, path bar and ↑ button stay, so the user sees where they
 * are and can walk back out. When it's the `/home` autofs case, `onGoHome`
 * turns the dead-end into a one-click jump to the real home folder.
 */
function EmptyChart({ node, onGoHome }: { node: DiskNode; onGoHome?: () => void }) {
  return (
    <div
      className="mx-auto flex w-full flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-[var(--color-border)] px-6 py-12 text-center"
      style={{ maxWidth: VIEW }}
    >
      <div className="flex h-16 w-16 items-center justify-center rounded-full bg-[var(--color-surface)] text-[var(--color-muted)]">
        <FolderOpen size={28} />
      </div>
      <div className="space-y-0.5">
        <p className="text-[14px] font-semibold">Dieser Ordner ist leer</p>
        <p className="text-[11px] text-[var(--color-muted)] tabular-nums">
          Keine Einträge · {formatBytes(node.size)}
        </p>
      </div>
      {onGoHome ? (
        <>
          <p className="max-w-[300px] text-[11px] leading-relaxed text-[var(--color-muted)]">
            Auf macOS ist <code className="rounded bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">/home</code> nur
            ein leerer System-Mount. Dein persönlicher Ordner liegt unter{" "}
            <code className="rounded bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">/Users</code>.
          </p>
          <button
            type="button"
            onClick={onGoHome}
            className="mt-1 inline-flex items-center gap-1.5 rounded-lg bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)] transition-opacity hover:opacity-90"
          >
            <Home size={14} /> Persönlichen Ordner öffnen
          </button>
        </>
      ) : (
        <p className="max-w-[280px] text-[11px] leading-relaxed text-[var(--color-muted)]">
          Keine Dateien oder Unterordner in „{node.name}“.
        </p>
      )}
    </div>
  );
}

/**
 * Right-click context menu for a ring segment. Shows what was hit (name, size,
 * full path) and moves it to the Trash on click — Finder's secondary-click
 * model: the Trash is recoverable, so the right-click plus the menu click IS
 * the deliberate act (no extra confirm; user decision 2026-09-15). Closes on
 * Esc (handled by the panel's key handler, which owns the keyboard while the
 * menu is open), on a mousedown outside the menu, or after the action.
 * Positioned at the cursor and clamped into the viewport.
 */
function SegmentMenu({
  menu,
  busy,
  onTrash,
  onClose,
}: {
  menu: { x: number; y: number; item: CollectorItem };
  busy: boolean;
  onTrash: () => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: menu.x, top: menu.y });

  // Clamp into the viewport once the real menu size is known.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const pad = 8;
    const left = Math.max(pad, Math.min(menu.x, window.innerWidth - r.width - pad));
    const top = Math.max(pad, Math.min(menu.y, window.innerHeight - r.height - pad));
    setPos({ left, top });
  }, [menu.x, menu.y]);

  // A mousedown outside closes it. A right-click on another segment lands here
  // first (closing this one), then the panel re-opens a fresh menu there — so
  // the menu appears to move to the newly-clicked segment.
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    window.addEventListener("mousedown", onDown, true);
    return () => window.removeEventListener("mousedown", onDown, true);
  }, [onClose]);

  const { item } = menu;
  return (
    <div
      ref={ref}
      role="menu"
      className="fixed z-50 min-w-[200px] max-w-[300px] overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-1 text-[11px] shadow-xl"
      style={{ left: pos.left, top: pos.top }}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      <div className="flex items-center gap-1.5 px-2 pb-1 pt-1.5">
        <span className="shrink-0 text-[var(--color-muted)]">
          {item.is_dir ? <Folder size={12} /> : <FileIcon size={12} />}
        </span>
        <span className="min-w-0 flex-1 truncate font-medium" title={item.name}>
          {item.name}
        </span>
        <span className="shrink-0 tabular-nums text-[var(--color-muted)]">{formatBytes(item.size)}</span>
      </div>
      <div
        className="truncate px-2 pb-1.5 font-[var(--font-mono)] text-[10px] text-[var(--color-muted)]"
        title={item.path}
      >
        {item.path}
      </div>
      <div className="mx-1 mb-1 h-px bg-[var(--color-border)]" />
      <button
        type="button"
        role="menuitem"
        disabled={busy}
        onClick={() => {
          onTrash();
          onClose();
        }}
        className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-[var(--color-fg)] hover:bg-red-500/15 hover:text-red-500 disabled:opacity-50"
      >
        <Trash2 size={13} className="shrink-0" />
        {item.is_dir ? "Ordner in den Papierkorb" : "Datei in den Papierkorb"}
      </button>
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
