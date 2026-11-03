import type { RecordCaptionAnchorH, RecordCaptionAnchorV, RecordCaptionSettings, RecordEditDoc } from "../../lib/ipc";
import {
  CAPTION_FONT_SIZE_UI_RANGE,
  CAPTION_INSET_STEP,
  CAPTION_INSET_X_MAX,
  CAPTION_INSET_Y_MAX,
  CAPTION_PLATE_OPACITY_RANGE,
  CAPTION_WORDS_RANGE,
  TEXT_FONTS,
  captionInsets,
} from "../../lib/editor";
import {
  TEXT_PALETTE,
  captionLook,
  captionLookOf,
  captionPlateOf,
  captionPlatePatch,
  setCaptions,
  type CaptionLookId,
  type CaptionPlate,
} from "../../lib/captions-ui";
import { useT, type MessageKey } from "../../lib/i18n";
import { Chip, Chips, Group, RangeSlider, Swatch, Toggle } from "./InspectorParts";
import { TranscribeBox } from "./TranscribeBox";
import type { Transcription } from "./useTranscription";

export interface CaptionsTabProps {
  doc: RecordEditDoc;
  /** Output width / height (the insets' defaults depend on it). */
  aspect: number;
  tr: Transcription;
  onCommit: (doc: RecordEditDoc) => void;
  onLive: (doc: RecordEditDoc) => void;
  onLiveEnd: () => void;
}

const LOOKS: { id: CaptionLookId; label: MessageKey }[] = [
  { id: "classic", label: "rec6.cap.look_classic" },
  { id: "bold", label: "rec6.cap.look_bold" },
  { id: "minimal", label: "rec6.cap.look_minimal" },
  { id: "light", label: "rec6.cap.look_light" },
];
const PLATES: { id: CaptionPlate; label: MessageKey }[] = [
  { id: "none", label: "rec6.cap.plate_none" },
  { id: "dark", label: "rec6.cap.plate_dark" },
  { id: "light", label: "rec6.cap.plate_light" },
];
const ANCHOR_V: { id: RecordCaptionAnchorV; label: MessageKey }[] = [
  { id: "top", label: "rec6.cap.top" },
  { id: "bottom", label: "rec6.cap.bottom" },
];
const ANCHOR_H: { id: RecordCaptionAnchorH; label: MessageKey }[] = [
  { id: "left", label: "rec6.cap.left" },
  { id: "center", label: "rec6.cap.center" },
  { id: "right", label: "rec6.cap.right" },
];

const pct = (v: number) => `${Math.round(v * 10) / 10} %`;

/** Captions: on/off, the named looks, text, plate, placement and words per line. */
export function CaptionsTab({ doc, aspect, tr, onCommit, onLive, onLiveEnd }: CaptionsTabProps) {
  const t = useT();
  const s = doc.captions;
  const has = (doc.transcripts ?? []).some((x) => x.words.length > 0);
  const commit = (patch: Partial<RecordCaptionSettings>) => onCommit(setCaptions(doc, patch));
  const live = (patch: Partial<RecordCaptionSettings>) => onLive(setCaptions(doc, patch));
  const look = captionLookOf(s);
  const plate = captionPlateOf(s);
  const inset = captionInsets(s, aspect);

  return (
    <>
      <Toggle label={t("rec6.cap.show")} checked={s.enabled} onChange={(enabled) => commit({ enabled })} />
      {!has && !s.enabled && <p className="text-[11px] text-[var(--color-muted)]">{t("rec6.cap.needs_transcript")}</p>}
      <Group label={t("rec6.tr.title")}>
        <TranscribeBox tr={tr} lang={s.lang} compact={has} />
      </Group>
      <Group label={t("rec6.cap.look")}>
        <Chips>
          {LOOKS.map((l) => (
            <Chip key={l.id} label={t(l.label)} active={look === l.id} onClick={() => commit({ ...captionLook(l.id) })} />
          ))}
        </Chips>
      </Group>
      <Group label={t("rec6.cap.font")}>
        <Chips>
          {TEXT_FONTS.map((f) => (
            <Chip key={f} label={f} active={s.fontFamily === f} onClick={() => commit({ fontFamily: f })} />
          ))}
        </Chips>
      </Group>
      <Toggle label={t("rec6.cap.bold")} checked={s.bold} onChange={(bold) => commit({ bold })} />
      <RangeSlider
        label={t("rec6.cap.size")}
        min={CAPTION_FONT_SIZE_UI_RANGE[0]}
        max={CAPTION_FONT_SIZE_UI_RANGE[1]}
        step={1}
        value={Math.min(CAPTION_FONT_SIZE_UI_RANGE[1], Math.max(CAPTION_FONT_SIZE_UI_RANGE[0], s.fontSize))}
        format={(v) => `${Math.round(v)} px`}
        onLive={(fontSize) => live({ fontSize })}
        onEnd={onLiveEnd}
      />
      <Group label={t("rec6.cap.color")}>
        <div className="grid grid-cols-8 gap-1.5">
          {TEXT_PALETTE.map((c) => (
            <Swatch
              key={c}
              label={t("rec3.insp.color_n", { color: c })}
              active={s.color.toLowerCase() === c}
              onClick={() => commit({ color: c })}
              className="aspect-square shadow-[inset_0_0_0_1px_var(--color-border)]"
              style={{ background: c }}
            />
          ))}
        </div>
      </Group>
      <Group label={t("rec6.cap.plate")}>
        <Chips>
          {PLATES.map((p) => (
            <Chip key={p.id} label={t(p.label)} active={plate === p.id} onClick={() => commit(captionPlatePatch(s, p.id))} />
          ))}
          {plate === "custom" && <Chip label={t("rec6.cap.plate_custom")} active onClick={() => undefined} />}
        </Chips>
        {s.plate && (
          <RangeSlider
            label={t("rec6.cap.plate_opacity")}
            min={CAPTION_PLATE_OPACITY_RANGE[0]}
            max={CAPTION_PLATE_OPACITY_RANGE[1]}
            step={0.01}
            value={Math.min(CAPTION_PLATE_OPACITY_RANGE[1], Math.max(CAPTION_PLATE_OPACITY_RANGE[0], s.plateOpacity))}
            format={(v) => `${Math.round(v * 100)} %`}
            onLive={(plateOpacity) => live({ plateOpacity })}
            onEnd={onLiveEnd}
          />
        )}
      </Group>
      <Group label={t("rec6.cap.position")}>
        <Chips>
          {ANCHOR_V.map((a) => (
            <Chip key={a.id} label={t(a.label)} active={s.anchorV === a.id} onClick={() => commit({ anchorV: a.id })} />
          ))}
        </Chips>
        <Chips>
          {ANCHOR_H.map((a) => (
            <Chip key={a.id} label={t(a.label)} active={s.anchorH === a.id} onClick={() => commit({ anchorH: a.id })} />
          ))}
        </Chips>
      </Group>
      <RangeSlider
        label={t(s.anchorV === "top" ? "rec6.cap.from_top" : "rec6.cap.from_bottom")}
        min={0}
        max={CAPTION_INSET_Y_MAX}
        step={CAPTION_INSET_STEP}
        value={inset.y}
        format={pct}
        onLive={(insetY) => live({ insetY })}
        onEnd={onLiveEnd}
      />
      {s.anchorH !== "center" && (
        <RangeSlider
          label={t(s.anchorH === "left" ? "rec6.cap.from_left" : "rec6.cap.from_right")}
          min={0}
          max={CAPTION_INSET_X_MAX}
          step={CAPTION_INSET_STEP}
          value={inset.x}
          format={pct}
          onLive={(insetX) => live({ insetX })}
          onEnd={onLiveEnd}
        />
      )}
      {(s.insetY != null || s.insetX != null) && (
        <button
          type="button"
          className="text-[11px] text-[var(--color-muted)] underline"
          onClick={() => commit({ insetY: null, insetX: null })}
          data-testid="ed-cap-inset-reset"
        >
          {t("rec6.cap.inset_reset")}
        </button>
      )}
      <Group label={t("rec6.cap.words")}>
        <RangeSlider
          label={t("rec6.cap.words_min")}
          min={CAPTION_WORDS_RANGE[0]}
          max={CAPTION_WORDS_RANGE[1]}
          step={1}
          value={s.minWords}
          format={(v) => String(Math.round(v))}
          onLive={(minWords) => live({ minWords })}
          onEnd={onLiveEnd}
        />
        <RangeSlider
          label={t("rec6.cap.words_max")}
          min={CAPTION_WORDS_RANGE[0]}
          max={CAPTION_WORDS_RANGE[1]}
          step={1}
          value={s.maxWords}
          format={(v) => String(Math.round(v))}
          onLive={(maxWords) => live({ maxWords })}
          onEnd={onLiveEnd}
        />
      </Group>
    </>
  );
}
