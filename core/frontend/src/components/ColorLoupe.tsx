import { useEffect, useRef, useState } from "react";
import { colorLoupeData, colorLoupePick, colorLoupeCancel } from "../lib/ipc";

/**
 * Custom screen-eyedropper **loupe** (the window labelled `color-loupe`).
 *
 * On open, Rust captures a one-shot snapshot of the cursor's display and hands
 * it here as base64; this overlay covers that display, hides the OS cursor, and
 * draws a magnified circular loupe that follows the mouse with the **live hex
 * shown right under it** (which Apple's NSColorSampler can't do). Because it
 * works off a static snapshot, magnification + colour read happen entirely in
 * the webview (no per-frame screen capture) → smooth.
 *
 * Click picks the pixel under the reticle; Esc cancels. The hex label animates
 * (per-character "heat" flare) as the colour changes while you move.
 */
const LOUPE = 152; // circle diameter (CSS px)
const ZOOM = 11; // magnification
const HEX_CHARS = 7; // "#RRGGBB"

export function ColorLoupe() {
  const [ready, setReady] = useState(false);
  const imgCanvas = useRef<HTMLCanvasElement | null>(null);
  const imgCtx = useRef<CanvasRenderingContext2D | null>(null);
  const scale = useRef(1);
  const mouse = useRef({ x: window.innerWidth / 2, y: window.innerHeight / 2 });
  const curHex = useRef("#000000");
  const loupeWrap = useRef<HTMLDivElement>(null);
  const loupeCanvas = useRef<HTMLCanvasElement>(null);
  const hexCells = useRef<(HTMLSpanElement | null)[]>([]);
  const swatchRef = useRef<HTMLSpanElement>(null);
  const hexBoxRef = useRef<HTMLDivElement>(null);
  const lastHex = useRef("");

  // Fetch the snapshot + load it into an offscreen canvas.
  useEffect(() => {
    let cancelled = false;
    colorLoupeData()
      .then((d) => {
        if (cancelled) return;
        if (!d) {
          void colorLoupeCancel();
          return;
        }
        const img = new Image();
        img.onload = () => {
          if (cancelled) return;
          const cv = document.createElement("canvas");
          cv.width = img.naturalWidth;
          cv.height = img.naturalHeight;
          const c = cv.getContext("2d", { willReadFrequently: true });
          if (!c) return;
          c.drawImage(img, 0, 0);
          imgCanvas.current = cv;
          imgCtx.current = c;
          scale.current = img.naturalWidth / window.innerWidth || 1;
          setReady(true);
        };
        img.src = `data:image/png;base64,${d.b64}`;
      })
      .catch(() => void colorLoupeCancel());
    return () => {
      cancelled = true;
    };
  }, []);

  // Input: track the cursor, pick on click, cancel on Esc.
  useEffect(() => {
    const mm = (e: MouseEvent) => {
      mouse.current = { x: e.clientX, y: e.clientY };
    };
    const down = (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      void colorLoupePick(curHex.current);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        void colorLoupeCancel();
      }
    };
    window.addEventListener("mousemove", mm, true);
    window.addEventListener("mousedown", down, true);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("mousemove", mm, true);
      window.removeEventListener("mousedown", down, true);
      window.removeEventListener("keydown", key, true);
    };
  }, []);

  // Render loop: magnify around the cursor, read the hex, position the loupe.
  useEffect(() => {
    if (!ready) return;
    let raf = 0;
    const draw = () => {
      const ic = imgCtx.current;
      const lc = loupeCanvas.current;
      const wrap = loupeWrap.current;
      if (ic && lc && imgCanvas.current && wrap) {
        const s = scale.current;
        const iw = imgCanvas.current.width;
        const ih = imgCanvas.current.height;
        const ix = Math.max(0, Math.min(iw - 1, Math.round(mouse.current.x * s)));
        const iy = Math.max(0, Math.min(ih - 1, Math.round(mouse.current.y * s)));

        // Read the pixel under the reticle → hex.
        const px = ic.getImageData(ix, iy, 1, 1).data;
        const hex =
          "#" +
          [px[0], px[1], px[2]]
            .map((v) => v.toString(16).padStart(2, "0"))
            .join("")
            .toUpperCase();
        curHex.current = hex;

        // Magnified crop (nearest-neighbour → crisp pixels).
        const lctx = lc.getContext("2d");
        if (lctx) {
          const half = LOUPE / (2 * ZOOM);
          lctx.imageSmoothingEnabled = false;
          lctx.clearRect(0, 0, LOUPE, LOUPE);
          lctx.drawImage(
            imgCanvas.current,
            ix - half,
            iy - half,
            half * 2,
            half * 2,
            0,
            0,
            LOUPE,
            LOUPE,
          );
          // Pixel grid.
          lctx.lineWidth = 1;
          lctx.strokeStyle = "rgba(0,0,0,0.16)";
          lctx.beginPath();
          for (let g = 0; g <= LOUPE; g += ZOOM) {
            lctx.moveTo(g + 0.5, 0);
            lctx.lineTo(g + 0.5, LOUPE);
            lctx.moveTo(0, g + 0.5);
            lctx.lineTo(LOUPE, g + 0.5);
          }
          lctx.stroke();
          // Center reticle (the target pixel) — double outline for contrast.
          const r = Math.floor(LOUPE / 2 / ZOOM) * ZOOM;
          lctx.lineWidth = 2;
          lctx.strokeStyle = "rgba(0,0,0,0.9)";
          lctx.strokeRect(r - 1, r - 1, ZOOM + 2, ZOOM + 2);
          lctx.strokeStyle = "rgba(255,255,255,0.95)";
          lctx.strokeRect(r, r, ZOOM, ZOOM);
        }

        // Position the loupe at the cursor; flip the hex label above near the
        // bottom edge so it's never clipped.
        wrap.style.transform = `translate(${mouse.current.x}px, ${mouse.current.y}px)`;
        const flip = mouse.current.y > window.innerHeight - (LOUPE / 2 + 56);
        if (hexBoxRef.current) {
          hexBoxRef.current.style.top = flip
            ? `${-(LOUPE / 2 + 38)}px`
            : `${LOUPE / 2 + 12}px`;
        }

        // Animated hex label: heat each char that changed.
        if (hex !== lastHex.current) {
          for (let i = 0; i < HEX_CHARS; i++) {
            const cell = hexCells.current[i];
            const ch = hex[i] ?? "";
            if (cell && cell.textContent !== ch) {
              cell.textContent = ch;
              cell.classList.remove("loupe-hot");
              void cell.offsetWidth;
              cell.classList.add("loupe-hot");
            }
          }
          if (swatchRef.current) swatchRef.current.style.backgroundColor = hex;
          lastHex.current = hex;
        }
      }
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  }, [ready]);

  return (
    <div className="loupe-root">
      <div ref={loupeWrap} className="loupe-anchor">
        <div className="loupe-circle">
          <canvas ref={loupeCanvas} width={LOUPE} height={LOUPE} />
        </div>
        <div ref={hexBoxRef} className="loupe-hex">
          <span ref={swatchRef} className="loupe-swatch" />
          <span className="loupe-code">
            {Array.from({ length: HEX_CHARS }).map((_, i) => (
              <span
                key={i}
                className="loupe-digit"
                ref={(el) => {
                  hexCells.current[i] = el;
                }}
              >
                {i === 0 ? "#" : "0"}
              </span>
            ))}
          </span>
        </div>
      </div>
    </div>
  );
}
