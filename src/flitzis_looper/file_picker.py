"""One owned Unicode picker process; no audio, GUI callbacks or blocking polling."""

import json
import subprocess
import sys
import tempfile
from pathlib import Path

_MAX_RESULT_BYTES = 128 * 1024
_INVALID_RESULT = "Audio file dialog returned an invalid result"


def _unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            message = "Audio file dialog returned duplicate fields"
            raise ValueError(message)
        result[key] = value
    return result


def _selected_result(value: object) -> str | None:
    if not isinstance(value, dict):
        raise TypeError(_INVALID_RESULT)
    status = value.get("status")
    if status == "selected" and set(value) == {"status", "path"}:
        path = value["path"]
        if isinstance(path, str) and path and "\0" not in path and Path(path).is_absolute():
            return path
    elif status == "cancelled" and set(value) == {"status"}:
        return None
    elif status == "failed" and set(value) == {"status", "message"}:
        message = value["message"]
        if isinstance(message, str) and message:
            raise RuntimeError(message)
    raise ValueError(_INVALID_RESULT)


class AudioFilePicker:
    """Keep native dialog lifetime outside the App's GIL and shutdown path."""

    def __init__(self) -> None:
        with tempfile.NamedTemporaryFile(
            prefix="flitzis-audio-picker-", suffix=".json", delete=False
        ) as result:
            self._result_path = Path(result.name)
        self.cancelled = False
        self.cleanup_error: str | None = None
        try:
            self._process = subprocess.Popen(
                [sys.executable, "-m", "flitzis_looper.file_picker_worker", str(self._result_path)],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            )
        except OSError:
            self._result_path.unlink(missing_ok=True)
            raise

    def ready(self) -> bool:
        return self._process.poll() is not None

    def _remove_result(self) -> None:
        try:
            self._result_path.unlink(missing_ok=True)
        except OSError as error:
            self.cleanup_error = (
                f"Could not remove terminal audio picker result {self._result_path}: {error}"
            )

    def cancel(self, *, wait: bool = False) -> bool:
        """Cancel this exact child even before its native window has appeared."""
        self.cancelled = True
        if not self.ready():
            try:
                self._process.terminate()
            except OSError:
                if not self.ready():
                    raise
        if wait and not self.ready():
            self._process.wait(timeout=5)
        if not self.ready():
            return False
        self._remove_result()
        return True

    def result(self) -> str | None:
        """Consume only a terminal child's bounded result; cancellation always wins."""
        if not self.ready():
            message = "Audio file dialog is still open"
            raise RuntimeError(message)
        try:
            if self.cancelled:
                return None
            if self._process.returncode != 0:
                message = "Audio file dialog process failed"
                raise RuntimeError(message)
            with self._result_path.open("rb") as result:
                encoded = result.read(_MAX_RESULT_BYTES + 1)
            if len(encoded) > _MAX_RESULT_BYTES:
                message = "Audio file dialog result exceeds its byte limit"
                raise ValueError(message)
            value = json.loads(
                encoded.decode("utf-8", errors="strict"), object_pairs_hook=_unique_object
            )
            return _selected_result(value)
        finally:
            self._remove_result()
