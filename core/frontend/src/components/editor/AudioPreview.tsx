import { useCallback, useEffect, useRef } from "react";
import type { EditorSourcePos, RecordClickHit, RecordTrackSpan } from "../../lib/ipc";
import { clickVolume, dueClickHits } from "../../lib/editor";
import { dbToGain, entryAt, fileMsAt, gainAt, limiterCurve } from "../../lib/audio-mix";

/** Resync when an element is this far from where the playhead says it should be. */
const DRIFT_MS = 250;
/** Click hits are scheduled this far ahead of the reported playhead (it arrives every ~66 ms). */
export const CLICK_LOOKAHEAD_MS = 120;
/** A playhead step larger than this (or backwards) is a seek: nothing in between sounds. */
export const CLICK_JUMP_MS = 1000;

export interface ClickPreview {
  hits: RecordClickHit[];
  downSrc: string | null;
  upSrc: string | null;
  /** The user's click volume (dB), applied like the export. */
  gainDb: number;
  /** Click sound on (and a pointer to click with). */
  on: boolean;
}

/** A music/voiceover file the preview plays (`index` into `doc.audioTracks`). */
export interface PreviewTrack {
  index: number;
  src: string;
}

/**
 * The preview's mix: every element goes through its own gain into one limiter at the
 * compositor's ceiling (WebAudio, output only — the microphone is never opened here), so
 * levels above 0 dB are heard. Without WebAudio each element's volume is used, capped at 1.
 */
class Mix {
  private readonly nodes = new WeakMap<HTMLMediaElement, GainNode>();
  private constructor(
    readonly ctx: AudioContext,
    private readonly out: AudioNode,
  ) {}

  static create(): Mix | null {
    const Ctx: typeof AudioContext | undefined =
      typeof window === "undefined" ? undefined : (window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext);
    if (!Ctx) return null;
    try {
      const ctx = new Ctx({ latencyHint: "playback" });
      const limiter = ctx.createWaveShaper();
      limiter.curve = limiterCurve() as Float32Array<ArrayBuffer>;
      limiter.oversample = "4x";
      limiter.connect(ctx.destination);
      return new Mix(ctx, limiter);
    } catch {
      return null;
    }
  }

  /** Routes `el` into the mix (once) and sets its level; false: it plays on its own. */
  set(el: HTMLMediaElement, gain: number): boolean {
    let g = this.nodes.get(el);
    if (!g) {
      try {
        const src = this.ctx.createMediaElementSource(el);
        g = this.ctx.createGain();
        src.connect(g);
        g.connect(this.out);
        this.nodes.set(el, g);
        el.volume = 1;
      } catch {
        return false;
      }
    }
    g.gain.setTargetAtTime(gain, this.ctx.currentTime, 0.015);
    return true;
  }

  close() {
    void this.ctx.close?.().catch(() => undefined);
  }
}

/**
 * Sound of the preview: system audio (from `screen.mp4`) and the microphone (`mic.wav`)
 * as two `<audio>` elements, cued from the source position and speed Rust reports with the
 * playhead (`EditorPlayback.source/speed`) — on a clip or speed change and whenever they
 * drift; `playbackRate` follows the speed. Music and voiceover tracks play from Rust's
 * programme entries (`EditorView.tracks`, never re-projected here) at 1×, each at its entry's
 * level and fades. All levels go through the mix (`Mix`); `duckDb` lowers everything (a
 * voiceover take runs).
 */
export function AudioPreview({
  screenSrc,
  micSrc,
  micOffsetMs,
  micGainDb,
  playing,
  source,
  speed,
  positionMs = 0,
  clicks = null,
  systemGainDb = 0,
  tracks = [],
  spans = [],
  duckDb = 0,
}: {
  screenSrc: string | null;
  micSrc: string | null;
  micOffsetMs: number;
  micGainDb: number;
  playing: boolean;
  source: EditorSourcePos | null;
  speed: number;
  /** Programme ms of the playhead (the click sound's clock). */
  positionMs?: number;
  clicks?: ClickPreview | null;
  /** Level of the recorded system sound (dB, ±12). */
  systemGainDb?: number;
  /** The track files (`index` into `doc.audioTracks`). */
  tracks?: PreviewTrack[];
  /** Rust's programme entries of every track (`EditorView.tracks`). */
  spans?: RecordTrackSpan[];
  /** Everything this much quieter (dB, ≤ 0). */
  duckDb?: number;
}) {
  const sys = useRef<HTMLAudioElement>(null);
  const mic = useRef<HTMLAudioElement>(null);
  const trackEls = useRef(new Map<number, HTMLAudioElement>());
  const last = useRef<{ clip: number; speed: number } | null>(null);
  const mix = useRef<Mix | null>(null);
  useEffect(() => () => mix.current?.close(), []);

  /** Sets one element's level through the mix (or its volume, capped at 1). */
  const level = useCallback((el: HTMLMediaElement | null, gain: number) => {
    if (!el) return;
    const g = Math.max(0, gain);
    if (!mix.current?.set(el, g)) el.volume = Math.min(1, g);
  }, []);

  // The mix starts with the first play (a user gesture), never before.
  useEffect(() => {
    if (!playing) return;
    mix.current ??= Mix.create();
    void mix.current?.ctx.resume?.().catch(() => undefined);
  }, [playing]);

  const duck = dbToGain(Math.min(0, duckDb));
  useEffect(() => {
    level(sys.current, dbToGain(systemGainDb) * duck);
    level(mic.current, dbToGain(micGainDb) * duck);
  }, [systemGainDb, micGainDb, duck, playing, level, screenSrc, micSrc]);

  // Tracks: each file follows its entry under the playhead (seek on a new entry or drift).
  const trackLast = useRef(new Map<number, string>());
  useEffect(() => {
    for (const [index, el] of trackEls.current) {
      const entry = playing ? entryAt(spans, index, positionMs) : null;
      if (!entry || entry.muted) {
        trackLast.current.delete(index);
        el.pause();
        continue;
      }
      level(el, gainAt(entry, positionMs) * duck);
      const key = `${entry.repeat}:${entry.progStartMs}`;
      const want = fileMsAt(entry, positionMs) / 1000;
      if (trackLast.current.get(index) !== key || Math.abs(el.currentTime - want) * 1000 > DRIFT_MS) {
        try {
          el.currentTime = want;
        } catch {
          /* not seekable yet */
        }
      }
      trackLast.current.set(index, key);
      el.playbackRate = 1;
      if (el.paused) void el.play?.()?.catch(() => undefined);
    }
  }, [playing, positionMs, spans, duck, level, tracks]);

  useEffect(() => {
    const els: [HTMLAudioElement | null, number][] = [
      [sys.current, 0],
      [mic.current, micOffsetMs],
    ];
    if (!playing || !source) {
      last.current = null;
      for (const [el] of els) el?.pause();
      return;
    }
    const cue = { clip: source.clip, sourceMs: source.sourceMs, speed: speed > 0 ? speed : 1 };
    const jumped = !last.current || last.current.clip !== cue.clip || last.current.speed !== cue.speed;
    last.current = { clip: cue.clip, speed: cue.speed };
    for (const [el, offset] of els) {
      if (!el) continue;
      const want = Math.max(0, cue.sourceMs - offset) / 1000;
      if (jumped || Math.abs(el.currentTime - want) * 1000 > DRIFT_MS) {
        try {
          el.currentTime = want;
        } catch {
          /* not seekable yet */
        }
      }
      el.playbackRate = cue.speed;
      if (el.paused) void el.play?.()?.catch(() => undefined);
    }
  }, [playing, source, speed, micOffsetMs]);

  // Click sound: the hits ahead of the playhead are scheduled on timers (the playhead only
  // arrives every ~66 ms), each played once; a seek or a pause drops what is pending.
  const scheduled = useRef<{ last: number; upTo: number; timers: number[] }>({ last: -1, upTo: -1, timers: [] });
  const clickRef = useRef(clicks);
  useEffect(() => {
    clickRef.current = clicks;
  });
  const clickOn = !!clicks?.on && playing;
  useEffect(() => {
    const st = scheduled.current;
    const drop = () => {
      for (const id of st.timers) window.clearTimeout(id);
      st.timers = [];
    };
    if (!clickOn) {
      drop();
      st.last = st.upTo = -1;
      return;
    }
    if (st.last < 0 || positionMs < st.last || positionMs - st.last > CLICK_JUMP_MS) {
      // Start or seek: from here on (a hit right at the playhead still sounds).
      drop();
      st.upTo = positionMs - 1;
    }
    st.last = positionMs;
    const c = clickRef.current;
    if (!c) return;
    const to = positionMs + CLICK_LOOKAHEAD_MS;
    for (const h of dueClickHits(c.hits, st.upTo, to)) {
      const src = h.release ? c.upSrc : c.downSrc;
      if (!src) continue;
      const vol = clickVolume(h.gain, c.gainDb);
      const id = window.setTimeout(() => {
        st.timers = st.timers.filter((x) => x !== id);
        const a = new Audio(src);
        a.volume = vol;
        void a.play?.()?.catch(() => undefined);
      }, Math.max(0, h.progMs - positionMs));
      st.timers.push(id);
    }
    st.upTo = Math.max(st.upTo, to);
  }, [clickOn, positionMs]);
  useEffect(() => () => scheduled.current.timers.forEach((id) => window.clearTimeout(id)), []);

  return (
    <>
      {screenSrc && <audio ref={sys} src={screenSrc} preload="auto" crossOrigin="anonymous" data-testid="ed-audio-system" />}
      {micSrc && <audio ref={mic} src={micSrc} preload="auto" crossOrigin="anonymous" data-testid="ed-audio-mic" />}
      {tracks.map((tr) => (
        <audio
          key={`${tr.index}:${tr.src}`}
          ref={(el) => {
            if (el) trackEls.current.set(tr.index, el);
            else trackEls.current.delete(tr.index);
          }}
          src={tr.src}
          preload="auto"
          crossOrigin="anonymous"
          data-testid="ed-audio-track"
        />
      ))}
    </>
  );
}
