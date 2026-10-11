"""M-ID pair transition with real Native WAV receipts and explicit ACK/PCM doubles.

The controller must request fresh source-bound descriptors. Native complete PCM
descriptor/callback behavior is certified by the separate Native pair tests.
"""

import hashlib
import json
from pathlib import Path
from threading import Event
from time import monotonic
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.material_migration_cleanup import (
    ArtifactRequest,
    MigrationArtifactReconciler,
)
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.material_migration_model import (
    MaterialMigrationJournal,
    MigrationArtifactRecord,
    MigrationAssignment,
)
from flitzis_looper.models import STEM_KINDS, PadContentIdentity, ProjectState
from flitzis_looper.stem_pair_selection import StemPairSelection
from flitzis_looper_audio import MaterialMigrationJournalStore, MigrationArtifactLease
from tests.flitzis_looper.conftest import FakePreparedSourceTicket, write_test_stem_marker
from tests.flitzis_looper.controller.test_material_migration import (
    _IDS,
    _MATERIAL,
    _NEW,
    _OLD,
    _DeliveredLease,
    _StemWork,
)
from tests.flitzis_looper.controller.test_material_migration_pair import _PairRig, _PreparedPair
from tests.flitzis_looper.controller.test_material_migration_reconciliation import (
    _ControlProofWorker,
)
from tests.flitzis_looper.controller.test_readable_material_migration import (
    _readable,
    _ReadablePreparation,
)

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.models import StemMixMode


class _MIdPreparation(_ReadablePreparation):
    def metadata_json(self) -> str:
        metadata = json.loads(super().metadata_json())
        metadata["old_reference"] = _NEW
        metadata |= {"artifact_ledger_schema": 1, "created_original": False, "created_cache": False}
        return json.dumps(metadata)


class _MovedWork(_StemWork):
    def __init__(self, target: str) -> None:
        super().__init__(state="ready", created=True)
        self.target = target

    def cache_reference(self) -> str:
        return self.target


class _ReadableMIdRig(_PairRig):
    def __init__(
        self, controller: AppController, audio: Mock, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        super().__init__(controller, audio, monkeypatch)
        self.old_selection = self.selection
        self.old_version = self.canonical_version
        target, material = _readable(Path(_OLD))
        root = Path(target).parent.as_posix()
        digest = hashlib.sha256(Path(_NEW).read_bytes()).hexdigest()
        self.canonical_version = f"{target}|sha256-v1:{digest}"
        self.selection = StemPairSelection(
            descriptor_reference=f"{root}/.pcm-cache/stems/v1/.pairs/{'2' * 32}.json",
            stem_set_identity="3" * 64,
            wav_generation=f"{root}/stems/.ready-{'4' * 32}",
            pcm_generation=f"{root}/.pcm-cache/stems/v1/.ready-{'5' * 32}",
        )
        self.base.preparation = _MIdPreparation(target, material)
        audio.prepare_material_migration.return_value = self.base.preparation
        self.target = target
        for sample_id in _IDS:
            content = controller.project.pad_content[sample_id]
            assert content is not None
            controller.project.sample_paths[sample_id] = _NEW
            controller.project.pad_content[sample_id] = PadContentIdentity(
                instance_id=content.instance_id, material_id=_MATERIAL
            )

        def copy(
            _preparation: object, cache: str, old_version: str, new_version: str, _id: str
        ) -> _MovedWork:
            assert cache == self.old_selection.wav_generation
            assert old_version == self.old_version
            assert new_version == self.canonical_version
            proof = MigrationArtifactLease.capture(str(Path("samples").resolve()), cache)
            try:
                marker = json.loads((Path(cache) / ".complete.json").read_bytes())
                assert marker["source_version"] == old_version
                moved = Path(self.selection.wav_generation)
                moved.mkdir(parents=True)
                for kind in STEM_KINDS:
                    (moved / f"{kind}.wav").write_bytes((Path(cache) / f"{kind}.wav").read_bytes())
                write_test_stem_marker(moved, new_version)
            finally:
                proof.release()
            return _MovedWork(self.selection.wav_generation)

        def prepare(
            sample_id: int,
            version: str,
            cache: str,
            ticket: FakePreparedSourceTicket,
            *,
            components: bool,
            descriptor_reference: str | None,
        ) -> _PreparedPair:
            assert sample_id in _IDS
            assert version == self.canonical_version
            assert cache == self.selection.wav_generation
            assert descriptor_reference is None
            assert any(
                owner == sample_id and captured is ticket for owner, captured in self.tickets
            )
            result = _PreparedPair(self.selection, components=components)
            self.prepared.append(result)
            return result

        audio.prepare_material_migration_stems.side_effect = copy
        audio.prepare_stem_pair.side_effect = prepare
        controller._assets.sync_assignments()
        controller.persistence.mark_dirty()
        controller.persistence.flush()
        self.base.initial_config = controller.persistence.config_path.read_bytes()

    def admit_sources(self) -> None:
        self.base.begin()
        original_reconciler = self.service._artifact_reconciler
        assert original_reconciler is not None
        original_reconciler.shut_down()
        # Native Original/WAV receipts are productive; PCM/ACK execution remains
        # an explicit control substitute, as in the established ledger fixture.
        self.service._artifact_reconciler = _ControlProofWorker(
            Path("samples").resolve(), self.controller.persistence.config_path
        )
        self.service._ledger_enabled = True
        self.service._start_old_artifact_capture(
            _NEW, [subscriber.capture for subscriber in self.service._subscribers.values()]
        )
        for _ in range(5):
            self.service.poll()
            if self.service.status == "adoption_pending":
                break
        assert self.service.status == "adoption_pending"
        assert set(self.base.sources) == set(_IDS)
        journal = self.service._journal
        assert journal is not None
        records = journal.artifacts
        assert any(
            record.role == "rollback"
            and record.evidence.reference == self.old_selection.wav_generation
            for record in records
        )

    def source_ack(self, *sample_ids: int) -> None:
        """Deliver each fresh subscriber's actual readable target lease."""
        events: list[dict[str, object]] = []
        for sample_id in sample_ids or tuple(self.base.sources):
            source = self.base.sources[sample_id]
            source.state = "acknowledged"
            owner = _DeliveredLease(str(Path(self.target).absolute()))
            self.base.delivered[sample_id] = owner
            events.append({
                "type": "success",
                "id": sample_id,
                "request_id": source.request_id,
                "path": self.target,
                "original_lease": owner,
                "duration": 999.0,
                "analysis": {"bpm": 80.0, "key": "Bm", "beat_grid": {}},
            })
        self.audio.poll_loader_events.side_effect = [*events, None]
        self.controller.loader.poll_loader_events()
        self.service.poll()


@pytest.mark.parametrize("mode", ["full_mix", "all_stems"])
@pytest.mark.parametrize("available", [True, False])
def test_m_id_pair_moves_to_readable_with_own_new_descriptor_work_and_current_intent(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    mode: StemMixMode,
    *,
    available: bool,
) -> None:
    rig = _ReadableMIdRig(controller, audio_engine_mock, monkeypatch)
    project = controller.project
    for slot in _IDS:
        project.pad_stem_mix_mode[slot] = mode
        entry = project.stem_cache[slot]
        assert entry is not None
        project.stem_cache[slot] = entry.model_copy(update={"available": available})
    before = project.model_copy(deep=True)
    rig.admit_sources()
    rig.source_ack()
    assert len(rig.runner.tasks) == 2
    assert project.stem_cache == before.stem_cache
    assert Path(rig.old_selection.wav_generation).is_dir()
    project.pad_gain_db[215] = -8.0
    project.pad_key_intent[0] = project.pad_key_intent[0].changed(extra_shift=-2)
    controller.persistence.mark_dirty()
    rig.runner.return_next()
    rig.service.poll()
    assert rig.service.status == "adoption_pending"
    assert project.sample_paths[0] == _NEW
    assert controller.persistence.config_path.read_bytes() == rig.base.initial_config
    if mode == "all_stems":
        assert rig.tickets[0][1].status == "pending"
        rig.pair_ack(0)
        rig.service.poll()
    rig.runner.return_next()
    rig.service.poll()
    if mode == "all_stems":
        assert rig.service.status == "adoption_pending"
        assert rig.tickets[1][1] is not rig.tickets[0][1]
        rig.pair_ack(215)
        rig.service.poll()
    else:
        audio_engine_mock.publish_stem_pair.assert_not_called()
        assert all(not pair.has_components() for pair in rig.prepared)
    assert rig.service.status == "config_committed"
    saved = ProjectPersistence.from_config_path(controller.persistence.config_path).project
    _assert_saved_transition(rig, saved, before)
    assert Path(rig.old_selection.wav_generation).is_dir()


def _assert_saved_transition(
    rig: _ReadableMIdRig, saved: ProjectState, before: ProjectState
) -> None:
    for slot in _IDS:
        assert saved.sample_paths[slot] == rig.target
        saved_content = saved.pad_content[slot]
        old_content = before.pad_content[slot]
        assert saved_content is not None
        assert old_content is not None
        assert saved_content.instance_id == old_content.instance_id
        entry = saved.stem_cache[slot]
        assert entry is not None
        assert entry.source_version == rig.canonical_version
        assert entry.pair == rig.selection
        assert entry.pair != rig.old_selection
        assert entry.available
        assert entry.stems == expected_stem_files(rig.selection.wav_generation)
    assert saved.pad_gain_db[215] == -8.0
    assert saved.pad_key_intent[0].extra_shift == -2


def test_old_m_id_pair_receipt_is_rollback_and_retires_only_after_last_held_reader(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rig = _ReadableMIdRig(controller, audio_engine_mock, monkeypatch)
    samples = Path("samples").resolve()
    cache = rig.old_selection.wav_generation
    worker = MigrationArtifactReconciler(samples, controller.persistence.config_path)
    reader = MigrationArtifactLease.capture(str(samples), cache)
    try:
        capture = worker.capture([ArtifactRequest(cache, "rollback")]).result(timeout=10)
        assert len(capture.records) == 1
        record = capture.records[0]
        assert record.role == "rollback"
        assert record.evidence.kind == "stem_directory"
        capture.release()
        current = ProjectState()
        writer = ProjectPersistence(current)
        writer.config_path = samples / "empty.config.json"
        writer.flush()
        # Remove the real current config's old selection before reconciling it.
        controller.project.sample_paths = current.sample_paths.copy()
        controller.project.pad_content = current.pad_content.copy()
        controller.project.stem_cache = current.stem_cache.copy()
        controller.persistence.mark_dirty()
        controller.persistence.flush()
        controller._assets.release_saved_assignments()
        journal = MaterialMigrationJournal(
            transaction_id="6" * 32,
            phase="failed",
            captured_revision=0,
            config_reference=writer.config_reference,
            snapshot_json=current.model_dump_json(),
            assignments=(
                MigrationAssignment(sample_id=0, instance_id="7" * 32, old_reference=_NEW),
            ),
            artifacts=(
                MigrationArtifactRecord(role="rollback", created=False, evidence=record.evidence),
            ),
        )
        store = MaterialMigrationJournalStore(str(samples), journal.transaction_id)
        store.append(journal.model_dump_json())
        worker.shut_down()
        worker = MigrationArtifactReconciler(samples, writer.config_path)
        result = worker.retire(journal, current).result(timeout=10)
        assert not result.errors
        assert result.queued == (cache,)
        assert Path(cache).is_dir()
        reader.release()
        # The actual Native retirement lease owns terminal observation.
        deadline = monotonic() + 10
        while result.leases[0].retirement_status() == "pending" and monotonic() < deadline:
            Event().wait(0.01)
        assert result.leases[0].retirement_status() == "complete"
        assert worker.settled(result)
        assert not Path(cache).exists()
        assert Path(rig.target).is_file()
    finally:
        reader.release()
        worker.shut_down()
