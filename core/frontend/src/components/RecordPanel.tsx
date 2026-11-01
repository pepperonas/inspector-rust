import { useCallback, useEffect, useMemo, useState } from "react";
import { Square, Video } from "lucide-react";
import {
  recordDeleteProject,
  recordListProjects,
  recordOpenHud,
  recordRevealProject,
  recordStatus,
  recordStop,
  type RecordProjectItem,
  type RecordStatus,
} from "../lib/ipc";
import { locale, t, useT, type MessageKey } from "../lib/i18n";

/**
 * `record` — recent recording projects (macOS, stage 1).
 *
 * Show-while-typed: only reads the project list and the session status — it
 * never opens a microphone or starts a capture. Enter (handled here once the
 * panel has the keyboard, else by the command dispatch) opens the HUD.
 * Delete is a TWO-STAGE inline confirm, never `window.confirm`.
 */
const POLL_MS = 1500;

export function mmss(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const two = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${two(m)}:${two(sec)}` : `${m}:${two(sec)}`;
}

/** Error sentinel `record.x_y` → localized text (others pass through). */
function errText(e: unknown): string {
  const raw = String(e);
  const m = /record\.([a-z_]+)/.exec(raw);
  if (m) {
    const key = `rec2.err.${m[1]}` as MessageKey;
    const txt = t(key, { detail: "" });
    if (txt !== key) return txt;
  }
  return raw;
}

export function RecordPanel({
  focused,
  onRequestFocus,
  onExit,
}: {
  focused: boolean;
  /** ↓ in the search bar asks for the keyboard (to select a project). */
  onRequestFocus: () => void;
  onExit: () => void;
}) {
  useT();
  const [status, setStatus] = useState<RecordStatus | null>(null);
  const [items, setItems] = useState<RecordProjectItem[] | null>(null);
  const [sel, setSel] = useState(0);
  const [armed, setArmed] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    void recordStatus().then(setStatus).catch(() => {});
    void recordListProjects().then(setItems).catch((e) => setError(errText(e)));
  }, []);

  useEffect(() => {
    refresh();
    const id = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(id);
  }, [refresh]);

  const list = useMemo(() => items ?? [], [items]);
  const supported = status ? status.supported : true;
  const running = !!status && status.phase !== "idle" && status.phase !== "picking";

  const newRecording = useCallback(() => {
    recordOpenHud().catch((e) => setError(errText(e)));
  }, []);

  const remove = useCallback(
    (dir: string) => {
      setArmed(null);
      recordDeleteProject(dir)
        .then(refresh)
        .catch((e) => setError(errText(e)));
    },
    [refresh],
  );

  // ↓ from the search bar hands the panel the keyboard.
  useEffect(() => {
    if (focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowDown" && list.length > 0 && supported) onRequestFocus();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focused, list.length, supported, onRequestFocus]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      const item = list[sel];
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setArmed(null);
        setSel((s) => Math.max(0, Math.min(list.length - 1, s + (e.key === "ArrowDown" ? 1 : -1))));
      } else if (e.key === "Enter" && e.metaKey && item) {
        e.preventDefault();
        recordRevealProject(item.dir).catch((err) => setError(errText(err)));
      } else if (e.key === "Enter") {
        e.preventDefault();
        if (supported && !running) newRecording();
      } else if ((e.key === "Backspace" || e.key === "Delete") && item && !item.active) {
        e.preventDefault();
        if (armed === item.dir) remove(item.dir);
        else setArmed(item.dir);
      } else if (e.key === "Escape") {
        e.preventDefault();
        if (armed) setArmed(null);
        else onExit();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focused, list, sel, armed, supported, running, newRecording, remove, onExit]);

  const chip =
    "md3-press rounded border border-[var(--color-border)] px-2 py-0.5 text-[11px] hover:border-[var(--color-accent)] disabled:opacity-40";

  const statusChip = (p: RecordProjectItem) => {
    if (p.active) return <span className="text-rose-400">{t("rec2.panel.recording_now")}</span>;
    if (p.status === "incomplete")
      return <span className="text-amber-400">{t("rec2.panel.incomplete")}</span>;
    if (p.status === "failed") return <span className="text-rose-400">{t("rec2.panel.failed")}</span>;
    return null;
  };

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[12px]">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 text-[13px] font-semibold text-[var(--color-fg)]">
          <Video size={14} className="text-[var(--color-accent)]" />
          {t("rec2.hud.title")}
        </div>
        {supported && !running && (
          <button type="button" className={chip} onClick={newRecording}>
            {t("rec2.panel.new")}
          </button>
        )}
      </div>

      {error && (
        <div className="rounded-md border border-rose-500/40 bg-rose-500/10 px-2 py-1.5 text-[11px]">
          {error}
        </div>
      )}

      {!supported ? (
        <div className="rounded-md border border-[var(--color-border)] px-3 py-2 text-[var(--color-muted)]">
          {status?.reason}
        </div>
      ) : (
        <>
          {running && status && (
            <div className="flex items-center justify-between rounded-md border border-rose-500/40 bg-rose-500/10 px-3 py-2">
              <span className="font-medium text-[var(--color-fg)]">
                {t(`rec2.phase.${status.phase}` as MessageKey)} · {mmss(status.elapsedMs)}
              </span>
              <button
                type="button"
                className={chip}
                onClick={() => recordStop().then(refresh).catch((e) => setError(errText(e)))}
              >
                <Square size={10} fill="currentColor" className="mr-1 inline" />
                {t("rec2.bar.stop")}
              </button>
            </div>
          )}

          {items && list.length === 0 && (
            <div className="text-[var(--color-muted)]">{t("rec2.panel.empty")}</div>
          )}

          <div className="rounded-md border border-[var(--color-border)] empty:hidden">
            {list.slice(0, 20).map((p, i) => {
              const selected = focused && i === sel;
              const isArmed = armed === p.dir;
              return (
                <div
                  key={p.dir}
                  className={`flex items-center gap-2.5 px-2.5 py-2 ${
                    i > 0 ? "border-t border-[var(--color-border)]" : ""
                  } ${selected ? "bg-[var(--color-accent)]/10" : ""}`}
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-baseline gap-2">
                      <span className="truncate font-medium text-[var(--color-fg)]">{p.title}</span>
                      <span className="shrink-0 text-[10px]">{statusChip(p)}</span>
                    </div>
                    <div className="text-[10px] text-[var(--color-muted)]">
                      {new Date(p.createdAt).toLocaleString(locale())} · {mmss(p.durationMs)}
                    </div>
                    {p.failure && !p.active && (
                      <div className="mt-0.5 text-[10px] text-amber-400">{p.failure}</div>
                    )}
                  </div>
                  {isArmed ? (
                    <span className="flex items-center gap-1.5">
                      <span className="text-[11px] text-rose-400">{t("rec2.panel.delete_q")}</span>
                      <button type="button" className={chip} onClick={() => remove(p.dir)}>
                        {t("rec2.panel.delete_yes")}
                      </button>
                      <button type="button" className={chip} onClick={() => setArmed(null)}>
                        {t("common.cancel")}
                      </button>
                    </span>
                  ) : (
                    <span className="flex items-center gap-1.5">
                      <button
                        type="button"
                        className={chip}
                        onClick={() => recordRevealProject(p.dir).catch((e) => setError(errText(e)))}
                      >
                        {t("rec2.panel.reveal")}
                      </button>
                      {!p.active && (
                        <button type="button" className={chip} onClick={() => setArmed(p.dir)}>
                          {t("rec2.panel.delete")}
                        </button>
                      )}
                    </span>
                  )}
                </div>
              );
            })}
          </div>
        </>
      )}

      <div className="text-[10px] text-[var(--color-muted)]">{t("rec2.panel.keys")}</div>
    </div>
  );
}
