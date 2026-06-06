import { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  ArrowUpRight,
  Check,
  Circle,
  Copy,
  Droplets,
  Hash,
  Highlighter,
  Minus,
  Redo2,
  Square,
  SquareSlash,
  Type,
  Undo2,
  X,
} from "lucide-react";
import {
  editorCancel,
  editorCopy,
  editorSave,
  getPendingScreenshotInfo,
  setEditorSize,
} from "../lib/ipc";
import {
  COLOR_PRESETS,
  makeDragAnnotation,
  nextStepNumber,
  type Annotation,
  type ArrowAnnotation,
  type BlurAnnotation,
  type Tool,
} from "../lib/editor-geometry";

/**
 * Screenshot annotation editor (mounted in the `screenshot-editor`
 * Tauri window). Loads the currently-pending screenshot, renders it
 * onto a canvas, and lets the user layer annotation types on top:
 *
 *   • Arrow      — line + filled arrowhead.
 *   • Line       — plain straight line.
 *   • Text       — click position, type, Enter commits.
 *   • Rectangle  — empty-outline box.
 *   • Ellipse    — empty-outline ellipse.
 *   • Highlight  — translucent yellow box (CleanShot-style marker).
 *   • Blur       — pixelate the underlying pixels (mosaic, no deps).
 *   • Redact     — opaque black block (fully hides content).
 *   • Step       — click-placed numbered badge (auto-incrementing).
 *
 * Geometry + the annotation data model live in `lib/editor-geometry.ts`
 * (pure, unit-tested); this component owns the canvas drawing.
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
  const [copied, setCopied] = useState(false);

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

    // Preview of an in-progress drag (drag-based tools only).
    if (dragStart && dragCurrent && tool !== "text" && tool !== "step") {
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
  /** Copy the edited canvas to the clipboard (Cmd/Ctrl+C) — stay in the
   *  editor. Briefly flips the toolbar's "Copy" affordance to "Copied". */
  const copy = useCallback(async () => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    try {
      await editorCopy(canvas.toDataURL("image/png"));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch (e) {
      console.error("editor copy failed", e);
    }
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
      } else if (mod && e.key.toLowerCase() === "c") {
        // Copy the edited screenshot to the clipboard, stay in the editor.
        e.preventDefault();
        void copy();
      } else if (e.key === "Escape") {
        e.preventDefault();
        cancel();
      } else if (!mod && e.key.length === 1) {
        // Single-key tool shortcuts. Match CleanShot-X reasonably.
        const key = e.key.toLowerCase();
        if (key === "a") setTool("arrow");
        else if (key === "l") setTool("line");
        else if (key === "t") setTool("text");
        else if (key === "r") setTool("rect");
        else if (key === "e") setTool("ellipse");
        else if (key === "h") setTool("highlight");
        else if (key === "b") setTool("blur");
        else if (key === "x") setTool("redact");
        else if (key === "n") setTool("step");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [undo, redo, save, copy, cancel, textInput]);

  // Persist the editor window size (debounced) so the next open restores it.
  useEffect(() => {
    const win = getCurrentWebviewWindow();
    let timer: number | undefined;
    let unlisten: UnlistenFn | undefined;
    void win
      .onResized(({ payload }) => {
        if (timer) window.clearTimeout(timer);
        timer = window.setTimeout(() => {
          void (async () => {
            // onResized gives a physical size; store logical px so the
            // builder's inner_size restores the same visual size.
            const sf = await win.scaleFactor().catch(() => 1);
            void setEditorSize(payload.width / sf, payload.height / sf).catch(
              () => undefined,
            );
          })();
        }, 400);
      })
      .then((u) => {
        unlisten = u;
      });
    return () => {
      if (timer) window.clearTimeout(timer);
      unlisten?.();
    };
  }, []);

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
    const p = toCanvasCoords(e);
    // v0.66.0 — when a text input is open, suppress the default focus
    // shift this click would cause. In WKWebView `mousedown` fires
    // *before* the input's `blur`, so without this the blur handler would
    // run after we've already committed + opened the next input here and
    // would wrongly close that fresh input (text-adding felt broken —
    // you couldn't place a second text by clicking). preventDefault keeps
    // focus on the overlay input so no stray blur fires; we commit and
    // re-open here ourselves. (Toolbar/button clicks still blur → commit.)
    if (textInput) {
      e.preventDefault();
    }
    // If a text input is open, commit it first (single-click semantics,
    // consistent with native macOS apps — TextEdit, Pages, etc.).
    if (textInput) {
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
      // Fall through — the click that committed the text now ALSO
      // starts the next action (drag-start or text-relocate). Only
      // skip the new-action if user is still on the text tool AND
      // we want a clean "click outside to dismiss" UX → re-place
      // the text input at the new spot, which matches Spotlight-style
      // text-placement editors.
      if (tool === "text") {
        setTextInput({ x: p.x, y: p.y, value: "" });
        return;
      }
      // Non-text tool → fall through to the drag-start path below.
    }
    if (tool === "text") {
      // Click-to-place text input — no existing input to dismiss.
      setTextInput({ x: p.x, y: p.y, value: "" });
      return;
    }
    if (tool === "step") {
      // Click-to-place numbered badge; number auto-increments.
      setAnnotations((cur) => [
        ...cur,
        {
          type: "step",
          x: p.x,
          y: p.y,
          number: nextStepNumber(cur),
          color,
          size: Math.max(14, strokeWidth * 4),
        },
      ]);
      setRedoStack([]);
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
            onClick={() => void copy()}
            title="Copy edited image to clipboard (⌘C)"
            className="rounded-md border border-[var(--color-border)] px-3 py-1 text-[12px] hover:bg-[var(--color-bg)]"
          >
            <span className="flex items-center gap-1">
              <Copy size={12} /> {copied ? "Copied" : "Copy (⌘C)"}
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

/** A single tool button. Module-level (not defined during render) so it's
 *  a stable component identity. */
function ToolBtn({
  t,
  icon,
  label,
  shortcut,
  active,
  onSelect,
}: {
  t: Tool;
  icon: React.ReactNode;
  label: string;
  shortcut: string;
  active: Tool;
  onSelect: (t: Tool) => void;
}) {
  return (
    <button
      onClick={() => onSelect(t)}
      title={`${label} (${shortcut})`}
      className={
        "flex h-10 w-10 items-center justify-center rounded-md border transition-colors " +
        (active === t
          ? "border-[var(--color-accent)] bg-[var(--color-accent)]/15 text-[var(--color-accent)]"
          : "border-[var(--color-border)] hover:bg-[var(--color-bg)]")
      }
    >
      {icon}
    </button>
  );
}

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
  return (
    <div className="flex w-14 shrink-0 flex-col items-center gap-1.5 border-r border-[var(--color-border)] bg-[var(--color-surface)] p-2">
      <ToolBtn t="arrow" icon={<ArrowUpRight size={16} />} label="Arrow" shortcut="A" active={tool} onSelect={setTool} />
      <ToolBtn t="line" icon={<Minus size={16} />} label="Line" shortcut="L" active={tool} onSelect={setTool} />
      <ToolBtn t="text" icon={<Type size={16} />} label="Text" shortcut="T" active={tool} onSelect={setTool} />
      <ToolBtn t="rect" icon={<Square size={16} />} label="Rectangle" shortcut="R" active={tool} onSelect={setTool} />
      <ToolBtn t="ellipse" icon={<Circle size={16} />} label="Ellipse" shortcut="E" active={tool} onSelect={setTool} />
      <ToolBtn t="highlight" icon={<Highlighter size={16} />} label="Highlight" shortcut="H" active={tool} onSelect={setTool} />
      <ToolBtn t="blur" icon={<Droplets size={16} />} label="Blur" shortcut="B" active={tool} onSelect={setTool} />
      <ToolBtn t="redact" icon={<SquareSlash size={16} />} label="Redact" shortcut="X" active={tool} onSelect={setTool} />
      <ToolBtn t="step" icon={<Hash size={16} />} label="Step badge" shortcut="N" active={tool} onSelect={setTool} />

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

function drawAnnotation(
  ctx: CanvasRenderingContext2D,
  a: Annotation,
  source: HTMLImageElement,
) {
  switch (a.type) {
    case "arrow":
      drawArrow(ctx, a);
      break;
    case "line":
      ctx.save();
      ctx.strokeStyle = a.color;
      ctx.lineWidth = a.width;
      ctx.lineCap = "round";
      ctx.beginPath();
      ctx.moveTo(a.x1, a.y1);
      ctx.lineTo(a.x2, a.y2);
      ctx.stroke();
      ctx.restore();
      break;
    case "rect":
      ctx.strokeStyle = a.color;
      ctx.lineWidth = a.width;
      ctx.strokeRect(a.x, a.y, a.w, a.h);
      break;
    case "ellipse":
      ctx.save();
      ctx.strokeStyle = a.color;
      ctx.lineWidth = a.width;
      ctx.beginPath();
      ctx.ellipse(
        a.x + a.w / 2,
        a.y + a.h / 2,
        Math.abs(a.w / 2),
        Math.abs(a.h / 2),
        0,
        0,
        Math.PI * 2,
      );
      ctx.stroke();
      ctx.restore();
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
    case "redact":
      // Opaque black block — fully hides the content (vs. blur's mosaic).
      ctx.save();
      ctx.fillStyle = "#000000";
      ctx.fillRect(a.x, a.y, a.w, a.h);
      ctx.restore();
      break;
    case "step":
      drawStep(ctx, a);
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

/** Numbered badge: filled circle + centred white number. */
function drawStep(
  ctx: CanvasRenderingContext2D,
  a: { x: number; y: number; number: number; color: string; size: number },
) {
  const r = a.size;
  ctx.save();
  ctx.fillStyle = a.color;
  ctx.beginPath();
  ctx.arc(a.x, a.y, r, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = "#ffffff";
  ctx.font = `bold ${Math.round(r * 1.1)}px var(--font-sans, sans-serif)`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(String(a.number), a.x, a.y + 1);
  ctx.restore();
}

/** A sleek arrow: rounded shaft into a **concave-back** arrowhead (the
 *  back edges cave inward toward the tip, the polished CleanShot-style
 *  look rather than a plain flat triangle). Head scales with stroke width
 *  but is capped so it stays proportional, and the shaft stops at the
 *  head's notch so the two flow into one another with no overshoot. */
function drawArrow(ctx: CanvasRenderingContext2D, a: ArrowAnnotation) {
  const dx = a.x2 - a.x1;
  const dy = a.y2 - a.y1;
  const len = Math.hypot(dx, dy);
  if (len < 1) return;
  const angle = Math.atan2(dy, dx);
  const ux = Math.cos(angle);
  const uy = Math.sin(angle);

  // Head length scales with stroke width, capped, and never longer than
  // the arrow itself (short drags still look right).
  const headLen = Math.min(len * 0.9, Math.min(48, Math.max(16, a.width * 4.5)));
  const barb = Math.PI / 6; // 30° half-angle
  const notch = headLen * 0.62; // back-center pulled toward the tip → concave

  const tipX = a.x2;
  const tipY = a.y2;
  const lx = tipX - headLen * Math.cos(angle - barb);
  const ly = tipY - headLen * Math.sin(angle - barb);
  const rx = tipX - headLen * Math.cos(angle + barb);
  const ry = tipY - headLen * Math.sin(angle + barb);
  const nx = tipX - ux * notch;
  const ny = tipY - uy * notch;

  ctx.save();
  ctx.strokeStyle = a.color;
  ctx.fillStyle = a.color;
  ctx.lineWidth = a.width;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";

  // Shaft — flows into the head's notch (not the bare tip).
  ctx.beginPath();
  ctx.moveTo(a.x1, a.y1);
  ctx.lineTo(nx, ny);
  ctx.stroke();

  // Concave arrowhead: tip → left barb → notch → right barb.
  ctx.beginPath();
  ctx.moveTo(tipX, tipY);
  ctx.lineTo(lx, ly);
  ctx.lineTo(nx, ny);
  ctx.lineTo(rx, ry);
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
