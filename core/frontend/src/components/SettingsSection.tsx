import type React from "react";
import { Zap } from "lucide-react";

// The two building blocks every Settings section is made of. Shared so a
// section can live in its own file (LanguageSection, …) without importing
// SettingsPanel (which imports the section — a cycle).

export function Section({

  icon,
  title,
  subtitle,
  id,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  subtitle?: string;
  /** Deep-link anchor for the `settings <section>` command (settings-<id>). */
  id?: string;
  children: React.ReactNode;
}) {
  return (
    <div id={id ? `settings-${id}` : undefined} className="scroll-mt-12 rounded-lg">
      <div className="mb-1 flex items-center gap-2">
        {icon}
        <h2 className="text-[14px] font-semibold">{title}</h2>
      </div>
      {subtitle && (
        <p className="mb-4 text-[12px] text-[var(--color-muted)]">{subtitle}</p>
      )}
      <div className="rounded-lg border border-[var(--color-border)] p-4">{children}</div>
    </div>
  );
}

export function Row({
  label,
  help,
  children,
}: {
  label: string;
  help?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="mb-4 last:mb-0">
      <div className="mb-1 flex items-center gap-2 text-[12px] font-medium">
        <Zap size={11} className="text-[var(--color-accent)] opacity-70" />
        <span>{label}</span>
      </div>
      <div>{children}</div>
      {help && (
        <p className="mt-1 text-[11px] leading-snug text-[var(--color-muted)]">{help}</p>
      )}
    </div>
  );
}
