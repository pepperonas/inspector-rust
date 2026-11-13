import { useEffect, useRef, useState } from "react";
import {
  recordCameraPreviewFrame,
  recordCameraPreviewStart,
  recordCameraPreviewStop,
  recordMicDevices,
  recordMicLevel,
  recordMicMeterStart,
  recordMicMeterStop,
  type MicDevice,
  type MicLevel,
} from "../lib/ipc";
import { useT } from "../lib/i18n";
import {
  DEVICE_POLL_MS, deviceMissing, HOT_LEVEL, LEVEL_POLL_MS, parsePreviewFrame, peakStep, PREVIEW_POLL_MS, sameDevices,
  shownDevice, storedName, type Peak,
} from "../lib/record-devices";

/** The microphones, re-read while the component lives (a plugged-in device appears by itself). */
export function useRecordMics(): { devices: MicDevice[]; loaded: boolean } {
  const [devices, setDevices] = useState<MicDevice[]>([]);
  const [loaded, setLoaded] = useState(false);
  useEffect(() => {
    let live = true;
    const read = () =>
      void recordMicDevices()
        .then((d) => {
          if (!live) return;
          setDevices((prev) => (sameDevices(prev, d) ? prev : d));
          setLoaded(true);
        })
        .catch(() => undefined);
    read();
    const id = window.setInterval(read, DEVICE_POLL_MS);
    return () => {
      live = false;
      window.clearInterval(id);
    };
  }, []);
  return { devices, loaded };
}

const selectClass = "rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1.5 text-[12px] outline-none";

/** Microphone choice with its live level. `meter` off = only the choice (Settings). */
export function MicPicker({
  value,
  onChange,
  mics,
  loaded,
  meter,
}: {
  value: string | undefined;
  onChange: (id: string) => void;
  mics: MicDevice[];
  loaded: boolean;
  meter: boolean;
}) {
  const t = useT();
  const missing = deviceMissing(mics, value, loaded);
  return (
    <div className="flex flex-col gap-1.5">
      <select
        data-testid="rec-mic-device"
        aria-label={t("rec10.mic_device")}
        value={shownDevice(mics, value)}
        onChange={(e) => onChange(e.target.value)}
        className={selectClass}
      >
        <option value="">{t("rec10.mic_default")}</option>
        {mics.map((m) => (
          <option key={m.id} value={m.id}>
            {m.name}
          </option>
        ))}
      </select>
      {missing && value && (
        <p data-testid="rec-mic-missing" className="text-[11px] text-amber-500">
          {t("rec10.mic_missing", { name: storedName(value) })}
        </p>
      )}
      {meter && <MicMeter device={shownDevice(mics, value)} />}
    </div>
  );
}

/** Level bar of `device` ("" = the default input). Opens the microphone only while mounted. */
export function MicMeter({ device }: { device: string }) {
  const t = useT();
  const [lvl, setLvl] = useState<MicLevel | null>(null);
  const [peak, setPeak] = useState<Peak>({ level: 0, heldMs: 0 });
  useEffect(() => {
    let live = true;
    void recordMicMeterStart(device).catch(() => undefined);
    const id = window.setInterval(() => {
      void recordMicLevel()
        .then((l) => {
          if (!live) return;
          setLvl(l);
          setPeak((p) => peakStep(p, l?.level ?? 0, LEVEL_POLL_MS));
        })
        .catch(() => undefined);
    }, LEVEL_POLL_MS);
    return () => {
      live = false;
      window.clearInterval(id);
      void recordMicMeterStop().catch(() => undefined);
    };
  }, [device]);

  const level = lvl?.level ?? 0;
  const hot = peak.level >= HOT_LEVEL;
  return (
    <div className="flex items-center gap-2" data-testid="rec-mic-meter">
      <div
        role="meter"
        aria-label={t("rec10.mic_level")}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(level * 100)}
        className="relative flex-1 overflow-hidden rounded-full bg-[var(--color-bg)]"
        style={{ height: 6 }}
      >
        <div
          data-testid="rec-mic-bar"
          className="anim-keep h-full origin-left"
          style={{ transform: `scaleX(${level})`, background: hot ? "#ef4444" : "#10b981" }}
        />
        <div
          className="anim-keep absolute top-0 h-full"
          style={{ left: `${Math.min(99, peak.level * 100)}%`, width: 2, background: hot ? "#ef4444" : "var(--color-fg)", opacity: 0.7 }}
        />
      </div>
      <span className="w-[5.5rem] shrink-0 text-right text-[11px] text-[var(--color-muted)]">
        {lvl && !lvl.live ? t("rec10.mic_silent") : hot ? t("rec10.mic_hot") : t("rec10.mic_ok")}
      </span>
    </div>
  );
}

/** Live picture of camera `device` ("" = the default camera), mirrored like a mirror. */
export function CameraPreview({ device }: { device: string }) {
  const t = useT();
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const urlRef = useRef<string | null>(null);
  useEffect(() => {
    let live = true;
    let gen = 0;
    let busy = false;
    setError(null);
    void recordCameraPreviewStart(device).catch((e) => live && setError(String(e)));
    const id = window.setInterval(() => {
      if (busy) return;
      busy = true;
      void recordCameraPreviewFrame(gen)
        .then((buf) => {
          const f = parsePreviewFrame(buf);
          if (!live || !f) return;
          gen = f.gen;
          const next = URL.createObjectURL(new Blob([f.jpeg as BlobPart], { type: "image/jpeg" }));
          if (urlRef.current) URL.revokeObjectURL(urlRef.current);
          urlRef.current = next;
          setUrl(next);
        })
        .catch(() => undefined)
        .finally(() => {
          busy = false;
        });
    }, PREVIEW_POLL_MS);
    return () => {
      live = false;
      window.clearInterval(id);
      void recordCameraPreviewStop().catch(() => undefined);
      if (urlRef.current) URL.revokeObjectURL(urlRef.current);
      urlRef.current = null;
      setUrl(null);
    };
  }, [device]);

  return (
    <div
      data-testid="rec-camera-preview"
      className="relative mx-auto overflow-hidden rounded-xl bg-black"
      style={{ width: 240, aspectRatio: "16 / 9" }}
    >
      {url ? (
        <img
          src={url}
          alt={t("rec10.camera_preview")}
          className="h-full w-full"
          style={{ objectFit: "cover", transform: "scaleX(-1)" }}
        />
      ) : (
        <span className="absolute inset-0 flex items-center justify-center px-3 text-center text-[11px] text-white/70">
          {error ?? t("rec10.camera_starting")}
        </span>
      )}
    </div>
  );
}
