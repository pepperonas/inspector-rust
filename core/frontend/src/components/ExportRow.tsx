import { Download } from "lucide-react";

export type ExportFormat = "html" | "pdf" | "png" | "csv";

/**
 * The export chips shared by every report panel (`loc`, `repo`, …).
 *
 * One row, one component: the formats differ per report but the affordance
 * must not. It lived inside `LocPanel` until `repo` and the timesheet gained
 * PDF too — copying it would have been the third place to keep in step.
 */
export function ExportRow<F extends ExportFormat>({
  formats,
  busy,
  done,
  onExport,
  label = "Export:",
}: {
  // Generic over the union the caller actually offers, so `onExport` hands
  // back exactly those — a plain `ExportFormat` would force every panel to
  // re-narrow a format it never displayed.
  formats: readonly F[];
  busy: string | null;
  done: string | null;
  onExport: (fmt: F) => void;
  label?: string;
}) {
  return (
    <div className="flex flex-wrap items-center gap-1.5 text-[11px]">
      <Download size={12} className="shrink-0 text-[var(--color-muted)]" />
      <span className="text-[var(--color-muted)]">{label}</span>
      {formats.map((f) => (
        <button
          key={f}
          type="button"
          disabled={busy !== null}
          onClick={() => onExport(f)}
          className="rounded-full border border-[var(--color-border)] px-2 py-0.5 uppercase tracking-wide hover:border-[var(--color-accent)] hover:text-[var(--color-accent)] disabled:opacity-40"
        >
          {busy === f ? "…" : f}
        </button>
      ))}
      {done && <span className="truncate text-[var(--color-muted)]">→ {done}</span>}
    </div>
  );
}
