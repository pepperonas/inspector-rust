"""Offline behaviour tests for the embedded worker, no Whisper install needed."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

source = Path(__file__).resolve().parents[1] / "core/rust-lib/assets/transcribe/runner.py"
spec = importlib.util.spec_from_file_location("transcribe_runner", source)
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class WorkerTests(unittest.TestCase):
    def test_existing_transcripts_are_preserved_including_numbered_names(self):
        with tempfile.TemporaryDirectory() as directory:
            video = Path(directory) / "Grüße heute.MP4"
            original = video.with_suffix(".txt")
            original.write_text("existing", encoding="utf-8")
            second = video.with_name("Grüße heute 2.txt")
            second.write_text("second", encoding="utf-8")
            result = Path(worker.write_transcript(video, "  Hallo Welt!  "))
            self.assertEqual(result.name, "Grüße heute 3.txt")
            self.assertEqual(result.read_text(encoding="utf-8"), "Hallo Welt!\n")
            self.assertEqual(original.read_text(encoding="utf-8"), "existing")
            self.assertEqual(second.read_text(encoding="utf-8"), "second")

    def test_failed_write_removes_partial_output(self):
        class BrokenStream(io.StringIO):
            def write(self, text):
                raise OSError("disk full")

        path = Path("/video.mp4")
        with patch.object(Path, "open", return_value=BrokenStream()), patch.object(Path, "unlink") as unlink:
            with self.assertRaisesRegex(OSError, "disk full"):
                worker.write_transcript(path, "hello")
            unlink.assert_called_once_with(missing_ok=True)

    def test_bad_media_does_not_stop_batch_and_auto_detection_is_per_file(self):
        class Model:
            def transcribe(self, filename, **options):
                if options["language"] is not None:
                    raise AssertionError("automatic detection disabled")
                if Path(filename).stem == "broken":
                    raise ValueError("no audio track")
                return {"text": Path(filename).stem}

        class Whisper:
            @staticmethod
            def load_model(*args, **kwargs):
                return Model()

        with tempfile.TemporaryDirectory() as directory:
            paths = [str(Path(directory) / name) for name in ["first.mp4", "broken.mp4", "last.mp3"]]
            request = {"paths": paths, "language": None}
            output = io.StringIO()
            with patch.object(worker, "load_whisper", return_value=(Whisper, None)), patch("sys.stdin", io.StringIO(json.dumps(request))), patch("sys.argv", ["runner"]), contextlib.redirect_stdout(output):
                worker.main()
            summary = json.loads(output.getvalue())
            self.assertEqual([Path(p).name for p in summary["outputs"]], ["first.txt", "last.txt"])
            self.assertEqual(summary["failed"], [[paths[1], "no audio track"]])
            self.assertFalse(Path(paths[1]).with_suffix(".txt").exists())


if __name__ == "__main__":
    unittest.main()
