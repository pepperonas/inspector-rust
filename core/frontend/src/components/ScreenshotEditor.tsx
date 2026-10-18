import { useCallback, useEffect, useLayoutEffect, useReducer, useRef, useState } from "react";
import type { RefObject } from "react";
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
  MousePointer2,
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
  getPendingScreenshotDataUrl,
  setEditorSize,
} from "../lib/ipc";
import {
  COLOR_PRESETS,
  EMPTY_DOC,
  boundsOf,
  handlesFor,
  hitTest,
  makeDragAnnotation,
  moveAnnotation,
  nextStepNumber,
  normalizeAnnotation,
  reflowSteps,
  resizeAnnotation,
  restyleAnnotation,
  type Annotation,
  type ArrowAnnotation,
  type BlurAnnotation,
  type EditorDoc,
  type Handle,
  type Point,
  type Style,
  type Tool,
} from "../lib/editor-geometry";
import { canRedo, canUndo, historyReducer, initHistory } from "../lib/editor-history";
import { t, useT } from "../lib/i18n";

/**
 * Screenshot annotation editor (mounted in the `screenshot-editor`
 * Tauri window). Loads the currently-pending screenshot, renders it
 * onto a canvas, and lets the user layer annotation types on top.
 *
 * Since the CleanShot-X-parity rework the editor is **object-based**: the
 * `select` tool selects an existing annotation (click its body, or grab a
 * resize handle), moves it, resizes it, restyles it (the toolbar controls act
 * on the selection), or deletes it (Delete/Backspace). Undo/redo is a snapshot
 * history of the whole document (`lib/editor-history.ts`), so a move/resize/
 * restyle/delete is each an undoable step — not just "added the last shape".
 *
 * Geometry + the annotation data model live in `lib/editor-geometry.ts`
 * (pure, unit-tested); this component owns the canvas drawing + interaction.
 *
 * Save bakes the canvas to PNG and ships it to the backend, which writes it to
 * ~/Downloads, pushes to clipboard + history, and re-shows the preview.
 *
 * Hotkeys: V select · A/L/T/R/E/H/B/X/N tools · ⌘Z / ⌘⇧Z undo/redo ·
 * ⌘S save · ⌘C copy · Delete/Backspace remove selection · Esc deselect/cancel.
 */

type Interaction =
  | { kind: "draw"; start: Point; current: Point }
  | { kind: "move"; id: string; startPt: Point; orig: Annotation; live: Annotation }
  | { kind: "resize"; id: string; handle: Handle; startPt: Point; orig: Annotation; live: Annotation }
  | null;

export function ScreenshotEditor() {
  useT();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  /** The decoded screenshot image. Kept in a ref because we need it
   *  for every redraw (background) and for blur (sampling source pixels). */
  const imgRef = useRef<HTMLImageElement | null>(null);

  const [imgReady, setImgReady] = useState(false);
  const [tool, setTool] = useState<Tool>("arrow");

  // Live drawing style. Changing a control also restyles the current
  // selection (see applyStyle). opacity/shadow/fontSize controls arrive in
  // later phases; the state exists now because new annotations carry them.
  const [color, setColor] = useState<string>(COLOR_PRESETS[0]);
  const [strokeWidth, setStrokeWidth] = useState<number>(4);
  const [opacity] = useState<number>(1);
  const [shadow] = useState<boolean>(false);
  const [fontSize] = useState<number>(28);
  const styleObj: Style = { color, width: strokeWidth, opacity, shadow, fontSize };

  // Undo/redo history over the whole document.
  const [hist, dispatch] = useReducer(
    (s: Parameters<typeof historyReducer<EditorDoc>>[0], a: Parameters<typeof historyReducer<EditorDoc>>[1]) =>
      historyReducer(s, a),
    undefined,
    () => initHistory(EMPTY_DOC),
  );
  const annotations = hist.present.annotations;

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [interaction, setInteraction] = useState<Interaction>(null);

  /** Inline text-input state. Only non-null while the user is typing. */
  const [textInput, setTextInput] = useState<{ x: number; y: number; value: string } | null>(null);

  const [saving, setSaving] = useState(false);
  const [copied, setCopied] = useState(false);
  const fitBoxRef = useRef<HTMLDivElement>(null);
  const idRef = useRef(0);
  const nextId = () => `a${++idRef.current}`;

  const annById = (id: string | null): Annotation | undefined =>
    id ? annotations.find((a) => a.id === id) : undefined;

  // Canvas→CSS pixel ratio: a Retina shot's backing store is 2× its display
  // size, so handle hit-radii and selection chrome must scale by this to stay
  // a consistent visual size.
  const pxRatio = useCallback(() => {
    const c = canvasRef.current;
    if (!c) return 1;
    const w = c.getBoundingClientRect().width;
    return w > 0 ? c.width / w : 1;
  }, []);

  // ── Commit helpers (each is one undoable history step) ───────────
  const commitAnns = useCallback(
    (anns: Annotation[]) => dispatch({ type: "commit", next: { ...hist.present, annotations: anns } }),
    [hist.present],
  );
  const commitAppend = useCallback((a: Annotation) => commitAnns([...annotations, a]), [commitAnns, annotations]);
  const commitReplace = useCallback(
    (id: string, a: Annotation) => commitAnns(annotations.map((x) => (x.id === id ? a : x))),
    [commitAnns, annotations],
  );
  const commitDelete = useCallback(
    (id: string) => commitAnns(reflowSteps(annotations.filter((x) => x.id !== id))),
    [commitAnns, annotations],
  );

  // ── Fit the canvas into the viewport, deterministically ─────────
  const fitCanvas = useCallback(() => {
    const canvas = canvasRef.current;
    const box = fitBoxRef.current;
    const img = imgRef.current;
    if (!canvas || !box || !img) return;
    const cs = getComputedStyle(box);
    const availW = box.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
    const availH = box.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
    if (availW <= 0 || availH <= 0) return;
    const scale = Math.min(
      availW / img.naturalWidth,
      availH / img.naturalHeight,
      1 / (window.devicePixelRatio || 1),
    );
    canvas.style.width = `${Math.max(1, Math.round(img.naturalWidth * scale))}px`;
    canvas.style.height = `${Math.max(1, Math.round(img.naturalHeight * scale))}px`;
  }, []);

  useEffect(() => {
    if (!imgReady) return;
    fitCanvas();
    const box = fitBoxRef.current;
    if (!box) return;
    const ro = new ResizeObserver(() => fitCanvas());
    ro.observe(box);
    return () => ro.disconnect();
  }, [imgReady, fitCanvas]);

  // ── Load the pending screenshot on mount ─────────────────────────
  const loadImage = useCallback(async () => {
    const dataUrl = await getPendingScreenshotDataUrl().catch(() => null);
    if (!dataUrl) {
      console.error("editor: no pending screenshot data URL");
      return;
    }
    const img = new Image();
    img.onload = () => {
      imgRef.current = img;
      setImgReady(true);
    };
    img.onerror = (e) => console.error("editor: screenshot image failed to load", e);
    img.src = dataUrl;
  }, []);

  useEffect(() => {
    void loadImage();
    let unlisten: UnlistenFn | undefined;
    void listen("editor-screenshot-changed", () => {
      dispatch({ type: "reset", present: EMPTY_DOC });
      setSelectedId(null);
      void loadImage();
    }).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  }, [loadImage]);

  // Drop a selection whose annotation vanished (undo past its creation, delete).
  useEffect(() => {
    if (selectedId && !annotations.some((a) => a.id === selectedId)) setSelectedId(null);
  }, [annotations, selectedId]);

  // ── Redraw on every state change ─────────────────────────────────
  useEffect(() => {
    if (!imgReady) return;
    const canvas = canvasRef.current;
    const img = imgRef.current;
    if (!canvas || !img) return;
    if (canvas.width !== img.naturalWidth || canvas.height !== img.naturalHeight) {
      canvas.width = img.naturalWidth;
      canvas.height = img.naturalHeight;
      fitCanvas();
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(img, 0, 0);

    // Substitute the live-dragged annotation during a move/resize.
    const list =
      interaction && (interaction.kind === "move" || interaction.kind === "resize")
        ? annotations.map((a) => (a.id === interaction.id ? interaction.live : a))
        : annotations;
    for (const a of list) drawAnnotation(ctx, a, img);

    // Preview of an in-progress draw.
    if (interaction && interaction.kind === "draw" && tool !== "text" && tool !== "step") {
      const preview = makeDragAnnotation(tool, interaction.start, interaction.current, styleObj, "preview");
      if (preview) drawAnnotation(ctx, preview, img);
    }

    // Selection chrome for the selected annotation (follows a live drag).
    const sel = list.find((a) => a.id === selectedId);
    if (sel) drawSelection(ctx, sel, pxRatio());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hist, interaction, selectedId, tool, color, strokeWidth, opacity, shadow, fontSize, imgReady, fitCanvas, pxRatio]);

  // ── Undo / Redo / Save / Cancel ──────────────────────────────────
  const undo = useCallback(() => dispatch({ type: "undo" }), []);
  const redo = useCallback(() => dispatch({ type: "redo" }), []);
  const save = useCallback(async () => {
    const canvas = canvasRef.current;
    if (!canvas || saving) return;
    setSaving(true);
    try {
      await editorSave(canvas.toDataURL("image/png"));
    } catch (e) {
      console.error("editor save failed", e);
    } finally {
      setSaving(false);
    }
  }, [saving]);
  const cancel = useCallback(() => {
    void editorCancel().catch(() => undefined);
  }, []);
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

  // Changing a style control also restyles the current selection.
  const applyColor = (c: string) => {
    setColor(c);
    if (selectedId) {
      const a = annById(selectedId);
      if (a) commitReplace(selectedId, restyleAnnotation(a, { color: c }));
    }
  };
  const applyWidth = (w: number) => {
    setStrokeWidth(w);
    if (selectedId) {
      const a = annById(selectedId);
      if (a) commitReplace(selectedId, restyleAnnotation(a, { width: w }));
    }
  };

  // ── Hotkeys ──────────────────────────────────────────────────────
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (textInput) return; // typing into the text overlay
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
        e.preventDefault();
        void copy();
      } else if ((e.key === "Delete" || e.key === "Backspace") && selectedId) {
        e.preventDefault();
        commitDelete(selectedId);
        setSelectedId(null);
      } else if ((e.key === "ArrowUp" || e.key === "ArrowDown" || e.key === "ArrowLeft" || e.key === "ArrowRight") && selectedId) {
        e.preventDefault();
        const a = annById(selectedId);
        if (a) {
          const step = e.shiftKey ? 10 : 1;
          const dx = e.key === "ArrowLeft" ? -step : e.key === "ArrowRight" ? step : 0;
          const dy = e.key === "ArrowUp" ? -step : e.key === "ArrowDown" ? step : 0;
          commitReplace(selectedId, moveAnnotation(a, dx, dy));
        }
      } else if (e.key === "Escape") {
        e.preventDefault();
        if (selectedId) setSelectedId(null);
        else cancel();
      } else if (!mod && e.key.length === 1) {
        const key = e.key.toLowerCase();
        if (key === "v") setTool("select");
        else if (key === "a") setTool("arrow");
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
  });

  // Persist the editor window size (debounced).
  useEffect(() => {
    const win = getCurrentWebviewWindow();
    let timer: number | undefined;
    let unlisten: UnlistenFn | undefined;
    void win
      .onResized(({ payload }) => {
        if (timer) window.clearTimeout(timer);
        timer = window.setTimeout(() => {
          void (async () => {
            const sf = await win.scaleFactor().catch(() => 1);
            void setEditorSize(payload.width / sf, payload.height / sf).catch(() => undefined);
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
  const toCanvasCoords = (e: React.MouseEvent<HTMLCanvasElement>): Point => {
    const canvas = canvasRef.current;
    if (!canvas) return { x: 0, y: 0 };
    const rect = canvas.getBoundingClientRect();
    const sx = canvas.width / rect.width;
    const sy = canvas.height / rect.height;
    return { x: (e.clientX - rect.left) * sx, y: (e.clientY - rect.top) * sy };
  };

  const grabHandle = (p: Point): Handle | null => {
    const a = annById(selectedId);
    if (!a) return null;
    const r = 9 * pxRatio();
    for (const h of handlesFor(a)) {
      if (Math.abs(p.x - h.x) <= r && Math.abs(p.y - h.y) <= r) return h.pos;
    }
    return null;
  };

  const placeStep = (p: Point) => {
    const a: Annotation = { id: nextId(), type: "step", x: p.x, y: p.y, number: nextStepNumber(annotations), color, size: fontSize, opacity, shadow };
    commitAppend(a);
    setSelectedId(a.id);
  };

  const onCanvasMouseDown = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const p = toCanvasCoords(e);
    // Commit an open text input first (single-click semantics).
    if (textInput) {
      e.preventDefault();
      const v = textInput.value.trim();
      if (v.length > 0) {
        commitAppend({ id: nextId(), type: "text", x: textInput.x, y: textInput.y, text: v, color, size: fontSize, opacity, shadow });
      }
      setTextInput(null);
      if (tool === "text") {
        setTextInput({ x: p.x, y: p.y, value: "" });
        return;
      }
      // else fall through into the tool branch
    }

    if (tool === "select") {
      const handle = grabHandle(p);
      if (handle && selectedId) {
        const orig = annById(selectedId);
        if (orig) setInteraction({ kind: "resize", id: selectedId, handle, startPt: p, orig, live: orig });
        return;
      }
      const hit = hitTest(annotations, p, 6 * pxRatio());
      if (hit) {
        setSelectedId(hit);
        const orig = annotations.find((a) => a.id === hit)!;
        setInteraction({ kind: "move", id: hit, startPt: p, orig, live: orig });
      } else {
        setSelectedId(null);
      }
      return;
    }
    if (tool === "text") {
      setTextInput({ x: p.x, y: p.y, value: "" });
      return;
    }
    if (tool === "step") {
      placeStep(p);
      return;
    }
    setInteraction({ kind: "draw", start: p, current: p });
  };

  const onCanvasMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!interaction) return;
    const p = toCanvasCoords(e);
    if (interaction.kind === "draw") {
      setInteraction({ ...interaction, current: p });
    } else if (interaction.kind === "move") {
      setInteraction({ ...interaction, live: moveAnnotation(interaction.orig, p.x - interaction.startPt.x, p.y - interaction.startPt.y) });
    } else if (interaction.kind === "resize") {
      setInteraction({ ...interaction, live: resizeAnnotation(interaction.orig, interaction.handle, p.x - interaction.startPt.x, p.y - interaction.startPt.y) });
    }
  };

  const onCanvasMouseUp = (e: React.MouseEvent<HTMLCanvasElement>) => {
    if (!interaction) return;
    if (interaction.kind === "draw") {
      const end = toCanvasCoords(e);
      const a = makeDragAnnotation(tool, interaction.start, end, styleObj, nextId());
      if (a && !isDegenerate(a)) {
        commitAppend(a);
        setSelectedId(a.id);
      }
    } else if (interaction.kind === "move" || interaction.kind === "resize") {
      commitReplace(interaction.id, normalizeAnnotation(interaction.live));
    }
    setInteraction(null);
  };

  const commitTextInput = () => {
    if (!textInput) return;
    const v = textInput.value.trim();
    if (v.length > 0) {
      commitAppend({ id: nextId(), type: "text", x: textInput.x, y: textInput.y, text: v, color, size: fontSize, opacity, shadow });
    }
    setTextInput(null);
  };

  const cursor =
    tool === "select" ? "default" : tool === "text" ? "text" : tool === "blur" ? "cell" : "crosshair";

  // ── Render ────────────────────────────────────────────────────────
  return (
    <div className="flex h-screen w-screen flex-col bg-[var(--color-bg)] text-[var(--color-fg)]">
      <div className="flex shrink-0 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
        <span className="text-[12px] font-semibold text-[var(--color-muted)]">{t("shot.edit")}</span>
        <div className="flex items-center gap-1">
          <IconButton onClick={undo} disabled={!canUndo(hist)} title={t("shot.undo_z")}>
            <Undo2 size={14} />
          </IconButton>
          <IconButton onClick={redo} disabled={!canRedo(hist)} title={t("shot.redo_z")}>
            <Redo2 size={14} />
          </IconButton>
          <div className="mx-2 h-5 w-px bg-[var(--color-border)]" />
          <button
            onClick={cancel}
            className="rounded-md border border-[var(--color-border)] px-3 py-1 text-[12px] hover:bg-[var(--color-bg)]"
          >
            <span className="flex items-center gap-1">
              <X size={12} /> {t("common.cancel")}
            </span>
          </button>
          <button
            onClick={() => void copy()}
            title={t("shot.copy_edited_image_to_clipboard")}
            className="rounded-md border border-[var(--color-border)] px-3 py-1 text-[12px] hover:bg-[var(--color-bg)]"
          >
            <span className="flex items-center gap-1">
              <Copy size={12} />{" "}
              {copied ? <span className="md3-success-pop">{t("shot.copied")}</span> : t("shot.copy_c")}
            </span>
          </button>
          <button
            onClick={save}
            disabled={saving}
            className="rounded-md bg-[var(--color-accent)] px-3 py-1 text-[12px] font-medium text-[var(--color-accent-fg)] hover:opacity-90 disabled:opacity-50"
          >
            <span className="flex items-center gap-1">
              <Check size={12} /> {saving ? t("shot.saving") : t("shot.save_s")}
            </span>
          </button>
        </div>
      </div>

      <div className="flex min-h-0 flex-1">
        <Toolbar tool={tool} setTool={setTool} color={color} setColor={applyColor} strokeWidth={strokeWidth} setStrokeWidth={applyWidth} />
        <div
          ref={fitBoxRef}
          className="relative flex min-h-0 flex-1 items-center justify-center overflow-hidden bg-[#0f0f0f] p-4"
        >
          {imgReady ? (
            <div className="relative">
              <canvas
                ref={canvasRef}
                onMouseDown={onCanvasMouseDown}
                onMouseMove={onCanvasMouseMove}
                onMouseUp={onCanvasMouseUp}
                onMouseLeave={(e) => {
                  if (interaction) onCanvasMouseUp(e);
                }}
                className="shadow-2xl"
                style={{ cursor }}
              />
              {textInput && (
                <TextInputOverlay
                  canvasRef={canvasRef}
                  input={textInput}
                  color={color}
                  fontSize={fontSize}
                  onChange={(v) => setTextInput((cur) => (cur ? { ...cur, value: v } : cur))}
                  onCommit={commitTextInput}
                  onCancel={() => setTextInput(null)}
                />
              )}
            </div>
          ) : (
            <span className="text-[12px] text-[var(--color-muted)]">{t("shot.loading_screenshot")}</span>
          )}
        </div>
      </div>
    </div>
  );
}

/** Reject a degenerate drag (a click, not a drag) so draw tools don't drop
 *  zero-size dots/lines. */
function isDegenerate(a: Annotation): boolean {
  if (a.type === "arrow" || a.type === "line") return Math.hypot(a.x2 - a.x1, a.y2 - a.y1) < 3;
  const b = boundsOf(a);
  return b.w < 3 && b.h < 3;
}

// ── Toolbar component ───────────────────────────────────────────────

function ToolBtn({
  t: toolId,
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
      onClick={() => onSelect(toolId)}
      title={`${label} (${shortcut})`}
      className={
        "flex h-10 w-10 items-center justify-center rounded-md border transition-colors " +
        (active === toolId
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
  useT();
  return (
    <div className="flex w-14 shrink-0 flex-col items-center gap-1.5 overflow-y-auto border-r border-[var(--color-border)] bg-[var(--color-surface)] p-2">
      <ToolBtn t="select" icon={<MousePointer2 size={16} />} label={t("shot.select")} shortcut="V" active={tool} onSelect={setTool} />
      <div className="my-0.5 h-px w-8 bg-[var(--color-border)]" />
      <ToolBtn t="arrow" icon={<ArrowUpRight size={16} />} label={t("shot.arrow")} shortcut="A" active={tool} onSelect={setTool} />
      <ToolBtn t="line" icon={<Minus size={16} />} label={t("shot.line")} shortcut="L" active={tool} onSelect={setTool} />
      <ToolBtn t="text" icon={<Type size={16} />} label={t("shot.text")} shortcut="T" active={tool} onSelect={setTool} />
      <ToolBtn t="rect" icon={<Square size={16} />} label={t("shot.rectangle")} shortcut="R" active={tool} onSelect={setTool} />
      <ToolBtn t="ellipse" icon={<Circle size={16} />} label={t("shot.ellipse")} shortcut="E" active={tool} onSelect={setTool} />
      <ToolBtn t="highlight" icon={<Highlighter size={16} />} label={t("shot.highlight")} shortcut="H" active={tool} onSelect={setTool} />
      <ToolBtn t="blur" icon={<Droplets size={16} />} label={t("shot.blur")} shortcut="B" active={tool} onSelect={setTool} />
      <ToolBtn t="redact" icon={<SquareSlash size={16} />} label={t("shot.redact")} shortcut="X" active={tool} onSelect={setTool} />
      <ToolBtn t="step" icon={<Hash size={16} />} label={t("shot.step_badge")} shortcut="N" active={tool} onSelect={setTool} />

      <div className="mt-3 flex flex-col items-center gap-1.5">
        {COLOR_PRESETS.map((c) => (
          <button
            key={c}
            onClick={() => setColor(c)}
            title={c}
            className={
              "h-6 w-6 rounded-full border-2 " +
              (color === c ? "border-[var(--color-accent)] scale-110" : "border-[var(--color-border)]")
            }
            style={{ backgroundColor: c }}
          />
        ))}
      </div>

      <div className="mt-3 flex flex-col items-center gap-1">
        <span className="text-[9px] uppercase tracking-wider text-[var(--color-muted)]">{t("shot.size")}</span>
        <input
          type="range"
          min={2}
          max={16}
          value={strokeWidth}
          onChange={(e) => setStrokeWidth(parseInt(e.target.value, 10))}
          className="h-1 w-10 rotate-90 cursor-pointer accent-[var(--color-accent)]"
          style={{ marginTop: 16 }}
        />
        <span className="mt-7 text-[10px] tabular-nums text-[var(--color-muted)]">{strokeWidth}px</span>
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
  canvasRef,
  input,
  color,
  fontSize,
  onChange,
  onCommit,
  onCancel,
}: {
  canvasRef: RefObject<HTMLCanvasElement | null>;
  input: { x: number; y: number; value: string };
  color: string;
  fontSize: number;
  onChange: (v: string) => void;
  onCommit: () => void;
  onCancel: () => void;
}) {
  useT();
  const inputRef = useRef<HTMLInputElement>(null);
  const [geom, setGeom] = useState<{ left: number; top: number; fontPx: number } | null>(null);
  useLayoutEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const cssScaleX = rect.width / canvas.width;
    const cssScaleY = rect.height / canvas.height;
    setGeom({
      left: input.x * cssScaleX,
      top: input.y * cssScaleY - (fontSize * cssScaleY) / 2,
      fontPx: fontSize * cssScaleY,
    });
  }, [canvasRef, input.x, input.y, fontSize]);
  useEffect(() => {
    inputRef.current?.focus();
  }, []);
  if (!geom) return null;
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
        e.stopPropagation();
      }}
      style={{
        position: "absolute",
        left: geom.left,
        top: geom.top,
        fontSize: geom.fontPx,
        color,
        background: "rgba(0,0,0,0.4)",
        border: "1px dashed rgba(255,255,255,0.5)",
        padding: "2px 4px",
        outline: "none",
        minWidth: 80,
        fontFamily: "var(--font-sans, sans-serif)",
        fontWeight: 600,
      }}
      placeholder={t("shot.type_enter")}
    />
  );
}

// ── Drawing helpers ────────────────────────────────────────────────

function drawAnnotation(ctx: CanvasRenderingContext2D, a: Annotation, source: HTMLImageElement) {
  ctx.save();
  const op = (a as { opacity?: number }).opacity;
  if (typeof op === "number") ctx.globalAlpha = op;
  if ((a as { shadow?: boolean }).shadow) {
    ctx.shadowColor = "rgba(0,0,0,0.45)";
    ctx.shadowBlur = 8;
    ctx.shadowOffsetY = 2;
  }
  switch (a.type) {
    case "arrow":
      drawArrow(ctx, a);
      break;
    case "line":
      ctx.strokeStyle = a.color;
      ctx.lineWidth = a.width;
      ctx.lineCap = "round";
      ctx.beginPath();
      ctx.moveTo(a.x1, a.y1);
      ctx.lineTo(a.x2, a.y2);
      ctx.stroke();
      break;
    case "rect":
      ctx.strokeStyle = a.color;
      ctx.lineWidth = a.width;
      ctx.strokeRect(a.x, a.y, a.w, a.h);
      break;
    case "ellipse":
      ctx.strokeStyle = a.color;
      ctx.lineWidth = a.width;
      ctx.beginPath();
      ctx.ellipse(a.x + a.w / 2, a.y + a.h / 2, Math.abs(a.w / 2), Math.abs(a.h / 2), 0, 0, Math.PI * 2);
      ctx.stroke();
      break;
    case "highlight":
      ctx.fillStyle = a.color;
      ctx.globalAlpha = 0.35;
      ctx.fillRect(a.x, a.y, a.w, a.h);
      break;
    case "blur":
      drawBlur(ctx, a, source);
      break;
    case "redact":
      ctx.fillStyle = "#000000";
      ctx.fillRect(a.x, a.y, a.w, a.h);
      break;
    case "step":
      drawStep(ctx, a);
      break;
    case "text":
      ctx.fillStyle = a.color;
      ctx.font = `bold ${a.size}px var(--font-sans, sans-serif)`;
      ctx.textBaseline = "middle";
      ctx.fillText(a.text, a.x, a.y);
      break;
  }
  ctx.restore();
}

/** Dashed outline + square handles around the selected annotation. Sizes are
 *  scaled by `px` (canvas→CSS ratio) so they read consistently on Retina. */
function drawSelection(ctx: CanvasRenderingContext2D, a: Annotation, px: number) {
  const b = boundsOf(a);
  ctx.save();
  ctx.strokeStyle = "#3b82f6";
  ctx.lineWidth = 1.5 * px;
  ctx.setLineDash([6 * px, 4 * px]);
  ctx.strokeRect(b.x, b.y, b.w, b.h);
  ctx.setLineDash([]);
  const s = 5 * px;
  for (const h of handlesFor(a)) {
    ctx.fillStyle = "#ffffff";
    ctx.strokeStyle = "#3b82f6";
    ctx.lineWidth = 1.5 * px;
    ctx.beginPath();
    ctx.rect(h.x - s, h.y - s, s * 2, s * 2);
    ctx.fill();
    ctx.stroke();
  }
  ctx.restore();
}

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

function drawArrow(ctx: CanvasRenderingContext2D, a: ArrowAnnotation) {
  const dx = a.x2 - a.x1;
  const dy = a.y2 - a.y1;
  const len = Math.hypot(dx, dy);
  if (len < 1) return;
  const angle = Math.atan2(dy, dx);
  const ux = Math.cos(angle);
  const uy = Math.sin(angle);
  const headLen = Math.min(len * 0.9, Math.min(48, Math.max(16, a.width * 4.5)));
  const barb = Math.PI / 6;
  const notch = headLen * 0.62;
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
  ctx.beginPath();
  ctx.moveTo(a.x1, a.y1);
  ctx.lineTo(nx, ny);
  ctx.stroke();
  ctx.beginPath();
  ctx.moveTo(tipX, tipY);
  ctx.lineTo(lx, ly);
  ctx.lineTo(nx, ny);
  ctx.lineTo(rx, ry);
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

function drawBlur(ctx: CanvasRenderingContext2D, a: BlurAnnotation, source: HTMLImageElement) {
  const x = Math.round(a.x);
  const y = Math.round(a.y);
  const w = Math.round(a.w);
  const h = Math.round(a.h);
  if (w < 1 || h < 1) return;
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
      const o = (by * w + bx) * 4;
      const r = imgData.data[o];
      const g = imgData.data[o + 1];
      const b = imgData.data[o + 2];
      ctx.fillStyle = `rgb(${r},${g},${b})`;
      ctx.fillRect(x + bx, y + by, Math.min(block, w - bx), Math.min(block, h - by));
    }
  }
}
