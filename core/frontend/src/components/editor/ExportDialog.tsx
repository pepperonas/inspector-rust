import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { X } from "lucide-react";
import { editorCancelExport, editorExport, type EditorExportOptions, type RecordOutputTarget } from "../../lib/ipc";
import { useT, type MessageKey } from "../../lib/i18n";

const MP4_FPS = [24, 30, 60] as const;
const GIF_FPS = [15, 20, 24, 30] as const;
const GIF_WIDTHS = [480, 640, 854, 1080] as const;
const TARGETS: RecordOutputTarget[] = ["p720", "p1080", "source"];

type Phase =
  | { kind: "setup" }
  | { kind: "running"; frames: number; total: number }
  | { kind: "done"; path: string }
  | { kind: "failed"; reason: string; cancelled: boolean };

const errText = (t: (k: MessageKey) => string, reason: string) =>
  reason.startsWith("record.editor.") ? t(reason as MessageKey) : reason;

/**
 * Export settings + progress. Outcome arrives as `record-export-*` events; the dialog can
 * be closed while an export runs (the helper keeps going), re-opening shows the progress.
 * Task 7 extends it (Finder reveal, remembered settings).
 */
export function ExportDialog({ onClose }: { onClose: () => void }) {
  const t = useT();
  const [opts, setOpts] = useState<EditorExportOptions>({
    kind: "mp4",
    target: "p1080",
    fps: 30,
    codec: "h264",
    gifWidth: 640,
    loopForever: true,
  });
  const [phase, setPhase] = useState<Phase>({ kind: "setup" });

  useEffect(() => {
    const offs: Promise<() => void>[] = [
      listen<{ frames: number; total: number }>("record-export-progress", (e) =>
        setPhase({ kind: "running", frames: e.payload.frames, total: e.payload.total }),
      ),
      listen<{ path: string }>("record-export-done", (e) => setPhase({ kind: "done", path: e.payload.path })),
      listen<{ reason: string; cancelled: boolean }>("record-export-failed", (e) =>
        setPhase({ kind: "failed", reason: e.payload.reason, cancelled: e.payload.cancelled }),
      ),
    ];
    return () => offs.forEach((p) => void p.then((u) => u()));
  }, []);

  const start = async () => {
    setPhase({ kind: "running", frames: 0, total: 0 });
    try {
      await editorExport(opts);
    } catch (e) {
      setPhase({ kind: "failed", reason: String(e), cancelled: false });
    }
  };

  const chip = (active: boolean) =>
    "rounded-full border px-2.5 py-1 text-[11px] font-medium " +
    (active
      ? "border-[var(--color-accent)] bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
      : "border-[var(--color-border)] hover:bg-[var(--color-bg)]");
  const row = (label: string, body: React.ReactNode) => (
    <div className="flex items-center gap-3">
      <span className="w-24 shrink-0 text-[12px] text-[var(--color-muted)]">{label}</span>
      <div className="flex flex-wrap gap-1.5">{body}</div>
    </div>
  );
  const pct = phase.kind === "running" && phase.total > 0 ? Math.min(1, phase.frames / phase.total) : 0;

  return (
    <div className="md3-overlay-in fixed inset-0 z-30 flex items-center justify-center bg-black/50" onPointerDown={onClose}>
      <div
        role="dialog"
        aria-label={t("rec2.ed.export_title")}
        className="md3-pop-in w-[420px] rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 text-[var(--color-fg)] shadow-2xl"
        onPointerDown={(e) => e.stopPropagation()}
      >
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-[14px] font-semibold">{t("rec2.ed.export_title")}</h2>
          <button onClick={onClose} aria-label={t("rec2.ed.close")} className="rounded p-1 hover:bg-[var(--color-bg)]">
            <X size={14} />
          </button>
        </div>
        {phase.kind === "setup" && (
          <div className="flex flex-col gap-2.5">
            {row(
              t("rec2.ed.export_format"),
              (["mp4", "gif"] as const).map((k) => (
                <button key={k} className={chip(opts.kind === k)} onClick={() => setOpts({ ...opts, kind: k, fps: k === "gif" ? 15 : 30 })}>
                  {k.toUpperCase()}
                </button>
              )),
            )}
            {opts.kind === "mp4" ? (
              <>
                {row(
                  t("rec2.ed.export_size"),
                  TARGETS.map((tg) => (
                    <button key={tg} className={chip(opts.target === tg)} onClick={() => setOpts({ ...opts, target: tg })}>
                      {tg === "source" ? t("rec2.ed.export_source") : tg === "p720" ? "720p" : "1080p"}
                    </button>
                  )),
                )}
                {row(
                  "FPS",
                  MP4_FPS.map((f) => (
                    <button key={f} className={chip(opts.fps === f)} onClick={() => setOpts({ ...opts, fps: f })}>
                      {f}
                    </button>
                  )),
                )}
                {row(
                  t("rec2.ed.export_codec"),
                  (["h264", "hevc"] as const).map((c) => (
                    <button key={c} className={chip(opts.codec === c)} onClick={() => setOpts({ ...opts, codec: c })}>
                      {c === "h264" ? "H.264" : "HEVC"}
                    </button>
                  )),
                )}
              </>
            ) : (
              <>
                {row(
                  t("rec2.ed.export_width"),
                  GIF_WIDTHS.map((w) => (
                    <button key={w} className={chip(opts.gifWidth === w)} onClick={() => setOpts({ ...opts, gifWidth: w })}>
                      {w}
                    </button>
                  )),
                )}
                {row(
                  "FPS",
                  GIF_FPS.map((f) => (
                    <button key={f} className={chip(opts.fps === f)} onClick={() => setOpts({ ...opts, fps: f })}>
                      {f}
                    </button>
                  )),
                )}
                {row(
                  t("rec2.ed.export_loop"),
                  <input
                    type="checkbox"
                    checked={opts.loopForever}
                    onChange={(e) => setOpts({ ...opts, loopForever: e.target.checked })}
                  />,
                )}
              </>
            )}
            <p className="text-[11px] text-[var(--color-muted)]">{t("rec2.ed.export_where")}</p>
            <button
              onClick={start}
              className="mt-1 self-end rounded-lg bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)]"
            >
              {t("rec2.ed.export_start")}
            </button>
          </div>
        )}
        {phase.kind === "running" && (
          <div className="flex flex-col gap-2">
            <div className="h-2 overflow-hidden rounded-full bg-[var(--color-bg)]">
              <div
                className="anim-keep h-full origin-left bg-[var(--color-accent)] transition-transform duration-(--duration-fast) ease-sharp"
                style={{ transform: `scaleX(${pct})` }}
              />
            </div>
            <span className="text-[12px] tabular-nums text-[var(--color-muted)]">
              {phase.total > 0 ? `${Math.round(pct * 100)} %` : t("rec2.ed.export_preparing")}
            </span>
            <button
              onClick={() => void editorCancelExport().catch(() => undefined)}
              className="self-end rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] hover:bg-[var(--color-bg)]"
            >
              {t("rec2.ed.export_cancel")}
            </button>
          </div>
        )}
        {phase.kind === "done" && (
          <p className="break-all text-[12px]">{t("rec2.ed.export_done", { path: phase.path })}</p>
        )}
        {phase.kind === "failed" && (
          <div className="flex flex-col gap-2">
            <p className="text-[12px] text-red-500">
              {phase.cancelled ? t("rec2.ed.export_cancelled") : errText(t, phase.reason)}
            </p>
            <button
              onClick={() => setPhase({ kind: "setup" })}
              className="self-end rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] hover:bg-[var(--color-bg)]"
            >
              {t("rec2.ed.export_again")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
