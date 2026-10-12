import { useEffect, useMemo, useState } from "react";
import { imageSizes, type FinderItem, type ImageInfo } from "../lib/ipc";
import {
  describeSpec,
  exceedsCap,
  parseResizeCommand,
  targetSize,
  type ResizeSpec,
} from "../lib/resize";
import { t, tn, useT } from "../lib/i18n";

/**
 * `rz` preview (v0.153.0): which modes exist, and -- for the images actually
 * selected in Finder -- how many there are and what each one becomes.
 *
 * ⚠️ The probe is header-only (`image_sizes`) and debounced. Reading the
 * dimensions of a whole selection on every keystroke is the one thing here
 * that could freeze the UI, so it runs once per settled selection, never per
 * character.
 */
const MODES: ReadonlyArray<{ syntax: string; readonly meaning: string }> = [
  { syntax: "rz 50", get meaning() { return t("rz.mode.one"); } },
  { syntax: "rz 50x25", get meaning() { return t("rz.mode.axes"); } },
  { syntax: "rz 1200x800", get meaning() { return t("rz.mode.px"); } },
  { syntax: "rz px 1200x800", get meaning() { return t("rz.mode.px_explicit"); } },
  { syntax: "rz % 150", get meaning() { return t("rz.mode.pct_explicit"); } },
];

const DEBOUNCE_MS = 220;

export function ResizePanel({
  arg,
  files,
  selectionReadable,
}: {
  arg: string;
  /** Live Finder selection, already filtered to images. */
  files: FinderItem[] | null;
  /** Could the selection be read at all? Denied Automation is NOT "empty". */
  selectionReadable: boolean;
}) {
  useT();
  const spec = useMemo(() => parseResizeCommand(arg), [arg]);
  const paths = useMemo(() => (files ?? []).map((f) => f.path), [files]);
  const key = paths.join(" ");
  const [info, setInfo] = useState<ImageInfo[] | null>(null);
  const [probing, setProbing] = useState(false);

  useEffect(() => {
    if (!key) {
      setInfo(null);
      return;
    }
    let cancelled = false;
    setProbing(true);
    const id = window.setTimeout(() => {
      void imageSizes(key.split(" "))
        .then((r) => {
          if (!cancelled) setInfo(r);
        })
        .catch(() => {
          if (!cancelled) setInfo(null);
        })
        .finally(() => {
          if (!cancelled) setProbing(false);
        });
    }, DEBOUNCE_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(id);
    };
  }, [key]);

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[12px]">
      <div>
        <div className="text-[13px] font-semibold text-[var(--color-fg)]">{t("rz.scale_images")}</div>
        <div className="text-[var(--color-muted)]">
          {spec
            ? t("rz.target", { spec: describeSpec(spec) })
            : t("rz.pick_a_mode_one_number")}
        </div>
      </div>

      <div className="rounded-md border border-[var(--color-border)]">
        {MODES.map((m, i) => (
          <div
            key={m.syntax}
            className={`flex items-baseline gap-2 px-2 py-1 ${
              i > 0 ? "border-t border-[var(--color-border)]" : ""
            }`}
          >
            <code className="shrink-0 font-[var(--font-mono)] text-[11px] text-[var(--color-accent)]">
              {m.syntax}
            </code>
            <span className="text-[var(--color-muted)]">{m.meaning}</span>
          </div>
        ))}
      </div>

      <Outcome
        spec={spec}
        files={files}
        info={info}
        probing={probing}
        selectionReadable={selectionReadable}
      />
    </div>
  );
}

function Outcome({
  spec,
  files,
  info,
  probing,
  selectionReadable,
}: {
  spec: ResizeSpec | null;
  files: FinderItem[] | null;
  info: ImageInfo[] | null;
  probing: boolean;
  selectionReadable: boolean;
}) {
  if (!selectionReadable) {
    // ⚠️ Never render "0 Bilder" here -- that would be a lie. Not being able
    // to READ the selection is a different fact from an empty selection.
    return (
      <div className="rounded-md border border-amber-500/40 bg-amber-500/10 px-2 py-1.5 text-[11px] text-[var(--color-fg)]">
        {t("rz.finder_selection_unreadable_automation_p")}
      </div>
    );
  }
  const n = files?.length ?? 0;
  if (n === 0) {
    return (
      <div className="text-[11px] text-[var(--color-muted)]">
        {t("rz.no_image_selected_in_finder")}
      </div>
    );
  }

  return (
    <div>
      <div className="mb-1 text-[11px] font-medium text-[var(--color-fg)]">
        {tn("rz.selected", n)}
        {probing ? <span className="ml-1 text-[var(--color-muted)]">· {t("rz.measuring")}</span> : null}
      </div>
      <div className="rounded-md border border-[var(--color-border)]">
        {(files ?? []).map((f, i) => {
          const probe = info?.find((p) => p.path === f.path);
          const src =
            probe?.width != null && probe.height != null
              ? { w: probe.width, h: probe.height }
              : null;
          const tgt = src && spec ? targetSize(src, spec) : null;
          const over = tgt ? exceedsCap(tgt) : false;
          return (
            <div
              key={f.path}
              className={`flex items-baseline justify-between gap-2 px-2 py-1 ${
                i > 0 ? "border-t border-[var(--color-border)]" : ""
              }`}
            >
              <span className="truncate text-[var(--color-fg)]">{f.name}</span>
              <span className="shrink-0 font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">
                {src ? `${src.w}×${src.h}` : t("rz.unreadable")}
                {tgt ? (
                  <>
                    {" → "}
                    <span className={over ? "text-red-400" : "text-[var(--color-fg)]"}>
                      {tgt.w}×{tgt.h}
                    </span>
                  </>
                ) : null}
                {probe?.format ? ` (${probe.format})` : ""}
                {over ? <span className="ml-1 text-red-400">{t("rz.over_cap")}</span> : null}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
