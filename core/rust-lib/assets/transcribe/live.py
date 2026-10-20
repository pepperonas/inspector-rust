"""Live dictation worker: JSON lines in, JSON lines out.

Rust records the microphone, cuts the stream into segments at pauses and
writes each one as raw 16 kHz mono little-endian int16 into a private cache
file. This worker decodes them:

  {"op": "decode", "seq": n, "kind": "partial"|"final", "path": ..., "prompt": ...}
      -> {"event": "text", "seq": n, "kind": ..., "text": ...}
  {"op": "finalize", "path": ...}
      -> {"event": "final", "text": ...}   (the whole recording, accurate model)

Errors come back as {"event": "error", "seq": n|null, "error": ...}; the worker
keeps running. It exits when stdin closes. Paths are data, never code.
"""
import importlib.util
import json
import os
import sys

LIVE_MODEL = os.environ.get("IR_WHISPER_LIVE_MODEL", "base")
FINAL_MODEL = os.environ.get("IR_WHISPER_MODEL", "small")


def emit(event, **fields):
    fields["event"] = event
    sys.stdout.write(json.dumps(fields, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def load(path):
    import numpy as np

    pcm = np.fromfile(path, dtype="<i2")
    return pcm.astype(np.float32) / 32768.0


def main():
    if importlib.util.find_spec("truststore") is not None:
        import truststore

        truststore.inject_into_ssl()
    if importlib.util.find_spec("whisper") is None:
        raise RuntimeError(
            "Whisper fehlt. Einmal python3 scripts/setup-transcribe.py ausführen "
            "(Windows: py scripts/setup-transcribe.py)."
        )
    import whisper
    from whisper.tokenizer import LANGUAGES, TO_LANGUAGE_CODE

    language = os.environ.get("IR_TRANSCRIBE_LANGUAGE") or None
    if language:
        language = TO_LANGUAGE_CODE.get(language.lower(), language.lower())
        if language not in LANGUAGES:
            raise ValueError(f"Unbekannte Sprache: {language}. Beispiele: de, en, fr, auto.")

    emit("model", name=LIVE_MODEL)
    live = whisper.load_model(LIVE_MODEL, device="cpu")
    emit("ready")

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        seq = None
        try:
            req = json.loads(line)
            op = req.get("op")
            seq = req.get("seq")
            if op == "decode":
                result = live.transcribe(
                    load(req["path"]),
                    language=language,
                    task="transcribe",
                    verbose=None,
                    fp16=False,
                    # Each segment is decoded on its own; feeding back the
                    # model's own text is what makes Whisper loop on silence.
                    condition_on_previous_text=False,
                    initial_prompt=req.get("prompt") or None,
                )
                emit("text", seq=seq, kind=req.get("kind", "partial"), text=result["text"].strip())
            elif op == "finalize":
                emit("finalizing", name=FINAL_MODEL)
                final = live if FINAL_MODEL == LIVE_MODEL else whisper.load_model(FINAL_MODEL, device="cpu")
                result = final.transcribe(
                    load(req["path"]), language=language, task="transcribe", verbose=None, fp16=False
                )
                emit("final", text=result["text"].strip())
            else:
                emit("error", seq=seq, error=f"unknown op: {op}")
        except Exception as error:  # one bad request must not end the session
            emit("error", seq=seq, error=str(error))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
