import { useCallback, useEffect, useRef, useState } from "react";
import { Activity, Mic, Square } from "lucide-react";
import { BpmAnalyzer } from "../lib/bpm";
import { hueSetLight, type HueLight } from "../lib/ipc";

/**
 * Beat-reactive Hue control — the mic-driven "disco" section of the Hue panel
 * (ported in spirit from the raspi3 `disco-controller`). It reuses IR's own
 * `BpmAnalyzer` (the very analyzer the disco app was ported *from*) + the
 * `BpmDetector` mic graph: mic → highpass 30 Hz → lowpass 100 Hz → analyser →
 * energy-onset beat detection.
 *
 * On each confident beat it drives the lamps as a **round-robin chase**: punch
 * the next lamp bright (+ colour) and settle the previous one. Round-robin (one
 * lamp per write, ~≤10/s) is deliberate — the Hue **group** endpoint is rate-
 * limited to ~1 cmd/s, so flashing all lamps via group 0 would get throttled;
 * the disco app round-robins individual lamps for exactly this reason.
 *
 * Lifecycle safety: the mic + audio graph + rAF loop start only while running
 * and are torn down on stop **and on unmount** (so a dismissed overlay never
 * keeps the mic open). Lamp on/brightness is snapshotted on start and restored
 * (to warm white) on stop.
 */

type Mode = "rainbow" | "pulse" | "strobe";

/** Rainbow chase palette (vivid hues; warm-white intentionally excluded). */
const RAINBOW_HEX = [
  "#FF2D2D", "#FF8A00", "#FFE000", "#3DDC5F", "#1FD0E0", "#2E6BFF", "#FF2D9B",
];
/** Default fixed colour for pulse / strobe; pickable via the swatches. */
const FIXED_SWATCHES = ["#FF2D9B", "#2E6BFF", "#1FD0E0", "#3DDC5F", "#FFE000", "#FF8A00", "#FF2D2D", "#FFCB8E"];
const WARM_WHITE = "#FFCB8E";

const PUNCH_BRI = 100; // % on the beat
const FLOOR_RAINBOW = 22; // % the previous lamp settles to
const FLOOR_STROBE = 1; // near-dark between beats → hard blink
const MIN_CONFIDENCE = 0.15; // ignore low-confidence onsets (disco uses 0.15)
const MIN_BEAT_GAP_MS = 90; // transient double-trigger guard
const READOUT_MS = 140; // ~7 Hz display refresh (refs drive the rest)

export function HueBeatSync({ lamps }: { lamps: HueLight[] }) {
  const [running, setRunning] = useState(false);
  const [mode, setMode] = useState<Mode>("rainbow");
  const [sensitivity, setSensitivity] = useState(0.5);
  const [fixedHex, setFixedHex] = useState(FIXED_SWATCHES[0]);
  const [error, setError] = useState<string | null>(null);
  // Throttled readout for display.
  const [readout, setReadout] = useState({ bpm: 0, level: 0, confidence: 0, beat: false });

  // Live values mutated by the rAF loop without re-rendering. Synced via
  // effects (writing refs during render trips react-hooks/refs); a one-tick
  // lag is irrelevant to the audio loop.
  const lampsRef = useRef(lamps);
  const modeRef = useRef(mode);
  const fixedRef = useRef(fixedHex);
  useEffect(() => {
    lampsRef.current = lamps;
  }, [lamps]);
  useEffect(() => {
    modeRef.current = mode;
  }, [mode]);
  useEffect(() => {
    fixedRef.current = fixedHex;
  }, [fixedHex]);

  const ctxRef = useRef<AudioContext | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const rafRef = useRef<number | null>(null);
  const analyzerRef = useRef(new BpmAnalyzer());
  const rrRef = useRef(0);
  const prevIdRef = useRef<string | null>(null);
  const lastBeatRef = useRef(0);
  const palRef = useRef(0);
  const snapshotRef = useRef<{ id: string; on: boolean; brightness: number }[]>([]);
  const sensRef = useRef(sensitivity);

  // Keep the analyzer threshold in sync with the slider.
  useEffect(() => {
    sensRef.current = sensitivity;
    analyzerRef.current.setSensitivity(sensitivity);
  }, [sensitivity]);

  const writeLamp = useCallback(
    (id: string, briPct: number, hex: string | null) => {
      void hueSetLight(id, briPct > 0, briPct > 0 ? briPct : null, hex).catch(() => undefined);
    },
    [],
  );

  const onBeat = useCallback(() => {
    const ids = lampsRef.current.map((l) => l.id);
    if (ids.length === 0) return;
    const m = modeRef.current;
    const n = ids.length;
    const id = ids[rrRef.current % n];
    rrRef.current = (rrRef.current + 1) % n;

    // Colour for this beat.
    let hex: string;
    if (m === "rainbow") {
      hex = RAINBOW_HEX[palRef.current % RAINBOW_HEX.length];
      palRef.current = (palRef.current + 1) % RAINBOW_HEX.length;
    } else {
      hex = fixedRef.current;
    }

    // Punch the next lamp bright; settle the previous one.
    writeLamp(id, PUNCH_BRI, hex);
    const prev = prevIdRef.current;
    if (prev && prev !== id) {
      writeLamp(prev, m === "strobe" ? FLOOR_STROBE : FLOOR_RAINBOW, m === "rainbow" ? null : hex);
    }
    prevIdRef.current = id;
  }, [writeLamp]);

  // Snapshot current lamp state so we can restore it when the disco stops.
  const snapshot = useCallback(() => {
    snapshotRef.current = lampsRef.current.map((l) => ({
      id: l.id,
      on: l.on,
      brightness: l.brightness,
    }));
  }, []);

  const restore = useCallback(() => {
    for (const s of snapshotRef.current) {
      void hueSetLight(s.id, s.on, s.on ? s.brightness : null, s.on ? WARM_WHITE : null).catch(
        () => undefined,
      );
    }
  }, []);

  // Start / stop the mic + analysis when `running` flips.
  useEffect(() => {
    if (!running) return;
    let cancelled = false;
    snapshot();
    analyzerRef.current = new BpmAnalyzer();
    analyzerRef.current.setSensitivity(sensRef.current);
    rrRef.current = 0;
    palRef.current = 0;
    prevIdRef.current = null;

    void (async () => {
      try {
        const stream = await navigator.mediaDevices.getUserMedia({
          audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false },
        });
        if (cancelled) {
          stream.getTracks().forEach((t) => t.stop());
          return;
        }
        streamRef.current = stream;
        // "playback" latency hint → larger output buffer; avoids glitching
        // other apps' audio when the mic opens (see BpmDetector for the why).
        const ctx = new AudioContext({ latencyHint: "playback" });
        ctxRef.current = ctx;
        const source = ctx.createMediaStreamSource(stream);
        const hp = ctx.createBiquadFilter();
        hp.type = "highpass";
        hp.frequency.value = 30;
        hp.Q.value = 0.7;
        const lp = ctx.createBiquadFilter();
        lp.type = "lowpass";
        lp.frequency.value = 100;
        lp.Q.value = 1.5;
        const detect = ctx.createAnalyser();
        detect.fftSize = 1024;
        source.connect(hp);
        hp.connect(lp);
        lp.connect(detect);

        const buf = new Float32Array(detect.fftSize);
        let lastReadout = 0;
        const loop = () => {
          if (cancelled) return;
          const now = performance.now();
          detect.getFloatTimeDomainData(buf);
          analyzerRef.current.push(buf, now);
          const est = analyzerRef.current.estimate(now);
          const level = analyzerRef.current.currentEnergy();

          if (
            est.beatJustFired &&
            est.confidence >= MIN_CONFIDENCE &&
            now - lastBeatRef.current >= MIN_BEAT_GAP_MS
          ) {
            lastBeatRef.current = now;
            onBeat();
          }

          if (now - lastReadout >= READOUT_MS) {
            lastReadout = now;
            setReadout({
              bpm: Math.round(est.bpm),
              level: Math.min(1, level * 8),
              confidence: est.confidence,
              beat: now - lastBeatRef.current < 120,
            });
          }
          rafRef.current = requestAnimationFrame(loop);
        };
        rafRef.current = requestAnimationFrame(loop);
      } catch {
        if (!cancelled) {
          setError("Microphone unavailable — grant mic access in System Settings.");
          setRunning(false);
        }
      }
    })();

    return () => {
      cancelled = true;
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
      streamRef.current?.getTracks().forEach((t) => t.stop());
      streamRef.current = null;
      void ctxRef.current?.close().catch(() => undefined);
      ctxRef.current = null;
      restore();
    };
  }, [running, snapshot, restore, onBeat]);

  const swatchColor = mode === "rainbow" ? null : fixedHex;

  return (
    <div className="mt-3 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
      <div className="flex items-center justify-between">
        <span className="flex items-center gap-1.5 text-[12px] font-medium">
          <Activity size={13} className="text-[var(--color-accent)]" /> Beat sync
        </span>
        <button
          type="button"
          onClick={() => {
            setError(null);
            setRunning((r) => !r);
          }}
          className={
            "md3-press flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-medium " +
            (running
              ? "bg-rose-600 text-white"
              : "bg-[var(--color-accent)] text-[var(--color-accent-fg)]")
          }
        >
          {running ? <Square size={11} /> : <Mic size={11} />}
          {running ? "Stop" : "Listen"}
        </button>
      </div>

      {/* Live readout */}
      <div className="mt-2 flex items-center gap-3">
        <span className="tabular-nums text-[18px] font-semibold leading-none">
          {readout.bpm > 0 ? readout.bpm : "—"}
          <span className="ml-1 text-[10px] font-normal text-[var(--color-muted)]">BPM</span>
        </span>
        <span className="relative h-2 flex-1 overflow-hidden rounded-full bg-[var(--color-border)]">
          <span
            className="block h-full rounded-full bg-[var(--color-accent)] transition-[width] duration-100"
            style={{ width: `${Math.round(readout.level * 100)}%` }}
          />
        </span>
        <span
          className="h-2.5 w-2.5 rounded-full transition-opacity"
          style={{
            background: "var(--color-accent)",
            opacity: running && readout.beat ? 1 : 0.2,
          }}
        />
      </div>

      {/* Mode picker */}
      <div className="mt-2.5 flex gap-1.5">
        {(["rainbow", "pulse", "strobe"] as Mode[]).map((m) => (
          <button
            key={m}
            type="button"
            onClick={() => setMode(m)}
            className={
              "md3-press flex-1 rounded px-2 py-1 text-[11px] capitalize " +
              (mode === m
                ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                : "border border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-fg)]")
            }
          >
            {m}
          </button>
        ))}
      </div>

      {/* Fixed colour (pulse / strobe only) */}
      {swatchColor !== null && (
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          <span className="text-[10px] text-[var(--color-muted)]">Colour</span>
          {FIXED_SWATCHES.map((hex) => (
            <button
              key={hex}
              type="button"
              aria-label={hex}
              onClick={() => setFixedHex(hex)}
              className={
                "h-4 w-4 rounded-full border transition-transform hover:scale-110 " +
                (fixedHex === hex ? "border-white ring-1 ring-white" : "border-black/20")
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
          value={sensitivity}
          onChange={(e) => setSensitivity(Number(e.target.value))}
          className="flex-1 accent-[var(--color-accent)]"
        />
      </div>

      {error ? (
        <p className="mt-2 text-[11px] text-rose-400">{error}</p>
      ) : (
        <p className="mt-2 text-[10px] text-[var(--color-muted)]">
          Listens to the mic and pulses your lamps to the beat (round-robin chase). Stops — and
          restores your lamps — when you close this.
        </p>
      )}
    </div>
  );
}
