import { useCallback, useEffect, useState } from "react";
import { AppWindow, Camera, Mic, Monitor, StickyNote, Volume2, X } from "lucide-react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listen } from "@tauri-apps/api/event";
import {
  getRecordConfig,
  recordOpenNotes,
  recordPickSource,
  recordStart,
  recordStatus,
  setRecordConfig,
  type RecordConfig,
  type RecordStatus,
} from "../lib/ipc";
import { canStart } from "../lib/record";
import { useT } from "../lib/i18n";
import { CameraDeniedHint, useRecordCamera } from "./useRecordCamera";

/** Error sentinel `record.x_y` → locale key `rec2.err.x_y`. */
function errorText(t: ReturnType<typeof useT>, raw: string): string {
  const m = /^record\.([a-z_]+)(?::(.*))?$/.exec(raw);
  if (!m) return raw;
  const key = `rec2.err.${m[1]}`;
  const out = t(key as Parameters<typeof t>[0], { detail: m[2] ?? "" });
  return out === key ? raw : out;
}

/** Source pick + options (window `record-hud`). */
export function RecordHud() {
  const t = useT();
  const [status, setStatus] = useState<RecordStatus | null>(null);
  const [cfg, setCfg] = useState<RecordConfig | null>(null);
  /** Bumped by every user change; the effect below stores the config it produced. */
  const [edits, setEdits] = useState(0);
  const [title, setTitle] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const cam = useRecordCamera();

  useEffect(() => {
    void recordStatus().then(setStatus).catch(() => undefined);
    void getRecordConfig().then(setCfg).catch(() => undefined);
    const offs: Array<() => void> = [];
    let dead = false;
    const add = (p: Promise<() => void>) => void p.then((u) => (dead ? u() : offs.push(u)));
    add(listen<RecordStatus>("record-status", (e) => setStatus(e.payload)));
    add(listen("record-source-picked", () => void recordStatus().then(setStatus).catch(() => undefined)));
    add(
      listen<{ error: string | null }>("record-pick-cancelled", (e) => {
        if (e.payload?.error) setError(t("rec2.hud.pick_failed", { detail: e.payload.error }));
        void recordStatus().then(setStatus).catch(() => undefined);
      }),
    );
    return () => {
      dead = true;
      offs.forEach((u) => u());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const close = useCallback(() => void getCurrentWebviewWindow().close(), []);

  // Functional update: a change applied after an await (the camera dialog) builds on the
  // latest config instead of the one captured before it.
  const patch = (p: Partial<RecordConfig>) => {
    setCfg((prev) => (prev ? { ...prev, ...p } : prev));
    setEdits((n) => n + 1);
  };
  useEffect(() => {
    if (edits > 0 && cfg) void setRecordConfig(cfg).catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [edits]);

  const pick = (kind: "display" | "window") => {
    setError(null);
    recordPickSource(kind).catch((e) => setError(errorText(t, String(e))));
  };

  const cameraOn = cam.isOn(cfg?.camera);

  const setCamera = async (on: boolean) => {
    if (!on) return patch({ camera: false });
    if (await cam.turnOn()) patch({ camera: true });
  };

  const ready = !!status && !!cfg && canStart(status) && !starting;

  const start = useCallback(() => {
    if (!status || !cfg || !canStart(status) || starting) return;
    setStarting(true);
    setError(null);
    recordStart({
      systemAudio: cfg.systemAudio,
      mic: cfg.mic,
      cursorMode: cfg.cursorMode,
      countdown: cfg.countdown,
      title: title.trim(),
      camera: cameraOn,
      cameraDevice: cfg.cameraDevice ?? "",
      cameraQuality: cfg.cameraQuality ?? "1080p",
    }).catch((e) => {
      setStarting(false);
      setError(errorText(t, String(e)));
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status, cfg, starting, title, cameraOn]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
      else if (e.key === "Enter" && !e.isComposing) {
        const el = e.target as HTMLElement | null;
        const tag = el?.tagName;
        if (tag === "BUTTON" || tag === "SELECT" || tag === "TEXTAREA") return;
        if (tag === "INPUT" && (el as HTMLInputElement).type === "checkbox") return;
        start();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close, start]);

  if (status && !status.supported) {
    return (
      <Shell title={t("rec2.hud.title")} onClose={close} closeLabel={t("rec2.hud.close")}>
        <p className="text-[13px] text-[var(--color-muted)]">{status.reason}</p>
      </Shell>
    );
  }

  const toggle = (on: boolean, label: string, icon: React.ReactNode, set: (v: boolean) => void) => (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      onClick={() => set(!on)}
      className={
        "flex items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-[12px] transition-colors duration-(--duration-fast) ease-sharp " +
        (on
          ? "border-[var(--color-accent)] bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
          : "border-[var(--color-border)] text-[var(--color-muted)] hover:bg-[var(--color-bg)]")
      }
    >
      {icon}
      {label}
    </button>
  );

  return (
    <Shell
      title={t("rec2.hud.title")}
      onClose={close}
      closeLabel={t("rec2.hud.close")}
      footer={
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void recordOpenNotes().catch(() => undefined)}
            className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-2.5 py-1.5 text-[12px] hover:bg-[var(--color-bg)]"
          >
            <StickyNote size={13} />
            {t("rec2.hud.notes")}
          </button>
          <button
            type="button"
            onClick={start}
            disabled={!ready}
            className="ml-auto flex items-center gap-1.5 rounded-lg bg-red-600 px-4 py-1.5 text-[13px] font-medium text-white hover:bg-red-500 disabled:opacity-50"
          >
            <span className="h-2.5 w-2.5 rounded-full bg-white" />
            {starting ? t("rec2.hud.starting") : t("rec2.hud.record")}
          </button>
        </div>
      }
    >
      <div className="grid grid-cols-2 gap-2">
        <BigButton icon={<Monitor size={22} />} label={t("rec2.hud.display")} onClick={() => pick("display")} />
        <BigButton icon={<AppWindow size={22} />} label={t("rec2.hud.window")} onClick={() => pick("window")} />
      </div>
      {status?.source && (
        <div className="flex items-center justify-between rounded-lg bg-[var(--color-bg)] px-3 py-1.5 text-[12px]">
          <span className="truncate">
            <span className="text-[var(--color-muted)]">{t("rec2.hud.source")}: </span>
            {status.source}
          </span>
        </div>
      )}
      {cfg && (
        <div className="flex flex-wrap gap-1.5">
          {toggle(cfg.systemAudio, t("rec2.hud.system_audio"), <Volume2 size={13} />, (v) => patch({ systemAudio: v }))}
          {toggle(cfg.mic, t("rec2.hud.mic"), <Mic size={13} />, (v) => patch({ mic: v }))}
          {toggle(cameraOn, t("rec5.hud.camera"), <Camera size={13} />, (v) => void setCamera(v))}
          {toggle(cfg.cursorMode === "editable-overlay", t("rec2.hud.cursor"), null, (v) =>
            patch({ cursorMode: v ? "editable-overlay" : "system" }),
          )}
          {toggle(cfg.countdown, t("rec2.hud.countdown"), null, (v) => patch({ countdown: v }))}
        </div>
      )}
      {cfg && cam.showHint(cfg.camera) && <CameraDeniedHint />}
      {cfg && cameraOn && cam.devices.length > 1 && (
        <select
          data-testid="rec-camera-device"
          aria-label={t("rec5.hud.camera_device")}
          value={cam.devices.some((c) => c.id === cfg.cameraDevice) ? cfg.cameraDevice : ""}
          onChange={(e) => patch({ cameraDevice: e.target.value })}
          className="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[12px] outline-none"
        >
          <option value="">{t("rec5.set.camera_device_default")}</option>
          {cam.devices.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
      )}
      <input
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        placeholder={t("rec2.hud.title_field")}
        className="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] px-3 py-1.5 text-[13px] outline-none"
      />
      {error && <p className="text-[12px] text-red-500">{error}</p>}
    </Shell>
  );
}

function BigButton({ icon, label, onClick }: { icon: React.ReactNode; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex flex-col items-center gap-1.5 rounded-xl border border-[var(--color-border)] py-4 text-[13px] font-medium transition-colors duration-(--duration-fast) ease-sharp hover:bg-[var(--color-bg)]"
    >
      {icon}
      {label}
    </button>
  );
}

/**
 * Header and `footer` (the Record row) stay put; the middle scrolls when it is taller than the
 * window (the window is sized for the measured worst case, see `HUD_SIZE` in session.rs, but
 * a long error message must never push the Record button out of reach).
 */
function Shell({
  title,
  onClose,
  closeLabel,
  children,
  footer,
}: {
  title: string;
  onClose: () => void;
  closeLabel: string;
  children: React.ReactNode;
  footer?: React.ReactNode;
}) {
  return (
    <div className="flex h-screen w-screen flex-col gap-3 rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 text-[var(--color-fg)] shadow-2xl">
      <div data-tauri-drag-region className="flex items-center justify-between">
        <span className="text-[12px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">{title}</span>
        <button
          onClick={onClose}
          aria-label={closeLabel}
          className="rounded-md p-1 text-[var(--color-muted)] hover:bg-[var(--color-bg)] hover:text-[var(--color-fg)]"
        >
          <X size={14} />
        </button>
      </div>
      <div data-testid="rec-hud-body" className="-mx-1 flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-1">
        {children}
      </div>
      {footer && (
        <div data-testid="rec-hud-footer" className="shrink-0">
          {footer}
        </div>
      )}
    </div>
  );
}
