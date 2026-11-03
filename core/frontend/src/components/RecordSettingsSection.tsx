import { useEffect, useState } from "react";
import { Video } from "lucide-react";
import {
  getRecordConfig,
  recordCameraDevices,
  setRecordConfig,
  type CameraDevice,
  type CameraQuality,
  type RecordConfig,
} from "../lib/ipc";
import { t, useT } from "../lib/i18n";
import { Section, Row } from "./SettingsSection";
import { ShortcutsDialog, useEditorShortcuts } from "./editor/ShortcutsDialog";

/** Settings → Recording: project folder + the defaults the HUD starts with. */
export function RecordSettingsSection() {
  useT();
  const [cfg, setCfg] = useState<RecordConfig | null>(null);
  const [bindings, saveBindings] = useEditorShortcuts();
  const [keysOpen, setKeysOpen] = useState(false);
  const [cams, setCams] = useState<CameraDevice[]>([]);

  useEffect(() => {
    void getRecordConfig().then(setCfg).catch(() => {});
    void recordCameraDevices().then(setCams).catch(() => {});
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
  const box = (
    key: "countdown" | "systemAudio" | "mic" | "openEditor" | "autoZoom" | "camera",
    label: string,
    help?: string,
  ) => (
    <Row label={label} help={help}>
      <input
        type="checkbox"
        data-testid={`set-record-${key}`}
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
      {box("camera", t("rec5.hud.camera"), t("rec5.set.camera_help"))}
      <Row label={t("rec5.set.camera_device")}>
        <select
          data-testid="set-record-camera-device"
          className="rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
          value={cams.some((c) => c.id === cfg.cameraDevice) ? cfg.cameraDevice : ""}
          onChange={(e) => save({ ...cfg, cameraDevice: e.target.value })}
        >
          <option value="">{t("rec5.set.camera_device_default")}</option>
          {cams.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
      </Row>
      <Row label={t("rec5.set.camera_quality")} help={t("rec5.set.camera_quality_help")}>
        <select
          data-testid="set-record-camera-quality"
          className="rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
          value={cfg.cameraQuality ?? "1080p"}
          onChange={(e) => save({ ...cfg, cameraQuality: e.target.value as CameraQuality })}
        >
          {(["1080p", "1440p", "2160p"] as const).map((q) => (
            <option key={q} value={q}>
              {q}
            </option>
          ))}
        </select>
      </Row>
      {box("openEditor", t("rec2.set.open_editor"), t("rec2.set.open_editor_help"))}
      {box("autoZoom", t("rec3.set.auto_zoom"), t("rec3.set.auto_zoom_help"))}
      <Row label={t("rec4.keys.settings_label")} help={t("rec4.keys.settings_help")}>
        <button
          type="button"
          className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[12px]"
          onClick={() => setKeysOpen(true)}
          data-testid="set-record-shortcuts"
        >
          {t("rec4.keys.settings_open")}
        </button>
      </Row>
      {keysOpen && <ShortcutsDialog bindings={bindings} onSave={saveBindings} onClose={() => setKeysOpen(false)} />}
    </Section>
  );
}
