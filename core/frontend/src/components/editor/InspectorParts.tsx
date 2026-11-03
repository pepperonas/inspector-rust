import type { CSSProperties, ReactNode } from "react";
import { INSPECTOR_RANGES, clampSetting, type InspectorSetting } from "../../lib/editor";

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

export function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex items-center justify-between gap-2 text-[12px]">
      {label}
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
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
