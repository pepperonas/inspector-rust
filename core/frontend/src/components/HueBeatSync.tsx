import { Activity, Mic, Square } from "lucide-react";
import {
  discoEngine,
  useDiscoState,
  FIXED_SWATCHES,
  type DiscoMode,
} from "../lib/disco-engine";

/**
 * Beat-sync panel — a thin UI over the persistent `discoEngine` singleton
 * (`lib/disco-engine.ts`). All the audio/light work lives in the engine so it
 * keeps running after the popup is dismissed; this component only renders the
 * engine's state and forwards control. The `disco` command drives the same
 * engine, so the two stay in sync automatically (via `useSyncExternalStore`).
 */
export function HueBeatSync() {
  const st = useDiscoState();
  const showSwatches = st.mode !== "rainbow";

  return (
    <div className="mt-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
      <div className="flex items-center justify-between">
        <span className="flex items-center gap-1.5 text-[12px] font-medium">
          <Activity size={13} className="text-[var(--color-accent)]" /> Beat sync
        </span>
        <button
          type="button"
          onClick={() => void discoEngine.toggle()}
          className={
            "md3-press flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-medium " +
            (st.running
              ? "bg-rose-600 text-white"
              : "bg-[var(--color-accent)] text-[var(--color-accent-fg)]")
          }
        >
          {st.running ? <Square size={11} /> : <Mic size={11} />}
          {st.running ? "Stop" : "Listen"}
        </button>
      </div>

      {/* Live readout */}
      <div className="mt-2 flex items-center gap-3">
        <span className="tabular-nums text-[18px] font-semibold leading-none">
          {st.bpm > 0 ? st.bpm : "—"}
          <span className="ml-1 text-[10px] font-normal text-[var(--color-muted)]">BPM</span>
        </span>
        <span className="relative h-2 flex-1 overflow-hidden rounded-full bg-[var(--color-border)]">
          <span
            className="block h-full rounded-full bg-[var(--color-accent)] transition-[width] duration-100"
            style={{ width: `${Math.round(st.level * 100)}%` }}
          />
        </span>
        <span
          className="h-2.5 w-2.5 rounded-full transition-opacity"
          style={{ background: "var(--color-accent)", opacity: st.running && st.beat ? 1 : 0.2 }}
        />
      </div>

      {/* Mode picker */}
      <div className="mt-2.5 flex gap-1.5">
        {(["rainbow", "pulse", "strobe"] as DiscoMode[]).map((m) => (
          <button
            key={m}
            type="button"
            onClick={() => discoEngine.setMode(m)}
            className={
              "md3-press flex-1 rounded px-2 py-1 text-[11px] capitalize " +
              (st.mode === m
                ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                : "border border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-fg)]")
            }
          >
            {m}
          </button>
        ))}
      </div>

      {/* Fixed colour (pulse / strobe only) */}
      {showSwatches && (
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          <span className="text-[10px] text-[var(--color-muted)]">Colour</span>
          {FIXED_SWATCHES.map((hex) => (
            <button
              key={hex}
              type="button"
              aria-label={hex}
              onClick={() => discoEngine.setFixedColor(hex)}
              className={
                "h-4 w-4 rounded-full border transition-transform hover:scale-110 " +
                (st.fixedHex === hex ? "border-white ring-1 ring-white" : "border-black/20")
              }
              style={{ background: hex }}
            />
          ))}
        </div>
      )}

      {/* Sensitivity */}
      <div className="mt-2.5 flex items-center gap-2">
        <span className="text-[10px] text-[var(--color-muted)]">Sensitivity</span>
        <input
          type="range"
          min={0}
          max={1}
          step={0.05}
          value={st.sensitivity}
          onChange={(e) => discoEngine.setSensitivity(Number(e.target.value))}
          className="flex-1 accent-[var(--color-accent)]"
        />
      </div>

      {st.error ? (
        <p className="mt-2 text-[11px] text-rose-400">{st.error}</p>
      ) : (
        <p className="mt-2 text-[10px] text-[var(--color-muted)]">
          Pulses your lamps to the beat from the mic. Keeps running after you close the popup —
          stop it here or with <code className="text-[var(--color-fg)]">disco 0</code>.
        </p>
      )}
    </div>
  );
}
