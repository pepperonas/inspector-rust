import { useState, type CSSProperties, type ReactNode } from "react";
import type {
  EditorAssets,
  RecordAspect,
  RecordBackground,
  RecordEditDoc,
  RecordFrame,
  RecordWallpaperMotion,
} from "../../lib/ipc";
import {
  BOUNCE_LEVELS,
  COLOR_PRESETS,
  GRADIENT_PRESETS,
  INSPECTOR_RANGES,
  SHADOW_LEVELS,
  clampSetting,
  motionAllowed,
  setAppearance,
  setCursorStyle,
  type InspectorSetting,
} from "../../lib/editor";
import { useT, type MessageKey } from "../../lib/i18n";

export interface InspectorProps {
  doc: RecordEditDoc;
  /** `null` while loading. */
  assets: EditorAssets | null;
  /** Shipped asset path → URL. */
  assetUrl: (path: string) => string;
  /** File under `<project>/assets` → URL. */
  projectAssetUrl: (file: string) => string;
  /** A discrete change: one undo step. */
  onCommit: (doc: RecordEditDoc) => void;
  /** A slider move: shown live, no undo step. */
  onLive: (doc: RecordEditDoc) => void;
  /** The slider was released: the gesture becomes one undo step. */
  onLiveEnd: () => void;
  /** Picks and imports a file; resolves to its name in the project, or null. */
  onImport: (kind: "image" | "cursor") => Promise<string | null>;
}

type Tab = "background" | "frame" | "cursor";
const TABS: { id: Tab; label: MessageKey }[] = [
  { id: "background", label: "rec3.insp.tab_background" },
  { id: "frame", label: "rec3.insp.tab_frame" },
  { id: "cursor", label: "rec3.insp.tab_cursor" },
];
const MOTIONS: { id: RecordWallpaperMotion; label: MessageKey }[] = [
  { id: "none", label: "rec3.insp.motion_none" },
  { id: "drift", label: "rec3.insp.motion_drift" },
  { id: "aurora", label: "rec3.insp.motion_aurora" },
  { id: "waves", label: "rec3.insp.motion_waves" },
];
const FRAMES: { id: RecordFrame; label: MessageKey }[] = [
  { id: "none", label: "rec3.insp.frame_none" },
  { id: "window", label: "rec3.insp.frame_window" },
  { id: "laptop", label: "rec3.insp.frame_laptop" },
  { id: "phone", label: "rec3.insp.frame_phone" },
  { id: "monitor", label: "rec3.insp.frame_monitor" },
];
const SHADOW_LABELS: MessageKey[] = ["rec3.insp.shadow_0", "rec3.insp.shadow_1", "rec3.insp.shadow_2", "rec3.insp.shadow_3"];
const BOUNCE_LABELS: MessageKey[] = ["rec3.insp.bounce_0", "rec3.insp.bounce_1", "rec3.insp.bounce_2"];
const ASPECTS: RecordAspect[] = ["auto", "source", "16:9", "9:16", "1:1", "4:3", "3:4", "21:9"];
const THEME_LABELS: Record<string, MessageKey> = {
  default: "rec3.insp.theme.default",
  "studio-ink": "rec3.insp.theme.studio-ink",
  "prism-glow": "rec3.insp.theme.prism-glow",
  "pop-coral": "rec3.insp.theme.pop-coral",
  "pixel-candy": "rec3.insp.theme.pixel-candy",
  "star-sprout": "rec3.insp.theme.star-sprout",
  "glass-lens": "rec3.insp.theme.glass-lens",
};

const sameBg = (a: RecordBackground, b: RecordBackground) => JSON.stringify(a) === JSON.stringify(b);
const fmt = (v: number, digits: number) => (Math.round(v * 10 ** digits) / 10 ** digits).toString();

/**
 * The editor's settings column: background, frame and pointer. Every chip and swatch is one
 * undo step (`onCommit`); a slider moves live (`onLive`) and becomes one step on release.
 */
export function Inspector(props: InspectorProps) {
  const t = useT();
  const [tab, setTab] = useState<Tab>("background");
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="ed-inspector">
      <div role="tablist" aria-label={t("rec3.insp.title")} className="flex gap-1 border-b border-[var(--color-border)] px-3 py-2">
        {TABS.map((x) => (
          <button
            key={x.id}
            role="tab"
            aria-selected={tab === x.id}
            onClick={() => setTab(x.id)}
            className={
              "rounded-md px-2.5 py-1 text-[12px] transition-colors duration-(--duration-fast) ease-sharp " +
              (tab === x.id ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]" : "text-[var(--color-muted)] hover:bg-[var(--color-bg)]")
            }
          >
            {t(x.label)}
          </button>
        ))}
      </div>
      <div role="tabpanel" className="min-h-0 flex-1 space-y-4 overflow-y-auto px-3 py-3">
        {tab === "background" && <BackgroundTab {...props} />}
        {tab === "frame" && <FrameTab {...props} />}
        {tab === "cursor" && <CursorTab {...props} />}
      </div>
    </div>
  );
}

function BackgroundTab({ doc, assets, assetUrl, projectAssetUrl, onCommit, onLive, onLiveEnd, onImport }: InspectorProps) {
  const t = useT();
  const a = doc.appearance;
  const bg = a.background;
  const setBg = (background: RecordBackground) => onCommit(setAppearance(doc, { background }));
  const pickImage = async () => {
    const file = await onImport("image");
    if (file) setBg({ kind: "image", file });
  };
  const motionOk = motionAllowed(bg);
  return (
    <>
      <Group label={t("rec3.insp.wallpapers")}>
        <div className="grid grid-cols-4 gap-1.5">
          {(assets?.wallpapers ?? []).map((w, i) => (
            <Swatch
              key={w.id}
              label={t("rec3.insp.wallpaper_n", { n: i + 1 })}
              active={sameBg(bg, { kind: "wallpaper", id: w.id })}
              onClick={() => setBg({ kind: "wallpaper", id: w.id })}
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
              active={sameBg(bg, g)}
              onClick={() => setBg(g)}
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
              active={sameBg(bg, { kind: "color", color: c })}
              onClick={() => setBg({ kind: "color", color: c })}
              className="aspect-square shadow-[inset_0_0_0_1px_var(--color-border)]"
              style={{ background: c }}
            />
          ))}
        </div>
      </Group>
      <Group label={t("rec3.insp.own_image")}>
        <div className="flex items-center gap-2">
          {bg.kind === "image" && (
            <img src={projectAssetUrl(bg.file)} alt="" className="h-9 w-16 rounded-md border border-[var(--color-accent)] object-cover" />
          )}
          <button type="button" onClick={() => void pickImage()} className={chipClass(false)}>
            {t("rec3.insp.pick_image")}
          </button>
        </div>
      </Group>
      <Group label={t("rec3.insp.motion")} hint={motionOk ? undefined : t("rec3.insp.motion_hint")}>
        <Chips>
          {MOTIONS.map((m) => (
            <Chip
              key={m.id}
              label={t(m.label)}
              active={motionOk && a.backgroundMotion === m.id}
              disabled={!motionOk}
              onClick={() => onCommit(setAppearance(doc, { backgroundMotion: m.id }))}
            />
          ))}
        </Chips>
      </Group>
      <Slider
        label={t("rec3.insp.bg_blur")}
        setting="backgroundBlur"
        value={a.backgroundBlur}
        format={(v) => `${Math.round(v * 100)} %`}
        onLive={(v) => onLive(setAppearance(doc, { backgroundBlur: v }))}
        onEnd={onLiveEnd}
      />
    </>
  );
}

function FrameTab({ doc, onCommit, onLive, onLiveEnd }: InspectorProps) {
  const t = useT();
  const a = doc.appearance;
  const live = (patch: Parameters<typeof setAppearance>[1]) => onLive(setAppearance(doc, patch));
  return (
    <>
      <Slider
        label={t("rec3.insp.padding")}
        setting="padding"
        value={a.padding}
        format={(v) => `${Math.round(v)} %`}
        onLive={(padding) => live({ padding })}
        onEnd={onLiveEnd}
      />
      <Slider
        label={t("rec3.insp.radius")}
        setting="radiusPx"
        value={a.radiusPx}
        format={(v) => `${Math.round(v)} px`}
        onLive={(radiusPx) => live({ radiusPx })}
        onEnd={onLiveEnd}
      />
      <Group label={t("rec3.insp.shadow")}>
        <Chips>
          {SHADOW_LEVELS.map((v, i) => (
            <Chip
              key={v}
              label={t(SHADOW_LABELS[i])}
              active={Math.abs(a.shadow - v) < 0.001}
              onClick={() => onCommit(setAppearance(doc, { shadow: v }))}
            />
          ))}
        </Chips>
        <Slider
          label={t("rec3.insp.shadow")}
          setting="shadow"
          value={a.shadow}
          format={(v) => fmt(v, 2)}
          onLive={(shadow) => live({ shadow })}
          onEnd={onLiveEnd}
          bare
        />
      </Group>
      <Slider
        label={t("rec3.insp.motion_blur")}
        setting="motionBlur"
        value={a.motionBlur}
        format={(v) => fmt(v, 2)}
        onLive={(motionBlur) => live({ motionBlur })}
        onEnd={onLiveEnd}
      />
      <Group label={t("rec3.insp.device")}>
        <Chips>
          {FRAMES.map((f) => (
            <Chip key={f.id} label={t(f.label)} active={a.frame === f.id} onClick={() => onCommit(setAppearance(doc, { frame: f.id }))} />
          ))}
        </Chips>
      </Group>
      <Group label={t("rec3.insp.frame_theme")}>
        <Chips>
          {(["light", "dark"] as const).map((th) => (
            <Chip
              key={th}
              label={t(th === "light" ? "rec3.insp.theme_light" : "rec3.insp.theme_dark")}
              active={a.frameTheme === th}
              disabled={a.frame === "none"}
              onClick={() => onCommit(setAppearance(doc, { frameTheme: th }))}
            />
          ))}
        </Chips>
      </Group>
      <Group label={t("rec3.insp.aspect")}>
        <Chips>
          {ASPECTS.map((asp) => (
            <Chip
              key={asp}
              label={asp === "auto" ? t("rec3.insp.aspect_auto") : asp === "source" ? t("rec3.insp.aspect_source") : asp}
              active={doc.output.aspect === asp}
              onClick={() => onCommit({ ...doc, output: { ...doc.output, aspect: asp } })}
            />
          ))}
        </Chips>
      </Group>
    </>
  );
}

function CursorTab({ doc, assets, assetUrl, projectAssetUrl, onCommit, onLive, onLiveEnd, onImport }: InspectorProps) {
  const t = useT();
  if (!assets) return null;
  if (!assets.hasCursorData) {
    return <p className="text-[12px] text-[var(--color-muted)]" data-testid="ed-no-cursor">{t("rec3.insp.no_cursor")}</p>;
  }
  const c = doc.cursor;
  const commit = (patch: Parameters<typeof setCursorStyle>[1]) => onCommit(setCursorStyle(doc, patch));
  const live = (patch: Parameters<typeof setCursorStyle>[1]) => onLive(setCursorStyle(doc, patch));
  const pickCursor = async () => {
    const file = await onImport("cursor");
    if (file) commit({ custom: { file, hotspotX: 0, hotspotY: 0 } });
  };
  return (
    <>
      <Toggle label={t("rec3.insp.cursor_show")} checked={c.show} onChange={(show) => commit({ show })} />
      <Toggle label={t("rec3.insp.cursor_autohide")} checked={c.autoHide} onChange={(autoHide) => commit({ autoHide })} />
      <Group label={t("rec3.insp.cursor_style")}>
        <div className="grid grid-cols-4 gap-1.5">
          {assets.themes.map((th) => {
            const name = THEME_LABELS[th.id] ? t(THEME_LABELS[th.id]) : th.id;
            return (
              <Swatch
                key={th.id}
                label={name}
                title={name}
                active={!c.custom && c.theme === th.id}
                onClick={() => commit({ theme: th.id, custom: null })}
                className="flex aspect-square items-center justify-center bg-[var(--color-bg)]"
              >
                <img src={assetUrl(th.preview)} alt="" className="h-7 w-7 object-contain" draggable={false} />
              </Swatch>
            );
          })}
        </div>
      </Group>
      <Group label={t("rec3.insp.own_cursor")}>
        <div className="flex items-center gap-2">
          {c.custom && (
            <img src={projectAssetUrl(c.custom.file)} alt="" className="h-9 w-9 rounded-md border border-[var(--color-accent)] object-contain" />
          )}
          <button type="button" onClick={() => void pickCursor()} className={chipClass(false)}>
            {t("rec3.insp.pick_cursor")}
          </button>
          {c.custom && (
            <button type="button" onClick={() => commit({ custom: null })} className={chipClass(false)}>
              {t("rec3.insp.remove_cursor")}
            </button>
          )}
        </div>
        {c.custom && (
          <>
            <Slider
              label={t("rec3.insp.hotspot_x")}
              setting="hotspot"
              value={c.custom.hotspotX}
              format={(v) => `${Math.round(v * 100)} %`}
              onLive={(hotspotX) => c.custom && live({ custom: { ...c.custom, hotspotX } })}
              onEnd={onLiveEnd}
            />
            <Slider
              label={t("rec3.insp.hotspot_y")}
              setting="hotspot"
              value={c.custom.hotspotY}
              format={(v) => `${Math.round(v * 100)} %`}
              onLive={(hotspotY) => c.custom && live({ custom: { ...c.custom, hotspotY } })}
              onEnd={onLiveEnd}
            />
          </>
        )}
      </Group>
      <Slider
        label={t("rec3.insp.cursor_size")}
        setting="cursorSize"
        value={c.size}
        format={(v) => `${fmt(v, 1)}×`}
        onLive={(size) => live({ size })}
        onEnd={onLiveEnd}
      />
      <Slider
        label={t("rec3.insp.smoothing")}
        setting="smoothing"
        value={c.smoothing}
        format={(v) => fmt(v, 2)}
        onLive={(smoothing) => live({ smoothing })}
        onEnd={onLiveEnd}
      />
      <Slider
        label={t("rec3.insp.motion_blur")}
        setting="cursorMotionBlur"
        value={c.motionBlur}
        format={(v) => fmt(v, 2)}
        onLive={(motionBlur) => live({ motionBlur })}
        onEnd={onLiveEnd}
      />
      <Group label={t("rec3.insp.bounce")}>
        <Chips>
          {BOUNCE_LEVELS.map((v, i) => (
            <Chip key={v} label={t(BOUNCE_LABELS[i])} active={Math.round(c.clickBounce) === v} onClick={() => commit({ clickBounce: v })} />
          ))}
        </Chips>
      </Group>
      <Toggle label={t("rec3.insp.click_impact")} checked={c.clickImpact} onChange={(clickImpact) => commit({ clickImpact })} />
      <Toggle label={t("rec3.insp.click_sound")} checked={c.clickSound} onChange={(clickSound) => commit({ clickSound })} />
      {c.clickSound && (
        <Slider
          label={t("rec3.insp.click_volume")}
          setting="clickSoundGainDb"
          value={c.clickSoundGainDb}
          format={(v) => `${Math.round(v)} dB`}
          onLive={(clickSoundGainDb) => live({ clickSoundGainDb })}
          onEnd={onLiveEnd}
        />
      )}
    </>
  );
}

// ── small parts ─────────────────────────────────────────────────────────────

function Group({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <section className="space-y-1.5">
      <h3 className="text-[11px] font-medium text-[var(--color-muted)]">{label}</h3>
      {children}
      {hint && <p className="text-[11px] text-[var(--color-muted)]">{hint}</p>}
    </section>
  );
}

const Chips = ({ children }: { children: ReactNode }) => <div className="flex flex-wrap gap-1">{children}</div>;

const chipClass = (active: boolean) =>
  "rounded-md border px-2 py-1 text-[11px] transition-colors duration-(--duration-fast) ease-sharp disabled:opacity-40 " +
  (active
    ? "border-[var(--color-accent)] bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
    : "border-[var(--color-border)] hover:bg-[var(--color-bg)]");

function Chip({ label, active, disabled, onClick }: { label: string; active: boolean; disabled?: boolean; onClick: () => void }) {
  return (
    <button type="button" aria-pressed={active} disabled={disabled} onClick={onClick} className={chipClass(active)}>
      {label}
    </button>
  );
}

function Swatch({
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

function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex items-center justify-between gap-2 text-[12px]">
      {label}
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
    </label>
  );
}

function Slider({
  label,
  setting,
  value,
  format,
  onLive,
  onEnd,
  bare,
}: {
  label: string;
  setting: InspectorSetting;
  value: number;
  format: (v: number) => string;
  onLive: (v: number) => void;
  onEnd: () => void;
  /** Inside a group that already shows the label. */
  bare?: boolean;
}) {
  const r = INSPECTOR_RANGES[setting];
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
        min={r.min}
        max={r.max}
        step={r.step}
        value={value}
        aria-label={label}
        onChange={(e) => onLive(clampSetting(setting, Number(e.target.value)))}
        onPointerUp={onEnd}
        onKeyUp={onEnd}
        onBlur={onEnd}
        className="w-full"
      />
    </label>
  );
}
