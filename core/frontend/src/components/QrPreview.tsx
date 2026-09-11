import { useEffect, useMemo, useRef, useState } from "react";
import { drawQr, qrMatrix, qrPngBase64 } from "../lib/qr";
import { qrSave } from "../lib/ipc";

/** Canvas QR preview for the `qr <text>` command. Always renders black-on-white
 *  so the code scans regardless of the app theme. */
export function QrPreview({ text }: { text: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const result = useMemo(() => {
    try {
      return { matrix: qrMatrix(text), error: null };
    } catch (e) {
      return { matrix: null, error: String(e) };
    }
  }, [text]);
  useEffect(() => {
    if (!ref.current || !result.matrix) return;
    try {
      drawQr(ref.current, text);
    } catch (e) {
      setError(String(e));
    }
  }, [text, result]);
  const save = async (format: "png" | "stl") => {
    if (busy || !result.matrix) return;
    setBusy(true);
    setError(null);
    setSaved(null);
    try {
      setSaved(
        await qrSave(
          format === "png" ? { pngB64: qrPngBase64(text) } : { matrix: result.matrix },
        ),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3">
      {result.matrix && (
        <canvas
          ref={ref}
          className="max-w-full shrink-0 border border-[var(--color-border)] bg-white"
          aria-label="QR code preview"
        />
      )}
      <div className="flex gap-2">
        {(["png", "stl"] as const).map((format) => (
          <button
            key={format}
            disabled={busy || !result.matrix}
            onClick={() => void save(format)}
            className="md3-press rounded-lg bg-[var(--color-accent)] px-3 py-2 text-xs text-[var(--color-accent-fg)] disabled:opacity-40"
          >
            Save {format.toUpperCase()}
          </button>
        ))}
      </div>
      <p className="text-xs text-[var(--color-muted)]">Saves to Downloads.</p>
      {result.matrix && (
        <p className="text-xs text-[var(--color-muted)]">
          STL: {result.matrix.length + 8} × {result.matrix.length + 8} × 2.6 mm · 1 mm
          modules. Print a light base, then switch to dark filament at 2 mm for the raised
          code. STL contains no colours. Test scanning the finished print.
        </p>
      )}
      {saved && (
        <p role="status" className="break-all text-xs">
          Saved: {saved}
        </p>
      )}
      {(result.error || error) && (
        <p role="alert" className="break-all text-xs text-red-400">
          {result.error || error}
        </p>
      )}
    </div>
  );
}
