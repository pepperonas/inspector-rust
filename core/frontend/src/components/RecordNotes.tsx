import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { getRecordNotes, recordCloseNotes, setRecordNotes, type RecordNotes as Notes } from "../lib/ipc";
import { useT } from "../lib/i18n";

/** Teleprompter notes (window `record-notes`): text, size, optional auto-scroll. Never captured. */
export function RecordNotes() {
  const t = useT();
  const [notes, setNotes] = useState<Notes>({ text: "", font: 28, speed: 0 });
  const [loaded, setLoaded] = useState(false);
  const area = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    void getRecordNotes()
      .then((n) => setNotes(n))
      .catch(() => undefined)
      .finally(() => setLoaded(true));
  }, []);

  // Persist (debounced) once the stored value has been read.
  useEffect(() => {
    if (!loaded) return;
    const id = window.setTimeout(() => void setRecordNotes(notes).catch(() => undefined), 400);
    return () => window.clearTimeout(id);
  }, [notes, loaded]);

  // Auto-scroll: speed 1–100 → 4–120 px/s.
  useEffect(() => {
    if (notes.speed <= 0) return;
    const pxPerSec = 4 + (notes.speed / 100) * 116;
    let last = performance.now();
    let acc = 0;
    const id = window.setInterval(() => {
      const el = area.current;
      const n = performance.now();
      acc += ((n - last) / 1000) * pxPerSec;
      last = n;
      if (el && acc >= 1) {
        el.scrollTop += Math.floor(acc);
        acc -= Math.floor(acc);
      }
    }, 50);
    return () => window.clearInterval(id);
  }, [notes.speed]);

  const close = () => void recordCloseNotes().catch(() => undefined);

  return (
    <div
      className="flex h-screen w-screen flex-col gap-2 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-[var(--color-fg)] shadow-2xl"
      onKeyDown={(e) => e.key === "Escape" && close()}
    >
      <div data-tauri-drag-region className="flex items-center justify-between">
        <span className="text-[12px] font-semibold uppercase tracking-wide text-[var(--color-muted)]">
          {t("rec2.notes.title")}
        </span>
        <button
          onClick={close}
          aria-label={t("rec2.notes.close")}
          className="rounded-md p-1 text-[var(--color-muted)] hover:bg-[var(--color-bg)] hover:text-[var(--color-fg)]"
        >
          <X size={14} />
        </button>
      </div>
      <textarea
        ref={area}
        value={notes.text}
        onChange={(e) => setNotes({ ...notes, text: e.target.value })}
        placeholder={t("rec2.notes.placeholder")}
        style={{ fontSize: notes.font, lineHeight: 1.35 }}
        className="min-h-0 flex-1 resize-none rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] p-2 outline-none"
      />
      <div className="flex items-center gap-3 text-[11px] text-[var(--color-muted)]">
        <label className="flex flex-1 items-center gap-2">
          {t("rec2.notes.font")}
          <input
            type="range"
            min={12}
            max={96}
            value={notes.font}
            onChange={(e) => setNotes({ ...notes, font: Number(e.target.value) })}
            className="flex-1"
          />
        </label>
        <label className="flex flex-1 items-center gap-2">
          {t("rec2.notes.speed")}
          <input
            type="range"
            min={0}
            max={100}
            value={notes.speed}
            onChange={(e) => setNotes({ ...notes, speed: Number(e.target.value) })}
            className="flex-1"
            title={notes.speed === 0 ? t("rec2.notes.speed_off") : String(notes.speed)}
          />
        </label>
      </div>
    </div>
  );
}
