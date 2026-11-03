import { Mic, Music, Square, Trash2, Volume2, VolumeX, X } from "lucide-react";
import type { RecordAudioTrack, RecordEditDoc } from "../../lib/ipc";
import {
  FADE_MAX_MS,
  MAX_MIC_DB,
  MIN_MIC_DB,
  SYSTEM_GAIN_RANGE,
  TRACK_GAIN_RANGE,
  removeTrack,
  setMicGain,
  setSystemGain,
  updateTrack,
  formatTimecode,
} from "../../lib/editor";
import { dbfsToLevel, rmsToDbfs } from "../../lib/audio-level";
import { useT } from "../../lib/i18n";
import { Group, RangeSlider, Toggle } from "./InspectorParts";
import { trackName } from "./AudioLane";

/** Level sliders move in 0.5-dB steps, fades in 50-ms steps (upstream's grid). */
export const GAIN_STEP_DB = 0.5;
export const FADE_STEP_MS = 50;

const fmtDb = (v: number) => `${v > 0 ? "+" : ""}${(Math.round(v * 2) / 2).toFixed(1)} dB`;
const fmtMs = (v: number) => `${(v / 1000).toFixed(2)} s`;
/** Snaps to the slider grid (and into range). */
export const snap = (v: number, step: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, Math.round(v / step) * step));

/** A running voiceover take as the editor shows it. */
export interface VoiceoverView {
  /** `starting` until Rust started the take; `finishing` while the track is laid down. */
  phase: "starting" | "recording" | "finishing";
  /** Last microphone RMS (0…1). */
  rms: number;
  elapsedMs: number;
  maxMs: number;
}

export interface AudioTabProps {
  doc: RecordEditDoc;
  /** The project has a microphone recording (the mic level applies). */
  hasMic: boolean;
  selected: string | null;
  onSelect: (id: string | null) => void;
  onCommit: (doc: RecordEditDoc) => void;
  onLive: (doc: RecordEditDoc) => void;
  onLiveEnd: () => void;
  /** Picks a music file and lays it under the playhead (M). */
  onAddMusic: () => void;
  /** Starts a voiceover take at the playhead (V). */
  onVoiceover: () => void;
  /** The newest document (for slider gestures that started on an older one). */
  getDoc?: () => RecordEditDoc;
}

/**
 * The Audio tab: system sound and microphone levels, adding music and recording a voiceover,
 * and the settings of the selected track (level, loop, fades, mute, name, delete). Sliders
 * move live and become one undo step on release.
 */
export function AudioTab(p: AudioTabProps) {
  const t = useT();
  const doc = p.doc;
  const cur = () => p.getDoc?.() ?? doc;
  const sel = p.selected ? (doc.audioTracks.find((x) => x.id === p.selected) ?? null) : null;
  const patch = (next: Partial<RecordAudioTrack>) => (sel ? updateTrack(cur(), sel.id, next) : cur());
  return (
    <div className="space-y-4" data-testid="ed-audio-tab">
      <div className="space-y-1">
        <RangeSlider
          label={t("rec4.audio.system")}
          min={SYSTEM_GAIN_RANGE[0]}
          max={SYSTEM_GAIN_RANGE[1]}
          step={GAIN_STEP_DB}
          value={doc.systemGainDb}
          format={fmtDb}
          onLive={(v) => p.onLive(setSystemGain(cur(), snap(v, GAIN_STEP_DB, ...SYSTEM_GAIN_RANGE)))}
          onEnd={p.onLiveEnd}
        />
        <p className="text-[11px] text-[var(--color-muted)]">{t("rec4.audio.system_hint")}</p>
      </div>
      {p.hasMic && (
        <RangeSlider
          label={t("rec2.ed.mic")}
          min={MIN_MIC_DB}
          max={MAX_MIC_DB}
          step={GAIN_STEP_DB}
          value={doc.micGainDb}
          format={fmtDb}
          onLive={(v) => p.onLive(setMicGain(cur(), snap(v, GAIN_STEP_DB, MIN_MIC_DB, MAX_MIC_DB)))}
          onEnd={p.onLiveEnd}
        />
      )}
      <div className="flex flex-wrap gap-1.5">
        <button
          type="button"
          onClick={p.onAddMusic}
          title={`${t("rec4.audio.add_music_hint")} (M)`}
          className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-2.5 py-1.5 text-[12px] hover:bg-[var(--color-bg)]"
          data-testid="ed-add-music"
        >
          <Music size={13} /> {t("rec4.audio.add_music")}
        </button>
        <button
          type="button"
          onClick={p.onVoiceover}
          title={`${t("rec4.audio.voiceover_hint")} (V)`}
          className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-2.5 py-1.5 text-[12px] hover:bg-[var(--color-bg)]"
          data-testid="ed-voiceover"
        >
          <Mic size={13} /> {t("rec4.audio.voiceover")}
        </button>
      </div>

      {doc.audioTracks.length > 0 ? (
        <Group label={t("rec4.audio.tracks")}>
          <ul className="space-y-1">
            {doc.audioTracks.map((tr) => (
              <li key={tr.id}>
                <button
                  type="button"
                  aria-pressed={sel?.id === tr.id}
                  onClick={() => p.onSelect(sel?.id === tr.id ? null : tr.id)}
                  className={
                    "flex w-full items-center gap-2 rounded-md border px-2 py-1 text-left text-[12px] transition-colors duration-(--duration-fast) ease-sharp " +
                    (sel?.id === tr.id ? "border-[var(--color-accent)]" : "border-[var(--color-border)] hover:bg-[var(--color-bg)]")
                  }
                >
                  {tr.kind === "music" ? <Music size={12} className="shrink-0" /> : <Mic size={12} className="shrink-0" />}
                  <span className={"min-w-0 flex-1 truncate " + (tr.muted ? "line-through opacity-60" : "")}>{trackName(tr)}</span>
                  {tr.muted && <VolumeX size={12} className="shrink-0 text-[var(--color-muted)]" aria-label={t("rec4.audio.muted")} />}
                </button>
              </li>
            ))}
          </ul>
        </Group>
      ) : (
        <p className="text-[11px] text-[var(--color-muted)]">{t("rec4.audio.none")}</p>
      )}

      {sel && (
        <div className="space-y-3 border-t border-[var(--color-border)] pt-3" data-testid="ed-audio-track-settings">
          <label className="block space-y-1 text-[12px]">
            <span>{t("rec4.audio.name")}</span>
            <input
              key={sel.id}
              type="text"
              defaultValue={sel.label}
              placeholder={trackName({ ...sel, label: "" })}
              maxLength={80}
              aria-label={t("rec4.audio.name")}
              onBlur={(e) => {
                const label = e.target.value.trim();
                if (label !== sel.label) p.onCommit(patch({ label }));
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") (e.target as HTMLInputElement).blur();
              }}
              className="w-full rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[12px]"
            />
          </label>
          <RangeSlider
            label={t("rec4.audio.level")}
            min={TRACK_GAIN_RANGE[0]}
            max={TRACK_GAIN_RANGE[1]}
            step={GAIN_STEP_DB}
            value={sel.gainDb}
            format={fmtDb}
            onLive={(v) => p.onLive(patch({ gainDb: snap(v, GAIN_STEP_DB, ...TRACK_GAIN_RANGE) }))}
            onEnd={p.onLiveEnd}
          />
          <RangeSlider
            label={t("rec4.audio.fade_in")}
            min={0}
            max={FADE_MAX_MS}
            step={FADE_STEP_MS}
            value={sel.fadeInMs}
            format={fmtMs}
            onLive={(v) => p.onLive(patch({ fadeInMs: snap(v, FADE_STEP_MS, 0, FADE_MAX_MS) }))}
            onEnd={p.onLiveEnd}
          />
          <RangeSlider
            label={t("rec4.audio.fade_out")}
            min={0}
            max={FADE_MAX_MS}
            step={FADE_STEP_MS}
            value={sel.fadeOutMs}
            format={fmtMs}
            onLive={(v) => p.onLive(patch({ fadeOutMs: snap(v, FADE_STEP_MS, 0, FADE_MAX_MS) }))}
            onEnd={p.onLiveEnd}
          />
          <Toggle label={t("rec4.audio.loop")} checked={sel.looped} onChange={(v) => p.onCommit(patch({ looped: v }))} />
          <Toggle label={t("rec4.audio.mute")} checked={sel.muted} onChange={(v) => p.onCommit(patch({ muted: v }))} />
          <button
            type="button"
            onClick={() => {
              p.onCommit(removeTrack(cur(), sel.id));
              p.onSelect(null);
            }}
            className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-2.5 py-1.5 text-[12px] text-red-500 hover:bg-[var(--color-bg)]"
          >
            <Trash2 size={13} /> {t("rec4.audio.delete")}
          </button>
        </div>
      )}
    </div>
  );
}

/** The meter's 0…1 position for a microphone RMS (−60…0 dBFS). */
export const voiceoverMeter = (rms: number) => dbfsToLevel(rmsToDbfs(rms), -60, 0);

/**
 * A running voiceover take: microphone level, time, Stop (V) and Discard (Esc). Shown in the
 * settings column and the transport bar while a take runs; editing is off meanwhile.
 */
export function VoiceoverPanel({
  take,
  onStop,
  onCancel,
  compact = false,
}: {
  take: VoiceoverView;
  onStop: () => void;
  onCancel: () => void;
  compact?: boolean;
}) {
  const t = useT();
  const busy = take.phase !== "recording";
  const meter = (
    <span className="relative block h-1.5 w-full min-w-16 overflow-hidden rounded-full bg-[var(--color-bg)]" aria-hidden>
      <span
        className="anim-keep absolute inset-y-0 left-0 w-full origin-left rounded-full bg-red-500 transition-transform duration-(--duration-fast) ease-sharp"
        style={{ transform: `scaleX(${voiceoverMeter(take.rms)})` }}
        data-testid="ed-vo-meter"
      />
    </span>
  );
  const time = (
    <span className="text-[12px] tabular-nums">
      {formatTimecode(take.elapsedMs)} <span className="text-[var(--color-muted)]">/ {formatTimecode(take.maxMs)}</span>
    </span>
  );
  const status =
    take.phase === "starting" ? t("rec4.audio.vo_starting") : take.phase === "finishing" ? t("rec4.audio.vo_finishing") : t("rec4.audio.vo_recording");
  const buttons = (
    <>
      <button
        type="button"
        onClick={onStop}
        disabled={busy}
        title={`${t("rec4.audio.vo_stop")} (V)`}
        className="flex items-center gap-1.5 rounded-lg bg-red-500 px-2.5 py-1.5 text-[12px] font-medium text-white disabled:opacity-40"
        data-testid="ed-vo-stop"
      >
        <Square size={12} fill="currentColor" /> {t("rec4.audio.vo_stop")}
      </button>
      <button
        type="button"
        onClick={onCancel}
        disabled={take.phase === "finishing"}
        title={`${t("rec4.audio.vo_cancel")} (Esc)`}
        className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-2.5 py-1.5 text-[12px] hover:bg-[var(--color-bg)] disabled:opacity-40"
        data-testid="ed-vo-cancel"
      >
        <X size={12} /> {t("rec4.audio.vo_cancel")}
      </button>
    </>
  );
  if (compact) {
    return (
      <div className="flex items-center gap-2" role="status" aria-label={status} data-testid="ed-vo-bar">
        <span className="h-2 w-2 shrink-0 rounded-full bg-red-500" aria-hidden />
        {time}
        <span className="w-24">{meter}</span>
        {buttons}
      </div>
    );
  }
  return (
    <div className="space-y-3" role="status" data-testid="ed-vo-panel">
      <p className="flex items-center gap-2 text-[12px] font-medium">
        <Volume2 size={13} className="text-red-500" /> {status}
      </p>
      {meter}
      {time}
      <p className="text-[11px] text-[var(--color-muted)]">{t("rec4.audio.vo_hint")}</p>
      <div className="flex gap-1.5">{buttons}</div>
    </div>
  );
}
