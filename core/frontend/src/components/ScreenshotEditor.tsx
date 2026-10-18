import { useCallback, useEffect, useLayoutEffect, useReducer, useRef, useState } from "react";
import type { RefObject } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  ArrowUpRight,
  Check,
  Circle,
  Copy,
  Crop,
  Droplets,
  Focus,
  Hash,
  Highlighter,
  Minus,
  MousePointer2,
  PenTool,
  Redo2,
  Smile,
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
  clampRectToImage,
  dragRect,
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
  type PenAnnotation,
  type Point,
  type Rect,
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
  | { kind: "pen"; points: Point[] }
  | { kind: "crop"; start: Point; current: Point }
  | null;

/** A curated sticker set (no dependency — just characters). */
const EMOJIS = [
  "😀", "😂", "😍", "🤔", "😎", "🤯", "👍", "👎", "🙏", "👀", "🙌", "👋",
  "🔥", "✅", "❌", "⭐", "❤️", "🎉", "💡", "⚠️", "🚀", "💯", "📌", "➡️",
] as const;

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
  const [opacity, setOpacity] = useState<number>(1);
  const [shadow, setShadow] = useState<boolean>(false);
  const [fontSize, setFontSize] = useState<number>(28);
  const [emoji, setEmoji] = useState<string>(EMOJIS[0]);
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
  const commitCrop = useCallback(
    (crop: Rect | null) => dispatch({ type: "commit", next: { ...hist.present, crop } }),
    [hist.present],
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

    // Substitute the live-dragged annotation during a move/resize.
    const list =
      interaction && (interaction.kind === "move" || interaction.kind === "resize")
        ? annotations.map((a) => (a.id === interaction.id ? interaction.live : a))
        : annotations;
    composeScene(ctx, img, list);

    // Preview of an in-progress draw.
    if (interaction && interaction.kind === "draw" && tool !== "text" && tool !== "step") {
      const preview = makeDragAnnotation(tool, interaction.start, interaction.current, styleObj, "preview");
      if (preview) drawAnnotation(ctx, preview, img);
    }
    // Preview of an in-progress freehand pen stroke.
    if (interaction && interaction.kind === "pen") {
      drawAnnotation(ctx, { id: "preview", type: "pen", points: interaction.points, color, width: strokeWidth, opacity, shadow }, img);
    }

    // Crop chrome (dim outside + border) — editor-only, never in the export.
    const crop = interaction && interaction.kind === "crop" ? dragRect(interaction.start, interaction.current) : hist.present.crop;
    if (crop && (crop.w > 0 || crop.h > 0)) drawCropChrome(ctx, crop, canvas.width, canvas.height, pxRatio());

    // Selection chrome (follows a live drag; hidden while cropping).
    const sel = tool !== "crop" ? list.find((a) => a.id === selectedId) : undefined;
    if (sel) drawSelection(ctx, sel, pxRatio());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hist, interaction, selectedId, tool, color, strokeWidth, opacity, shadow, fontSize, imgReady, fitCanvas, pxRatio]);

  // ── Undo / Redo / Save / Cancel ──────────────────────────────────
  const undo = useCallback(() => dispatch({ type: "undo" }), []);
  const redo = useCallback(() => dispatch({ type: "redo" }), []);
  // Bake the final PNG on a FRESH offscreen canvas (content only — no
  // selection handles / crop chrome / in-progress preview, which live on the
  // on-screen canvas) and honour the crop viewport.
  const exportDataUrl = useCallback((): string | null => {
    const img = imgRef.current;
    if (!img) return null;
    const crop = hist.present.crop;
    const off = document.createElement("canvas");
    const ctx = off.getContext("2d");
    if (!ctx) return null;
    if (crop && crop.w >= 1 && crop.h >= 1) {
      off.width = Math.round(crop.w);
      off.height = Math.round(crop.h);
      ctx.translate(-Math.round(crop.x), -Math.round(crop.y));
    } else {
      off.width = img.naturalWidth;
      off.height = img.naturalHeight;
    }
    composeScene(ctx, img, hist.present.annotations);
    return off.toDataURL("image/png");
  }, [hist.present]);
  const save = useCallback(async () => {
    if (saving) return;
    const dataUrl = exportDataUrl();
    if (!dataUrl) return;
    setSaving(true);
    try {
      await editorSave(dataUrl);
    } catch (e) {
      console.error("editor save failed", e);
    } finally {
      setSaving(false);
    }
  }, [saving, exportDataUrl]);
  const cancel = useCallback(() => {
    void editorCancel().catch(() => undefined);
  }, []);
  const copy = useCallback(async () => {
    const dataUrl = exportDataUrl();
    if (!dataUrl) return;
    try {
      await editorCopy(dataUrl);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch (e) {
      console.error("editor copy failed", e);
    }
  }, [exportDataUrl]);

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
  const applyOpacity = (o: number) => {
    setOpacity(o);
    if (selectedId) {
      const a = annById(selectedId);
      if (a) commitReplace(selectedId, restyleAnnotation(a, { opacity: o }));
    }
  };
  const applyFontSize = (n: number) => {
    setFontSize(n);
    if (selectedId) {
      const a = annById(selectedId);
      if (a) commitReplace(selectedId, restyleAnnotation(a, { fontSize: n }));
    }
  };
  const applyShadow = (on: boolean) => {
    setShadow(on);
    if (selectedId) {
      const a = annById(selectedId);
      if (a) commitReplace(selectedId, restyleAnnotation(a, { shadow: on }));
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
        else if (key === "p") setTool("pen");
        else if (key === "c") setTool("crop");
        else if (key === "s") setTool("spotlight");
        else if (key === "m") setTool("emoji");
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
    if (tool === "emoji") {
      const a: Annotation = { id: nextId(), type: "emoji", x: p.x, y: p.y, char: emoji, size: Math.max(48, fontSize), opacity, shadow };
      commitAppend(a);
      setSelectedId(a.id);
      return;
    }
    if (tool === "pen") {
      setInteraction({ kind: "pen", points: [p] });
      return;
    }
    if (tool === "crop") {
      setInteraction({ kind: "crop", start: p, current: p });
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
    } else if (interaction.kind === "pen") {
      setInteraction({ ...interaction, points: [...interaction.points, p] });
    } else if (interaction.kind === "crop") {
      setInteraction({ ...interaction, current: p });
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
    } else if (interaction.kind === "pen") {
      if (interaction.points.length > 1) {
        const a: Annotation = { id: nextId(), type: "pen", points: interaction.points, color, width: strokeWidth, opacity, shadow };
        commitAppend(a);
        setSelectedId(a.id);
      }
    } else if (interaction.kind === "crop") {
      const img = imgRef.current;
      const r = dragRect(interaction.start, interaction.current);
      if (r.w < 5 && r.h < 5) {
        if (hist.present.crop) commitCrop(null); // a click clears an existing crop
      } else if (img) {
        commitCrop(clampRectToImage(r, img.naturalWidth, img.naturalHeight));
      }
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
        <Toolbar
          tool={tool}
          setTool={setTool}
          color={color}
          setColor={applyColor}
          strokeWidth={strokeWidth}
          setStrokeWidth={applyWidth}
          opacity={opacity}
          setOpacity={applyOpacity}
          fontSize={fontSize}
          setFontSize={applyFontSize}
          shadow={shadow}
          setShadow={applyShadow}
        />
        <div
          ref={fitBoxRef}
          className="relative flex min-h-0 flex-1 items-center justify-center overflow-hidden bg-[#0f0f0f] p-4"
        >
          {imgReady ? (
            <div className="relative">
              {tool === "emoji" && <EmojiBar current={emoji} onPick={setEmoji} />}
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
  opacity,
  setOpacity,
  fontSize,
  setFontSize,
  shadow,
  setShadow,
}: {
  tool: Tool;
  setTool: (t: Tool) => void;
  color: string;
  setColor: (c: string) => void;
  strokeWidth: number;
  setStrokeWidth: (n: number) => void;
  opacity: number;
  setOpacity: (n: number) => void;
  fontSize: number;
  setFontSize: (n: number) => void;
  shadow: boolean;
  setShadow: (on: boolean) => void;
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
      <ToolBtn t="pen" icon={<PenTool size={16} />} label={t("shot.pen")} shortcut="P" active={tool} onSelect={setTool} />
      <ToolBtn t="emoji" icon={<Smile size={16} />} label={t("shot.emoji")} shortcut="M" active={tool} onSelect={setTool} />
      <ToolBtn t="spotlight" icon={<Focus size={16} />} label={t("shot.spotlight")} shortcut="S" active={tool} onSelect={setTool} />
      <ToolBtn t="crop" icon={<Crop size={16} />} label={t("shot.crop")} shortcut="C" active={tool} onSelect={setTool} />

      <div className="mt-3 grid grid-cols-2 place-items-center gap-1.5">
        {COLOR_PRESETS.map((c) => (
          <button
            key={c}
            onClick={() => setColor(c)}
            title={c}
            className={
              "h-5 w-5 rounded-full border-2 " +
              (color === c ? "border-[var(--color-accent)] scale-110" : "border-[var(--color-border)]")
            }
            style={{ backgroundColor: c }}
          />
        ))}
        {/* Custom colour — opens the native colour panel. */}
        <input
          type="color"
          value={/^#[0-9a-fA-F]{6}$/.test(color) ? color : "#ffffff"}
          onChange={(e) => setColor(e.target.value)}
          title={t("shot.custom_color")}
          className="h-5 w-5 cursor-pointer rounded-full border-2 border-dashed border-[var(--color-border)] bg-transparent p-0"
        />
      </div>

      <SliderRow label={t("shot.size")} min={2} max={16} value={strokeWidth} suffix="px" onChange={setStrokeWidth} />
      <SliderRow
        label={t("shot.opacity")}
        min={10}
        max={100}
        value={Math.round(opacity * 100)}
        suffix="%"
        onChange={(v) => setOpacity(v / 100)}
      />
      <SliderRow label={t("shot.font_size")} min={12} max={120} value={fontSize} onChange={setFontSize} />

      <button
        onClick={() => setShadow(!shadow)}
        title={t("shot.shadow")}
        className={
          "mt-3 flex h-8 w-8 items-center justify-center rounded-md border " +
          (shadow ? "border-[var(--color-accent)] bg-[var(--color-accent)]/15" : "border-[var(--color-border)] hover:bg-[var(--color-bg)]")
        }
      >
        <span
          className="h-3.5 w-3.5 rounded-[2px] bg-current"
          style={{ boxShadow: shadow ? "2px 2px 0 rgba(0,0,0,0.55)" : "none" }}
        />
      </button>
    </div>
  );
}

/** A compact labelled horizontal slider for the toolbar. */
function SliderRow({
  label,
  min,
  max,
  value,
  suffix,
  onChange,
}: {
  label: string;
  min: number;
  max: number;
  value: number;
  suffix?: string;
  onChange: (n: number) => void;
}) {
  return (
    <div className="mt-2 flex w-full flex-col items-center gap-0.5">
      <span className="text-[9px] uppercase tracking-wider text-[var(--color-muted)]">{label}</span>
      <input
        type="range"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(parseInt(e.target.value, 10))}
        className="w-11 cursor-pointer accent-[var(--color-accent)]"
      />
      <span className="text-[10px] tabular-nums text-[var(--color-muted)]">
        {value}
        {suffix ?? ""}
      </span>
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

/** Floating emoji/sticker palette, shown while the emoji tool is active. */
function EmojiBar({ current, onPick }: { current: string; onPick: (e: string) => void }) {
  return (
    <div className="absolute left-1/2 top-2 z-10 flex max-w-[92%] -translate-x-1/2 flex-wrap justify-center gap-1 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-1.5 shadow-lg">
      {EMOJIS.map((e) => (
        <button
          key={e}
          onClick={() => onPick(e)}
          className={
            "flex h-7 w-7 items-center justify-center rounded text-[18px] leading-none " +
            (current === e ? "bg-[var(--color-accent)]/25 ring-1 ring-[var(--color-accent)]" : "hover:bg-[var(--color-bg)]")
          }
        >
          {e}
        </button>
      ))}
    </div>
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
    case "pen":
      drawPen(ctx, a);
      break;
    case "emoji":
      ctx.font = `${a.size}px var(--font-sans, sans-serif)`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(a.char, a.x, a.y);
      break;
  }
  ctx.restore();
}

/** Draw the full scene CONTENT (image + annotations) — no selection chrome,
 *  no crop dim, no in-progress preview. Shared by the on-screen redraw and the
 *  offscreen export so the baked PNG is always clean. Clears in device space
 *  so it is correct under the export's crop translate. */
function composeScene(ctx: CanvasRenderingContext2D, img: HTMLImageElement, list: Annotation[]) {
  ctx.save();
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
  ctx.restore();
  ctx.drawImage(img, 0, 0);
  for (const a of list) if (a.type !== "spotlight") drawAnnotation(ctx, a, img);

  // Spotlight: a dim layer over the whole image with a transparent hole for
  // each spotlight rect, composited on top so the focused region shows at full
  // brightness. Built on its own layer (destination-out would otherwise erase
  // the content, not just the dim). This is CONTENT — it IS in the export.
  const spots = list.filter((a) => a.type === "spotlight");
  if (spots.length) {
    const layer = document.createElement("canvas");
    layer.width = img.naturalWidth;
    layer.height = img.naturalHeight;
    const lx = layer.getContext("2d");
    if (lx) {
      lx.fillStyle = "rgba(0,0,0,0.6)";
      lx.fillRect(0, 0, layer.width, layer.height);
      lx.globalCompositeOperation = "destination-out";
      for (const s of spots) if (s.type === "spotlight") lx.fillRect(s.x, s.y, s.w, s.h);
      ctx.drawImage(layer, 0, 0);
    }
  }
}

function drawPen(ctx: CanvasRenderingContext2D, a: PenAnnotation) {
  if (a.points.length === 0) return;
  if (a.points.length === 1) {
    ctx.fillStyle = a.color;
    ctx.beginPath();
    ctx.arc(a.points[0].x, a.points[0].y, a.width / 2, 0, Math.PI * 2);
    ctx.fill();
    return;
  }
  ctx.strokeStyle = a.color;
  ctx.lineWidth = a.width;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(a.points[0].x, a.points[0].y);
  for (let i = 1; i < a.points.length; i++) ctx.lineTo(a.points[i].x, a.points[i].y);
  ctx.stroke();
}

/** Dim everything outside the crop + a dashed border. Editor-only chrome —
 *  the export is cropped instead, so this is never baked in. `crop` is already
 *  normalised (dragRect / clampRectToImage). */
function drawCropChrome(ctx: CanvasRenderingContext2D, crop: Rect, W: number, H: number, px: number) {
  ctx.save();
  ctx.fillStyle = "rgba(0,0,0,0.5)";
  ctx.fillRect(0, 0, W, crop.y);
  ctx.fillRect(0, crop.y + crop.h, W, H - (crop.y + crop.h));
  ctx.fillRect(0, crop.y, crop.x, crop.h);
  ctx.fillRect(crop.x + crop.w, crop.y, W - (crop.x + crop.w), crop.h);
  ctx.strokeStyle = "#ffffff";
  ctx.lineWidth = 1.5 * px;
  ctx.setLineDash([6 * px, 4 * px]);
  ctx.strokeRect(crop.x, crop.y, crop.w, crop.h);
  ctx.setLineDash([]);
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
