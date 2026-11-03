import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { FolderOpen, Square, Video } from "lucide-react";
import {
  editorOpen,
  recordDeleteProject,
  recordListProjects,
  recordOpenDialog,
  recordOpenHud,
  recordRenameProject,
  recordRevealProject,
  recordStatus,
  recordStop,
  type RecordProjectItem,
  type RecordStatus,
} from "../lib/ipc";
import { locale, t, useT, type MessageKey } from "../lib/i18n";
import { filterProjects, lastChange } from "../lib/record-projects";
import { ProjectThumb } from "./ProjectThumb";

/**
 * `record` — recent recording projects (macOS, stage 1).
 *
 * Show-while-typed: only reads the project list and the session status — it
 * never opens a microphone or starts a capture. Enter (handled here once the
 * panel has the keyboard, else by the command dispatch) opens the HUD; once
 * a project is selected, Enter opens it in the editor (stage 2).
 * Delete is a TWO-STAGE inline confirm, never `window.confirm`.
 *
 * Stage 4: every project is listed (scrolling, no cap), most recently edited first; the
 * command's argument (`record <text>`) searches name and date; F2 or the Rename chip
 * renames inline (Enter saves, Esc cancels); "Open project…" opens any project folder.
 */
const POLL_MS = 1500;

/** Last dispatch signal handled — module-level so a remount never replays it. */
let lastOpenSignal = 0;

export function mmss(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const two = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${two(m)}:${two(sec)}` : `${m}:${two(sec)}`;
}

/** Error sentinel `record.x_y` → localized text (others pass through). */
export function errText(e: unknown): string {
  const raw = String(e);
  // Editor sentinels are catalogue keys themselves (`record.editor.no_video`).
  if (/^record\.editor\.[a-z_]+$/.test(raw)) {
    const txt = t(raw as MessageKey);
    if (txt !== raw) return txt;
  }
  if (/^record\.title_[a-z_]+$/.test(raw)) {
    const txt = t(`rec4.projects.${raw.slice("record.".length)}` as MessageKey);
    if (!txt.startsWith("rec4.")) return txt;
  }
  const m = /record\.([a-z_]+)/.exec(raw);
  if (m) {
    const key = `rec2.err.${m[1]}` as MessageKey;
    const txt = t(key, { detail: "" });
    if (txt !== key) return txt;
  }
  return raw;
}

/** A finished (or incomplete-with-video) take; failed or still running ones can't be edited. */
const editable = (p: RecordProjectItem | undefined) => !!p && !p.active && p.status !== "failed";

export function RecordPanel({
  focused,
  query = "",
  openSignal = 0,
  onRequestFocus,
  onExit,
}: {
  focused: boolean;
  /** The command's argument: searches title and date (accent- and case-insensitive). */
  query?: string;
  /** Bumped by the `record` command dispatch: open the HUD once per bump. */
  openSignal?: number;
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
  const [renaming, setRenaming] = useState<{ dir: string; value: string } | null>(null);
  const rows = useRef(new Map<string, HTMLDivElement>());

  const refresh = useCallback(() => {
    void recordStatus().then(setStatus).catch(() => {});
    void recordListProjects().then(setItems).catch((e) => setError(errText(e)));
  }, []);

  useEffect(() => {
    refresh();
    const id = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(id);
  }, [refresh]);

  const list = useMemo(() => filterProjects(items ?? [], query, locale()), [items, query]);
  // The list can shrink under the selection (a delete, another poll): clamp
  // at render time so the highlight and the keys always name a real row.
  const selIdx = Math.min(sel, Math.max(0, list.length - 1));
  const supported = status ? status.supported : true;
  const running = !!status && status.phase !== "idle" && status.phase !== "picking";

  const newRecording = useCallback(() => {
    recordOpenHud().catch((e) => setError(errText(e)));
  }, []);

  // `record` + Enter from the search bar: open the HUD, or show in this
  // panel why not (a take already runs → "already running").
  useEffect(() => {
    if (openSignal <= lastOpenSignal) return;
    lastOpenSignal = openSignal;
    newRecording();
  }, [openSignal, newRecording]);

  const openEditor = useCallback((dir: string) => {
    setError(null);
    editorOpen(dir).catch((e) => setError(errText(e)));
  }, []);

  const openOther = useCallback(() => {
    setError(null);
    recordOpenDialog()
      .then((dir) => {
        if (dir) openEditor(dir);
      })
      .catch((e) => setError(errText(e)));
  }, [openEditor]);

  const startRename = useCallback(
    (p: RecordProjectItem) => {
      if (p.active) return;
      setArmed(null);
      setError(null);
      if (!focused) onRequestFocus();
      setRenaming({ dir: p.dir, value: p.title });
    },
    [focused, onRequestFocus],
  );

  const commitRename = useCallback(() => {
    if (!renaming) return;
    const { dir, value } = renaming;
    recordRenameProject(dir, value)
      .then(() => {
        setRenaming(null);
        refresh();
      })
      .catch((e) => setError(errText(e)));
  }, [renaming, refresh]);

  // The keyboard selection stays in view while the list scrolls.
  const selDir = list[selIdx]?.dir;
  useEffect(() => {
    if (!focused || !selDir) return;
    rows.current.get(selDir)?.scrollIntoView?.({ block: "nearest" });
  }, [focused, selDir]);

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
    if (!focused || renaming) return;
    const onKey = (e: KeyboardEvent) => {
      const item = list[selIdx];
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setArmed(null);
        setSel(Math.max(0, Math.min(list.length - 1, selIdx + (e.key === "ArrowDown" ? 1 : -1))));
      } else if (e.key === "Enter" && e.metaKey && item) {
        e.preventDefault();
        recordRevealProject(item.dir).catch((err) => setError(errText(err)));
      } else if (e.key === "Enter") {
        e.preventDefault();
        // An armed delete is a question: Enter must not answer it with a
        // new recording — it backs out of the question instead.
        if (armed) setArmed(null);
        else if (item) {
          if (editable(item)) openEditor(item.dir);
        } else if (supported && !running) newRecording();
      } else if (e.key === "F2" && item && !item.active) {
        e.preventDefault();
        startRename(item);
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
  }, [focused, renaming, list, selIdx, armed, supported, running, newRecording, openEditor, remove, startRename, onExit]);

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
        {supported && (
          <span className="flex items-center gap-1.5">
            <button type="button" className={chip} onClick={openOther} title={t("rec4.projects.open_hint")}>
              <FolderOpen size={11} className="mr-1 inline" />
              {t("rec4.projects.open")}
            </button>
            {!running && (
              <button type="button" className={chip} onClick={newRecording}>
                {t("rec2.panel.new")}
              </button>
            )}
          </span>
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
            <div className="text-[var(--color-muted)]">
              {items.length > 0 && query.trim() ? t("rec4.projects.no_match", { query: query.trim() }) : t("rec2.panel.empty")}
            </div>
          )}

          <div className="rounded-md border border-[var(--color-border)] empty:hidden">
            {list.map((p, i) => {
              const selected = focused && i === selIdx;
              const isArmed = armed === p.dir;
              const isRenaming = renaming?.dir === p.dir;
              const changed = lastChange(p);
              return (
                <div
                  key={p.dir}
                  ref={(el) => {
                    if (el) rows.current.set(p.dir, el);
                    else rows.current.delete(p.dir);
                  }}
                  data-testid="record-row"
                  className={`flex items-center gap-2.5 px-2.5 py-2 ${
                    i > 0 ? "border-t border-[var(--color-border)]" : ""
                  } ${selected ? "bg-[var(--color-accent)]/10" : ""}`}
                >
                  <ProjectThumb src={p.thumb} />
                  <div className="min-w-0 flex-1">
                    {isRenaming ? (
                      <input
                        autoFocus
                        aria-label={t("rec4.projects.rename")}
                        maxLength={200}
                        value={renaming.value}
                        onChange={(e) => setRenaming({ dir: p.dir, value: e.target.value })}
                        onFocus={(e) => e.currentTarget.select()}
                        onKeyDown={(e) => {
                          // The input owns its keys: the list and the popup's Esc fallback never see them.
                          e.stopPropagation();
                          if (e.key === "Enter") {
                            e.preventDefault();
                            commitRename();
                          } else if (e.key === "Escape") {
                            e.preventDefault();
                            setRenaming(null);
                            setError(null);
                          }
                        }}
                        className="w-full rounded border border-[var(--color-accent)] bg-[var(--color-bg)] px-1.5 py-0.5 text-[12px] text-[var(--color-fg)] outline-none"
                      />
                    ) : (
                      <div className="flex items-baseline gap-2">
                        <span className="truncate font-medium text-[var(--color-fg)]">{p.title}</span>
                        <span className="shrink-0 text-[10px]">{statusChip(p)}</span>
                      </div>
                    )}
                    <div
                      className="text-[10px] text-[var(--color-muted)]"
                      title={t("rec4.projects.created", { date: new Date(p.createdAt).toLocaleString(locale()) })}
                    >
                      {new Date(changed).toLocaleString(locale())} · {mmss(p.durationMs)}
                    </div>
                    {p.failure && !p.active && (
                      <div className="mt-0.5 text-[10px] text-amber-400">{p.failure}</div>
                    )}
                  </div>
                  {isRenaming ? null : isArmed ? (
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
                      {editable(p) && (
                        <button type="button" className={chip} onClick={() => openEditor(p.dir)}>
                          {t("rec2.panel.edit")}
                        </button>
                      )}
                      {!p.active && (
                        <button type="button" className={chip} onClick={() => startRename(p)}>
                          {t("rec4.projects.rename")}
                        </button>
                      )}
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
