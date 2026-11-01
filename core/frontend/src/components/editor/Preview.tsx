import { useEffect, useRef, useState, type ReactNode } from "react";
import { editorFrame, editorResize, type EditorFrame } from "../../lib/ipc";
import { fitPreview } from "../../lib/editor";

/** The helper renders the preview at most this large; the canvas scales it to the panel. */
const RENDER_MAX = { w: 1280, h: 720 };
const RESIZE_DEBOUNCE_MS = 250;

async function draw(canvas: HTMLCanvasElement, f: EditorFrame) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  if (canvas.width !== f.width) canvas.width = f.width;
  if (canvas.height !== f.height) canvas.height = f.height;
  const img = new ImageData(new Uint8ClampedArray(f.rgba), f.width, f.height);
  if (typeof createImageBitmap === "function") {
    const bmp = await createImageBitmap(img);
    ctx.drawImage(bmp, 0, 0);
    bmp.close();
  } else {
    ctx.putImageData(img, 0, 0);
  }
}

/**
 * Preview canvas: pulls `editor_frame` once per animation frame (never two requests in
 * flight), stops while the window is hidden or `active` is false, and tells the helper
 * the panel size (debounced). `children` cover the whole panel (crop mode, fatal state).
 */
export function Preview({ ratio, active, dim, children }: { ratio: number; active: boolean; dim: boolean; children?: ReactNode }) {
  const box = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [panel, setPanel] = useState({ w: 0, h: 0 });

  useEffect(() => {
    const el = box.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver((entries) => {
      const r = entries[0]?.contentRect;
      if (r) setPanel({ w: r.width, h: r.height });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Render size follows the panel (device pixels), debounced so a window drag sends one.
  const sent = useRef("");
  useEffect(() => {
    if (!panel.w || !panel.h) return;
    const dpr = window.devicePixelRatio || 1;
    const size = fitPreview(panel.w * dpr, panel.h * dpr, ratio, RENDER_MAX.w, RENDER_MAX.h);
    const key = `${size.w}x${size.h}`;
    if (key === sent.current) return;
    const id = window.setTimeout(() => {
      sent.current = key;
      editorResize(size.w, size.h).catch(() => undefined);
    }, RESIZE_DEBOUNCE_MS);
    return () => window.clearTimeout(id);
  }, [panel, ratio]);

  useEffect(() => {
    if (!active) return;
    let dead = false;
    let inflight = false;
    let gen = 0;
    let raf = 0;
    const loop = () => {
      if (dead) return;
      if (!inflight && document.visibilityState === "visible") {
        inflight = true;
        editorFrame(gen)
          .then(async (f) => {
            if (f && !dead && canvas.current) {
              gen = f.gen;
              await draw(canvas.current, f);
            }
          })
          .catch(() => undefined)
          .finally(() => {
            inflight = false;
          });
      }
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => {
      dead = true;
      cancelAnimationFrame(raf);
    };
  }, [active]);

  const disp = fitPreview(panel.w, panel.h, ratio, Infinity, Infinity);
  return (
    <div ref={box} className="relative flex min-h-0 flex-1 items-center justify-center overflow-hidden bg-black/90">
      <div className="relative" style={{ width: disp.w, height: disp.h }}>
        <canvas
          ref={canvas}
          className={"h-full w-full bg-black " + (dim ? "opacity-40 grayscale" : "")}
          data-testid="ed-canvas"
        />
      </div>
      {children}
    </div>
  );
}
