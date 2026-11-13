import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, BatteryCharging, BatteryFull, BatteryLow, BatteryMedium, ChevronDown, ChevronRight, Mouse, RefreshCw } from "lucide-react";
import { aiTasksState, listActionHotkeys, logiRefresh, logiRequestAccess, logiSetConfig, logiView } from "../lib/ipc";
import {
  assignableControls,
  batteryLevel,
  buttonLabelKey,
  clampThreshold,
  configComplete,
  DIRECTION_SLOTS,
  hasDirections,
  shownDpi,
  shownWheel,
  SMART_MAX,
  SMART_MIN,
  withAction,
  type ButtonSlot,
  type LogiConfig,
  type LogiStatus,
  type WheelMode,
} from "../lib/logi";
import { isTyping } from "../lib/panel-keys";
import { useT, type MessageKey } from "../lib/i18n";
import { ActionPicker, type ActionChoices } from "./ActionPicker";
import type { BindingAction } from "../lib/gesture-bindings";

/** Changes are saved this long after the last one (a dragged slider, a typed
 *  address). */
export const SAVE_DEBOUNCE_MS = 300;

const chip =
  "md3-press rounded-md border border-[var(--color-border)] px-2 py-1 text-[11px] transition-colors hover:border-[var(--color-accent)]";
const chipOn = "md3-press rounded-md border border-[var(--color-accent)] bg-[var(--color-accent)] px-2 py-1 text-[11px] text-[var(--color-accent-fg)]";
const card = "rounded-md border border-[var(--color-border)] p-2.5";

const WHEEL_MODES: WheelMode[] = ["smart", "ratchet", "free"];

/**
 * `logi` / `mouse` — settings of a Logitech Bluetooth mouse: pointer speed,
 * scroll wheel, buttons. Shows while the keyword is typed (reading the cached
 * status costs nothing); every change is saved and written to the mouse.
 */
export function LogiPanel({
  focused,
  onExit,
  embedded = false,
}: {
  focused: boolean;
  onExit: () => void;
  /** Inside the Settings tab: no own scrolling or padding. */
  embedded?: boolean;
}) {
  const shell = embedded ? "flex flex-col gap-3 text-[12px]" : "flex h-full flex-col gap-3 overflow-y-auto p-3 text-[12px]";
  const t = useT();
  const [status, setStatus] = useState<LogiStatus | null>(null);
  const [draft, setDraft] = useState<LogiConfig | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState<Set<number>>(new Set());
  const [choices, setChoices] = useState<ActionChoices>({ hotkeys: [], tasks: [] });
  const timer = useRef<number | null>(null);
  /** The config the user is editing; a status update must not replace it. */
  const edited = useRef(false);

  const refresh = useCallback(() => {
    setBusy(true);
    logiRefresh()
      .then((v) => {
        setStatus(v.status);
        if (!edited.current) setDraft(v.config);
        setError(null);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(false));
  }, []);

  useEffect(() => {
    logiView()
      .then((v) => {
        setStatus(v.status);
        setDraft(v.config);
      })
      .catch((e) => setError(String(e)));
    refresh();
    listActionHotkeys()
      .then((h) => setChoices((c) => ({ ...c, hotkeys: h.filter((x) => x.id !== "gestureunintended").map((x) => ({ id: x.id, label: x.label })) })))
      .catch(() => {});
    aiTasksState()
      .then((s) => setChoices((c) => ({ ...c, tasks: s.tasks.map((x) => ({ id: x.id, name: x.name, approved: x.approved })) })))
      .catch(() => {});
    // Connect, disconnect and battery changes arrive as an event.
    const un = listen("logi-changed", () => {
      logiView()
        .then((v) => setStatus(v.status))
        .catch(() => {});
    });
    return () => {
      void un.then((f) => f()).catch(() => {});
      if (timer.current !== null) window.clearTimeout(timer.current);
    };
  }, [refresh]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      if (e.key === "Escape") {
        e.preventDefault();
        onExit();
      } else if (!isTyping(e) && (e.key === "r" || e.key === "R") && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        refresh();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focused, onExit, refresh]);

  /** Take a change: show it at once, save it once it is complete. */
  const change = useCallback((next: LogiConfig) => {
    edited.current = true;
    setDraft(next);
    if (timer.current !== null) window.clearTimeout(timer.current);
    if (!configComplete(next)) return;
    timer.current = window.setTimeout(() => {
      timer.current = null;
      logiSetConfig(next)
        .then((v) => {
          setStatus(v.status);
          setError(null);
        })
        .catch((e) => setError(String(e)));
    }, SAVE_DEBOUNCE_MS);
  }, []);

  const setSlot = (cid: number, slot: ButtonSlot, action: BindingAction | null) => {
    if (draft) change(withAction(draft, cid, slot, action));
  };

  if (!status || !draft) {
    return <div className="p-4 text-[12px] text-[var(--color-muted)]">{t("logi.loading")}</div>;
  }

  const header = (
    <div className="flex items-center gap-2.5">
      <Mouse size={18} className="shrink-0 text-[var(--color-accent)]" />
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px] font-medium">{status.connected ? status.name : t("logi.title")}</div>
        <div className="text-[11px] text-[var(--color-muted)]">
          {status.connected ? t("logi.connected") : t("logi.not_connected")}
        </div>
      </div>
      {status.battery && <BatteryChip percent={status.battery.percent} charging={status.battery.charging} />}
      <button type="button" className={chip} onClick={refresh} disabled={busy} title={t("logi.refresh")}>
        <RefreshCw size={11} className={`mr-1 inline ${busy ? "animate-spin" : ""}`} />
        {t("logi.refresh")}
      </button>
    </div>
  );

  if (!status.supported) {
    return (
      <div className={shell}>
        {header}
        <div className={card}>{t("logi.unsupported")}</div>
      </div>
    );
  }

  const wheel = shownWheel(draft, status);
  const dpi = shownDpi(draft, status);
  const invert = draft.invert ?? status.invert;
  const controls = assignableControls(status.controls);

  return (
    <div className={shell}>
      {header}

      {error && <div className="rounded-md border border-rose-500/40 bg-rose-500/10 px-2 py-1.5 text-[11px]">{error}</div>}

      {status.permission_denied && (
        <div className="rounded-md border border-amber-500/40 bg-amber-500/10 p-2.5">
          <div className="mb-1 font-medium">{t("logi.permission.title")}</div>
          <p className="mb-2 text-[11px] text-[var(--color-muted)]">{t("logi.permission.body")}</p>
          <button type="button" className={chip} onClick={() => void logiRequestAccess().catch(() => {})}>
            {t("logi.permission.button")}
          </button>
        </div>
      )}

      {status.options_running && (
        <div className="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-2.5 text-[11px]">
          <AlertTriangle size={13} className="mt-0.5 shrink-0 text-amber-500" />
          <span>{t("logi.options_running")}</span>
        </div>
      )}

      {status.refused && !status.permission_denied && (
        <div className="rounded-md border border-amber-500/40 bg-amber-500/10 p-2.5" role="status" data-testid="logi-refused">
          <div className="mb-1 font-medium">{t("logi.refused.title", { name: status.name || "Logitech" })}</div>
          <p className="text-[11px] text-[var(--color-muted)]">{t(status.secure_input ? "logi.refused.secure" : "logi.refused.body")}</p>
        </div>
      )}

      {!status.connected && !status.permission_denied && !status.refused && (
        <div className={`${card} text-[var(--color-muted)]`}>{t("logi.connect_hint")}</div>
      )}

      {status.connected && status.dpi && (
        <section className={card} aria-label={t("logi.pointer")}>
          <div className="mb-1.5 flex items-baseline justify-between">
            <span className="font-medium">{t("logi.pointer")}</span>
            <span className="text-[11px] text-[var(--color-muted)]">{t("logi.dpi_now", { dpi: dpi ?? status.dpi.current })}</span>
          </div>
          <div className="flex flex-wrap gap-1.5">
            {status.dpi.options.map((o) => (
              <button
                key={o}
                type="button"
                aria-pressed={o === dpi}
                className={o === dpi ? chipOn : chip}
                onClick={() => {
                  setStatus((s) => (s && s.dpi ? { ...s, dpi: { ...s.dpi, current: o } } : s));
                  change({ ...draft, dpi: o });
                }}
              >
                {o}
                {o === status.dpi?.default ? " ·" : ""}
              </button>
            ))}
          </div>
          <p className="mt-1.5 text-[11px] text-[var(--color-muted)]">{t("logi.dpi_hint")}</p>
        </section>
      )}

      {status.connected && (wheel || invert !== null) && (
        <section className={card} aria-label={t("logi.wheel")}>
          <div className="mb-1.5 font-medium">{t("logi.wheel")}</div>
          {wheel && (
            <>
              <div className="flex flex-wrap gap-1.5">
                {WHEEL_MODES.map((m) => (
                  <button
                    key={m}
                    type="button"
                    aria-pressed={wheel.mode === m}
                    className={wheel.mode === m ? chipOn : chip}
                    onClick={() => change({ ...draft, wheel: { ...wheel, mode: m } })}
                  >
                    {t(`logi.wheel.${m}` as MessageKey)}
                  </button>
                ))}
              </div>
              <p className="mt-1.5 text-[11px] text-[var(--color-muted)]">{t(`logi.wheel.${wheel.mode}_hint` as MessageKey)}</p>
              {wheel.mode === "smart" && (
                <label className="mt-2 flex items-center gap-2">
                  <span className="shrink-0 text-[11px] text-[var(--color-muted)]">{t("logi.wheel.threshold")}</span>
                  <input
                    type="range"
                    min={SMART_MIN}
                    max={SMART_MAX}
                    value={wheel.threshold}
                    onChange={(e) => change({ ...draft, wheel: { mode: "smart", threshold: clampThreshold(Number(e.target.value)) } })}
                    className="min-w-0 flex-1 accent-[var(--color-accent)]"
                  />
                  <span className="w-6 text-right font-mono text-[11px]">{wheel.threshold}</span>
                </label>
              )}
            </>
          )}
          {invert !== null && (
            <label className="mt-2 flex cursor-pointer items-center gap-2">
              <input
                type="checkbox"
                checked={invert}
                onChange={(e) => change({ ...draft, invert: e.target.checked })}
                className="accent-[var(--color-accent)]"
              />
              <span>{t("logi.invert")}</span>
            </label>
          )}
          {invert !== null && <p className="mt-1 text-[11px] text-[var(--color-muted)]">{t("logi.invert_hint")}</p>}
        </section>
      )}

      {status.connected && controls.length > 0 && (
        <section className={card} aria-label={t("logi.buttons")}>
          <div className="mb-1 font-medium">{t("logi.buttons")}</div>
          <p className="mb-2 text-[11px] text-[var(--color-muted)]">{t("logi.buttons_hint")}</p>
          <div className="flex flex-col gap-2">
            {controls.map((c) => {
              const b = draft.buttons[String(c.cid)];
              const expanded = open.has(c.cid) || hasDirections(b);
              const name = t(buttonLabelKey(c.cid));
              return (
                <div key={c.cid} className="border-t border-[var(--color-border)] pt-2 first:border-t-0 first:pt-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="w-28 shrink-0">{name}</span>
                    <ActionPicker
                      value={b?.press ?? null}
                      onChange={(a) => setSlot(c.cid, "press", a)}
                      noneLabel={b && hasDirections(b) ? t("logi.none") : t("logi.default")}
                      label={t("logi.press_of", { button: name })}
                      choices={choices}
                    />
                  </div>
                  <button
                    type="button"
                    className="mt-1 flex items-center gap-1 text-[11px] text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                    aria-expanded={expanded}
                    disabled={hasDirections(b)}
                    onClick={() =>
                      setOpen((s) => {
                        const n = new Set(s);
                        if (n.has(c.cid)) n.delete(c.cid);
                        else n.add(c.cid);
                        return n;
                      })
                    }
                  >
                    {expanded ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
                    {t("logi.gestures")}
                  </button>
                  {expanded && (
                    <div className="mt-1.5 flex flex-col gap-1.5 pl-3">
                      {DIRECTION_SLOTS.map((slot) => (
                        <div key={slot} className="flex flex-wrap items-center gap-2">
                          <span className="w-[100px] shrink-0 text-[11px] text-[var(--color-muted)]">{t(`logi.dir.${slot}` as MessageKey)}</span>
                          <ActionPicker
                            value={b?.[slot] ?? null}
                            onChange={(a) => setSlot(c.cid, slot, a)}
                            noneLabel={t("logi.none")}
                            label={t("logi.dir_of", { button: name, dir: t(`logi.dir.${slot}` as MessageKey) })}
                            choices={choices}
                          />
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
          {!configComplete(draft) && <p className="mt-2 text-[11px] text-amber-500">{t("logi.incomplete")}</p>}
        </section>
      )}

      {focused && <div className="text-[11px] text-[var(--color-muted)]">{t("logi.keys")}</div>}
    </div>
  );
}

function BatteryChip({ percent, charging }: { percent: number; charging: boolean }) {
  const t = useT();
  const level = batteryLevel(percent);
  const Icon = charging ? BatteryCharging : level === "low" ? BatteryLow : level === "mid" ? BatteryMedium : BatteryFull;
  const color = level === "low" && !charging ? "text-rose-500" : level === "mid" && !charging ? "text-amber-500" : "text-emerald-500";
  return (
    <span className="flex shrink-0 items-center gap-1 text-[11px]" title={charging ? t("logi.charging") : t("logi.battery")}>
      <Icon size={14} className={color} />
      <span className="font-mono">{percent} %</span>
    </span>
  );
}
