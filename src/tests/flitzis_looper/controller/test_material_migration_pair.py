"""Real project/journal persistence with explicit pair/source controller doubles.

These tests certify migration policy and the ordinary runner boundary. Their
manually settled tickets do not certify native ACKs, PCM integrity or hearing.
"""

import hashlib
from concurrent.futures import Future
from pathlib import Path
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import material_migration as migration_module
from flitzis_looper.controller.material_migration import MaterialMigrationController
from flitzis_looper.controller.material_migration_cleanup import (
    ArtifactCapture,
    ArtifactRequest,
    MigrationArtifactReconciler,
)
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.key_intent import MAX_KEY_EPOCH
from flitzis_looper.material_migration_model import MigrationAssignment
from flitzis_looper.models import STEM_KINDS, PadContentIdentity, StemCacheEntry
from flitzis_looper.stem_pair_selection import StemPairSelection
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import FakePreparedSourceTicket, write_test_stem_marker
from tests.flitzis_looper.controller.test_material_migration import (
    _IDS,
    _MATERIAL,
    _NEW,
    _OLD,
    _DeliveredLease,
    _Harness,
    _StemWork,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller import AppController
    from flitzis_looper.models import ProjectState


class _ManualRunner:
    """Hold actual Future-producing callbacks until the test models worker return."""

    def __init__(self) -> None:
        self.tasks: list[Callable[[], None]] = []

    def __call__(self, target: Callable[[], None]) -> None:
        self.tasks.append(target)

    def return_next(self) -> None:
        self.tasks.pop(0)()


class _PreparedPair:
    """Opaque native-result double; it grants no actual source or deletion rights."""

    def __init__(self, selection: StemPairSelection, *, components: bool) -> None:
        self.selection = selection
        self.components = components
        self.selected = 0
        self.discarded = 0

    def selection_json(self) -> str:
        return self.selection.model_dump_json()

    def has_components(self) -> bool:
        return self.components

    def select(self) -> None:
        self.selected += 1

    def discard(self) -> None:
        self.discarded += 1


class _PairRig:
    def __init__(
        self, controller: AppController, audio: Mock, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setattr(migration_module, "ProjectAssetLease", _DeliveredLease)
        self.base = _Harness(controller, audio)
        self.controller = controller
        self.audio = audio
        self.runner = _ManualRunner()
        self.service = MaterialMigrationController(
            controller.persistence,
            controller.session,
            audio,
            controller._assets,
            controller.loader,
            stem_task_runner=self.runner,
        )
        controller.material_migration = self.service
        self.base.migration = self.service
        root = f"samples/materials/M{_MATERIAL}"
        self.selection = StemPairSelection(
            descriptor_reference=f"{root}/.pcm-cache/stems/v1/.pairs/{'b' * 32}.json",
            stem_set_identity="c" * 64,
            wav_generation=f"{root}/stems/.ready-{'e' * 32}",
            pcm_generation=f"{root}/.pcm-cache/stems/v1/.ready-{'f' * 32}",
        )
        digest = hashlib.sha256(Path(_OLD).read_bytes()).hexdigest()
        self.old_version = f"{_OLD}|sha256-v1:{digest}"
        self.canonical_version = f"{_NEW}|sha256-v1:{digest}"
        cache = Path(self.selection.wav_generation)
        cache.mkdir(parents=True)
        for kind in STEM_KINDS:
            write_mono_pcm16_wav(cache / f"{kind}.wav", 44_100)
        # Actual WAV marker lineage differs from the current legacy subscriber.
        # Native full pair/PCM recognition is deliberately outside this fake.
        write_test_stem_marker(cache, self.canonical_version)
        entry = StemCacheEntry(
            source_version=self.old_version,
            cache_dir=self.selection.wav_generation,
            stems=expected_stem_files(self.selection.wav_generation),
            available=True,
            pair=self.selection,
        )
        for sample_id in _IDS:
            controller.project.stem_cache[sample_id] = entry.model_copy(deep=True)
            controller.project.pad_stem_mix_mode[sample_id] = "all_stems"
        audio.prepare_material_migration_stems.return_value = _StemWork(state="ready")
        self.tickets: list[tuple[int, FakePreparedSourceTicket]] = []
        self.prepared: list[_PreparedPair] = []
        self.order: list[tuple[str, int]] = []

        def capture(sample_id: int, version: str) -> FakePreparedSourceTicket:
            assert version == self.canonical_version
            ticket = FakePreparedSourceTicket("captured")
            self.tickets.append((sample_id, ticket))
            return ticket

        def prepare(
            sample_id: int,
            version: str,
            cache_dir: str,
            ticket: FakePreparedSourceTicket,
            components: object,
            descriptor_reference: str | None,
        ) -> _PreparedPair:
            assert version == self.canonical_version
            assert cache_dir == self.selection.wav_generation
            assert descriptor_reference == self.selection.descriptor_reference
            assert any(owner == sample_id and current is ticket for owner, current in self.tickets)
            assert isinstance(components, bool)
            result = _PreparedPair(self.selection, components=components)
            self.prepared.append(result)
            return result

        def publish(prepared: _PreparedPair, ticket: FakePreparedSourceTicket) -> None:
            assert prepared in self.prepared
            assert ticket.status == "captured"
            sample_id = next(owner for owner, current in self.tickets if current is ticket)
            self.order.append(("publish", sample_id))
            ticket.status = "pending"

        audio.capture_prepared_source.side_effect = capture
        audio.prepare_stem_pair = Mock(side_effect=prepare)
        audio.publish_stem_pair = Mock(side_effect=publish)
        audio.set_stem_pair_full_mix = Mock(
            side_effect=lambda sample_id: self.order.append(("release", sample_id))
        )
        controller._assets.sync_assignments()
        controller.persistence.mark_dirty()
        controller.persistence.flush()
        self.base.initial_config = controller.persistence.config_path.read_bytes()

    def admit_sources(self) -> None:
        self.base.begin()
        self.service.poll()
        assert self.service.status == "adoption_pending"
        assert set(self.base.sources) == set(_IDS)

    def source_ack(self, *sample_ids: int) -> None:
        self.base.acknowledge(*sample_ids)
        self.service.poll()

    def pair_ack(self, sample_id: int) -> None:
        ticket = next(
            ticket
            for owner, ticket in reversed(self.tickets)
            if owner == sample_id and ticket.publication_status() == "pending"
        )
        assert ticket.status == "pending"
        ticket.status = "accepted"


@pytest.fixture
def pair_migration(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> _PairRig:
    return _PairRig(controller, audio_engine_mock, monkeypatch)


def _assert_saved_pair_preservation(
    rig: _PairRig, expected: ProjectState, before_content: list[str | None]
) -> None:
    project = rig.controller.project
    saved = ProjectPersistence.from_config_path(rig.controller.persistence.config_path)
    assert saved.load_error is None
    assert saved.project.pad_key_intent == expected.pad_key_intent == project.pad_key_intent
    assert saved.project.pad_key_intent[0].analysis_epoch == MAX_KEY_EPOCH
    assert saved.project.pad_gain_db == expected.pad_gain_db
    assert saved.project.sample_analysis == expected.sample_analysis
    assert saved.project.sample_durations == expected.sample_durations
    for sample_id in _IDS:
        content = saved.project.pad_content[sample_id]
        assert content is not None
        assert content.instance_id == before_content[sample_id]
        assert content.material_id == _MATERIAL
        entry = saved.project.stem_cache[sample_id]
        assert entry is not None
        assert entry.pair == rig.selection
        assert entry.source_version == rig.canonical_version
        assert saved.project.sample_paths[sample_id] == _NEW


def test_pair_migration_waits_for_worker_return_and_each_own_ack_then_saves_current_intent(
    pair_migration: _PairRig,
) -> None:
    rig = pair_migration
    project = rig.controller.project
    before_content = [value.instance_id if value else None for value in project.pad_content]
    rig.admit_sources()
    # Current edits made after capture belong to the committed image.
    project.pad_key_intent[0] = project.pad_key_intent[0].changed(extra_shift=2, retrigger=False)
    project.pad_key_intent[215] = project.pad_key_intent[215].corrected("F#m")
    project.pad_gain_db[215] = -7.0
    rig.controller.persistence.mark_dirty()
    expected = project.model_copy(deep=True)
    rig.source_ack()
    assert len(rig.runner.tasks) == 2
    assert rig.audio.prepare_stem_pair.call_count == 0
    rig.service.poll()
    rig.audio.publish_stem_pair.assert_not_called()
    rig.base.assert_retained()
    rig.runner.return_next()
    rig.service.poll()
    assert rig.audio.publish_stem_pair.call_count == 1
    assert rig.tickets[0][1].publication_status() == "pending"
    assert len(rig.runner.tasks) == 1
    rig.pair_ack(0)
    rig.service.poll()
    assert rig.service.status == "adoption_pending"
    rig.base.assert_retained()
    rig.runner.return_next()
    rig.service.poll()
    assert rig.tickets[1][1] is not rig.tickets[0][1]
    assert rig.tickets[1][1].publication_status() == "pending"
    assert all(pair.selected == 0 for pair in rig.prepared[1:])
    rig.pair_ack(215)
    rig.service.poll()
    assert rig.service.status == "config_committed"
    _assert_saved_pair_preservation(rig, expected, before_content)
    rig.audio.prepare_material_migration_stems.assert_called_once()
    copy = rig.audio.prepare_material_migration_stems.call_args.args
    assert copy[1:4] == (
        rig.selection.wav_generation,
        rig.canonical_version,
        rig.canonical_version,
    )
    assert rig.base.hold.release_count == rig.base.preparation.release_count == 1
    assert rig.base.preparation.abort_count == 0
    assert rig.controller._assets._reserved == 0


def test_full_mix_migration_keeps_frozen_disk_pair_without_preparing_four_live_components(
    pair_migration: _PairRig,
) -> None:
    rig = pair_migration
    for sample_id in _IDS:
        rig.controller.project.pad_stem_mix_mode[sample_id] = "full_mix"
    rig.admit_sources()
    rig.source_ack()
    assert rig.service.status == "config_committed"
    assert not rig.runner.tasks
    rig.audio.prepare_stem_pair.assert_not_called()
    rig.audio.publish_stem_pair.assert_not_called()
    rig.audio.set_stem_pair_full_mix.assert_not_called()
    saved = ProjectPersistence.from_config_path(rig.controller.persistence.config_path).project
    assert saved.stem_cache[0] is not None
    assert saved.stem_cache[0].pair == rig.selection
    assert saved.pad_stem_mix_mode[0] == "full_mix"
    # The real ordinary controller queue must do fresh work when ALL is requested.
    assert rig.controller.stems.set_stem_mix_mode(0, "all_stems")
    rig.controller.stems.on_frame_render()
    assert rig.audio.prepare_stem_pair.call_count == 1
    assert rig.audio.publish_stem_pair.call_count == 1
    assert not rig.controller.stems.stems_available(0)
    rig.pair_ack(0)
    rig.controller.stems.on_frame_render()
    assert rig.controller.stems.stems_available(0)
    assert rig.controller.project.stem_cache[0] is not None
    assert rig.controller.project.stem_cache[0].pair == rig.selection


def test_full_mix_after_pending_pair_orders_release_and_later_all_requires_new_worker_ticket(
    pair_migration: _PairRig,
) -> None:
    rig = pair_migration
    rig.admit_sources()
    rig.source_ack(0)
    assert len(rig.runner.tasks) == 1
    rig.runner.return_next()
    rig.service.poll()
    first = rig.tickets[0][1]
    assert first.status == "pending"
    rig.controller.project.pad_stem_mix_mode[0] = "full_mix"
    rig.controller.persistence.mark_dirty()
    rig.service.poll()
    assert rig.order == [("publish", 0), ("release", 0)]
    assert rig.service.status == "adoption_pending"  # #216 has no Source ACK yet.
    rig.controller.project.pad_stem_mix_mode[0] = "all_stems"
    rig.controller.persistence.mark_dirty()
    rig.service.poll()
    assert len(rig.runner.tasks) == 1
    assert len(rig.tickets) == 2
    assert rig.tickets[1][1] is not first
    assert rig.audio.prepare_stem_pair.call_count == 1
    rig.runner.return_next()
    rig.service.poll()
    assert rig.order == [("publish", 0), ("release", 0), ("publish", 0)]
    rig.pair_ack(0)
    rig.source_ack(215)
    rig.runner.return_next()
    rig.service.poll()
    rig.pair_ack(215)
    rig.service.poll()
    assert rig.service.status == "config_committed"
    assert rig.controller.project.stem_cache[0] is not None
    assert rig.controller.project.stem_cache[0].pair == rig.selection


def test_full_mix_demand_cannot_settle_an_unreturned_pair_worker_via_future_gc(
    pair_migration: _PairRig,
) -> None:
    rig = pair_migration
    rig.admit_sources()
    rig.source_ack(0)
    subscriber = rig.service._subscribers[0]
    future = subscriber.pair_future
    assert future is not None
    assert not future.done()
    assert len(rig.runner.tasks) == 1
    for sample_id in _IDS:
        rig.controller.project.pad_stem_mix_mode[sample_id] = "full_mix"
    rig.controller.persistence.mark_dirty()
    rig.source_ack(215)
    assert rig.service.status == "adoption_pending"
    assert subscriber.pair_future is future
    assert not future.done()
    assert rig.controller.project.sample_paths[0] == _OLD
    rig.audio.publish_stem_pair.assert_not_called()
    rig.base.assert_retained()
    rig.runner.return_next()
    assert future.done()
    rig.service.poll()
    assert rig.service.status == "config_committed"
    rig.audio.publish_stem_pair.assert_not_called()
    assert rig.prepared[0].selected == 1
    saved = ProjectPersistence.from_config_path(rig.controller.persistence.config_path).project
    assert saved.sample_paths[0] == saved.sample_paths[215] == _NEW
    assert saved.stem_cache[0] is not None
    assert saved.stem_cache[0].pair == rig.selection
    assert saved.pad_stem_mix_mode[0] == "full_mix"
    assert rig.base.hold.release_count == 1


def test_existing_pair_artifact_requests_are_target_noncreations_for_all_three_areas(
    pair_migration: _PairRig,
) -> None:
    rig = pair_migration
    worker = Mock(spec=MigrationArtifactReconciler)
    returned: Future[ArtifactCapture] = Future()
    returned.set_result(ArtifactCapture((), []))
    worker.capture.return_value = returned
    rig.service._artifact_reconciler = cast("MigrationArtifactReconciler", worker)
    assignments = [
        MigrationAssignment(
            sample_id=sample_id, instance_id=f"{sample_id + 1:032x}", old_reference=_OLD
        )
        for sample_id in _IDS
    ]
    rig.service._start_old_artifact_capture(_OLD, assignments)
    requests = cast("list[ArtifactRequest]", worker.capture.call_args.args[0])
    assert requests == [
        ArtifactRequest(_OLD, "rollback"),
        ArtifactRequest(rig.selection.wav_generation, "target", created=False),
        ArtifactRequest(rig.selection.pcm_generation, "target", created=False),
        ArtifactRequest(rig.selection.descriptor_reference, "target", created=False),
    ]
    assert rig.service._artifact_future is returned
    assert not returned.result().records  # This mock manufactures no native proof.


def test_pair_worker_error_preserves_current_tuple_and_both_owner_fences(
    pair_migration: _PairRig,
) -> None:
    rig = pair_migration
    rig.admit_sources()
    rig.source_ack()
    before = rig.controller.project.model_copy(deep=True)
    rig.audio.prepare_stem_pair.side_effect = RuntimeError("verified pair worker failed")
    rig.runner.return_next()
    rig.service.poll()
    assert rig.service.status == "unresolved"
    assert "verified pair worker failed" in (rig.service.error or "")
    assert rig.controller.project == before
    assert rig.controller.persistence.config_path.read_bytes() == rig.base.initial_config
    rig.audio.publish_stem_pair.assert_not_called()
    rig.base.assert_retained()


def test_newer_stem_reference_during_pair_work_wins_and_migration_cannot_overwrite_it(
    pair_migration: _PairRig,
) -> None:
    rig = pair_migration
    rig.admit_sources()
    rig.source_ack()
    replacement = StemCacheEntry(
        source_version="newer performer stem version",
        cache_dir="samples/stems/#1",
        stems=expected_stem_files("samples/stems/#1"),
        available=False,
    )
    rig.controller.project.stem_cache[0] = replacement
    rig.controller.project.pad_key_intent[0] = rig.controller.project.pad_key_intent[0].changed(
        extra_shift=-2
    )
    rig.controller.persistence.mark_dirty()
    while rig.runner.tasks:
        rig.runner.return_next()
    rig.service.poll()
    assert rig.service.status == "unresolved"
    assert "newer stem selection" in (rig.service.error or "")
    rig.audio.publish_stem_pair.assert_not_called()
    assert all(ticket.status == "captured" for _, ticket in rig.tickets)
    assert rig.controller.project.stem_cache[0] is replacement
    assert rig.controller.project.pad_key_intent[0].extra_shift == -2
    assert rig.controller.persistence.config_path.read_bytes() == rig.base.initial_config
    rig.base.assert_retained()


def test_new_content_replacing_pair_subscriber_is_preserved_while_other_pad_commits(
    pair_migration: _PairRig,
) -> None:
    rig = pair_migration
    rig.admit_sources()
    # Only #1 has begun a pair worker. #216 is replaced before its Source ACK.
    rig.source_ack(0)
    replacement_path = Path("samples/replacement.wav")
    write_mono_pcm16_wav(replacement_path, 48_000)
    replacement_content = PadContentIdentity(instance_id="9" * 32)
    project = rig.controller.project
    project.sample_paths[215] = replacement_path.as_posix()
    project.pad_content[215] = replacement_content
    project.stem_cache[215] = None
    rig.controller.persistence.mark_dirty()
    rig.runner.return_next()
    rig.service.poll()
    rig.pair_ack(0)
    rig.service.poll()
    assert rig.service.status == "config_committed"
    assert rig.base.sources[215].phase() == "rejected"
    assert len(rig.tickets) == 1
    saved = ProjectPersistence.from_config_path(rig.controller.persistence.config_path).project
    assert saved.sample_paths[0] == _NEW
    assert saved.sample_paths[215] == replacement_path.as_posix()
    assert saved.pad_content[215] == replacement_content
    assert saved.stem_cache[215] is None
    assert saved.stem_cache[0] is not None
    assert saved.stem_cache[0].pair == rig.selection
