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
from pathlib import Path
from time import monotonic

from pydantic import ValidationError

from flitzis_looper.models import ProjectState

PROJECT_ASSETS_DIR = Path("samples")
PROJECT_CONFIG_PATH = PROJECT_ASSETS_DIR / "flitzis_looper.config.json"


class ProjectPersistence:
    """Debounced persistence for `ProjectState`."""

    project: ProjectState
    config_path: Path = PROJECT_CONFIG_PATH
    debounce_seconds: float = 10.0

    _dirty: bool = False
    _last_write_monotonic: float | None = None

    def __init__(self, project: ProjectState | None = None):
        self.project = ProjectState() if project is None else project

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

        data = self.project.model_dump(mode="json")
        sample_paths = data.get("sample_paths")
        if isinstance(sample_paths, list):
            data["sample_paths"] = self._normalize_sample_paths_for_save(sample_paths)

        text = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
        self._atomic_write_text(text)

        self._dirty = False
        self._last_write_monotonic = now

    def _atomic_write_text(self, content: str) -> None:
        self.config_path.parent.mkdir(parents=True, exist_ok=True)

        tmp_path: Path | None = None
        try:
            with tempfile.NamedTemporaryFile(
                mode="w",
                encoding="utf-8",
                dir=self.config_path.parent,
                prefix=f".{self.config_path.name}.",
                suffix=".tmp",
                delete=False,
            ) as tmp:
                tmp_path = Path(tmp.name)
                tmp.write(content)
                tmp.flush()
                os.fsync(tmp.fileno())

            if tmp_path is not None:
                os.replace(tmp_path, self.config_path)
        finally:
            if tmp_path is not None:
                with suppress(OSError):
                    tmp_path.unlink(missing_ok=True)

    @staticmethod
    def from_config_path(config_path: Path = PROJECT_CONFIG_PATH) -> ProjectPersistence:
        """Load `ProjectState` from disk.

        Args:
            config_path: Project config file path.

        Returns:
            Loaded `ProjectState`, or defaults when missing/invalid.
        """
        try:
            raw = config_path.read_text(encoding="utf-8")
        except FileNotFoundError:
            state = ProjectState()
        else:
            try:
                state = ProjectState.model_validate_json(raw)
            except (json.JSONDecodeError, ValidationError) as _:
                state = ProjectState()

        return ProjectPersistence(state)

    @staticmethod
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
