import { useCallback, useEffect, useRef, useState } from "react";
import { Power, Plug, BatteryMedium, RefreshCw } from "lucide-react";
import { nosleepStatus, nosleepSet, type NoSleepStatus } from "../lib/ipc";

/**
 * `nosleep` — toggle the PERSISTENT AC idle-sleep profile (v0.124.0, macOS).
 * Shows the live `pmset` AC + battery sleep values and a big on/off switch
 * that runs `pmset -c sleep 0|1` behind one admin prompt. Distinct from
 * `wakelock dark` (a session assertion): this survives reboots. Enter-
 * activated. The footer's sleep-status indicator reflects the result.
 */
export function NoSleepPanel({ arg, focused, onExit }: { arg: string; focused: boolean; onExit: () => void }) {
  const [status, setStatus] = useState<NoSleepStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const aliveRef = useRef(true);

  const refresh = useCallback(() => {
    nosleepStatus()
      .then((s) => {
        if (aliveRef.current) setStatus(s);
      })
      .catch((e) => {
        if (aliveRef.current) setErr(String(e));
      });
  }, []);

  useEffect(() => {
    aliveRef.current = true;
    refresh();
    return () => {
      aliveRef.current = false;
    };
  }, [refresh]);

  const apply = useCallback(
    (disable: boolean) => {
      setBusy(true);
      setErr(null);
      nosleepSet(disable)
        .then((s) => {
          if (aliveRef.current) setStatus(s);
        })
        .catch((e) => {
          if (aliveRef.current) setErr(String(e));
        })
        .finally(() => {
          if (aliveRef.current) setBusy(false);
        });
    },
    [],
  );

  // `nosleep on` / `nosleep off` from the command bar act immediately.
  const actedRef = useRef(false);
  useEffect(() => {
    const a = arg.trim().toLowerCase();
    if (!actedRef.current && (a === "on" || a === "off")) {
      actedRef.current = true;
      apply(a === "on");
    }
  }, [arg, apply]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit]);

  const fmt = (min: number | null) => (min == null ? "—" : min === 0 ? "nie" : min === 1 ? "1 min" : `${min} min`);

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <Power size={15} className="text-[var(--color-accent)]" /> Dauerschlaf-Sperre
        </div>
        <button type="button" onClick={refresh} title="Aktualisieren" className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]">
          <RefreshCw size={13} />
        </button>
      </div>

      {status && !status.supported ? (
        <p className="text-[12px] text-[var(--color-muted)]">Nur auf macOS verfügbar.</p>
      ) : status === null ? (
        <p className="text-[12px] text-[var(--color-muted)]">Lese Energieprofil…</p>
      ) : (
        <>
          {/* The switch. */}
          <button
            type="button"
            disabled={busy}
            onClick={() => apply(!status.ac_disabled)}
            className={
              "flex items-center justify-between gap-3 rounded-xl border p-3 text-left transition-colors disabled:opacity-50 " +
              (status.ac_disabled
                ? "border-[var(--color-accent)] bg-[color-mix(in_srgb,var(--color-accent)_12%,transparent)]"
                : "border-[var(--color-border)] hover:border-[var(--color-accent)]")
            }
          >
            <span>
              <span className="block text-[13px] font-medium">
                {status.ac_disabled ? "Aktiv — Mac schläft am Netzteil nie" : "Aus — normales Einschlafen"}
              </span>
              <span className="mt-0.5 block text-[11px] text-[var(--color-muted)]">
                {busy ? "Wende an (Admin-Dialog)…" : status.ac_disabled ? "Tippen zum Ausschalten" : "Tippen zum Einschalten (Admin nötig)"}
              </span>
            </span>
            {/* Track/knob. */}
            <span
              className="relative h-6 w-11 shrink-0 rounded-full transition-colors"
              style={{ backgroundColor: status.ac_disabled ? "var(--color-accent)" : "var(--color-border)" }}
            >
              <span
                className="absolute top-0.5 left-0.5 h-5 w-5 rounded-full bg-white transition-transform"
                style={{ transform: status.ac_disabled ? "translateX(20px)" : "translateX(0)" }}
              />
            </span>
          </button>

          {err && <p className="text-[11px] text-amber-500">{err}</p>}

          {/* Live profile readout. */}
          <div className="grid grid-cols-2 gap-2">
            <div className="rounded-xl border border-[var(--color-border)] p-3">
              <div className="flex items-center gap-1.5 text-[11px] text-[var(--color-muted)]">
                <Plug size={12} /> Netzteil
              </div>
              <div className="mt-0.5 text-[15px] font-semibold">{fmt(status.ac_sleep)}</div>
              <div className="text-[10px] text-[var(--color-muted)]">Idle-Sleep</div>
            </div>
            <div className="rounded-xl border border-[var(--color-border)] p-3">
              <div className="flex items-center gap-1.5 text-[11px] text-[var(--color-muted)]">
                <BatteryMedium size={12} /> Batterie
              </div>
              <div className="mt-0.5 text-[15px] font-semibold">{fmt(status.battery_sleep)}</div>
              <div className="text-[10px] text-[var(--color-muted)]">Idle-Sleep (unberührt)</div>
            </div>
          </div>

          <p className="text-[11px] leading-snug text-[var(--color-muted)]">
            Schreibt das <b>dauerhafte</b> Energieprofil (<code>pmset -c sleep 0</code>) — überlebt
            Neustarts, bis du es ausschaltest. Braucht Admin-Rechte. Für nur diese Sitzung ohne
            Admin gibt es <code>wakelock dark</code>. Der Bildschirm schläft weiterhin normal; nur
            der <em>System</em>-Sleep am Netzteil ist gesperrt.
          </p>
        </>
      )}
      {focused && <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">Esc schließen</p>}
    </div>
  );
}
