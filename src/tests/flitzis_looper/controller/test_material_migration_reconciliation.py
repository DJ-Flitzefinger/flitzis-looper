"""Real journal/config recovery with explicit controller-only audio and PCM doubles.

Original/WAV receipts and inventory tests use native filesystem proofs. Fake source,
timing, PCM preparation and immediate worker settlement certify controller policy,
not native callbacks, PCM recognition, realtime behavior or audible output.
"""

import hashlib
import json
from concurrent.futures import Future
from pathlib import Path
from threading import Event, get_ident
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import AppController
from flitzis_looper.controller import material_migration as migration_module
from flitzis_looper.controller.material_migration_cleanup import (
    ArtifactCapture,
    ArtifactRequest,
    CleanupResult,
    MigrationArtifactReconciler,
    MigrationCleanupQueue,
)
from flitzis_looper.controller.material_migration_recovery import inspect_migration_recovery
from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.key_intent import MAX_KEY_EPOCH
from flitzis_looper.material_migration_model import (
    MaterialMigrationJournal,
    MigrationArtifactEvidence,
    MigrationArtifactRecord,
    MigrationAssignment,
)
from flitzis_looper.models import STEM_KINDS, PadContentIdentity, ProjectState, StemCacheEntry
from flitzis_looper_audio import (
    MaterialMigrationJournalStore,
    MigrationArtifactLease,
    MigrationProjectGuard,
)
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import write_test_stem_marker
from tests.flitzis_looper.controller.test_material_migration import (
    _IDS,
    _MATERIAL,
    _NEW,
    _OLD,
    _DeliveredLease,
    _Harness,
    _Hold,
    _Preparation,
    _SourceTicket,
    _StemWork,
)

if TYPE_CHECKING:
    from collections.abc import Iterable


def _complete[T](value: T) -> Future[T]:
    result: Future[T] = Future()
    result.set_result(value)
    return result


class _LedgerPreparation(_Preparation):
    """Fake decoded PCM; original bytes and target path are real and preexisting."""

    def __init__(self, source: str, target: str, material: str) -> None:
        super().__init__()
        self.source = source
        self.target = target
        self.material = material

    def metadata_json(self) -> str:
        original = Path(self.source)
        return json.dumps({
            "old_reference": self.source,
            "new_reference": self.target,
            "material_id": self.material,
            "original": {
                "sha256": hashlib.sha256(original.read_bytes()).hexdigest(),
                "bytes": original.stat().st_size,
            },
            "decoder_identity": "b" * 64,
            "playback_identity": "c" * 64,
            "cache_path": f"samples/materials/M{self.material}/.pcm-cache/v1/.ready-{'d' * 32}",
            "artifact_ledger_schema": 1,
            "created_original": False,
            "created_cache": False,
        })


class _ControlProofWorker(MigrationArtifactReconciler):
    """Immediate control seam: native original/WAV proofs, explicitly absent fake PCM.

    A request for fake PCM is recorded rather than producing a fabricated receipt.
    Cleanup remains pending without deleting files; actual reconciliation is tested
    separately with the unmodified production worker and native inventory.
    """

    def __init__(self, samples: Path, config: Path) -> None:
        super().__init__(samples, config)
        self.requests: list[tuple[ArtifactRequest, ...]] = []
        self.captures: list[ArtifactCapture] = []
        self.retirements: list[MaterialMigrationJournal] = []
        self.finished = False

    def capture(self, requests: Iterable[ArtifactRequest]) -> Future[ArtifactCapture]:
        requested = tuple(requests)
        self.requests.append(requested)
        actual = tuple(item for item in requested if ".pcm-cache" not in item.reference)
        capture = self._capture(actual) if actual else ArtifactCapture((), [])
        self.captures.append(capture)
        return _complete(capture)

    def retire(
        self, journal: MaterialMigrationJournal, current: ProjectState
    ) -> Future[CleanupResult]:
        self.retirements.append(journal)
        pending = tuple(
            item.evidence.reference for item in journal.artifacts if item.role == "rollback"
        )
        return _complete(CleanupResult(queued=pending, pending=pending))

    def settled(self, result: CleanupResult) -> bool:
        return self.finished


class _RecoveryRig:
    def __init__(
        self, controller: AppController, audio: Mock, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        self.base = _Harness(controller, audio)
        self.controller = controller
        self.audio = audio
        self.sources: dict[int, _SourceTicket] = {}
        self.all_sources: list[_SourceTicket] = []
        self.holds: list[_Hold] = []
        self.workers: list[_ControlProofWorker] = []
        self.preparations = {_OLD: _LedgerPreparation(_OLD, _NEW, _MATERIAL)}
        self.next_request = 20_000

        def proof_worker(samples: Path, config: Path) -> _ControlProofWorker:
            worker = _ControlProofWorker(samples, config)
            self.workers.append(worker)
            return worker

        def hold(_ids: list[int]) -> _Hold:
            result = _Hold()
            self.holds.append(result)
            return result

        def adopt(sample_id: int, preparation: object) -> _SourceTicket:
            assert preparation in self.preparations.values()
            ticket = _SourceTicket(sample_id)
            ticket.request_id = self.next_request
            self.next_request += 1
            self.sources[sample_id] = ticket
            self.all_sources.append(ticket)
            return ticket

        monkeypatch.setattr(migration_module, "ProjectAssetLease", _DeliveredLease)
        monkeypatch.setattr(migration_module, "MigrationArtifactReconciler", proof_worker)
        audio.migration_artifact_ledger_supported.return_value = True
        audio.hold_material_migration = Mock(side_effect=hold)
        audio.prepare_material_migration = Mock(
            side_effect=lambda source: self.preparations[source]
        )
        audio.adopt_material_migration = Mock(side_effect=adopt)
        audio.pad_timing_intent.side_effect = lambda sample_id: (
            self.controller.project.pad_timing_intent[sample_id]
        )
        audio.load_sample_async.side_effect = lambda sample_id, *_args, **_kwargs: (
            50_000 + sample_id
        )
        audio.poll_loader_events.side_effect = None
        audio.poll_loader_events.return_value = None
        controller.material_migration = migration_module.MaterialMigrationController(
            controller.persistence, controller.session, audio, controller._assets, controller.loader
        )
        self.controllers = [controller]
        assert self.service._ledger_enabled is True

    @property
    def service(self) -> migration_module.MaterialMigrationController:
        return self.controller.material_migration

    @property
    def persistence(self) -> ProjectPersistence:
        return self.controller.persistence

    def records(self, transaction: str) -> list[MaterialMigrationJournal]:
        return [
            MaterialMigrationJournal.model_validate_json(path.read_bytes())
            for path in sorted((Path("samples/.material-migrations") / transaction).glob("*.json"))
        ]

    def pending(self) -> str:
        transaction = self.service.begin(0)
        self.service.poll()
        self.service.poll()
        assert self.service.status == "adoption_pending"
        assert set(self.sources) == set(_IDS)
        return transaction

    def reopen(self) -> None:
        config = self.persistence.config_path.resolve()
        self.service.shut_down()
        # A closed process owns no journal-directory handle. Runtime audio below
        # is intentionally a controller double, not a retained parent permission.
        self.service._store = None
        self.sources = {}
        self.audio.poll_loader_events.side_effect = None
        self.controller = AppController(project_config_path=config)
        self.controllers.append(self.controller)
        # Settle ordinary startup requests through their actual event-routing path.
        # These are control fakes, not native source ACKs.
        events = [
            {
                "type": "success",
                "id": sample_id,
                "request_id": self.controller.loader._load_request_ids[sample_id],
                "cached_path": self.controller.project.sample_paths[sample_id],
            }
            for sample_id in tuple(self.controller.session.loading_sample_ids)
        ]
        self.audio.poll_loader_events.side_effect = [*events, None]
        self.controller.loader.poll_loader_events()
        self.audio.poll_loader_events.side_effect = None
        self.audio.poll_loader_events.return_value = None
        assert not self.controller.session.loading_sample_ids
        assert self.service._ledger_enabled is True

    def acknowledge(self) -> None:
        for ticket in self.sources.values():
            ticket.state = "acknowledged"
        self.service.poll()
        if self.service._history_future is not None:
            self.service._history_future.result(timeout=10)
            self.service.poll()

    def close(self) -> None:
        for controller in self.controllers:
            controller.material_migration.shut_down()
            controller.loader.shut_down()
            controller._assets.release_saved_assignments()
            # Close test-local directory guards even if a Mock closure keeps the
            # controller alive. Durable journal bytes are deliberately untouched.
            controller.material_migration._store = None
        for worker in self.workers:
            for capture in worker.captures:
                capture.release()
            worker.shut_down()


@pytest.fixture
def recovery(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> Iterable[_RecoveryRig]:
    result = _RecoveryRig(controller, audio_engine_mock, monkeypatch)
    try:
        yield result
    finally:
        result.close()


def _assert_actual_artifact_requests(
    recovery: _RecoveryRig, child: MaterialMigrationJournal
) -> None:
    assert child.artifacts
    assert all(item.evidence.samples_identity for item in child.artifacts)
    assert any(
        ".pcm-cache" in item.reference
        for request in recovery.workers[-1].requests
        for item in request
    )


def _assert_preserved_recovery_settings(saved: ProjectState, expected: ProjectState) -> None:
    assert saved.sample_paths[0] == saved.sample_paths[215] == _NEW
    assert saved.pad_key_intent == expected.pad_key_intent
    assert saved.pad_gain_db == expected.pad_gain_db
    assert saved.sample_analysis == expected.sample_analysis
    for sample_id in _IDS:
        content, old = saved.pad_content[sample_id], expected.pad_content[sample_id]
        assert content is not None
        assert old is not None
        assert content.instance_id == old.instance_id
        assert content.material_id == _MATERIAL


@pytest.mark.parametrize("old_phase", ["pending", "claimed", "acknowledged"])
def test_reopened_app_uses_child_journal_and_new_tickets_for_every_saved_audio_phase(
    recovery: _RecoveryRig, old_phase: str
) -> None:
    parent = recovery.pending()
    old_sources = dict(recovery.sources)
    for source in old_sources.values():
        source.state = old_phase
    project = recovery.controller.project
    project.pad_key_intent[0] = project.pad_key_intent[0].changed(extra_shift=17)
    project.pad_gain_db[215] = -8.0
    recovery.persistence.mark_dirty()
    expected = project.model_copy(deep=True)
    disk_before = recovery.persistence.config_path.read_bytes()
    recovery.reopen()

    assert recovery.service.status == "unresolved"
    assert not recovery.sources
    assert recovery.controller.project.pad_key_intent == expected.pad_key_intent
    assert recovery.controller.project.pad_key_intent[0].analysis_epoch == MAX_KEY_EPOCH
    assert recovery.controller.project.pad_gain_db == expected.pad_gain_db
    assert recovery.controller.project.pad_content == expected.pad_content
    assert recovery.persistence.config_path.read_bytes() == disk_before
    assert not recovery.persistence.flush_if_dirty()
    with pytest.raises(PersistenceFenceError):
        recovery.persistence.flush()

    recovery.service.poll()
    recovery.service.poll()
    child = recovery.service._journal
    assert child is not None
    assert child.transaction_id != parent
    assert child.resume_of == parent
    assert child.alias is not None
    assert child.alias.resume_of == parent
    assert set(recovery.sources) == set(_IDS)
    assert all(source.phase() == "pending" for source in recovery.sources.values())
    assert not {source.request_id for source in old_sources.values()} & {
        source.request_id for source in recovery.sources.values()
    }
    assert all(source not in old_sources.values() for source in recovery.sources.values())
    assert recovery.persistence.config_path.read_bytes() == disk_before
    _assert_actual_artifact_requests(recovery, child)

    recovery.acknowledge()
    assert recovery.records(child.transaction_id)[-1].phase == "config_committed"
    recovery.service.poll()
    saved = ProjectPersistence.from_config_path(recovery.persistence.config_path).project
    _assert_preserved_recovery_settings(saved, expected)
    assert recovery.persistence._migration_owner is None
    assert recovery.holds[-1].release_count == 1
    forbidden = {"source_ticket", "source_request_id", "source_phase", "native_ack"}
    assert not (Path("samples/.material-migrations") / parent).exists()
    assert all(
        forbidden.isdisjoint(item.model_dump()) for item in recovery.records(child.transaction_id)
    )


def test_newer_saved_config_wins_over_captured_digest_and_recovery_snapshot(
    recovery: _RecoveryRig,
) -> None:
    parent = recovery.pending()
    captured = recovery.records(parent)[0]
    newer = recovery.controller.project.model_copy(deep=True)
    newer.pad_key_intent[0] = newer.pad_key_intent[0].changed(extra_shift=-17)
    newer.pad_gain_db[215] = -13.0
    newer.volume = 0.25
    newer.config_revision = recovery.persistence.revision + 20
    writer = ProjectPersistence(newer)
    writer.config_path = recovery.persistence.config_path
    writer.flush()
    saved_bytes = writer.config_path.read_bytes()
    assert hashlib.sha256(saved_bytes).hexdigest() != captured.config_sha256
    recovery.reopen()
    assert recovery.controller.project.pad_key_intent == newer.pad_key_intent
    assert recovery.controller.project.pad_gain_db == newer.pad_gain_db
    assert recovery.controller.project.volume == 0.25
    recovery.service.poll()
    recovery.service.poll()
    recovery.acknowledge()
    saved = ProjectPersistence.from_config_path(writer.config_path).project
    assert saved.pad_key_intent == newer.pad_key_intent
    assert saved.pad_gain_db == newer.pad_gain_db
    assert saved.volume == 0.25
    assert recovery.service._journal is not None
    assert recovery.service._journal.config_sha256 == hashlib.sha256(saved_bytes).hexdigest()


@pytest.mark.parametrize("failure", ["transfer", "first_child_record"])
def test_failed_recovery_fence_transfer_or_first_record_never_opens_ordinary_writer(
    recovery: _RecoveryRig, monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    parent = recovery.pending()
    recovery.reopen()
    original_fence = recovery.persistence._migration_owner
    disk = recovery.persistence.config_path.read_bytes()
    holds_before = len(recovery.holds)
    if failure == "transfer":
        monkeypatch.setattr(
            recovery.persistence,
            "transfer_migration",
            Mock(side_effect=OSError("injected transfer capture failure")),
        )
    else:
        monkeypatch.setattr(
            migration_module,
            "MaterialMigrationJournalStore",
            Mock(side_effect=OSError("injected first child journal failure")),
        )
    recovery.service.poll()
    assert recovery.service.status == "unresolved"
    assert recovery.persistence._migration_owner is not None
    if failure == "transfer":
        assert recovery.persistence._migration_owner == original_fence
    assert recovery.persistence.config_path.read_bytes() == disk
    assert not recovery.persistence.flush_if_dirty()
    with pytest.raises(PersistenceFenceError):
        recovery.persistence.flush()
    assert len(recovery.holds) == holds_before
    assert recovery.holds[-1].release_count == 0
    assert not recovery.sources
    assert recovery.records(parent)[-1].phase == "unresolved"
    assert Path(_OLD).is_file()
    assert Path(_NEW).is_file()


def test_newer_regular_import_wins_before_child_subscriber_admission(
    recovery: _RecoveryRig,
) -> None:
    parent = recovery.pending()
    recovery.reopen()
    replacement = Path("samples/newer.wav")
    write_mono_pcm16_wav(replacement, 48_000)
    content = PadContentIdentity(instance_id="f" * 32)
    project = recovery.controller.project
    project.sample_paths[215] = replacement.as_posix()
    project.pad_content[215] = content
    project.pad_key_intent[215] = project.pad_key_intent[215].changed(extra_shift=11)
    expected_key = project.pad_key_intent[215]
    recovery.persistence.mark_dirty()
    recovery.service.poll()
    recovery.service.poll()
    assert set(recovery.sources) == {0}
    assert recovery.service._journal is not None
    assert recovery.service._journal.resume_of == parent
    recovery.acknowledge()
    saved = ProjectPersistence.from_config_path(recovery.persistence.config_path).project
    assert saved.sample_paths[0] == _NEW
    assert saved.sample_paths[215] == replacement.as_posix()
    assert saved.pad_content[215] == content
    assert saved.pad_key_intent[215] == expected_key


def _second_material(recovery: _RecoveryRig) -> str:
    source = "samples/second.wav"
    target = f"samples/materials/M{'9' * 32}/original/second.wav"
    Path(source).write_bytes(Path(_OLD).read_bytes())
    Path(target).parent.mkdir(parents=True)
    Path(target).write_bytes(Path(source).read_bytes())
    recovery.preparations[source] = _LedgerPreparation(source, target, "9" * 32)
    recovery.controller.project.sample_paths[215] = source
    recovery.persistence.mark_dirty()
    recovery.persistence.flush()
    return source


def test_multiple_pending_material_journals_get_serial_independent_children(
    recovery: _RecoveryRig,
) -> None:
    second = _second_material(recovery)
    expected = recovery.controller.project.model_copy(deep=True)
    parents = ("1" * 32, "2" * 32)
    for transaction, sample_id, reference in zip(parents, _IDS, (_OLD, second), strict=True):
        content = expected.pad_content[sample_id]
        assert content is not None
        journal = MaterialMigrationJournal(
            transaction_id=transaction,
            phase="adoption_pending",
            config_reference=recovery.persistence.config_reference,
            captured_revision=recovery.persistence.revision,
            intent_revision=recovery.persistence.revision,
            config_sha256=hashlib.sha256(recovery.persistence.config_path.read_bytes()).hexdigest(),
            snapshot_json=expected.model_dump_json(),
            assignments=(
                MigrationAssignment(
                    sample_id=sample_id, instance_id=content.instance_id, old_reference=reference
                ),
            ),
        )
        MaterialMigrationJournalStore(str(Path("samples").resolve()), transaction).append(
            journal.model_dump_json()
        )
    recovery.reopen()
    recovery_hold = recovery.holds[-1]
    assert len(recovery.service._recovery_pending) == 2
    children: list[str] = []
    for sample_id, parent in zip(_IDS, parents, strict=True):
        recovery.service.poll()
        recovery.service.poll()
        child = recovery.service._journal
        assert child is not None
        assert child.resume_of == parent
        children.append(child.transaction_id)
        assert child.assignments[0].sample_id == sample_id
        assert recovery.sources[sample_id].phase() == "pending"
        assert recovery_hold.release_count == 0
        recovery.acknowledge()
        assert recovery.records(child.transaction_id)[-1].phase == "config_committed"
    recovery.service.poll()
    assert len(set(children)) == 2
    assert not set(children) & set(parents)
    assert recovery_hold.release_count == 1
    assert recovery.persistence._migration_owner is None
    saved = ProjectPersistence.from_config_path(recovery.persistence.config_path).project
    assert saved.pad_key_intent == expected.pad_key_intent
    for sample_id, reference in zip(_IDS, (_OLD, second), strict=True):
        before, after = expected.pad_content[sample_id], saved.pad_content[sample_id]
        assert before is not None
        assert after is not None
        assert before.instance_id == after.instance_id
        assert saved.sample_paths[sample_id] == recovery.preparations[reference].target


def test_pending_physical_cleanup_does_not_block_next_distinct_material_adoption(
    recovery: _RecoveryRig,
) -> None:
    second = _second_material(recovery)
    before = recovery.controller.project.model_copy(deep=True)
    # Both fake PCM preparations describe a shared legacy playback identity. The
    # physical queue outcome is a control double; native reader lifetime is tested
    # by the Rust recovery/ColdStore oracles, not inferred from these metadata.
    assert (
        json.loads(recovery.preparations[_OLD].metadata_json())["playback_identity"]
        == (json.loads(recovery.preparations[second].metadata_json())["playback_identity"])
    )
    recovery.service.schedule_after_restore()
    recovery.service.poll()
    recovery.service.poll()
    assert set(recovery.sources) == {0}
    recovery.acknowledge()
    first = recovery.service._journal
    assert first is not None
    assert first.phase == "config_committed"
    # Simulate an already physically queued prior cleanup. Current batches defer
    # new physical cleanup until adoption ends; an existing real-reader wait must
    # still leave the next admission reachable. The outcome here is a policy double.
    recovery.service._cleanup._journal = first
    recovery.service._cleanup._pending = CleanupResult(queued=(_OLD,), pending=(_OLD,))
    recovery.service.poll()
    recovery.service.poll()
    assert recovery.service._cleanup.busy
    assert not recovery.service._cleanup.preparing
    assert set(recovery.sources) == set(_IDS)
    assert recovery.sources[215].phase() == "pending"
    assert recovery.records(first.transaction_id)[-1].cleanup_complete is False
    assert Path(_OLD).is_file()
    assert Path(second).is_file()
    recovery.acknowledge()
    saved = ProjectPersistence.from_config_path(recovery.persistence.config_path).project
    assert saved.sample_paths[0] == _NEW
    assert saved.sample_paths[215] == recovery.preparations[second].target
    assert saved.pad_key_intent == before.pad_key_intent
    assert [value.instance_id if value else None for value in saved.pad_content] == [
        value.instance_id if value else None for value in before.pad_content
    ]


def test_mixed_canonical_and_legacy_project_keeps_canonical_content_and_key_intent(
    recovery: _RecoveryRig,
) -> None:
    project = recovery.controller.project
    old_content = project.pad_content[0]
    assert old_content is not None
    project.sample_paths[0] = _NEW
    project.pad_content[0] = PadContentIdentity(
        instance_id=old_content.instance_id, material_id=_MATERIAL
    )
    work = _StemWork(state="ready", created=False)
    target = work.cache_reference()
    old_cache = Path("samples/stems/#216")
    new_cache = Path(target)
    old_cache.mkdir(parents=True)
    new_cache.mkdir(parents=True)
    digest = hashlib.sha256(Path(_OLD).read_bytes()).hexdigest()
    old_version, new_version = f"{_OLD}|sha256-v1:{digest}", f"{_NEW}|sha256-v1:{digest}"
    for cache, version in ((old_cache, old_version), (new_cache, new_version)):
        for kind in STEM_KINDS:
            write_mono_pcm16_wav(cache / f"{kind}.wav", 44_100)
        write_test_stem_marker(cache, version)
    project.stem_cache[0] = StemCacheEntry(
        source_version=new_version,
        cache_dir=target,
        stems=expected_stem_files(target),
        available=True,
    )
    project.stem_cache[215] = StemCacheEntry(
        source_version=old_version,
        cache_dir=old_cache.as_posix(),
        stems=expected_stem_files(old_cache.as_posix()),
        available=True,
    )
    old_wav_bytes = {path.name: path.read_bytes() for path in old_cache.iterdir()}
    recovery.audio.prepare_material_migration_stems.return_value = work
    # Publication acceptance is a controller double; the native WAV receipts are real.
    publication = Mock()
    publication.publication_status.return_value = "accepted"
    recovery.audio.capture_prepared_source.return_value = publication
    expected = project.model_copy(deep=True)
    recovery.persistence.mark_dirty()
    recovery.persistence.flush()
    recovery.service.schedule_after_restore()
    recovery.service.poll()
    recovery.service.poll()
    recovery.service.poll()  # Complete target WAV receipt before source admission.
    assert set(recovery.sources) == {215}
    assert recovery.audio.prepare_material_migration.call_args.args == (_OLD,)
    recovery.acknowledge()
    saved = ProjectPersistence.from_config_path(recovery.persistence.config_path).project
    assert saved.sample_paths[0] == expected.sample_paths[0] == _NEW
    assert saved.pad_content[0] == expected.pad_content[0]
    assert saved.pad_key_intent == expected.pad_key_intent
    assert saved.sample_paths[215] == _NEW
    assert saved.stem_cache[0] == expected.stem_cache[0]
    assert saved.stem_cache[215] is not None
    assert saved.stem_cache[215].source_version == new_version
    assert saved.stem_cache[215].cache_dir == target
    assert {path.name: path.read_bytes() for path in old_cache.iterdir()} == old_wav_bytes
    assert recovery.audio.prepare_material_migration_stems.call_args.args[1:4] == (
        old_cache.as_posix(),
        old_version,
        new_version,
    )
    assert recovery.service._journal is not None
    stem_records = [
        item
        for item in recovery.service._journal.artifacts
        if item.evidence.kind == "stem_directory"
    ]
    assert {item.evidence.reference for item in stem_records} == {old_cache.as_posix(), target}
    assert all(len(item.evidence.files) == 6 for item in stem_records)


@pytest.mark.parametrize("damage", ["unknown_child", "gap", "exhausted"])
def test_unknown_gapped_or_exhausted_journal_keeps_real_files_and_writer_fenced(
    recovery: _RecoveryRig, damage: str
) -> None:
    parent = recovery.pending()
    recovery.service.shut_down()
    directory = Path("samples/.material-migrations") / parent
    latest = recovery.records(parent)[-1]
    if damage == "unknown_child":
        (directory / "unrecognized.json").write_text("{}", encoding="utf-8")
    elif damage == "gap":
        (directory / "01.json").unlink()
    else:
        store = MaterialMigrationJournalStore.open(str(Path("samples").resolve()), parent)
        for _ in range(16 - len(recovery.records(parent))):
            store.append(latest.model_dump_json())
        newer = recovery.controller.project.model_copy(deep=True)
        for sample_id in _IDS:
            newer.sample_paths[sample_id] = None
            newer.pad_content[sample_id] = None
        writer = ProjectPersistence(newer)
        writer.config_path = recovery.persistence.config_path
        writer.flush()
    journal_bytes = {path.name: path.read_bytes() for path in directory.iterdir()}
    original_bytes = Path(_OLD).read_bytes()
    recovery.reopen()
    recovery.service.poll()
    assert recovery.service.status == "unresolved"
    assert recovery.persistence._migration_owner is not None
    assert not recovery.sources
    assert not recovery.persistence.flush_if_dirty()
    assert Path(_OLD).read_bytes() == original_bytes
    assert Path(_NEW).is_file()
    assert {path.name: path.read_bytes() for path in directory.iterdir()} == journal_bytes
    assert recovery.holds[-1].release_count == 0
    assert recovery.service._recovery_errors


def _rollback_journal(
    samples: Path, config: Path, evidence: MigrationArtifactEvidence
) -> MaterialMigrationJournal:
    snapshot = ProjectState.model_validate_json(config.read_bytes())
    journal = MaterialMigrationJournal(
        transaction_id="7" * 32,
        phase="failed",
        config_reference=str(config.resolve()),
        captured_revision=snapshot.config_revision,
        config_sha256=hashlib.sha256(config.read_bytes()).hexdigest(),
        snapshot_json=snapshot.model_dump_json(),
        assignments=(MigrationAssignment(sample_id=0, instance_id="1" * 32, old_reference=_OLD),),
        artifacts=(MigrationArtifactRecord(role="rollback", created=False, evidence=evidence),),
    )
    store = MaterialMigrationJournalStore(str(samples), journal.transaction_id)
    store.append(journal.model_dump_json())
    return journal


def test_real_inventory_preserves_unavailable_stems_referenced_by_other_saved_project(
    recovery: _RecoveryRig,
) -> None:
    samples = Path("samples").resolve()
    for worker in recovery.workers:
        worker.shut_down()
    cache = samples / "stems" / "#216"
    cache.mkdir(parents=True)
    for kind in STEM_KINDS:
        write_mono_pcm16_wav(cache / f"{kind}.wav", 44_100)
    source_version = f"{_OLD}|sha256-v1:{hashlib.sha256(Path(_OLD).read_bytes()).hexdigest()}"
    write_test_stem_marker(cache, source_version)
    reference = "samples/stems/#216"
    native = MigrationArtifactLease.capture(str(samples), reference)
    try:
        evidence = MigrationArtifactEvidence.model_validate_json(native.receipt_json())
    finally:
        native.release()
    own = ProjectState()
    own_writer = ProjectPersistence(own)
    own_writer.config_path = samples / "current.config.json"
    own_writer.flush()
    other = ProjectState()
    other.stem_cache[215] = StemCacheEntry(
        source_version=source_version,
        cache_dir=reference,
        stems=expected_stem_files(reference),
        available=False,
    )
    other_writer = ProjectPersistence(other)
    other_writer.config_path = samples / "other.config.json"
    other_writer.flush()
    journal = _rollback_journal(samples, own_writer.config_path, evidence)
    exact = {path.name: path.read_bytes() for path in cache.iterdir()}
    reconciler = MigrationArtifactReconciler(samples, own_writer.config_path)
    try:
        result = reconciler.retire(journal, own).result(timeout=10)
        assert result.protected == (reference,)
        assert not result.queued
        assert not result.errors
        assert {path.name: path.read_bytes() for path in cache.iterdir()} == exact
        assert not MaterialMigrationJournal.model_validate_json(
            (samples / ".material-migrations" / journal.transaction_id / "00.json").read_bytes()
        ).cleanup_complete
    finally:
        reconciler.shut_down()


def _assert_protected_outcome(
    queue: MigrationCleanupQueue, reconciler: MigrationArtifactReconciler, current: ProjectState
) -> None:
    queue.poll(reconciler, current, admit=False)
    assert queue.error is not None
    assert "protected" in queue.error
    assert Path(_OLD).is_file()


def _assert_visible_worker_failure(
    result: CleanupResult, journal: MaterialMigrationJournal
) -> None:
    assert result.errors == ("injected config inventory failure",)
    assert not result.queued
    assert not result.protected
    assert result.journal == journal


def test_cleanup_worker_failure_stays_visible_and_retry_checks_newer_current_reference(
    recovery: _RecoveryRig, monkeypatch: pytest.MonkeyPatch
) -> None:
    samples = Path("samples").resolve()
    for worker in recovery.workers:
        worker.shut_down()
    config = samples / "cleanup.config.json"
    current = ProjectState()
    writer = ProjectPersistence(current)
    writer.config_path = config
    writer.flush()
    native = MigrationArtifactLease.capture(str(samples), _OLD)
    try:
        evidence = MigrationArtifactEvidence.model_validate_json(native.receipt_json())
    finally:
        native.release()
    journal = _rollback_journal(samples, config, evidence)
    reconciler = MigrationArtifactReconciler(samples, config)
    queue = MigrationCleanupQueue(samples)
    queue.add(journal)
    actual_references = reconciler._references
    monkeypatch.setattr(
        reconciler, "_references", Mock(side_effect=OSError("injected config inventory failure"))
    )
    try:
        queue.poll(reconciler, current, admit=True)
        assert queue._future is not None
        failure = queue._future.result(timeout=10)
        _assert_visible_worker_failure(failure, journal)
        queue.poll(reconciler, current, admit=False)
        assert queue.error is not None
        assert "inventory failure" in queue.error
        assert Path(_OLD).is_file()
        assert not journal.cleanup_complete
        monkeypatch.setattr(reconciler, "_references", actual_references)
        current.sample_paths[215] = _OLD
        current.pad_content[215] = PadContentIdentity(instance_id="f" * 32)
        writer.mark_dirty()
        writer.flush()
        queue.retry()
        queue.poll(reconciler, current, admit=True)
        assert queue._future is not None
        result = queue._future.result(timeout=10)
        assert not result.errors, result.errors
        assert result.protected == (_OLD,)
        assert not result.queued
        _assert_protected_outcome(queue, reconciler, current)
        scanned = inspect_migration_recovery(samples, config)
        assert not scanned.errors
        assert scanned.settled
        assert scanned.settled[0].cleanup_complete is False
    finally:
        reconciler.shut_down()


def test_inventory_runs_in_bounded_worker_and_reads_newer_saved_reference_before_queueing(
    recovery: _RecoveryRig, monkeypatch: pytest.MonkeyPatch
) -> None:
    for worker in recovery.workers:
        worker.shut_down()
    for sample_id in _IDS:
        recovery.controller.project.sample_paths[sample_id] = None
        recovery.controller.project.pad_content[sample_id] = None
    recovery.persistence.mark_dirty()
    recovery.persistence.flush()
    samples = Path("samples").resolve()
    current = ProjectState()
    writer = ProjectPersistence(current)
    writer.config_path = samples / "inventory.config.json"
    writer.flush()
    native = MigrationArtifactLease.capture(str(samples), _OLD)
    try:
        evidence = MigrationArtifactEvidence.model_validate_json(native.receipt_json())
    finally:
        native.release()
    journal = _rollback_journal(samples, writer.config_path, evidence)
    reconciler = MigrationArtifactReconciler(samples, writer.config_path)
    started, resume = Event(), Event()
    caller_thread = get_ident()
    actual_references = reconciler._references

    def references(
        image: ProjectState, records: list[str]
    ) -> tuple[set[Path], set[str], list[str]]:
        assert get_ident() != caller_thread
        started.set()
        assert resume.wait(10), "bounded inventory test was not released"
        return actual_references(image, records)

    monkeypatch.setattr(reconciler, "_references", references)
    try:
        future = reconciler.retire(journal, current)
        assert started.wait(10)
        assert not future.done()
        with pytest.raises((OSError, RuntimeError, ValueError)):
            MigrationProjectGuard(str(samples), str((samples / "new.config.json").resolve()))
        current.sample_paths[215] = _OLD
        current.pad_content[215] = PadContentIdentity(instance_id="f" * 32)
        writer.mark_dirty()
        writer.flush()
        resume.set()
        result = future.result(timeout=10)
        assert not result.errors, result.errors
        assert result.protected == (_OLD,)
        assert not result.queued
        assert not result.errors
        assert Path(_OLD).is_file()
    finally:
        resume.set()
        reconciler.shut_down()
