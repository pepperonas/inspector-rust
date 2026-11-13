import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { X } from "lucide-react";
import {
  editorCancelExport, editorExport, editorExportCover, editorExportInfo, editorPlatformPresets,
  type EditorExportInfo, type RecordAspect, type RecordPlatformSpec,
} from "../../lib/ipc";
import { locale, useT, type MessageKey } from "../../lib/i18n";
import {
  applyPreset, clampMbps, CUSTOM_MBPS, GIF_FPS, GIF_WIDTHS, limitText, mbpsText, MP4_FPS, parseStored, PLATFORM_NAME,
  QUALITIES, sizeText, STORE_KEY, TARGET_NAME, TARGETS, type ExportOpts,
} from "../../lib/export-opts";

function loadOpts(): ExportOpts {
  try {
    return parseStored(localStorage.getItem(STORE_KEY));
  } catch {
    return parseStored(null);
  }
}

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
 * On success Rust shows the file in the Finder and raises the toast — the dialog only says where.
 */
export function ExportDialog({
  onClose,
  aspect,
  onAspect,
  positionMs = 0,
}: {
  onClose: () => void;
  /** The document's aspect; a platform preset asks to change it through `onAspect`. */
  aspect?: RecordAspect;
  onAspect?: (a: RecordAspect) => void;
  /** The playhead: the frame a cover image is taken from. */
  positionMs?: number;
}) {
  const t = useT();
  const [opts, setOptsRaw] = useState<ExportOpts>(loadOpts);
  const [phase, setPhase] = useState<Phase>({ kind: "setup" });
  const [presets, setPresets] = useState<RecordPlatformSpec[]>([]);
  const [info, setInfo] = useState<EditorExportInfo | null>(null);
  const [cover, setCover] = useState<{ path: string } | { error: string } | "busy" | null>(null);

  const setOpts = (next: ExportOpts) => {
    setOptsRaw(next);
    try {
      localStorage.setItem(STORE_KEY, JSON.stringify(next));
    } catch {
      // the choice is just not remembered
    }
  };

  useEffect(() => {
    let live = true;
    void editorPlatformPresets().then((p) => live && setPresets(p)).catch(() => undefined);
    return () => { live = false; };
  }, []);

  // Size, codec, bitrate and warnings come from Rust — for the options AND the aspect shown.
  const optsKey = JSON.stringify(opts);
  useEffect(() => {
    let live = true;
    void editorExportInfo(JSON.parse(optsKey) as ExportOpts)
      .then((i) => live && setInfo(i))
      .catch(() => live && setInfo(null));
    return () => { live = false; };
  }, [optsKey, aspect]);

  const [aspectNote, setAspectNote] = useState<RecordAspect | null>(null);
  const pickPreset = (spec: RecordPlatformSpec) => {
    setOpts(applyPreset(opts, spec));
    if (aspect !== undefined && aspect !== spec.aspect && onAspect) {
      onAspect(spec.aspect);
      setAspectNote(spec.aspect);
    }
  };

  const saveCover = async () => {
    setCover("busy");
    try {
      setCover({ path: await editorExportCover(opts, Math.round(positionMs)) });
    } catch (e) {
      setCover({ error: String(e) });
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

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
        style={{ width: 460, maxHeight: "92vh" }}
        className="md3-pop-in overflow-y-auto rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 text-[var(--color-fg)] shadow-2xl"
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
                {presets.length > 0 &&
                  row(
                    t("rec9.platform"),
                    <>
                      <button className={chip(opts.preset === null)} onClick={() => setOpts({ ...opts, preset: null })}>
                        {t("rec9.platform_none")}
                      </button>
                      {presets.map((p) => (
                        <button key={p.id} className={chip(opts.preset === p.id)} onClick={() => pickPreset(p)}>
                          {PLATFORM_NAME[p.id]}
                        </button>
                      ))}
                    </>,
                  )}
                {row(
                  t("rec2.ed.export_size"),
                  TARGETS.map((tg) => (
                    <button key={tg} className={chip(opts.target === tg)} onClick={() => setOpts({ ...opts, target: tg })}>
                      {tg === "source" ? t("rec2.ed.export_source") : TARGET_NAME[tg]}
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
                {row(
                  t("rec9.quality"),
                  <>
                    {QUALITIES.map((q) => (
                      <button
                        key={q}
                        className={chip(opts.quality === q)}
                        onClick={() =>
                          setOpts({ ...opts, quality: q, bitrateMbps: q === "custom" ? (opts.bitrateMbps ?? 12) : null })
                        }
                      >
                        {t(`rec9.q.${q}` as MessageKey)}
                      </button>
                    ))}
                    {opts.quality === "custom" && (
                      <label className="flex items-center gap-1 text-[11px]">
                        <input
                          type="number"
                          aria-label={t("rec9.bitrate")}
                          min={CUSTOM_MBPS[0]}
                          max={CUSTOM_MBPS[1]}
                          step={0.5}
                          value={opts.bitrateMbps ?? 12}
                          onChange={(e) => setOpts({ ...opts, bitrateMbps: clampMbps(Number(e.target.value)) })}
                          style={{ width: 64 }}
                          className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5 tabular-nums"
                        />
                        Mbit/s
                      </label>
                    )}
                  </>,
                )}
                {row(
                  t("rec9.loudness"),
                  (["-16", "-14"] as const).map((l) => (
                    <button key={l} className={chip(opts.loudness === l)} onClick={() => setOpts({ ...opts, loudness: l })}>
                      {l} LUFS
                    </button>
                  )),
                )}
                {row(
                  t("rec9.captions_file"),
                  <>
                    {([null, "srt", "vtt"] as const).map((f) => (
                      <button
                        key={f ?? "off"}
                        disabled={f !== null && info !== null && !info.hasCaptions}
                        className={chip(opts.captionsSidecar === f) + " disabled:opacity-40"}
                        onClick={() => setOpts({ ...opts, captionsSidecar: f })}
                      >
                        {f === null ? t("rec9.captions_none") : f.toUpperCase()}
                      </button>
                    ))}
                    {info !== null && !info.hasCaptions && (
                      <span className="self-center text-[11px] text-[var(--color-muted)]">{t("rec9.captions_need")}</span>
                    )}
                  </>,
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
            {opts.kind === "mp4" && info && (
              <div data-testid="export-info" className="flex flex-col gap-1 rounded-lg bg-[var(--color-bg)] px-2.5 py-2 text-[11px]">
                <span className="tabular-nums">
                  {info.width} × {info.height} · {info.codec === "h264" ? "H.264" : "HEVC"}
                  {info.bitrateBps !== null && ` · ${mbpsText(info.bitrateBps, locale())} Mbit/s`}
                  {info.estimatedBytes !== null && ` · ${t("rec9.about", { size: sizeText(info.estimatedBytes, locale()) })}`}
                </span>
                {info.forcedHevc && <span className="text-[var(--color-muted)]">{t("rec9.forced_hevc")}</span>}
                {info.upscale !== null && (
                  <span className="text-[var(--color-muted)]">
                    {t("rec9.upscaled", { factor: info.upscale.toLocaleString(locale()) })}
                  </span>
                )}
                {info.durationWarning && opts.preset && (
                  <span className="text-amber-500">
                    {t(info.durationWarning[0] === "toolong" ? "rec9.too_long" : "rec9.long", {
                      platform: PLATFORM_NAME[opts.preset],
                      limit: limitText(info.durationWarning[1]),
                    })}
                  </span>
                )}
              </div>
            )}
            {aspectNote && <p className="text-[11px] text-[var(--color-muted)]">{t("rec9.aspect_set", { aspect: aspectNote })}</p>}
            <p className="text-[11px] text-[var(--color-muted)]">{t("rec2.ed.export_where")}</p>
            {cover !== null && cover !== "busy" && (
              <p className={"break-all text-[11px] " + ("error" in cover ? "text-red-500" : "text-[var(--color-muted)]")}>
                {"error" in cover ? errText(t, cover.error) : t("rec9.cover_saved", { path: cover.path })}
              </p>
            )}
            <div className="mt-1 flex items-center justify-end gap-2">
              {opts.kind === "mp4" && (
                <button
                  onClick={() => void saveCover()}
                  disabled={cover === "busy"}
                  title={t("rec9.cover_hint")}
                  className="rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] hover:bg-[var(--color-bg)] disabled:opacity-50"
                >
                  {t("rec9.cover")}
                </button>
              )}
              <button
                onClick={start}
                className="rounded-lg bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)]"
              >
                {t("rec2.ed.export_start")}
              </button>
            </div>
          </div>
        )}
        {phase.kind === "running" && (
          <div className="flex flex-col gap-2">
            <div className="h-2 overflow-hidden rounded-full bg-[var(--color-bg)]">
              <div
                data-testid="export-progress"
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
