import { useCallback, useEffect, useRef, useState } from "react";
import { Power, Plug, BatteryMedium, RefreshCw } from "lucide-react";
import { nosleepStatus, nosleepSet, type NoSleepStatus } from "../lib/ipc";
import { t, useT } from "../lib/i18n";
import { InlineMd } from "./InlineMd";

/**
 * `nosleep` — toggle the PERSISTENT AC idle-sleep profile (v0.124.0, macOS).
 * Shows the live `pmset` AC + battery sleep values and a big on/off switch
 * that runs `pmset -c sleep 0|1` behind one admin prompt. Distinct from
 * `wakelock dark` (a session assertion): this survives reboots. Enter-
 * activated. The footer's sleep-status indicator reflects the result.
 */
export function NoSleepPanel({ arg, focused, onExit }: { arg: string; focused: boolean; onExit: () => void }) {
  useT();
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

  const fmt = (min: number | null) => (min == null ? "—" : min === 0 ? t("nosleep.never") : t("nosleep.minutes", { n: min }));

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <Power size={15} className="text-[var(--color-accent)]" /> {t("nosleep.permanent_sleep_lock")}
        </div>
        <button type="button" onClick={refresh} title={t("nosleep.refresh")} className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]">
          <RefreshCw size={13} />
        </button>
      </div>

      {status && !status.supported ? (
        <p className="text-[12px] text-[var(--color-muted)]">{t("nosleep.only_available_on_macos")}</p>
      ) : status === null ? (
        <p className="text-[12px] text-[var(--color-muted)]">{t("nosleep.reading_the_power_profile")}</p>
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
                {status.ac_disabled ? t("nosleep.active_the_mac_never_sleeps") : t("nosleep.off_sleeps_normally")}
              </span>
              <span className="mt-0.5 block text-[11px] text-[var(--color-muted)]">
                {busy ? t("nosleep.applying_admin_dialog") : status.ac_disabled ? t("nosleep.click_to_turn_off") : t("nosleep.click_to_turn_on_admin")}
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
                <Plug size={12} /> {t("nosleep.ac_power")}
              </div>
              <div className="mt-0.5 text-[15px] font-semibold">{fmt(status.ac_sleep)}</div>
              <div className="text-[10px] text-[var(--color-muted)]">{t("nosleep.idle_sleep")}</div>
            </div>
            <div className="rounded-xl border border-[var(--color-border)] p-3">
              <div className="flex items-center gap-1.5 text-[11px] text-[var(--color-muted)]">
                <BatteryMedium size={12} /> {t("nosleep.battery")}
              </div>
              <div className="mt-0.5 text-[15px] font-semibold">{fmt(status.battery_sleep)}</div>
              <div className="text-[10px] text-[var(--color-muted)]">{t("nosleep.idle_sleep_unchanged")}</div>
            </div>
          </div>

          <p className="text-[11px] leading-snug text-[var(--color-muted)]">
            <InlineMd text={t("nosleep.explain")} />
          </p>
        </>
      )}
      {focused && <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">{t("nosleep.esc_close")}</p>}
    </div>
  );
}
