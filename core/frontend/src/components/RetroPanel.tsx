/**
 * `8bit` / `16bit` — settings panel of the live retro overlay, with a live
 * preview of the monitor under the cursor rendered with exactly these
 * settings (CPU reference in Rust, ≤ 10 fps, only while this panel is
 * mounted). Keyboard-first like the brightness panel: ↑↓ row, ←→ value,
 * Enter starts/stops the overlay, Esc leaves. Every change is saved and
 * applied at once — to the preview and to a running overlay.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { Gamepad2, Save, RotateCcw, X } from "lucide-react";
import {
  retroGetConfig,
  retroOpenPermission,
  retroPresetApply,
  retroPresetDelete,
  retroPresetSave,
  retroPresets,
  retroPreviewStart,
  retroPreviewStop,
  retroReset,
  retroRun,
  retroSetConfig,
  retroStatus,
  type RetroConfig,
  type RetroPreset,
  type RetroStatus,
} from "../lib/ipc";
import { RETRO_ROWS, adjustRow, moveRow, retroErrorText, rowInactive, rowValue } from "../lib/retro";
import { useTauriEvent } from "../hooks/useTauriEvent";

interface Props {
  focused: boolean;
  /** Typed keyword — `16bit` opens the panel in 16-bit mode. */
  keyword: "8bit" | "16bit";
  onExit: () => void;
}

const SAVE_DEBOUNCE_MS = 120;

function b64ToBytes(b64: string): Uint8ClampedArray<ArrayBuffer> {
  const bin = atob(b64);
  const out = new Uint8ClampedArray(new ArrayBuffer(bin.length));
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function RetroPanel({ focused, keyword, onExit }: Props) {
  const [cfg, setCfg] = useState<RetroConfig | null>(null);
  const [presets, setPresets] = useState<RetroPreset[]>([]);
  const [status, setStatus] = useState<RetroStatus | null>(null);
  const [sel, setSel] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [presetName, setPresetName] = useState("");
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const saveTimer = useRef<number | null>(null);
  const cfgRef = useRef<RetroConfig | null>(null);
  const rowRefs = useRef<(HTMLDivElement | null)[]>([]);

  const selRef = useRef(0);
  useEffect(() => {
    cfgRef.current = cfg;
  }, [cfg]);
  useEffect(() => {
    selRef.current = sel;
  }, [sel]);

  const refreshStatus = useCallback(() => {
    void retroStatus().then(setStatus).catch(() => {});
  }, []);

  // Initial load; `16bit` switches to the 16-bit slot.
  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        let c = await retroGetConfig();
        if (keyword === "16bit" && c.mode !== "16bit") c = await retroSetConfig({ ...c, mode: "16bit" });
        if (!live) return;
        setCfg(c);
        setPresets(await retroPresets());
      } catch (e) {
        if (live) setError(String(e));
      }
    })();
    refreshStatus();
    return () => {
      live = false;
    };
  }, [keyword, refreshStatus]);

  // Live preview while mounted.
  useEffect(() => {
    let cancelled = false;
    retroPreviewStart().catch((e) => {
      if (!cancelled) setPreviewError(String(e));
    });
    return () => {
      cancelled = true;
      void retroPreviewStop();
    };
  }, []);

  useTauriEvent<{ w: number; h: number; data: string }>("retro-preview-frame", (ev) => {
    const c = canvasRef.current;
    if (!c) return;
    const { w, h, data } = ev.payload;
    if (c.width !== w) c.width = w;
    if (c.height !== h) c.height = h;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    ctx.putImageData(new ImageData(b64ToBytes(data), w, h), 0, 0);
  });
  // The preview captures the screen: only while the popup is actually
  // visible, not while the panel merely stays mounted behind a hidden popup.
  useTauriEvent("popup-hidden", () => void retroPreviewStop());
  useTauriEvent("window-shown", () => {
    retroPreviewStart().catch((e) => setPreviewError(String(e)));
  });
  useTauriEvent<boolean>("retro-state-changed", () => refreshStatus(), [refreshStatus]);
  useTauriEvent<RetroConfig>("retro-config-changed", (ev) => setCfg(ev.payload));
  useTauriEvent<string>("retro-error", (ev) => setError(retroErrorText(ev.payload)));

  const commit = useCallback((next: RetroConfig) => {
    setCfg(next);
    if (saveTimer.current) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      void retroSetConfig(next).catch((e) => setError(String(e)));
    }, SAVE_DEBOUNCE_MS);
  }, []);

  useEffect(
    () => () => {
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
    },
    [],
  );

  const toggleOverlay = useCallback(() => {
    setError(null);
    void retroRun("toggle").catch((e) => setError(String(e)));
  }, []);

  // Keyboard (only while the panel owns focus).
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
        return;
      }
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
      if (e.key === "Enter") {
        // Toggling needs no loaded config — never swallow it while loading.
        e.preventDefault();
        e.stopPropagation();
        toggleOverlay();
        return;
      }
      const c = cfgRef.current;
      if (!c) return;
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        e.stopPropagation();
        setSel((i) => moveRow(i, e.key === "ArrowDown" ? 1 : -1));
      } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        e.stopPropagation();
        commit(adjustRow(RETRO_ROWS[selRef.current].id, c, e.key === "ArrowRight" ? 1 : -1));
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit, commit, toggleOverlay]);

  useEffect(() => {
    rowRefs.current[sel]?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  const savePreset = async () => {
    const name = presetName.trim();
    if (!name) return;
    try {
      // Make sure the latest values are stored before snapshotting them.
      if (cfgRef.current) await retroSetConfig(cfgRef.current);
      setPresets(await retroPresetSave(name));
      setPresetName("");
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  if (status && !status.supported) {
    return (
      <div className="flex h-full flex-col gap-2 p-4 text-[var(--color-fg)]">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <Gamepad2 size={15} className="text-[var(--color-accent)]" /> Retro-Overlay
        </div>
        <p className="text-[12px] text-[var(--color-muted)]">{status.note ?? "Auf diesem System nicht verfügbar."}</p>
      </div>
    );
  }

  const needsAx = !!cfg && (cfg.mode === "8bit" ? cfg.eight : cfg.sixteen).focus && status && !status.accessibility;

  return (
    <div className="flex h-full flex-col gap-2 overflow-y-auto p-3 text-[var(--color-fg)]">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <Gamepad2 size={15} className="text-[var(--color-accent)]" /> Retro-Overlay
        </div>
        <button
          type="button"
          onClick={toggleOverlay}
          className={
            "rounded-full px-2.5 py-0.5 text-[11px] font-medium transition-colors duration-(--duration-fast) " +
            (status?.running
              ? "bg-emerald-500/20 text-emerald-500"
              : "bg-[var(--color-surface)] text-[var(--color-muted)] hover:text-[var(--color-fg)]")
          }
          title="Enter oder ⌃⇧⌥8"
        >
          {status?.running ? "● läuft — beenden" : "starten"}
        </button>
      </div>

      {status && !status.screen_permission && (
        <div className="rounded-lg bg-amber-500/10 px-2.5 py-1.5 text-[11px] text-amber-600">
          Für die Vorschau und das Overlay braucht Inspector Rust die Erlaubnis zur Bildschirmaufnahme.{" "}
          <button type="button" className="underline" onClick={() => void retroOpenPermission("screen")}>
            Systemeinstellungen öffnen
          </button>
        </div>
      )}
      {needsAx && (
        <div className="rounded-lg bg-amber-500/10 px-2.5 py-1.5 text-[11px] text-amber-600">
          Der Fokus-Modus braucht Bedienungshilfen — ohne die Erlaubnis wird alles gleich grob gezeichnet.{" "}
          <button type="button" className="underline" onClick={() => void retroOpenPermission("accessibility")}>
            Öffnen
          </button>
        </div>
      )}
      {error && (
        <div className="rounded-lg bg-rose-500/10 px-2.5 py-1.5 text-[11px] text-rose-600">{error}</div>
      )}

      <div className="shrink-0 overflow-hidden rounded-lg border border-[var(--color-border)] bg-black">
        <canvas ref={canvasRef} className="block w-full" style={{ imageRendering: "pixelated", aspectRatio: "16 / 10" }} />
        {previewError && (
          <p className="px-2 py-1 text-[10px] text-[var(--color-muted)]">Vorschau: {retroErrorText(previewError)}</p>
        )}
      </div>

      {cfg && (
        <div className="flex flex-col">
          {RETRO_ROWS.map((row, i) => {
            const header = i === 0 || RETRO_ROWS[i - 1].group !== row.group;
            const active = focused && i === sel;
            const dim = rowInactive(row, cfg);
            return (
              <div key={row.id} ref={(el) => { rowRefs.current[i] = el; }}>
                {header && (
                  <div className="mt-1.5 mb-0.5 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">
                    {row.group}
                  </div>
                )}
                <div
                  onClick={() => setSel(i)}
                  className={
                    "flex items-center justify-between rounded-md px-2 py-1 text-[12px] transition-colors duration-(--duration-fast) " +
                    (active ? "bg-[var(--color-accent)]/12 ring-1 ring-[var(--color-accent)]" : "") +
                    (dim ? " opacity-55" : "")
                  }
                >
                  <span>{row.label}</span>
                  <span className="flex items-center gap-1.5 tabular-nums">
                    <button
                      type="button"
                      aria-label={`${row.label} verringern`}
                      className="rounded px-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                      onClick={(e) => {
                        e.stopPropagation();
                        setSel(i);
                        commit(adjustRow(row.id, cfg, -1));
                      }}
                    >
                      ‹
                    </button>
                    <span className="min-w-[88px] text-right font-medium">{rowValue(row.id, cfg)}</span>
                    <button
                      type="button"
                      aria-label={`${row.label} erhöhen`}
                      className="rounded px-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                      onClick={(e) => {
                        e.stopPropagation();
                        setSel(i);
                        commit(adjustRow(row.id, cfg, 1));
                      }}
                    >
                      ›
                    </button>
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <div className="mt-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">Presets</div>
      <div className="flex flex-wrap gap-1">
        {presets.map((p) => (
          <span
            key={p.name}
            className="flex items-center gap-1 rounded-full bg-[var(--color-surface)] px-2 py-0.5 text-[11px]"
          >
            <button
              type="button"
              onClick={() =>
                void retroPresetApply(p.name)
                  .then(setCfg)
                  .catch((e) => setError(String(e)))
              }
              title={`${p.mode === "8bit" ? "8-Bit" : "16-Bit"} · ${p.settings.palette}`}
            >
              {p.name}
            </button>
            {!p.builtin && (
              <button
                type="button"
                aria-label={`Preset ${p.name} löschen`}
                className="text-[var(--color-muted)] hover:text-rose-500"
                onClick={() =>
                  void retroPresetDelete(p.name)
                    .then(setPresets)
                    .catch((e) => setError(String(e)))
                }
              >
                <X size={11} />
              </button>
            )}
          </span>
        ))}
      </div>
      <div className="flex items-center gap-1.5">
        <input
          value={presetName}
          onChange={(e) => setPresetName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              e.stopPropagation();
              void savePreset();
            }
          }}
          placeholder="Name für neuen Preset"
          maxLength={40}
          className="min-w-0 flex-1 rounded-md border border-[var(--color-border)] bg-transparent px-2 py-1 text-[11px]"
        />
        <button
          type="button"
          onClick={() => void savePreset()}
          className="flex items-center gap-1 rounded-md bg-[var(--color-surface)] px-2 py-1 text-[11px]"
        >
          <Save size={11} /> Speichern
        </button>
        <button
          type="button"
          onClick={() => void retroReset().then(setCfg).catch((e) => setError(String(e)))}
          className="flex items-center gap-1 rounded-md bg-[var(--color-surface)] px-2 py-1 text-[11px]"
          title="Aktiven Modus auf Standardwerte"
        >
          <RotateCcw size={11} /> Zurücksetzen
        </button>
      </div>

      <p className="mt-1 text-[10px] text-[var(--color-muted)]">
        {focused
          ? "↑↓ Zeile · ←→ Wert · Enter Overlay an/aus · Esc zurück"
          : "Enter übernimmt die Tastatur."}{" "}
        · ⌃⇧⌥8 an/aus · ⌃⇧⌥9 Fokus
      </p>
    </div>
  );
}
