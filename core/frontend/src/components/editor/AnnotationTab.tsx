import type { ReactNode } from "react";
import type {
  RecordAnnotation,
  RecordAnnotationKind,
  RecordArrowDirection,
  RecordBlurShape,
  RecordBlurStyle,
  RecordBlurTint,
  RecordEditDoc,
  RecordTextAlign,
  RecordTextAnimation,
} from "../../lib/ipc";
import {
  BLUR_BLOCK_RANGE,
  BLUR_INTENSITY_RANGE,
  DEFAULT_TEXT_PLATE,
  FONT_SIZE_RANGE,
  TEXT_FONTS,
  removeAnnotation,
  updateAnnotation,
  type AnnotationBodyPatch,
} from "../../lib/editor";
import { useT, type MessageKey } from "../../lib/i18n";
import { Chip, Chips, Group, RangeSlider, chipClass } from "./InspectorParts";

/** The CSS family the editor registers each bundled annotation font under. */
export const annotationFontFamily = (family: string) => `IR ${family}`;
/** `font-family` value: the bundled face, then a generic fallback of the same kind. */
export const annotationFontCss = (family: string) =>
  `"${annotationFontFamily(family)}", ${family === "IBM Plex Mono" ? "monospace" : family === "Caveat" ? "cursive" : family === "Lora" ? "serif" : "sans-serif"}`;

/** Font files already registered in this window (an editor remount must not add them again). */
const registeredFonts = new Set<string>();

/**
 * Registers the bundled annotation fonts (`EditorAssets.fonts`) as webview font faces, so the
 * picker shows every name in its real face — once per window, however often a project is
 * opened into it. Missing files only lose the preview face.
 */
export function registerAnnotationFonts(fonts: readonly { family: string; regular: string; bold: string }[], url: (path: string) => string) {
  if (typeof FontFace === "undefined" || typeof document === "undefined" || !document.fonts) return;
  for (const f of fonts) {
    for (const [path, weight] of [
      [f.regular, "400"],
      [f.bold, "700"],
    ] as const) {
      const key = `${f.family}\u0000${weight}\u0000${path}`;
      if (registeredFonts.has(key)) continue;
      registeredFonts.add(key);
      const face = new FontFace(annotationFontFamily(f.family), `url("${url(path)}")`, { weight });
      document.fonts.add(face);
      face.load().catch(() => undefined);
    }
  }
}

export const ANNOTATION_KINDS: readonly { id: RecordAnnotationKind; label: MessageKey }[] = [
  { id: "text", label: "rec4.ann.kind_text" },
  { id: "image", label: "rec4.ann.kind_image" },
  { id: "arrow", label: "rec4.ann.kind_arrow" },
  { id: "blur", label: "rec4.ann.kind_blur" },
];

const PLATES: { id: string; value: string; label: MessageKey }[] = [
  { id: "none", value: "transparent", label: "rec4.ann.plate_none" },
  { id: "dark", value: DEFAULT_TEXT_PLATE, label: "rec4.ann.plate_dark" },
  { id: "light", value: "rgba(255, 255, 255, 0.85)", label: "rec4.ann.plate_light" },
];
const ALIGNS: { id: RecordTextAlign; label: MessageKey }[] = [
  { id: "left", label: "rec4.ann.align_left" },
  { id: "center", label: "rec4.ann.align_center" },
  { id: "right", label: "rec4.ann.align_right" },
];
const ANIMATIONS: { id: RecordTextAnimation; label: MessageKey }[] = [
  { id: "none", label: "rec4.ann.anim_none" },
  { id: "fade", label: "rec4.ann.anim_fade" },
  { id: "rise", label: "rec4.ann.anim_rise" },
  { id: "pop", label: "rec4.ann.anim_pop" },
  { id: "slide-left", label: "rec4.ann.anim_slide-left" },
  { id: "typewriter", label: "rec4.ann.anim_typewriter" },
  { id: "pulse", label: "rec4.ann.anim_pulse" },
];
/** The 3×3 direction grid; the middle cell stays empty. */
const DIRECTION_GRID: (RecordArrowDirection | null)[] = ["up-left", "up", "up-right", "left", null, "right", "down-left", "down", "down-right"];
const DIRECTION_GLYPH: Record<RecordArrowDirection, string> = {
  "up-left": "↖",
  up: "↑",
  "up-right": "↗",
  left: "←",
  right: "→",
  "down-left": "↙",
  down: "↓",
  "down-right": "↘",
};
const BLUR_STYLES: { id: RecordBlurStyle; label: MessageKey }[] = [
  { id: "blur", label: "rec4.ann.blur_style_blur" },
  { id: "mosaic", label: "rec4.ann.blur_style_mosaic" },
];
const BLUR_SHAPES: { id: RecordBlurShape; label: MessageKey }[] = [
  { id: "rectangle", label: "rec4.ann.shape_rectangle" },
  { id: "oval", label: "rec4.ann.shape_oval" },
  { id: "freehand", label: "rec4.ann.shape_freehand" },
];
const BLUR_TINTS: { id: RecordBlurTint; label: MessageKey }[] = [
  { id: "white", label: "rec4.ann.tint_white" },
  { id: "black", label: "rec4.ann.tint_black" },
];
/** Arrow stroke range in the picker (upstream's UI; Rust only requires a finite ≥ 0 value). */
const ARROW_STROKE = { min: 1, max: 16, step: 1 };

/** A `#rrggbb` the colour input can show (anything else → `fallback`). */
const hexOf = (c: string, fallback: string) => (/^#[0-9a-f]{6}$/i.test(c) ? c : fallback);

export interface AnnotationTabProps {
  doc: RecordEditDoc;
  selected: string | null;
  onSelect: (id: string | null) => void;
  /** A new annotation of `kind` at the playhead (the editor resolves the source position). */
  onAdd: (kind: RecordAnnotationKind) => void;
  onCommit: (doc: RecordEditDoc) => void;
  onLive: (doc: RecordEditDoc) => void;
  onLiveEnd: () => void;
  onImport: (kind: "image" | "cursor") => Promise<string | null>;
  projectAssetUrl: (file: string) => string;
  /** The newest document — an edit made after an await (the file dialog) must start from it. */
  getDoc?: () => RecordEditDoc;
}

/**
 * The inspector's **Annotation** tab: add buttons, and the settings of the annotation selected on
 * the timeline (or with Tab). Chips are one undo step; sliders, colour pickers and the text field
 * move live and become one step when released or left.
 */
export function AnnotationTab(props: AnnotationTabProps) {
  const t = useT();
  const { doc, selected, onAdd } = props;
  const a = selected ? doc.annotations.find((x) => x.id === selected) : undefined;
  return (
    <>
      <Group label={t("rec4.ann.add")} hint={t("rec4.ann.add_hint")}>
        <Chips>
          {ANNOTATION_KINDS.map((k) => (
            <button key={k.id} type="button" className={chipClass(false)} onClick={() => onAdd(k.id)} data-testid={`ed-ann-add-${k.id}`}>
              {t(k.label)}
            </button>
          ))}
        </Chips>
      </Group>
      {!a ? (
        <p className="text-[12px] text-[var(--color-muted)]" data-testid="ed-ann-none">
          {t("rec4.ann.none_selected")}
        </p>
      ) : (
        <AnnotationSettings {...props} a={a} />
      )}
    </>
  );
}

function AnnotationSettings({ doc, a, onSelect, onCommit, onLive, onLiveEnd, onImport, projectAssetUrl, getDoc }: AnnotationTabProps & { a: RecordAnnotation }) {
  const t = useT();
  const body = (patch: AnnotationBodyPatch) => updateAnnotation(doc, a.id, { body: patch });
  const commit = (patch: AnnotationBodyPatch) => onCommit(body(patch));
  const live = (patch: AnnotationBodyPatch) => onLive(body(patch));
  const b = a.body;
  let settings: ReactNode;
  if (b.kind === "text") {
    settings = (
      <>
        <label className="block space-y-1 text-[12px]">
          <span>{t("rec4.ann.content")}</span>
          <textarea
            value={b.content}
            rows={3}
            placeholder={t("rec4.ann.content_placeholder")}
            onChange={(e) => live({ content: e.target.value })}
            onBlur={onLiveEnd}
            className="w-full resize-y rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px]"
            style={{ fontFamily: annotationFontCss(b.fontFamily), fontWeight: b.bold ? 700 : 400 }}
            data-testid="ed-ann-content"
          />
        </label>
        <Group label={t("rec4.ann.font")}>
          <div className="grid grid-cols-1 gap-1" role="radiogroup" aria-label={t("rec4.ann.font")}>
            {TEXT_FONTS.map((f) => (
              <button
                key={f}
                type="button"
                role="radio"
                aria-checked={b.fontFamily === f}
                onClick={() => commit({ fontFamily: f })}
                className={chipClass(b.fontFamily === f) + " text-left text-[13px]"}
                style={{ fontFamily: annotationFontCss(f) }}
              >
                {f}
              </button>
            ))}
          </div>
        </Group>
        <RangeSlider
          label={t("rec4.ann.font_size")}
          min={FONT_SIZE_RANGE[0]}
          max={FONT_SIZE_RANGE[1]}
          step={1}
          value={b.fontSize}
          format={(v) => `${Math.round(v)} px`}
          onLive={(fontSize) => live({ fontSize })}
          onEnd={onLiveEnd}
        />
        <ColorRow label={t("rec4.ann.color")} value={hexOf(b.color, "#ffffff")} onLive={(color) => live({ color })} onEnd={onLiveEnd} />
        <Group label={t("rec4.ann.plate")}>
          <Chips>
            {PLATES.map((p) => (
              <Chip key={p.id} label={t(p.label)} active={b.background === p.value} onClick={() => commit({ background: p.value })} />
            ))}
          </Chips>
        </Group>
        <Group label={t("rec4.ann.style")}>
          <Chips>
            <Chip label={t("rec4.ann.bold")} active={b.bold} onClick={() => commit({ bold: !b.bold })} />
            <Chip label={t("rec4.ann.italic")} active={b.italic} onClick={() => commit({ italic: !b.italic })} />
            <Chip label={t("rec4.ann.underline")} active={b.underline} onClick={() => commit({ underline: !b.underline })} />
          </Chips>
        </Group>
        <Group label={t("rec4.ann.align")}>
          <Chips>
            {ALIGNS.map((x) => (
              <Chip key={x.id} label={t(x.label)} active={b.align === x.id} onClick={() => commit({ align: x.id })} />
            ))}
          </Chips>
        </Group>
        <Group label={t("rec4.ann.animation")}>
          <Chips>
            {ANIMATIONS.map((x) => (
              <Chip key={x.id} label={t(x.label)} active={b.animation === x.id} onClick={() => commit({ animation: x.id })} />
            ))}
          </Chips>
        </Group>
      </>
    );
  } else if (b.kind === "image") {
    const pick = async () => {
      const file = await onImport("image");
      if (!file) return;
      // The dialog took a while: build on the document as it is now, not as it was rendered.
      const latest = getDoc?.() ?? doc;
      if (!latest.annotations.some((x) => x.id === a.id && x.body.kind === "image")) return;
      onCommit(updateAnnotation(latest, a.id, { body: { file } }));
    };
    settings = (
      <Group label={t("rec4.ann.image")} hint={b.file ? undefined : t("rec4.ann.image_empty")}>
        <div className="flex items-center gap-2">
          {b.file && (
            <img src={projectAssetUrl(b.file)} alt="" className="h-12 w-20 rounded-md border border-[var(--color-border)] object-contain" />
          )}
          <button type="button" className={chipClass(false)} onClick={() => void pick()} data-testid="ed-ann-pick-image">
            {t("rec4.ann.pick_image")}
          </button>
        </div>
      </Group>
    );
  } else if (b.kind === "arrow") {
    settings = (
      <>
        <Group label={t("rec4.ann.direction")}>
          <div className="grid w-28 grid-cols-3 gap-1" role="radiogroup" aria-label={t("rec4.ann.direction")}>
            {DIRECTION_GRID.map((d, i) =>
              d ? (
                <button
                  key={d}
                  type="button"
                  role="radio"
                  aria-checked={b.direction === d}
                  aria-label={t(`rec4.ann.dir_${d}` as MessageKey)}
                  title={t(`rec4.ann.dir_${d}` as MessageKey)}
                  onClick={() => commit({ direction: d })}
                  className={chipClass(b.direction === d) + " aspect-square text-[14px]"}
                >
                  {DIRECTION_GLYPH[d]}
                </button>
              ) : (
                <span key={i} />
              ),
            )}
          </div>
        </Group>
        <ColorRow label={t("rec4.ann.color")} value={hexOf(b.color, "#34b27b")} onLive={(color) => live({ color })} onEnd={onLiveEnd} />
        <RangeSlider
          label={t("rec4.ann.stroke")}
          min={ARROW_STROKE.min}
          max={ARROW_STROKE.max}
          step={ARROW_STROKE.step}
          value={b.strokeWidth}
          format={(v) => `${Math.round(v)}`}
          onLive={(strokeWidth) => live({ strokeWidth })}
          onEnd={onLiveEnd}
        />
      </>
    );
  } else {
    settings = (
      <>
        <Group label={t("rec4.ann.blur_style")}>
          <Chips>
            {BLUR_STYLES.map((x) => (
              <Chip key={x.id} label={t(x.label)} active={b.style === x.id} onClick={() => commit({ style: x.id })} />
            ))}
          </Chips>
        </Group>
        <Group label={t("rec4.ann.shape")} hint={b.shape === "freehand" ? t("rec4.ann.freehand_hint") : undefined}>
          <Chips>
            {BLUR_SHAPES.map((x) => (
              <Chip key={x.id} label={t(x.label)} active={b.shape === x.id} onClick={() => commit({ shape: x.id })} />
            ))}
          </Chips>
        </Group>
        <Group label={t("rec4.ann.tint")} hint={b.style === "mosaic" ? undefined : t("rec4.ann.tint_hint")}>
          <Chips>
            {BLUR_TINTS.map((x) => (
              <Chip key={x.id} label={t(x.label)} active={b.color === x.id} disabled={b.style !== "mosaic"} onClick={() => commit({ color: x.id })} />
            ))}
          </Chips>
        </Group>
        {b.style === "blur" ? (
          <RangeSlider
            label={t("rec4.ann.intensity")}
            min={BLUR_INTENSITY_RANGE[0]}
            max={BLUR_INTENSITY_RANGE[1]}
            step={1}
            value={b.intensity}
            format={(v) => `${Math.round(v)}`}
            onLive={(intensity) => live({ intensity })}
            onEnd={onLiveEnd}
          />
        ) : (
          <RangeSlider
            label={t("rec4.ann.block_size")}
            min={BLUR_BLOCK_RANGE[0]}
            max={BLUR_BLOCK_RANGE[1]}
            step={1}
            value={b.blockSize}
            format={(v) => `${Math.round(v)} px`}
            onLive={(blockSize) => live({ blockSize })}
            onEnd={onLiveEnd}
          />
        )}
      </>
    );
  }
  return (
    <div className="space-y-4" data-testid={`ed-ann-settings-${b.kind}`}>
      {settings}
      <button
        type="button"
        className={chipClass(false) + " text-red-500"}
        onClick={() => {
          onCommit(removeAnnotation(doc, a.id));
          onSelect(null);
        }}
      >
        {t("rec4.ann.delete")}
      </button>
    </div>
  );
}

/** A colour picker: moves live while picking, one undo step when left. */
function ColorRow({ label, value, onLive, onEnd }: { label: string; value: string; onLive: (hex: string) => void; onEnd: () => void }) {
  return (
    <label className="flex items-center justify-between gap-2 text-[12px]">
      {label}
      <input
        type="color"
        value={value}
        aria-label={label}
        onChange={(e) => onLive(e.target.value)}
        onBlur={onEnd}
        className="h-7 w-10 cursor-pointer rounded border border-[var(--color-border)] bg-transparent"
      />
    </label>
  );
}
