import type { CSSProperties, ReactNode } from "react";
import { COLOR_PRESETS, GRADIENT_PRESETS, INSPECTOR_RANGES, clampSetting, type InspectorSetting } from "../../lib/editor";
import type { EditorAssets, RecordBackground } from "../../lib/ipc";
import { useT } from "../../lib/i18n";

/** Shared controls of the editor's settings column (inspector tabs and the annotation tab). */

export function Group({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <section className="space-y-1.5">
      <h3 className="text-[11px] font-medium text-[var(--color-muted)]">{label}</h3>
      {children}
      {hint && <p className="text-[11px] text-[var(--color-muted)]">{hint}</p>}
    </section>
  );
}

export const Chips = ({ children }: { children: ReactNode }) => <div className="flex flex-wrap gap-1">{children}</div>;

export const chipClass = (active: boolean) =>
  "rounded-md border px-2 py-1 text-[11px] transition-colors duration-(--duration-fast) ease-sharp disabled:opacity-40 " +
  (active
    ? "border-[var(--color-accent)] bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
    : "border-[var(--color-border)] hover:bg-[var(--color-bg)]");

export function Chip({ label, active, disabled, onClick }: { label: string; active: boolean; disabled?: boolean; onClick: () => void }) {
  return (
    <button type="button" aria-pressed={active} disabled={disabled} onClick={onClick} className={chipClass(active)}>
      {label}
    </button>
  );
}

export function Swatch({
  label,
  title,
  active,
  onClick,
  className,
  style,
  children,
}: {
  label: string;
  title?: string;
  active: boolean;
  onClick: () => void;
  className?: string;
  style?: CSSProperties;
  children?: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={active}
      title={title ?? label}
      onClick={onClick}
      style={style}
      className={
        "rounded-md border-2 p-0 transition-colors duration-(--duration-fast) ease-sharp " +
        (active ? "border-[var(--color-accent)]" : "border-transparent hover:border-[var(--color-border)]") +
        (className ? ` ${className}` : "")
      }
    >
      {children}
    </button>
  );
}

export function Toggle({
  label,
  checked,
  onChange,
  disabled,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <label className="flex items-center justify-between gap-2 text-[12px]">
      {label}
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
    </label>
  );
}

/** A slider over `[min, max]`: moves live (`onLive`), one undo step on release (`onEnd`). */
export function RangeSlider({
  label,
  min,
  max,
  step,
  value,
  format,
  onLive,
  onEnd,
  bare,
}: {
  label: string;
  min: number;
  max: number;
  step: number;
  value: number;
  format: (v: number) => string;
  onLive: (v: number) => void;
  onEnd: () => void;
  /** Inside a group that already shows the label. */
  bare?: boolean;
}) {
  return (
    <label className="block space-y-1 text-[12px]">
      {!bare && (
        <span className="flex justify-between">
          <span>{label}</span>
          <span className="tabular-nums text-[var(--color-muted)]">{format(value)}</span>
        </span>
      )}
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        aria-label={label}
        onChange={(e) => {
          const v = Number(e.target.value);
          onLive(Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : min);
        }}
        onPointerUp={onEnd}
        onKeyUp={onEnd}
        onBlur={onEnd}
        className="w-full"
      />
    </label>
  );
}

/** A slider over one of the inspector's settings (`INSPECTOR_RANGES`). */
export function Slider({
  setting,
  onLive,
  ...rest
}: {
  label: string;
  setting: InspectorSetting;
  value: number;
  format: (v: number) => string;
  onLive: (v: number) => void;
  onEnd: () => void;
  bare?: boolean;
}) {
  const r = INSPECTOR_RANGES[setting];
  return <RangeSlider {...rest} min={r.min} max={r.max} step={r.step} onLive={(v) => onLive(clampSetting(setting, v))} />;
}

const sameBg = (a: RecordBackground, b: RecordBackground) => JSON.stringify(a) === JSON.stringify(b);

/** Wallpapers, gradients, colours and an own image: the background picker of the background
 *  tab, also the camera's own background (one picker, never a second copy). */
export function BackgroundPicker({
  value,
  onPick,
  assets,
  assetUrl,
  projectAssetUrl,
  onImport,
}: {
  value: RecordBackground;
  onPick: (bg: RecordBackground) => void;
  assets: EditorAssets | null;
  assetUrl: (path: string) => string;
  projectAssetUrl: (file: string) => string;
  onImport: (kind: "image" | "cursor") => Promise<string | null>;
}) {
  const t = useT();
  const pickImage = async () => {
    const file = await onImport("image");
    if (file) onPick({ kind: "image", file });
  };
  return (
    <>
      <Group label={t("rec3.insp.wallpapers")}>
        <div className="grid grid-cols-4 gap-1.5">
          {(assets?.wallpapers ?? []).map((w, i) => (
            <Swatch
              key={w.id}
              label={t("rec3.insp.wallpaper_n", { n: i + 1 })}
              active={sameBg(value, { kind: "wallpaper", id: w.id })}
              onClick={() => onPick({ kind: "wallpaper", id: w.id })}
              className="aspect-video"
            >
              <img src={assetUrl(w.thumb)} alt="" className="h-full w-full rounded-[5px] object-cover" draggable={false} />
            </Swatch>
          ))}
        </div>
      </Group>
      <Group label={t("rec3.insp.gradients")}>
        <div className="grid grid-cols-8 gap-1.5">
          {GRADIENT_PRESETS.map((g, i) => (
            <Swatch
              key={i}
              label={t("rec3.insp.gradient_n", { n: i + 1 })}
              active={sameBg(value, g)}
              onClick={() => onPick(g)}
              className="aspect-square shadow-[inset_0_0_0_1px_var(--color-border)]"
              style={{ background: `linear-gradient(${g.angleDeg}deg, ${g.stops.join(", ")})` }}
            />
          ))}
        </div>
      </Group>
      <Group label={t("rec3.insp.colors")}>
        <div className="grid grid-cols-8 gap-1.5">
          {COLOR_PRESETS.map((c) => (
            <Swatch
              key={c}
              label={t("rec3.insp.color_n", { color: c })}
              active={sameBg(value, { kind: "color", color: c })}
              onClick={() => onPick({ kind: "color", color: c })}
              className="aspect-square shadow-[inset_0_0_0_1px_var(--color-border)]"
              style={{ background: c }}
            />
          ))}
        </div>
      </Group>
      <Group label={t("rec3.insp.own_image")}>
        <div className="flex items-center gap-2">
          {value.kind === "image" && (
            <img src={projectAssetUrl(value.file)} alt="" className="h-9 w-16 rounded-md border border-[var(--color-accent)] object-cover" />
          )}
          <button type="button" onClick={() => void pickImage()} className={chipClass(false)}>
            {t("rec3.insp.pick_image")}
          </button>
        </div>
      </Group>
    </>
  );
}
