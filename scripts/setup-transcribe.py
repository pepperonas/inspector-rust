#!/usr/bin/env python3
"""Install local Whisper into the dedicated Inspector Rust environment."""
import os
from pathlib import Path
import subprocess
import sys


def main():
    if sys.platform == "darwin":
        base = Path.home() / "Library/Application Support"
    elif sys.platform == "win32":
        base = Path(os.environ["LOCALAPPDATA"])
    else:
        base = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))
    environment = base / "InspectorRust/transcribe/venv"
    subprocess.run([sys.executable, "-m", "venv", str(environment)], check=True)
    python = environment / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python3")
    subprocess.run([str(python), "-m", "pip", "install", "openai-whisper", "truststore"], check=True)
    print(f"Whisper bereit: {python}")
    print("ffmpeg muss ebenfalls installiert sein. Das Modell wird beim ersten Lauf geladen.")


if __name__ == "__main__":
    main()
