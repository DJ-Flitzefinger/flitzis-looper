"""Real App/confirmation/pool/config joins; audio and neural work remain substitutes."""

from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import Mock

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.performance_confirmation import (
    StemBatchConfirmation,
    StemDeleteConfirmation,
)
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_job import StemGenerationInterest
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.controller import (
    test_stem_admission as admission_fixtures,
)
from tests.flitzis_looper.controller import (
    test_stem_batch_integration as batch_fixtures,
)
from tests.flitzis_looper.controller import (
    test_stem_disk_commit as disk_fixtures,
)
from tests.flitzis_looper.controller import (
    test_stem_pending_admission as pending_fixtures,
)

if TYPE_CHECKING:
    import pytest

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_delete import StemDeleteSummary
    from tests.flitzis_looper.conftest import FakeStemGenerationBackend

disk_commit = disk_fixtures.disk_commit


def _populate_duplicate_slots(controller: AppController, audio: Mock, count: int) -> None:
    source = controller.project.sample_paths[0]
    assert source is not None
    for pad_id in range(NUM_SAMPLES):
        if pad_id >= count:
            controller.project.sample_paths[pad_id] = None
            controller.project.pad_content[pad_id] = None
            continue
        controller.project.sample_paths[pad_id] = source
        controller.project.sample_durations[pad_id] = 128 / 44_100
        controller.project.pad_content[pad_id] = PadContentIdentity(
            instance_id=f"{pad_id + 1:032x}"
        )
        if pad_id not in {0, 215}:
            audio.load_sample_async(pad_id, source)
    controller.stems._assets.sync_assignments()
    batch_fixtures._loaded_metadata(controller, audio, tuple(range(count)))
    controller.project.selected_pad = 215
    controller.project.selected_bank = 5
    controller.session.settings_open = False


def _confirm_generation_batch(controller: AppController) -> StemBatchConfirmation:
    warning = controller.performance_confirmation
    warning.request_all_loaded_stems()
    intent = warning.pending
    assert isinstance(intent, StemBatchConfirmation)
    warning.accept(intent)
    assert warning.pending is None
    return intent


def _delete_summary(controller: AppController) -> StemDeleteSummary:
    summary = controller.stem_delete.summary
    assert summary is not None
    assert summary.total == sum((
        summary.queued,
        summary.removed,
        summary.noop,
        summary.skipped,
        summary.failed,
    ))
    return summary


def _confirm_deletion_without_custody(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    logical = controller.stem_batch.capture_unadmitted_interests()
    assert len(logical) == NUM_SAMPLES
    forbidden = Mock(side_effect=AssertionError("warning or accept acquired physical authority"))
    with monkeypatch.context() as boundary:
        for name in ("open", "stat", "is_file", "is_dir", "iterdir", "resolve"):
            boundary.setattr(Path, name, forbidden)
        for name in ("acquire", "reserve", "sync_assignments"):
            boundary.setattr(controller.stems._assets, name, forbidden)
        controller.performance_confirmation.request_all_loaded_stem_deletion()
        warning = controller.performance_confirmation.pending
        assert isinstance(warning, StemDeleteConfirmation)
        assert len(warning.inventory.items) == NUM_SAMPLES
        for pad_id in (0, 214, 215):
            item = warning.inventory.items[pad_id]
            assert item is not None
            assert item.pad_id == pad_id
            assert item.logical_pending == frozenset({logical[pad_id]})
        assert set(controller.stems._requested_generations) == {0, 215}
        assert len(controller.stem_batch.capture_unadmitted_interests()) == NUM_SAMPLES
        controller.performance_confirmation.accept(warning)
        controller.performance_confirmation.accept(warning)
    assert not forbidden.called
    assert controller.performance_confirmation.pending is None


def test_hidden_settings_216_delete_consumes_exact_batch_before_any_later_admission(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    with admission_fixtures._held_pool() as held:
        held.fill(32)
        pending_fixtures._production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        _populate_duplicate_slots(controller, audio_engine_mock, NUM_SAMPLES)
        stems = controller.stems
        assert stems.generate_stems_async(0)
        assert stems.generate_stems_async(215)
        admitted = {pad_id: row.owner for pad_id, row in stems._requested_generations.items()}
        assert set(admitted) == {0, 215}
        _confirm_generation_batch(controller)
        before = controller.project.model_dump()
        reserved = stems._assets._reserved
        _confirm_deletion_without_custody(controller, monkeypatch)
        assert not controller.stem_batch.capture_unadmitted_interests()
        batch = controller.stem_batch.summary
        assert batch is not None
        assert batch.cancelled == batch.total == NUM_SAMPLES
        assert not batch.active
        assert controller.project.model_dump() == before
        assert stems._assets._reserved == reserved
        audio_engine_mock.capture_offline_stem_source.assert_not_called()

        for frame in range(27):
            controller.on_frame_render()
            summary = _delete_summary(controller)
            assert summary.total == NUM_SAMPLES
            assert summary.removed == (frame + 1) * 8
            assert summary.queued == NUM_SAMPLES - summary.removed
            assert (summary.noop, summary.skipped, summary.failed) == (0, 0, 0)
            assert summary.active == (frame < 26)
            assert set(stems._requested_generations) <= {215}
            assert not stems._jobs
            audio_engine_mock.capture_offline_stem_source.assert_not_called()
        assert not stems._requested_generations
        assert all(not owner.current for owner in admitted.values())
        assert not stem_backend.requests
        assert controller.stem_batch.summary == batch
        assert controller.project.model_dump() == before
        assert not controller.session.settings_open
        retained = _delete_summary(controller)
        controller.on_frame_render()
        assert controller.stem_delete.summary == retained
        assert not stems._requested_generations
        assert not stems._jobs
        assert not any(event.is_set() for event in held.queued)


def test_old_215_slot_warning_preserves_new_pad_216_and_new_same_settings_admission(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with admission_fixtures._held_pool() as held:
        held.fill(32)
        pending_fixtures._production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        _populate_duplicate_slots(controller, audio_engine_mock, 215)
        old_token = StemGenerationInterest("same-label")
        old_owner = controller.stems.request_generation_interest(0, old_token)
        assert old_owner is not None
        controller.performance_confirmation.request_all_loaded_stem_deletion()
        warning = controller.performance_confirmation.pending
        assert isinstance(warning, StemDeleteConfirmation)
        assert warning.inventory.items[214] is not None
        assert warning.inventory.items[215] is None
        assert sum(item is not None for item in warning.inventory.items) == 215
        controller.stems.cancel_generation_interest(old_owner, old_token)
        replacement = controller.stems.request_generation_interest(
            0, StemGenerationInterest(old_token.owner)
        )
        assert replacement is not None
        source = controller.project.sample_paths[0]
        assert source is not None
        controller.project.sample_paths[215] = source
        controller.project.pad_content[215] = PadContentIdentity(instance_id="f" * 32)
        audio_engine_mock.load_sample_async(215, source)
        before = controller.project.model_dump()
        controller.performance_confirmation.accept(warning)

        for _ in range(27):
            controller.on_frame_render()
        summary = _delete_summary(controller)
        assert (summary.total, summary.skipped, summary.noop, summary.removed) == (215, 1, 214, 0)
        assert not summary.active
        assert summary.items[0].state == "skipped"
        assert summary.items[-1].pad_id == 214
        assert replacement.current
        assert controller.stems._requested_generations[0].owner is replacement
        assert controller.project.model_dump() == before
        assert not controller.stems._jobs
        audio_engine_mock.capture_offline_stem_source.assert_not_called()
        controller.on_frame_render()
        assert controller.stem_delete.summary == summary
        assert replacement.current


def test_prepare_shutdown_advances_logical_delete_without_closing_backend_early(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with admission_fixtures._held_pool() as held:
        held.fill(32)
        pending_fixtures._production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        _populate_duplicate_slots(controller, audio_engine_mock, NUM_SAMPLES)
        _confirm_generation_batch(controller)
        controller.performance_confirmation.request_all_loaded_stem_deletion()
        warning = controller.performance_confirmation.pending
        assert isinstance(warning, StemDeleteConfirmation)
        controller.performance_confirmation.accept(warning)

        for frame in range(27):
            assert controller.prepare_shutdown() == (frame == 26)
            summary = _delete_summary(controller)
            assert summary.removed == (frame + 1) * 8
            assert summary.queued == NUM_SAMPLES - summary.removed
            assert not controller.stems._shutting_down
            assert not controller._shutdown_complete
            audio_engine_mock.shut_down.assert_not_called()
            audio_engine_mock.stop_all.assert_not_called()
            assert held.pool.try_reserve() is None
        assert not controller.stem_delete.active
        assert not controller.stems._requested_generations
        assert not controller.stems._jobs
        controller.shut_down()
        assert controller.stems._shutting_down
        assert controller._shutdown_complete
        audio_engine_mock.shut_down.assert_called_once()


def test_committed_disk_selection_delete_and_stale_neighbor_survive_actual_config_reopen(
    disk_commit: disk_fixtures._DiskCommit,
) -> None:
    rig = disk_commit
    app = rig.controller
    rig.jobs.pop()()
    app.stems.on_frame_render()
    current = app.project.stem_cache[0]
    assert current is not None
    assert current.pair is not None
    assert current.available
    assert rig.prepared.selected
    assert app._persistence.config_path.read_bytes() != rig.encoded
    batch_fixtures._loaded_metadata(app, rig.audio, (0, 1))
    app.project.selected_pad = 215
    app.project.selected_bank = 5
    app.session.settings_open = False
    sources = tuple(app.project.sample_paths)
    app.performance_confirmation.request_all_loaded_stem_deletion()
    warning = app.performance_confirmation.pending
    assert isinstance(warning, StemDeleteConfirmation)
    app.project.pad_content[1] = PadContentIdentity(instance_id="a" * 32)
    app.performance_confirmation.accept(warning)
    app.on_frame_render()
    summary = _delete_summary(app)
    assert (summary.total, summary.removed, summary.skipped, summary.active) == (2, 1, 1, False)
    assert app.project.stem_cache[0] is None
    assert app.project.stem_cache[1] is rig.independent
    assert tuple(app.project.sample_paths) == sources
    assert (app.project.selected_pad, app.project.selected_bank) == (215, 5)
    assert not app.session.settings_open
    app._persistence.flush()
    reopened = ProjectPersistence.from_config_path(app._persistence.config_path)
    assert reopened.load_error is None
    assert reopened.project.stem_cache[0] is None
    assert reopened.project.pad_stem_mix_mode[0] == "full_mix"
    assert reopened.project.stem_cache[1] == rig.independent
    assert tuple(reopened.project.sample_paths) == sources
    assert (reopened.project.selected_pad, reopened.project.selected_bank) == (215, 5)
    assert Path(sources[0] or "").is_file()
    rig.audio.capture_offline_stem_source.assert_not_called()
