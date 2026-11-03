import { useState } from "react";
import type {
  EditorAssets,
  RecordCameraAnchor,
  RecordCameraBgMode,
  RecordCameraMask,
  RecordCameraStyle,
  RecordEditDoc,
  RecordEditorView,
  RecordLayoutPreset,
} from "../../lib/ipc";
import { CAMERA_SIZE_RANGE, formatTimecode, setCamera } from "../../lib/editor";
import {
  ANCHOR_GRID,
  LAYOUT_PRESETS,
  PRESET_LABEL,
  canShrink,
  effectiveRoundness,
  hasAnyCamera,
  passageAt,
  setLayoutRegionPreset,
  withMask,
} from "../../lib/camera-layout";
import { BackgroundPicker, Chip, Chips, Group, RangeSlider, Toggle, chipClass } from "./InspectorParts";
import { CameraFraming } from "./CameraFraming";
import { useT, type MessageKey } from "../../lib/i18n";

export interface CameraTabProps {
  doc: RecordEditDoc;
  /** Rust's spans and per-clip camera answer (never re-derived here). */
  view: Pick<RecordEditorView, "doc" | "layoutSpans" | "clipCameras">;
  /** Playhead, programme ms. */
  positionMs: number;
  /** Whether the helper can cut the camera's background out; `null` while asking (effects stay offered). */
  segmentation: boolean | null;
  /** The camera file to frame on (asset URL), `null` when none can be shown. */
  cameraSrc: string | null;
  assets: EditorAssets | null;
  assetUrl: (path: string) => string;
  projectAssetUrl: (file: string) => string;
  onImport: (kind: "image" | "cursor") => Promise<string | null>;
  onCommit: (doc: RecordEditDoc) => void;
  onLive: (doc: RecordEditDoc) => void;
  onLiveEnd: () => void;
  /** The layout region selected on the timeline: it is the one "from here" edits, wherever the playhead is. */
  selected?: string | null;
  /** "From here" outside any region: a region with `preset` at the playhead (Rust maps it). */
  onAddRegion: (preset: RecordLayoutPreset) => void;
}

const ANCHOR_LABEL: Record<RecordCameraAnchor, MessageKey> = {
  "top-left": "rec5.cam.anchor.top-left",
  top: "rec5.cam.anchor.top",
  "top-right": "rec5.cam.anchor.top-right",
  left: "rec5.cam.anchor.left",
  right: "rec5.cam.anchor.right",
  "bottom-left": "rec5.cam.anchor.bottom-left",
  bottom: "rec5.cam.anchor.bottom",
  "bottom-right": "rec5.cam.anchor.bottom-right",
};
const BG_MODES: { id: RecordCameraBgMode; label: MessageKey }[] = [
  { id: "none", label: "rec5.cam.bg_none" },
  { id: "transparent", label: "rec5.cam.bg_transparent" },
  { id: "blur", label: "rec5.cam.bg_blur" },
  { id: "custom", label: "rec5.cam.bg_custom" },
];
const MASKS: { id: RecordCameraMask; label: MessageKey }[] = [
  { id: "square", label: "rec5.cam.mask_square" },
  { id: "rectangle", label: "rec5.cam.mask_rectangle" },
];

/** A preset's icon: the frame (viewBox 32×22) with where the camera goes. */
export function PresetIcon({ preset }: { preset: RecordLayoutPreset }) {
  const screen = "fill-current opacity-35";
  const cam = "fill-current";
  return (
    <svg viewBox="0 0 32 22" width={30} height={21} aria-hidden="true">
      {preset === "picture-in-picture" && (
        <>
          <rect x="1" y="1" width="30" height="20" rx="2" className={screen} />
          <rect x="21" y="12" width="8" height="7" rx="2" className={cam} />
        </>
      )}
      {preset === "dual-frame" && (
        <>
          <rect x="1" y="4" width="19" height="14" rx="2" className={screen} />
          <rect x="22" y="4" width="9" height="14" rx="2" className={cam} />
        </>
      )}
      {preset === "vertical-stack" && (
        <>
          <rect x="9" y="1" width="14" height="11" rx="2" className={screen} />
          <rect x="9" y="13" width="14" height="8" rx="2" className={cam} />
        </>
      )}
      {preset === "camera-only" && <rect x="1" y="1" width="30" height="20" rx="2" className={cam} />}
      {preset === "no-webcam" && (
        <>
          <rect x="1" y="1" width="30" height="20" rx="2" className={screen} />
          <line x1="4" y1="19" x2="28" y2="3" stroke="currentColor" strokeWidth="2" />
        </>
      )}
    </svg>
  );
}

/** The anchor's icon: the frame with a dot where the camera sits. */
function AnchorIcon({ anchor }: { anchor: RecordCameraAnchor }) {
  const col = anchor.endsWith("left") ? 7 : anchor.endsWith("right") ? 25 : 16;
  const row = anchor.startsWith("top") ? 6 : anchor.startsWith("bottom") ? 16 : 11;
  return (
    <svg viewBox="0 0 32 22" width={26} height={18} aria-hidden="true">
      <rect x="1" y="1" width="30" height="20" rx="2" fill="none" stroke="currentColor" strokeWidth="1.5" opacity="0.5" />
      <circle cx={col} cy={row} r="3.5" className="fill-current" />
    </svg>
  );
}

/**
 * The camera tab: layout (whole video or the passage under the playhead), the camera's look
 * (mirror, shrink during a zoom, mask, rounding, size, position), its background and framing.
 * Without any camera in the recording everything is shown but off, with a note.
 */
export function CameraTab(p: CameraTabProps) {
  const t = useT();
  const { doc } = p;
  const c = doc.camera;
  /** Dragged in the preview: no anchor applies until one is chosen again. */
  const free = c.position != null;
  const hasCamera = hasAnyCamera(p.view);
  const regions = (doc.layoutRegions ?? []).length > 0;
  // A selected region wins over the playhead's (its passage from Rust's span).
  const selectedSpan = p.selected
    ? (p.view.layoutSpans ?? []).find((s) => p.view.doc.layoutRegions[s.index]?.id === p.selected)
    : undefined;
  const passage = !regions
    ? null
    : selectedSpan
      ? { id: p.selected as string, progStartMs: selectedSpan.progStartMs, progEndMs: selectedSpan.progEndMs }
      : passageAt(p.view, p.positionMs);
  // "From here" when the playhead sits in a region; the user's choice otherwise (until it moves in/out).
  const [scopeChoice, setScopeChoice] = useState<"all" | "here" | null>(null);
  const [seenPassage, setSeenPassage] = useState(passage?.id ?? null);
  if ((passage?.id ?? null) !== seenPassage) {
    setSeenPassage(passage?.id ?? null);
    setScopeChoice(null);
  }
  const scope: "all" | "here" = !regions ? "all" : (scopeChoice ?? (passage ? "here" : "all"));
  const region = passage ? doc.layoutRegions.find((r) => r.id === passage.id) : undefined;
  const shownPreset = scope === "here" && region ? region.preset : c.preset;

  const commit = (patch: Partial<RecordCameraStyle>) => p.onCommit(setCamera(doc, patch));
  const live = (patch: Partial<RecordCameraStyle>) => p.onLive(setCamera(doc, patch));
  const pickPreset = (preset: RecordLayoutPreset) => {
    if (scope === "all") commit({ preset });
    else if (region) p.onCommit(setLayoutRegionPreset(doc, region.id, preset));
    else p.onAddRegion(preset);
  };
  const isPip = shownPreset === "picture-in-picture";
  // Effects stay offered until the helper says a definite no (`null` = still asking: no lock
  // without a reason; a scene effect the helper cannot run is drawn without it).
  const segOk = p.segmentation !== false;
  const bg = c.background;
  const roundness = effectiveRoundness(c);

  return (
    <>
      {!hasCamera && (
        <p className="text-[12px] text-[var(--color-muted)]" data-testid="ed-camera-none">
          {t("rec5.cam.none")}
        </p>
      )}
      <fieldset disabled={!hasCamera} className="m-0 min-w-0 space-y-4 border-0 p-0 disabled:opacity-50" data-testid="ed-camera-controls">
        <Group label={t("rec5.cam.layout")}>
          {regions ? (
            <Chips>
              <Chip label={t("rec5.cam.scope_whole")} active={scope === "all"} onClick={() => setScopeChoice("all")} />
              <Chip label={t("rec5.cam.scope_here")} active={scope === "here"} onClick={() => setScopeChoice("here")} />
            </Chips>
          ) : null}
          <p className="text-[11px] text-[var(--color-muted)]" data-testid="ed-camera-scope">
            {scope === "all"
              ? t("rec5.cam.scope_whole_hint")
              : passage
                ? t("rec5.cam.passage", { start: formatTimecode(passage.progStartMs), end: formatTimecode(passage.progEndMs) })
                : t("rec5.cam.passage_none")}
          </p>
          <div className="grid grid-cols-5 gap-1.5">
            {LAYOUT_PRESETS.map((preset) => {
              const active = shownPreset === preset;
              return (
                <button
                  key={preset}
                  type="button"
                  aria-label={t(PRESET_LABEL[preset])}
                  title={t(PRESET_LABEL[preset])}
                  aria-pressed={active}
                  onClick={() => pickPreset(preset)}
                  className={
                    "flex items-center justify-center rounded-md border py-1.5 transition-colors duration-(--duration-fast) ease-sharp " +
                    (active
                      ? "border-[var(--color-accent)] bg-[var(--color-accent)]/15 text-[var(--color-accent)]"
                      : "border-[var(--color-border)] text-[var(--color-muted)] hover:bg-[var(--color-bg)]")
                  }
                >
                  <PresetIcon preset={preset} />
                </button>
              );
            })}
          </div>
          <p className="text-[11px] text-[var(--color-fg)]">{t(PRESET_LABEL[shownPreset])}</p>
        </Group>

        <Group label={t("rec5.cam.shared")}>
          <Toggle label={t("rec5.cam.mirror")} checked={c.mirror} onChange={(mirror) => commit({ mirror })} disabled={!hasCamera} />
          {canShrink(shownPreset) && (
            <Toggle
              label={t("rec5.cam.shrink")}
              checked={c.reactiveZoom}
              onChange={(reactiveZoom) => commit({ reactiveZoom })}
              disabled={!hasCamera}
            />
          )}
        </Group>

        {isPip && (
          <>
            <Group label={t("rec5.cam.mask")}>
              <Chips>
                {MASKS.map((m) => (
                  <Chip key={m.id} label={t(m.label)} active={c.mask === m.id} onClick={() => p.onCommit(setCamera(doc, withMask(c, m.id)))} />
                ))}
              </Chips>
              <RangeSlider
                label={t("rec5.cam.roundness")}
                min={0}
                max={1}
                step={0.01}
                value={roundness}
                format={(v) => `${Math.round(v * 100)} %`}
                onLive={(v) => live({ roundness: v })}
                onEnd={p.onLiveEnd}
              />
              {c.roundness != null && (
                <button type="button" className={chipClass(false)} onClick={() => commit({ roundness: null })}>
                  {t("rec5.cam.roundness_reset")}
                </button>
              )}
            </Group>
            <RangeSlider
              label={t("rec5.cam.size")}
              min={CAMERA_SIZE_RANGE[0]}
              max={CAMERA_SIZE_RANGE[1]}
              step={1}
              value={c.sizePct}
              format={(v) => `${Math.round(v)} %`}
              onLive={(v) => live({ sizePct: Math.round(v) })}
              onEnd={p.onLiveEnd}
            />
            <Group label={t("rec5.cam.position")}>
              <div role="group" aria-label={t("rec5.cam.position")} className="grid w-max grid-cols-3 gap-1">
                {ANCHOR_GRID.flat().map((a, i) =>
                  a ? (
                    <button
                      key={a}
                      type="button"
                      aria-label={t(ANCHOR_LABEL[a])}
                      title={t(ANCHOR_LABEL[a])}
                      aria-pressed={!free && c.anchor === a}
                      onClick={() => commit({ anchor: a, position: null })}
                      className={
                        "flex h-8 w-11 items-center justify-center rounded-md border transition-colors duration-(--duration-fast) ease-sharp " +
                        (!free && c.anchor === a
                          ? "border-[var(--color-accent)] text-[var(--color-accent)]"
                          : "border-[var(--color-border)] text-[var(--color-muted)] hover:bg-[var(--color-bg)]")
                      }
                    >
                      <AnchorIcon anchor={a} />
                    </button>
                  ) : (
                    <span key={`gap-${i}`} aria-hidden="true" />
                  ),
                )}
              </div>
              {free ? (
                <div className="flex flex-wrap items-center gap-2" data-testid="ed-camera-free">
                  <span className="text-[11px] text-[var(--color-muted)]">{t("rec5.cam.position_free")}</span>
                  <button type="button" className={chipClass(false)} onClick={() => commit({ position: null })}>
                    {t("rec5.cam.position_reset")}
                  </button>
                </div>
              ) : (
                <p className="text-[11px] text-[var(--color-muted)]">{t("rec5.cam.position_hint")}</p>
              )}
            </Group>
          </>
        )}

        <Group label={t("rec5.cam.background")}>
          <Chips>
            {BG_MODES.map((m) => (
              <Chip
                key={m.id}
                label={t(m.label)}
                active={bg.mode === m.id}
                disabled={m.id !== "none" && !segOk}
                onClick={() => commit({ background: { ...bg, mode: m.id } })}
              />
            ))}
          </Chips>
          {p.segmentation === false && (
            <p className="text-[11px] text-[var(--color-muted)]" data-testid="ed-camera-seg-off">
              {t("rec5.cam.seg_off")}
            </p>
          )}
        </Group>
        {segOk && bg.mode === "blur" && (
          <RangeSlider
            label={t("rec5.cam.blur")}
            min={0}
            max={1}
            step={0.01}
            value={bg.blur}
            format={(v) => `${Math.round(v * 100)} %`}
            onLive={(blur) => live({ background: { ...bg, blur } })}
            onEnd={p.onLiveEnd}
          />
        )}
        {segOk && bg.mode === "custom" && (
          <BackgroundPicker
            value={bg.custom}
            onPick={(custom) => commit({ background: { ...bg, custom } })}
            assets={p.assets}
            assetUrl={p.assetUrl}
            projectAssetUrl={p.projectAssetUrl}
            onImport={p.onImport}
          />
        )}

        <Group label={t("rec5.cam.framing")}>
          <CameraFraming
            src={hasCamera ? p.cameraSrc : null}
            crop={c.crop}
            disabled={!hasCamera}
            onLive={(crop) => live({ crop })}
            onEnd={p.onLiveEnd}
            onCommit={(crop) => commit({ crop })}
          />
        </Group>
      </fieldset>
    </>
  );
}
