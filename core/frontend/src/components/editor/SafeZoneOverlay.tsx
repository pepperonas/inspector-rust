import { safeRect, type SafeZone } from "../../lib/safe-zones";

/**
 * The parts of an upright video a platform's interface covers, shaded over the preview.
 * Editor only — the scene never contains it. Never takes the pointer.
 */
export function SafeZoneOverlay({ zone, label }: { zone: SafeZone; label: string }) {
  const r = safeRect(zone);
  return (
    <div data-testid="safe-zone" aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden">
      <div
        className="absolute"
        style={{ ...r, border: "1px dashed rgba(255,255,255,0.8)", boxShadow: "0 0 0 9999px rgba(0,0,0,0.38)", borderRadius: 2 }}
      />
      <span
        className="absolute rounded font-medium"
        style={{ left: 6, top: 6, padding: "2px 6px", fontSize: 10, color: "#fff", background: "rgba(0,0,0,0.6)" }}
      >
        {label}
      </span>
    </div>
  );
}
