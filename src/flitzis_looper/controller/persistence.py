"""Project persistence for Flitzi's Looper.

This module implements the `project-persistence` OpenSpec delta:
- Persist/restore `ProjectState` from `samples/flitzis_looper.config.json`.
- Debounced saving (at most once every 10 seconds).

Audio file caching (copy/decode/resample to WAV) is intentionally out of scope for
this module and is handled by the separate `load-audio-files` change.
"""

import hashlib
import json
import os
import re
import tempfile
import threading
from contextlib import suppress
from pathlib import Path
from time import monotonic
from typing import TYPE_CHECKING

from pydantic import ValidationError

from flitzis_looper.controller.key_metadata_persistence import recover_project_key_intent
from flitzis_looper.controller.timing_persistence import (
    TimingPersistenceError,
    verified_project_timing,
)
from flitzis_looper.material_migration_model import MaterialMigrationAlias
from flitzis_looper.models import ProjectState

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper_audio import AudioEngine

PROJECT_ASSETS_DIR = Path("samples")
PROJECT_CONFIG_PATH = PROJECT_ASSETS_DIR / "flitzis_looper.config.json"


class PersistenceFenceError(OSError):
    """An active transaction or newer revision prevents this config write."""


def _validate_transaction_id(transaction_id: str) -> None:
    if type(transaction_id) is not str or re.fullmatch(r"[0-9a-f]{32}", transaction_id) is None:
        message = "material migration transaction ID must be 32 lowercase hex characters"
        raise ValueError(message)


class ProjectPersistence:
    """Debounced persistence for `ProjectState`."""

    project: ProjectState
    config_path: Path = PROJECT_CONFIG_PATH
    debounce_seconds: float = 10.0

    _dirty: bool = False
    _last_write_monotonic: float | None = None

    def __init__(self, project: ProjectState | None = None):
        self.project = ProjectState() if project is None else project
        self._writer = threading.RLock()
        self._revision = self.project.config_revision
        self._migration_owner: str | None = None
        self._dirty = False
        self.load_error: str | None = None
        self._last_write_monotonic = None
        self._audio: AudioEngine | None = None
        self._on_timing_error: Callable[[int, str], None] | None = None
        self._last_timing_rejection_monotonic: float | None = None

    def bind_audio(
        self, audio: AudioEngine, on_timing_error: Callable[[int, str], None] | None = None
    ) -> None:
        """Bind the native current owner used to verify accepted evidence at save."""
        self._audio = audio
        self._on_timing_error = on_timing_error

    def mark_dirty(self) -> None:
        """Mark the project as requiring a future save."""
        with self._writer:
            self._revision += 1
            self._dirty = True

    @property
    def revision(self) -> int:
        """The current intent revision, including changes not yet written."""
        with self._writer:
            return self._revision

    @property
    def config_reference(self) -> str:
        """Bind journal intent to the selected actual config, independently of its bytes."""
        return os.path.normcase(str(self.config_path.resolve()))

    def capture_migration(self, transaction_id: str) -> tuple[int, ProjectState, str | None]:
        """Fence all config writers before capturing rollback intent and file identity."""
        _validate_transaction_id(transaction_id)
        with self._writer:
            if self._migration_owner is not None:
                message = "another material migration owns the config writer"
                raise PersistenceFenceError(message)
            self._migration_owner = transaction_id
            try:
                revision = self._revision
                snapshot = self.project.model_copy(deep=True)
                try:
                    config_digest = hashlib.sha256(self.config_path.read_bytes()).hexdigest()
                except FileNotFoundError:
                    config_digest = None
            except BaseException:
                self._migration_owner = None
                raise
            else:
                return revision, snapshot, config_digest

    def recover_migration_intent(self, transaction_id: str, snapshot: ProjectState) -> None:
        """Preserve validated current intent under a newly acquired recovery fence."""
        _validate_transaction_id(transaction_id)
        validated = ProjectState.model_validate(snapshot.model_dump())
        with self._writer:
            if self._migration_owner != transaction_id:
                message = "material recovery does not own the config writer"
                raise PersistenceFenceError(message)
            for field in ProjectState.model_fields:
                setattr(self.project, field, getattr(validated, field))
            self._revision = max(self._revision, validated.config_revision)
            self._dirty = True

    def release_migration(self, transaction_id: str) -> None:
        """Release only this transaction's writer fence after a settled outcome."""
        _validate_transaction_id(transaction_id)
        with self._writer:
            if self._migration_owner != transaction_id:
                message = "material migration does not own the config writer"
                raise PersistenceFenceError(message)
            self._migration_owner = None

    def transfer_migration(
        self, previous: str, transaction_id: str
    ) -> tuple[int, ProjectState, str | None]:
        """Transfer a fresh recovery fence without an ordinary-writer opening."""
        _validate_transaction_id(previous)
        _validate_transaction_id(transaction_id)
        with self._writer:
            if self._migration_owner != previous:
                message = "material recovery does not own the config writer"
                raise PersistenceFenceError(message)
            revision = self._revision
            snapshot = self.project.model_copy(deep=True)
            try:
                digest = hashlib.sha256(self.config_path.read_bytes()).hexdigest()
            except FileNotFoundError:
                digest = None
            self._migration_owner = transaction_id
            return revision, snapshot, digest

    def commit_migration(
        self, transaction_id: str, expected_revision: int, snapshot: ProjectState
    ) -> tuple[int, str]:
        """Persist one verified related-reference image under the common config writer.

        The coordinator prepares its image from current intent after actual source/timing
        acknowledgement. This method publishes no model references or native authority.
        The caller keeps both owner sets until the returned durable outcome is settled.
        """
        _validate_transaction_id(transaction_id)
        if type(expected_revision) is not int or expected_revision < 0:
            message = "material migration revision must be a nonnegative strict integer"
            raise ValueError(message)
        with self._writer:
            if self._migration_owner != transaction_id:
                message = "material migration does not own the config writer"
                raise PersistenceFenceError(message)
            if expected_revision != self._revision:
                message = "project intent changed before migration commit"
                raise PersistenceFenceError(message)
            validated = ProjectState.model_validate(snapshot.model_dump())
            text = self._write_snapshot(validated, expected_revision, monotonic())
            return expected_revision, hashlib.sha256(text.encode("utf-8")).hexdigest()

    def maybe_flush(self, *, now: float | None = None) -> bool:
        """Write config if dirty and the debounce window has elapsed."""
        if not self._dirty or self._migration_owner is not None:
            return False

        now = monotonic() if now is None else now
        if (
            self._last_timing_rejection_monotonic is not None
            and now - self._last_timing_rejection_monotonic < self.debounce_seconds
        ):
            return False
        if self._last_write_monotonic is not None:
            elapsed = now - self._last_write_monotonic
            if elapsed < self.debounce_seconds:
                return False

        try:
            self.flush(now=now)
        except TimingPersistenceError, PersistenceFenceError:
            return False
        return True

    def flush_if_dirty(self, *, now: float | None = None) -> bool:
        """Write config immediately when there are pending project changes."""
        if not self._dirty or self._migration_owner is not None:
            return False

        try:
            self.flush(now=now)
        except TimingPersistenceError, PersistenceFenceError:
            return False
        return True

    def flush(self, *, now: float | None = None) -> None:
        """Write config to disk (atomic)."""
        now = monotonic() if now is None else now
        with self._writer:
            if self._migration_owner is not None:
                message = "material migration owns the config writer"
                raise PersistenceFenceError(message)
            revision = self._revision
            self._write_snapshot(self.project, revision, now)

    def _write_snapshot(self, project: ProjectState, revision: int, now: float) -> str:
        """Verify and write while retaining changes newer than the captured revision."""
        try:
            snapshot = verified_project_timing(project, self._audio)
        except TimingPersistenceError as error:
            self._dirty = True
            self._last_timing_rejection_monotonic = now
            if self._on_timing_error is not None:
                self._on_timing_error(error.sample_id, f"Timing save rejected: {error}")
            raise
        if self._revision != revision:
            self._dirty = True
            message = "project intent changed during timing verification"
            raise PersistenceFenceError(message)
        data = snapshot.model_dump(mode="json")
        data["config_revision"] = revision
        sample_paths = data.get("sample_paths")
        if isinstance(sample_paths, list):
            data["sample_paths"] = self._normalize_sample_paths_for_save(sample_paths)
            pad_key_lock = data.get("pad_key_lock")
            if isinstance(pad_key_lock, list):
                data["pad_key_lock"] = self._normalize_pad_key_lock_for_save(
                    sample_paths,
                    pad_key_lock,
                )

        text = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
        self._atomic_write_text(text)

        self.project.config_revision = revision
        self._dirty = self._revision != revision
        self._last_timing_rejection_monotonic = None
        self._last_write_monotonic = now
        return text

    def _atomic_write_text(self, content: str) -> None:
        self.config_path.parent.mkdir(parents=True, exist_ok=True)

        tmp_path: Path | None = None
        try:
            with tempfile.NamedTemporaryFile(
                mode="w",
                encoding="utf-8",
                newline="",
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
            except json.JSONDecodeError:
                state = ProjectState()
            except ValidationError as error:
                state = ProjectPersistence._recover_metadata_fields(raw, error)
                persistence = ProjectPersistence(state)
                if any(
                    item["loc"][0] in {"material_migrations", "config_revision"}
                    for item in error.errors()
                    if item["loc"]
                ):
                    persistence.load_error = "Unsupported migration metadata retained on disk"
                return persistence

        return ProjectPersistence(state)

    @staticmethod
    def _recover_metadata_fields(raw: str, error: ValidationError) -> ProjectState:
        """Neutralize only malformed new metadata, preserving all valid performer intent."""
        if any(
            item["loc"][0] not in {"pad_key_intent", "material_migrations", "config_revision"}
            for item in error.errors()
            if item["loc"]
        ):
            return ProjectState()
        try:
            recovered = json.loads(raw)
            if not isinstance(recovered, dict):
                return ProjectState()
            if "pad_key_intent" in recovered:
                recovered = recover_project_key_intent(recovered)
            revision = recovered.get("config_revision", 0)
            if type(revision) is not int or revision < 0:
                recovered["config_revision"] = 0
            aliases = recovered.get("material_migrations", {})
            valid: dict[str, MaterialMigrationAlias] = {}
            if isinstance(aliases, dict):
                for key, value in list(aliases.items())[:216]:
                    try:
                        alias = MaterialMigrationAlias.model_validate(value)
                    except ValidationError:
                        continue
                    if key == alias.transaction_id:
                        valid[key] = alias
            recovered["material_migrations"] = valid
            return ProjectState.model_validate(recovered)
        except json.JSONDecodeError, ValidationError:
            return ProjectState()

    @staticmethod
    def _normalize_sample_paths_for_save(sample_paths: list[str | None]) -> list[str | None]:
        cwd = Path.cwd().resolve()

        normalized: list[str | None] = []
        for value in sample_paths:
            if value is None:
                normalized.append(None)
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

    @staticmethod
    def _normalize_pad_key_lock_for_save(
        sample_paths: list[object],
        pad_key_lock: list[object],
    ) -> list[object]:
        normalized = list(pad_key_lock)
        for sample_id, sample_path in enumerate(sample_paths):
            if sample_id >= len(normalized):
                break
            if sample_path is None:
                normalized[sample_id] = False
        return normalized
