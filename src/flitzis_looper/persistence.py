"""Project persistence for Flitzi's Looper.

This module implements the `project-persistence` OpenSpec delta:
- Persist/restore `ProjectState` from `samples/flitzis_looper.config.json`.
- Debounced saving (at most once every 10 seconds).

Audio file caching (copy/decode/resample to WAV) is intentionally out of scope for
this module and is handled by the separate `load-audio-files` change.
"""

import json
import os
import tempfile
from contextlib import suppress
from dataclasses import dataclass
from pathlib import Path
from time import monotonic
from typing import Any

from pydantic import ValidationError

from flitzis_looper.models import ProjectState

PROJECT_ASSETS_DIR = Path("samples")
PROJECT_CONFIG_PATH = PROJECT_ASSETS_DIR / "flitzis_looper.config.json"


def load_project_state(config_path: Path = PROJECT_CONFIG_PATH) -> ProjectState:
    """Load `ProjectState` from disk.

    Args:
        config_path: Project config file path.

    Returns:
        Loaded `ProjectState`, or defaults when missing/invalid.
    """
    try:
        raw = config_path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return ProjectState()

    try:
        return ProjectState.model_validate_json(raw)
    except json.JSONDecodeError:
        return ProjectState()
    except ValidationError:
        return ProjectState()


def probe_wav_sample_rate(path: Path) -> int | None:
    """Return the WAV sample rate from the file header.

    This performs a lightweight validation to support startup preflight checks.

    Args:
        path: Path to a WAV file.

    Returns:
        Sample rate in Hz, or None when the file is not a valid/parsable WAV.
    """
    data: bytes | None
    try:
        with path.open("rb") as f:
            data = f.read(64 * 1024)
    except FileNotFoundError:
        data = None
    except OSError:
        data = None

    if data is None or len(data) < 44:
        return None

    if data[0:4] != b"RIFF" or data[8:12] != b"WAVE":
        return None

    sample_rate: int | None = None
    offset = 12
    # Basic RIFF chunk scan.
    while offset + 8 <= len(data):
        chunk_id = data[offset : offset + 4]
        chunk_size = int.from_bytes(data[offset + 4 : offset + 8], "little", signed=False)
        chunk_data_start = offset + 8
        chunk_data_end = chunk_data_start + chunk_size

        if chunk_id == b"fmt ":
            if chunk_data_end > len(data) or chunk_size < 16:
                break

            sample_rate = int.from_bytes(
                data[chunk_data_start + 4 : chunk_data_start + 8],
                "little",
                signed=False,
            )
            break

        # Chunks are word-aligned.
        offset = chunk_data_end + (chunk_size % 2)

    return sample_rate


def _normalize_sample_paths_for_save(sample_paths: list[str | None]) -> list[str | None]:
    cwd = Path.cwd().resolve()

    normalized: list[str | None] = []
    for value in sample_paths:
        if value is None:
            normalized.append(None)
            continue

        # Avoid mangling Windows paths in cross-platform configs.
        if "\\" in value:
            normalized.append(value)
            continue

        path = Path(value)
        try:
            abs_path = path if path.is_absolute() else (cwd / path)
            rel = abs_path.resolve().relative_to(cwd)
        except OSError:
            normalized.append(value)
            continue
        except ValueError:
            normalized.append(value)
            continue

        normalized.append(rel.as_posix())

    return normalized


def _atomic_write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)

    tmp_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            suffix=".tmp",
            delete=False,
        ) as tmp:
            tmp_path = Path(tmp.name)
            tmp.write(content)
            tmp.flush()
            os.fsync(tmp.fileno())

        if tmp_path is not None:
            os.replace(tmp_path, path)
    finally:
        if tmp_path is not None:
            with suppress(OSError):
                tmp_path.unlink(missing_ok=True)


@dataclass(slots=True)
class ProjectPersistence:
    """Debounced persistence for `ProjectState`."""

    project: ProjectState
    config_path: Path = PROJECT_CONFIG_PATH
    debounce_seconds: float = 10.0

    _dirty: bool = False
    _last_write_monotonic: float | None = None

    def mark_dirty(self) -> None:
        """Mark the project as requiring a future save."""
        self._dirty = True

    def maybe_flush(self, *, now: float | None = None) -> bool:
        """Write config if dirty and the debounce window has elapsed."""
        if not self._dirty:
            return False

        now = monotonic() if now is None else now
        if self._last_write_monotonic is not None:
            elapsed = now - self._last_write_monotonic
            if elapsed < self.debounce_seconds:
                return False

        self.flush(now=now)
        return True

    def flush(self, *, now: float | None = None) -> None:
        """Write config to disk (atomic)."""
        now = monotonic() if now is None else now

        data: dict[str, Any] = self.project.model_dump(mode="json")
        sample_paths = data.get("sample_paths")
        if isinstance(sample_paths, list):
            data["sample_paths"] = _normalize_sample_paths_for_save(sample_paths)

        text = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
        _atomic_write_text(self.config_path, text)

        self._dirty = False
        self._last_write_monotonic = now
