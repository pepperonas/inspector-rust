import { useCallback, useEffect, useRef, useState, type PointerEvent } from "react";
import { Lightbulb, Loader2, Power, RefreshCw, Wifi } from "lucide-react";
import { HueBeatSync } from "./HueBeatSync";
import {
  HUE_LINK_BUTTON,
  hueDiscover,
  hueForget,
  hueListLights,
  huePair,
  hueSetAll,
  hueSetBridgeIp,
  hueSetLight,
  hueStatus,
  type HueLight,
  type HueStatus,
} from "../lib/ipc";

/**
 * In-popup Philips Hue control, rendered in the **right preview column** (same
 * pattern as `brightness` / `sound`). Entered by pressing Enter on the `hue`
 * command row.
 *
 * Two states:
 *   1. **Not connected** → a connect card: discover the bridge (local SSDP) or
 *      type its IP, press the bridge's link button, Connect (pair).
 *   2. **Connected** → an "All lamps" master row (on/off · brightness · colour)
 *      followed by a row per lamp; colour-capable bulbs show 8 preset swatches.
 *
 * Keyboard model (when `focused` and connected):
 *   ↑ / ↓   select a row (row 0 = All lamps)
 *   ← / →   adjust the selected row's brightness (±5, debounced)
 *   Space   toggle the selected row on/off
 *   1 … 8   apply the matching colour preset (colour rows only)
 *   Enter   hand the arrow keys back to the left list (`onUnfocus`)
 *   Esc     leave hue mode (`onExit`)
 * Everything is also clickable (toggle pill, swatches, row select).
 */

const STEP = 10;

/** 8 colour presets shown as round swatches (the Rust side converts the hex to
 *  the bulb's xy colour space). First is a warm white for the "back to normal". */
const PRESETS: { name: string; hex: string }[] = [
  { name: "Warm white", hex: "#FFCB8E" },
  { name: "Red", hex: "#FF2D2D" },
  { name: "Orange", hex: "#FF8A00" },
  { name: "Yellow", hex: "#FFE000" },
  { name: "Green", hex: "#3DDC5F" },
  { name: "Cyan", hex: "#1FD0E0" },
  { name: "Blue", hex: "#2E6BFF" },
  { name: "Magenta", hex: "#FF2D9B" },
];

interface Row {
  /** Stable key: "all" or a lamp id. */
  key: string;
  name: string;
  on: boolean;
  brightness: number;
  dimmable: boolean;
  supports_color: boolean;
  reachable: boolean;
}

export function HuePanel({
  focused,
  onExit,
}: {
  focused: boolean;
  onExit: () => void;
}) {
  const [status, setStatus] = useState<HueStatus | null>(null);
  const [lights, setLights] = useState<HueLight[] | null>(null);
  const [sel, setSel] = useState(0);
  // Row DOM refs so Tab/↑↓ can keep the selected lamp centred in the scroll view.
  const rowRefs = useRef<(HTMLDivElement | null)[]>([]);
  useEffect(() => {
    rowRefs.current[sel]?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [sel]);

  // Local optimistic overrides so the UI reacts instantly. Keyed by row key.
  const [onMap, setOnMap] = useState<Record<string, boolean>>({});
  const [briMap, setBriMap] = useState<Record<string, number>>({});
  const briTimers = useRef<Record<string, number>>({});

  // Connect-flow state.
  const [ip, setIp] = useState("");
  const [busy, setBusy] = useState<null | "discover" | "pair" | "refresh">(null);
  const [connectMsg, setConnectMsg] = useState<string | null>(null);

  const loadLights = useCallback(async () => {
    const ls = await hueListLights();
    setLights(ls);
    setOnMap(Object.fromEntries(ls.map((l) => [l.id, l.on])));
    setBriMap(Object.fromEntries(ls.map((l) => [l.id, l.brightness])));
  }, []);

  const refreshStatus = useCallback(async () => {
    const st = await hueStatus().catch(() => null);
    setStatus(st);
    if (st?.bridge_ip) setIp((cur) => cur || st.bridge_ip!);
    if (st?.connected) {
      await loadLights().catch(() => setLights([]));
    }
    return st;
  }, [loadLights]);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  // Clear pending brightness debounces on unmount.
  useEffect(() => {
    const pending = briTimers.current;
    return () => {
      for (const t of Object.values(pending)) window.clearTimeout(t);
    };
  }, []);

  // ── Rows (All + each lamp) ────────────────────────────────────────────────
  const rows: Row[] = (() => {
    if (!lights) return [];
    const anyOn = lights.some((l) => (onMap[l.id] ?? l.on));
    const dimmables = lights.filter((l) => l.dimmable);
    const avgBri =
      dimmables.length > 0
        ? Math.round(
            dimmables.reduce((s, l) => s + (briMap[l.id] ?? l.brightness), 0) /
              dimmables.length,
          )
        : 100;
    const all: Row = {
      key: "all",
      name: "All lamps",
      on: onMap["all"] ?? anyOn,
      brightness: briMap["all"] ?? avgBri,
      dimmable: dimmables.length > 0,
      supports_color: lights.some((l) => l.supports_color),
      reachable: true,
    };
    const lampRows: Row[] = lights.map((l) => ({
      key: l.id,
      name: l.name,
      on: onMap[l.id] ?? l.on,
      brightness: briMap[l.id] ?? l.brightness,
      dimmable: l.dimmable,
      supports_color: l.supports_color,
      reachable: l.reachable,
    }));
    return [all, ...lampRows];
  })();

  // ── Mutators (optimistic + IPC) ───────────────────────────────────────────
  const applyOn = useCallback(
    (key: string, on: boolean) => {
      setOnMap((m) => ({ ...m, [key]: on }));
      const call = key === "all" ? hueSetAll(on, null, null) : hueSetLight(key, on, null, null);
      void call.catch(() => undefined);
      // "All" also flips every per-lamp local state so the child rows track it.
      if (key === "all" && lights) {
        setOnMap((m) => {
          const next: Record<string, boolean> = { ...m, all: on };
          for (const l of lights) next[l.id] = on;
          return next;
        });
      }
    },
    [lights],
  );

  const applyBrightness = useCallback((key: string, pct: number) => {
    const clamped = Math.max(0, Math.min(100, pct));
    setBriMap((m) => ({ ...m, [key]: clamped }));
    setOnMap((m) => ({ ...m, [key]: true }));
    if (briTimers.current[key]) window.clearTimeout(briTimers.current[key]);
    briTimers.current[key] = window.setTimeout(() => {
      const call =
        key === "all"
          ? hueSetAll(true, clamped, null)
          : hueSetLight(key, true, clamped, null);
      void call.catch(() => undefined);
    }, 120);
  }, []);

  // Map a pointer x-position on a slider track to a 0–100 % and apply it
  // (drives the mouse click / drag on the brightness bars).
  const setBrightnessFromPointer = useCallback(
    (e: PointerEvent<HTMLDivElement>, key: string) => {
      const rect = e.currentTarget.getBoundingClientRect();
      const frac = rect.width > 0 ? (e.clientX - rect.left) / rect.width : 0;
      applyBrightness(key, Math.round(Math.max(0, Math.min(1, frac)) * 100));
    },
    [applyBrightness],
  );

  const applyColor = useCallback((key: string, hex: string) => {
    setOnMap((m) => ({ ...m, [key]: true }));
    const bri = briMap[key];
    const call =
      key === "all"
        ? hueSetAll(true, bri ?? null, hex)
        : hueSetLight(key, true, bri ?? null, hex);
    void call.catch(() => undefined);
  }, [briMap]);

  // ── Keyboard (control view only) ──────────────────────────────────────────
  useEffect(() => {
    if (!focused || !status?.connected || rows.length === 0) return;
    const onKey = (e: KeyboardEvent) => {
      const n = rows.length;
      const row = rows[Math.min(sel, n - 1)];
      switch (e.key) {
        // Tab → next lamp, Shift+Tab → previous; both wrap around the ends
        // (the "All lamps" master is the top row).
        case "Tab":
          e.preventDefault();
          e.stopPropagation();
          setSel((s) => (e.shiftKey ? (s - 1 + n) % n : (s + 1) % n));
          break;
        // ↑ / ↓ also move the selection (no wrap), as a convenience.
        case "ArrowUp":
          e.preventDefault();
          e.stopPropagation();
          setSel((s) => Math.max(0, s - 1));
          break;
        case "ArrowDown":
          e.preventDefault();
          e.stopPropagation();
          setSel((s) => Math.min(n - 1, s + 1));
          break;
        // ← / → → 10% dimmer / brighter on the selected lamp.
        case "ArrowLeft":
          if (!row?.dimmable) return;
          e.preventDefault();
          e.stopPropagation();
          applyBrightness(row.key, row.brightness - STEP);
          break;
        case "ArrowRight":
          if (!row?.dimmable) return;
          e.preventDefault();
          e.stopPropagation();
          applyBrightness(row.key, row.brightness + STEP);
          break;
        // Enter (and Space) → toggle the current selection on/off.
        case "Enter":
        case " ":
          e.preventDefault();
          e.stopPropagation();
          if (row) applyOn(row.key, !row.on);
          break;
        case "Escape":
          e.preventDefault();
          e.stopPropagation();
          onExit();
          break;
        default:
          // 1–8 → colour preset for a colour-capable row.
          if (/^[1-8]$/.test(e.key) && row?.supports_color) {
            e.preventDefault();
            e.stopPropagation();
            applyColor(row.key, PRESETS[Number(e.key) - 1].hex);
          }
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, status, rows, sel, applyOn, applyBrightness, applyColor, onExit]);

  // ── Connect actions ───────────────────────────────────────────────────────
  const discover = async () => {
    setBusy("discover");
    setConnectMsg(null);
    try {
      const found = await hueDiscover();
      if (found) {
        setIp(found);
        await hueSetBridgeIp(found);
        setConnectMsg("Bridge found. Press its round link button, then Connect.");
      } else {
        setConnectMsg("No bridge found on the network. Enter its IP manually.");
      }
    } catch {
      setConnectMsg("Discovery failed. Enter the bridge IP manually.");
    } finally {
      setBusy(null);
    }
  };

  const connect = async () => {
    const target = ip.trim();
    if (!target) {
      setConnectMsg("Enter the bridge IP first (or use Discover).");
      return;
    }
    setBusy("pair");
    setConnectMsg(null);
    try {
      await huePair(target);
      await refreshStatus();
    } catch (e) {
      const msg = String(e);
      setConnectMsg(
        msg.includes(HUE_LINK_BUTTON)
          ? "Press the round link button on top of the bridge, then click Connect (within 30 s)."
          : `Connect failed: ${msg}`,
      );
    } finally {
      setBusy(null);
    }
  };

  const header = (
    <div className="flex items-center gap-2 text-[13px] font-medium">
      <Lightbulb size={15} className="text-[var(--color-accent)]" /> Philips Hue
    </div>
  );

  // ── Render: loading ───────────────────────────────────────────────────────
  if (status === null) {
    return (
      <div className="flex h-full flex-col gap-3 p-4 text-[var(--color-fg)]">
        {header}
        <p className="flex items-center gap-2 text-[12px] text-[var(--color-muted)]">
          <Loader2 size={13} className="animate-spin" /> Checking bridge…
        </p>
      </div>
    );
  }

  // ── Render: connect flow ──────────────────────────────────────────────────
  if (!status.connected) {
    return (
      <div className="flex h-full flex-col gap-4 overflow-y-auto p-4 text-[var(--color-fg)]">
        {header}
        <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-4 text-[12px]">
          <p className="mb-3 text-[var(--color-muted)]">
            {status.paired
              ? `Saved bridge ${status.bridge_ip ?? ""} didn't answer. Re-discover or re-connect.`
              : "Connect your Hue Bridge once. Discover it on the network (or type its IP), press the bridge's round link button, then Connect."}
          </p>

          <div className="mb-2 flex gap-2">
            <button
              type="button"
              onClick={() => void discover()}
              disabled={!!busy}
              className="md3-press flex items-center gap-1.5 rounded border border-[var(--color-border)] px-2.5 py-1.5 hover:border-[var(--color-accent)] disabled:opacity-50"
            >
              {busy === "discover" ? (
                <Loader2 size={13} className="animate-spin" />
              ) : (
                <Wifi size={13} />
              )}
              Discover
            </button>
            <input
              value={ip}
              onChange={(e) => setIp(e.target.value)}
              placeholder="192.168.x.x"
              className="flex-1 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 font-[var(--font-mono)] text-[12px] outline-none focus:border-[var(--color-accent)]"
            />
          </div>

          <button
            type="button"
            onClick={() => void connect()}
            disabled={!!busy}
            className="md3-press flex w-full items-center justify-center gap-1.5 rounded bg-[var(--color-accent)] px-3 py-2 font-medium text-[var(--color-accent-fg)] disabled:opacity-50"
          >
            {busy === "pair" ? <Loader2 size={13} className="animate-spin" /> : <Power size={13} />}
            Connect (press the bridge button first)
          </button>

          {connectMsg && (
            <p className="md3-banner-in mt-3 text-[11px] text-[var(--color-accent)]">{connectMsg}</p>
          )}
          {status.paired && (
            <button
              type="button"
              onClick={() => void hueForget().then(refreshStatus)}
              className="mt-3 text-[11px] text-[var(--color-muted)] underline hover:text-[var(--color-fg)]"
            >
              Forget saved bridge
            </button>
          )}
        </div>
        <p className="text-[10px] text-[var(--color-muted)]">
          Local only — talks to the bridge over your LAN, no Philips cloud.
        </p>
      </div>
    );
  }

  // ── Render: control view ──────────────────────────────────────────────────
  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)]">
      <div className="flex items-center justify-between">
        {header}
        <button
          type="button"
          title="Refresh"
          onClick={() => {
            setBusy("refresh");
            void loadLights().finally(() => setBusy(null));
          }}
          className="md3-press rounded p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
        >
          <RefreshCw size={13} className={busy === "refresh" ? "animate-spin" : ""} />
        </button>
      </div>

      {lights && lights.length === 0 ? (
        <p className="text-[12px] text-[var(--color-muted)]">No lamps found on this bridge.</p>
      ) : (
        <div className="flex flex-col gap-2.5">
          {rows.map((row, i) => {
            const active = focused && i === sel;
            const master = row.key === "all";
            return (
              <div
                key={row.key}
                ref={(el) => {
                  rowRefs.current[i] = el;
                }}
                onClick={() => setSel(i)}
                className={
                  "rounded-lg border px-3 py-2 transition-colors " +
                  (active
                    ? "border-[var(--color-accent)] bg-[var(--color-accent)]/10"
                    : "border-[var(--color-border)] hover:bg-[var(--color-surface)]") +
                  (master ? " bg-[var(--color-surface)]" : "")
                }
              >
                <div className="flex items-center justify-between gap-2">
                  <span
                    className={
                      "flex items-center gap-1.5 truncate text-[12px] " +
                      (master ? "font-semibold" : "")
                    }
                  >
                    {master && <Lightbulb size={12} className="text-[var(--color-accent)]" />}
                    <span className="truncate">{row.name}</span>
                    {!row.reachable && !master && (
                      <span className="text-[10px] text-[var(--color-muted)]">(off network)</span>
                    )}
                  </span>
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      applyOn(row.key, !row.on);
                    }}
                    className={
                      "md3-press flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium " +
                      (row.on
                        ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                        : "border border-[var(--color-border)] text-[var(--color-muted)]")
                    }
                  >
                    <Power size={11} /> {row.on ? "On" : "Off"}
                  </button>
                </div>

                {row.dimmable && (
                  <div className="mt-1.5 flex items-center gap-2">
                    {/* Click / drag anywhere on the track to set brightness. The
                        outer div is the hit area (taller than the visual bar so
                        it's easy to grab); pointer capture keeps the drag alive
                        outside the bar. */}
                    <div
                      role="slider"
                      aria-label={`${row.name} brightness`}
                      aria-valuenow={row.brightness}
                      aria-valuemin={0}
                      aria-valuemax={100}
                      className="group flex h-5 flex-1 cursor-pointer items-center"
                      onPointerDown={(e) => {
                        e.stopPropagation();
                        setSel(i);
                        e.currentTarget.setPointerCapture(e.pointerId);
                        setBrightnessFromPointer(e, row.key);
                      }}
                      onPointerMove={(e) => {
                        if (e.buttons === 1) setBrightnessFromPointer(e, row.key);
                      }}
                    >
                      <span className="h-2 w-full overflow-hidden rounded-full bg-[var(--color-border)] transition-[height] group-hover:h-2.5">
                        <span
                          className="block h-full rounded-full bg-[var(--color-accent)] transition-[width]"
                          style={{ width: `${row.on ? row.brightness : 0}%` }}
                        />
                      </span>
                    </div>
                    <span className="w-9 text-right text-[11px] tabular-nums text-[var(--color-muted)]">
                      {row.brightness}%
                    </span>
                  </div>
                )}

                {row.supports_color && (
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {PRESETS.map((p) => (
                      <button
                        key={p.hex}
                        type="button"
                        title={p.name}
                        aria-label={p.name}
                        onClick={(e) => {
                          e.stopPropagation();
                          applyColor(row.key, p.hex);
                        }}
                        className="h-5 w-5 rounded-full border border-black/20 shadow-sm transition-transform hover:scale-110"
                        style={{ background: p.hex }}
                      />
                    ))}
                  </div>
                )}
              </div>
            );
          })}

          <p className="mt-1 text-[11px] text-[var(--color-muted)]">
            {focused ? (
              <>Tab next lamp · ←→ ±10% · Enter on/off · 1–8 colour · Esc close</>
            ) : (
              <>Press Enter on the hue row to control the lamps with the keyboard.</>
            )}
          </p>

          {lights && lights.length > 0 && <HueBeatSync />}
        </div>
      )}
    </div>
  );
}
