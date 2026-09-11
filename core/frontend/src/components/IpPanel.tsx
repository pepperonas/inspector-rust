import { useCallback, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Globe2, Loader2, MapPin, RefreshCw } from "lucide-react";
import { ipFetch, type IpReport } from "../lib/ipc";
import { WORLD_MASK_H, WORLD_MASK_W, isLand, project } from "../lib/worldmask";

type Status = "loading" | "ok" | "error";

/** Public-IP lookup with an offline dotted world map and an OSM deep link. */
export function IpPanel({ focused, onExit }: { focused: boolean; onExit: () => void }) {
  const [report, setReport] = useState<IpReport | null>(null);
  const [status, setStatus] = useState<Status>("loading");
  const [error, setError] = useState("");
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const load = useCallback(async () => {
    setStatus("loading");
    setError("");
    try {
      setReport(await ipFetch());
      setStatus("ok");
    } catch (e) {
      setStatus("error");
      setError(String(e).replace(/^.*Error:\s*/, ""));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (
      !report ||
      report.latitude == null ||
      report.longitude == null ||
      !canvasRef.current
    )
      return;
    const canvas = canvasRef.current;
    const rect = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const width = Math.max(320, Math.round(rect.width * dpr));
    const height = Math.max(160, Math.round(rect.height * dpr));
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const styles = getComputedStyle(canvas);
    const muted = styles.getPropertyValue("--color-muted").trim() || "#64748b";
    const accent = styles.getPropertyValue("--color-accent").trim() || "#60a5fa";
    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = styles.getPropertyValue("--color-surface").trim() || "#111827";
    ctx.fillRect(0, 0, width, height);
    const sx = width / WORLD_MASK_W;
    const sy = height / WORLD_MASK_H;
    ctx.fillStyle = muted;
    ctx.globalAlpha = 0.45;
    for (let row = 0; row < WORLD_MASK_H; row += 2) {
      for (let col = 0; col < WORLD_MASK_W; col += 2) {
        if (isLand(col, row))
          ctx.fillRect(col * sx, row * sy, Math.max(1, sx), Math.max(1, sy));
      }
    }
    ctx.globalAlpha = 1;
    const point = project(report.longitude, report.latitude);
    const x = point.fx * width;
    const y = point.fy * height;
    ctx.strokeStyle = accent;
    ctx.globalAlpha = 0.3;
    ctx.beginPath();
    ctx.arc(x, y, Math.max(12, width * 0.025), 0, Math.PI * 2);
    ctx.stroke();
    ctx.globalAlpha = 1;
    ctx.fillStyle = accent;
    ctx.beginPath();
    ctx.arc(x, y, 5 * dpr, 0, Math.PI * 2);
    ctx.fill();
  }, [report]);

  useEffect(() => {
    if (!focused) return;
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onExit();
      }
      if (event.key.toLowerCase() === "r") {
        event.preventDefault();
        void load();
      }
    };
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, [focused, load, onExit]);

  const mapUrl =
    report?.latitude != null && report.longitude != null
      ? `https://www.openstreetmap.org/?mlat=${report.latitude}&mlon=${report.longitude}#map=8/${report.latitude}/${report.longitude}`
      : null;
  const place = [report?.city, report?.region, report?.country]
    .filter(Boolean)
    .join(", ");

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-3 text-sm">
      <div className="flex items-center gap-2">
        <Globe2 size={17} className="text-[var(--color-accent)]" />
        <span className="font-semibold">What is my IP?</span>
        <button
          onClick={() => void load()}
          disabled={status === "loading"}
          className="ml-auto rounded p-1.5 text-[var(--color-muted)] hover:bg-[var(--color-bg)]"
          title="Refresh (R)"
        >
          <RefreshCw size={14} className={status === "loading" ? "animate-spin" : ""} />
        </button>
      </div>
      {status === "loading" && (
        <div className="flex flex-1 items-center justify-center text-[var(--color-muted)]">
          <Loader2 className="animate-spin" size={24} />
        </div>
      )}
      {status === "error" && (
        <div
          role="alert"
          className="rounded-lg border border-red-400/40 bg-red-400/10 p-3 text-red-300"
        >
          {error}
          <div className="mt-2 text-xs text-[var(--color-muted)]">
            Network lookup needs an internet connection.
          </div>
        </div>
      )}
      {status === "ok" && report && (
        <>
          <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4">
            <div className="text-xs uppercase tracking-widest text-[var(--color-muted)]">
              Public IPv4 / IPv6
            </div>
            <div className="mt-1 break-all font-[var(--font-mono)] text-2xl font-semibold">
              {report.ip}
            </div>
            {place && (
              <div className="mt-2 flex items-center gap-1.5 text-[var(--color-fg)]">
                <MapPin size={14} className="text-[var(--color-accent)]" />
                {place}
                {report.postal ? ` · ${report.postal}` : ""}
              </div>
            )}
          </div>
          {report.latitude != null && report.longitude != null && (
            <div
              className="relative overflow-hidden rounded-xl border border-[var(--color-border)]"
              style={{ aspectRatio: "2 / 1" }}
            >
              <canvas
                ref={canvasRef}
                className="h-full w-full"
                aria-label="Approximate IP location map"
              />
            </div>
          )}
          <div className="grid grid-cols-2 gap-2 text-xs">
            {(
              [
                [
                  "Coordinates",
                  report.latitude != null && report.longitude != null
                    ? `${report.latitude.toFixed(4)}, ${report.longitude.toFixed(4)}`
                    : "—",
                ],
                ["Timezone", report.timezone || "—"],
                ["Network", report.organization || "—"],
                ["ASN", report.asn || "—"],
              ] as const
            ).map(([label, value]) => (
              <div
                key={label}
                className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2"
              >
                <div className="text-[var(--color-muted)]">{label}</div>
                <div className="mt-0.5 break-words font-medium">{value}</div>
              </div>
            ))}
          </div>
          {mapUrl && (
            <button
              onClick={() => void openUrl(mapUrl)}
              className="flex items-center justify-center gap-2 rounded-lg border border-[var(--color-border)] px-3 py-2 text-xs hover:bg-[var(--color-surface)]"
            >
              <ExternalLink size={13} />
              Open approximate location in OpenStreetMap
            </button>
          )}
          <p className="text-[10px] leading-relaxed text-[var(--color-muted)]">
            IP geolocation is approximate and usually identifies an ISP or city, not your
            exact address. The lookup provider receives your public IP; the result is not
            stored by Inspector Rust.
          </p>
        </>
      )}
    </div>
  );
}
