import { useCallback, useEffect, useState } from "react";
import { AppWindow, Mic, Monitor, StickyNote, Volume2, X } from "lucide-react";
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
  const [title, setTitle] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);

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

  const patch = (p: Partial<RecordConfig>) => {
    if (!cfg) return;
    const next = { ...cfg, ...p };
    setCfg(next);
    void setRecordConfig(next).catch(() => undefined);
  };

  const pick = (kind: "display" | "window") => {
    setError(null);
    recordPickSource(kind).catch((e) => setError(errorText(t, String(e))));
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
    }).catch((e) => {
      setStarting(false);
      setError(errorText(t, String(e)));
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status, cfg, starting, title]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
      else if (e.key === "Enter" && !e.isComposing) start();
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
    <Shell title={t("rec2.hud.title")} onClose={close} closeLabel={t("rec2.hud.close")}>
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
          {toggle(cfg.cursorMode === "editable-overlay", t("rec2.hud.cursor"), null, (v) =>
            patch({ cursorMode: v ? "editable-overlay" : "system" }),
          )}
          {toggle(cfg.countdown, t("rec2.hud.countdown"), null, (v) => patch({ countdown: v }))}
        </div>
      )}
      <input
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        placeholder={t("rec2.hud.title_field")}
        className="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] px-3 py-1.5 text-[13px] outline-none"
      />
      {error && <p className="text-[12px] text-red-500">{error}</p>}
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

function Shell({
  title,
  onClose,
  closeLabel,
  children,
}: {
  title: string;
  onClose: () => void;
  closeLabel: string;
  children: React.ReactNode;
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
      {children}
    </div>
  );
}
