import { useEffect, useMemo, useRef, useState } from "react";
import { MonitorIcon } from "lucide-react";
import {
  listBrightnessMonitors,
  setMonitorBrightness,
  type MonitorInfo,
} from "../lib/ipc";

/**
 * In-popup brightness control rendered in the **right preview column** (not a
 * separate window). Entered by pressing Enter on the `brightness` command row.
 *
 * Keyboard model (when `focused`):
 *   ↑ / ↓     select the monitor
 *   ← / →     adjust the selected monitor's brightness (±5)
 *   Enter     hand the arrow keys back to the left list (`onUnfocus`)
 *   Esc       leave brightness mode entirely (`onExit`)
 *
 * macOS + Windows dim in software (gamma); Linux uses hardware DDC. Writes are
 * debounced ~80 ms so a held arrow key doesn't flood the backend.
 */
const STEP = 5;
const MIN = 10;

export function BrightnessPanel({
  focused,
  onUnfocus,
  onExit,
}: {
  focused: boolean;
  onUnfocus: () => void;
  onExit: () => void;
}) {
  const [monitors, setMonitors] = useState<MonitorInfo[] | null>(null);
  const [values, setValues] = useState<Record<number, number>>({});
  const [sel, setSel] = useState(0);
  const timers = useRef<Record<number, number>>({});

  useEffect(() => {
    let alive = true;
    listBrightnessMonitors()
      .then((m) => {
        if (!alive) return;
        setMonitors(m);
        setValues(Object.fromEntries(m.map((x) => [x.id, x.brightness])));
      })
      .catch(() => {
        if (alive) setMonitors([]);
      });
    return () => {
      alive = false;
    };
  }, []);

  const controllable = useMemo(
    () => monitors?.filter((m) => m.supports_ddc) ?? [],
    [monitors],
  );

  const push = (id: number, percent: number) => {
    if (timers.current[id]) window.clearTimeout(timers.current[id]);
    timers.current[id] = window.setTimeout(() => {
      void setMonitorBrightness(id, percent).catch(() => undefined);
    }, 80);
  };

  // Arrow / Enter / Esc handling while the sliders own the keyboard. Inlined
  // in the effect (re-subscribes on sel/controllable change) so it always
  // reads the current selection — no stale closure.
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      const adjust = (delta: number) => {
        const m = controllable[sel];
        if (!m) return;
        setValues((v) => {
          const cur = v[m.id] ?? m.brightness;
          const next = Math.max(MIN, Math.min(100, cur + delta));
          push(m.id, next);
          return { ...v, [m.id]: next };
        });
      };
      switch (e.key) {
        case "ArrowUp":
          e.preventDefault();
          e.stopPropagation();
          setSel((s) => Math.max(0, s - 1));
          break;
        case "ArrowDown":
          e.preventDefault();
          e.stopPropagation();
          setSel((s) => Math.min(controllable.length - 1, s + 1));
          break;
        case "ArrowLeft":
          e.preventDefault();
          e.stopPropagation();
          adjust(-STEP);
          break;
        case "ArrowRight":
          e.preventDefault();
          e.stopPropagation();
          adjust(STEP);
          break;
        case "Enter":
          e.preventDefault();
          e.stopPropagation();
          onUnfocus();
          break;
        case "Escape":
          e.preventDefault();
          e.stopPropagation();
          onExit();
          break;
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, controllable, sel, onUnfocus, onExit]);

  // Keep the selection in range if the monitor list shrinks.
  useEffect(() => {
    if (sel >= controllable.length && controllable.length > 0) {
      setSel(controllable.length - 1);
    }
  }, [controllable.length, sel]);

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)]">
      <div className="flex items-center gap-2 text-[13px] font-medium">
        <MonitorIcon size={15} className="text-[var(--color-accent)]" /> Brightness
      </div>

      {monitors === null ? (
        <p className="text-[12px] text-[var(--color-muted)]">Probing monitors…</p>
      ) : controllable.length === 0 ? (
        <p className="text-[12px] text-[var(--color-muted)]">
          No controllable monitors found.
        </p>
      ) : (
        <div className="flex flex-col gap-3">
          {controllable.map((m, i) => {
            const value = values[m.id] ?? m.brightness;
            const active = focused && i === sel;
            return (
              <button
                key={m.id}
                type="button"
                onClick={() => setSel(i)}
                className={
                  "flex flex-col gap-1 rounded-lg border px-3 py-2 text-left transition-colors " +
                  (active
                    ? "border-[var(--color-accent)] bg-[var(--color-accent)]/10"
                    : "border-[var(--color-border)] hover:bg-[var(--color-surface)]")
                }
              >
                <span className="flex items-center justify-between text-[12px]">
                  <span className="truncate pr-2">{m.name}</span>
                  <span className="tabular-nums font-medium">{value}%</span>
                </span>
                <span className="h-2 w-full overflow-hidden rounded-full bg-[var(--color-border)]">
                  <span
                    className="block h-full rounded-full bg-[var(--color-accent)] transition-[width]"
                    style={{ width: `${value}%` }}
                  />
                </span>
              </button>
            );
          })}

          <p className="mt-1 text-[11px] text-[var(--color-muted)]">
            {focused ? (
              <>↑ ↓ select · ← → adjust · Enter back to list · Esc close</>
            ) : (
              <>Press Enter on the brightness row to control the sliders.</>
            )}
          </p>
          <p className="text-[10px] text-[var(--color-muted)]">
            Software dimming — adjusts emitted light, not the backlight.
          </p>
        </div>
      )}
    </div>
  );
}
