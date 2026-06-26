/**
 * `snap-overlay` window — the window-snapping zone preview. The whole window is
 * positioned/sized over the target zone by the Rust drag monitor; this just
 * fills it with a white-bordered, lightly-translucent panel. Click-through
 * (`set_ignore_cursor_events` on the Rust side) so it never interferes with the
 * drag. A soft entrance so it reads as a deliberate snap target.
 */
export function SnapOverlay() {
  return (
    <div
      className="snap-overlay-fill h-screen w-screen box-border"
      style={{
        border: "2.5px solid rgba(255,255,255,0.95)",
        borderRadius: 10,
        background:
          "color-mix(in srgb, var(--color-accent) 26%, rgba(255,255,255,0.10))",
        boxShadow: "inset 0 0 0 1px rgba(0,0,0,0.18)",
      }}
    />
  );
}
