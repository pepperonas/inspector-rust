/**
 * Record a fixed-length mono PCM clip from the microphone and downsample it to
 * 16 kHz `Int16Array` — the input the Shazam signature generator expects
 * (`shazam_recognize`). Raw audio (AGC/NS/echo-cancel off) gives the cleanest
 * fingerprint. Uses a ScriptProcessorNode for contiguous PCM (simplest reliable
 * capture in WKWebView; AudioWorklet would be the modern path but is overkill
 * for a one-shot 10 s grab).
 */

/** Linear-resample a Float32 mono buffer from `srcRate` to 16 kHz → Int16. */
export function downsampleTo16kInt16(input: Float32Array, srcRate: number): Int16Array {
  const dstRate = 16000;
  if (srcRate === dstRate) {
    const out = new Int16Array(input.length);
    for (let i = 0; i < input.length; i++) {
      const s = Math.max(-1, Math.min(1, input[i]));
      out[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
    }
    return out;
  }
  const ratio = srcRate / dstRate;
  const outLen = Math.floor(input.length / ratio);
  const out = new Int16Array(outLen);
  for (let i = 0; i < outLen; i++) {
    const pos = i * ratio;
    const i0 = Math.floor(pos);
    const i1 = Math.min(i0 + 1, input.length - 1);
    const frac = pos - i0;
    const v = input[i0] * (1 - frac) + input[i1] * frac;
    const s = Math.max(-1, Math.min(1, v));
    out[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
  }
  return out;
}

export interface MicRecording {
  samples: Int16Array; // 16 kHz mono
  cancel: () => void;
}

/**
 * Record `seconds` of mic audio, reporting progress 0..1. Resolves with the
 * 16 kHz Int16 samples. Rejects if mic permission is denied. The returned
 * promise also carries a `cancel()` on the object it resolves — but for a
 * one-shot grab the caller usually just awaits it.
 */
export async function recordMic16k(
  seconds: number,
  onProgress?: (p: number) => void,
): Promise<Int16Array> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: {
      echoCancellation: false,
      noiseSuppression: false,
      autoGainControl: false,
      channelCount: 1,
    },
  });
  const AC: typeof AudioContext =
    window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
  const ctx = new AC();
  const srcRate = ctx.sampleRate;
  const source = ctx.createMediaStreamSource(stream);
  const processor = ctx.createScriptProcessor(4096, 1, 1);
  const chunks: Float32Array[] = [];
  const target = Math.ceil(srcRate * seconds);
  let collected = 0;

  const cleanup = () => {
    try {
      processor.disconnect();
      source.disconnect();
      stream.getTracks().forEach((t) => t.stop());
      void ctx.close();
    } catch {
      /* ignore */
    }
  };

  return await new Promise<Int16Array>((resolve, reject) => {
    let done = false;
    processor.onaudioprocess = (e) => {
      if (done) return;
      const ch = e.inputBuffer.getChannelData(0);
      chunks.push(new Float32Array(ch));
      collected += ch.length;
      onProgress?.(Math.min(1, collected / target));
      if (collected >= target) {
        done = true;
        cleanup();
        const merged = new Float32Array(collected);
        let off = 0;
        for (const c of chunks) {
          merged.set(c, off);
          off += c.length;
        }
        resolve(downsampleTo16kInt16(merged, srcRate));
      }
    };
    // A muted gain to destination keeps the processor's callback firing.
    const mute = ctx.createGain();
    mute.gain.value = 0;
    source.connect(processor);
    processor.connect(mute);
    mute.connect(ctx.destination);

    // Safety timeout in case audio never flows (e.g. no input device).
    window.setTimeout(() => {
      if (done) return;
      done = true;
      cleanup();
      if (collected > srcRate) {
        const merged = new Float32Array(collected);
        let off = 0;
        for (const c of chunks) {
          merged.set(c, off);
          off += c.length;
        }
        resolve(downsampleTo16kInt16(merged, srcRate));
      } else {
        reject(new Error("no audio captured"));
      }
    }, (seconds + 4) * 1000);
  });
}
