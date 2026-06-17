/**
 * Shared "warm" AudioContext (v0.84.52).
 *
 * One process-wide `AudioContext` with a silent output unit kept running
 * between uses, so the audio output device stays open — when the mic is opened
 * later, macOS has less device reconfiguration to do (smaller mic-open glitch).
 *
 * The catch: a warm *output* unit + a mic added on top is exactly what made
 * macOS treat the context as a full-duplex "communication" session and **duck**
 * other apps' audio (the v0.84.50 regression). The fix is to wire the mic while
 * the context is **suspended** and `resume()` afterwards, so input + output come
 * up together — the v0.84.49 condition that didn't duck. `attachMic` does this.
 */

let ctx: AudioContext | null = null;
let silentOut: GainNode | null = null;

/** The shared context + its silent output node, created lazily and kept warm. */
function ensure(): { ctx: AudioContext; silentOut: GainNode } {
  if (!ctx || !silentOut) {
    ctx = new AudioContext({ latencyHint: "playback" });
    silentOut = ctx.createGain();
    silentOut.gain.value = 0; // never monitor anything through the speakers
    silentOut.connect(ctx.destination);
  }
  return { ctx, silentOut };
}

/** Get the shared warm context (created on first call). */
export function warmContext(): AudioContext {
  return ensure().ctx;
}

/**
 * Attach a mic stream to the shared context **without ducking**: suspend →
 * build the source + tie it into the silent output → resume (mic + output start
 * together). Returns the `MediaStreamSource` to wire your analysers onto.
 */
export async function attachMic(stream: MediaStream): Promise<MediaStreamAudioSourceNode> {
  const { ctx: c, silentOut: out } = ensure();
  await c.suspend().catch(() => undefined);
  const source = c.createMediaStreamSource(stream);
  source.connect(out); // ties the mic into the (warm) play-and-record session
  await c.resume().catch(() => undefined);
  return source;
}
