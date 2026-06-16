/**
 * Persistent beat-reactive Hue "disco" engine (v0.84.46).
 *
 * A **module-level singleton** — deliberately NOT a React component — so it
 * keeps running after the popup is dismissed (Esc / click-outside), until the
 * user explicitly stops it (Stop button or `disco 0`). The `HueBeatSync` panel
 * and the `disco` command are both just controllers over this one engine.
 *
 * Beat detection runs in a **`ScriptProcessorNode.onaudioprocess`** callback on
 * the Web Audio render thread — not a `requestAnimationFrame` loop — because a
 * hidden window throttles rAF to ~0, which would freeze detection while the
 * overlay is closed. The audio thread isn't visibility-throttled, so the lights
 * keep pulsing while the popup is hidden. (Caveat: this relies on WebKit
 * keeping the capture `AudioContext` alive while hidden — verified by the
 * "close popup, lights keep pulsing" test.)
 *
 * Lights are driven as a **round-robin chase** via `hueSetLight` (one lamp per
 * write): the Hue *group* endpoint is rate-limited to ~1 cmd/s, so flashing all
 * lamps via group 0 would get throttled — the disco-controller round-robins
 * individual lamps for exactly this reason.
 */
import { useSyncExternalStore } from "react";
import { BpmAnalyzer } from "./bpm";
import { hueListLights, hueSetLight } from "./ipc";

export type DiscoMode = "rainbow" | "pulse" | "strobe";

export interface DiscoState {
  running: boolean;
  mode: DiscoMode;
  sensitivity: number;
  fixedHex: string;
  /** Live BPM (0 = none yet). */
  bpm: number;
  /** Live level 0..1. */
  level: number;
  /** A beat fired very recently (drives the pulse dot). */
  beat: boolean;
  error: string | null;
}

/** Rainbow chase palette (vivid hues; warm-white intentionally excluded). */
export const RAINBOW_HEX = [
  "#FF2D2D", "#FF8A00", "#FFE000", "#3DDC5F", "#1FD0E0", "#2E6BFF", "#FF2D9B",
];
/** Fixed-colour swatches for pulse / strobe (shown in the panel). */
export const FIXED_SWATCHES = [
  "#FF2D9B", "#2E6BFF", "#1FD0E0", "#3DDC5F", "#FFE000", "#FF8A00", "#FF2D2D", "#FFCB8E",
];
const WARM_WHITE = "#FFCB8E";

const PUNCH_BRI = 100; // % on the beat
const FLOOR_RAINBOW = 22; // % the previous lamp settles to
const FLOOR_STROBE = 1; // near-dark between beats → hard blink
const MIN_CONFIDENCE = 0.15; // ignore low-confidence onsets (disco uses 0.15)
const MIN_BEAT_GAP_MS = 90; // transient double-trigger guard
const READOUT_MS = 140; // ~7 Hz UI refresh

class DiscoEngine {
  private state: DiscoState = {
    running: false,
    mode: "rainbow",
    sensitivity: 0.5,
    fixedHex: FIXED_SWATCHES[0],
    bpm: 0,
    level: 0,
    beat: false,
    error: null,
  };
  /** Cached immutable snapshot for `useSyncExternalStore` (stable identity
   *  between emits — recreated only when state changes). */
  private snap: DiscoState = { ...this.state };
  private listeners = new Set<() => void>();

  // Audio graph.
  private ctx: AudioContext | null = null;
  private stream: MediaStream | null = null;
  private processor: ScriptProcessorNode | null = null;
  private analyzer = new BpmAnalyzer();

  // Beat-chase bookkeeping.
  private lampIds: string[] = [];
  private lampSnapshot: { id: string; on: boolean; brightness: number }[] = [];
  private rr = 0;
  private prevId: string | null = null;
  private pal = 0;
  private lastBeat = 0;
  private lastReadout = 0;

  subscribe = (cb: () => void): (() => void) => {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  };

  getSnapshot = (): DiscoState => this.snap;

  private set(patch: Partial<DiscoState>) {
    this.state = { ...this.state, ...patch };
    this.snap = this.state;
    this.listeners.forEach((l) => l());
  }

  setMode(mode: DiscoMode) {
    this.set({ mode });
  }
  setFixedColor(fixedHex: string) {
    this.set({ fixedHex });
  }
  setSensitivity(sensitivity: number) {
    const s = Math.max(0, Math.min(1, sensitivity));
    this.analyzer.setSensitivity(s);
    this.set({ sensitivity: s });
  }

  isRunning(): boolean {
    return this.state.running;
  }

  async toggle(): Promise<void> {
    if (this.state.running) this.stop();
    else await this.start();
  }

  async start(): Promise<void> {
    if (this.state.running) return;
    this.set({ error: null });

    // Snapshot the lamps so we can restore them on stop.
    const lamps = await hueListLights().catch(() => []);
    if (lamps.length === 0) {
      this.set({ error: "No Hue lamps — connect a bridge with `hue` first." });
      return;
    }
    this.lampIds = lamps.map((l) => l.id);
    this.lampSnapshot = lamps.map((l) => ({ id: l.id, on: l.on, brightness: l.brightness }));
    this.rr = 0;
    this.pal = 0;
    this.prevId = null;

    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false },
      });
      this.stream = stream;
      // "playback" latency hint → larger output buffer; avoids glitching other
      // apps' audio while macOS reconfigures the shared device on mic-open.
      const ctx = new AudioContext({ latencyHint: "playback" });
      this.ctx = ctx;
      this.analyzer = new BpmAnalyzer();
      this.analyzer.setSensitivity(this.state.sensitivity);

      const source = ctx.createMediaStreamSource(stream);
      const hp = ctx.createBiquadFilter();
      hp.type = "highpass";
      hp.frequency.value = 30;
      hp.Q.value = 0.7;
      const lp = ctx.createBiquadFilter();
      lp.type = "lowpass";
      lp.frequency.value = 100;
      lp.Q.value = 1.5;
      // ScriptProcessorNode runs onaudioprocess on the audio thread (not
      // rAF) → keeps firing while the popup is hidden. It must reach
      // destination to be pulled, so route it through a muted gain.
      // eslint-disable-next-line @typescript-eslint/no-deprecated
      const processor = ctx.createScriptProcessor(1024, 1, 1);
      const silent = ctx.createGain();
      silent.gain.value = 0;
      source.connect(hp);
      hp.connect(lp);
      lp.connect(processor);
      processor.connect(silent);
      silent.connect(ctx.destination);
      this.processor = processor;

      processor.onaudioprocess = (e) => {
        const now = performance.now();
        // The input is the HP+LP-filtered bass band — exactly what BpmAnalyzer
        // expects. Copy out (the buffer is reused across callbacks).
        const samples = e.inputBuffer.getChannelData(0);
        this.analyzer.push(samples as Float32Array, now);
        const est = this.analyzer.estimate(now);
        const level = this.analyzer.currentEnergy();

        if (
          est.beatJustFired &&
          est.confidence >= MIN_CONFIDENCE &&
          now - this.lastBeat >= MIN_BEAT_GAP_MS
        ) {
          this.lastBeat = now;
          this.onBeat();
        }
        if (now - this.lastReadout >= READOUT_MS) {
          this.lastReadout = now;
          this.set({
            bpm: Math.round(est.bpm),
            level: Math.min(1, level * 8),
            beat: now - this.lastBeat < 120,
          });
        }
      };

      // Created from a user gesture so it should auto-run, but resume defensively
      // (a suspended context never fires onaudioprocess).
      void ctx.resume().catch(() => undefined);
      this.set({ running: true, error: null });
    } catch {
      this.teardownAudio();
      this.set({ error: "Microphone unavailable — grant mic access in System Settings." });
    }
  }

  stop(): void {
    if (!this.state.running && !this.ctx) return;
    this.teardownAudio();
    this.restoreLamps();
    this.set({ running: false, bpm: 0, level: 0, beat: false });
  }

  private teardownAudio() {
    if (this.processor) {
      this.processor.onaudioprocess = null;
      this.processor.disconnect();
      this.processor = null;
    }
    this.stream?.getTracks().forEach((t) => t.stop());
    this.stream = null;
    void this.ctx?.close().catch(() => undefined);
    this.ctx = null;
  }

  private restoreLamps() {
    for (const s of this.lampSnapshot) {
      void hueSetLight(s.id, s.on, s.on ? s.brightness : null, s.on ? WARM_WHITE : null).catch(
        () => undefined,
      );
    }
  }

  private onBeat() {
    const ids = this.lampIds;
    const n = ids.length;
    if (n === 0) return;
    const id = ids[this.rr % n];
    this.rr = (this.rr + 1) % n;

    const m = this.state.mode;
    let hex: string;
    if (m === "rainbow") {
      hex = RAINBOW_HEX[this.pal % RAINBOW_HEX.length];
      this.pal = (this.pal + 1) % RAINBOW_HEX.length;
    } else {
      hex = this.state.fixedHex;
    }

    void hueSetLight(id, true, PUNCH_BRI, hex).catch(() => undefined);
    const prev = this.prevId;
    if (prev && prev !== id) {
      const floor = m === "strobe" ? FLOOR_STROBE : FLOOR_RAINBOW;
      void hueSetLight(prev, true, floor, m === "rainbow" ? null : hex).catch(() => undefined);
    }
    this.prevId = id;
  }
}

/** The one process-wide disco engine. */
export const discoEngine = new DiscoEngine();

/** Subscribe a component to the engine's state (re-renders on change). */
export function useDiscoState(): DiscoState {
  return useSyncExternalStore(discoEngine.subscribe, discoEngine.getSnapshot);
}
