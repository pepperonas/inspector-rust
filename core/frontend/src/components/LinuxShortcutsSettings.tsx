import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, Keyboard, RefreshCw } from "lucide-react";
import {
  linuxApplyDesktopShortcuts,
  linuxScanDesktopShortcuts,
  linuxWebHotkeyToGsettings,
  type LinuxShortcutSetupScan,
} from "../lib/ipc";
import { HotkeyCapture } from "./HotkeyCapture";

const ACTION_LABELS: Record<string, string> = {
  toggle: "Open popup",
  ocr: "OCR region",
  screenshot: "Screenshot region",
  color: "Pick color",
};

type RowDraft = {
  id: string;
  name: string;
  arg: string;
  chosen: string;
  chosenDisplay: string;
  candidates: LinuxShortcutSetupScan["rows"][0]["candidates"];
  customWeb: string;
  useCustom: boolean;
};

function rowHasConflict(row: RowDraft): boolean {
  const match = row.candidates.find((c) => c.binding === row.chosen);
  return match !== undefined && !match.free;
}

export function LinuxShortcutsSettings() {
  const [scan, setScan] = useState<LinuxShortcutSetupScan | null>(null);
  const [rows, setRows] = useState<RowDraft[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<
    { kind: "ok"; message: string } | { kind: "err"; message: string } | null
  >(null);

  const loadScan = useCallback(async () => {
    setLoading(true);
    setStatus(null);
    try {
      const data = await linuxScanDesktopShortcuts();
      setScan(data);
      setRows(
        data.rows.map((r) => ({
          id: r.id,
          name: r.name,
          arg: r.arg,
          chosen: r.chosen,
          chosenDisplay: r.chosen_display,
          candidates: r.candidates,
          customWeb: "",
          useCustom: !r.candidates.some((c) => c.binding === r.chosen),
        })),
      );
    } catch (e) {
      setStatus({
        kind: "err",
        message: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadScan();
  }, [loadScan]);

  const updateRow = (id: string, patch: Partial<RowDraft>) => {
    setRows((prev) => prev.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  };

  const onSelectCandidate = (id: string, binding: string, display: string) => {
    updateRow(id, {
      chosen: binding,
      chosenDisplay: display,
      useCustom: false,
      customWeb: "",
    });
  };

  const onCustomRecorded = async (id: string, webShortcut: string) => {
    if (!webShortcut) {
      updateRow(id, { customWeb: "", useCustom: false });
      return;
    }
    try {
      const gsettings = await linuxWebHotkeyToGsettings(webShortcut);
      updateRow(id, {
        customWeb: webShortcut,
        useCustom: true,
        chosen: gsettings,
        chosenDisplay: webShortcut.replace(/Key/g, "").replace(/Digit/g, ""),
      });
    } catch (e) {
      setStatus({
        kind: "err",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  };

  const runAutoSetup = async () => {
    setBusy(true);
    setStatus(null);
    try {
      await linuxApplyDesktopShortcuts([]);
      await loadScan();
      setStatus({
        kind: "ok",
        message: "Shortcuts installed with automatic conflict resolution.",
      });
    } catch (e) {
      setStatus({
        kind: "err",
        message: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (!scan?.can_configure) return;
    setBusy(true);
    setStatus(null);
    try {
      const bindings = rows.map((r) => ({ id: r.id, binding: r.chosen }));
      await linuxApplyDesktopShortcuts(bindings);
      await loadScan();
      setStatus({ kind: "ok", message: "Desktop shortcuts saved." });
    } catch (e) {
      setStatus({
        kind: "err",
        message: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setBusy(false);
    }
  };

  const anyConflict = rows.some(rowHasConflict);

  return (
    <div>
      <div className="mb-1 flex items-center gap-2">
        <Keyboard size={16} className="text-[var(--color-accent)]" />
        <h2 className="text-[14px] font-semibold">Linux desktop shortcuts</h2>
      </div>
      <p className="mb-4 text-[12px] text-[var(--color-muted)]">
        On GNOME/Cinnamon (Wayland), global shortcuts are registered via gsettings.
        Conflicts are scanned automatically; adjust each action below or record your own
        combination.
      </p>

      <div className="rounded-lg border border-[var(--color-border)] p-4">
        {loading ? (
          <p className="text-[12px] text-[var(--color-muted)]">Scanning shortcuts…</p>
        ) : (
          <>
            {scan && (
              <p className="mb-3 text-[11px] text-[var(--color-muted)]">
                Desktop: <b className="text-[var(--color-fg)]">{scan.desktop}</b>
                {scan.profile && (
                  <>
                    {" "}
                    · profile <code className="text-[10px]">{scan.profile}</code>
                  </>
                )}
              </p>
            )}

            {scan?.message && (
              <div className="mb-3 rounded border border-amber-500/40 bg-amber-500/5 px-2.5 py-1.5 text-[11px] text-amber-200/90">
                {scan.message}
              </div>
            )}

            {!scan?.can_configure && scan?.message && (
              <p className="text-[12px] text-[var(--color-muted)]">{scan.message}</p>
            )}

            {scan?.can_configure &&
              rows.map((row) => (
                <div
                  key={row.id}
                  className="mb-4 border-b border-[var(--color-border)] pb-4 last:mb-0 last:border-0 last:pb-0"
                >
                  <div className="mb-2 flex items-center gap-2">
                    <span className="text-[12px] font-medium text-[var(--color-fg)]">
                      {ACTION_LABELS[row.id] ?? row.name}
                    </span>
                    <span className="text-[11px] text-[var(--color-muted)]">
                      → <code className="text-[10px]">{row.arg}</code>
                    </span>
                    {rowHasConflict(row) && (
                      <span className="ml-auto flex items-center gap-1 text-[10px] text-amber-400">
                        <AlertTriangle size={11} />
                        Conflict
                      </span>
                    )}
                  </div>

                  <label className="mb-1 block text-[11px] text-[var(--color-muted)]">
                    Preset
                  </label>
                  <select
                    className="mb-2 w-full rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px]"
                    value={row.useCustom ? "__custom__" : row.chosen}
                    disabled={busy}
                    onChange={(e) => {
                      const v = e.target.value;
                      if (v === "__custom__") {
                        updateRow(row.id, { useCustom: true });
                        return;
                      }
                      const cand = row.candidates.find((c) => c.binding === v);
                      if (cand) {
                        onSelectCandidate(row.id, cand.binding, cand.display);
                      }
                    }}
                  >
                    {row.candidates.map((c) => (
                      <option key={c.binding} value={c.binding}>
                        {c.display}
                        {c.free ? "" : " (occupied)"}
                      </option>
                    ))}
                    <option value="__custom__">Custom (record)…</option>
                  </select>

                  {(row.useCustom ||
                    !row.candidates.some((c) => c.binding === row.chosen)) && (
                    <div className="mt-1">
                      <label className="mb-1 block text-[11px] text-[var(--color-muted)]">
                        Record combination
                      </label>
                      <HotkeyCapture
                        value={row.customWeb}
                        pending={busy}
                        disabled={busy}
                        onChange={(web) => void onCustomRecorded(row.id, web)}
                      />
                      <p className="mt-1 text-[10px] text-[var(--color-muted)]">
                        Click the field, press your keys, then verify the label. Esc
                        cancels, Backspace clears.
                      </p>
                    </div>
                  )}

                  <p className="mt-2 text-[11px] text-[var(--color-muted)]">
                    Active:{" "}
                    <b className="text-[var(--color-fg)]">{row.chosenDisplay}</b>
                  </p>
                </div>
              ))}

            {anyConflict && scan?.can_configure && (
              <div className="mb-3 flex items-start gap-2 rounded border border-amber-500/40 bg-amber-500/5 px-2.5 py-1.5 text-[11px] text-amber-200/90">
                <AlertTriangle size={14} className="mt-0.5 shrink-0" />
                <span>
                  One or more chosen keys are still occupied. Pick a free preset or
                  record another combination before saving.
                </span>
              </div>
            )}

            <div className="mt-3 flex flex-wrap items-center gap-2">
              <button
                type="button"
                disabled={busy || loading}
                onClick={() => void loadScan()}
                className="flex items-center gap-1 rounded border border-[var(--color-border)] px-2.5 py-1 text-[11px] hover:border-[var(--color-accent)] disabled:opacity-50"
              >
                <RefreshCw size={12} />
                Rescan
              </button>
              {scan?.can_configure && (
                <>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void runAutoSetup()}
                    className="rounded border border-[var(--color-border)] px-2.5 py-1 text-[11px] hover:border-[var(--color-accent)] disabled:opacity-50"
                  >
                    Auto-resolve all
                  </button>
                  <button
                    type="button"
                    disabled={busy || anyConflict}
                    onClick={() => void save()}
                    className="rounded bg-[var(--color-accent)] px-3 py-1 text-[12px] text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-50"
                  >
                    {busy ? "Saving…" : "Save shortcuts"}
                  </button>
                </>
              )}
            </div>

            {status && (
              <div
                className={
                  "mt-3 rounded border px-2.5 py-1.5 text-[11px] " +
                  (status.kind === "err"
                    ? "border-red-500/40 bg-red-500/5 text-red-400"
                    : "border-[var(--color-border)] text-[var(--color-muted)]")
                }
              >
                {status.message}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
