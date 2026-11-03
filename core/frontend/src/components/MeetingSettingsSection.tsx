import { useEffect, useState } from "react";
import { AudioLines } from "lucide-react";
import {
  getMeetingConfig,
  meetingDisplays,
  meetingInputDevices,
  meetingModelDownload,
  meetingModelStatus,
  setMeetingConfig,
  type MeetingConfig,
  type MeetingModel,
} from "../lib/ipc";
import { useT, type MessageKey } from "../lib/i18n";
import { modelSizeLabel } from "../lib/meeting";
import { Row, Section } from "./SettingsSection";

const field = "rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]";

/** Settings → Live transcription: model, language, devices, storage, echo filter, ticker. */
export function MeetingSettingsSection() {
  const t = useT();
  const [cfg, setCfg] = useState<MeetingConfig | null>(null);
  const [models, setModels] = useState<MeetingModel[]>([]);
  const [devices, setDevices] = useState<string[]>([]);
  const [displays, setDisplays] = useState<string[]>([]);

  useEffect(() => {
    void getMeetingConfig().then(setCfg).catch(() => {});
    void meetingModelStatus().then(setModels).catch(() => {});
    void meetingInputDevices().then(setDevices).catch(() => {});
    void meetingDisplays().then(setDisplays).catch(() => {});
  }, []);

  if (!cfg) return null;
  const save = (next: MeetingConfig) => {
    setCfg(next);
    void setMeetingConfig(next).then(setCfg).catch(() => {});
  };
  const tick = (patch: Partial<MeetingConfig["ticker"]>) => save({ ...cfg, ticker: { ...cfg.ticker, ...patch } });
  const pickFolder = async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({ directory: true, multiple: false });
    if (typeof path === "string") save({ ...cfg, root: path });
  };
  const current = models.find((m) => m.id === cfg.model);

  return (
    <Section
      icon={<AudioLines size={16} className="text-[var(--color-accent)]" />}
      id="meeting"
      title={t("settings.section.meeting")}
      subtitle={t("mtg.set.subtitle")}
    >
      <Row label={t("mtg.set.model")} help={t("mtg.set.model_help")}>
        <div className="flex items-center gap-2">
          <select className={field} value={cfg.model} onChange={(e) => save({ ...cfg, model: e.target.value })} data-testid="set-meeting-model">
            {models.map((m) => (
              <option key={m.id} value={m.id}>
                {t(`mtg.model.${m.id}` as MessageKey)} · {modelSizeLabel(m.bytes)}
                {m.installed ? " ✓" : ""}
              </option>
            ))}
          </select>
          {current && !current.installed && (
            <button type="button" className="text-[12px] underline" onClick={() => void meetingModelDownload(current.id)}>
              {t("mtg.download", { size: modelSizeLabel(current.bytes) })}
            </button>
          )}
        </div>
      </Row>
      <Row label={t("mtg.set.lang")} help={t("mtg.set.lang_help")}>
        <select className={field} value={cfg.lang} onChange={(e) => save({ ...cfg, lang: e.target.value as MeetingConfig["lang"] })}>
          <option value="auto">{t("mtg.lang.auto")}</option>
          <option value="de">Deutsch</option>
          <option value="en">English</option>
        </select>
      </Row>
      <Row label={t("mtg.set.sources")}>
        <div className="flex gap-3">
          <label className="flex items-center gap-1.5">
            <input type="checkbox" checked={cfg.mic} onChange={(e) => save({ ...cfg, mic: e.target.checked })} /> {t("mtg.src_mic")}
          </label>
          <label className="flex items-center gap-1.5">
            <input type="checkbox" checked={cfg.sys} onChange={(e) => save({ ...cfg, sys: e.target.checked })} /> {t("mtg.src_sys")}
          </label>
        </div>
      </Row>
      <Row label={t("mtg.set.device")}>
        <select className={field} value={cfg.mic_device ?? ""} onChange={(e) => save({ ...cfg, mic_device: e.target.value || null })}>
          <option value="">{t("mtg.set.device_default")}</option>
          {devices.map((d) => (
            <option key={d} value={d}>
              {d}
            </option>
          ))}
        </select>
      </Row>
      <Row label={t("mtg.set.folder")} help={t("mtg.set.folder_help")}>
        <div className="flex gap-2">
          <input className={`${field} w-full`} placeholder="~/Documents/Transkripte" value={cfg.root ?? ""} onChange={(e) => setCfg({ ...cfg, root: e.target.value || null })} onBlur={() => save(cfg)} />
          <button type="button" className="text-[12px] underline" onClick={() => void pickFolder()}>
            {t("mtg.set.choose")}
          </button>
        </div>
      </Row>
      <Row label={t("mtg.set.audio")} help={t("mtg.set.audio_help")}>
        <div className="flex items-center gap-2">
          <input type="checkbox" checked={cfg.keep_audio} onChange={(e) => save({ ...cfg, keep_audio: e.target.checked })} />
          <input
            type="number"
            min={1}
            max={365}
            className={`${field} w-16`}
            value={cfg.audio_days}
            disabled={!cfg.keep_audio}
            onChange={(e) => save({ ...cfg, audio_days: Number(e.target.value) || 7 })}
          />
          <span className="text-[12px] text-[var(--color-muted)]">{t("mtg.set.days")}</span>
        </div>
      </Row>
      <Row label={t("mtg.set.dedupe")} help={t("mtg.set.dedupe_help")}>
        <div className="flex items-center gap-2">
          <select className={field} value={cfg.dedupe} onChange={(e) => save({ ...cfg, dedupe: e.target.value as MeetingConfig["dedupe"] })}>
            <option value="auto">{t("mtg.dedupe.auto")}</option>
            <option value="always">{t("mtg.dedupe.always")}</option>
            <option value="never">{t("mtg.dedupe.never")}</option>
          </select>
          <input
            type="range"
            min={0.5}
            max={0.95}
            step={0.05}
            value={cfg.dedupe_threshold}
            onChange={(e) => save({ ...cfg, dedupe_threshold: Number(e.target.value) })}
          />
          <span className="tabular-nums text-[12px]">{Math.round(cfg.dedupe_threshold * 100)} %</span>
        </div>
      </Row>
      <Row label={t("mtg.set.ticker")} help={t("mtg.set.ticker_help")}>
        <div className="flex flex-wrap items-center gap-2">
          <input type="checkbox" checked={cfg.ticker.visible} onChange={(e) => tick({ visible: e.target.checked })} />
          <select className={field} value={cfg.ticker.position} onChange={(e) => tick({ position: e.target.value as "top" | "bottom" })}>
            <option value="bottom">{t("mtg.pos.bottom")}</option>
            <option value="top">{t("mtg.pos.top")}</option>
          </select>
          <select className={field} value={cfg.ticker.display ?? ""} onChange={(e) => tick({ display: e.target.value || null })}>
            <option value="">{t("mtg.set.display_cursor")}</option>
            {displays.map((d) => (
              <option key={d} value={d}>
                {d}
              </option>
            ))}
          </select>
        </div>
      </Row>
      {(
        [
          ["width_pct", 30, 100, 5, "mtg.set.width", "%"],
          ["font_px", 11, 28, 1, "mtg.set.font", "px"],
          ["speed_px_s", 20, 200, 10, "mtg.set.speed", "px/s"],
        ] as const
      ).map(([key, min, max, step, label, unit]) => (
        <Row key={key} label={t(label)}>
          <div className="flex items-center gap-2">
            <input type="range" min={min} max={max} step={step} value={cfg.ticker[key]} onChange={(e) => tick({ [key]: Number(e.target.value) })} />
            <span className="tabular-nums text-[12px]">
              {cfg.ticker[key]} {unit}
            </span>
          </div>
        </Row>
      ))}
      <Row label={t("mtg.set.opacity")}>
        <div className="flex items-center gap-2">
          <input type="range" min={0.3} max={1} step={0.05} value={cfg.ticker.opacity} onChange={(e) => tick({ opacity: Number(e.target.value) })} />
          <span className="tabular-nums text-[12px]">{Math.round(cfg.ticker.opacity * 100)} %</span>
        </div>
      </Row>
      <p className="mt-2 text-[12px] text-[var(--color-muted)]">{t("mtg.set.hotkeys")}</p>
    </Section>
  );
}
