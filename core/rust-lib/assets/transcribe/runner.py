"""Embedded local Whisper worker. JSON in/out; media paths never become code."""
import importlib.util
import json
import os
from pathlib import Path
import sys


def load_whisper(language):
    if importlib.util.find_spec("whisper") is None:
        raise RuntimeError(
            "Whisper fehlt. Einmal python3 scripts/setup-transcribe.py ausführen "
            "(Windows: py scripts/setup-transcribe.py)."
        )
    import whisper
    from whisper.tokenizer import LANGUAGES, TO_LANGUAGE_CODE

    if language:
        language = language.lower()
        language = TO_LANGUAGE_CODE.get(language, language)
        if language not in LANGUAGES:
            raise ValueError(f"Unbekannte Sprache: {language}. Beispiele: de, en, fr, auto.")
    return whisper, language


def write_transcript(source, text):
    # Exclusive creation also protects simultaneous runs from overwriting.
    for number in range(1, 10001):
        suffix = "" if number == 1 else f" {number}"
        target = source.with_name(f"{source.stem}{suffix}.txt")
        try:
            stream = target.open("x", encoding="utf-8")
        except FileExistsError:
            continue
        try:
            with stream:
                stream.write(text.strip() + "\n")
        except BaseException:
            target.unlink(missing_ok=True)
            raise
        return str(target)
    raise RuntimeError("Zu viele vorhandene Transkripte für diese Datei.")


def main():
    # Python.org installations may lack a CA bundle. Use the OS trust store
    # for model downloads when available, retaining certificate verification.
    if importlib.util.find_spec("truststore") is not None:
        import truststore
        truststore.inject_into_ssl()
    request = json.load(sys.stdin)
    whisper, language = load_whisper(request["language"])
    if "--check" in sys.argv:
        return
    model = whisper.load_model(os.environ.get("IR_WHISPER_MODEL", "small"), device="cpu")
    summary = {"outputs": [], "failed": []}
    for filename in request["paths"]:
        try:
            # Reset automatic detection for EACH file in a multilingual batch.
            result = model.transcribe(
                filename, language=language, task="transcribe", verbose=None, fp16=False
            )
            summary["outputs"].append(write_transcript(Path(filename), result["text"]))
        except Exception as error:
            summary["failed"].append([filename, str(error)])
    print(json.dumps(summary, ensure_ascii=False))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
