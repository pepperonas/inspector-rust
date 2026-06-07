/**
 * QR-code helpers for the `qr <text>` command. `qrMatrix` is the pure,
 * unit-tested core (a square boolean grid of dark/light modules); the canvas
 * helpers render it for the preview / clipboard PNG and are browser-only.
 */
import qrcode from "qrcode-generator";

/** Build the QR module matrix for `text` (auto-sized, error-correction M).
 *  `matrix[r][c] === true` means a dark module. Throws if `text` is empty. */
export function qrMatrix(text: string): boolean[][] {
  if (!text) throw new Error("qr: empty text");
  const qr = qrcode(0, "M"); // typeNumber 0 = auto-fit
  qr.addData(text);
  qr.make();
  const n = qr.getModuleCount();
  const rows: boolean[][] = [];
  for (let r = 0; r < n; r++) {
    const row: boolean[] = [];
    for (let c = 0; c < n; c++) row.push(qr.isDark(r, c));
    rows.push(row);
  }
  return rows;
}

/** Draw a QR matrix onto a canvas at `scale` px per module with a quiet-zone
 *  `margin` (in modules). Sizes the canvas to fit. Browser-only. */
export function drawQr(
  canvas: HTMLCanvasElement,
  text: string,
  scale = 6,
  margin = 4,
  dark = "#000000",
  light = "#ffffff",
): void {
  const m = qrMatrix(text);
  const n = m.length;
  const size = (n + margin * 2) * scale;
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.fillStyle = light;
  ctx.fillRect(0, 0, size, size);
  ctx.fillStyle = dark;
  for (let r = 0; r < n; r++) {
    for (let c = 0; c < n; c++) {
      if (m[r][c]) {
        ctx.fillRect((c + margin) * scale, (r + margin) * scale, scale, scale);
      }
    }
  }
}

/** Render the QR to an offscreen canvas and return the **base64 PNG** body
 *  (no `data:` prefix) — what the `qr_copy_png` IPC expects. Browser-only. */
export function qrPngBase64(text: string, scale = 8, margin = 4): string {
  const canvas = document.createElement("canvas");
  // Always black-on-white so the QR scans regardless of the app theme.
  drawQr(canvas, text, scale, margin, "#000000", "#ffffff");
  const dataUrl = canvas.toDataURL("image/png");
  return dataUrl.slice(dataUrl.indexOf(",") + 1);
}
