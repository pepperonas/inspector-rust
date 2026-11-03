import { Video } from "lucide-react";

/**
 * A record project's preview picture (the editor writes it from its filmstrip); without one a
 * neutral placeholder of the same size, so rows never jump when a thumbnail appears.
 */
export function ProjectThumb({ src, className = "h-9 w-16" }: { src?: string | null; className?: string }) {
  return src ? (
    <img src={src} alt="" draggable={false} className={`${className} shrink-0 rounded object-cover`} data-testid="project-thumb" />
  ) : (
    <span
      className={`${className} flex shrink-0 items-center justify-center rounded border border-[var(--color-border)] bg-[var(--color-bg)] text-[var(--color-muted)]`}
      data-testid="project-thumb-empty"
      aria-hidden
    >
      <Video size={12} />
    </span>
  );
}
