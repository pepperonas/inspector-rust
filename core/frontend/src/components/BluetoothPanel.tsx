import { useCallback, useEffect, useRef, useState } from "react";
import { Bluetooth, BluetoothConnected, RefreshCw, Trash2 } from "lucide-react";
import {
  bluetoothConnect,
  bluetoothDisconnect,
  bluetoothList,
  bluetoothUnpair,
  type BtDevice,
} from "../lib/ipc";
import { t, useT } from "../lib/i18n";

/**
 * `bluetooth` / `bt` — manage paired devices inline (macOS, v0.159.0).
 *
 * Shows while fully typed (the `sound` pattern — listing paired devices is a
 * cheap sync read, no radio scan); Enter hands over the arrow keys. Actions:
 * connect/disconnect per device, and unpair behind a TWO-STAGE inline confirm
 * — never `window.confirm` (unreliable in the Tauri webview, the TOTP-delete
 * lesson), and unpairing throws the pairing record away, so a single stray
 * Enter must not be able to do it.
 *
 * ⚠️ Connect can take ~10 s against a switched-off device (the OS timeout) —
 * the row shows a spinner and stays interactive-locked ONLY for that device;
 * a second device's disconnect must not queue behind it.
 */
const POLL_MS = 3000;

export function BluetoothPanel({
  focused,
  onExit,
}: {
  focused: boolean;
  onExit: () => void;
}) {
  useT();
  const [devices, setDevices] = useState<BtDevice[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sel, setSel] = useState(0);
  /** Addresses with an action in flight (connect may block ~10 s). */
  const [busy, setBusy] = useState<Set<string>>(new Set());
  /** Address armed for unpair — the first stage of the confirm. */
  const [armed, setArmed] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);

  const refresh = useCallback(() => {
    bluetoothList()
      .then((d) => {
        setDevices(d);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refresh();
    const id = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(id);
  }, [refresh]);

  const withBusy = useCallback(
    (address: string, run: () => Promise<void>, done: string) => {
      setBusy((b) => new Set(b).add(address));
      setNote(null);
      run()
        .then(() => setNote(done))
        .catch((e) => {
          const msg = String(e);
          setNote(
            msg.includes("device_not_found")
              ? t("bt.device_is_no_longer_paired")
              : msg.includes("remove_unavailable")
                ? t("bt.unpairing_isn_t_available_macos")
                : msg,
          );
        })
        .finally(() => {
          setBusy((b) => {
            const n = new Set(b);
            n.delete(address);
            return n;
          });
          refresh();
        });
    },
    [refresh],
  );

  const toggle = useCallback(
    (d: BtDevice) => {
      setArmed(null);
      if (d.connected) {
        withBusy(d.address, () => bluetoothDisconnect(d.address), `${d.name} getrennt.`);
      } else {
        withBusy(d.address, () => bluetoothConnect(d.address), `${d.name} verbunden.`);
      }
    },
    [withBusy],
  );

  const unpair = useCallback(
    (d: BtDevice) => {
      setArmed(null);
      withBusy(d.address, () => bluetoothUnpair(d.address), `${d.name} entkoppelt.`);
    },
    [withBusy],
  );

  // Keyboard while focused: ↑↓ select · Enter connect/disconnect · ⌫ arm/confirm
  // unpair · Esc disarm, then exit.
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) {
        if (e.key !== "Escape") return;
      }
      const list = devices ?? [];
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setArmed(null);
        setSel((s) => {
          const n = e.key === "ArrowDown" ? s + 1 : s - 1;
          return Math.max(0, Math.min(list.length - 1, n));
        });
      } else if (e.key === "Enter" && list[sel]) {
        e.preventDefault();
        toggle(list[sel]);
      } else if (e.key === "Backspace" && list[sel]) {
        e.preventDefault();
        const d = list[sel];
        if (armed === d.address) unpair(d);
        else setArmed(d.address);
      } else if (e.key === "Escape") {
        e.preventDefault();
        if (armed) setArmed(null);
        else onExit();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focused, devices, sel, armed, toggle, unpair, onExit]);

  const chip =
    "md3-press rounded border border-[var(--color-border)] px-2 py-0.5 text-[11px] hover:border-[var(--color-accent)] disabled:opacity-40";

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[12px]" ref={listRef}>
      <div className="flex items-baseline justify-between">
        <div>
          <div className="text-[13px] font-semibold text-[var(--color-fg)]">Bluetooth</div>
          <div className="text-[var(--color-muted)]">
            {devices === null
              ? t("bt.loading_paired_devices")
              : t("bt.connected_of", { n: devices.filter((d) => d.connected).length, total: devices.length })}
          </div>
        </div>
        <button type="button" className={chip} onClick={refresh} title={t("bt.refresh")}>
          <RefreshCw size={11} className="mr-1 inline" />
          {t("bt.refresh")}
        </button>
      </div>

      {error && (
        <div className="rounded-md border border-rose-500/40 bg-rose-500/10 px-2 py-1.5 text-[11px]">
          {error}
        </div>
      )}

      <div className="rounded-md border border-[var(--color-border)]">
        {(devices ?? []).map((d, i) => {
          const isBusy = busy.has(d.address);
          const isArmed = armed === d.address;
          const selected = focused && i === sel;
          return (
            <div
              key={d.address}
              className={`flex items-center gap-2.5 px-2.5 py-2 ${
                i > 0 ? "border-t border-[var(--color-border)]" : ""
              } ${selected ? "bg-[var(--color-accent)]/10" : ""}`}
            >
              {d.connected ? (
                <BluetoothConnected size={14} className="shrink-0 text-[var(--color-accent)]" />
              ) : (
                <Bluetooth size={14} className="shrink-0 text-[var(--color-muted)]" />
              )}
              <div className="min-w-0 flex-1">
                <div className="flex items-baseline gap-2">
                  <span className="truncate font-medium text-[var(--color-fg)]">{d.name}</span>
                  <span
                    className={`h-1.5 w-1.5 shrink-0 rounded-full ${
                      d.connected ? "bg-emerald-400" : "bg-[var(--color-border)]"
                    }`}
                    aria-hidden
                  />
                </div>
                {/* ⚠️ The address is part of the surface: a device can be
                    paired TWICE (classic + LE, measured) — without it the
                    twins are indistinguishable. */}
                <div className="font-[var(--font-mono)] text-[10px] text-[var(--color-muted)]">
                  {d.kind} · {d.address}
                </div>
              </div>
              {isBusy ? (
                <span className="text-[11px] text-[var(--color-muted)]">
                  {d.connected ? t("bt.disconnecting") : t("bt.connecting")}
                </span>
              ) : isArmed ? (
                <span className="flex items-center gap-1.5">
                  <span className="text-[11px] text-rose-400">{t("bt.remove_pairing")}</span>
                  <button type="button" className={chip} onClick={() => unpair(d)}>
                    {t("bt.yes_unpair")}
                  </button>
                  <button type="button" className={chip} onClick={() => setArmed(null)}>
                    {t("common.cancel")}
                  </button>
                </span>
              ) : (
                <span className="flex items-center gap-1.5">
                  <button type="button" className={chip} onClick={() => toggle(d)}>
                    {d.connected ? t("bt.disconnect") : t("bt.connect")}
                  </button>
                  <button
                    type="button"
                    className={chip}
                    title={t("bt.unpair_removes_the_pairing_for")}
                    onClick={() => setArmed(d.address)}
                  >
                    <Trash2 size={11} />
                  </button>
                </span>
              )}
            </div>
          );
        })}
        {devices !== null && devices.length === 0 && (
          <div className="px-2.5 py-3 text-[11px] text-[var(--color-muted)]">
            {t("bt.no_paired_devices")}
          </div>
        )}
      </div>

      {note && <div className="text-[11px] text-[var(--color-muted)]">{note}</div>}
      <div className="mt-auto font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
        {t("bt.select_connect_disconnect_unpair_2")}
        <br />
        {t("bt.connecting_to_a_switched_off")}
      </div>
    </div>
  );
}
