"""Real pool backpressure; cached-source and device fixtures are explicit substitutes."""

import threading
from typing import TYPE_CHECKING, cast

from flitzis_looper.controller.stem_job import DIRECT_STEM_INTEREST, StemGenerationInterest
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.controller.test_shared_stem_material import _shared
from tests.flitzis_looper.controller.test_stem_admission import _held_pool

if TYPE_CHECKING:
    from pathlib import Path
    from unittest.mock import Mock

    import pytest

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_job import StemGenerationInterests, StemGenerationJob
    from flitzis_looper.controller.stem_workers import StemWorkerPool
    from flitzis_looper.controller.stems import _RequestedStemGeneration
    from flitzis_looper_audio import OfflineStemSourceLease


def _production_pool(
    controller: AppController, audio: Mock, tmp_path: Path, pool: StemWorkerPool
) -> None:
    _shared(controller, audio, tmp_path)
    stems = controller.stems
    version = stems._loaded_source_version_for_pad(0)
    assert version is not None
    source = audio.capture_offline_stem_source(0, version)
    # The ordinary fixture supplies cached original metadata but no waveform API.
    # Supply that missing read-only Native envelope without changing admission.
    audio.waveform_source_identity.return_value = (
        source.source_generation,
        source.digest,
        source.target_shape[2],
        source.target_shape[0],
    )
    stems._assets.sync_assignments()
    audio.capture_offline_stem_source.reset_mock()
    stems._stem_worker_pool = pool
    stems._stem_task_runner = pool
    stems._pair_preparations._runner = pool


def test_full_pool_keeps_request_metadata_without_job_source_or_retirement_mutation(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with _held_pool() as held:
        held.fill(32)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        reserved = stems._assets._reserved
        token = StemGenerationInterest("pending-batch")
        owner = stems.request_generation_interest(0, token)
        assert owner is not None
        assert stems.generation_interest_is_pending(owner)
        assert stems.is_stem_generation_running(0)
        assert stems.stem_generation_stage(0) == "Queued"
        assert not stems._jobs
        assert not stems._generation_sources
        assert not stems._generation_intents
        assert not controller.session.stem_generating_sample_ids
        assert stems._assets._reserved == reserved
        audio_engine_mock.capture_offline_stem_source.assert_not_called()
        stems.on_frame_render()
        audio_engine_mock.capture_offline_stem_source.assert_not_called()
        assert stems.stem_generation_error(0) is None
        assert stems.cancel_generation_interest(owner, token).disposition == "detached"
        assert not stems.generation_interest_is_pending(owner)


def test_pending_direct_and_batch_join_one_metadata_owner_before_any_source_capture(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with _held_pool() as held:
        held.fill(32)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        batch = StemGenerationInterest("same-pending-batch")
        owner = stems.request_generation_interest(0, batch)
        assert owner is not None
        assert stems.generate_stems_async(0)
        assert stems.request_generation_interest(0, batch) is owner
        assert stems.cancel_generation_interest(owner, batch).disposition == "remaining"
        assert stems.generation_interest_is_pending(owner)
        assert not stems._jobs
        audio_engine_mock.capture_offline_stem_source.assert_not_called()
        stems.shut_down()


def test_pending_original_waits_through_arrangement_fence_then_stales_without_new_slot_error(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with _held_pool() as held:
        held.fill(32)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        token = StemGenerationInterest("original-pending-content")
        owner = stems.request_generation_interest(0, token)
        assert owner is not None
        stems._pair_deferred_sample_ids = frozenset({0})
        controller.project.pad_content[0] = PadContentIdentity(instance_id="f" * 32)
        stems.on_frame_render()
        assert owner.failure is None
        assert stems.generation_interest_is_pending(owner)
        stems._pair_deferred_sample_ids = frozenset()
        stems.on_frame_render()
        assert owner.failure is not None
        assert not stems.generation_interest_is_pending(owner)
        assert stems.stem_generation_error(0) is None
        assert not controller.session.stem_generating_sample_ids
        audio_engine_mock.capture_offline_stem_source.assert_not_called()


def test_delete_without_existing_set_withdraws_pending_generation_before_later_capacity(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with _held_pool() as held:
        held.fill(32)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        token = StemGenerationInterest("deleted-pending-item")
        owner = stems.request_generation_interest(0, token)
        assert owner is not None
        assert not stems.delete_stems(0)
        assert not owner.current
        assert not stems.generation_interest_is_pending(owner)
        assert not stems._jobs
        audio_engine_mock.capture_offline_stem_source.assert_not_called()


def _assert_original_admission(
    controller: AppController,
    audio: Mock,
    requested: _RequestedStemGeneration,
    owner: StemGenerationInterests,
    observed: list[StemGenerationJob],
    sources: list[OfflineStemSourceLease],
) -> None:
    stems = controller.stems
    assert len(observed) == len(sources) == len(stems._jobs) == 1
    job = observed[0]
    (row,) = job.subscribers()
    assert job.request is row.request is requested.request
    assert job.intent is row.intent is requested.intent
    assert job.source is row.source is sources[0]
    assert row.interest_owner is owner
    assert requested.capture.content is not None
    assert requested.capture.source_identity is not None
    assert row.content_id == requested.capture.content.instance_id
    assert row.source.source_generation == requested.capture.source_identity[0]
    assert stems._generation_sources[0] is row.source
    assert stems._generation_intents[0] is requested.intent
    assert 0 not in stems._requested_generations
    assert 0 in controller.session.stem_generating_sample_ids
    assert stems.generation_interest_is_pending(owner)
    assert stems.stem_generation_error(0) is None
    stems.on_frame_render()
    audio.capture_offline_stem_source.assert_called_once_with(0, requested.request.source_version)


def test_returned_capacity_admits_exact_original_request_and_remaining_direct_owner(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    entered, finish, returned = threading.Event(), threading.Event(), threading.Event()
    observed: list[StemGenerationJob] = []
    sources: list[OfflineStemSourceLease] = []

    def observe_resolution(job: StemGenerationJob) -> None:
        # Only this worker body is substituted. Pool reservation, source capture,
        # job construction and control-side owner transfer remain production code.
        observed.append(job)
        entered.set()
        try:
            assert finish.wait(5)
        finally:
            returned.set()

    with _held_pool() as held:
        held.fill(32)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        capture = audio_engine_mock.capture_offline_stem_source.side_effect

        def capture_source(sample_id: int, version: str) -> OfflineStemSourceLease:
            source = cast("OfflineStemSourceLease", capture(sample_id, version))
            sources.append(source)
            return source

        audio_engine_mock.capture_offline_stem_source.side_effect = capture_source
        monkeypatch.setattr(stems, "_resolve_stem_identity", observe_resolution)
        token = StemGenerationInterest("batch-before-direct")
        owner = stems.request_generation_interest(0, token)
        assert owner is not None
        requested = stems._requested_generations[0]
        assert requested.owner is owner
        assert stems.generate_stems_async(0)
        assert stems.cancel_generation_interest(owner, token).disposition == "remaining"
        assert owner.current == frozenset({DIRECT_STEM_INTEREST})
        assert stems._requested_generations[0] is requested
        audio_engine_mock.capture_offline_stem_source.assert_not_called()

        held.release.set()
        assert held.queued[-1].wait(5)
        try:
            stems.on_frame_render()
            assert entered.wait(5)
            _assert_original_admission(
                controller, audio_engine_mock, requested, owner, observed, sources
            )
        finally:
            finish.set()
            assert returned.wait(5)
            stems.shut_down()


def test_pending_settings_aba_expires_before_source_admission_when_capacity_returns(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with _held_pool() as held:
        held.fill(32)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        token = StemGenerationInterest("pending-settings-aba")
        owner = stems.request_generation_interest(0, token)
        assert owner is not None
        requested = stems._requested_generations[0]
        shifts = controller.project.demucs_shifts
        overlap = controller.project.demucs_overlap
        controller.settings.set_demucs_quality(shifts=shifts + 1, overlap=overlap)
        controller.settings.set_demucs_quality(shifts=shifts, overlap=overlap)
        assert (
            controller.project.stem_processing_revision == requested.capture.processing_revision + 2
        )

        stems.on_frame_render()
        assert owner.failure is not None
        assert not stems.generation_interest_is_pending(owner)
        assert not stems.is_stem_generation_running(0)
        held.release.set()
        assert held.queued[-1].wait(5)
        stems.on_frame_render()
        audio_engine_mock.capture_offline_stem_source.assert_not_called()
        assert not stems._jobs
        assert not controller.session.stem_generating_sample_ids
        assert stems.stem_generation_error(0) is None


def test_same_source_bytes_with_later_loaded_generation_never_inherit_pending_request(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with _held_pool() as held:
        held.fill(32)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        stems = controller.stems
        token = StemGenerationInterest("pending-source-aba")
        owner = stems.request_generation_interest(0, token)
        assert owner is not None
        requested = stems._requested_generations[0]
        assert requested.capture.source_identity is not None
        generation, digest, frames, rate = requested.capture.source_identity
        # Explicit cached Native metadata substitute: same path/bytes/shape, new
        # loaded incarnation after an intervening replacement.
        audio_engine_mock.waveform_source_identity.return_value = (
            generation + 2,
            digest,
            frames,
            rate,
        )
        stems.on_frame_render()
        assert owner.failure is not None
        assert not stems.generation_interest_is_pending(owner)
        held.release.set()
        assert held.queued[-1].wait(5)
        stems.on_frame_render()
        audio_engine_mock.capture_offline_stem_source.assert_not_called()
        assert not stems._jobs
        assert not stems._generation_sources
        assert not stems._generation_intents
        assert not controller.session.stem_generating_sample_ids
        assert stems.stem_generation_error(0) is None
