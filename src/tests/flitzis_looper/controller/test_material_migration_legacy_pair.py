"""Legacy WAV migration policy with explicit controller doubles, not native ACK proof.

The actual Coordinator, Future boundary and JSON persistence run here. Native
PCM integrity, bounded pool admission and callback ACKs have separate Rust oracles.
"""

from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.models import StemCacheEntry
from tests.flitzis_looper.controller.test_material_migration import (
    _IDS,
    _NEW,
    _OLD,
    _old_stem_set,
)
from tests.flitzis_looper.controller.test_material_migration_pair import (
    _assert_saved_pair_preservation,
    _PairRig,
    _PreparedPair,
)

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from tests.flitzis_looper.conftest import FakePreparedSourceTicket


@pytest.fixture
def legacy_pair_migration(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> _PairRig:
    rig = _PairRig(controller, audio_engine_mock, monkeypatch)
    old = _old_stem_set()
    assert old.pair is None
    assert old.source_version == rig.old_version
    for sample_id in _IDS:
        controller.project.stem_cache[sample_id] = old.model_copy(deep=True)

    def prepare(
        sample_id: int,
        version: str,
        cache_dir: str,
        ticket: FakePreparedSourceTicket,
        components: object,
        descriptor_reference: str | None,
    ) -> _PreparedPair:
        assert sample_id in _IDS
        assert version == rig.canonical_version
        assert cache_dir == rig.selection.wav_generation
        assert descriptor_reference is None
        assert any(owner == sample_id and captured is ticket for owner, captured in rig.tickets)
        assert isinstance(components, bool)
        prepared = _PreparedPair(rig.selection, components=components)
        rig.prepared.append(prepared)
        return prepared

    audio_engine_mock.prepare_stem_pair.side_effect = prepare
    controller._assets.sync_assignments()
    controller.persistence.mark_dirty()
    controller.persistence.flush()
    rig.base.initial_config = controller.persistence.config_path.read_bytes()
    return rig


def test_legacy_all_waits_for_both_workers_and_independent_acks_before_strict_pair_json(
    legacy_pair_migration: _PairRig,
) -> None:
    rig = legacy_pair_migration
    project = rig.controller.project
    old_entries = tuple(project.stem_cache[sample_id] for sample_id in _IDS)
    content_ids = [value.instance_id if value else None for value in project.pad_content]
    rig.admit_sources()
    project.pad_key_intent[0] = project.pad_key_intent[0].changed(extra_shift=2)
    project.pad_gain_db[215] = -7.0
    rig.controller.persistence.mark_dirty()
    expected = project.model_copy(deep=True)
    rig.source_ack()
    assert len(rig.runner.tasks) == 2
    rig.audio.prepare_stem_pair.assert_not_called()
    rig.audio.publish_stem_pair.assert_not_called()
    assert tuple(project.stem_cache[sample_id] for sample_id in _IDS) == old_entries
    rig.base.assert_retained()

    rig.runner.return_next()
    rig.service.poll()
    assert rig.prepared[0].has_components()
    assert rig.tickets[0][0] == 0
    assert rig.tickets[0][1].publication_status() == "pending"
    rig.pair_ack(0)
    rig.service.poll()
    assert rig.service.status == "adoption_pending"
    assert project.sample_paths[0] == project.sample_paths[215] == _OLD
    assert tuple(project.stem_cache[sample_id] for sample_id in _IDS) == old_entries
    assert rig.controller.persistence.config_path.read_bytes() == rig.base.initial_config

    rig.runner.return_next()
    rig.service.poll()
    assert rig.prepared[1].has_components()
    assert rig.tickets[1][0] == 215
    assert rig.tickets[1][1] is not rig.tickets[0][1]
    assert rig.tickets[1][1].publication_status() == "pending"
    assert rig.service.status == "adoption_pending"
    rig.pair_ack(215)
    rig.service.poll()
    assert rig.service.status == "config_committed"
    _assert_saved_pair_preservation(rig, expected, content_ids)
    copied = rig.audio.prepare_material_migration_stems.call_args.args
    assert old_entries[0] is not None
    assert copied[1:4] == (
        old_entries[0].cache_dir,
        rig.old_version,
        rig.canonical_version,
    )
    assert rig.base.hold.release_count == rig.base.preparation.release_count == 1
    assert rig.base.preparation.abort_count == 0


def test_initial_full_mix_waits_for_real_future_returns_and_saves_pair_without_live_publish(
    legacy_pair_migration: _PairRig,
) -> None:
    rig = legacy_pair_migration
    project = rig.controller.project
    for sample_id in _IDS:
        project.pad_stem_mix_mode[sample_id] = "full_mix"
    content_ids = [value.instance_id if value else None for value in project.pad_content]
    expected = project.model_copy(deep=True)
    old_entries = tuple(project.stem_cache[sample_id] for sample_id in _IDS)
    rig.admit_sources()
    rig.source_ack()
    assert len(rig.runner.tasks) == 2
    futures = tuple(rig.service._subscribers[sample_id].pair_future for sample_id in _IDS)
    assert all(future is not None and not future.done() for future in futures)
    rig.service.poll()
    assert rig.service.status == "adoption_pending"
    assert tuple(project.stem_cache[sample_id] for sample_id in _IDS) == old_entries
    rig.base.assert_retained()

    rig.runner.return_next()
    rig.service.poll()
    assert not rig.prepared[0].has_components()
    assert rig.prepared[0].selected == 1
    assert rig.service.status == "adoption_pending"
    assert futures[1] is not None
    assert not futures[1].done()
    assert project.sample_paths[0] == _OLD
    rig.audio.publish_stem_pair.assert_not_called()
    rig.runner.return_next()
    rig.service.poll()
    assert rig.service.status == "config_committed"
    assert all(not prepared.has_components() for prepared in rig.prepared)
    assert all(prepared.selected >= 1 for prepared in rig.prepared)
    rig.audio.publish_stem_pair.assert_not_called()
    rig.audio.set_stem_pair_full_mix.assert_not_called()
    _assert_saved_pair_preservation(rig, expected, content_ids)
    saved = ProjectPersistence.from_config_path(rig.controller.persistence.config_path).project
    assert all(saved.pad_stem_mix_mode[sample_id] == "full_mix" for sample_id in _IDS)


def test_all_demand_during_noncomponent_future_requires_fresh_work_and_its_own_ack(
    legacy_pair_migration: _PairRig,
) -> None:
    rig = legacy_pair_migration
    project = rig.controller.project
    for sample_id in _IDS:
        project.pad_stem_mix_mode[sample_id] = "full_mix"
    rig.admit_sources()
    rig.source_ack(0)
    first_future = rig.service._subscribers[0].pair_future
    first_ticket = rig.tickets[0][1]
    assert first_future is not None
    assert not first_future.done()
    project.pad_stem_mix_mode[0] = "all_stems"
    rig.controller.persistence.mark_dirty()
    rig.service.poll()
    assert rig.service._subscribers[0].pair_future is first_future
    assert len(rig.runner.tasks) == 1
    rig.audio.publish_stem_pair.assert_not_called()
    rig.runner.return_next()
    rig.service.poll()
    assert first_future.done()
    assert not rig.prepared[0].has_components()
    assert len(rig.runner.tasks) == 1
    assert len(rig.tickets) == 2
    assert rig.tickets[1][1] is not first_ticket
    assert first_ticket.publication_status() != "accepted"
    assert project.sample_paths[0] == _OLD
    rig.audio.publish_stem_pair.assert_not_called()
    rig.runner.return_next()
    rig.service.poll()
    assert rig.prepared[1].has_components()
    assert rig.audio.publish_stem_pair.call_args.args[0] is rig.prepared[1]
    assert rig.tickets[1][1].publication_status() == "pending"
    rig.pair_ack(0)
    rig.source_ack(215)
    assert len(rig.runner.tasks) == 1
    rig.runner.return_next()
    rig.service.poll()
    assert rig.service.status == "config_committed"
    saved = ProjectPersistence.from_config_path(rig.controller.persistence.config_path).project
    assert saved.sample_paths[0] == saved.sample_paths[215] == _NEW
    assert saved.pad_stem_mix_mode[0] == "all_stems"
    assert saved.pad_stem_mix_mode[215] == "full_mix"
    assert all(saved.stem_cache[sample_id] is not None for sample_id in _IDS)
    assert rig.audio.publish_stem_pair.call_count == 1


def test_legacy_pair_native_preparation_failure_keeps_current_entries_and_both_fences(
    legacy_pair_migration: _PairRig,
) -> None:
    rig = legacy_pair_migration
    rig.admit_sources()
    rig.source_ack(0)
    before = rig.controller.project.model_copy(deep=True)
    rig.audio.prepare_stem_pair.side_effect = RuntimeError("legacy pair native preparation failed")
    rig.runner.return_next()
    rig.service.poll()
    assert rig.service.status == "unresolved"
    assert "legacy pair native preparation failed" in (rig.service.error or "")
    assert rig.controller.project == before
    assert rig.controller.persistence.config_path.read_bytes() == rig.base.initial_config
    assert all(entry is None or entry.pair is None for entry in rig.controller.project.stem_cache)
    rig.audio.publish_stem_pair.assert_not_called()
    rig.base.assert_retained()


def test_newer_legacy_stem_reference_wins_before_returned_pair_can_publish(
    legacy_pair_migration: _PairRig,
) -> None:
    rig = legacy_pair_migration
    rig.admit_sources()
    rig.source_ack(0)
    replacement = StemCacheEntry(
        source_version="newer performer stem selection",
        cache_dir="samples/stems/#216",
        stems=expected_stem_files("samples/stems/#216"),
        available=False,
    )
    rig.controller.project.stem_cache[0] = replacement
    rig.controller.project.pad_key_intent[0] = rig.controller.project.pad_key_intent[0].changed(
        extra_shift=-2
    )
    rig.controller.persistence.mark_dirty()
    rig.runner.return_next()
    rig.service.poll()
    assert rig.service.status == "unresolved"
    assert "newer stem selection" in (rig.service.error or "")
    assert rig.controller.project.stem_cache[0] is replacement
    assert rig.controller.project.pad_key_intent[0].extra_shift == -2
    assert rig.controller.project.sample_paths[0] == _OLD
    assert rig.controller.persistence.config_path.read_bytes() == rig.base.initial_config
    assert all(ticket.publication_status() == "captured" for _, ticket in rig.tickets)
    rig.audio.publish_stem_pair.assert_not_called()
    rig.base.assert_retained()
    assert Path(_OLD).is_file()
    assert Path(_NEW).is_file()
