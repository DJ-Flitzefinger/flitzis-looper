"""Actual config commit faults; Native complete-artifact verification is separate."""

import os
import tempfile
from dataclasses import dataclass
from typing import TYPE_CHECKING, Self, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import persistence as persistence_module
from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.controller.timing_persistence import (
    TimingPersistenceError,
    verified_project_timing,
)
from flitzis_looper.models import ProjectState, StemCacheEntry
from flitzis_looper.stem_pair_selection import StemGenerationProvenance, StemPairSelection

if TYPE_CHECKING:
    from pathlib import Path
    from tempfile import _TemporaryFileWrapper
    from types import TracebackType

    from flitzis_looper_audio import AudioEngine


def _entry(generation: str) -> StemCacheEntry:
    root = f"samples/materials/M{'a' * 32}"
    fingerprint = generation * 64
    provenance = StemGenerationProvenance(
        fingerprint_schema="effective-stem-v3",
        generation_fingerprint=fingerprint,
        producer_identity="e" * 64,
    )
    pair = StemPairSelection(
        descriptor_reference=f"{root}/.pcm-cache/stems/v1/.pairs/{generation * 32}.json",
        stem_set_identity=generation * 64,
        wav_generation=f"{root}/stems/.ready-{generation * 32}",
        pcm_generation=f"{root}/.pcm-cache/stems/v1/.ready-{generation * 32}",
        provenance=provenance,
    )
    return StemCacheEntry(
        source_version=f"{root}/original/source.wav|sha256-v1:{'1' * 64}",
        cache_dir=pair.wav_generation,
        stems=expected_stem_files(pair.wav_generation),
        available=True,
        pair=pair,
        fingerprint_schema="effective-stem-v3",
        generation_fingerprint=fingerprint,
        producer_identity=provenance.producer_identity,
    )


@dataclass(slots=True)
class _SavedSelection:
    persistence: ProjectPersistence
    previous: StemCacheEntry
    selected: StemCacheEntry
    encoded: bytes
    revision: int


@pytest.fixture
def saved_selection(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> _SavedSelection:
    monkeypatch.chdir(tmp_path)
    previous = _entry("b")
    project = ProjectState(volume=0.71)
    project.stem_cache[0] = previous
    persistence = ProjectPersistence(project)
    persistence.config_path = tmp_path / "samples" / "owned.config.json"
    persistence.flush(now=0.0)
    return _SavedSelection(
        persistence,
        previous,
        _entry("c"),
        persistence.config_path.read_bytes(),
        persistence.revision,
    )


def _assert_reopened(saved: _SavedSelection, expected: StemCacheEntry) -> ProjectState:
    reopened = ProjectPersistence.from_config_path(saved.persistence.config_path)
    assert reopened.load_error is None
    assert not reopened.invalid_stem_pair_ids
    assert reopened.project.stem_cache[0] == expected
    return reopened.project


def _assert_precommit_retained(saved: _SavedSelection) -> None:
    assert saved.persistence.project.stem_cache[0] is saved.previous
    assert saved.persistence.config_path.read_bytes() == saved.encoded
    assert saved.persistence.revision == saved.revision
    assert saved.persistence.project.config_revision == saved.revision
    assert not tuple(saved.persistence.config_path.parent.glob(".*.tmp"))
    _assert_reopened(saved, saved.previous)


class _FailingTemporaryWrite:
    """Fail the actual owned temp-file write after creating its real pathname."""

    def __init__(self, handle: _TemporaryFileWrapper[str]) -> None:
        self._handle = handle

    @property
    def name(self) -> str:
        return str(self._handle.name)

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self,
        _exc_type: type[BaseException] | None,
        _exc_value: BaseException | None,
        _traceback: TracebackType | None,
    ) -> None:
        self._handle.close()

    def write(self, _content: str) -> int:
        message = "injected owned temp write failure"
        raise OSError(message)


def _fail_temporary_write(monkeypatch: pytest.MonkeyPatch) -> None:
    create = tempfile.NamedTemporaryFile

    def failing_file(**options: object) -> _FailingTemporaryWrite:
        return _FailingTemporaryWrite(
            create(
                mode=str(options["mode"]),
                encoding=str(options["encoding"]),
                newline=str(options["newline"]),
                dir=cast("Path", options["dir"]),
                prefix=str(options["prefix"]),
                suffix=str(options["suffix"]),
                delete=bool(options["delete"]),
            )
        )

    monkeypatch.setattr(tempfile, "NamedTemporaryFile", failing_file)


@pytest.mark.parametrize("boundary", ["write", "fsync", "replace"])
def test_stem_config_io_failure_keeps_previous_durable_and_live_selection(
    saved_selection: _SavedSelection, monkeypatch: pytest.MonkeyPatch, boundary: str
) -> None:
    saved = saved_selection
    if boundary == "write":
        _fail_temporary_write(monkeypatch)
    else:
        monkeypatch.setattr(
            os,
            boundary,
            Mock(side_effect=OSError(f"injected owned {boundary} failure")),
        )
    with pytest.raises(OSError, match="injected owned"):
        saved.persistence.commit_stem_selection(0, saved.previous, saved.selected, lambda: True)
    _assert_precommit_retained(saved)


@pytest.mark.parametrize("reason", ["cancel", "model", "unload"])
def test_stem_guard_change_after_actual_fsync_prevents_actual_replace(
    saved_selection: _SavedSelection, monkeypatch: pytest.MonkeyPatch, reason: str
) -> None:
    saved = saved_selection
    fsync = os.fsync
    cancelled = False
    model = "A"
    loaded = True
    calls = 0

    def current() -> bool:
        nonlocal calls
        calls += 1
        return not cancelled and model == "A" and loaded

    def after_fsync(descriptor: int) -> None:
        nonlocal cancelled, model, loaded
        fsync(descriptor)
        if reason == "cancel":
            cancelled = True
        elif reason == "model":
            model = "B"
        else:
            loaded = False

    replace = Mock(wraps=os.replace)
    monkeypatch.setattr(os, "fsync", after_fsync)
    monkeypatch.setattr(os, "replace", replace)
    with pytest.raises(PersistenceFenceError, match="stem selection changed"):
        saved.persistence.commit_stem_selection(0, saved.previous, saved.selected, current)
    assert calls == 2
    replace.assert_not_called()
    _assert_precommit_retained(saved)


def test_equal_predecessor_replacement_is_fenced_by_exact_entry_identity(
    saved_selection: _SavedSelection, monkeypatch: pytest.MonkeyPatch
) -> None:
    saved = saved_selection
    fsync = os.fsync
    replacement = saved.previous.model_copy(deep=True)

    def after_fsync(descriptor: int) -> None:
        fsync(descriptor)
        saved.persistence.project.stem_cache[0] = replacement

    monkeypatch.setattr(os, "fsync", after_fsync)
    with pytest.raises(PersistenceFenceError, match="stem selection changed"):
        saved.persistence.commit_stem_selection(0, saved.previous, saved.selected, lambda: True)
    assert saved.persistence.project.stem_cache[0] is replacement
    assert saved.persistence.config_path.read_bytes() == saved.encoded
    assert saved.persistence.revision == saved.revision
    _assert_reopened(saved, saved.previous)


def test_newer_revision_after_fsync_is_not_overwritten_by_stem_commit(
    saved_selection: _SavedSelection, monkeypatch: pytest.MonkeyPatch
) -> None:
    saved = saved_selection
    fsync = os.fsync

    def after_fsync(descriptor: int) -> None:
        fsync(descriptor)
        saved.persistence.mark_dirty()

    monkeypatch.setattr(os, "fsync", after_fsync)
    with pytest.raises(PersistenceFenceError, match="project intent changed"):
        saved.persistence.commit_stem_selection(0, saved.previous, saved.selected, lambda: True)
    assert saved.persistence.project.stem_cache[0] is saved.previous
    assert saved.persistence.config_path.read_bytes() == saved.encoded
    assert saved.persistence.revision == saved.revision + 1
    assert saved.persistence._dirty is True
    _assert_reopened(saved, saved.previous)


@pytest.mark.parametrize("newer_intent", [False, True])
def test_cancel_after_replace_keeps_committed_config_and_leaves_live_transfer_to_controller(
    saved_selection: _SavedSelection,
    monkeypatch: pytest.MonkeyPatch,
    *,
    newer_intent: bool,
) -> None:
    saved = saved_selection
    replace = os.replace
    cancelled = False
    calls = 0

    def current() -> bool:
        nonlocal calls
        calls += 1
        return not cancelled

    def after_replace(source: str | Path, destination: str | Path) -> None:
        nonlocal cancelled
        replace(source, destination)
        cancelled = True
        if newer_intent:
            saved.persistence.project.volume = 0.23
            saved.persistence.mark_dirty()

    monkeypatch.setattr(os, "replace", after_replace)
    saved.persistence.commit_stem_selection(0, saved.previous, saved.selected, current)
    assert cancelled is True
    assert calls == 2
    assert saved.persistence.project.stem_cache[0] is saved.previous
    assert saved.persistence.project.config_revision == saved.revision + 1
    assert saved.persistence.revision == saved.revision + 1 + int(newer_intent)
    assert saved.persistence._dirty is newer_intent
    reopened = _assert_reopened(saved, saved.selected)
    assert reopened.config_revision == saved.revision + 1
    assert reopened.volume == 0.71
    if newer_intent:
        assert saved.persistence.project.volume == 0.23


def test_success_writes_current_other_intent_through_authoritative_timing_snapshot(
    saved_selection: _SavedSelection, monkeypatch: pytest.MonkeyPatch
) -> None:
    saved = saved_selection
    verify = verified_project_timing
    checked: list[ProjectState] = []
    saved.persistence.project.volume = 0.37
    saved.persistence.mark_dirty()

    def verify_candidate(project: ProjectState, audio: AudioEngine | None) -> ProjectState:
        assert saved.persistence.project.stem_cache[0] is saved.previous
        assert project.stem_cache[0] == saved.selected
        assert project is not saved.persistence.project
        checked.append(project)
        return verify(project, audio)

    monkeypatch.setattr(persistence_module, "verified_project_timing", verify_candidate)
    saved.persistence.commit_stem_selection(0, saved.previous, saved.selected, lambda: True)
    assert len(checked) == 1
    assert saved.persistence.project.stem_cache[0] is saved.previous
    assert saved.persistence.revision == saved.revision + 2
    assert saved.persistence.project.config_revision == saved.revision + 2
    assert saved.persistence._dirty is False
    reopened = _assert_reopened(saved, saved.selected)
    assert reopened.volume == 0.37
    assert reopened.config_revision == saved.revision + 2


def test_timing_evidence_rejection_keeps_old_config_before_selection_commit(
    saved_selection: _SavedSelection, monkeypatch: pytest.MonkeyPatch
) -> None:
    saved = saved_selection
    verify = Mock(side_effect=TimingPersistenceError(0, "injected Native timing rejection"))
    monkeypatch.setattr(persistence_module, "verified_project_timing", verify)
    with pytest.raises(TimingPersistenceError, match="injected Native timing rejection"):
        saved.persistence.commit_stem_selection(0, saved.previous, saved.selected, lambda: True)
    _assert_precommit_retained(saved)
    assert saved.persistence._dirty is True


@pytest.mark.parametrize("fence", ["current", "migration"])
def test_stem_config_admission_fence_does_not_begin_a_temp_write(
    saved_selection: _SavedSelection, monkeypatch: pytest.MonkeyPatch, fence: str
) -> None:
    saved = saved_selection
    if fence == "migration":
        saved.persistence.capture_migration("f" * 32)
    create = Mock(wraps=tempfile.NamedTemporaryFile)
    monkeypatch.setattr(tempfile, "NamedTemporaryFile", create)
    with pytest.raises(PersistenceFenceError):
        saved.persistence.commit_stem_selection(
            0, saved.previous, saved.selected, lambda: fence != "current"
        )
    create.assert_not_called()
    _assert_precommit_retained(saved)


def test_stem_config_commit_supports_no_predecessor(saved_selection: _SavedSelection) -> None:
    saved = saved_selection
    saved.persistence.project.stem_cache[0] = None
    saved.persistence.commit_stem_selection(0, None, saved.selected, lambda: True)
    assert saved.persistence.project.stem_cache[0] is None
    assert saved.persistence.revision == saved.revision + 1
    _assert_reopened(saved, saved.selected)
