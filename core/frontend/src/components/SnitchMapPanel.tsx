/**
 * `snitch map` — live outbound connections plotted on a world map (macOS).
 * A dotted equirectangular land basemap (offline, from `worldmask.ts`) with a
 * glowing dot per remote server; server locations are resolved online (ip-api,
 * batched + cached in Rust) — private/LAN IPs are never sent out. Below the map
 * a compact list of servers (country · city · ISP · app). Esc exits.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { Loader2, Globe2 } from "lucide-react";
import {
  snitchConnections,
  snitchGeolocate,
  type GeoLocation,
  type SnitchConnection,
} from "../lib/ipc";
import { WORLD_MASK_W, WORLD_MASK_H, isLand, project } from "../lib/worldmask";

interface Server {
  ip: string;
  loc: GeoLocation;
  apps: Set<string>;
  count: number;
}

function readColor(el: HTMLElement, varName: string, fallback: string): string {
  const v = getComputedStyle(el).getPropertyValue(varName).trim();
  return v || fallback;
}

export function SnitchMapPanel({
  focused,
  onExit,
}: {
  focused: boolean;
  onExit: () => void;
}) {
  const [servers, setServers] = useState<Server[]>([]);
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(0);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const geoCache = useRef<Map<string, GeoLocation | null>>(new Map());
  const serversRef = useRef<Server[]>([]);

  // Poll connections, geolocate the new public IPs, rebuild the server set.
  const refresh = useCallback(async () => {
    let conns: SnitchConnection[];
    try {
      conns = await snitchConnections();
    } catch {
      setLoading(false);
      return;
    }
    const unseen = Array.from(
      new Set(conns.map((c) => c.remote_ip).filter((ip) => !geoCache.current.has(ip))),
    );
    if (unseen.length) {
      setPending(unseen.length);
      unseen.forEach((ip) => geoCache.current.set(ip, null)); // mark in-flight (avoids re-request)
      try {
        const locs = await snitchGeolocate(unseen);
        for (const l of locs) geoCache.current.set(l.ip, l);
      } catch {
        /* leave as null; retried next cycle only if evicted */
      }
      setPending(0);
    }
    // Aggregate located servers.
    const byIp = new Map<string, Server>();
    for (const c of conns) {
      const loc = geoCache.current.get(c.remote_ip);
      if (!loc) continue;
      let s = byIp.get(c.remote_ip);
      if (!s) {
        s = { ip: c.remote_ip, loc, apps: new Set(), count: 0 };
        byIp.set(c.remote_ip, s);
      }
      s.apps.add(c.command);
      s.count += 1;
    }
    const list = Array.from(byIp.values()).sort((a, b) => b.count - a.count);
    serversRef.current = list;
    setServers(list);
    setLoading(false);
  }, []);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), 4000);
    return () => window.clearInterval(id);
  }, [refresh]);

  // Canvas render loop (dotted land basemap + pulsing server dots).
  useEffect(() => {
    let raf = 0;
    let t = 0;
    const draw = () => {
      const canvas = canvasRef.current;
      const wrap = wrapRef.current;
      if (canvas && wrap) {
        const dpr = Math.min(window.devicePixelRatio || 1, 2);
        const cssW = wrap.clientWidth;
        const cssH = wrap.clientHeight;
        if (cssW > 0 && cssH > 0) {
          if (canvas.width !== Math.round(cssW * dpr) || canvas.height !== Math.round(cssH * dpr)) {
            canvas.width = Math.round(cssW * dpr);
            canvas.height = Math.round(cssH * dpr);
          }
          const ctx = canvas.getContext("2d");
          if (ctx) {
            ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
            const accent = readColor(wrap, "--color-accent", "#b3c5ff");
            const muted = readColor(wrap, "--color-muted", "#8a8a99");
            ctx.clearRect(0, 0, cssW, cssH);
            // Land dots.
            const stepX = cssW / WORLD_MASK_W;
            const stepY = cssH / WORLD_MASK_H;
            const r = Math.max(0.5, Math.min(stepX, stepY) * 0.42);
            ctx.fillStyle = muted;
            ctx.globalAlpha = 0.28;
            for (let row = 0; row < WORLD_MASK_H; row++) {
              for (let col = 0; col < WORLD_MASK_W; col++) {
                if (isLand(col, row)) {
                  ctx.beginPath();
                  ctx.arc((col + 0.5) * stepX, (row + 0.5) * stepY, r, 0, Math.PI * 2);
                  ctx.fill();
                }
              }
            }
            ctx.globalAlpha = 1;
            // Server dots (pulsing).
            const pulse = 0.5 + 0.5 * Math.sin(t / 22);
            for (const s of serversRef.current) {
              const { fx, fy } = project(s.loc.lon, s.loc.lat);
              const x = fx * cssW;
              const y = fy * cssH;
              const base = 2 + Math.min(4, Math.log2(s.count + 1));
              ctx.beginPath();
              ctx.arc(x, y, base + pulse * 3, 0, Math.PI * 2);
              ctx.fillStyle = accent;
              ctx.globalAlpha = 0.18;
              ctx.fill();
              ctx.beginPath();
              ctx.arc(x, y, base, 0, Math.PI * 2);
              ctx.globalAlpha = 1;
              ctx.fillStyle = accent;
              ctx.fill();
            }
          }
        }
      }
      t += 1;
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  }, []);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit]);

  return (
    <div className="flex h-full flex-col gap-2 overflow-hidden p-3 text-sm">
      <div className="flex items-center gap-2 text-[var(--color-fg)]">
        <Globe2 size={16} className="text-rose-400" />
        <span className="font-semibold">Connections map</span>
        <span className="ml-auto text-xs text-[var(--color-muted)]">
          {servers.length} server{servers.length === 1 ? "" : "s"}
          {pending > 0 ? ` · locating ${pending}…` : ""}
        </span>
      </div>

      <div
        ref={wrapRef}
        className="relative w-full shrink-0 overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]"
        style={{ aspectRatio: "2 / 1" }}
      >
        <canvas ref={canvasRef} className="absolute inset-0 h-full w-full" />
        {loading && (
          <div className="absolute inset-0 flex items-center justify-center text-[var(--color-muted)]">
            <Loader2 size={22} className="animate-spin" />
          </div>
        )}
      </div>

      <div className="min-h-0 flex-1 space-y-1 overflow-y-auto pr-1">
        {servers.map((s) => (
          <div
            key={s.ip}
            className="flex items-center gap-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-xs"
            title={`${s.ip}\n${Array.from(s.apps).join(", ")}`}
          >
            <span className="min-w-0 flex-1 truncate text-[var(--color-fg)]">
              {s.loc.city ? `${s.loc.city}, ` : ""}
              {s.loc.country || s.ip}
            </span>
            <span className="min-w-0 max-w-[40%] truncate text-[var(--color-muted)]">
              {s.loc.isp}
            </span>
            <span className="shrink-0 tabular-nums text-[var(--color-muted)]">
              {Array.from(s.apps)[0]}
              {s.apps.size > 1 ? ` +${s.apps.size - 1}` : ""}
            </span>
          </div>
        ))}
        {!loading && servers.length === 0 && (
          <div className="pt-4 text-center text-[var(--color-muted)]">
            No located connections yet.
          </div>
        )}
      </div>
      <div className="text-center text-[11px] text-[var(--color-muted)]">
        Locations via ip-api.com (public IPs only) · Esc to exit
      </div>
    </div>
  );
}
