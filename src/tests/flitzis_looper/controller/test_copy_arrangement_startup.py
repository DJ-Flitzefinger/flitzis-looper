"""Real Store/Config startup evidence; scripted adapters are not Native ACK proof.

The adapters exercise Controller ownership and failure ordering against actual
Config and journal files. Separate Native recovery tests prove callback adoption.
"""

import hashlib
import json
import os
import subprocess
import sys
from collections import Counter
from contextlib import nullcontext
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller.copy_arrangement_journal import (
    CopyArrangementJournal,
    CopyJournalOwner,
    copy_after_snapshot,
    project_with_journal_copy,
    validated_copy_history,
)
from flitzis_looper.controller.copy_arrangement_startup import CopyArrangementStartup
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.pair_arrangement_journal import (
    PairArrangementAfterImage,
    PairArrangementJournal,
    PairJournalOwner,
)
from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.copy_snapshot import (
    CONTENT_FIELDS,
    capture_copy_snapshot,
    musical_snapshot_projection,
)
from flitzis_looper.material_migration_model import MaterialMigrationJournal, MigrationAssignment
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState, StemCacheEntry
from flitzis_looper.pair_snapshot import capture_pair_snapshot
from flitzis_looper_audio import MaterialMigrationJournalStore

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
    from flitzis_looper.controller.copy_arrangement_startup import OriginalCopyRecoveryHandoff
    from flitzis_looper_audio import InputRuntimePadBinding, RecoveredCopyAcceptance

_TRANSACTION = "a" * 32
_SOURCE = 0
_TARGET = 215
_SOURCE_ID = "1" * 32
_PREDECESSOR_ID = "2" * 32
_COPY_ID = "3" * 32


class _Acceptance:
    """Scripted Controller receipt; intentionally has no Native authority."""

    def __init__(self, audio: _Audio, binding: InputRuntimePadBinding) -> None:
        self.audio = audio
        self.binding = binding
        self.released = False

    @property
    def native_view(self) -> RecoveredCopyAcceptance:
        return cast("RecoveredCopyAcceptance", self)


class _Audio:
    """Require only the exact target binding; fail accidental A or mutation paths."""

    def __init__(self) -> None:
        self.binding = cast("InputRuntimePadBinding", object())
        self.source_binding = cast("InputRuntimePadBinding", object())
        self.allow_source = False
        self.binding_available = True
        self.ready = False
        self.valid = True
        self.lookups: list[int] = []
        self.captures: list[_Acceptance] = []
        self.validations: list[_Acceptance] = []
        self.releases: list[_Acceptance] = []
        self.release_pressure = 0
        self.release_error = False

    def current_input_runtime_pad_binding(self, sample_id: int) -> InputRuntimePadBinding | None:
        assert sample_id == _TARGET or (self.allow_source and sample_id == _SOURCE), (
            "recognized C must not look up surviving A"
        )
        self.lookups.append(sample_id)
        if not self.binding_available or (self.allow_source and not self.ready):
            return None
        return self.source_binding if sample_id == _SOURCE else self.binding

    def capture_recovered_pad_copy(
        self, binding: InputRuntimePadBinding
    ) -> RecoveredCopyAcceptance:
        assert binding is self.binding
        if not self.ready:
            message = "restored C lacks actual source/selection/timing acceptance"
            raise RuntimeError(message)
        acceptance = _Acceptance(self, binding)
        self.captures.append(acceptance)
        return acceptance.native_view

    def validate_recovered_pad_copy(self, acceptance: RecoveredCopyAcceptance) -> bool:
        captured = self.captures[-1]
        assert captured.audio is self
        assert acceptance is captured.native_view
        self.validations.append(captured)
        return self.valid and not captured.released

    def release_recovered_pad_copy(self, acceptance: RecoveredCopyAcceptance) -> bool:
        captured = self.captures[-1]
        assert captured.audio is self
        assert acceptance is captured.native_view
        self.releases.append(captured)
        if self.release_pressure:
            self.release_pressure -= 1
            if self.release_error:
                message = "recovered C reservation release is temporarily unavailable"
                raise RuntimeError(message)
            return False
        captured.released = True
        return True

    def capture_pad_copy(self, *args: object) -> None:
        pytest.fail("recognized C must not prepare another Copy")

    def publish_pad_copy(self, *args: object) -> None:
        pytest.fail("recognized C must not publish another Copy")

    def capture_recovered_pad_pair(self, *args: object) -> None:
        pytest.fail("Copy recovery must not reserve surviving A through Pair")


class _Stems:
    """Scripted selection gate; dedicated Stem/Native tests prove actual receipts."""

    def __init__(self) -> None:
        self.ready = True
        self.calls: list[tuple[int, StemCacheEntry | None]] = []

    def recovered_copy_selection_ready(self, sample_id: int, entry: StemCacheEntry | None) -> bool:
        assert sample_id == _TARGET
        self.calls.append((sample_id, entry))
        return self.ready


class _Reader:
    """Independent scripted asset lease retaining an actual fixture file reader."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self.handle = (path / "fixture.pin" if path.is_dir() else path).open("rb")
        self.released = False
        self.release_pressure = 0

    def release(self) -> None:
        if self.release_pressure:
            self.release_pressure -= 1
            message = "journal asset reader release temporarily unavailable"
            raise RuntimeError(message)
        self.handle.close()
        self.released = True


class _Retirement:
    def __init__(self) -> None:
        self.closed = 0
        self.activations = 0

    def activate(self) -> nullcontext[_Retirement]:
        self.activations += 1
        return nullcontext(self)

    def close(self) -> None:
        self.closed += 1


class _Assets:
    """Open actual fixture paths once per seal, including equal A/C paths."""

    def __init__(self, project: ProjectState) -> None:
        self.project = project
        self.retirement = _Retirement()
        self.readers: list[_Reader] = []
        self.synced: list[list[str | None]] = []

    @property
    def native_view(self) -> ProjectAssetLifecycle:
        return cast("ProjectAssetLifecycle", self)

    def reserve(self, capacity: int) -> _Retirement:
        assert capacity == 64
        return self.retirement

    def acquire(self, path: Path) -> _Reader:
        assert path.exists()
        reader = _Reader(path)
        self.readers.append(reader)
        return reader

    def sync_assignments(self) -> None:
        self.synced.append(list(self.project.sample_paths))


def _captured(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, stems: bool = False
) -> tuple[ProjectPersistence, CopyArrangementJournal]:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir(exist_ok=True)
    for name in ("A.wav", "B.wav"):
        (samples / name).write_bytes(f"actual fixture {name}".encode())
    project = ProjectState(key_lock=True)
    for slot, name, content_id in (
        (_SOURCE, "A.wav", _SOURCE_ID),
        (_TARGET, "B.wav", _PREDECESSOR_ID),
    ):
        project.sample_paths[slot] = f"samples/{name}"
        project.pad_content[slot] = PadContentIdentity(
            instance_id=content_id, material_id=("4" if slot == _SOURCE else "5") * 32
        )
        if stems:
            reference = f"samples/stems/#{slot + 1}"
            directory = tmp_path / reference
            directory.mkdir(parents=True)
            (directory / "fixture.pin").write_bytes(f"{name} selected stems".encode())
            project.stem_cache[slot] = StemCacheEntry(
                source_version=f"source-{name}", cache_dir=reference
            )
    project.pad_gain_db[_SOURCE] = -6.0
    project.pad_gain_db[_TARGET] = -12.0
    project.pad_gain_db[36] = -3.0
    project.manual_bpm[_SOURCE] = 123.456789
    project.pad_timing_intent[_SOURCE] = "tap"
    project.pad_loop_start_s[_SOURCE] = 1.25
    project.pad_loop_end_s[_SOURCE] = 12.5
    persistence = ProjectPersistence(project)
    persistence.config_path = samples / "copy.config.json"
    persistence.flush()
    revision, image, digest = persistence.capture_migration(_TRANSACTION)
    source = capture_copy_snapshot(image, _SOURCE)
    identity = PadContentIdentity(
        instance_id=_COPY_ID,
        material_id=source.projected().source_identity.material_id,
        copied_from=_SOURCE_ID,
    )
    return persistence, CopyArrangementJournal(
        transaction_id=_TRANSACTION,
        config_reference=persistence.config_reference,
        captured_revision=revision,
        intent_revision=revision,
        captured_config_sha256=digest,
        source_id=_SOURCE,
        target_id=_TARGET,
        source=source,
        predecessor=capture_copy_snapshot(image, _TARGET),
        after=copy_after_snapshot(source, identity),
        resolved_key_lock=True,
        captured_project_image_json=image.model_dump_json(),
        project_image_json=image.model_dump_json(),
        image_role="before",
        phase="captured",
    )


def _prior(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    after: bool = True,
    recorded: bool = True,
    source_change: str | None = None,
    stems: bool = False,
) -> tuple[Path, CopyArrangementJournal]:
    persistence, journal = _captured(tmp_path, monkeypatch, stems=stems)
    owner = CopyJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    pending = owner.append(journal.changed(phase="publication_pending"))
    current = owner.append(pending.changed(phase="native_ack_observed"))
    if source_change is not None:
        if source_change == "deleted":
            persistence.project.sample_paths[_SOURCE] = None
            persistence.project.pad_content[_SOURCE] = None
        elif source_change == "aba":
            persistence.project.pad_content[_SOURCE] = PadContentIdentity(
                instance_id="7" * 32, material_id="4" * 32
            )
        elif source_change == "edited":
            persistence.project.pad_gain_db[_SOURCE] = -24.0
        persistence.project.volume = 0.25
        persistence.project.pad_gain_db[36] = -22.0
        persistence.mark_dirty()
        current = owner.append(
            current.changed(
                intent_revision=persistence.revision,
                project_image_json=persistence.project.model_dump_json(),
            )
        )
    if after:
        candidate = project_with_journal_copy(persistence.project, current)
        receipt = persistence.commit_migration_receipt(
            _TRANSACTION, persistence.revision, candidate
        )
        if recorded:
            current = owner.append(current.committed(receipt))
    persistence.release_migration(_TRANSACTION)
    return persistence.config_path, current


@dataclass(slots=True)
class _App:
    persistence: ProjectPersistence
    project: ProjectState
    session: SessionState
    _audio: _Audio
    _assets: _Assets
    stems: _Stems


def _app(config: Path) -> _App:
    persistence = ProjectPersistence.from_config_path(config)
    persistence.config_path = config
    return _App(
        persistence=persistence,
        project=persistence.project,
        session=SessionState(),
        _audio=_Audio(),
        _assets=_Assets(persistence.project),
        stems=_Stems(),
    )


def _startup(app: _App) -> CopyArrangementStartup:
    return CopyArrangementStartup(cast("AppController", app))


def _before_image(project: ProjectState, journal: CopyArrangementJournal) -> ProjectState:
    data = project.model_dump(mode="python", exclude_computed_fields=True)
    predecessor = musical_snapshot_projection(journal.predecessor.projected())
    for field, array in CONTENT_FIELDS:
        data[array][journal.target_id] = predecessor[field]
    return ProjectState.model_validate(data)


@pytest.mark.parametrize("missing_slot", [_SOURCE, _TARGET])
def test_custody_retry_and_ordinary_loader_preserve_missing_original_participant(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, missing_slot: int
) -> None:
    config, _journal = _prior(tmp_path, monkeypatch, after=False)
    app = _app(config)
    missing_reference = app.project.sample_paths[missing_slot]
    assert missing_reference is not None
    missing_path = tmp_path / missing_reference
    original_bytes = missing_path.read_bytes()
    missing_path.unlink()
    startup = _startup(app)
    owner = app.persistence._migration_owner
    assert owner == _TRANSACTION
    assert startup.fenced_sample_ids == frozenset({_SOURCE, _TARGET})
    before = app.project.model_dump_json()
    readers = list(app._assets.readers)
    loader = LoaderController.__new__(LoaderController)
    loader._project, loader._session = app.project, app.session
    loader._deferred_restores, loader._deferred_restore_content = {}, {}
    loader._load_request_ids, loader._new_load_sample_ids = {}, set()
    loader._bind_restored_assignment_guard(lambda slot: slot in startup.fenced_sample_ids)
    monkeypatch.setattr(loader, "_output_sample_rate_hz", lambda: 48000)
    monkeypatch.setattr(loader, "_schedule_restored_load", lambda *_args, **_kwargs: True)
    monkeypatch.setattr(
        loader, "_clear_restored_pad", lambda _slot: pytest.fail("cleared journal A/B")
    )
    monkeypatch.setattr(
        loader, "_mark_project_changed", lambda: pytest.fail("invented unsaved intent")
    )
    loader.restore_samples_from_project_state()
    assert app.project.model_dump_json() == before
    assert missing_slot in loader._deferred_restores
    assert missing_slot in app.session.loading_sample_ids
    startup.poll()
    assert app.persistence._migration_owner == owner
    assert app._assets.readers == readers
    missing_path.write_bytes(original_bytes)
    # A scripted Source ACK completes the actual Loader obligation. Native tests
    # separately prove which real source/adoption guards may grant that permission.
    app.session.loading_sample_ids.clear()
    app._audio.allow_source = app._audio.ready = True
    handoff = startup.poll()
    assert handoff is not None
    assert startup._custody.acquired
    assert app._assets.readers[: len(readers)] == readers
    assert app.project.model_dump_json() == before
    assert handoff.initial.target_identity.instance_id == _COPY_ID


def test_selected_c_waits_for_its_actual_selection_gate_with_restore_observers_reachable(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, journal = _prior(tmp_path, monkeypatch, stems=True)
    app = _app(config)
    startup = _startup(app)
    app._audio.ready = True
    app.stems.ready = False
    startup.poll()
    assert startup.active
    assert startup.status == "restore_selection"
    assert startup.observer_fenced_sample_ids == frozenset()
    assert app._audio.captures == []
    assert app._audio.lookups == []
    assert app.persistence._migration_owner == _TRANSACTION
    assert app.stems.calls == [(_TARGET, journal.after.projected().stem_cache)]
    app.stems.ready = True
    startup.poll()
    assert not startup.active
    assert len(app._audio.captures) == 1


def test_recovered_latest_ack_records_newer_intent_before_actual_replace_without_receipt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, _journal = _prior(tmp_path, monkeypatch, recorded=False)
    app = _app(config)
    startup = _startup(app)
    app._audio.ready = True
    app.project.volume = 0.125
    app.project.pad_gain_db[_SOURCE] = -30.0
    app.persistence.mark_dirty()
    original_commit = app.persistence.commit_migration_receipt

    def replace_then_fail(
        transaction_id: str, expected_revision: int, snapshot: ProjectState
    ) -> None:
        original_commit(transaction_id, expected_revision, snapshot)
        message = "process boundary after actual replacement"
        raise RuntimeError(message)

    monkeypatch.setattr(app.persistence, "commit_migration_receipt", replace_then_fail)
    startup.poll()
    assert startup.active
    latest = CopyJournalOwner.open(config.parent, _TRANSACTION).journal
    assert latest is not None
    assert latest.phase == "native_ack_observed"
    assert latest.intent_revision == 1
    reopened = _app(config)
    resumed = _startup(reopened)
    assert resumed.status == "restore_after"
    reopened._audio.ready = True
    resumed.poll()
    assert not resumed.active
    assert reopened.project.volume == 0.125
    assert reopened.project.pad_gain_db[_SOURCE] == -30.0
    copied_content = reopened.project.pad_content[_TARGET]
    assert copied_content is not None
    assert copied_content.instance_id == _COPY_ID


@pytest.mark.parametrize("recognized_after", [False, True])
def test_asset_sync_retry_tracks_recovered_intent_separately_from_successful_pin_acquisition(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, recognized_after: bool
) -> None:
    config, journal = _prior(tmp_path, monkeypatch, after=recognized_after)
    app = _app(config)
    if recognized_after:
        owner = CopyJournalOwner.open(config.parent, _TRANSACTION)
        pre_c = _before_image(app.project, journal)
        pre_c.volume = 0.125
        owner.append(
            journal.changed(
                phase="native_ack_observed",
                image_role="before",
                intent_revision=1,
                project_image_json=pre_c.model_dump_json(),
                committed_revision=None,
                committed_config_sha256=None,
            )
        )
    sync = app._assets.sync_assignments
    failures = 1

    def sync_once() -> None:
        nonlocal failures
        if failures:
            failures -= 1
            message = "assignment acquisition temporarily unavailable"
            raise RuntimeError(message)
        sync()

    monkeypatch.setattr(app._assets, "sync_assignments", sync_once)
    startup = _startup(app)
    assert startup.active
    assert startup._custody.acquired
    assert not startup._selected_ready
    assert startup.error == "assignment acquisition temporarily unavailable"
    readers = list(app._assets.readers)
    # Retrying the already applied recovery image must also preserve subsequent
    # unrelated Human intent, rather than restore the historical volume again.
    app.project.volume = 0.25
    app.persistence.mark_dirty()
    app._audio.allow_source = not recognized_after
    app._audio.ready = True
    handoff = startup.poll()
    assert app._assets.synced
    assert app._assets.readers == readers
    assert startup._selected_ready
    assert app.project.volume == 0.25
    if recognized_after:
        assert not startup.active
        assert handoff is None
    else:
        assert handoff is not None
        assert handoff.initial.target_identity.instance_id == _COPY_ID


def test_selection_change_across_native_capture_retains_acceptance_without_config_write(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, _journal = _prior(tmp_path, monkeypatch)
    app = _app(config)
    startup = _startup(app)
    app._audio.ready = True
    before = config.read_bytes()
    readiness = iter([True, False, True])
    monkeypatch.setattr(app.stems, "recovered_copy_selection_ready", lambda *_args: next(readiness))
    startup.poll()
    assert startup.active
    assert startup.error is not None
    assert "across its Native capture" in startup.error
    assert len(app._audio.captures) == 1
    assert config.read_bytes() == before
    assert startup.observer_fenced_sample_ids == frozenset({_TARGET})
    startup.poll()
    assert not startup.active
    assert len(app._audio.captures) == 1


def _child(
    config: Path,
    parent: CopyArrangementJournal,
    *,
    phase: str = "captured",
    child_id: str = "b" * 32,
) -> CopyArrangementJournal:
    app = _app(config)
    revision, project, _ = app.persistence.capture_migration(child_id)
    initial = parent.changed(
        transaction_id=child_id,
        resume_of=parent.transaction_id,
        captured_revision=revision,
        intent_revision=revision,
        project_image_json=project.model_dump_json(),
        image_role="before",
        phase="captured",
        committed_revision=None,
        committed_config_sha256=None,
    )
    owner = CopyJournalOwner(config.parent, initial)
    current = owner.append(initial)
    if phase == "cancelled":
        current = owner.append(current.changed(phase="cancelled"))
    elif phase != "captured":
        current = owner.append(current.changed(phase="publication_pending"))
        if phase != "publication_pending":
            current = owner.append(current.changed(phase="native_ack_observed"))
            if phase in {"config_committed", "settled"}:
                receipt = app.persistence.commit_migration_receipt(
                    child_id, revision, project_with_journal_copy(project, current)
                )
                current = owner.append(current.committed(receipt))
                if phase == "settled":
                    current = owner.append(current.changed(phase="settled"))
    app.persistence.release_migration(child_id)
    return current


def _settle_handoff(app: _App, handoff: OriginalCopyRecoveryHandoff) -> CopyArrangementJournal:
    pending = handoff.journal.append(handoff.initial.changed(phase="publication_pending"))
    acknowledged = handoff.journal.append(pending.changed(phase="native_ack_observed"))
    receipt = app.persistence.commit_migration_receipt(
        handoff.initial.transaction_id,
        app.persistence.revision,
        project_with_journal_copy(app.project, handoff.initial),
    )
    # A live coordinator mirrors its accepted fixed C before durable finish.
    actual = receipt.projected()
    for field in ProjectState.model_fields:
        setattr(app.project, field, getattr(actual, field))
    committed = handoff.journal.append(acknowledged.committed(receipt))
    return handoff.journal.append(committed.changed(phase="settled"))


@pytest.mark.parametrize("recorded", [False, True])
def test_actual_after_and_replace_before_append_wait_for_fresh_c_without_recopy(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, recorded: bool
) -> None:
    config, journal = _prior(tmp_path, monkeypatch, recorded=recorded)
    app = _app(config)
    before = config.read_bytes()
    startup = _startup(app)
    assert startup.active
    assert startup.fenced_sample_ids == frozenset({_TARGET})
    assert startup.observer_fenced_sample_ids == frozenset()
    assert app.persistence._migration_owner == _TRANSACTION
    with pytest.raises(PersistenceFenceError, match="inheritance"):
        startup.ensure_global_key_lock_edit_allowed()
    app.session.loading_sample_ids.add(_TARGET)
    assert startup.poll() is None
    assert app._audio.lookups == []
    app.session.loading_sample_ids.clear()
    app.session.analyzing_sample_ids.add(_TARGET)
    assert startup.poll() is None
    assert app._audio.lookups == []
    app.session.analyzing_sample_ids.clear()
    app._audio.binding_available = False
    startup.poll()
    assert startup.active
    assert app._audio.captures == []
    app._audio.binding_available = True
    startup.poll()
    assert startup.active
    assert app._audio.captures == []
    assert config.read_bytes() == before
    assert startup.observer_fenced_sample_ids == frozenset()
    app._audio.ready = True
    startup.poll()
    assert not startup.active
    assert startup.status == "completed"
    assert len(app._audio.captures) == 1
    acceptance = app._audio.captures[0]
    assert app._audio.releases == [acceptance]
    assert acceptance.released
    assert set(app._audio.lookups) == {_TARGET}
    assert app.project.pad_content[_SOURCE] is not None
    assert app.project.pad_content[_SOURCE].instance_id == _SOURCE_ID
    assert app.project.pad_content[_TARGET] == journal.target_identity
    reopened = CopyJournalOwner.open(config.parent, _TRANSACTION)
    assert reopened.journal is not None
    assert reopened.journal.phase == "settled"
    assert (
        reopened.journal.committed_config_sha256 == hashlib.sha256(config.read_bytes()).hexdigest()
    )
    assert app.persistence._migration_owner is None
    assert app._assets.retirement.closed == 1
    assert all(reader.released for reader in app._assets.readers)
    assert startup.fenced_sample_ids == startup.observer_fenced_sample_ids == frozenset()
    startup.ensure_global_key_lock_edit_allowed()


@pytest.mark.parametrize("source_change", ["edited", "deleted", "aba"])
@pytest.mark.parametrize("recorded", [False, True])
def test_recognized_fixed_c_preserves_newer_or_deleted_a_and_unrelated_intent(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    source_change: str,
    *,
    recorded: bool,
) -> None:
    config, journal = _prior(tmp_path, monkeypatch, recorded=recorded, source_change=source_change)
    app = _app(config)
    original = app.project.model_dump_json()
    startup = _startup(app)
    assert startup.fenced_sample_ids == frozenset({_TARGET})
    app.session.loading_sample_ids.add(_SOURCE)
    app._audio.ready = True
    startup.poll()
    assert not startup.active
    assert app.project.model_dump_json() == original
    assert app.project.pad_gain_db[_TARGET] == -6.0
    assert app.project.pad_content[_TARGET] == journal.target_identity
    assert app.project.pad_gain_db[36] == -22.0
    assert app.project.volume == 0.25
    if source_change == "deleted":
        assert app.project.pad_content[_SOURCE] is None
        assert app.project.sample_paths[_SOURCE] is None
    elif source_change == "aba":
        source_content = app.project.pad_content[_SOURCE]
        assert source_content is not None
        assert source_content.instance_id == "7" * 32
    else:
        assert app.project.pad_gain_db[_SOURCE] == -24.0
    assert app._audio.lookups == [_TARGET]


def test_independent_original_and_stem_pins_exist_before_restore_even_when_a_and_c_share_paths(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, stems=True)
    app = _app(config)
    startup = _startup(app)
    paths = [reader.path for reader in app._assets.readers]
    assert paths == [
        config.parent / "A.wav",
        config.parent / "stems" / "#1",
        config.parent / "B.wav",
        config.parent / "stems" / "#216",
        config.parent / "A.wav",
        config.parent / "stems" / "#1",
    ]
    assert len({id(reader) for reader in app._assets.readers}) == 6
    assert all(not reader.released and not reader.handle.closed for reader in app._assets.readers)
    assert startup._custody.acquired
    assert app._audio.captures == []
    assert startup.observer_fenced_sample_ids == frozenset()
    app._audio.ready = True
    startup.poll()
    assert not startup.active
    assert all(reader.released and reader.handle.closed for reader in app._assets.readers)


@pytest.mark.parametrize("missing", ["A.wav", "B.wav", "stems/#1", "stems/#216"])
def test_missing_original_or_selected_stem_custody_fences_before_fresh_acceptance(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, missing: str
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, stems=True)
    missing_path = config.parent / missing
    if missing_path.is_dir():
        (missing_path / "fixture.pin").unlink()
        missing_path.rmdir()
    else:
        missing_path.unlink()
    original = config.read_bytes()
    app = _app(config)
    app._audio.ready = True
    startup = _startup(app)
    startup.poll()
    assert startup.active
    assert startup.error is not None
    assert "custody" in startup.error
    assert app._audio.lookups == []
    assert app._audio.captures == app._audio.releases == []
    assert app.persistence._migration_owner == _TRANSACTION
    assert config.read_bytes() == original
    assert app._assets.retirement.closed == 0
    assert all(not reader.released for reader in app._assets.readers)
    expected_paths = [
        config.parent / "A.wav",
        config.parent / "stems" / "#1",
        config.parent / "B.wav",
        config.parent / "stems" / "#216",
        config.parent / "A.wav",
        config.parent / "stems" / "#1",
    ]
    assert Counter(reader.path for reader in app._assets.readers) == Counter(
        path for path in expected_paths if path.exists()
    )
    owned = list(app._assets.readers)
    journal = CopyJournalOwner.open(config.parent, _TRANSACTION).journal
    assert journal is not None
    with pytest.raises(RuntimeError, match="custody"):
        startup._custody.acquire(
            app._assets.native_view, tmp_path, (journal.source, journal.predecessor, journal.after)
        )
    assert app._assets.readers == owned, "retry must retain the same independently owned readers"


@pytest.mark.parametrize("release_error", [False, True])
def test_release_failure_retains_one_c_owner_and_commits_later_a_edits_without_recapture(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, release_error: bool
) -> None:
    config, journal = _prior(tmp_path, monkeypatch)
    app = _app(config)
    app._audio.ready = True
    app._audio.release_pressure = 1
    app._audio.release_error = release_error
    startup = _startup(app)
    startup.poll()
    acceptance = app._audio.captures[0]
    assert startup.active
    assert startup.observer_fenced_sample_ids == frozenset({_TARGET})
    assert app.persistence._migration_owner == _TRANSACTION
    assert all(not reader.released for reader in app._assets.readers)
    app.project.pad_gain_db[_SOURCE] = -25.0
    app.project.pad_gain_db[36] = -23.0
    app.project.volume = 0.2
    app.persistence.mark_dirty()
    startup.poll()
    assert not startup.active
    assert app._audio.captures == [acceptance]
    assert app._audio.releases == [acceptance, acceptance]
    current = ProjectState.model_validate_json(config.read_bytes())
    assert current.pad_gain_db[_SOURCE] == -25.0
    assert current.pad_gain_db[36] == -23.0
    assert current.volume == 0.2
    assert current.pad_content[_TARGET] == journal.target_identity
    reopened = CopyJournalOwner.open(config.parent, _TRANSACTION)
    records = validated_copy_history(reopened.encoded, _TRANSACTION)
    earlier_commit = next(record for record in records if record.phase == "config_committed")
    assert any(
        record.phase == "native_ack_observed"
        and record.intent_revision > earlier_commit.intent_revision
        for record in records
    ), "newer intent must record its complete pre-C image before Config replacement"


def test_stale_c_acceptance_keeps_writer_pins_and_config_until_same_owner_is_valid(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, _ = _prior(tmp_path, monkeypatch)
    app = _app(config)
    app._audio.ready = True
    app._audio.valid = False
    original = config.read_bytes()
    startup = _startup(app)
    startup.poll()
    acceptance = app._audio.captures[0]
    startup.poll()
    assert startup.active
    assert startup.status == "recovery_pending"
    assert startup._after is not None
    assert startup._after.acceptance is acceptance.native_view
    assert app._audio.captures == [acceptance]
    assert app._audio.releases == []
    assert all(not reader.released for reader in app._assets.readers)
    assert app.persistence._migration_owner == _TRANSACTION
    assert config.read_bytes() == original
    app._audio.valid = True
    startup.poll()
    assert not startup.active
    assert app._audio.releases == [acceptance]


def test_config_replace_signal_retries_actual_written_outcome_with_same_c_acceptance(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, recorded=False)
    app = _app(config)
    app._audio.ready = True
    startup = _startup(app)
    write = app.persistence._atomic_write_text
    failed = False

    def replace_then_signal(text: str) -> None:
        nonlocal failed
        write(text)
        if not failed:
            failed = True
            message = "lost Config replacement return"
            raise OSError(message)

    monkeypatch.setattr(app.persistence, "_atomic_write_text", replace_then_signal)
    startup.poll()
    acceptance = app._audio.captures[0]
    assert startup.active
    assert app._audio.releases == []
    assert all(not reader.released for reader in app._assets.readers)
    actual = config.read_bytes()
    startup.poll()
    assert not startup.active
    assert app._audio.captures == [acceptance]
    assert app._audio.releases == [acceptance]
    assert config.read_bytes() == actual


def test_newer_recorded_pre_c_intent_recovers_through_previous_actual_receipt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, committed = _prior(tmp_path, monkeypatch)
    owner = CopyJournalOwner.open(config.parent, _TRANSACTION)
    app = _app(config)
    app.project.pad_gain_db[_SOURCE] = -24.0
    app.project.volume = 0.35
    app.persistence.mark_dirty()
    before = _before_image(app.project, committed)
    owner.append(
        committed.changed(
            phase="native_ack_observed",
            image_role="before",
            intent_revision=app.persistence.revision,
            project_image_json=before.model_dump_json(),
            committed_revision=None,
            committed_config_sha256=None,
        )
    )
    original_config = config.read_bytes()
    reopened = _app(config)
    assert reopened.project.pad_gain_db[_SOURCE] == -6.0
    startup = _startup(reopened)
    assert startup.active
    assert reopened.project.pad_gain_db[_SOURCE] == -24.0
    assert reopened.project.volume == 0.35
    assert reopened.project.pad_content[_TARGET] == committed.target_identity
    assert config.read_bytes() == original_config
    reopened._audio.ready = True
    startup.poll()
    assert not startup.active
    current = ProjectState.model_validate_json(config.read_bytes())
    assert current.pad_gain_db[_SOURCE] == -24.0
    assert current.volume == 0.35
    assert current.pad_content[_TARGET] == committed.target_identity
    assert len(reopened._audio.captures) == 1


def test_original_handoff_retains_fixed_c_and_transfers_only_its_existing_writer(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, parent = _prior(
        tmp_path, monkeypatch, after=False, source_change="unrelated", stems=True
    )
    app = _app(config)
    original = config.read_bytes()
    startup = _startup(app)
    assert app.project.volume == 0.25
    assert app.project.pad_gain_db[36] == -22.0
    assert app.persistence.revision == parent.intent_revision
    assert startup.fenced_sample_ids == frozenset({_SOURCE, _TARGET})
    assert startup.observer_fenced_sample_ids == frozenset()
    app.session.loading_sample_ids.add(_SOURCE)
    assert startup.poll() is None
    app.session.loading_sample_ids.clear()
    app._audio.allow_source = True
    assert startup.poll() is None
    assert app.persistence._migration_owner == _TRANSACTION
    assert app._audio.captures == []
    app._audio.ready = True
    handoff = startup.poll()
    assert handoff is not None
    assert handoff.initial.transaction_id != parent.transaction_id
    assert handoff.initial.resume_of == parent.transaction_id
    assert handoff.initial.source == parent.source
    assert handoff.initial.predecessor == parent.predecessor
    assert handoff.initial.after == parent.after
    assert handoff.initial.target_identity == parent.target_identity
    assert handoff.transferred
    assert handoff.delivered
    assert handoff.source_binding is app._audio.source_binding
    assert handoff.target_binding is app._audio.binding
    assert app.persistence._migration_owner == handoff.initial.transaction_id
    assert handoff.custody is startup._custody
    assert handoff.retirement is startup._retirement
    assert app._audio.captures == []
    assert startup.observer_fenced_sample_ids == frozenset({_TARGET})
    assert startup.poll() is None
    assert config.read_bytes() == original
    _settle_handoff(app, handoff)
    startup.complete_original(handoff)
    assert not startup.active
    assert handoff.completed
    assert not (config.parent / ".material-migrations" / _TRANSACTION).exists()
    latest = CopyJournalOwner.open(config.parent, handoff.initial.transaction_id).journal
    assert latest is not None
    assert latest.phase == "settled"
    assert app.persistence._migration_owner == handoff.initial.transaction_id
    assert app._assets.retirement.closed == 0
    assert all(not reader.released for reader in app._assets.readers)


@pytest.mark.parametrize("when", ["recorded_before", "during_restore"])
def test_original_a_aba_never_replays_copy_from_new_occupant(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, when: str
) -> None:
    config, _ = _prior(
        tmp_path,
        monkeypatch,
        after=False,
        source_change="aba" if when == "recorded_before" else None,
    )
    app = _app(config)
    original_config = config.read_bytes()
    startup = _startup(app)
    if when == "during_restore":
        app.project.pad_content[_SOURCE] = PadContentIdentity(
            instance_id="7" * 32, material_id="4" * 32
        )
        app.persistence.mark_dirty()
    startup.poll()
    assert startup.active
    assert startup.error is not None
    assert "A/B changed" in startup.error
    target_content = app.project.pad_content[_TARGET]
    assert target_content is not None
    assert target_content.instance_id == _PREDECESSOR_ID
    assert config.read_bytes() == original_config
    assert app._audio.captures == app._audio.releases == []
    assert app.persistence._migration_owner == _TRANSACTION
    assert all(not reader.released for reader in app._assets.readers)


@pytest.mark.parametrize("phase", ["config_committed", "settled"])
def test_failed_owned_append_keeps_same_acceptance_and_independent_pins_for_retry(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, phase: str
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, recorded=False)
    app = _app(config)
    app._audio.ready = True
    startup = _startup(app)
    assert startup._after is not None
    owner = startup._after.owner
    append = owner.append
    failed = False

    def fail_once(record: CopyArrangementJournal) -> CopyArrangementJournal:
        nonlocal failed
        if record.phase == phase and not failed:
            failed = True
            message = "owned terminal append temporarily unavailable"
            raise OSError(message)
        return append(record)

    monkeypatch.setattr(owner, "append", fail_once)
    startup.poll()
    acceptance = app._audio.captures[0]
    assert startup.active
    assert app.persistence._migration_owner == _TRANSACTION
    assert startup._after is not None
    assert startup._after.acceptance is acceptance.native_view
    assert all(not reader.released for reader in app._assets.readers)
    assert app._assets.retirement.closed == 0
    startup.poll()
    assert not startup.active
    assert app._audio.captures == [acceptance]
    latest = CopyJournalOwner.open(config.parent, _TRANSACTION).journal
    assert latest is not None
    assert latest.phase == "settled"


def test_asset_reader_release_failure_keeps_terminal_recovery_writer_owned_for_retry(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, _ = _prior(tmp_path, monkeypatch)
    app = _app(config)
    app._audio.ready = True
    startup = _startup(app)
    app._assets.readers[-1].release_pressure = 1
    startup.poll()
    acceptance = app._audio.captures[0]
    assert startup.active
    latest = CopyJournalOwner.open(config.parent, _TRANSACTION).journal
    assert latest is not None
    assert latest.phase == "settled"
    assert app.persistence._migration_owner == _TRANSACTION
    assert app._assets.retirement.closed == 0
    assert startup._custody.readers
    startup.poll()
    assert not startup.active
    assert app._audio.captures == [acceptance]
    assert app.persistence._migration_owner is None
    assert all(reader.released for reader in app._assets.readers)


@pytest.mark.parametrize(
    "damage", ["source_history", "target_history", "unknown_kind", "partial_history"]
)
def test_invalid_complete_history_is_preserved_with_no_native_or_cleanup_admission(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, damage: str
) -> None:
    config, _ = _prior(tmp_path, monkeypatch)
    directory = config.parent / ".material-migrations" / _TRANSACTION
    leaf = directory / "00.json"
    if damage == "partial_history":
        (directory / "04.json").write_text("{", encoding="utf-8")
    else:
        raw = json.loads(leaf.read_text(encoding="utf-8"))
        if damage == "unknown_kind":
            raw["kind"] = "future_copy"
        else:
            raw["source_id" if damage == "source_history" else "target_id"] = 36
        leaf.write_text(json.dumps(raw), encoding="utf-8")
    history = {entry.name: entry.read_bytes() for entry in directory.glob("*.json")}
    original = config.read_bytes()
    app = _app(config)
    app._audio.ready = True
    startup = _startup(app)
    startup.poll()
    assert startup.active
    assert startup.error
    assert startup.fenced_sample_ids == frozenset(range(216))
    assert startup.observer_fenced_sample_ids == frozenset()
    assert app._audio.lookups == []
    assert app._audio.captures == []
    assert app._assets.readers == []
    assert config.read_bytes() == original
    assert {entry.name: entry.read_bytes() for entry in directory.glob("*.json")} == history
    assert app.persistence._migration_owner is not None


@pytest.mark.parametrize(
    "damage", ["missing_parent", "two_roots", "fork", "cycle", "changed_fixed_c"]
)
def test_missing_or_ambiguous_copy_lineage_never_steals_writer_or_changes_history(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, damage: str
) -> None:
    config, parent = _prior(tmp_path, monkeypatch, after=False)
    child = _child(config, parent)
    if damage == "missing_parent":
        replacement = child.changed(resume_of="f" * 32)
        (config.parent / ".material-migrations" / child.transaction_id / "00.json").write_text(
            replacement.model_dump_json(), encoding="utf-8"
        )
    elif damage in {"two_roots", "fork"}:
        extra = _child(config, parent, child_id="c" * 32)
        if damage == "two_roots":
            (config.parent / ".material-migrations" / extra.transaction_id / "00.json").write_text(
                extra.changed(resume_of=None).model_dump_json(), encoding="utf-8"
            )
    elif damage == "cycle":
        for leaf in (config.parent / ".material-migrations" / _TRANSACTION).glob("*.json"):
            raw = json.loads(leaf.read_text(encoding="utf-8"))
            raw["resume_of"] = child.transaction_id
            leaf.write_text(json.dumps(raw), encoding="utf-8")
    else:
        identity = child.target_identity.model_copy(update={"instance_id": "8" * 32})
        replaced = child.changed(after=copy_after_snapshot(child.source, identity))
        (config.parent / ".material-migrations" / child.transaction_id / "00.json").write_text(
            replaced.model_dump_json(), encoding="utf-8"
        )
    directory = config.parent / ".material-migrations"
    history = {
        str(leaf.relative_to(directory)): leaf.read_bytes() for leaf in directory.glob("*/*.json")
    }
    original = config.read_bytes()
    app = _app(config)
    startup = _startup(app)
    startup.poll()
    assert startup.active
    assert startup.error
    assert app._audio.captures == []
    assert app._assets.readers == []
    assert app.persistence._migration_owner is not None
    assert config.read_bytes() == original
    assert {
        str(leaf.relative_to(directory)): leaf.read_bytes() for leaf in directory.glob("*/*.json")
    } == history


@pytest.mark.parametrize("other_kind", ["material", "pair", "writer"])
def test_unfinished_mixed_owners_are_visible_without_acquiring_their_writer_or_readers(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, other_kind: str
) -> None:
    config, journal = _prior(tmp_path, monkeypatch, after=False)
    app = _app(config)
    other_id = "b" * 32
    if other_kind == "writer":
        app.persistence.capture_migration(other_id)
    elif other_kind == "material":
        material = MaterialMigrationJournal(
            transaction_id=other_id,
            config_reference=journal.config_reference,
            captured_revision=0,
            intent_revision=0,
            config_sha256=journal.captured_config_sha256,
            snapshot_json=journal.project_image_json,
            assignments=(
                MigrationAssignment(
                    sample_id=_SOURCE, instance_id=_SOURCE_ID, old_reference="samples/A.wav"
                ),
            ),
        )
        MaterialMigrationJournalStore(str(config.parent), other_id).append(
            material.model_dump_json()
        )
    else:
        before = capture_pair_snapshot(app.project, _SOURCE, _TARGET, operation="swap")
        pair = PairArrangementJournal(
            transaction_id=other_id,
            config_reference=journal.config_reference,
            captured_revision=0,
            intent_revision=0,
            captured_config_sha256=journal.captured_config_sha256,
            before=before,
            after=PairArrangementAfterImage.from_before(before),
            project_image_json=app.project.model_dump_json(),
            image_role="before",
            phase="captured",
        )
        PairJournalOwner(config.parent, pair).append(pair)
    original = config.read_bytes()
    startup = _startup(app)
    startup.poll()
    assert startup.active
    assert startup.error
    assert app._audio.captures == []
    assert app._assets.readers == []
    assert config.read_bytes() == original
    assert app.persistence._migration_owner == (
        other_id if other_kind == "writer" else startup._fence
    )
    assert not app.persistence.flush_if_dirty()


@pytest.mark.parametrize(
    "changed", ["newer_config", "fixed_c", "global_key_lock", "unsaved_original"]
)
def test_changed_intent_remains_current_and_fenced_without_historical_repair(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, changed: str
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, after=changed != "unsaved_original")
    if changed == "newer_config":
        raw = json.loads(config.read_text(encoding="utf-8"))
        raw["volume"], raw["config_revision"] = 0.2, 10
        config.write_text(json.dumps(raw), encoding="utf-8")
    app = _app(config)
    if changed == "fixed_c":
        app.project.pad_gain_db[_TARGET] = -18.0
        app.persistence.mark_dirty()
    elif changed == "global_key_lock":
        app.project.key_lock = False
        app.persistence.mark_dirty()
    elif changed == "unsaved_original":
        app.project.volume = 0.15
        app.persistence.mark_dirty()
    before_config = config.read_bytes()
    before_project = app.project.model_dump_json()
    startup = _startup(app)
    app._audio.ready = True
    startup.poll()
    assert startup.active
    assert startup.error
    assert app.project.model_dump_json() == before_project
    assert config.read_bytes() == before_config
    assert app._audio.captures == []
    assert app.persistence._migration_owner is not None


@pytest.mark.parametrize(
    "phase", ["captured", "native_ack_observed", "config_committed", "settled", "cancelled"]
)
def test_fresh_subprocess_reopens_complete_copy_lineage_and_fixed_c(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, phase: str
) -> None:
    config, parent = _prior(tmp_path, monkeypatch, after=False)
    child = _child(config, parent, phase=phase)
    original = config.read_bytes()
    script = """
import json, sys
from pathlib import Path
from tests.flitzis_looper.controller.test_copy_arrangement_startup import _app, _startup
app = _app(Path(sys.argv[1]))
startup = _startup(app)
app._audio.allow_source = startup._original is not None
app._audio.ready = True
handoff = startup.poll()
print(json.dumps({
    'active': startup.active, 'status': startup.status, 'error': startup.error,
    'captures': len(app._audio.captures), 'lookups': app._audio.lookups,
    'handoff_parent': None if handoff is None else handoff.initial.resume_of,
    'fixed_c': None if handoff is None else handoff.initial.target_identity.instance_id,
    'ancestors': [] if handoff is None else [
        owner.initial.transaction_id for owner in handoff.ancestors
    ],
    'slots': [
        app.project.pad_content[i].instance_id if app.project.pad_content[i] else None
        for i in [0, 215]
    ],
}))
"""
    process = subprocess.run(
        [sys.executable, "-c", script, str(config)],
        cwd=tmp_path,
        env=os.environ | {"PYTHONPATH": str(Path(__file__).resolve().parents[3])},
        capture_output=True,
        text=True,
        check=True,
        timeout=30,
    )
    actual = json.loads(process.stdout.strip().splitlines()[-1])
    assert actual["error"] is None
    if phase in {"captured", "native_ack_observed"}:
        assert actual["active"]
        assert actual["handoff_parent"] == child.transaction_id
        assert actual["fixed_c"] == _COPY_ID
        assert actual["ancestors"] == [_TRANSACTION, child.transaction_id]
        assert actual["captures"] == 0
        assert actual["slots"] == [_SOURCE_ID, _PREDECESSOR_ID]
    else:
        assert not actual["active"]
        assert actual["status"] == "completed"
        assert actual["handoff_parent"] is None
        assert actual["captures"] == (0 if phase == "cancelled" else 1)
        assert actual["lookups"] == ([] if phase == "cancelled" else [_TARGET])
        assert actual["slots"] == [
            _SOURCE_ID,
            _PREDECESSOR_ID if phase == "cancelled" else _COPY_ID,
        ]
        assert not (config.parent / ".material-migrations" / _TRANSACTION).exists()
    assert config.read_bytes() == original
