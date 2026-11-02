import { useEffect, useRef } from "react";
import type { EditorSourcePos, RecordClickHit } from "../../lib/ipc";
import { clickVolume, dbToVolume, dueClickHits } from "../../lib/editor";

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

/**
 * Sound of the preview: system audio (from `screen.mp4`) and the microphone (`mic.wav`)
 * as two `<audio>` elements, cued from the source position and speed Rust reports with the
 * playhead (`EditorPlayback.source/speed`) — on a clip or speed change and whenever they
 * drift; `playbackRate` follows the speed.
 * The mic volume comes from `micGainDb` (an element cannot boost above 0 dB).
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
}) {
  const sys = useRef<HTMLAudioElement>(null);
  const mic = useRef<HTMLAudioElement>(null);
  const last = useRef<{ clip: number; speed: number } | null>(null);

  useEffect(() => {
    if (mic.current) mic.current.volume = dbToVolume(micGainDb);
  }, [micGainDb]);

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
      {screenSrc && <audio ref={sys} src={screenSrc} preload="auto" data-testid="ed-audio-system" />}
      {micSrc && <audio ref={mic} src={micSrc} preload="auto" data-testid="ed-audio-mic" />}
    </>
  );
}
