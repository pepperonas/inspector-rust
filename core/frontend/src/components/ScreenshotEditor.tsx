import { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  ArrowUpRight,
  Check,
  Droplets,
  Highlighter,
  Redo2,
  Square,
  Type,
  Undo2,
  X,
} from "lucide-react";
import {
  editorCancel,
  editorSave,
  getPendingScreenshotInfo,
} from "../lib/ipc";

/**
 * Screenshot annotation editor (mounted in the `screenshot-editor`
 * Tauri window). Loads the currently-pending screenshot, renders it
 * onto a canvas, and lets the user layer five annotation types on top:
 *
 *   • Arrow      — line + filled arrowhead.
 *   • Text       — click position, type, Enter commits.
 *   • Rectangle  — empty-outline box.
 *   • Highlight  — translucent yellow box (CleanShot-style marker).
 *   • Blur       — pixelate the underlying pixels (mosaic, no deps).
 *
 * Save bakes the canvas to PNG and ships it to the backend, which
 * writes it to ~/Downloads with the captured app-name + `-edited`
 * suffix, pushes to clipboard + history, and re-shows the preview.
 *
 * Hotkeys (all macOS, also work on Windows/Linux via Ctrl):
 *   ⌘Z / Ctrl+Z         — Undo.
 *   ⌘⇧Z / Ctrl+Shift+Z  — Redo.
 *   ⌘S / Ctrl+S         — Save.
 *   Esc                 — Cancel (close without saving).
 *
 * The canvas is sized to the screenshot's *natural* pixel dimensions
 * so the saved PNG is full-resolution. CSS scales it down to fit the
 * viewport. Mouse coords are converted via `canvas.width / rect.width`.
 */

type Tool = "arrow" | "text" | "rect" | "highlight" | "blur";

type AnnotationCommon = { color: string; width: number };
type ArrowAnnotation = AnnotationCommon & {
  type: "arrow";
  x1: number;
  y1: number;
  x2: number;
  y2: number;
};
type RectAnnotation = AnnotationCommon & {
  type: "rect";
  x: number;
  y: number;
  w: number;
  h: number;
};
type HighlightAnnotation = {
  type: "highlight";
  x: number;
  y: number;
  w: number;
  h: number;
  color: string;
};
type BlurAnnotation = {
  type: "blur";
  x: number;
  y: number;
  w: number;
  h: number;
  blockSize: number;
};
type TextAnnotation = {
  type: "text";
  x: number;
  y: number;
  text: string;
  color: string;
  size: number;
};
type Annotation =
  | ArrowAnnotation
  | RectAnnotation
  | HighlightAnnotation
  | BlurAnnotation
  | TextAnnotation;

const COLOR_PRESETS = ["#ef4444", "#facc15", "#ffffff", "#000000"] as const;

export function ScreenshotEditor() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  /** The decoded screenshot image. Kept in a ref because we need it
   *  for every redraw (background) and for blur (sampling source
   *  pixels). State would re-create the Image on every render. */
  const imgRef = useRef<HTMLImageElement | null>(null);

  const [imgReady, setImgReady] = useState(false);
  const [tool, setTool] = useState<Tool>("arrow");
  const [color, setColor] = useState<string>(COLOR_PRESETS[0]);
  const [strokeWidth, setStrokeWidth] = useState<number>(4);
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
  /** Annotations popped by Undo, available for Redo. Cleared on any
   *  new annotation (standard undo-stack semantics). */
  const [redoStack, setRedoStack] = useState<Annotation[]>([]);

  /** In-progress drag (mousedown..mouseup). When non-null, the
   *  redraw loop also paints a *preview* of what the drag will
   *  commit to, so the user gets live feedback. */
  const [dragStart, setDragStart] = useState<{ x: number; y: number } | null>(
    null,
  );
  const [dragCurrent, setDragCurrent] = useState<{
    x: number;
    y: number;
  } | null>(null);

  /** Inline text-input state. Only non-null while the user is typing.
   *  Position is in canvas coords; the input is rendered absolutely
   *  on top of the canvas, scaled by the same factor as the canvas. */
  const [textInput, setTextInput] = useState<{
    x: number;
    y: number;
    value: string;
  } | null>(null);

  const [saving, setSaving] = useState(false);

  // ── Load the pending screenshot on mount ─────────────────────────
  const loadImage = useCallback(async () => {
    const info = await getPendingScreenshotInfo().catch(() => null);
    if (!info) return;
    const img = new Image();
    img.onload = () => {
      imgRef.current = img;
      const canvas = canvasRef.current;
      if (canvas) {
        canvas.width = img.naturalWidth;
        canvas.height = img.naturalHeight;
      }
      setImgReady(true);
    };
    img.src = convertFileSrc(info.path);
  }, []);

  useEffect(() => {
    void loadImage();
    let unlisten: UnlistenFn | undefined;
    void listen("editor-screenshot-changed", () => {
      setAnnotations([]);
      setRedoStack([]);
      void loadImage();
    }).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  }, [loadImage]);

  // ── Redraw on every state change ─────────────────────────────────
  useEffect(() => {
    if (!imgReady) return;
    const canvas = canvasRef.current;
    const img = imgRef.current;
    if (!canvas || !img) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(img, 0, 0);

    for (const a of annotations) {
      drawAnnotation(ctx, a, img);
    }

    // Preview of an in-progress drag (arrow / rect / highlight / blur).
    if (dragStart && dragCurrent && tool !== "text") {
      const preview = makeDragAnnotation(
        tool,
        dragStart,
        dragCurrent,
        color,
        strokeWidth,
      );
      if (preview) drawAnnotation(ctx, preview, img);
    }
  }, [annotations, dragStart, dragCurrent, tool, color, strokeWidth, imgReady]);

  // ── Undo / Redo / Save / Cancel ──────────────────────────────────
  const undo = useCallback(() => {
    setAnnotations((cur) => {
      if (cur.length === 0) return cur;
      const next = cur.slice(0, -1);
      setRedoStack((r) => [...r, cur[cur.length - 1]]);
      return next;
    });
  }, []);
  const redo = useCallback(() => {
    setRedoStack((cur) => {
      if (cur.length === 0) return cur;
      const last = cur[cur.length - 1];
      setAnnotations((a) => [...a, last]);
      return cur.slice(0, -1);
    });
  }, []);
  const save = useCallback(async () => {
    const canvas = canvasRef.current;
    if (!canvas || saving) return;
    setSaving(true);
    try {
      const dataUrl = canvas.toDataURL("image/png");
      await editorSave(dataUrl);
    } catch (e) {
      console.error("editor save failed", e);
    } finally {
      setSaving(false);
    }
  }, [saving]);
  const cancel = useCallback(() => {
    void editorCancel().catch(() => undefined);
  }, []);

  // ── Hotkeys ──────────────────────────────────────────────────────
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Don't intercept while the user is typing into the text-input
      // overlay — typing Z there shouldn't undo.
      if (textInput) return;
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase() === "z" && !e.shiftKey) {
        e.preventDefault();
        undo();
      } else if (mod && e.key.toLowerCase() === "z" && e.shiftKey) {
        e.preventDefault();
        redo();
      } else if (mod && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void save();
      } else if (e.key === "Escape") {
        e.preventDefault();
        cancel();
      } else if (!mod && e.key.length === 1) {
        // Single-key tool shortcuts. Match CleanShot-X reasonably.
        const key = e.key.toLowerCase();
        if (key === "a") setTool("arrow");
        else if (key === "t") setTool("text");
        else if (key === "r") setTool("rect");
        else if (key === "h") setTool("highlight");
        else if (key === "b") setTool("blur");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [undo, redo, save, cancel, textInput]);

  // ── Mouse helpers ────────────────────────────────────────────────
  const toCanvasCoords = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current;
    if (!canvas) return { x: 0, y: 0 };
    const rect = canvas.getBoundingClientRect();
    const sx = canvas.width / rect.width;
    const sy = canvas.height / rect.height;
    return {
      x: (e.clientX - rect.left) * sx,
      y: (e.clientY - rect.top) * sy,
    };
  };

  const onCanvasMouseDown = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (textInput) return; // text input is open — clicks dismiss the editor focus, ignore
    const p = toCanvasCoords(e);
    if (tool === "text") {
      // Click-to-place text input. Existing text input (if any) is
      // committed/dropped by its own blur handler.
      setTextInput({ x: p.x, y: p.y, value: "" });
      return;
    }
    setDragStart(p);
    setDragCurrent(p);
  };
  const onCanvasMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!dragStart) return;
    setDragCurrent(toCanvasCoords(e));
  };
  const onCanvasMouseUp = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!dragStart) return;
    const end = toCanvasCoords(e);
    const a = makeDragAnnotation(tool, dragStart, end, color, strokeWidth);
    if (a) {
      setAnnotations((cur) => [...cur, a]);
      setRedoStack([]); // any new annotation invalidates redo
    }
    setDragStart(null);
    setDragCurrent(null);
  };

  const commitTextInput = () => {
    if (!textInput) return;
    const v = textInput.value.trim();
    if (v.length > 0) {
      setAnnotations((cur) => [
        ...cur,
        {
          type: "text",
          x: textInput.x,
          y: textInput.y,
          text: v,
          color,
          size: Math.max(14, strokeWidth * 4),
        },
      ]);
      setRedoStack([]);
    }
    setTextInput(null);
  };

  // ── Render ────────────────────────────────────────────────────────
  return (
    <div className="flex h-screen w-screen flex-col bg-[var(--color-bg)] text-[var(--color-fg)]">
      {/* Top bar */}
      <div className="flex shrink-0 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
        <span className="text-[12px] font-semibold text-[var(--color-muted)]">
          Edit screenshot
        </span>
        <div className="flex items-center gap-1">
          <IconButton
            onClick={undo}
            disabled={annotations.length === 0}
            title="Undo (⌘Z)"
          >
            <Undo2 size={14} />
          </IconButton>
          <IconButton
            onClick={redo}
            disabled={redoStack.length === 0}
            title="Redo (⌘⇧Z)"
          >
            <Redo2 size={14} />
          </IconButton>
          <div className="mx-2 h-5 w-px bg-[var(--color-border)]" />
          <button
            onClick={cancel}
            className="rounded-md border border-[var(--color-border)] px-3 py-1 text-[12px] hover:bg-[var(--color-bg)]"
          >
            <span className="flex items-center gap-1">
              <X size={12} /> Cancel
            </span>
          </button>
          <button
            onClick={save}
            disabled={saving}
            className="rounded-md bg-[var(--color-accent)] px-3 py-1 text-[12px] font-medium text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-50"
          >
            <span className="flex items-center gap-1">
              <Check size={12} /> {saving ? "Saving…" : "Save (⌘S)"}
            </span>
          </button>
        </div>
      </div>

      {/* Body: tool palette + canvas. */}
      <div className="flex min-h-0 flex-1">
        <Toolbar
          tool={tool}
          setTool={setTool}
          color={color}
          setColor={setColor}
          strokeWidth={strokeWidth}
          setStrokeWidth={setStrokeWidth}
        />
        <div className="relative flex min-h-0 flex-1 items-center justify-center overflow-auto bg-[#0f0f0f] p-4">
          {imgReady ? (
            <div className="relative">
              <canvas
                ref={canvasRef}
                onMouseDown={onCanvasMouseDown}
                onMouseMove={onCanvasMouseMove}
                onMouseUp={onCanvasMouseUp}
                onMouseLeave={(e) => {
                  // Commit the drag if the cursor leaves the canvas
                  // mid-stroke — feels less buggy than abandoning it.
                  if (dragStart) onCanvasMouseUp(e);
                }}
                className="max-h-[78vh] max-w-full cursor-crosshair shadow-2xl"
                style={{
                  cursor:
                    tool === "text" ? "text" : tool === "blur" ? "cell" : "crosshair",
                }}
              />
              {textInput && (
                <TextInputOverlay
                  canvas={canvasRef.current}
                  input={textInput}
                  color={color}
                  fontSize={Math.max(14, strokeWidth * 4)}
                  onChange={(v) =>
                    setTextInput((cur) => (cur ? { ...cur, value: v } : cur))
                  }
                  onCommit={commitTextInput}
                  onCancel={() => setTextInput(null)}
                />
              )}
            </div>
          ) : (
            <span className="text-[12px] text-[var(--color-muted)]">
              Loading screenshot…
            </span>
          )}
        </div>
      </div>
    </div>
  );
}

// ── Toolbar component ───────────────────────────────────────────────

function Toolbar({
  tool,
  setTool,
  color,
  setColor,
  strokeWidth,
  setStrokeWidth,
}: {
  tool: Tool;
  setTool: (t: Tool) => void;
  color: string;
  setColor: (c: string) => void;
  strokeWidth: number;
  setStrokeWidth: (n: number) => void;
}) {
  const Btn = ({
    t,
    icon,
    label,
    shortcut,
  }: {
    t: Tool;
    icon: React.ReactNode;
    label: string;
    shortcut: string;
  }) => (
    <button
      onClick={() => setTool(t)}
      title={`${label} (${shortcut})`}
      className={
        "flex h-10 w-10 items-center justify-center rounded-md border transition-colors " +
        (tool === t
          ? "border-[var(--color-accent)] bg-[var(--color-accent)]/15 text-[var(--color-accent)]"
          : "border-[var(--color-border)] hover:bg-[var(--color-bg)]")
      }
    >
      {icon}
    </button>
  );

  return (
    <div className="flex w-14 shrink-0 flex-col items-center gap-1.5 border-r border-[var(--color-border)] bg-[var(--color-surface)] p-2">
      <Btn t="arrow" icon={<ArrowUpRight size={16} />} label="Arrow" shortcut="A" />
      <Btn t="text" icon={<Type size={16} />} label="Text" shortcut="T" />
      <Btn t="rect" icon={<Square size={16} />} label="Rectangle" shortcut="R" />
      <Btn
        t="highlight"
        icon={<Highlighter size={16} />}
        label="Highlight"
        shortcut="H"
      />
      <Btn t="blur" icon={<Droplets size={16} />} label="Blur" shortcut="B" />

      <div className="mt-3 flex flex-col items-center gap-1.5">
        {COLOR_PRESETS.map((c) => (
          <button
            key={c}
            onClick={() => setColor(c)}
            title={c}
            className={
              "h-6 w-6 rounded-full border-2 " +
              (color === c
                ? "border-[var(--color-accent)] scale-110"
                : "border-[var(--color-border)]")
            }
            style={{ backgroundColor: c }}
          />
        ))}
      </div>

      <div className="mt-3 flex flex-col items-center gap-1">
        <span className="text-[9px] uppercase tracking-wider text-[var(--color-muted)]">
          Size
        </span>
        <input
          type="range"
          min={2}
          max={16}
          value={strokeWidth}
          onChange={(e) => setStrokeWidth(parseInt(e.target.value, 10))}
          className="h-1 w-10 rotate-90 cursor-pointer accent-[var(--color-accent)]"
          style={{ marginTop: 16 }}
        />
        <span className="mt-7 text-[10px] tabular-nums text-[var(--color-muted)]">
          {strokeWidth}px
        </span>
      </div>
    </div>
  );
}

function IconButton({
  onClick,
  disabled,
  title,
  children,
}: {
  onClick: () => void;
  disabled?: boolean;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      className="flex h-7 w-7 items-center justify-center rounded-md border border-[var(--color-border)] text-[var(--color-fg)] hover:bg-[var(--color-bg)] disabled:opacity-40"
    >
      {children}
    </button>
  );
}

// ── Inline text input overlay ──────────────────────────────────────

function TextInputOverlay({
  canvas,
  input,
  color,
  fontSize,
  onChange,
  onCommit,
  onCancel,
}: {
  canvas: HTMLCanvasElement | null;
  input: { x: number; y: number; value: string };
  color: string;
  fontSize: number;
  onChange: (v: string) => void;
  onCommit: () => void;
  onCancel: () => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    inputRef.current?.focus();
  }, []);
  if (!canvas) return null;
  const rect = canvas.getBoundingClientRect();
  const cssScaleX = rect.width / canvas.width;
  const cssScaleY = rect.height / canvas.height;
  const left = input.x * cssScaleX;
  // Center the input vertically on the click point, like the bake-out
  // text-y position (which uses fillText baseline=middle).
  const top = input.y * cssScaleY - (fontSize * cssScaleY) / 2;
  return (
    <input
      ref={inputRef}
      type="text"
      value={input.value}
      onChange={(e) => onChange(e.target.value)}
      onBlur={onCommit}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          onCommit();
        } else if (e.key === "Escape") {
          e.preventDefault();
          onCancel();
        }
        // Swallow keystrokes so the global ⌘Z / ⌘S hotkeys don't fire
        // while typing into the overlay.
        e.stopPropagation();
      }}
      style={{
        position: "absolute",
        left,
        top,
        fontSize: fontSize * cssScaleY,
        color,
        background: "rgba(0,0,0,0.4)",
        border: "1px dashed rgba(255,255,255,0.5)",
        padding: "2px 4px",
        outline: "none",
        minWidth: 80,
        fontFamily: "var(--font-sans, sans-serif)",
        fontWeight: 600,
      }}
      placeholder="Type & Enter"
    />
  );
}

// ── Drawing helpers ────────────────────────────────────────────────

function makeDragAnnotation(
  tool: Tool,
  start: { x: number; y: number },
  end: { x: number; y: number },
  color: string,
  width: number,
): Annotation | null {
  if (tool === "arrow") {
    return { type: "arrow", x1: start.x, y1: start.y, x2: end.x, y2: end.y, color, width };
  }
  if (tool === "rect") {
    return {
      type: "rect",
      x: Math.min(start.x, end.x),
      y: Math.min(start.y, end.y),
      w: Math.abs(end.x - start.x),
      h: Math.abs(end.y - start.y),
      color,
      width,
    };
  }
  if (tool === "highlight") {
    return {
      type: "highlight",
      x: Math.min(start.x, end.x),
      y: Math.min(start.y, end.y),
      w: Math.abs(end.x - start.x),
      h: Math.abs(end.y - start.y),
      color: "#facc15", // highlight is always yellow — like a marker
    };
  }
  if (tool === "blur") {
    // Block size scales with stroke width — fatter "brush" → coarser
    // mosaic. Min 6px so it actually looks blurred.
    const block = Math.max(6, width * 3);
    return {
      type: "blur",
      x: Math.min(start.x, end.x),
      y: Math.min(start.y, end.y),
      w: Math.abs(end.x - start.x),
      h: Math.abs(end.y - start.y),
      blockSize: block,
    };
  }
  return null;
}

function drawAnnotation(
  ctx: CanvasRenderingContext2D,
  a: Annotation,
  source: HTMLImageElement,
) {
  switch (a.type) {
    case "arrow":
      drawArrow(ctx, a);
      break;
    case "rect":
      ctx.strokeStyle = a.color;
      ctx.lineWidth = a.width;
      ctx.strokeRect(a.x, a.y, a.w, a.h);
      break;
    case "highlight":
      ctx.save();
      ctx.fillStyle = a.color;
      ctx.globalAlpha = 0.35;
      ctx.fillRect(a.x, a.y, a.w, a.h);
      ctx.restore();
      break;
    case "blur":
      drawBlur(ctx, a, source);
      break;
    case "text":
      ctx.save();
      ctx.fillStyle = a.color;
      ctx.font = `bold ${a.size}px var(--font-sans, sans-serif)`;
      ctx.textBaseline = "middle";
      ctx.fillText(a.text, a.x, a.y);
      ctx.restore();
      break;
  }
}

/** Line + filled arrowhead at (x2, y2). Arrowhead size scales with
 *  stroke width (capped) so a thick arrow has a visible head. */
function drawArrow(ctx: CanvasRenderingContext2D, a: ArrowAnnotation) {
  const dx = a.x2 - a.x1;
  const dy = a.y2 - a.y1;
  const len = Math.hypot(dx, dy);
  if (len < 1) return;
  const headLen = Math.min(24, Math.max(10, a.width * 3));
  const angle = Math.atan2(dy, dx);

  ctx.save();
  ctx.strokeStyle = a.color;
  ctx.fillStyle = a.color;
  ctx.lineWidth = a.width;
  ctx.lineCap = "round";

  // Shaft — stop short of the arrowhead tip so the line doesn't
  // overshoot the filled triangle.
  ctx.beginPath();
  ctx.moveTo(a.x1, a.y1);
  ctx.lineTo(a.x2 - Math.cos(angle) * headLen * 0.5, a.y2 - Math.sin(angle) * headLen * 0.5);
  ctx.stroke();

  // Filled triangle head.
  ctx.beginPath();
  ctx.moveTo(a.x2, a.y2);
  ctx.lineTo(
    a.x2 - headLen * Math.cos(angle - Math.PI / 7),
    a.y2 - headLen * Math.sin(angle - Math.PI / 7),
  );
  ctx.lineTo(
    a.x2 - headLen * Math.cos(angle + Math.PI / 7),
    a.y2 - headLen * Math.sin(angle + Math.PI / 7),
  );
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

/** Mosaic-style pixelation. We sample the *source image* (not the
 *  canvas) so blur is non-destructive: undoing the blur restores the
 *  pixels as they were in the original screenshot, not over whatever
 *  annotations happened to be drawn there. */
function drawBlur(
  ctx: CanvasRenderingContext2D,
  a: BlurAnnotation,
  source: HTMLImageElement,
) {
  const x = Math.round(a.x);
  const y = Math.round(a.y);
  const w = Math.round(a.w);
  const h = Math.round(a.h);
  if (w < 1 || h < 1) return;

  // Off-screen canvas to read pixel data from the source image.
  // Doing this every redraw is wasteful for very long sessions but
  // simple and correct. Optimisation candidate if it bites.
  const off = document.createElement("canvas");
  off.width = w;
  off.height = h;
  const offCtx = off.getContext("2d");
  if (!offCtx) return;
  offCtx.drawImage(source, x, y, w, h, 0, 0, w, h);
  const imgData = offCtx.getImageData(0, 0, w, h);

  const block = Math.round(a.blockSize);
  for (let by = 0; by < h; by += block) {
    for (let bx = 0; bx < w; bx += block) {
      // Sample the top-left pixel of the block. Faster than averaging
      // every pixel; the mosaic look hides the asymmetry.
      const off = (by * w + bx) * 4;
      const r = imgData.data[off];
      const g = imgData.data[off + 1];
      const b = imgData.data[off + 2];
      ctx.fillStyle = `rgb(${r},${g},${b})`;
      ctx.fillRect(
        x + bx,
        y + by,
        Math.min(block, w - bx),
        Math.min(block, h - by),
      );
    }
  }
}
