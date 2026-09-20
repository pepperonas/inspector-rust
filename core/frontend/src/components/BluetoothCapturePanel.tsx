import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  Bluetooth,
  Copy,
  Download,
  FileUp,
  Search,
  X,
} from "lucide-react";
import {
  btsniffDefaultFilename,
  btsniffExportJson,
  btsniffOpenFile,
  btsniffPacket,
  setSuppressHide,
} from "../lib/ipc";
import {
  BT_FILTERS,
  decodedText,
  diffChanged,
  directionGlyph,
  directionLabel,
  filterPackets,
  formatBytes,
  formatDuration,
  formatHandle,
  formatRelTime,
  hexString,
  isGattOp,
  protocolTag,
  type BtFilter,
  type BtPacketDetail,
  type BtPacketSlim,
  type OpenResult,
} from "../lib/bluetooth-capture";

interface Props {
  focused: boolean;
  onExit: () => void;
}

const ROW_HEIGHT = 34;

const CAPTURE_FILTERS = [
  { name: "Bluetooth-Capture", extensions: ["pklg", "log", "snoop", "btsnoop", "pcapng", "pcap", "cap"] },
  { name: "Alle Dateien", extensions: ["*"] },
];

export function BluetoothCapturePanel({ focused, onExit }: Props) {
  const [result, setResult] = useState<OpenResult | null>(null);
  const [filter, setFilter] = useState<BtFilter>("all");
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<number | null>(null);
  const [detail, setDetail] = useState<BtPacketDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);

  const containerRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const packets = useMemo(() => result?.packets ?? [], [result]);
  const stats = result?.stats;
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

  const importPath = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    try {
      const r = await btsniffOpenFile(path);
      setResult(r);
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

  const onExport = useCallback(async () => {
    if (!result) return;
    await setSuppressHide(true).catch(() => {});
    try {
      const suggested = await btsniffDefaultFilename();
      const target = await saveDialog({
        title: "Analyse als JSON exportieren",
        defaultPath: suggested,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (typeof target === "string") await btsniffExportJson(target);
    } catch (e) {
      setError(String(e));
    } finally {
      await setSuppressHide(false).catch(() => {});
    }
  }, [result]);

  // Drag-and-drop import — pin the popup for the drag (dragging from the OS
  // steals focus, and the popup hides on focus-loss). The SnippetsPanel pattern.
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

  // Fetch the inspector detail when the selection changes.
  useEffect(() => {
    if (selected == null) {
      setDetail(null);
      return;
    }
    let cancelled = false;
    btsniffPacket(selected)
      .then((d) => {
        if (!cancelled) setDetail(d);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [selected]);

  useEffect(() => {
    if (focused) containerRef.current?.focus();
  }, [focused]);

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
      /* clipboard unavailable — ignore */
    }
  }, []);

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const inSearch = e.target === searchRef.current;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      if (inSearch) {
        searchRef.current?.blur();
        containerRef.current?.focus();
      } else {
        onExit();
      }
      return;
    }
    // Cmd/Ctrl+F focuses search from anywhere.
    if (e.key === "f" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      searchRef.current?.focus();
      return;
    }
    if (inSearch) return; // let the search field handle the rest
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

  // ── idle / error states ──
  const showIdle = !result && !busy;

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
            {result ? result.source_label : "Bluetooth-Capture"}
          </span>
          {result && (
            <span className="shrink-0 rounded bg-[var(--color-accent)]/15 px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-[var(--color-accent)]">
              {result.format}
            </span>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <button
            onClick={() => void onPickFile()}
            title="Capture-Datei importieren"
            className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
          >
            <FileUp className="h-4 w-4" />
          </button>
          <button
            onClick={() => void onExport()}
            disabled={!result}
            title="Analyse als JSON exportieren"
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

      {showIdle && (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 px-4 text-center">
          <Bluetooth className="h-9 w-9 text-[var(--color-accent)]/70" />
          <div className="text-[13px] font-medium">Capture-Datei analysieren</div>
          <p className="max-w-[280px] text-[11px] leading-relaxed text-[var(--color-muted)]">
            Importiere ein <b>.pklg</b> (macOS PacketLogger), ein{" "}
            <b>btsnoop_hci.log</b> (Android) oder ein <b>.pcapng</b> (Wireshark).
            HCI, ACL, L2CAP, ATT und GATT werden dekodiert — mit Byte-Diff zum
            Reverse-Engineering.
          </p>
          <button
            onClick={() => void onPickFile()}
            className="rounded-lg border border-[var(--color-accent)]/40 bg-[var(--color-accent)]/10 px-3 py-1.5 text-[12px] font-medium text-[var(--color-fg)] hover:bg-[var(--color-accent)]/20"
          >
            Datei wählen …
          </button>
          <p className="text-[10px] text-[var(--color-muted)]">
            … oder eine Datei hierher ziehen
          </p>
        </div>
      )}

      {busy && (
        <div className="flex flex-1 items-center justify-center text-[12px] text-[var(--color-muted)]">
          Analysiere …
        </div>
      )}

      {result && !busy && (
        <>
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
          <div
            ref={listRef}
            className="min-h-0 flex-1 overflow-y-auto rounded-md border border-[var(--color-border)]"
          >
            {filtered.length === 0 ? (
              <div className="flex h-full items-center justify-center text-[11px] text-[var(--color-muted)]">
                Keine Pakete für diesen Filter
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

          {/* inspector or summary */}
          <div className="shrink-0 overflow-y-auto rounded-md border border-[var(--color-border)] p-2 [max-height:42%]">
            {detail ? (
              <PacketInspector
                detail={detail}
                copied={copied}
                onCopyHex={() => void copy(hexString(detail.raw), "hex")}
                onCopyDecoded={() => void copy(decodedText(detail.decoded), "decoded")}
                onClose={() => setSelected(null)}
              />
            ) : (
              stats && <SessionSummary stats={stats} />
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
        selected
          ? "bg-[var(--color-accent)]/20"
          : "hover:bg-[var(--color-surface)]"
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

      {copied && (
        <div className="text-[9px] text-[var(--color-accent)]">{copied} kopiert ✓</div>
      )}

      {/* decoded fields */}
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

      {/* hex dump with byte-diff highlighting */}
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
              <span className="text-[var(--color-muted)]">
                {base.toString(16).padStart(4, "0")}
              </span>
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

function SessionSummary({ stats }: { stats: OpenResult["stats"] }) {
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
      <p className="text-[9px] leading-snug text-[var(--color-muted)]">
        Ein Paket wählen für Hex-Dump, dekodierte Felder und den Byte-Diff zum
        vorherigen Paket desselben ATT-Handles.
      </p>
    </div>
  );
}
