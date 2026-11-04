import { useEffect, useState } from "react";
import { Video } from "lucide-react";
import {
  getRecordConfig,
  recordCliPath,
  setRecordConfig,
  type CameraQuality,
  type RecordConfig,
} from "../lib/ipc";
import { t, useT } from "../lib/i18n";
import { Section, Row } from "./SettingsSection";
import { ShortcutsDialog, useEditorShortcuts } from "./editor/ShortcutsDialog";
import { CameraDeniedHint, useRecordCamera } from "./useRecordCamera";

function CopyLine({ text, label }: { text: string; label: string }) {
  const [done, setDone] = useState(false);
  const copy = async () => {
    try {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(text);
      setDone(true);
      window.setTimeout(() => setDone(false), 1800);
    } catch {
      /* clipboard unavailable */
    }
  };
  return (
    <div className="mb-1 flex items-center gap-2">
      <span className="shrink-0 text-[11px] text-[var(--color-muted)]">{label}</span>
      <code
        data-testid="set-record-cli-line"
        className="min-w-0 flex-1 select-all truncate rounded border border-[var(--color-border)] px-2 py-1 text-[11px]"
        title={text}
      >
        {text}
      </code>
      <button
        type="button"
        className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[12px] transition-colors duration-(--duration-fast) ease-sharp"
        onClick={() => void copy()}
      >
        {done ? t("rec8.cli.copied") : t("rec8.cli.copy")}
      </button>
    </div>
  );
}

/** Settings → Recording: project folder + the defaults the HUD starts with. */
export function RecordSettingsSection() {
  useT();
  const [cfg, setCfg] = useState<RecordConfig | null>(null);
  /** Bumped by the camera switch; the effect below stores the config it produced. */
  const [cameraEdits, setCameraEdits] = useState(0);
  const [bindings, saveBindings] = useEditorShortcuts();
  const [keysOpen, setKeysOpen] = useState(false);
  const cam = useRecordCamera();
  const [cliPath, setCliPath] = useState<string | null | undefined>(undefined);
  useEffect(() => {
    void recordCliPath().then(setCliPath).catch(() => setCliPath(null));
  }, []);

  useEffect(() => {
    void getRecordConfig().then(setCfg).catch(() => {});
  }, []);

  const save = (next: RecordConfig) => {
    setCfg(next);
    void setRecordConfig(next).then(setCfg).catch(() => {});
  };

  /** Same flow as the HUD: asks once when undecided; refused stays off (hint below). */
  // Functional update after the await, so a change made while the dialog was open survives.
  const setCamera = async (on: boolean) => {
    if (on && !(await cam.turnOn())) return;
    setCfg((prev) => (prev ? { ...prev, camera: on } : prev));
    setCameraEdits((n) => n + 1);
  };
  useEffect(() => {
    if (cameraEdits > 0 && cfg) void setRecordConfig(cfg).then(setCfg).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cameraEdits]);

  const pickFolder = async () => {
    if (!cfg) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({ directory: true, multiple: false });
    if (typeof path === "string") save({ ...cfg, projectDir: path });
  };

  if (!cfg) return null;
  const box = (
    key: "countdown" | "systemAudio" | "mic" | "openEditor" | "autoZoom",
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
      <Row label={t("rec5.hud.camera")} help={t("rec5.set.camera_help")}>
        <input
          type="checkbox"
          data-testid="set-record-camera"
          checked={cam.isOn(cfg.camera)}
          onChange={(e) => void setCamera(e.target.checked)}
        />
      </Row>
      {cam.showHint(cfg.camera) && <CameraDeniedHint />}
      <Row label={t("rec5.set.camera_device")}>
        <select
          data-testid="set-record-camera-device"
          className="rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
          value={cam.devices.some((c) => c.id === cfg.cameraDevice) ? cfg.cameraDevice : ""}
          onChange={(e) => save({ ...cfg, cameraDevice: e.target.value })}
        >
          <option value="">{t("rec5.set.camera_device_default")}</option>
          {cam.devices.map((c) => (
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
      <Row label={t("rec8.cli.label")} help={t("rec8.cli.help")}>
        {cliPath ? (
          <div data-testid="set-record-cli">
            <CopyLine label={t("rec8.cli.mcp")} text={`claude mcp add inspector-rust-record -- "${cliPath}" mcp`} />
            <CopyLine label={t("rec8.cli.link")} text={`ln -s "${cliPath}" ~/.local/bin/ir-record`} />
          </div>
        ) : cliPath === null ? (
          <div data-testid="set-record-cli-dev" className="text-[11px] text-[var(--color-muted)]">
            {t("rec8.cli.dev")}
          </div>
        ) : null}
      </Row>
      {keysOpen && <ShortcutsDialog bindings={bindings} onSave={saveBindings} onClose={() => setKeysOpen(false)} />}
    </Section>
  );
}
