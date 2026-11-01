import { useEffect, useRef } from "react";
import type { EditorSourcePos } from "../../lib/ipc";
import { dbToVolume } from "../../lib/editor";

/** Resync when an element is this far from where the playhead says it should be. */
const DRIFT_MS = 250;

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
}: {
  screenSrc: string | null;
  micSrc: string | null;
  micOffsetMs: number;
  micGainDb: number;
  playing: boolean;
  source: EditorSourcePos | null;
  speed: number;
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

  return (
    <>
      {screenSrc && <audio ref={sys} src={screenSrc} preload="auto" data-testid="ed-audio-system" />}
      {micSrc && <audio ref={mic} src={micSrc} preload="auto" data-testid="ed-audio-mic" />}
    </>
  );
}
