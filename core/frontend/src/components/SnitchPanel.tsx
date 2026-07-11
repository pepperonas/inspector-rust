/**
 * `snitch` — network monitor + best-effort per-app outbound blocker (macOS).
 * Inline panel (same family as stats/hue/boom). Lists apps with live network
 * connections; each has an allow/block toggle. Blocking is BEST-EFFORT (a pf
 * watcher daemon pushes the app's remote IPs into a block table) — clearly
 * labelled, never presented as a real firewall (that needs a system extension
 * Apple won't grant a self-signed app). Keyboard: ↑/↓ select · Space/Enter
 * toggle block · Esc exit. `snitch map` opens the world-map view instead.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { Ban, Globe, Loader2, Shield, ShieldOff, Wifi } from "lucide-react";
import {
  snitchArm,
  snitchDisarm,
  snitchIsArmed,
  snitchListApps,
  snitchSetBlocked,
  type SnitchApp,
} from "../lib/ipc";

export function SnitchPanel({
  focused,
  onInteract,
  onExit,
}: {
  focused: boolean;
  onInteract?: () => void;
  onExit: () => void;
}) {
  const [apps, setApps] = useState<SnitchApp[]>([]);
  const [loading, setLoading] = useState(true);
  const [armed, setArmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [row, setRow] = useState(0);
  const rowRefs = useRef<(HTMLDivElement | null)[]>([]);

  const refresh = useCallback(async () => {
    try {
      const [a, arm] = await Promise.all([snitchListApps(), snitchIsArmed()]);
      setApps(a);
      setArmed(arm);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), 2500);
    return () => window.clearInterval(id);
  }, [refresh]);

  const blockedCount = apps.filter((a) => a.blocked).length;

  const toggle = useCallback(
    async (app: SnitchApp) => {
      const next = apps.map((a) =>
        a.key === app.key ? { ...a, blocked: !a.blocked } : a,
      );
      setApps(next);
      const blocked = next.filter((a) => a.blocked).map((a) => a.command);
      try {
        await snitchSetBlocked(blocked);
        // First block arms the watcher (one admin prompt); clearing the last
        // block leaves it running with an empty table (disarm via the header).
        if (blocked.length > 0 && !armed && !busy) {
          setBusy(true);
          try {
            await snitchArm();
            setArmed(true);
          } catch (e) {
            const msg = String(e);
            if (!msg.includes("cancelled")) setError(msg);
            // Arm was declined → roll the toggle back so state stays honest.
            setApps((cur) => cur.map((a) => (a.key === app.key ? { ...a, blocked: false } : a)));
            await snitchSetBlocked(
              next.filter((a) => a.blocked && a.key !== app.key).map((a) => a.command),
            );
          } finally {
            setBusy(false);
          }
        }
      } catch (e) {
        setError(String(e));
      }
    },
    [apps, armed, busy],
  );

  const disarm = useCallback(async () => {
    setBusy(true);
    try {
      await snitchDisarm();
      await snitchSetBlocked([]);
      setApps((cur) => cur.map((a) => ({ ...a, blocked: false })));
      setArmed(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      switch (e.key) {
        case "ArrowUp":
          setRow((r) => Math.max(0, r - 1));
          break;
        case "ArrowDown":
          setRow((r) => Math.min(apps.length - 1, r + 1));
          break;
        case " ":
        case "Enter":
          if (apps[row]) void toggle(apps[row]);
          break;
        case "Escape":
          onExit();
          break;
        default:
          return;
      }
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, apps, row, toggle, onExit]);

  useEffect(() => {
    rowRefs.current[row]?.scrollIntoView({ block: "nearest" });
  }, [row]);

  return (
    <div className="flex h-full flex-col gap-2 overflow-hidden p-3 text-sm">
      <div className="flex items-center gap-2 text-[var(--color-fg)]">
        <Wifi size={16} className="text-rose-400" />
        <span className="font-semibold">Network monitor</span>
        {armed ? (
          <button
            type="button"
            onClick={() => {
              void disarm();
              onInteract?.();
            }}
            disabled={busy}
            className="ml-auto flex items-center gap-1 rounded-full bg-rose-600/90 px-2.5 py-1 text-xs font-semibold text-white hover:bg-rose-600"
            title="Stop blocking + restore the firewall"
          >
            <Shield size={12} /> Blocking on
          </button>
        ) : (
          <span className="ml-auto flex items-center gap-1 text-xs text-[var(--color-muted)]">
            <ShieldOff size={12} /> Blocking off
          </span>
        )}
      </div>

      {loading ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 text-[var(--color-muted)]">
          <Loader2 size={24} className="animate-spin" />
          <div>Reading connections…</div>
        </div>
      ) : (
        <div className="min-h-0 flex-1 space-y-1 overflow-y-auto pr-1">
          {apps.length === 0 && (
            <div className="pt-6 text-center text-[var(--color-muted)]">
              No apps are connected right now.
            </div>
          )}
          {apps.map((a, i) => {
            const sel = i === row;
            return (
              <div
                key={a.key}
                ref={(el) => {
                  rowRefs.current[i] = el;
                }}
                className={
                  "cursor-pointer rounded-lg border px-2.5 py-2 transition-colors " +
                  (sel
                    ? "border-rose-500/60 bg-rose-500/10"
                    : "border-[var(--color-border)] bg-[var(--color-surface)]")
                }
                onClick={() => {
                  setRow(i);
                  void toggle(a);
                  onInteract?.();
                }}
              >
                <div className="flex items-center gap-2">
                  {a.blocked ? (
                    <Ban size={16} className="shrink-0 text-rose-500" />
                  ) : (
                    <Globe size={16} className="shrink-0 text-[var(--color-muted)]" />
                  )}
                  <span
                    className={
                      "min-w-0 flex-1 truncate " +
                      (a.blocked
                        ? "text-[var(--color-muted)] line-through"
                        : "text-[var(--color-fg)]")
                    }
                    title={a.remotes.join("\n")}
                  >
                    {a.command}
                  </span>
                  <span className="shrink-0 text-xs tabular-nums text-[var(--color-muted)]">
                    {a.connection_count} conn
                  </span>
                  <span
                    className={
                      "w-12 shrink-0 rounded-full px-1.5 py-0.5 text-center text-[11px] font-semibold " +
                      (a.blocked
                        ? "bg-rose-600 text-white"
                        : "bg-[var(--color-border)] text-[var(--color-muted)]")
                    }
                  >
                    {a.blocked ? "Blocked" : "Allow"}
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {error && (
        <div className="rounded bg-amber-500/15 px-2 py-1 text-xs text-amber-300">{error}</div>
      )}
      <div className="text-center text-[11px] leading-tight text-[var(--color-muted)]">
        {blockedCount > 0 && armed
          ? `Best-effort blocking ${blockedCount} app${blockedCount === 1 ? "" : "s"} via pf — first packets of new connections may leak.`
          : "Toggle an app to block its internet (best-effort, needs admin once). `snitch map` shows the world map."}
      </div>
    </div>
  );
}
