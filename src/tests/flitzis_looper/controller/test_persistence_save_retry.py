"""Optional saves survive real atomic-replace failures without weakening commit owners."""

import errno
import hashlib
import os
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.control_diagnostics import poll_control_diagnostics
from flitzis_looper.controller.persistence import (
    PersistenceFenceError,
    ProjectPersistence,
)
from flitzis_looper.models import ProjectState

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller import AppController


_TRANSACTION = "1" * 32


class _ReplaceGate:
    """Fail only the actual replacement after the real temporary write and fsync."""

    def __init__(self, config: Path, error_type: type[OSError] = PermissionError) -> None:
        self.config = config
        self.error_type = error_type
        self.denied = True
        self.calls = 0
        self._replace = os.replace

    def __call__(self, source: Path, destination: Path) -> None:
        assert destination == self.config
        assert source.is_file()
        self.calls += 1
        if self.denied:
            error_number = errno.EACCES if self.error_type is PermissionError else errno.EIO
            raise self.error_type(
                error_number, "replacement temporarily unavailable", str(destination)
            )
        self._replace(source, destination)


def _saved(tmp_path: Path) -> ProjectPersistence:
    persistence = ProjectPersistence(ProjectState(volume=0.25))
    persistence.config_path = tmp_path / "samples" / "flitzis_looper.config.json"
    persistence.mark_dirty()
    persistence.flush(now=0.0)
    return persistence


@pytest.mark.parametrize("immediate", [False, True])
@pytest.mark.parametrize("error_type", [OSError, PermissionError])
def test_optional_replace_failure_preserves_config_then_saves_latest_intent(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    immediate: bool,
    error_type: type[OSError],
) -> None:
    persistence = _saved(tmp_path)
    previous = persistence.config_path.read_bytes()
    written_revision = persistence.project.config_revision
    persistence.project.volume = 0.5
    persistence.mark_dirty()
    failed_revision = persistence.revision
    gate = _ReplaceGate(persistence.config_path, error_type)
    monkeypatch.setattr(os, "replace", gate)

    saved = persistence.flush_if_dirty(now=20.0) if immediate else persistence.maybe_flush(now=20.0)

    assert not saved
    assert gate.calls == 1
    assert persistence.config_path.read_bytes() == previous
    assert persistence._dirty
    assert persistence.revision == failed_revision
    assert persistence.project.config_revision == written_revision
    assert persistence._last_write_monotonic == 0.0
    assert persistence.save_error is not None
    assert "replacement temporarily unavailable" in persistence.save_error
    assert not list(persistence.config_path.parent.glob(f".{persistence.config_path.name}.*.tmp"))

    persistence.project.volume = 0.8
    persistence.project.pad_gain_db[215] = -9.0
    persistence.mark_dirty()
    latest_revision = persistence.revision
    gate.denied = False
    for now in (20.001, 25.0, 29.999):
        assert not persistence.maybe_flush(now=now)
    assert gate.calls == 1
    assert persistence.save_error is not None
    assert persistence.config_path.read_bytes() == previous

    assert persistence.maybe_flush(now=30.0)
    restored = ProjectState.model_validate_json(persistence.config_path.read_bytes())
    assert gate.calls == 2
    assert restored.volume == pytest.approx(0.8)
    assert restored.pad_gain_db[215] == -9.0
    assert restored.config_revision == latest_revision
    assert not persistence._dirty
    assert persistence._last_write_monotonic == 30.0
    assert persistence.save_error is None


def test_explicit_save_can_retry_before_automatic_backoff_expires(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence = _saved(tmp_path)
    persistence.project.volume = 0.75
    persistence.mark_dirty()
    gate = _ReplaceGate(persistence.config_path)
    monkeypatch.setattr(os, "replace", gate)
    assert not persistence.maybe_flush(now=20.0)
    gate.denied = False
    assert not persistence.maybe_flush(now=20.001)

    assert persistence.flush_if_dirty(now=20.001)

    assert gate.calls == 2
    assert persistence.save_error is None
    assert not persistence._dirty
    assert ProjectState.model_validate_json(persistence.config_path.read_bytes()).volume == 0.75


@pytest.mark.parametrize("migration", [False, True])
def test_raw_write_failure_still_raises_and_preserves_transaction_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, migration: bool
) -> None:
    persistence = _saved(tmp_path)
    previous = persistence.config_path.read_bytes()
    persistence.project.volume = 0.75
    persistence.mark_dirty()
    revision = persistence.revision
    snapshot = persistence.project.model_copy(deep=True)
    if migration:
        revision, snapshot, _ = persistence.capture_migration(_TRANSACTION)
    gate = _ReplaceGate(persistence.config_path)
    monkeypatch.setattr(os, "replace", gate)

    if migration:
        with pytest.raises(PermissionError, match="replacement temporarily unavailable"):
            persistence.commit_migration_receipt(_TRANSACTION, revision, snapshot)
    else:
        with pytest.raises(PermissionError, match="replacement temporarily unavailable"):
            persistence.flush(now=20.0)

    assert persistence.config_path.read_bytes() == previous
    assert persistence._dirty
    assert persistence.revision == revision
    assert persistence._last_write_monotonic == 0.0
    assert persistence.save_error is None, "raw failures belong to their existing caller"
    assert not list(persistence.config_path.parent.glob(f".{persistence.config_path.name}.*.tmp"))
    if migration:
        assert persistence._migration_owner == _TRANSACTION
        assert not persistence.maybe_flush(now=100.0)
        assert not persistence.flush_if_dirty(now=100.0)
        with pytest.raises(PersistenceFenceError):
            persistence.flush(now=100.0)
        assert gate.calls == 1

    gate.denied = False
    if migration:
        receipt = persistence.commit_migration_receipt(_TRANSACTION, revision, snapshot)
        assert receipt.revision == revision
        assert receipt.sha256 == hashlib.sha256(persistence.config_path.read_bytes()).hexdigest()
        assert persistence._migration_owner == _TRANSACTION
        persistence.release_migration(_TRANSACTION)
    else:
        persistence.flush(now=30.0)
    assert gate.calls == 2
    assert not persistence._dirty
    assert persistence.save_error is None


def test_save_failure_console_deduplicates_and_reports_recurrence_after_real_success(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    persistence = controller.persistence
    persistence.flush(now=0.0)
    persistence.project.volume = 0.5
    persistence.mark_dirty()
    gate = _ReplaceGate(persistence.config_path)
    monkeypatch.setattr(os, "replace", gate)
    capsys.readouterr()
    assert not persistence.maybe_flush(now=20.0)
    poll_control_diagnostics(controller)
    first = capsys.readouterr().err
    assert first.count("operation=project_save") == 1
    assert "Project save deferred:" in first
    assert str(persistence.config_path.absolute()) not in first
    for now in (20.001, 25.0, 30.0):
        assert not persistence.maybe_flush(now=now)
        poll_control_diagnostics(controller)
    assert not capsys.readouterr().err
    assert gate.calls == 2

    gate.denied = False
    assert persistence.flush_if_dirty(now=30.001)
    poll_control_diagnostics(controller)
    assert not capsys.readouterr().err
    persistence.project.volume = 0.75
    persistence.mark_dirty()
    gate.denied = True
    assert not persistence.maybe_flush(now=50.0)
    poll_control_diagnostics(controller)
    assert capsys.readouterr().err.count("operation=project_save") == 1
