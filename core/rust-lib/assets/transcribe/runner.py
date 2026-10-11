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


def emit(event, **fields):
    """One JSON line per event on stdout — the Rust side reads them live."""
    fields["event"] = event
    sys.stdout.write(json.dumps(fields, ensure_ascii=False) + "\n")
    sys.stdout.flush()


class _Progress:
    """Stand-in for whisper's tqdm bar: reports frames done for one file.

    Whisper drives `tqdm.tqdm(total=content_frames, ...)` + `pbar.update(n)`
    while decoding. Replacing the bar keeps verbose=None (nothing printed)
    and turns each update into a `progress` event. `disable` is ignored on
    purpose — that flag only means "don't draw", we never draw anyway."""

    current = 0

    def __init__(self, total=None, **_kwargs):
        self.total = total or 0
        self.done = 0

    def __enter__(self):
        return self

    def __exit__(self, *_exc):
        return False

    def update(self, n=1):
        self.done += n
        emit("progress", index=_Progress.current, done=self.done, total=self.total)


def install_progress():
    # `whisper.transcribe` the ATTRIBUTE is the function (the package
    # re-exports it); the module lives only in sys.modules.
    module = sys.modules.get("whisper.transcribe")
    if module is not None and hasattr(module, "tqdm"):
        module.tqdm = type("tqdm_shim", (), {"tqdm": _Progress})


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
    install_progress()
    emit("model")
    model = whisper.load_model(os.environ.get("IR_WHISPER_MODEL", "small"), device="cpu")
    for index, filename in enumerate(request["paths"]):
        _Progress.current = index
        emit("start", index=index)
        try:
            # Reset automatic detection for EACH file in a multilingual batch.
            result = model.transcribe(
                filename, language=language, task="transcribe", verbose=None, fp16=False
            )
            output = write_transcript(Path(filename), result["text"])
            emit("done", index=index, output=output, language=result.get("language"))
        except Exception as error:
            emit("failed", index=index, error=str(error))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
