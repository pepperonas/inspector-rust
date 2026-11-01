import { useEffect, useState } from "react";
import { Video } from "lucide-react";
import { getRecordConfig, setRecordConfig, type RecordConfig } from "../lib/ipc";
import { t, useT } from "../lib/i18n";
import { Section, Row } from "./SettingsSection";

/** Settings → Recording: project folder + the defaults the HUD starts with. */
export function RecordSettingsSection() {
  useT();
  const [cfg, setCfg] = useState<RecordConfig | null>(null);

  useEffect(() => {
    void getRecordConfig().then(setCfg).catch(() => {});
  }, []);

  const save = (next: RecordConfig) => {
    setCfg(next);
    void setRecordConfig(next).then(setCfg).catch(() => {});
  };

  const pickFolder = async () => {
    if (!cfg) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({ directory: true, multiple: false });
    if (typeof path === "string") save({ ...cfg, projectDir: path });
  };

  if (!cfg) return null;
  const box = (key: "countdown" | "systemAudio" | "mic", label: string) => (
    <Row label={label}>
      <input
        type="checkbox"
        checked={cfg[key]}
        onChange={(e) => save({ ...cfg, [key]: e.target.checked })}
      />
    </Row>
  );

  return (
    <Section
      icon={<Video size={16} className="text-[var(--color-accent)]" />}
      title={t("settings.section.record")}
      subtitle={t("rec2.set.subtitle")}
      id="record"
    >
      <Row label={t("rec2.set.folder")} help={t("rec2.set.folder_help")}>
        <div className="flex gap-2">
          <input
            className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
            value={cfg.projectDir}
            onChange={(e) => setCfg({ ...cfg, projectDir: e.target.value })}
            onBlur={() => save(cfg)}
            spellCheck={false}
          />
          <button
            type="button"
            className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[12px]"
            onClick={() => void pickFolder()}
          >
            {t("common.choose")}
          </button>
        </div>
      </Row>
      {box("countdown", t("rec2.hud.countdown"))}
      <Row label={t("rec2.set.cursor")} help={t("rec2.set.cursor_help")}>
        <select
          className="rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
          value={cfg.cursorMode}
          onChange={(e) => save({ ...cfg, cursorMode: e.target.value as RecordConfig["cursorMode"] })}
        >
          <option value="editable-overlay">{t("rec2.set.cursor_editable")}</option>
          <option value="system">{t("rec2.set.cursor_system")}</option>
        </select>
      </Row>
      {box("systemAudio", t("rec2.hud.system_audio"))}
      {box("mic", t("rec2.hud.mic"))}
    </Section>
  );
}
