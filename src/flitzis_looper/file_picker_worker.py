"""Isolated native Unicode picker. Never import the App or start an audio device."""

import json
import sys
from pathlib import Path

from imgui_bundle import portable_file_dialogs as pfd

_FILTERS = [
    "Audio files",
    "*.wav *.WAV *.aiff *.AIFF *.aif *.AIF *.flac *.FLAC *.mp3 *.MP3 *.ogg *.OGG",
]


def main() -> None:
    result_path = Path(sys.argv[1])
    try:
        picker = pfd.open_file("Load Audio", filters=_FILTERS, options=pfd.opt.none)
        paths = picker.result()
        value = {"status": "selected", "path": paths[0]} if paths else {"status": "cancelled"}
    except (OSError, RuntimeError, UnicodeError) as error:
        value = {"status": "failed", "message": f"Could not select an audio file: {error}"}
    result_path.write_text(json.dumps(value, ensure_ascii=False), encoding="utf-8")


if __name__ == "__main__":
    main()
