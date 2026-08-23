import { useCallback, useEffect, useRef, useState } from "react";
import {
  Smartphone,
  Camera,
  Video,
  RefreshCw,
  Wifi,
  Bluetooth,
  Plane,
  BellOff,
  Sun,
  Volume2,
  Lock,
  Moon,
  Play,
  Square,
  Trash2,
  XCircle,
} from "lucide-react";
import {
  adbStatus,
  adbDashboard,
  adbSet,
  adbKey,
  adbText,
  adbTap,
  adbSwipe,
  adbScreenshot,
  adbRecordStart,
  adbRecordStop,
  adbPackages,
  adbAppAction,
  adbWifiTcpip,
  adbWifiConnect,
  adbWifiDisconnect,
  type AdbStatus,
  type AdbDashboard,
} from "../lib/ipc";
import {
  NAV_KEYS,
  DPAD_KEYS,
  filterPackages,
  kbHuman,
  uptimeHuman,
  deviceLabel,
  validTap,
  validSwipe,
  textSendable,
} from "../lib/adb";
import { confirmDialog } from "../lib/confirm";

/**
 * `adb` — Android device control in the preview column (v0.119.0). The
 * popup-sized companion to ADBOSS: five views (Info · Steuern · Remote ·
 * Apps · WLAN) behind a chip row, preselectable via the command arg
 * (`adb remote` / `adb apps` / `adb wifi`). Enter-activated; the dashboard
 * polls every 5 s while visible (the ADBOSS default cadence).
 */
export type AdbView = "info" | "control" | "remote" | "apps" | "wifi";

const POLL_MS = 5000;

export function AdbPanel({
  initialView,
  focused,
  onExit,
}: {
  initialView: AdbView;
  focused: boolean;
  onExit: () => void;
}) {
  const [status, setStatus] = useState<AdbStatus | null>(null);
  const [serial, setSerial] = useState<string | null>(null);
  const [view, setView] = useState<AdbView>(initialView);
  const [dash, setDash] = useState<AdbDashboard | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const aliveRef = useRef(true);
  const scrollRef = useRef<HTMLDivElement>(null);

  // Device discovery — on mount + every poll (a plugged/unplugged phone must
  // surface without reopening the panel).
  const refreshStatus = useCallback(() => {
    adbStatus()
      .then((s) => {
        if (!aliveRef.current) return;
        setStatus(s);
        setErr(null);
        setSerial((cur) => {
          const online = s.devices.filter((d) => d.state === "device");
          if (cur && s.devices.some((d) => d.serial === cur)) return cur;
          return online[0]?.serial ?? s.devices[0]?.serial ?? null;
        });
      })
      .catch((e) => {
        if (aliveRef.current) setErr(String(e));
      });
  }, []);

  useEffect(() => {
    aliveRef.current = true;
    refreshStatus();
    const id = window.setInterval(refreshStatus, POLL_MS);
    return () => {
      aliveRef.current = false;
      window.clearInterval(id);
    };
  }, [refreshStatus]);

  // Dashboard poll — only in the Info view, only with an authorized device.
  const device = status?.devices.find((d) => d.serial === serial) ?? null;
  const deviceReady = device?.state === "device";
  useEffect(() => {
    if (view !== "info" || !serial || !deviceReady) return;
    let stale = false;
    const tick = () => {
      adbDashboard(serial)
        .then((d) => {
          if (!stale && aliveRef.current) setDash(d);
        })
        .catch(() => undefined); // transient — keep the last reading
    };
    tick();
    const id = window.setInterval(tick, POLL_MS);
    return () => {
      stale = true;
      window.clearInterval(id);
    };
  }, [view, serial, deviceReady]);

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

  const flash = (msg: string) => {
    setNote(msg);
    window.setTimeout(() => setNote((n) => (n === msg ? null : n)), 2500);
  };

  return (
    <div
      ref={scrollRef}
      className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]"
    >
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <Smartphone size={15} className="text-[var(--color-accent)]" /> Android
        </div>
        <button
          type="button"
          onClick={refreshStatus}
          title="Geräte neu suchen"
          className="rounded-md p-1 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
        >
          <RefreshCw size={13} />
        </button>
      </div>

      {status === null ? (
        <p className="text-[12px] text-[var(--color-muted)]">Suche Geräte…</p>
      ) : !status.found ? (
        <SetupCard
          title="adb ist nicht installiert."
          body="brew install android-platform-tools — danach hier neu suchen. Details: Settings → Android (adb)."
        />
      ) : (
        <>
          <DevicePicker
            status={status}
            serial={serial}
            onPick={setSerial}
          />
          {device?.state === "unauthorized" && (
            <SetupCard
              title="Gerät nicht autorisiert."
              body="Auf dem Handy den Dialog „USB-Debugging zulassen?“ bestätigen (Haken bei „immer erlauben“), dann neu suchen."
            />
          )}
          {status.devices.length === 0 && (
            <SetupCard
              title="Kein Gerät verbunden."
              body="Handy per USB anschließen (Entwickleroptionen → USB-Debugging an) — oder unter WLAN per IP verbinden."
            />
          )}
          <ViewChips view={view} onPick={setView} />
          {err && <p className="text-[11px] text-amber-500">{err}</p>}
          {note && <p className="text-[11px] text-emerald-500">{note}</p>}

          {view === "info" && deviceReady && serial && (
            <InfoView dash={dash} recording={status.recording} serial={serial} onFlash={flash} onStatus={refreshStatus} />
          )}
          {view === "control" && deviceReady && serial && (
            <ControlView serial={serial} dash={dash} onFlash={flash} />
          )}
          {view === "remote" && deviceReady && serial && (
            <RemoteView serial={serial} onFlash={flash} />
          )}
          {view === "apps" && deviceReady && serial && (
            <AppsView serial={serial} onFlash={flash} />
          )}
          {view === "wifi" && <WifiView status={status} onFlash={flash} onChanged={refreshStatus} />}
        </>
      )}
      {focused && (
        <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">Esc schließen</p>
      )}
    </div>
  );
}

function SetupCard({ title, body }: { title: string; body: string }) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3">
      <p className="text-[12px] font-medium">{title}</p>
      <p className="mt-1 text-[11px] leading-snug text-[var(--color-muted)]">{body}</p>
    </div>
  );
}

function DevicePicker({
  status,
  serial,
  onPick,
}: {
  status: AdbStatus;
  serial: string | null;
  onPick: (s: string) => void;
}) {
  if (status.devices.length === 0) return null;
  if (status.devices.length === 1) {
    const d = status.devices[0];
    return (
      <p className="text-[11px] text-[var(--color-muted)]">
        {deviceLabel(d)} <span className="opacity-60">· {d.serial}</span>
      </p>
    );
  }
  return (
    <select
      value={serial ?? ""}
      onChange={(e) => onPick(e.target.value)}
      className="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-1 text-[11px] outline-none"
      aria-label="Gerät wählen"
    >
      {status.devices.map((d) => (
        <option key={d.serial} value={d.serial}>
          {deviceLabel(d)}
        </option>
      ))}
    </select>
  );
}

const VIEWS: Array<{ id: AdbView; label: string }> = [
  { id: "info", label: "Info" },
  { id: "control", label: "Steuern" },
  { id: "remote", label: "Remote" },
  { id: "apps", label: "Apps" },
  { id: "wifi", label: "WLAN" },
];

function ViewChips({ view, onPick }: { view: AdbView; onPick: (v: AdbView) => void }) {
  return (
    <div className="flex flex-wrap items-center gap-1">
      {VIEWS.map((v) => (
        <button
          key={v.id}
          type="button"
          onClick={() => onPick(v.id)}
          className={
            "rounded-full px-2.5 py-0.5 text-[11px] transition-colors " +
            (view === v.id
              ? "bg-[var(--color-accent)] font-medium text-[var(--color-accent-fg)]"
              : "border border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-fg)]")
          }
        >
          {v.label}
        </button>
      ))}
    </div>
  );
}

// ── Info (dashboard + capture) ──────────────────────────────────────────────

function MiniBar({ pct, color }: { pct: number; color?: string }) {
  const p = Math.max(0, Math.min(100, pct));
  return (
    <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--color-border)]">
      <div
        className="h-full rounded-full"
        style={{ width: `${p}%`, backgroundColor: color ?? "var(--color-accent)" }}
      />
    </div>
  );
}

function InfoView({
  dash,
  serial,
  recording,
  onFlash,
  onStatus,
}: {
  dash: AdbDashboard | null;
  serial: string;
  recording: boolean;
  onFlash: (m: string) => void;
  onStatus: () => void;
}) {
  const [busy, setBusy] = useState(false);
  if (!dash) return <p className="text-[12px] text-[var(--color-muted)]">Lese Gerät…</p>;
  const memPct = dash.mem_total_kb > 0 ? (dash.mem_used_kb / dash.mem_total_kb) * 100 : 0;
  const stPct =
    dash.storage_total_kb > 0 ? (dash.storage_used_kb / dash.storage_total_kb) * 100 : 0;
  const batColor =
    (dash.battery_level ?? 100) <= 20
      ? "#ef4444"
      : (dash.battery_level ?? 100) <= 50
        ? "#f59e0b"
        : "#22c55e";
  return (
    <>
      <div className="rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
        <div className="flex items-center justify-between text-[12px]">
          <span className="font-medium">
            {dash.manufacturer} {dash.model}
          </span>
          <span className="text-[var(--color-muted)]">Android {dash.android_version}</span>
        </div>
        <div className="mt-1 grid grid-cols-2 gap-x-3 gap-y-0.5 text-[11px] text-[var(--color-muted)]">
          <span>SDK {dash.sdk}</span>
          <span className="truncate text-right" title={dash.build_id}>
            {dash.build_id}
          </span>
          <span>{dash.resolution || "—"} · {dash.dpi || "—"} dpi</span>
          <span className="text-right">Uptime {uptimeHuman(dash.uptime_secs)}</span>
        </div>
      </div>

      {dash.battery_level != null && (
        <div className="rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
          <div className="mb-1 flex items-center justify-between text-[11px]">
            <span>
              Akku <span className="font-medium">{dash.battery_level}%</span>{" "}
              <span className="text-[var(--color-muted)]">· {dash.battery_status}</span>
            </span>
            <span className="text-[var(--color-muted)] tabular-nums">
              {dash.battery_temp_c != null ? `${dash.battery_temp_c.toFixed(1)} °C` : ""}
              {dash.battery_voltage_mv != null ? ` · ${(dash.battery_voltage_mv / 1000).toFixed(2)} V` : ""}
            </span>
          </div>
          <MiniBar pct={dash.battery_level} color={batColor} />
        </div>
      )}

      <div className="rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
        <div className="mb-1 flex items-center justify-between text-[11px]">
          <span>RAM</span>
          <span className="text-[var(--color-muted)] tabular-nums">
            {kbHuman(dash.mem_used_kb)} / {kbHuman(dash.mem_total_kb)}
          </span>
        </div>
        <MiniBar pct={memPct} />
        <div className="mb-1 mt-2 flex items-center justify-between text-[11px]">
          <span>Speicher</span>
          <span className="text-[var(--color-muted)] tabular-nums">
            {kbHuman(dash.storage_used_kb)} / {kbHuman(dash.storage_total_kb)}
          </span>
        </div>
        <MiniBar pct={stPct} />
        <div className="mt-2 text-[11px] text-[var(--color-muted)]">
          {dash.wifi_ssid ? `WLAN ${dash.wifi_ssid}` : "WLAN —"}
          {dash.ip ? ` · ${dash.ip}` : ""}
          {dash.rssi_dbm != null ? ` · ${dash.rssi_dbm} dBm` : ""}
        </div>
      </div>

      <div className="flex items-center gap-2">
        <button
          type="button"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            adbScreenshot(serial)
              .then(() => onFlash("Screenshot im Mac-Clipboard + Verlauf ✓"))
              .catch((e) => onFlash(String(e)))
              .finally(() => setBusy(false));
          }}
          className="flex items-center gap-1.5 rounded-md border border-[var(--color-border)] px-2.5 py-1 text-[11px] hover:text-[var(--color-accent)] disabled:opacity-40"
        >
          <Camera size={12} /> Screenshot
        </button>
        {recording ? (
          <button
            type="button"
            onClick={() => {
              adbRecordStop()
                .then((p) => {
                  onFlash(`Aufnahme gespeichert: ${p.split("/").pop()}`);
                  onStatus();
                })
                .catch((e) => onFlash(String(e)));
            }}
            className="flex items-center gap-1.5 rounded-md border border-red-500 px-2.5 py-1 text-[11px] text-red-500"
          >
            <Square size={12} /> Aufnahme stoppen
          </button>
        ) : (
          <button
            type="button"
            onClick={() => {
              adbRecordStart(serial)
                .then(() => {
                  onFlash("Aufnahme läuft (Geräte-Display)…");
                  onStatus();
                })
                .catch((e) => onFlash(String(e)));
            }}
            className="flex items-center gap-1.5 rounded-md border border-[var(--color-border)] px-2.5 py-1 text-[11px] hover:text-[var(--color-accent)]"
          >
            <Video size={12} /> Aufnehmen
          </button>
        )}
      </div>
    </>
  );
}

// ── Steuern (quick controls) ────────────────────────────────────────────────

function ControlView({
  serial,
  dash,
  onFlash,
}: {
  serial: string;
  dash: AdbDashboard | null;
  onFlash: (m: string) => void;
}) {
  const set = (what: string, value: number, label: string) => {
    adbSet(serial, what, value)
      .then(() => onFlash(`${label} ✓`))
      .catch((e) => onFlash(String(e)));
  };
  const [brightness, setBrightness] = useState<number>(dash?.brightness ?? 128);
  const [volMedia, setVolMedia] = useState<number>(dash?.volume_media ?? 7);
  const debRef = useRef<number | undefined>(undefined);
  const debounced = (fn: () => void) => {
    window.clearTimeout(debRef.current);
    debRef.current = window.setTimeout(fn, 250);
  };
  return (
    <>
      <div className="rounded-xl border border-[var(--color-border)] p-3">
        <p className="mb-2 text-[11px] font-medium">Funk & Modi</p>
        <div className="grid grid-cols-2 gap-1.5">
          <TogglePair icon={<Wifi size={12} />} label="WLAN" onOn={() => set("wifi", 1, "WLAN an")} onOff={() => set("wifi", 0, "WLAN aus")} />
          <TogglePair icon={<Bluetooth size={12} />} label="Bluetooth" onOn={() => set("bluetooth", 1, "BT an")} onOff={() => set("bluetooth", 0, "BT aus")} />
          <TogglePair icon={<Plane size={12} />} label="Flugmodus" onOn={() => set("airplane", 1, "Flugmodus an")} onOff={() => set("airplane", 0, "Flugmodus aus")} />
          <TogglePair icon={<BellOff size={12} />} label="Nicht stören" onOn={() => set("dnd", 1, "DND an")} onOff={() => set("dnd", 0, "DND aus")} />
        </div>
      </div>
      <div className="rounded-xl border border-[var(--color-border)] p-3">
        <div className="flex items-center gap-2 text-[11px]">
          <Sun size={12} className="shrink-0 text-[var(--color-muted)]" />
          <input
            type="range"
            min={0}
            max={255}
            value={brightness}
            onChange={(e) => {
              const v = Number(e.target.value);
              setBrightness(v);
              debounced(() => set("brightness", v, `Helligkeit ${v}`));
            }}
            className="w-full accent-[var(--color-accent)]"
            aria-label="Helligkeit"
          />
          <span className="w-8 text-right tabular-nums">{brightness}</span>
        </div>
        <div className="mt-2 flex items-center gap-2 text-[11px]">
          <Volume2 size={12} className="shrink-0 text-[var(--color-muted)]" />
          <input
            type="range"
            min={0}
            max={15}
            value={volMedia}
            onChange={(e) => {
              const v = Number(e.target.value);
              setVolMedia(v);
              debounced(() => set("volume_media", v, `Medien-Lautstärke ${v}`));
            }}
            className="w-full accent-[var(--color-accent)]"
            aria-label="Medien-Lautstärke"
          />
          <span className="w-8 text-right tabular-nums">{volMedia}</span>
        </div>
      </div>
      <div className="flex items-center gap-1.5">
        <ActionBtn icon={<Sun size={12} />} label="Wecken" onClick={() => set("screen_wake", 0, "Display an")} />
        <ActionBtn icon={<Moon size={12} />} label="Display aus" onClick={() => set("screen_sleep", 0, "Display aus")} />
        <ActionBtn icon={<Lock size={12} />} label="Sperren" onClick={() => set("screen_lock", 0, "Gesperrt")} />
      </div>
    </>
  );
}

function TogglePair({
  icon,
  label,
  onOn,
  onOff,
}: {
  icon: React.ReactNode;
  label: string;
  onOn: () => void;
  onOff: () => void;
}) {
  return (
    <div className="flex items-center justify-between gap-1 rounded-md border border-[var(--color-border)] px-2 py-1 text-[11px]">
      <span className="flex items-center gap-1 text-[var(--color-muted)]">
        {icon} {label}
      </span>
      <span className="flex gap-1">
        <button type="button" onClick={onOn} className="rounded px-1.5 py-0.5 hover:bg-[var(--color-border)]">an</button>
        <button type="button" onClick={onOff} className="rounded px-1.5 py-0.5 text-[var(--color-muted)] hover:bg-[var(--color-border)]">aus</button>
      </span>
    </div>
  );
}

function ActionBtn({
  icon,
  label,
  onClick,
  danger,
}: {
  icon?: React.ReactNode;
  label: string;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={
        "flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-[11px] " +
        (danger
          ? "border-red-500 text-red-500 hover:bg-red-500/10"
          : "border-[var(--color-border)] hover:text-[var(--color-accent)]")
      }
    >
      {icon} {label}
    </button>
  );
}

// ── Remote (input) ──────────────────────────────────────────────────────────

function RemoteView({ serial, onFlash }: { serial: string; onFlash: (m: string) => void }) {
  const key = (code: string) => {
    adbKey(serial, code).catch((e) => onFlash(String(e)));
  };
  const [text, setText] = useState("");
  const [tapXY, setTapXY] = useState({ x: 540, y: 1200 });
  const [sw, setSw] = useState({ x1: 540, y1: 1600, x2: 540, y2: 600, dur: 300 });
  return (
    <>
      <div className="rounded-xl border border-[var(--color-border)] p-3">
        <div className="grid grid-cols-4 gap-1.5">
          {NAV_KEYS.map((k) => (
            <button
              key={k.code}
              type="button"
              onClick={() => key(k.code)}
              className="rounded-md border border-[var(--color-border)] px-1 py-1.5 text-[11px] hover:text-[var(--color-accent)]"
            >
              {k.label}
            </button>
          ))}
        </div>
        <div className="mx-auto mt-3 grid w-36 grid-cols-3 gap-1.5">
          <span />
          <DpadBtn k={DPAD_KEYS.up} onKey={key} />
          <span />
          <DpadBtn k={DPAD_KEYS.left} onKey={key} />
          <DpadBtn k={DPAD_KEYS.center} onKey={key} accent />
          <DpadBtn k={DPAD_KEYS.right} onKey={key} />
          <span />
          <DpadBtn k={DPAD_KEYS.down} onKey={key} />
          <DpadBtn k={DPAD_KEYS.enter} onKey={key} />
        </div>
      </div>

      <div className="rounded-xl border border-[var(--color-border)] p-3">
        <p className="mb-1.5 text-[11px] font-medium">Text senden</p>
        <div className="flex gap-1.5">
          <input
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => e.stopPropagation()}
            placeholder="Text (nur ASCII)"
            className="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px]"
          />
          <button
            type="button"
            disabled={!textSendable(text)}
            onClick={() => {
              adbText(serial, text)
                .then(() => {
                  onFlash("Text gesendet ✓");
                  setText("");
                })
                .catch((e) => onFlash(String(e)));
            }}
            className="rounded-md bg-[var(--color-accent)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-accent-fg)] disabled:opacity-40"
          >
            Senden
          </button>
        </div>
        {text.length > 0 && !textSendable(text) && (
          <p className="mt-1 text-[10px] text-amber-500">
            `input text` kann nur ASCII zustellen — Umlaute/Emoji gehen nicht.
          </p>
        )}
      </div>

      <div className="rounded-xl border border-[var(--color-border)] p-3">
        <p className="mb-1.5 text-[11px] font-medium">Tap / Swipe</p>
        <div className="flex items-center gap-1.5 text-[11px]">
          <NumIn v={tapXY.x} set={(x) => setTapXY((t) => ({ ...t, x }))} label="X" />
          <NumIn v={tapXY.y} set={(y) => setTapXY((t) => ({ ...t, y }))} label="Y" />
          <button
            type="button"
            disabled={!validTap(tapXY.x, tapXY.y)}
            onClick={() => adbTap(serial, tapXY.x, tapXY.y).catch((e) => onFlash(String(e)))}
            className="ml-auto rounded-md border border-[var(--color-border)] px-2.5 py-1 hover:text-[var(--color-accent)] disabled:opacity-40"
          >
            Tap
          </button>
        </div>
        <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-[11px]">
          <NumIn v={sw.x1} set={(x1) => setSw((s) => ({ ...s, x1 }))} label="X₁" />
          <NumIn v={sw.y1} set={(y1) => setSw((s) => ({ ...s, y1 }))} label="Y₁" />
          <NumIn v={sw.x2} set={(x2) => setSw((s) => ({ ...s, x2 }))} label="X₂" />
          <NumIn v={sw.y2} set={(y2) => setSw((s) => ({ ...s, y2 }))} label="Y₂" />
          <NumIn v={sw.dur} set={(dur) => setSw((s) => ({ ...s, dur }))} label="ms" wide />
          <button
            type="button"
            disabled={!validSwipe(sw.x1, sw.y1, sw.x2, sw.y2, sw.dur)}
            onClick={() =>
              adbSwipe(serial, sw.x1, sw.y1, sw.x2, sw.y2, sw.dur).catch((e) => onFlash(String(e)))
            }
            className="ml-auto rounded-md border border-[var(--color-border)] px-2.5 py-1 hover:text-[var(--color-accent)] disabled:opacity-40"
          >
            Swipe
          </button>
        </div>
      </div>
    </>
  );
}

function DpadBtn({ k, onKey, accent }: { k: { label: string; code: string }; onKey: (c: string) => void; accent?: boolean }) {
  return (
    <button
      type="button"
      onClick={() => onKey(k.code)}
      className={
        "rounded-md border px-1 py-1.5 text-[11px] " +
        (accent
          ? "border-[var(--color-accent)] font-medium text-[var(--color-accent)]"
          : "border-[var(--color-border)] hover:text-[var(--color-accent)]")
      }
    >
      {k.label}
    </button>
  );
}

function NumIn({ v, set, label, wide }: { v: number; set: (n: number) => void; label: string; wide?: boolean }) {
  return (
    <label className="flex items-center gap-1 text-[var(--color-muted)]">
      {label}
      <input
        type="number"
        value={v}
        onChange={(e) => set(Number(e.target.value))}
        onKeyDown={(e) => e.stopPropagation()}
        className={
          (wide ? "w-16" : "w-14") +
          " rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1 py-0.5 text-[11px] text-[var(--color-fg)] tabular-nums"
        }
      />
    </label>
  );
}

// ── Apps (light manager) ────────────────────────────────────────────────────

function AppsView({ serial, onFlash }: { serial: string; onFlash: (m: string) => void }) {
  const [pkgs, setPkgs] = useState<string[] | null>(null);
  const [query, setQuery] = useState("");
  const [system, setSystem] = useState(false);
  const [sel, setSel] = useState<string | null>(null);

  useEffect(() => {
    setPkgs(null);
    adbPackages(serial, system)
      .then(setPkgs)
      .catch((e) => onFlash(String(e)));
    // onFlash is a stable-enough toast fn; the list depends on device+filter.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serial, system]);

  const act = async (action: string, confirmMsg?: string) => {
    if (!sel) return;
    if (confirmMsg && !(await confirmDialog(confirmMsg, "Android"))) return;
    adbAppAction(serial, action, sel)
      .then((out) => onFlash(out ? out.split("\n")[0] : `${action} ✓`))
      .catch((e) => onFlash(String(e)));
  };

  const shown = pkgs ? filterPackages(pkgs, query).slice(0, 120) : [];
  return (
    <>
      <div className="flex items-center gap-1.5">
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.stopPropagation()}
          placeholder="Paket suchen…"
          className="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px]"
        />
        <label className="flex shrink-0 items-center gap-1 text-[10px] text-[var(--color-muted)]">
          <input
            type="checkbox"
            checked={system}
            onChange={(e) => setSystem(e.target.checked)}
            className="accent-[var(--color-accent)]"
          />
          System
        </label>
      </div>
      {pkgs === null ? (
        <p className="text-[12px] text-[var(--color-muted)]">Lade Pakete…</p>
      ) : (
        <div className="max-h-56 overflow-y-auto rounded-xl border border-[var(--color-border)] [contain:content]">
          {shown.map((p) => (
            <button
              key={p}
              type="button"
              onClick={() => setSel(p === sel ? null : p)}
              className={
                "block w-full truncate px-2 py-1 text-left font-[var(--font-mono)] text-[11px] " +
                (p === sel
                  ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                  : "hover:bg-[var(--color-border)]")
              }
            >
              {p}
            </button>
          ))}
          {shown.length === 0 && (
            <p className="p-2 text-[11px] text-[var(--color-muted)]">Keine Treffer.</p>
          )}
        </div>
      )}
      {sel && (
        <div className="flex flex-wrap items-center gap-1.5">
          <ActionBtn icon={<Play size={11} />} label="Start" onClick={() => void act("launch")} />
          <ActionBtn icon={<XCircle size={11} />} label="Stoppen" onClick={() => void act("stop")} />
          <ActionBtn
            label="Daten löschen"
            danger
            onClick={() => void act("clear", `Alle Daten von ${sel} löschen? (App wird zurückgesetzt)`)}
          />
          <ActionBtn
            icon={<Trash2 size={11} />}
            label="Deinstallieren"
            danger
            onClick={() => void act("uninstall", `${sel} wirklich deinstallieren?`)}
          />
        </div>
      )}
      {pkgs !== null && filterPackages(pkgs, query).length > 120 && (
        <p className="text-[10px] text-[var(--color-muted)]">
          {filterPackages(pkgs, query).length} Treffer — Suche verfeinern (120 angezeigt).
        </p>
      )}
    </>
  );
}

// ── WLAN-ADB ────────────────────────────────────────────────────────────────

function WifiView({
  status,
  onFlash,
  onChanged,
}: {
  status: AdbStatus;
  onFlash: (m: string) => void;
  onChanged: () => void;
}) {
  const [ip, setIp] = useState("");
  const usb = status.devices.filter((d) => !d.wifi && d.state === "device");
  const wifi = status.devices.filter((d) => d.wifi);
  return (
    <>
      {usb.map((d) => (
        <div key={d.serial} className="rounded-xl border border-[var(--color-border)] p-3">
          <p className="text-[11px] font-medium">{deviceLabel(d)} (USB)</p>
          <p className="mt-0.5 text-[10px] leading-snug text-[var(--color-muted)]">
            Schaltet adbd auf TCP/IP (Port 5555), holt die WLAN-IP und verbindet — danach kann das
            Kabel ab.
          </p>
          <button
            type="button"
            onClick={() => {
              adbWifiTcpip(d.serial, 5555)
                .then(async (deviceIp) => {
                  if (!deviceIp) {
                    onFlash("TCP/IP an — IP nicht erkennbar, unten manuell verbinden.");
                    return;
                  }
                  // adbd restarts; give it a beat before connecting.
                  await new Promise((r) => window.setTimeout(r, 1500));
                  return adbWifiConnect(`${deviceIp}:5555`).then((msg) => {
                    onFlash(msg);
                    onChanged();
                  });
                })
                .catch((e) => onFlash(String(e)));
            }}
            className="mt-1.5 rounded-md bg-[var(--color-accent)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-accent-fg)]"
          >
            WLAN-ADB aktivieren
          </button>
        </div>
      ))}
      <div className="rounded-xl border border-[var(--color-border)] p-3">
        <p className="mb-1.5 text-[11px] font-medium">Per IP verbinden</p>
        <div className="flex gap-1.5">
          <input
            value={ip}
            onChange={(e) => setIp(e.target.value)}
            onKeyDown={(e) => e.stopPropagation()}
            placeholder="192.168.178.42:5555"
            className="w-full rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 font-[var(--font-mono)] text-[12px]"
          />
          <button
            type="button"
            disabled={ip.trim().length === 0}
            onClick={() => {
              adbWifiConnect(ip.trim())
                .then((msg) => {
                  onFlash(msg);
                  onChanged();
                })
                .catch((e) => onFlash(String(e)));
            }}
            className="rounded-md border border-[var(--color-border)] px-2.5 py-1 text-[11px] hover:text-[var(--color-accent)] disabled:opacity-40"
          >
            Verbinden
          </button>
        </div>
      </div>
      {wifi.map((d) => (
        <div
          key={d.serial}
          className="flex items-center justify-between rounded-xl border border-[var(--color-border)] p-3 text-[11px]"
        >
          <span>
            {deviceLabel(d)} <span className="text-[var(--color-muted)]">· {d.serial}</span>
          </span>
          <button
            type="button"
            onClick={() => {
              adbWifiDisconnect(d.serial)
                .then(() => {
                  onFlash("Getrennt.");
                  onChanged();
                })
                .catch((e) => onFlash(String(e)));
            }}
            className="rounded-md border border-[var(--color-border)] px-2 py-0.5 text-[var(--color-muted)] hover:text-red-500"
          >
            Trennen
          </button>
        </div>
      ))}
      {usb.length === 0 && wifi.length === 0 && (
        <SetupCard
          title="Kein Gerät für WLAN-ADB."
          body="Einmalig per USB anschließen und „WLAN-ADB aktivieren“ — oder direkt eine bekannte IP:Port verbinden (Gerät muss schon im TCP/IP-Modus sein)."
        />
      )}
    </>
  );
}
