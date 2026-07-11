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
/** Mic sources attached via `attachMic` — consulted by `suspendWarmIfIdle` so
 * the context is never parked under a live bpm/disco capture. Sources whose
 * stream tracks have ended (the consumer stopped them) are pruned lazily. */
let micSources: MediaStreamAudioSourceNode[] = [];

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
  // Diagnostics: confirm every mic user shares the ONE warm context (a second
  // live source here means two callers overlap — expected only briefly). A
  // fresh context per caller was the stutter cause (v0.84.253).
  console.debug(
    `[warm-audio] attachMic — ctx.state=${c.state} rate=${c.sampleRate} liveMics=${micSources.length}`,
  );
  await c.suspend().catch(() => undefined);
  const source = c.createMediaStreamSource(stream);
  source.connect(out); // ties the mic into the (warm) play-and-record session
  await c.resume().catch(() => undefined);
  micSources.push(source);
  return source;
}

/**
 * Detach a mic session opened with `attachMic`: disconnect the source (cuts all
 * its downstream taps), stop the stream's tracks, and drop it from the live-mic
 * set. **Never closes the warm context** — it stays warm for the next mic user
 * (the whole point: one persistent play-and-record session avoids re-triggering
 * the CoreAudio device reconfiguration that stutters other apps' playback).
 */
export function detachMic(source: MediaStreamAudioSourceNode | null, stream: MediaStream | null): void {
  try {
    source?.disconnect();
  } catch {
    /* already disconnected */
  }
  if (source) micSources = micSources.filter((s) => s !== source);
  stream?.getTracks().forEach((t) => t.stop());
  console.debug(`[warm-audio] detachMic — remaining liveMics=${micSources.length} (ctx kept warm)`);
}

/** Whether any attached mic stream still has a live track (bpm/disco active).
 * Programmatic `track.stop()` doesn't fire `ended`, so liveness is polled here
 * at decision time instead of relying on events. */
function micActive(): boolean {
  micSources = micSources.filter((s) =>
    s.mediaStream.getTracks().some((t) => t.readyState === "live"),
  );
  return micSources.length > 0;
}

/**
 * Park the warm context (v0.84.240, battery). A running output unit makes
 * coreaudiod hold a `PreventUserIdleSystemSleep` assertion for the webview —
 * on its own that stopped the Mac from ever idle-sleeping, and with boom on it
 * kept boom Audio "running" so boom's idle gate could never suspend either.
 * Driven by the Rust side's `warm-audio-suspend` event (boom silence gate /
 * boom disabled). Refuses while a mic is live — boom's probe then fails and
 * its bridge stays running, which is correct: the user is using audio.
 */
export function suspendWarmIfIdle(): void {
  if (ctx && ctx.state === "running" && !micActive()) {
    void ctx.suspend().catch(() => undefined);
  }
}

/**
 * (Re)start the warm context — the `warm-audio-resume` handler (boom bridge
 * resumed / boom enabled). Creates the context if it never existed (boom
 * enabled after launch, where the gated pre-warm was skipped).
 */
export function resumeWarm(): void {
  prewarmAudio();
}
