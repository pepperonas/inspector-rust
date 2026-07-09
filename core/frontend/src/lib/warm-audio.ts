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
 * Pre-warm the shared context at app startup (v0.84.238). Previously the
 * context was created lazily on the FIRST `bpm`/`disco` start — so its output
 * unit spun up on the default output device exactly while the user was
 * listening to music, and with **boom** enabled that reconfigured the boom
 * Audio device mid-playback (the boom bridge's ring drained → audible
 * dropouts/stutter at BPM-detector start). Warming at launch moves that
 * one-time device spin-up to a harmless moment; later `bpm` starts attach the
 * mic to an already-running context. Idempotent; failures are non-fatal (the
 * lazy path still works).
 */
export function prewarmAudio(): void {
  try {
    const { ctx: c } = ensure();
    // A context created without a user gesture may start suspended — resume
    // so the silent output unit is actually running (Tauri webviews allow it).
    if (c.state === "suspended") void c.resume().catch(() => undefined);
  } catch {
    // No audio hardware / context limit — the lazy path remains.
  }
}

/**
 * Attach a mic stream to the shared context **without ducking**: suspend →
 * build the source + tie it into the silent output → resume (mic + output start
 * together). Returns the `MediaStreamSource` to wire your analysers onto.
 */
export async function attachMic(
  stream: MediaStream,
): Promise<MediaStreamAudioSourceNode> {
  const { ctx: c, silentOut: out } = ensure();
  await c.suspend().catch(() => undefined);
  const source = c.createMediaStreamSource(stream);
  source.connect(out); // ties the mic into the (warm) play-and-record session
  await c.resume().catch(() => undefined);
  return source;
}
