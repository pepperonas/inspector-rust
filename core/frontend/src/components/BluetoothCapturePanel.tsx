import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  Bluetooth,
  Copy,
  Download,
  FileUp,
  Pause,
  Play,
  Radio,
  Search,
  Square,
  Trash2,
  X,
} from "lucide-react";
import { confirmDialog } from "../lib/confirm";
import {
  btsniffDefaultFilename,
  btsniffExportJson,
  btsniffLiveClear,
  btsniffLiveExportJson,
  btsniffLivePacket,
  btsniffLivePause,
  btsniffLiveResume,
  btsniffLiveStart,
  btsniffLiveStop,
  btsniffOpenFile,
  btsniffPacket,
  btsniffSetupStatus,
  setSuppressHide,
} from "../lib/ipc";
import {
  BT_FILTERS,
  decodedText,
  defaultCaptureFilename,
  diffChanged,
  directionGlyph,
  directionLabel,
  filterPackets,
  formatBytes,
  formatDuration,
  formatHandle,
  formatLiveDuration,
  formatRelTime,
  hexString,
  isGattOp,
  isLiveRunning,
  protocolTag,
  type BtFilter,
  type BtPacketDetail,
  type BtPacketSlim,
  type CaptureStats,
  type LiveStatus,
  type OpenResult,
  type SetupStatus,
} from "../lib/bluetooth-capture";

interface Props {
  focused: boolean;
  onExit: () => void;
}

type View = "idle" | "analyzer" | "live";

const ROW_HEIGHT = 34;
const PRIVACY_KEY = "btsniff-privacy-ack";
const CAPTURE_FILTERS = [
  { name: "Bluetooth-Capture", extensions: ["pklg", "log", "snoop", "btsnoop", "pcapng", "pcap", "cap"] },
  { name: "Alle Dateien", extensions: ["*"] },
];

export function BluetoothCapturePanel({ focused, onExit }: Props) {
  const [view, setView] = useState<View>("idle");
  const [filter, setFilter] = useState<BtFilter>("all");
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<number | null>(null);
  const [detail, setDetail] = useState<BtPacketDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);

  // analyzer (file) mode
  const [result, setResult] = useState<OpenResult | null>(null);

  // live mode
  const [setup, setSetup] = useState<SetupStatus | null>(null);
  const [livePackets, setLivePackets] = useState<BtPacketSlim[]>([]);
  const [liveStatus, setLiveStatus] = useState<LiveStatus | null>(null);
  const [filename, setFilename] = useState(() => defaultCaptureFilename(new Date()));
  const [autoScroll, setAutoScroll] = useState(true);
  const [newCount, setNewCount] = useState(0);
  const [clock, setClock] = useState(0);
  const [privacyAck, setPrivacyAck] = useState(true);

  const containerRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const mountedRef = useRef(true);
  const unlistenRef = useRef<UnlistenFn[]>([]);
  const autoScrollRef = useRef(true);
  const liveStateRef = useRef<LiveStatus["state"]>("idle");

  const packets = useMemo(
    () => (view === "live" ? livePackets : (result?.packets ?? [])),
    [view, livePackets, result],
  );
  const stats: CaptureStats | undefined =
    view === "live" ? liveStatus?.stats : result?.stats;
  const filtered = useMemo(
    () => filterPackets(packets, filter, search),
    [packets, filter, search],
  );

  const virtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => listRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  // ── setup detection (§16) + first-run privacy hint (§21) ──
  useEffect(() => {
    btsniffSetupStatus().then(setSetup).catch(() => {});
    try {
      setPrivacyAck(localStorage.getItem(PRIVACY_KEY) === "1");
    } catch {
      setPrivacyAck(false);
    }
  }, []);

  const ackPrivacy = () => {
    setPrivacyAck(true);
    try {
      localStorage.setItem(PRIVACY_KEY, "1");
    } catch {
      /* ignore */
    }
  };

  // ── live clock (animates between the ~120 ms status ticks) ──
  useEffect(() => {
    if (view !== "live" || !liveStatus || !isLiveRunning(liveStatus.state)) return;
    const id = window.setInterval(() => setClock(Date.now()), 100);
    return () => window.clearInterval(id);
  }, [view, liveStatus]);

  const teardownListeners = useCallback(() => {
    unlistenRef.current.forEach((u) => u());
    unlistenRef.current = [];
  }, []);

  // ── analyzer import ──
  const importPath = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    try {
      const r = await btsniffOpenFile(path);
      setResult(r);
      setView("analyzer");
      setSelected(null);
      setDetail(null);
      setFilter("all");
      setSearch("");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const onPickFile = useCallback(async () => {
    await setSuppressHide(true).catch(() => {});
    try {
      const picked = await openDialog({
        multiple: false,
        directory: false,
        filters: CAPTURE_FILTERS,
        title: "Bluetooth-Capture öffnen",
      });
      if (typeof picked === "string") await importPath(picked);
    } catch (e) {
      setError(String(e));
    } finally {
      await setSuppressHide(false).catch(() => {});
    }
  }, [importPath]);

  // ── live capture ──
  const startLive = useCallback(async () => {
    setError(null);
    setLivePackets([]);
    setSelected(null);
    setDetail(null);
    setFilter("all");
    setSearch("");
    setNewCount(0);
    setAutoScroll(true);
    autoScrollRef.current = true;
    setView("live");

    teardownListeners();
    const u1 = await listen<BtPacketSlim[]>("btsniff-packet", (ev) => {
      const batch = ev.payload;
      if (!batch.length) return;
      setLivePackets((prev) => [...prev, ...batch]);
      if (!autoScrollRef.current) setNewCount((c) => c + batch.length);
    });
    const u2 = await listen<LiveStatus>("btsniff-state", (ev) => {
      const st = ev.payload;
      setLiveStatus(st);
      liveStateRef.current = st.state;
      if (st.state === "error" && st.error) setError(st.error);
    });
    // Closed while the listeners were attaching → never start (§20: a capture
    // must not keep running unnoticed). The unmount cleanup ran before these
    // listeners existed, so they are released here.
    if (!mountedRef.current) {
      u1();
      u2();
      return;
    }
    unlistenRef.current = [u1, u2];

    try {
      const st = await btsniffLiveStart();
      if (!mountedRef.current) {
        // Closed while the start was in flight: the cleanup saw an idle
        // state and stopped nothing — stop the capture that just began.
        void btsniffLiveStop().catch(() => {});
        return;
      }
      setLiveStatus(st);
      liveStateRef.current = st.state;
      if (st.state === "error" && st.error) setError(st.error);
    } catch (e) {
      setError(String(e));
    }
  }, [teardownListeners]);

  const pauseLive = useCallback(async () => {
    try {
      setLiveStatus(await btsniffLivePause());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const resumeLive = useCallback(async () => {
    try {
      setLiveStatus(await btsniffLiveResume());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const stopLive = useCallback(async () => {
    try {
      const st = await btsniffLiveStop();
      setLiveStatus(st);
      liveStateRef.current = st.state;
    } catch (e) {
      setError(String(e));
    }
    teardownListeners();
  }, [teardownListeners]);

  const clearLive = useCallback(async () => {
    if (livePackets.length > 0) {
      const ok = await confirmDialog(
        "Aktuelle Darstellung leeren? Nicht gespeicherte Pakete gehen verloren.",
        "Capture leeren",
      );
      if (!ok) return;
    }
    try {
      setLiveStatus(await btsniffLiveClear());
    } catch {
      /* ignore */
    }
    setLivePackets([]);
    setSelected(null);
    setDetail(null);
    setNewCount(0);
  }, [livePackets.length]);

  const newCapture = useCallback(() => {
    setView("idle");
    setResult(null);
    setLivePackets([]);
    setLiveStatus(null);
    setSelected(null);
    setDetail(null);
    setError(null);
    setFilename(defaultCaptureFilename(new Date()));
  }, []);

  // Toggle pause/resume from the keyboard (Space).
  const togglePause = useCallback(() => {
    const s = liveStatus?.state;
    if (s === "capturing") void pauseLive();
    else if (s === "paused_view") void resumeLive();
  }, [liveStatus?.state, pauseLive, resumeLive]);

  // ── save / export ──
  const saveCapture = useCallback(async () => {
    await setSuppressHide(true).catch(() => {});
    try {
      const stem = filename.replace(/\.[^.]+$/, "") || "inspector-bluetooth";
      const suggested =
        view === "analyzer" ? await btsniffDefaultFilename() : `${stem}.json`;
      const target = await saveDialog({
        title: "Analyse als JSON speichern",
        defaultPath: suggested,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (typeof target === "string") {
        if (view === "analyzer") await btsniffExportJson(target);
        else await btsniffLiveExportJson(target);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      await setSuppressHide(false).catch(() => {});
    }
  }, [filename, view]);

  // ── drag-drop import (works from idle too) ──
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebviewWindow()
      .onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter" || p.type === "over") {
          void setSuppressHide(true).catch(() => {});
          setDragOver(true);
        } else if (p.type === "leave") {
          setDragOver(false);
          void setSuppressHide(false).catch(() => {});
        } else if (p.type === "drop") {
          setDragOver(false);
          void setSuppressHide(false).catch(() => {});
          const path = p.paths[0];
          if (path) void importPath(path);
        }
      })
      .then((un) => {
        if (cancelled) un();
        else unlisten = un;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
      void setSuppressHide(false).catch(() => {});
    };
  }, [importPath]);

  // ── cleanup: never leave a capture running unnoticed (§20) ──
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (isLiveRunning(liveStateRef.current)) {
        void btsniffLiveStop().catch(() => {});
      }
      unlistenRef.current.forEach((u) => u());
      unlistenRef.current = [];
    };
  }, []);

  // ── selection → detail (mode-aware source) ──
  useEffect(() => {
    if (selected == null) {
      setDetail(null);
      return;
    }
    let cancelled = false;
    const fetch = view === "live" ? btsniffLivePacket : btsniffPacket;
    fetch(selected)
      .then((d) => {
        if (!cancelled) setDetail(d);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [selected, view]);

  useEffect(() => {
    if (focused) containerRef.current?.focus();
  }, [focused]);

  // ── auto-scroll to newest while following ──
  useEffect(() => {
    if (view === "live" && autoScroll && filtered.length > 0) {
      virtualizer.scrollToIndex(filtered.length - 1, { align: "end" });
    }
  }, [view, autoScroll, filtered.length, virtualizer]);

  const onListScroll = useCallback(() => {
    const el = listRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
    autoScrollRef.current = atBottom;
    setAutoScroll(atBottom);
    if (atBottom) setNewCount(0);
  }, []);

  const jumpToLatest = useCallback(() => {
    autoScrollRef.current = true;
    setAutoScroll(true);
    setNewCount(0);
    if (filtered.length > 0) virtualizer.scrollToIndex(filtered.length - 1, { align: "end" });
  }, [filtered.length, virtualizer]);

  const selectedPos = useMemo(
    () => (selected == null ? -1 : filtered.findIndex((p) => p.index === selected)),
    [filtered, selected],
  );

  const selectByPos = useCallback(
    (pos: number) => {
      if (filtered.length === 0) return;
      const clamped = Math.max(0, Math.min(pos, filtered.length - 1));
      setSelected(filtered[clamped].index);
      virtualizer.scrollToIndex(clamped, { align: "auto" });
    },
    [filtered, virtualizer],
  );

  const copy = useCallback(async (text: string, label: string) => {
    try {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(text);
      setCopied(label);
      window.setTimeout(() => setCopied(null), 1400);
    } catch {
      /* clipboard unavailable */
    }
  }, []);

  const running = liveStatus ? isLiveRunning(liveStatus.state) : false;
  const stopped = view === "live" && liveStatus?.state === "stopped";
  const canSave = view === "analyzer" || stopped;

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const inSearch = e.target === searchRef.current;
    const inInput = inSearch || (e.target as HTMLElement)?.tagName === "INPUT";
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (inInput) {
        (e.target as HTMLElement).blur();
        containerRef.current?.focus();
      } else if (detail) {
        setSelected(null);
      } else {
        onExit();
      }
      return;
    }
    if (e.key === "f" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      searchRef.current?.focus();
      return;
    }
    if (e.key === "s" && (e.metaKey || e.ctrlKey) && canSave) {
      e.preventDefault();
      void saveCapture();
      return;
    }
    if (inInput) return;
    if (e.key === " " && running) {
      e.preventDefault();
      togglePause();
      return;
    }
    if (e.key === "/") {
      e.preventDefault();
      searchRef.current?.focus();
      return;
    }
    const base = selectedPos < 0 ? 0 : selectedPos;
    switch (e.key) {
      case "ArrowDown":
      case "j":
        e.preventDefault();
        selectByPos(selectedPos < 0 ? 0 : base + 1);
        break;
      case "ArrowUp":
      case "k":
        e.preventDefault();
        selectByPos(base - 1);
        break;
      case "PageDown":
        e.preventDefault();
        selectByPos(base + 12);
        break;
      case "PageUp":
        e.preventDefault();
        selectByPos(base - 12);
        break;
      case "Home":
        e.preventDefault();
        selectByPos(0);
        break;
      case "End":
        e.preventDefault();
        selectByPos(filtered.length - 1);
        break;
    }
  };

  const durationMs = liveStatus
    ? (running ? Math.max(0, clock - liveStatus.started_at_ms) : liveStatus.stats.duration_us / 1000)
    : 0;

  return (
    <div
      ref={containerRef}
      tabIndex={-1}
      onKeyDown={onKeyDown}
      className="relative flex h-full flex-col gap-2 p-3 text-[var(--color-fg)] outline-none [contain:paint]"
    >
      {/* header */}
      <div className="flex shrink-0 items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2 text-[13px] font-medium">
          <Bluetooth className="h-4 w-4 shrink-0 text-[var(--color-accent)]" />
          <span className="truncate">
            {view === "analyzer" && result ? result.source_label : "Bluetooth Capture"}
          </span>
          {view === "analyzer" && result && (
            <span className="shrink-0 rounded bg-[var(--color-accent)]/15 px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-[var(--color-accent)]">
              {result.format}
            </span>
          )}
          {view === "live" && liveStatus && <LiveBadge status={liveStatus} durationMs={durationMs} />}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {view !== "idle" && (
            <button
              onClick={newCapture}
              title="Neuer Capture"
              className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            >
              <FileUp className="h-4 w-4" />
            </button>
          )}
          <button
            onClick={() => void saveCapture()}
            disabled={!canSave}
            title="Analyse als JSON speichern (⌘/Ctrl+S)"
            className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)] disabled:opacity-30"
          >
            <Download className="h-4 w-4" />
          </button>
        </div>
      </div>

      {error && (
        <div className="md3-banner-in shrink-0 rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-[11px] leading-snug">
          {error}
        </div>
      )}

      {/* ── IDLE ── */}
      {view === "idle" && !busy && (
        <IdleView
          setup={setup}
          filename={filename}
          onFilename={setFilename}
          onStart={() => void startLive()}
          onImport={() => void onPickFile()}
          privacyAck={privacyAck}
          onAckPrivacy={ackPrivacy}
        />
      )}

      {busy && (
        <div className="flex flex-1 items-center justify-center text-[12px] text-[var(--color-muted)]">
          Analysiere …
        </div>
      )}

      {/* ── LIVE / ANALYZER (shared timeline) ── */}
      {(view === "analyzer" || view === "live") && !busy && (
        <>
          {view === "live" && (
            <LiveControls
              status={liveStatus}
              onPause={() => void pauseLive()}
              onResume={() => void resumeLive()}
              onStop={() => void stopLive()}
              onClear={() => void clearLive()}
            />
          )}

          {/* filters + search */}
          <div className="flex shrink-0 flex-col gap-1.5">
            <div className="flex gap-1 overflow-x-auto pb-0.5 [scrollbar-width:none]">
              {BT_FILTERS.map((f) => (
                <button
                  key={f.id}
                  onClick={() => setFilter(f.id)}
                  className={`shrink-0 rounded-full px-2 py-0.5 text-[10px] font-medium transition-colors ${
                    filter === f.id
                      ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                      : "bg-[var(--color-surface)] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                  }`}
                >
                  {f.label}
                </button>
              ))}
            </div>
            <div className="flex items-center gap-1.5 rounded-md border border-[var(--color-border)] px-2 py-1">
              <Search className="h-3.5 w-3.5 shrink-0 text-[var(--color-muted)]" />
              <input
                ref={searchRef}
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Suchen (Opcode, Handle, UUID …)"
                className="min-w-0 flex-1 bg-transparent text-[11px] outline-none placeholder:text-[var(--color-muted)]"
              />
              {search && (
                <button
                  onClick={() => setSearch("")}
                  className="shrink-0 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                >
                  <X className="h-3 w-3" />
                </button>
              )}
            </div>
          </div>

          {/* timeline */}
          <div className="relative min-h-0 flex-1">
            <div
              ref={listRef}
              onScroll={view === "live" ? onListScroll : undefined}
              className="h-full overflow-y-auto rounded-md border border-[var(--color-border)]"
            >
              {filtered.length === 0 ? (
                <div className="flex h-full items-center justify-center px-4 text-center text-[11px] text-[var(--color-muted)]">
                  {view === "live" && running
                    ? "Warte auf Pakete … (Geräte in der Nähe müssen aktiv senden)"
                    : "Keine Pakete für diesen Filter"}
                </div>
              ) : (
                <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
                  {virtualizer.getVirtualItems().map((vr) => {
                    const p = filtered[vr.index];
                    return (
                      <PacketRow
                        key={p.index}
                        p={p}
                        selected={p.index === selected}
                        style={{
                          position: "absolute",
                          top: 0,
                          left: 0,
                          width: "100%",
                          height: ROW_HEIGHT,
                          transform: `translateY(${vr.start}px)`,
                        }}
                        onSelect={() => setSelected(p.index)}
                      />
                    );
                  })}
                </div>
              )}
            </div>
            {view === "live" && !autoScroll && newCount > 0 && (
              <button
                onClick={jumpToLatest}
                className="md3-banner-in absolute bottom-2 left-1/2 -translate-x-1/2 rounded-full bg-[var(--color-accent)] px-3 py-1 text-[10px] font-medium text-[var(--color-accent-fg)] shadow-lg"
              >
                ↓ {newCount} neue Pakete
              </button>
            )}
          </div>

          {/* inspector or summary */}
          <div className="shrink-0 overflow-y-auto rounded-md border border-[var(--color-border)] p-2 [max-height:42%]">
            {detail ? (
              <PacketInspector
                detail={detail}
                copied={copied}
                onCopyHex={() => void copy(hexString(detail.raw), "Hex")}
                onCopyDecoded={() => void copy(decodedText(detail.decoded), "Info")}
                onClose={() => setSelected(null)}
              />
            ) : (
              stats && <SessionSummary stats={stats} dropped={liveStatus?.dropped ?? 0} />
            )}
          </div>
        </>
      )}

      {dragOver && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center rounded-lg border-2 border-dashed border-[var(--color-accent)] bg-[var(--color-bg)]/80 text-[13px] font-medium text-[var(--color-accent)]">
          Zum Importieren ablegen
        </div>
      )}
    </div>
  );
}

function LiveBadge({ status, durationMs }: { status: LiveStatus; durationMs: number }) {
  const paused = status.state === "paused_view";
  const capturing = status.state === "capturing";
  const label = capturing ? "LIVE" : paused ? "PAUSED" : status.state === "stopped" ? "STOPPED" : status.state.toUpperCase();
  const color = capturing
    ? "text-red-400"
    : paused
      ? "text-amber-400"
      : "text-[var(--color-muted)]";
  return (
    <span className={`flex shrink-0 items-center gap-1 font-[var(--font-mono)] text-[10px] ${color}`}>
      <span
        className={`inline-block h-2 w-2 rounded-full ${capturing ? "bg-red-500 motion-safe:animate-pulse" : paused ? "bg-amber-500" : "bg-[var(--color-muted)]"}`}
      />
      {label}
      {paused ? (
        <span className="text-[var(--color-muted)]">· {status.buffered} gepuffert</span>
      ) : (
        <span className="tabular-nums text-[var(--color-muted)]">{formatLiveDuration(durationMs)}</span>
      )}
    </span>
  );
}

function LiveControls({
  status,
  onPause,
  onResume,
  onStop,
  onClear,
}: {
  status: LiveStatus | null;
  onPause: () => void;
  onResume: () => void;
  onStop: () => void;
  onClear: () => void;
}) {
  const s = status?.state;
  const running = s === "capturing" || s === "paused_view" || s === "starting";
  const btn =
    "flex items-center gap-1 rounded-md border border-[var(--color-border)] px-2 py-1 text-[11px] hover:bg-[var(--color-surface)] disabled:opacity-30";
  return (
    <div className="flex shrink-0 items-center gap-1.5">
      {s === "paused_view" ? (
        <button onClick={onResume} className={btn}>
          <Play className="h-3 w-3" /> Fortsetzen
        </button>
      ) : (
        <button onClick={onPause} disabled={s !== "capturing"} className={btn}>
          <Pause className="h-3 w-3" /> Pause
        </button>
      )}
      <button onClick={onStop} disabled={!running} className={btn}>
        <Square className="h-3 w-3" /> Stop
      </button>
      <button onClick={onClear} className={btn}>
        <Trash2 className="h-3 w-3" /> Leeren
      </button>
    </div>
  );
}

function IdleView({
  setup,
  filename,
  onFilename,
  onStart,
  onImport,
  privacyAck,
  onAckPrivacy,
}: {
  setup: SetupStatus | null;
  filename: string;
  onFilename: (v: string) => void;
  onStart: () => void;
  onImport: () => void;
  privacyAck: boolean;
  onAckPrivacy: () => void;
}) {
  const unsupported = setup?.availability === "unsupported_platform";
  return (
    <div className="flex flex-1 flex-col gap-3 overflow-y-auto px-1 py-2">
      {!privacyAck && (
        <div className="md3-banner-in flex items-start gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-[10px] leading-relaxed text-[var(--color-muted)]">
          <span className="flex-1">
            Bluetooth-Captures können Gerätekennungen und Nutzdaten enthalten. Sie
            bleiben lokal, solange du sie nicht ausdrücklich exportierst.
          </span>
          <button onClick={onAckPrivacy} className="shrink-0 text-[var(--color-fg)] hover:opacity-70">
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      )}

      <div className="flex items-center gap-2">
        <Radio className="h-4 w-4 text-[var(--color-accent)]" />
        <span className="text-[12px] font-medium">Live-Capture</span>
        <span className="ml-auto flex items-center gap-1 font-[var(--font-mono)] text-[10px] text-[var(--color-muted)]">
          <span className="inline-block h-2 w-2 rounded-full bg-emerald-500" /> READY
        </span>
      </div>

      <label className="flex flex-col gap-1">
        <span className="text-[10px] uppercase tracking-wide text-[var(--color-muted)]">Quelle</span>
        <div className="rounded-md border border-[var(--color-border)] px-2 py-1.5 text-[11px] text-[var(--color-muted)]">
          {setup?.source_label ?? "System Bluetooth"}
        </div>
      </label>

      <label className="flex flex-col gap-1">
        <span className="text-[10px] uppercase tracking-wide text-[var(--color-muted)]">Capture-Datei</span>
        <input
          value={filename}
          onChange={(e) => onFilename(e.target.value)}
          spellCheck={false}
          className="rounded-md border border-[var(--color-border)] bg-transparent px-2 py-1.5 font-[var(--font-mono)] text-[11px] outline-none focus:border-[var(--color-accent)]"
        />
        <span className="text-[9px] text-[var(--color-muted)]">
          Es wird nichts automatisch geschrieben. Gespeichert wird die dekodierte
          Analyse als JSON (.pklg-Schreiben ist geplant).
        </span>
      </label>

      {unsupported ? (
        <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-[11px] leading-relaxed text-[var(--color-muted)]">
          {setup?.message}
        </div>
      ) : (
        <button
          onClick={onStart}
          className="flex items-center justify-center gap-1.5 rounded-lg border border-[var(--color-accent)]/40 bg-[var(--color-accent)]/10 px-3 py-2 text-[12px] font-medium text-[var(--color-fg)] hover:bg-[var(--color-accent)]/20"
        >
          <span className="inline-block h-2.5 w-2.5 rounded-full bg-red-500" />
          Capture starten
        </button>
      )}
      {setup && !unsupported && (
        <p className="text-[9px] leading-snug text-[var(--color-muted)]">{setup.message}</p>
      )}

      <div className="grid grid-cols-4 gap-2 rounded-md border border-[var(--color-border)] p-2 text-[11px]">
        {(
          [
            ["Pakete", "0"],
            ["RX", "0 B"],
            ["TX", "0 B"],
            ["Dauer", "00:00.000"],
          ] as const
        ).map(([k, v]) => (
          <div key={k} className="flex flex-col">
            <span className="text-[9px] uppercase tracking-wide text-[var(--color-muted)]">{k}</span>
            <span className="tabular-nums">{v}</span>
          </div>
        ))}
      </div>

      <button
        onClick={onImport}
        className="flex items-center justify-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[11px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
      >
        <FileUp className="h-3.5 w-3.5" /> Capture-Datei importieren (.pklg / btsnoop / pcapng)
      </button>
    </div>
  );
}

function PacketRow({
  p,
  selected,
  style,
  onSelect,
}: {
  p: BtPacketSlim;
  selected: boolean;
  style: React.CSSProperties;
  onSelect: () => void;
}) {
  const tx = p.direction === "tx";
  const gatt = p.protocol === "att" && isGattOp(p.opcode);
  return (
    <button
      style={style}
      onClick={onSelect}
      className={`flex items-center gap-1.5 border-b border-[var(--color-border)]/40 px-2 text-left font-[var(--font-mono)] text-[10px] ${
        selected ? "bg-[var(--color-accent)]/20" : "hover:bg-[var(--color-surface)]"
      }`}
    >
      <span className="w-[46px] shrink-0 tabular-nums text-[var(--color-muted)]">
        {formatRelTime(p.t_us)}
      </span>
      <span
        className={`w-3 shrink-0 text-center ${tx ? "text-sky-400" : p.direction === "rx" ? "text-amber-400" : "text-[var(--color-muted)]"}`}
        title={directionLabel(p.direction)}
      >
        {directionGlyph(p.direction)}
      </span>
      <span
        className={`w-[42px] shrink-0 rounded px-1 text-center text-[9px] uppercase ${
          gatt
            ? "bg-[var(--color-accent)]/25 font-semibold text-[var(--color-accent)]"
            : "text-[var(--color-muted)]"
        }`}
      >
        {protocolTag(p.protocol)}
      </span>
      <span className="w-[46px] shrink-0 tabular-nums text-[var(--color-muted)]">
        {formatHandle(p.att_handle ?? p.handle)}
      </span>
      <span className="min-w-0 flex-1 truncate text-[var(--color-fg)]">{p.summary}</span>
    </button>
  );
}

function PacketInspector({
  detail,
  copied,
  onCopyHex,
  onCopyDecoded,
  onClose,
}: {
  detail: BtPacketDetail;
  copied: string | null;
  onCopyHex: () => void;
  onCopyDecoded: () => void;
  onClose: () => void;
}) {
  const perRow = 8;
  const rows: number[][] = [];
  for (let i = 0; i < detail.raw.length; i += perRow) {
    rows.push(detail.raw.slice(i, i + perRow));
  }
  return (
    <div className="flex flex-col gap-2 text-[11px]">
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-1.5 font-medium">
          <span className="text-[var(--color-accent)]">#{detail.index}</span>
          <span className="truncate">{detail.opcode ?? protocolTag(detail.protocol)}</span>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <button
            onClick={onCopyHex}
            title="Hex kopieren"
            className="flex items-center gap-0.5 rounded px-1 py-0.5 text-[9px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          >
            <Copy className="h-3 w-3" /> Hex
          </button>
          <button
            onClick={onCopyDecoded}
            title="Dekodiert kopieren"
            className="flex items-center gap-0.5 rounded px-1 py-0.5 text-[9px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          >
            <Copy className="h-3 w-3" /> Info
          </button>
          <button
            onClick={onClose}
            className="rounded p-0.5 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>

      {copied && <div className="text-[9px] text-[var(--color-accent)]">{copied} kopiert ✓</div>}

      {detail.decoded.length > 0 && (
        <div className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5">
          {detail.decoded.map(([k, v], i) => (
            <div key={i} className="contents">
              <span className="text-[var(--color-muted)]">{k}</span>
              <span className="font-[var(--font-mono)] tabular-nums">{v}</span>
            </div>
          ))}
        </div>
      )}

      <div className="font-[var(--font-mono)] text-[10px] leading-tight">
        {detail.diff && detail.prev_index != null && (
          <div className="mb-0.5 text-[9px] text-[var(--color-muted)]">
            Byte-Diff vs. #{detail.prev_index} — geänderte Bytes hervorgehoben
          </div>
        )}
        {rows.map((row, r) => {
          const base = r * perRow;
          return (
            <div key={r} className="flex gap-2">
              <span className="text-[var(--color-muted)]">{base.toString(16).padStart(4, "0")}</span>
              <span className="flex gap-1">
                {row.map((b, c) => {
                  const idx = base + c;
                  return (
                    <span
                      key={c}
                      className={
                        diffChanged(detail.diff, idx)
                          ? "rounded-sm bg-[var(--color-accent)]/40 px-0.5 font-semibold text-[var(--color-fg)]"
                          : ""
                      }
                    >
                      {b.toString(16).padStart(2, "0")}
                    </span>
                  );
                })}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function SessionSummary({ stats, dropped }: { stats: CaptureStats; dropped: number }) {
  const cell = (label: string, value: string) => (
    <div className="flex flex-col">
      <span className="text-[9px] uppercase tracking-wide text-[var(--color-muted)]">{label}</span>
      <span className="tabular-nums">{value}</span>
    </div>
  );
  return (
    <div className="flex flex-col gap-2 text-[11px]">
      <div className="text-[10px] font-medium uppercase tracking-wide text-[var(--color-muted)]">
        Zusammenfassung
      </div>
      <div className="grid grid-cols-3 gap-x-2 gap-y-1.5">
        {cell("Pakete", String(stats.packets))}
        {cell("→ TX", `${stats.tx_packets}`)}
        {cell("← RX", `${stats.rx_packets}`)}
        {cell("Bytes", formatBytes(stats.bytes))}
        {cell("Dauer", formatDuration(stats.duration_us))}
        {cell("Handles", String(stats.unique_handles))}
        {cell("ATT-Writes", String(stats.att_writes))}
        {cell("ATT-Reads", String(stats.att_reads))}
        {cell("Notifications", String(stats.notifications))}
        {cell("Indications", String(stats.indications))}
        {cell("ATT-Handles", String(stats.unique_att_handles))}
        {cell("UUIDs", String(stats.unique_uuids))}
      </div>
      {dropped > 0 && (
        <p className="text-[9px] text-amber-400">
          {dropped} Pakete verworfen (Speichergrenze erreicht) — nie stiller Verlust.
        </p>
      )}
      <p className="text-[9px] leading-snug text-[var(--color-muted)]">
        Ein Paket wählen für Hex-Dump, dekodierte Felder und den Byte-Diff zum
        vorherigen Paket desselben Handles.
      </p>
    </div>
  );
}
