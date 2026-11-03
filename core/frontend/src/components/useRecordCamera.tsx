import { useEffect, useRef, useState } from "react";
import {
  recordCameraAuth,
  recordCameraDevices,
  recordCameraRequest,
  recordOpenCameraSettings,
  type CameraAuth,
  type CameraDevice,
} from "../lib/ipc";
import { useT } from "../lib/i18n";

/**
 * Camera permission + devices for the record HUD and Settings — one flow for both:
 * turning the camera on asks once when undecided; refused stays off and shows the hint.
 */
export function useRecordCamera() {
  const [auth, setAuth] = useState<CameraAuth | null>(null);
  const [devices, setDevices] = useState<CameraDevice[]>([]);
  const [attempted, setAttempted] = useState(false);
  const asking = useRef(false);

  useEffect(() => {
    void recordCameraAuth().then(setAuth).catch(() => undefined);
    void recordCameraDevices().then(setDevices).catch(() => undefined);
  }, []);

  /** Resolves true only when the camera may be used; never stores anything itself. */
  const turnOn = async (): Promise<boolean> => {
    if (asking.current) return false;
    asking.current = true;
    try {
      // A refusal may have been lifted in System Settings meanwhile: read it again then.
      const stale = auth === null || auth === "denied" || auth === "restricted";
      let a = stale ? await recordCameraAuth().catch(() => auth) : auth;
      if (a === "not-determined") a = await recordCameraRequest().catch(() => a);
      setAuth(a);
      setAttempted(true);
      return a === "authorized";
    } finally {
      asking.current = false;
    }
  };

  const blocked = auth === "denied" || auth === "restricted";
  /** The camera counts as on only with a stored "on" AND the permission granted. */
  const isOn = (stored: boolean | undefined) => !!stored && auth === "authorized";
  /** The hint shows after a turn-on attempt here, or when "on" was stored but is refused. */
  const showHint = (stored: boolean | undefined) => blocked && (attempted || !!stored);

  return { auth, devices, turnOn, isOn, showHint };
}

export function CameraDeniedHint() {
  const t = useT();
  return (
    <div
      data-testid="rec-camera-denied"
      className="flex items-center gap-2 rounded-lg bg-[var(--color-bg)] px-3 py-1.5 text-[12px] text-[var(--color-muted)]"
    >
      <span className="flex-1">{t("rec5.hud.camera_denied")}</span>
      <button
        type="button"
        data-testid="rec-camera-settings"
        onClick={() => void recordOpenCameraSettings().catch(() => undefined)}
        className="shrink-0 rounded-md border border-[var(--color-border)] px-2 py-1 text-[var(--color-fg)] hover:bg-[var(--color-surface)]"
      >
        {t("rec5.hud.camera_settings")}
      </button>
    </div>
  );
}
