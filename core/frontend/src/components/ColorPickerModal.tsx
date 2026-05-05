import { useEffect, useMemo, useRef, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy, X } from "lucide-react";
import {
  hsvToRgb,
  readableForeground,
  rgbToHex,
  rgbToHsl,
  rgbToHsv,
  tryParseColor,
} from "../lib/colors";

interface Props {
  open: boolean;
  onClose: () => void;
}

type Format = "hex" | "rgb" | "hsl";

/**
 * Cross-platform custom color picker.
 *
 * Why we don't use `<input type="color">`: WKWebView (Tauri's renderer
 * on macOS) doesn't reliably show the OS picker for hidden inputs, and
 * even when it does, the async `change` event fires *outside* the
 * user-gesture context, so `navigator.clipboard.writeText` can be
 * blocked. A custom modal sidesteps both — the picker UI lives entirely
 * in the WebView, the clipboard write goes through Tauri's
 * `plugin-clipboard-manager` (no browser-API restrictions).
 *
 * Internally we keep state in HSV because that maps cleanly to a
 * standard 2D saturation/value picker + a 1D hue slider. Output formats
 * (HEX, RGB, HSL) are derived on demand.
 */
export function ColorPickerModal({ open, onClose }: Props) {
  // Default to a pleasant blue.
  const [hue, setHue] = useState(220);
  const [sat, setSat] = useState(80);
  const [val, setVal] = useState(95);
  const [hexInput, setHexInput] = useState("#3366FF");
  const [hexInputValid, setHexInputValid] = useState(true);
  const [format, setFormat] = useState<Format>("hex");
  const [copied, setCopied] = useState(false);

  // Re-sync the hex input whenever the HSV sliders move.
  useEffect(() => {
    const [r, g, b] = hsvToRgb(hue, sat, val);
    setHexInput(rgbToHex(r, g, b));
    setHexInputValid(true);
  }, [hue, sat, val]);

  // Reset the "Copied!" feedback when the user moves the sliders.
  useEffect(() => {
    setCopied(false);
  }, [hue, sat, val, format]);

  // Close on Escape.
  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, [open, onClose]);

  const [r, g, b] = useMemo(() => hsvToRgb(hue, sat, val), [hue, sat, val]);
  const hsl = useMemo(() => rgbToHsl(r, g, b), [r, g, b]);
  const hex = useMemo(() => rgbToHex(r, g, b), [r, g, b]);
  const fg = useMemo(() => readableForeground(r, g, b), [r, g, b]);

  const outputs: Record<Format, string> = useMemo(
    () => ({
      hex,
      rgb: `rgb(${r}, ${g}, ${b})`,
      hsl: `hsl(${hsl.h}, ${hsl.s}%, ${hsl.l}%)`,
    }),
    [hex, r, g, b, hsl],
  );

  const onHexInputChange = (value: string) => {
    setHexInput(value);
    const parsed = tryParseColor(value);
    if (parsed) {
      const [hh, ss, vv] = rgbToHsv(parsed.r, parsed.g, parsed.b);
      setHue(hh);
      setSat(ss);
      setVal(vv);
      setHexInputValid(true);
    } else {
      setHexInputValid(false);
    }
  };

  const onCopy = async () => {
    try {
      await writeText(outputs[format]);
      setCopied(true);
    } catch (err) {
      console.error("clipboard write failed", err);
    }
  };

  if (!open) return null;

  return (
    <div
      onClick={(e) => {
        // Click on the backdrop (not on the modal content) closes.
        if (e.target === e.currentTarget) onClose();
      }}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
    >
      <div className="w-[420px] max-w-[92vw] rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] p-4 shadow-2xl">
        {/* Header */}
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-[14px] font-semibold">Color picker</h2>
          <button
            onClick={onClose}
            className="rounded p-1 text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]"
            title="Close (Esc)"
          >
            <X size={14} />
          </button>
        </div>

        {/* Saturation/Value 2D picker */}
        <SVPicker
          hue={hue}
          sat={sat}
          val={val}
          onChange={(s, v) => {
            setSat(s);
            setVal(v);
          }}
        />

        {/* Hue slider */}
        <div className="mt-3">
          <HueSlider hue={hue} onChange={setHue} />
        </div>

        {/* Big preview swatch */}
        <div
          className="mt-3 flex h-16 items-center justify-center rounded border border-[var(--color-border)] font-[var(--font-mono)] text-[16px] font-semibold tracking-wide"
          style={{ backgroundColor: hex, color: fg }}
        >
          {hex}
        </div>

        {/* Hex input */}
        <div className="mt-3 flex items-center gap-2">
          <label className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
            Hex
          </label>
          <input
            type="text"
            value={hexInput}
            onChange={(e) => onHexInputChange(e.target.value)}
            spellCheck={false}
            autoComplete="off"
            autoCorrect="off"
            autoCapitalize="off"
            className={
              "flex-1 rounded border bg-[var(--color-surface)] px-2 py-1 font-[var(--font-mono)] text-[13px] outline-none " +
              (hexInputValid
                ? "border-[var(--color-border)] focus:border-[var(--color-accent)]"
                : "border-red-500 focus:border-red-500")
            }
            placeholder="#3366FF"
          />
        </div>

        {/* Format tabs + output */}
        <div className="mt-3 flex items-center gap-1">
          {(["hex", "rgb", "hsl"] as const).map((f) => (
            <button
              key={f}
              onClick={() => setFormat(f)}
              className={
                "rounded px-2.5 py-1 text-[11px] font-medium uppercase tracking-wide " +
                (format === f
                  ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                  : "text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]")
              }
            >
              {f}
            </button>
          ))}
          <code className="ml-2 flex-1 truncate rounded bg-[var(--color-surface)] px-2 py-1 font-[var(--font-mono)] text-[12px]">
            {outputs[format]}
          </code>
        </div>

        {/* Action buttons */}
        <div className="mt-4 flex items-center justify-end gap-2">
          <button
            onClick={onClose}
            className="rounded px-3 py-1 text-[12px] text-[var(--color-muted)] hover:bg-[var(--color-surface)]"
          >
            Close
          </button>
          <button
            onClick={() => void onCopy()}
            className={
              "flex items-center gap-1.5 rounded px-3 py-1 text-[12px] font-medium " +
              (copied
                ? "bg-emerald-500 text-white"
                : "bg-[var(--color-accent)] text-[var(--color-accent-fg)] hover:opacity-90")
            }
          >
            {copied ? <Check size={12} /> : <Copy size={12} />}
            {copied ? "Copied!" : `Copy ${format.toUpperCase()}`}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── 2D Saturation / Value picker ───────────────────────────────────────────

function SVPicker({
  hue,
  sat,
  val,
  onChange,
}: {
  hue: number;
  sat: number;
  val: number;
  onChange: (sat: number, val: number) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState(false);

  const updateFromEvent = (clientX: number, clientY: number) => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const x = Math.max(0, Math.min(rect.width, clientX - rect.left));
    const y = Math.max(0, Math.min(rect.height, clientY - rect.top));
    const newSat = (x / rect.width) * 100;
    const newVal = (1 - y / rect.height) * 100;
    onChange(newSat, newVal);
  };

  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => updateFromEvent(e.clientX, e.clientY);
    const onUp = () => setDragging(false);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dragging]);

  return (
    <div
      ref={ref}
      onMouseDown={(e) => {
        setDragging(true);
        updateFromEvent(e.clientX, e.clientY);
      }}
      className="relative h-44 w-full cursor-crosshair select-none overflow-hidden rounded border border-[var(--color-border)]"
      style={{ backgroundColor: `hsl(${hue}, 100%, 50%)` }}
    >
      {/* White → transparent (left → right) for saturation */}
      <div
        className="pointer-events-none absolute inset-0"
        style={{ background: "linear-gradient(to right, #fff, rgba(255,255,255,0))" }}
      />
      {/* Black overlay (top → bottom) for value */}
      <div
        className="pointer-events-none absolute inset-0"
        style={{ background: "linear-gradient(to top, #000, rgba(0,0,0,0))" }}
      />
      {/* Crosshair indicator */}
      <div
        className="pointer-events-none absolute h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-[0_0_0_1px_rgba(0,0,0,0.5)]"
        style={{ left: `${sat}%`, top: `${100 - val}%` }}
      />
    </div>
  );
}

// ── Hue slider ──────────────────────────────────────────────────────────────

function HueSlider({ hue, onChange }: { hue: number; onChange: (hue: number) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState(false);

  const updateFromEvent = (clientX: number) => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const x = Math.max(0, Math.min(rect.width, clientX - rect.left));
    onChange((x / rect.width) * 360);
  };

  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => updateFromEvent(e.clientX);
    const onUp = () => setDragging(false);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dragging]);

  return (
    <div
      ref={ref}
      onMouseDown={(e) => {
        setDragging(true);
        updateFromEvent(e.clientX);
      }}
      className="relative h-3 w-full cursor-pointer select-none rounded border border-[var(--color-border)]"
      style={{
        background:
          "linear-gradient(to right, #f00 0%, #ff0 17%, #0f0 33%, #0ff 50%, #00f 67%, #f0f 83%, #f00 100%)",
      }}
    >
      <div
        className="pointer-events-none absolute top-1/2 h-4 w-1 -translate-x-1/2 -translate-y-1/2 rounded-sm border border-black/40 bg-white shadow"
        style={{ left: `${(hue / 360) * 100}%` }}
      />
    </div>
  );
}
