/**
 * `figlet` → PNG: render the ASCII banner to a tightly-cropped image for the
 * Shift+Enter copy path. The crop is PURE (unit-tested): drop empty edge
 * lines, strip the common leading indent and all trailing whitespace, so the
 * bitmap hugs the glyphs. The canvas render (`bannerPngBase64`) is the thin
 * browser-only edge, styled with the CURRENT theme colours (WYSIWYG — the
 * image looks exactly like the preview, readable on light and dark targets).
 */

/**
 * Crop the banner text to its glyph bounding box: remove leading/trailing
 * blank lines, the common leading-space indent of the remaining lines, and
 * per-line trailing whitespace. Whitespace INSIDE the art (line-internal
 * spaces, shorter lines) is preserved verbatim. Returns `[]` for an
 * empty/whitespace-only banner.
 */
export function cropBanner(text: string): string[] {
  const raw = text.replace(/\r\n?/g, "\n").split("\n");
  let first = 0;
  let last = raw.length - 1;
  while (first <= last && raw[first].trim() === "") first++;
  while (last >= first && raw[last].trim() === "") last--;
  if (first > last) return [];
  const lines = raw.slice(first, last + 1).map((l) => l.replace(/\s+$/, ""));
  // Common leading indent across NON-EMPTY lines (an interior blank line must
  // not force the indent to 0).
  let indent = Infinity;
  for (const l of lines) {
    if (l === "") continue;
    const m = l.match(/^ */);
    indent = Math.min(indent, m ? m[0].length : 0);
  }
  if (!Number.isFinite(indent) || indent === 0) return lines;
  return lines.map((l) => l.slice(indent));
}

/** The banner's character-grid size after cropping (cols = longest line). */
export function bannerGrid(lines: string[]): { cols: number; rows: number } {
  let cols = 0;
  for (const l of lines) cols = Math.max(cols, l.length);
  return { cols, rows: lines.length };
}

/** Current theme colours for the PNG, read from the app's CSS custom
 *  properties (falls back to the dark palette when unset, e.g. in tests). */
export function themePngColors(): { fg: string; bg: string } {
  const style = getComputedStyle(document.documentElement);
  const fg = style.getPropertyValue("--color-fg").trim() || "#e5e7eb";
  const bg = style.getPropertyValue("--color-bg").trim() || "#111318";
  return { fg, bg };
}

/**
 * Render the cropped banner to an offscreen canvas and return the **base64
 * PNG body** (no `data:` prefix) — what the `figlet_copy_png` IPC expects.
 * Monospace, 2× scale for crispness, a small uniform padding so glyphs don't
 * touch the edge. Returns `null` for an empty banner or when the canvas is
 * unavailable. Browser-only (not unit-tested — the pure crop/grid above is).
 */
export function bannerPngBase64(
  text: string,
  colors: { fg: string; bg: string },
  fontSize = 14,
  scale = 2,
  pad = 10,
): string | null {
  const lines = cropBanner(text);
  if (lines.length === 0) return null;
  const { cols, rows } = bannerGrid(lines);

  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;

  const font = `${fontSize * scale}px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`;
  ctx.font = font;
  const charW = ctx.measureText("M").width;
  const lineH = Math.round(fontSize * scale * 1.2);
  canvas.width = Math.ceil(cols * charW + 2 * pad * scale);
  canvas.height = Math.ceil(rows * lineH + 2 * pad * scale);

  // Sizing the canvas resets the context — set everything again.
  ctx.font = font;
  ctx.textBaseline = "top";
  ctx.fillStyle = colors.bg;
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.fillStyle = colors.fg;
  for (let i = 0; i < lines.length; i++) {
    ctx.fillText(lines[i], pad * scale, pad * scale + i * lineH);
  }
  const dataUrl = canvas.toDataURL("image/png");
  return dataUrl.slice(dataUrl.indexOf(",") + 1);
}
