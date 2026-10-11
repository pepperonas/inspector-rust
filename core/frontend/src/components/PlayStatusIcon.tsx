/**
 * Play-status badge for the Spotify history.
 *
 *  - listened: green disc, a bold arrow that runs into an end stop ("to the end")
 *  - partial:  orange tilde inside a ring that shows how much was heard
 *  - skipped:  rose double chevron
 *  - playing:  green equalizer bars (animated; still under reduced motion)
 *  - paused:   slate pause bars (static — nothing is playing)
 *
 * Pure inline SVG, colours from fixed hues so the meaning is the same in
 * light and dark themes; the text label next to it stays for screen readers.
 */
import { STATUS_LABEL, playedFraction } from "../lib/spotify";
import type { SpotifyPlayRow } from "../lib/ipc";

export const STATUS_COLOR: Record<SpotifyPlayRow["status"], string> = {
  listened: "#10b981",
  partial: "#f97316",
  skipped: "#f43f5e",
  playing: "#10b981",
  paused: "#64748b",
};

export function PlayStatusIcon({
  row,
  size = 26,
}: {
  row: Pick<SpotifyPlayRow, "status" | "played_ms" | "duration_ms">;
  size?: number;
}) {
  const color = STATUS_COLOR[row.status];
  const frac = playedFraction(row);
  const label =
    row.status === "partial" && frac != null
      ? `${STATUS_LABEL.partial} · ${Math.round(frac * 100)} %`
      : STATUS_LABEL[row.status];
  // Tooltip via the wrapper, not an SVG <title>: a <title> would repeat the
  // status text next to the visible label (screen readers already get aria-label).
  return (
    <span title={label} className="inline-flex shrink-0">
      <svg
        viewBox="0 0 24 24"
        width={size}
        height={size}
        role="img"
        aria-label={label}
        data-status={row.status}
        data-testid="play-status-icon"
        className="shrink-0"
      >
        <circle cx={12} cy={12} r={11} fill={color} fillOpacity={0.16} />
        {row.status === "listened" && (
          <g
            stroke={color}
            strokeWidth={3}
            strokeLinecap="round"
            strokeLinejoin="round"
            fill="none"
          >
            <circle cx={12} cy={12} r={10} strokeWidth={1.6} />
            <path d="M6.5 12h8" />
            <path d="M11.5 8l4 4-4 4" />
            <path d="M18 7.5v9" />
          </g>
        )}
        {row.status === "partial" && (
          <g fill="none" stroke={color} strokeLinecap="round">
            <circle cx={12} cy={12} r={10} strokeWidth={1.6} strokeOpacity={0.25} />
            {frac != null && frac > 0 && (
              <circle
                cx={12}
                cy={12}
                r={10}
                strokeWidth={1.8}
                pathLength={1}
                strokeDasharray={`${frac} 1`}
                transform="rotate(-90 12 12)"
                data-testid="play-status-progress"
              />
            )}
            <path
              d="M6.5 13.2c1.3-2.2 2.9-2.3 4.4-.9s3.1 1.3 4.6-.9"
              strokeWidth={2.8}
              strokeLinejoin="round"
            />
          </g>
        )}
        {row.status === "skipped" && (
          <g
            stroke={color}
            strokeWidth={2.6}
            strokeLinecap="round"
            strokeLinejoin="round"
            fill="none"
          >
            <path d="M7 8l4 4-4 4" />
            <path d="M12.5 8l4 4-4 4" />
          </g>
        )}
        {row.status === "paused" && (
          <g fill={color}>
            <rect x={7.5} y={6.5} width={3.4} height={11} rx={1.2} />
            <rect x={13.1} y={6.5} width={3.4} height={11} rx={1.2} />
          </g>
        )}
        {row.status === "playing" && (
          <g fill={color}>
            <rect
              className="eq-bar"
              x={6.5}
              y={7}
              width={2.6}
              height={10}
              rx={1.3}
              style={{ animationDelay: "0ms" }}
            />
            <rect
              className="eq-bar"
              x={10.7}
              y={7}
              width={2.6}
              height={10}
              rx={1.3}
              style={{ animationDelay: "-260ms" }}
            />
            <rect
              className="eq-bar"
              x={14.9}
              y={7}
              width={2.6}
              height={10}
              rx={1.3}
              style={{ animationDelay: "-520ms" }}
            />
          </g>
        )}
      </svg>
    </span>
  );
}
