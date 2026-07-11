/**
 * `snitch map` — live outbound connections on a zoomable world map (macOS).
 *
 * A dotted equirectangular land basemap (offline, from `worldmask.ts`) with:
 *  - a "home" marker at this machine's own geolocated location,
 *  - a dim dot per remote server (located online via ip-api, cached in Rust),
 *  - for servers whose process is **actively transferring right now**
 *    (per-process bytes/s from `nettop`), a glowing green dot + a curved arc
 *    from home with **packets flowing** along it (animated).
 *
 * Fully zoomable/pannable: scroll to zoom toward the cursor, drag to pan,
 * double-click to zoom in, +/−/reset controls, keyboard (+/−, arrows, 0). The
 * canvas culls off-screen land cells so high zoom stays smooth; the hover
 * info-box is drawn on the canvas (no React re-render per mouse move). Esc exits.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { Loader2, Globe2, Plus, Minus, Maximize2, ArrowUpRight } from "lucide-react";
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

const ACTIVE_BPS = 2000;
const ACTIVE = "#34d399"; // emerald — live data flow
const MIN_ZOOM = 1;
const MAX_ZOOM = 9;

function readColor(el: HTMLElement, varName: string, fallback: string): string {
  return getComputedStyle(el).getPropertyValue(varName).trim() || fallback;
}
function humanRate(bps: number): string {
  if (bps >= 1e6) return `${(bps / 1e6).toFixed(1)} MB/s`;
  if (bps >= 1e3) return `${(bps / 1e3).toFixed(0)} KB/s`;
  return `${bps} B/s`;
}
const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

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
  const [activityMap, setActivityMap] = useState<Map<number, number>>(new Map());
  const [zoomLabel, setZoomLabel] = useState(1);

  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const geoCache = useRef<Map<string, GeoLocation | null>>(new Map());
  const serversRef = useRef<Server[]>([]);
  const activityRef = useRef<Map<number, number>>(new Map());
  const homeRef = useRef<GeoLocation | null>(null);

  // View transform (animated). center = the map fraction (0..1) at the viewport
  // centre; zoom = scale. `cur` is what's drawn, `tgt` is the target the draw
  // loop eases toward — so wheel/buttons feel buttery.
  const zoomCur = useRef(1);
  const zoomTgt = useRef(1);
  const centerCur = useRef({ cx: 0.5, cy: 0.5 });
  const centerTgt = useRef({ cx: 0.5, cy: 0.5 });
  const dragging = useRef(false);
  const lastPtr = useRef({ x: 0, y: 0 });
  const moved = useRef(false);
  // Hover info drawn on-canvas (a ref → zero re-render while moving the mouse).
  const hover = useRef<{ ip: string; sx: number; sy: number } | null>(null);

  const serverBps = useCallback((s: Server): number => {
    let max = 0;
    for (const pid of s.pids) max = Math.max(max, activityRef.current.get(pid) ?? 0);
    return max;
  }, []);

  const clampCenter = useCallback((cx: number, cy: number, zoom: number) => {
    const half = 0.5 / zoom;
    return {
      cx: zoom <= 1 ? 0.5 : clamp(cx, half, 1 - half),
      cy: zoom <= 1 ? 0.5 : clamp(cy, half, 1 - half),
    };
  }, []);

  // ── Data polling (connections 4 s, activity 3 s, home once) ──
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
        setActiveCount(serversRef.current.filter((s) => serverBps(s) >= ACTIVE_BPS).length);
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

  // ── Interaction: wheel (zoom to cursor), drag (pan), dblclick, hover ──
  useEffect(() => {
    const canvas = canvasRef.current;
    const wrap = wrapRef.current;
    if (!canvas || !wrap) return;

    const rectOf = () => canvas.getBoundingClientRect();
    const zoomToward = (sx: number, sy: number, factor: number, w: number, h: number) => {
      const z = zoomTgt.current;
      const c = centerTgt.current;
      const fx = (sx - w / 2) / (w * z) + c.cx;
      const fy = (sy - h / 2) / (h * z) + c.cy;
      const nz = clamp(z * factor, MIN_ZOOM, MAX_ZOOM);
      const ncx = fx - (sx - w / 2) / (w * nz);
      const ncy = fy - (sy - h / 2) / (h * nz);
      zoomTgt.current = nz;
      centerTgt.current = clampCenter(ncx, ncy, nz);
      setZoomLabel(nz);
    };

    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const r = rectOf();
      zoomToward(e.clientX - r.left, e.clientY - r.top, Math.exp(-e.deltaY * 0.0016), r.width, r.height);
    };
    const onDblClick = (e: MouseEvent) => {
      const r = rectOf();
      zoomToward(e.clientX - r.left, e.clientY - r.top, 1.9, r.width, r.height);
    };
    const onPointerDown = (e: PointerEvent) => {
      dragging.current = true;
      moved.current = false;
      lastPtr.current = { x: e.clientX, y: e.clientY };
      canvas.setPointerCapture(e.pointerId);
      canvas.style.cursor = "grabbing";
    };
    const onPointerMove = (e: PointerEvent) => {
      const r = rectOf();
      const sx = e.clientX - r.left;
      const sy = e.clientY - r.top;
      if (dragging.current) {
        const dx = e.clientX - lastPtr.current.x;
        const dy = e.clientY - lastPtr.current.y;
        if (Math.abs(dx) + Math.abs(dy) > 2) moved.current = true;
        lastPtr.current = { x: e.clientX, y: e.clientY };
        const z = zoomTgt.current;
        const nc = clampCenter(
          centerTgt.current.cx - dx / (r.width * z),
          centerTgt.current.cy - dy / (r.height * z),
          z,
        );
        centerTgt.current = nc;
        centerCur.current = nc; // 1:1, no easing lag while dragging
        return;
      }
      // Hover: nearest server dot within ~14 px (screen space).
      const z = zoomCur.current;
      const c = centerCur.current;
      let best: { ip: string; d: number } | null = null;
      for (const s of serversRef.current) {
        const { fx, fy } = project(s.loc.lon, s.loc.lat);
        const x = (fx - c.cx) * r.width * z + r.width / 2;
        const y = (fy - c.cy) * r.height * z + r.height / 2;
        const d = Math.hypot(x - sx, y - sy);
        if (d < 14 && (!best || d < best.d)) best = { ip: s.ip, d };
      }
      hover.current = best ? { ip: best.ip, sx, sy } : null;
      canvas.style.cursor = best ? "pointer" : "grab";
    };
    const onPointerUp = (e: PointerEvent) => {
      dragging.current = false;
      canvas.releasePointerCapture(e.pointerId);
      canvas.style.cursor = "grab";
    };
    const onLeave = () => {
      hover.current = null;
    };

    canvas.addEventListener("wheel", onWheel, { passive: false });
    canvas.addEventListener("dblclick", onDblClick);
    canvas.addEventListener("pointerdown", onPointerDown);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerup", onPointerUp);
    canvas.addEventListener("pointerleave", onLeave);
    canvas.style.cursor = "grab";
    return () => {
      canvas.removeEventListener("wheel", onWheel);
      canvas.removeEventListener("dblclick", onDblClick);
      canvas.removeEventListener("pointerdown", onPointerDown);
      canvas.removeEventListener("pointermove", onPointerMove);
      canvas.removeEventListener("pointerup", onPointerUp);
      canvas.removeEventListener("pointerleave", onLeave);
    };
  }, [clampCenter]);

  // Button / keyboard zoom helpers.
  const zoomBy = useCallback(
    (factor: number) => {
      const nz = clamp(zoomTgt.current * factor, MIN_ZOOM, MAX_ZOOM);
      zoomTgt.current = nz;
      centerTgt.current = clampCenter(centerTgt.current.cx, centerTgt.current.cy, nz);
      setZoomLabel(nz);
    },
    [clampCenter],
  );
  const resetView = useCallback(() => {
    zoomTgt.current = 1;
    centerTgt.current = { cx: 0.5, cy: 0.5 };
    setZoomLabel(1);
  }, []);

  // ── Canvas render loop ──
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
            // Ease view toward target.
            zoomCur.current += (zoomTgt.current - zoomCur.current) * 0.22;
            if (Math.abs(zoomTgt.current - zoomCur.current) < 0.001) zoomCur.current = zoomTgt.current;
            centerCur.current = {
              cx: centerCur.current.cx + (centerTgt.current.cx - centerCur.current.cx) * 0.22,
              cy: centerCur.current.cy + (centerTgt.current.cy - centerCur.current.cy) * 0.22,
            };
            const zoom = zoomCur.current;
            const c = centerCur.current;

            ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
            const accent = readColor(wrap, "--color-accent", "#b3c5ff");
            const muted = readColor(wrap, "--color-muted", "#8a8a99");
            const fg = readColor(wrap, "--color-fg", "#e8e8ee");
            const surface = readColor(wrap, "--color-surface", "#1c1c22");
            ctx.clearRect(0, 0, cssW, cssH);

            const sx = (fx: number) => (fx - c.cx) * cssW * zoom + cssW / 2;
            const sy = (fy: number) => (fy - c.cy) * cssH * zoom + cssH / 2;
            const toXY = (loc: GeoLocation) => {
              const p = project(loc.lon, loc.lat);
              return { x: sx(p.fx), y: sy(p.fy) };
            };

            // Land dots — culled to the visible fractional window.
            const half = 0.5 / zoom;
            const colMin = Math.max(0, Math.floor((c.cx - half) * WORLD_MASK_W) - 1);
            const colMax = Math.min(WORLD_MASK_W, Math.ceil((c.cx + half) * WORLD_MASK_W) + 1);
            const rowMin = Math.max(0, Math.floor((c.cy - half) * WORLD_MASK_H) - 1);
            const rowMax = Math.min(WORLD_MASK_H, Math.ceil((c.cy + half) * WORLD_MASK_H) + 1);
            const cell = (cssW * zoom) / WORLD_MASK_W;
            const r = clamp(cell * 0.42, 0.6, 2.6);
            ctx.fillStyle = muted;
            ctx.globalAlpha = 0.22;
            for (let row = rowMin; row < rowMax; row++) {
              const yy = sy((row + 0.5) / WORLD_MASK_H);
              for (let col = colMin; col < colMax; col++) {
                if (isLand(col, row)) {
                  ctx.beginPath();
                  ctx.arc(sx((col + 0.5) / WORLD_MASK_W), yy, r, 0, Math.PI * 2);
                  ctx.fill();
                }
              }
            }
            ctx.globalAlpha = 1;

            const home = homeRef.current ? toXY(homeRef.current) : null;
            const pulse = 0.5 + 0.5 * Math.sin(t / 20);

            // Active arcs + flowing packets.
            if (home) {
              for (const s of serversRef.current) {
                if (serverBps(s) < ACTIVE_BPS) continue;
                const p = toXY(s.loc);
                const dist = Math.hypot(p.x - home.x, p.y - home.y);
                const cx = (home.x + p.x) / 2;
                const cy = Math.min(home.y, p.y) - dist * 0.22;
                ctx.beginPath();
                ctx.moveTo(home.x, home.y);
                ctx.quadraticCurveTo(cx, cy, p.x, p.y);
                ctx.strokeStyle = ACTIVE;
                ctx.globalAlpha = 0.35;
                ctx.lineWidth = 1;
                ctx.stroke();
                const bez = (u: number) => ({
                  x: (1 - u) * (1 - u) * home.x + 2 * (1 - u) * u * cx + u * u * p.x,
                  y: (1 - u) * (1 - u) * home.y + 2 * (1 - u) * u * cy + u * u * p.y,
                });
                for (let k = 0; k < 2; k++) {
                  const u = (t / 60 + k / 2) % 1;
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

            // Server dots (constant screen size).
            const showLabels = zoom > 2.6;
            for (const s of serversRef.current) {
              const { x, y } = toXY(s.loc);
              if (x < -20 || x > cssW + 20 || y < -20 || y > cssH + 20) continue;
              const active = serverBps(s) >= ACTIVE_BPS;
              const hovered = hover.current?.ip === s.ip;
              const base = 2 + Math.min(4, Math.log2(s.count + 1));
              const color = active ? ACTIVE : accent;
              ctx.beginPath();
              ctx.arc(x, y, base + (active ? 4 + pulse * 4 : 2), 0, Math.PI * 2);
              ctx.fillStyle = color;
              ctx.globalAlpha = active ? 0.22 : 0.12;
              ctx.fill();
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
              if ((showLabels && active) || hovered) {
                const label = s.loc.city || s.loc.country || s.ip;
                ctx.font = "600 10px ui-sans-serif, system-ui, sans-serif";
                ctx.fillStyle = fg;
                ctx.globalAlpha = 0.85;
                ctx.fillText(label, x + base + 4, y + 3);
                ctx.globalAlpha = 1;
              }
            }

            // Home marker.
            if (home && home.x > -20 && home.x < cssW + 20) {
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

            // Hover info-box (drawn on canvas → no re-render).
            const hv = hover.current;
            if (hv) {
              const s = serversRef.current.find((x) => x.ip === hv.ip);
              if (s) {
                const bps = serverBps(s);
                const l1 = `${s.loc.city ? s.loc.city + ", " : ""}${s.loc.country || s.ip}`;
                const l2 = s.loc.isp || s.ip;
                const l3 = `${Array.from(s.apps).join(", ")}${bps >= ACTIVE_BPS ? "  ·  " + humanRate(bps) : ""}`;
                ctx.font = "600 11px ui-sans-serif, system-ui, sans-serif";
                const w = Math.max(ctx.measureText(l1).width, ctx.measureText(l2).width, ctx.measureText(l3).width) + 16;
                const bh = 50;
                let bx = hv.sx + 12;
                let by = hv.sy + 12;
                if (bx + w > cssW) bx = hv.sx - w - 12;
                if (by + bh > cssH) by = hv.sy - bh - 12;
                ctx.fillStyle = surface;
                ctx.globalAlpha = 0.95;
                ctx.beginPath();
                ctx.roundRect(bx, by, w, bh, 6);
                ctx.fill();
                ctx.strokeStyle = muted;
                ctx.globalAlpha = 0.3;
                ctx.stroke();
                ctx.globalAlpha = 1;
                ctx.fillStyle = fg;
                ctx.fillText(l1, bx + 8, by + 16);
                ctx.font = "10px ui-sans-serif, system-ui, sans-serif";
                ctx.fillStyle = muted;
                ctx.fillText(l2, bx + 8, by + 30);
                ctx.fillStyle = bps >= ACTIVE_BPS ? ACTIVE : muted;
                ctx.fillText(l3, bx + 8, by + 44);
              }
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

  // Keyboard: Esc exit, +/− zoom, arrows pan, 0 reset.
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      const panStep = 0.12 / zoomTgt.current;
      switch (e.key) {
        case "Escape":
          onExit();
          break;
        case "+":
        case "=":
          zoomBy(1.4);
          break;
        case "-":
        case "_":
          zoomBy(1 / 1.4);
          break;
        case "0":
          resetView();
          break;
        case "ArrowLeft":
          centerTgt.current = clampCenter(centerTgt.current.cx - panStep, centerTgt.current.cy, zoomTgt.current);
          break;
        case "ArrowRight":
          centerTgt.current = clampCenter(centerTgt.current.cx + panStep, centerTgt.current.cy, zoomTgt.current);
          break;
        case "ArrowUp":
          centerTgt.current = clampCenter(centerTgt.current.cx, centerTgt.current.cy - panStep, zoomTgt.current);
          break;
        case "ArrowDown":
          centerTgt.current = clampCenter(centerTgt.current.cx, centerTgt.current.cy + panStep, zoomTgt.current);
          break;
        default:
          return;
      }
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit, zoomBy, resetView, clampCenter]);

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
        className="relative w-full shrink-0 select-none overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]"
        style={{ aspectRatio: "2 / 1" }}
      >
        <canvas ref={canvasRef} className="absolute inset-0 h-full w-full" />
        {loading && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center text-[var(--color-muted)]">
            <Loader2 size={22} className="animate-spin" />
          </div>
        )}
        {/* Zoom controls */}
        <div className="absolute bottom-2 right-2 flex flex-col gap-1">
          <button
            type="button"
            onClick={() => zoomBy(1.4)}
            className="flex h-7 w-7 items-center justify-center rounded-md border border-[var(--color-border)] bg-[var(--color-bg)]/80 text-[var(--color-fg)] backdrop-blur hover:bg-[var(--color-surface)]"
            title="Zoom in (scroll / +)"
          >
            <Plus size={14} />
          </button>
          <button
            type="button"
            onClick={() => zoomBy(1 / 1.4)}
            className="flex h-7 w-7 items-center justify-center rounded-md border border-[var(--color-border)] bg-[var(--color-bg)]/80 text-[var(--color-fg)] backdrop-blur hover:bg-[var(--color-surface)]"
            title="Zoom out (scroll / −)"
          >
            <Minus size={14} />
          </button>
          <button
            type="button"
            onClick={resetView}
            className="flex h-7 w-7 items-center justify-center rounded-md border border-[var(--color-border)] bg-[var(--color-bg)]/80 text-[var(--color-fg)] backdrop-blur hover:bg-[var(--color-surface)]"
            title="Reset view (0)"
          >
            <Maximize2 size={13} />
          </button>
        </div>
        {zoomLabel > 1.02 && (
          <div className="pointer-events-none absolute bottom-2 left-2 rounded bg-[var(--color-bg)]/70 px-1.5 py-0.5 text-[10px] tabular-nums text-[var(--color-muted)] backdrop-blur">
            {zoomLabel.toFixed(1)}×
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
              className={
                "flex items-center gap-2 rounded border px-2 py-1 text-xs transition-colors " +
                (active
                  ? "border-emerald-500/50 bg-emerald-500/10"
                  : "border-[var(--color-border)] bg-[var(--color-surface)]")
              }
              title={`${s.ip}\n${Array.from(s.apps).join(", ")}`}
            >
              {active && <ArrowUpRight size={13} className="shrink-0 text-emerald-400" />}
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
        Scroll to zoom · drag to pan · green = data flowing now · Esc to exit
      </div>
    </div>
  );
}
