/**
 * Feed NATIVE-captured mic PCM (streamed from Rust `cpal`, event `mic-audio`)
 * into a Web-Audio graph via a ScriptProcessor *source*, so real-time features
 * (BPM detector + disco) get their audio without ever calling `getUserMedia` —
 * which makes WKWebView reconfigure the shared audio device and pause other
 * apps' playback at mic-open (v0.84.255/.256). Ref-counted: BPM + disco can run
 * at once and share the ONE native capture.
 *
 * The returned `source` is a normal `AudioNode` you connect exactly where the
 * old `MediaStreamAudioSource` went — the whole downstream analysis graph
 * (filters, analysers, BpmAnalyzer, visualizer) is unchanged.
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { micCaptureStart, micCaptureStop } from "./ipc";

// ONE native mic capture is shared by all consumers (BPM detector + disco).
// Ref-count so the first consumer starts it and the last one stops it — a
// consumer stopping while another is still listening must NOT kill the stream.
let refCount = 0;

interface AudioChunk {
  rate: number;
  b64: string;
}

/** base64 → little-endian i16 → Float32 (−1..1). */
function decodeChunk(b64: string): Float32Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  const i16 = new Int16Array(bytes.buffer, 0, bytes.length >> 1);
  const out = new Float32Array(i16.length);
  for (let i = 0; i < i16.length; i++) out[i] = i16[i] / 32768;
  return out;
}

/** Linear-resample mono Float32 from `srcRate` to `dstRate`. */
function resample(input: Float32Array, srcRate: number, dstRate: number): Float32Array {
  if (srcRate === dstRate || input.length === 0) return input;
  const ratio = srcRate / dstRate;
  const outLen = Math.floor(input.length / ratio);
  const out = new Float32Array(outLen);
  for (let i = 0; i < outLen; i++) {
    const pos = i * ratio;
    const i0 = Math.floor(pos);
    const i1 = Math.min(i0 + 1, input.length - 1);
    const frac = pos - i0;
    out[i] = input[i0] * (1 - frac) + input[i1] * frac;
  }
  return out;
}

export interface FedMic {
  /** Connect this where the mic MediaStreamSource used to go. */
  source: AudioNode;
  /** Stop the native capture + tear down the feed nodes (keeps the ctx warm). */
  stop: () => void;
}

/**
 * Start native mic capture + return a source node fed by it. The source is a
 * ScriptProcessor whose output is the incoming PCM (resampled to the context
 * rate). A muted gain → destination gives the graph a pull path so the
 * processor fires; the analysers tap off the source as before.
 */
export async function startFedMic(ctx: AudioContext): Promise<FedMic> {
  // First consumer starts the native capture; later consumers share it.
  if (refCount === 0) await micCaptureStart();
  refCount++;
  let released = false;
  const release = () => {
    if (released) return;
    released = true;
    refCount = Math.max(0, refCount - 1);
    if (refCount === 0) void micCaptureStop();
  };

  const cap = Math.max(1, Math.floor(ctx.sampleRate * 1.5)); // ~1.5 s ring
  const ring = new Float32Array(cap);
  let readPos = 0;
  let writePos = 0;
  let filled = 0;

  const feed = ctx.createScriptProcessor(2048, 0, 1);
  feed.onaudioprocess = (e) => {
    const out = e.outputBuffer.getChannelData(0);
    for (let i = 0; i < out.length; i++) {
      if (filled > 0) {
        out[i] = ring[readPos];
        readPos = (readPos + 1) % cap;
        filled--;
      } else {
        out[i] = 0; // underrun → brief silence (imperceptible for BPM/viz)
      }
    }
  };
  // Pull path: the processor only runs if something downstream reaches the
  // destination. A muted gain does that without making any sound.
  const muted = ctx.createGain();
  muted.gain.value = 0;
  feed.connect(muted);
  muted.connect(ctx.destination);

  // Resolve once the first PCM chunk arrives; reject if none does (mic denied /
  // unavailable) so the caller's error UI shows instead of a silent dead graph.
  let gotFirst = false;
  let onFirst: () => void = () => undefined;
  const firstChunk = new Promise<void>((res) => {
    onFirst = res;
  });

  const unlisten: UnlistenFn = await listen<AudioChunk>("mic-audio", (e) => {
    if (!gotFirst) {
      gotFirst = true;
      onFirst();
    }
    const { rate, b64 } = e.payload;
    const pcm = resample(decodeChunk(b64), rate, ctx.sampleRate);
    for (let i = 0; i < pcm.length; i++) {
      ring[writePos] = pcm[i];
      writePos = (writePos + 1) % cap;
      if (filled < cap) filled++;
      else readPos = (readPos + 1) % cap; // overrun → drop the oldest sample
    }
  });

  const teardown = () => {
    unlisten();
    try {
      feed.disconnect();
      muted.disconnect();
    } catch {
      /* already gone */
    }
    release(); // ref-counted: stops the native capture only when the last consumer leaves
  };

  const timeout = new Promise<never>((_res, rej) =>
    window.setTimeout(() => rej(new Error("No audio from the microphone")), 2800),
  );
  try {
    await Promise.race([firstChunk, timeout]);
  } catch (e) {
    teardown();
    throw e;
  }

  return { source: feed, stop: teardown };
}
