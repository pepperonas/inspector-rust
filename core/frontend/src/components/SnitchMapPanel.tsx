/**
 * `snitch map` — live outbound connections on a world map (macOS).
 *
 * A dotted equirectangular land basemap (offline, from `worldmask.ts`) with:
 *  - a "home" marker at this machine's own geolocated location,
 *  - a dim dot per remote server (located online via ip-api, cached in Rust),
 *  - for servers whose process is **actively transferring right now**
 *    (per-process bytes/s from `nettop`), a glowing green dot + a curved arc
 *    from home with **packets flowing** along it (animated), so you can see at
 *    a glance where data is going this second.
 *
 * Below the map, a server list (active first) with live throughput. Esc exits.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { Loader2, Globe2, ArrowUpRight } from "lucide-react";
import {
  snitchActivity,
  snitchConnections,
  snitchGeolocate,
  snitchHome,
  type GeoLocation,
  type SnitchConnection,
} from "../lib/ipc";
import { WORLD_MASK_W, WORLD_MASK_H, isLand, project } from "../lib/worldmask";

interface Server {
  ip: string;
  loc: GeoLocation;
  apps: Set<string>;
  pids: Set<number>;
  count: number;
}

/** A server is "active" above this throughput (ignore keepalives/ACKs). */
const ACTIVE_BPS = 2000;
const ACTIVE = "#34d399"; // emerald — live data flow

function readColor(el: HTMLElement, varName: string, fallback: string): string {
  return getComputedStyle(el).getPropertyValue(varName).trim() || fallback;
}

function humanRate(bps: number): string {
  if (bps >= 1e6) return `${(bps / 1e6).toFixed(1)} MB/s`;
  if (bps >= 1e3) return `${(bps / 1e3).toFixed(0)} KB/s`;
  return `${bps} B/s`;
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
  const [activeCount, setActiveCount] = useState(0);
  // Render-safe copy of the activity map (the canvas loop reads the ref; the
  // list rows can't read a ref during render, so they read this state).
  const [activityMap, setActivityMap] = useState<Map<number, number>>(new Map());
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const geoCache = useRef<Map<string, GeoLocation | null>>(new Map());
  const serversRef = useRef<Server[]>([]);
  const activityRef = useRef<Map<number, number>>(new Map()); // pid → bytes/s
  const homeRef = useRef<GeoLocation | null>(null);
  const hoverRef = useRef<string | null>(null);

  // Per-server current throughput (max over its processes) + active flag.
  const serverBps = useCallback((s: Server): number => {
    let max = 0;
    for (const pid of s.pids) max = Math.max(max, activityRef.current.get(pid) ?? 0);
    return max;
  }, []);

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
      unseen.forEach((ip) => geoCache.current.set(ip, null));
      try {
        const locs = await snitchGeolocate(unseen);
        for (const l of locs) geoCache.current.set(l.ip, l);
      } catch {
        /* leave null */
      }
      setPending(0);
    }
    const byIp = new Map<string, Server>();
    for (const c of conns) {
      const loc = geoCache.current.get(c.remote_ip);
      if (!loc) continue;
      let s = byIp.get(c.remote_ip);
      if (!s) {
        s = { ip: c.remote_ip, loc, apps: new Set(), pids: new Set(), count: 0 };
        byIp.set(c.remote_ip, s);
      }
      s.apps.add(c.command);
      s.pids.add(c.pid);
      s.count += 1;
    }
    const list = Array.from(byIp.values()).sort((a, b) => {
      const ab = serverBps(a) >= ACTIVE_BPS ? 1 : 0;
      const bb = serverBps(b) >= ACTIVE_BPS ? 1 : 0;
      return bb - ab || b.count - a.count;
    });
    serversRef.current = list;
    setServers(list);
    setActiveCount(list.filter((s) => serverBps(s) >= ACTIVE_BPS).length);
    setLoading(false);
  }, [serverBps]);

  // Connections + geo (4 s), activity (3 s, nettop ~1 s), home (once).
  useEffect(() => {
    void snitchHome().then((h) => (homeRef.current = h)).catch(() => undefined);
    void refresh();
    const idC = window.setInterval(() => void refresh(), 4000);
    const pollAct = async () => {
      try {
        const acts = await snitchActivity();
        const m = new Map<number, number>();
        for (const a of acts) m.set(a.pid, a.bytes_per_sec);
        activityRef.current = m;
        setActivityMap(m);
        setActiveCount(
          serversRef.current.filter((s) => serverBps(s) >= ACTIVE_BPS).length,
        );
      } catch {
        /* keep last */
      }
    };
    void pollAct();
    const idA = window.setInterval(() => void pollAct(), 3000);
    return () => {
      window.clearInterval(idC);
      window.clearInterval(idA);
    };
  }, [refresh, serverBps]);

  // Canvas render loop.
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
            ctx.globalAlpha = 0.22;
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

            const toXY = (loc: GeoLocation) => {
              const { fx, fy } = project(loc.lon, loc.lat);
              return { x: fx * cssW, y: fy * cssH };
            };
            const home = homeRef.current ? toXY(homeRef.current) : null;
            const pulse = 0.5 + 0.5 * Math.sin(t / 20);

            // Arcs + flowing packets for ACTIVE servers only (keeps it calm).
            if (home) {
              for (const s of serversRef.current) {
                if (serverBps(s) < ACTIVE_BPS) continue;
                const p = toXY(s.loc);
                const dist = Math.hypot(p.x - home.x, p.y - home.y);
                const cx = (home.x + p.x) / 2;
                const cy = Math.min(home.y, p.y) - dist * 0.22; // lift upward
                // arc
                ctx.beginPath();
                ctx.moveTo(home.x, home.y);
                ctx.quadraticCurveTo(cx, cy, p.x, p.y);
                ctx.strokeStyle = ACTIVE;
                ctx.globalAlpha = 0.35;
                ctx.lineWidth = 1;
                ctx.stroke();
                // flowing packets (home → server)
                const bez = (u: number) => ({
                  x: (1 - u) * (1 - u) * home.x + 2 * (1 - u) * u * cx + u * u * p.x,
                  y: (1 - u) * (1 - u) * home.y + 2 * (1 - u) * u * cy + u * u * p.y,
                });
                for (let k = 0; k < 2; k++) {
                  const u = ((t / 60 + k / 2) % 1);
                  const pt = bez(u);
                  ctx.beginPath();
                  ctx.arc(pt.x, pt.y, 1.8, 0, Math.PI * 2);
                  ctx.fillStyle = ACTIVE;
                  ctx.globalAlpha = 0.9 * (1 - u * 0.5);
                  ctx.fill();
                }
                ctx.globalAlpha = 1;
              }
            }

            // Server dots.
            for (const s of serversRef.current) {
              const { x, y } = toXY(s.loc);
              const bps = serverBps(s);
              const active = bps >= ACTIVE_BPS;
              const hovered = hoverRef.current === s.ip;
              const base = 2 + Math.min(4, Math.log2(s.count + 1));
              const color = active ? ACTIVE : accent;
              // halo
              ctx.beginPath();
              ctx.arc(x, y, base + (active ? 4 + pulse * 4 : 2), 0, Math.PI * 2);
              ctx.fillStyle = color;
              ctx.globalAlpha = active ? 0.22 : 0.12;
              ctx.fill();
              // core
              ctx.beginPath();
              ctx.arc(x, y, base, 0, Math.PI * 2);
              ctx.globalAlpha = active ? 1 : 0.55;
              ctx.fillStyle = color;
              ctx.fill();
              if (hovered) {
                ctx.beginPath();
                ctx.arc(x, y, base + 6, 0, Math.PI * 2);
                ctx.strokeStyle = color;
                ctx.globalAlpha = 1;
                ctx.lineWidth = 1.5;
                ctx.stroke();
              }
              ctx.globalAlpha = 1;
            }

            // Home marker.
            if (home) {
              ctx.beginPath();
              ctx.arc(home.x, home.y, 4 + pulse * 2, 0, Math.PI * 2);
              ctx.strokeStyle = accent;
              ctx.globalAlpha = 0.6;
              ctx.lineWidth = 1.5;
              ctx.stroke();
              ctx.beginPath();
              ctx.arc(home.x, home.y, 2.5, 0, Math.PI * 2);
              ctx.fillStyle = accent;
              ctx.globalAlpha = 1;
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
  }, [serverBps]);

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
        <span className="ml-auto flex items-center gap-2 text-xs text-[var(--color-muted)]">
          {activeCount > 0 && (
            <span className="flex items-center gap-1 font-semibold text-emerald-400">
              <span className="inline-block h-2 w-2 animate-pulse rounded-full bg-emerald-400" />
              {activeCount} active
            </span>
          )}
          <span>
            {servers.length} server{servers.length === 1 ? "" : "s"}
            {pending > 0 ? ` · locating ${pending}…` : ""}
          </span>
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
        {servers.map((s) => {
          let bps = 0;
          for (const pid of s.pids) bps = Math.max(bps, activityMap.get(pid) ?? 0);
          const active = bps >= ACTIVE_BPS;
          return (
            <div
              key={s.ip}
              onMouseEnter={() => (hoverRef.current = s.ip)}
              onMouseLeave={() => (hoverRef.current = null)}
              className={
                "flex items-center gap-2 rounded border px-2 py-1 text-xs transition-colors " +
                (active
                  ? "border-emerald-500/50 bg-emerald-500/10"
                  : "border-[var(--color-border)] bg-[var(--color-surface)]")
              }
              title={`${s.ip}\n${Array.from(s.apps).join(", ")}`}
            >
              {active && (
                <ArrowUpRight size={13} className="shrink-0 text-emerald-400" />
              )}
              <span className="min-w-0 flex-1 truncate text-[var(--color-fg)]">
                {s.loc.city ? `${s.loc.city}, ` : ""}
                {s.loc.country || s.ip}
              </span>
              {active ? (
                <span className="shrink-0 font-semibold tabular-nums text-emerald-400">
                  {humanRate(bps)}
                </span>
              ) : (
                <span className="min-w-0 max-w-[38%] truncate text-[var(--color-muted)]">
                  {s.loc.isp}
                </span>
              )}
              <span className="shrink-0 tabular-nums text-[var(--color-muted)]">
                {Array.from(s.apps)[0]}
                {s.apps.size > 1 ? ` +${s.apps.size - 1}` : ""}
              </span>
            </div>
          );
        })}
        {!loading && servers.length === 0 && (
          <div className="pt-4 text-center text-[var(--color-muted)]">
            No located connections yet.
          </div>
        )}
      </div>
      <div className="text-center text-[11px] text-[var(--color-muted)]">
        Green = data flowing now · locations via ip-api.com (public IPs only) · Esc to exit
      </div>
    </div>
  );
}
