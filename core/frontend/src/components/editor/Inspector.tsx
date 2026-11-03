import { useState } from "react";
import type {
  EditorAssets,
  RecordAnnotationKind,
  RecordAspect,
  RecordBackground,
  RecordEditDoc,
  RecordFrame,
  RecordWallpaperMotion,
  RecordZoomRegion,
} from "../../lib/ipc";
import {
  BOUNCE_LEVELS,
  COLOR_PRESETS,
  GRADIENT_PRESETS,
  SHADOW_LEVELS,
  ZOOM_DEPTHS,
  fmtZoom,
  motionAllowed,
  removeAutoZooms,
  removeZoom,
  setAppearance,
  setCursorStyle,
  updateZoom,
  zoomScale,
} from "../../lib/editor";
import { Chip, Chips, Group, Slider, Swatch, Toggle, chipClass } from "./InspectorParts";
import { AnnotationTab } from "./AnnotationTab";
import { AudioTab, VoiceoverPanel, type VoiceoverView } from "./AudioTab";
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
  /** The zoom region (`id`) selected on the timeline; the zoom tab edits it. */
  selectedZoom: string | null;
  onSelectZoom: (id: string | null) => void;
  /** Plans auto-zooms (Rust `editor_auto_zoom`); the editor records the answer as one step. */
  onAutoZoom: () => void;
  /** The annotation (`id`) selected on the timeline or with Tab; the annotation tab edits it. */
  selectedAnnotation?: string | null;
  onSelectAnnotation?: (id: string | null) => void;
  /** A new annotation of `kind` at the playhead. */
  onAddAnnotation?: (kind: RecordAnnotationKind) => void;
  /** The newest document (for edits that finish after an await). */
  getDoc?: () => RecordEditDoc;
  /** The project has a microphone recording. */
  hasMic?: boolean;
  /** The music/voiceover track (`id`) selected on the timeline; the audio tab edits it. */
  selectedTrack?: string | null;
  onSelectTrack?: (id: string | null) => void;
  /** Picks a music file and lays it under the playhead. */
  onAddMusic?: () => void;
  /** Starts a voiceover take at the playhead. */
  onVoiceover?: () => void;
  /** A running voiceover take: the column shows only it (editing is off meanwhile). */
  voiceover?: VoiceoverView | null;
  onVoiceoverStop?: () => void;
  onVoiceoverCancel?: () => void;
}

type Tab = "background" | "frame" | "cursor" | "zoom" | "annotation" | "audio";
const TABS: { id: Tab; label: MessageKey }[] = [
  { id: "background", label: "rec3.insp.tab_background" },
  { id: "frame", label: "rec3.insp.tab_frame" },
  { id: "cursor", label: "rec3.insp.tab_cursor" },
  { id: "zoom", label: "rec3.zoom.tab" },
  { id: "annotation", label: "rec4.ann.tab" },
  { id: "audio", label: "rec4.audio.tab" },
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
  const selectedAnnotation = props.selectedAnnotation ?? null;
  const selectedTrack = props.selectedTrack ?? null;
  const [tab, setTab] = useState<Tab>(
    props.selectedZoom ? "zoom" : selectedAnnotation ? "annotation" : selectedTrack ? "audio" : "background",
  );
  // Selecting a zoom or an annotation brings its settings up (derived during render).
  const [seenZoom, setSeenZoom] = useState(props.selectedZoom);
  if (props.selectedZoom !== seenZoom) {
    setSeenZoom(props.selectedZoom);
    if (props.selectedZoom) setTab("zoom");
  }
  const [seenAnnotation, setSeenAnnotation] = useState(selectedAnnotation);
  if (selectedAnnotation !== seenAnnotation) {
    setSeenAnnotation(selectedAnnotation);
    if (selectedAnnotation) setTab("annotation");
  }
  const [seenTrack, setSeenTrack] = useState(selectedTrack);
  if (selectedTrack !== seenTrack) {
    setSeenTrack(selectedTrack);
    if (selectedTrack) setTab("audio");
  }
  // A running take: the column shows the take only (and the audio tab once it is over).
  const take = props.voiceover ?? null;
  const [sawTake, setSawTake] = useState(!!take);
  if (!!take !== sawTake) {
    setSawTake(!!take);
    if (take) setTab("audio");
  }
  if (take) {
    return (
      <div className="flex min-h-0 flex-1 flex-col px-3 py-3" data-testid="ed-inspector">
        <VoiceoverPanel take={take} onStop={props.onVoiceoverStop ?? (() => undefined)} onCancel={props.onVoiceoverCancel ?? (() => undefined)} />
      </div>
    );
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="ed-inspector">
      <div role="tablist" aria-label={t("rec3.insp.title")} className="flex flex-wrap gap-1 border-b border-[var(--color-border)] px-3 py-2">
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
        {tab === "zoom" && <ZoomTab {...props} />}
        {tab === "annotation" && (
          <AnnotationTab
            doc={props.doc}
            selected={selectedAnnotation}
            onSelect={props.onSelectAnnotation ?? (() => undefined)}
            onAdd={props.onAddAnnotation ?? (() => undefined)}
            onCommit={props.onCommit}
            onLive={props.onLive}
            onLiveEnd={props.onLiveEnd}
            onImport={props.onImport}
            projectAssetUrl={props.projectAssetUrl}
            getDoc={props.getDoc}
          />
        )}
        {tab === "audio" && (
          <AudioTab
            doc={props.doc}
            hasMic={!!props.hasMic}
            selected={selectedTrack}
            onSelect={props.onSelectTrack ?? (() => undefined)}
            onCommit={props.onCommit}
            onLive={props.onLive}
            onLiveEnd={props.onLiveEnd}
            onAddMusic={props.onAddMusic ?? (() => undefined)}
            onVoiceover={props.onVoiceover ?? (() => undefined)}
            getDoc={props.getDoc}
          />
        )}
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

function ZoomTab({ doc, assets, selectedZoom, onSelectZoom, onAutoZoom, onCommit, onLive, onLiveEnd }: InspectorProps) {
  const t = useT();
  const z = selectedZoom ? doc.zooms.find((x) => x.id === selectedZoom) : undefined;
  const hasData = !!assets?.hasCursorData;
  const autos = doc.zooms.filter((x) => x.origin === "auto").length;
  // Editing a planned zoom makes it the user's: "Remove auto-zooms" must not take it away.
  const patch = (zz: RecordZoomRegion, p: Partial<Omit<RecordZoomRegion, "id">>) => updateZoom(doc, zz.id, { ...p, origin: "manual" });
  return (
    <>
      <Group label={t("rec3.zoom.auto_title")} hint={hasData ? t("rec3.zoom.auto_hint") : undefined}>
        <div className="flex flex-wrap gap-1">
          <button
            type="button"
            className={chipClass(false)}
            disabled={!hasData}
            title={hasData ? t("rec3.zoom.auto_hint") : t("rec3.insp.no_cursor")}
            onClick={onAutoZoom}
          >
            {t("rec3.zoom.auto")}
          </button>
          <button type="button" className={chipClass(false)} disabled={autos === 0} onClick={() => onCommit(removeAutoZooms(doc))}>
            {t("rec3.zoom.remove_auto")}
          </button>
        </div>
      </Group>
      {!z ? (
        <p className="text-[12px] text-[var(--color-muted)]" data-testid="ed-zoom-none">
          {t("rec3.zoom.none_selected")}
        </p>
      ) : (
        <>
          <Group label={t("rec3.zoom.depth")}>
            <Chips>
              {ZOOM_DEPTHS.map((f, i) => (
                <Chip
                  key={f}
                  label={fmtZoom(f)}
                  active={z.customScale == null && z.depth === i + 1}
                  onClick={() => onCommit(patch(z, { depth: i + 1, customScale: null }))}
                />
              ))}
            </Chips>
          </Group>
          <Slider
            label={t("rec3.zoom.own_scale")}
            setting="zoomScale"
            value={zoomScale(z)}
            format={fmtZoom}
            onLive={(v) => onLive(patch(z, { customScale: v }))}
            onEnd={onLiveEnd}
          />
          <Group label={t("rec3.zoom.focus")}>
            <Chips>
              <Chip label={t("rec3.zoom.focus_auto")} active={z.focusMode === "auto"} onClick={() => onCommit(patch(z, { focusMode: "auto" }))} />
              <Chip label={t("rec3.zoom.focus_manual")} active={z.focusMode === "manual"} onClick={() => onCommit(patch(z, { focusMode: "manual" }))} />
            </Chips>
            {z.focusMode === "manual" && (
              <p className="text-[11px] text-[var(--color-muted)]" data-testid="ed-focus-hint">
                {t("rec3.zoom.focus_hint")}
              </p>
            )}
          </Group>
          <Toggle label={t("rec3.zoom.hide_cursor")} checked={z.hideCursor} onChange={(hideCursor) => onCommit(patch(z, { hideCursor }))} />
          <button
            type="button"
            className={chipClass(false) + " text-red-500"}
            onClick={() => {
              onCommit(removeZoom(doc, z.id));
              onSelectZoom(null);
            }}
          >
            {t("rec3.zoom.delete")}
          </button>
        </>
      )}
    </>
  );
}
