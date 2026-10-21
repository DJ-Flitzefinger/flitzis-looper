"""Raw controller routing and lifecycle; actual Native custody is tested separately."""

from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.controller.raw_source_beatmap_fakes import (
    FakeRawCandidate,
    raw_event,
    raw_job,
)
from tests.flitzis_looper.controller.test_source_beatmap_selection import _assigned, _event, _job

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.source_beatmap_selection import SourceBeatmapSelection


def _raw_assigned(controller: AppController, audio: Mock) -> SourceBeatmapSelection:
    _assigned(controller, audio)
    maps = controller.loader.source_beatmaps
    maps._source_generation = Mock(return_value=71)
    return maps


def _review(controller: AppController, audio: Mock) -> tuple[Mock, FakeRawCandidate]:
    maps = _raw_assigned(controller, audio)
    job, candidate = raw_job(audio), FakeRawCandidate()
    maps.analyze_original(0, expected_attempt=0)
    maps.raw_completed(0, raw_event(job, candidate))
    return job, candidate


def test_app_binds_explicit_local_worker_root_without_changing_ordinary_analysis(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    assert (
        controller._beat_this_installation_root
        == Path.cwd().absolute().parent / "analysis-runtime" / "beat-this"
    )
    raw_job(audio_engine_mock)
    maps.analyze_original(0, expected_attempt=0)
    audio_engine_mock.begin_source_beatmap_raw_async.assert_not_called()
    audio_engine_mock.begin_offline_analysis.assert_not_called()


def test_raw_original_completion_preserves_accepted_delivery_config_and_audio(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    audio = audio_engine_mock
    maps = _raw_assigned(controller, audio)
    accepted_job = _job(audio)
    maps.select(0, "Native accepted record")
    maps.completed(0, _event(accepted_job))
    before, intent = (
        controller.project.model_dump(),
        controller.project.pad_source_beatmap_intent[0],
    )
    saved = controller._persistence.config_path.read_bytes()
    geometry, delivery = maps.effective(0), maps.playback_delivery(0)
    changed = Mock()
    monkeypatch.setattr(maps, "changed", changed)
    job, candidate = raw_job(audio), FakeRawCandidate()
    audio.reset_mock()

    maps.analyze_original(0, expected_attempt=0)
    assert maps.status(0) == "effective"
    event = raw_event(job, candidate)
    audio.poll_loader_events.side_effect = [event, None]
    controller.loader.poll_loader_events()
    page = maps.inspect_raw(
        0, expected_attempt=1, expected_revision=candidate.revision, start_index=0
    )

    assert len(page["events"]) == 128
    assert maps.raw_state(0).status == "unreviewed"
    assert maps.effective(0) is geometry
    assert maps.playback_delivery(0) is delivery
    assert controller.project.pad_source_beatmap_intent[0] is intent
    assert controller.project.model_dump() == before
    assert controller._persistence.config_path.read_bytes() == saved
    changed.assert_not_called()
    job.resolve_source_beatmap_raw_completion.assert_called_once_with(event["_native_event_owner"])
    job.resolve_source_beatmap_artifact_completion.assert_not_called()
    audio.begin_source_beatmap_beat_this_raw_async.assert_called_once_with(
        sample_id=0,
        installation_root=str(controller._beat_this_installation_root),
        expected_source_generation=71,
    )
    audio.begin_source_beatmap_raw_async.assert_not_called()
    audio.seek_sample.assert_not_called()
    audio.stop_all.assert_not_called()


def test_raw_only_never_enters_accepted_pending_or_marks_project_changed(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    before = controller.project.model_dump()
    changed = Mock()
    monkeypatch.setattr(maps, "changed", changed)
    job = raw_job(audio_engine_mock)
    maps.analyze_original(0, expected_attempt=0)
    assert maps.raw_state(0).status == "queued"
    assert not maps.is_selected(0)
    assert maps.status(0) == "none"
    assert not maps._pending
    assert maps.effective(0) is None
    assert maps.playback_delivery(0) is None
    job.source_beatmap_async_status.return_value["stage"] = "running"
    maps.poll(frozenset())
    assert maps.raw_state(0).status == "running"
    assert controller.project.model_dump() == before
    assert not maps.errors
    changed.assert_not_called()


@pytest.mark.parametrize("pending_at", ["native_resolution", "source_generation"])
def test_raw_completion_retries_original_event_when_placement_becomes_pending_after_lookup(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    pending_at: str,
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    accepted_job = _job(audio_engine_mock)
    maps.select(0, "Native accepted record")
    maps.completed(0, _event(accepted_job))
    before = controller.project.model_dump()
    saved = controller._persistence.config_path.read_bytes()
    geometry, delivery = maps.effective(0), maps.playback_delivery(0)
    changed = Mock()
    monkeypatch.setattr(maps, "changed", changed)
    job, candidate = raw_job(audio_engine_mock), FakeRawCandidate()
    maps.analyze_original(0, expected_attempt=0)
    event = raw_event(job, candidate)
    owner = event["_native_event_owner"]
    job.source_beatmap_async_status.return_value.update(
        stage="ready", terminal=True, raw_review_retired=False
    )
    # The first Native loader lookup succeeds. A global publication may begin
    # before either of these later checks, even for an unrelated participant.
    audio_engine_mock._resolve_loader_event.return_value = (0, False)
    if pending_at == "native_resolution":
        job.resolve_source_beatmap_raw_completion.side_effect = [
            RuntimeError("loader event content placement pending"),
            candidate,
        ]
    else:
        maps._source_generation = Mock(side_effect=[None, 71])
    audio_engine_mock.poll_loader_events.side_effect = [event, None]

    controller.loader.poll_loader_events()

    assert maps.raw_state(0).status == "running"
    assert controller.loader._pair_held_loader_events == [event]
    assert controller.loader._pair_held_loader_events[0] is event
    job.cancel.assert_not_called()
    assert controller.project.model_dump() == before
    assert controller._persistence.config_path.read_bytes() == saved
    changed.assert_not_called()
    audio_engine_mock.poll_loader_events.side_effect = [None]

    controller.loader.poll_loader_events()

    assert not controller.loader._pair_held_loader_events
    assert maps.raw_state(0).status == "unreviewed"
    assert maps._raw is not None
    assert maps._raw.candidate is candidate
    assert maps._raw.delivery is owner
    assert maps.effective(0) is geometry
    assert maps.playback_delivery(0) is delivery
    assert controller.project.model_dump() == before
    assert controller._persistence.config_path.read_bytes() == saved
    changed.assert_not_called()
    job.cancel.assert_not_called()
    assert job.resolve_source_beatmap_raw_completion.call_count == 2
    assert all(
        call.args[0] is owner for call in job.resolve_source_beatmap_raw_completion.call_args_list
    )


def test_raw_completion_does_not_hide_unrelated_native_resolution_errors(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    job, candidate = raw_job(audio_engine_mock), FakeRawCandidate()
    maps.analyze_original(0, expected_attempt=0)
    event = raw_event(job, candidate)
    job.resolve_source_beatmap_raw_completion.side_effect = RuntimeError("raw resolution failed")
    audio_engine_mock.poll_loader_events.side_effect = [event, None]

    with pytest.raises(RuntimeError, match="raw resolution failed"):
        controller.loader.poll_loader_events()

    assert not controller.loader._pair_held_loader_events
    job.cancel.assert_not_called()


def test_raw_completion_still_cancels_its_original_job_for_a_different_source_generation(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    before = controller.project.model_dump()
    job, candidate = raw_job(audio_engine_mock), FakeRawCandidate()
    maps.analyze_original(0, expected_attempt=0)
    maps._source_generation = Mock(return_value=72)
    event = raw_event(job, candidate)
    audio_engine_mock.poll_loader_events.side_effect = [event, None]

    controller.loader.poll_loader_events()

    assert not controller.loader._pair_held_loader_events
    assert maps.raw_state(0).status == "cancel_retiring"
    assert maps._raw is not None
    assert maps._raw.job is job
    assert maps._raw.candidate is None
    assert controller.project.model_dump() == before
    job.cancel.assert_called_once()


def test_public_raw_fields_and_another_delivery_cannot_resolve_original_job(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    job, candidate = raw_job(audio_engine_mock), FakeRawCandidate()
    maps.analyze_original(0, expected_attempt=0)
    original = raw_event(job, candidate)
    maps.raw_completed(0, {"raw_candidate": candidate, "raw_revision": candidate.revision})
    job.resolve_source_beatmap_raw_completion.assert_not_called()
    maps.raw_completed(0, {"_native_event_owner": object(), "raw_candidate": candidate})
    assert maps.raw_state(0).status == "queued"
    maps.raw_completed(0, original)
    assert maps.raw_state(0).status == "unreviewed"
    assert controller.project.pad_source_beatmap_intent[0] is None


def test_discard_requires_actual_job_and_raw_reader_retirement_before_reacquisition(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    job, candidate = _review(controller, audio_engine_mock)
    maps = controller.loader.source_beatmaps
    maps.discard_raw(0, expected_attempt=1, expected_revision=candidate.revision)
    assert maps._raw is not None
    assert maps._raw.candidate is None
    assert maps._raw.delivery is None
    job.cancel.assert_called_once()
    for terminal, retired in ((False, False), (True, False), (False, True)):
        job.source_beatmap_async_status.return_value = {
            "stage": "cancelled",
            "terminal": terminal,
            "raw_review_retired": retired,
        }
        maps.poll(frozenset())
        assert maps.raw_state(0).status == "cancel_retiring"
        with pytest.raises(RuntimeError, match="occupied or retiring"):
            maps.analyze_original(0, expected_attempt=1)
    job.source_beatmap_async_status.return_value.update(terminal=True, raw_review_retired=True)
    maps.poll(frozenset())
    assert maps.raw_state(0).status == "none"
    second = raw_job(audio_engine_mock)
    maps.analyze_original(0, expected_attempt=1)
    maps.raw_completed(0, raw_event(second, FakeRawCandidate()))
    assert maps.raw_state(0).attempt == 2
    with pytest.raises(ValueError, match="attempt changed"):
        maps.inspect_raw(0, expected_attempt=1, expected_revision=candidate.revision, start_index=0)
    with pytest.raises(ValueError, match="attempt changed"):
        maps.discard_raw(0, expected_attempt=1, expected_revision=candidate.revision)
    assert maps.raw_state(0).status == "unreviewed"


def test_failed_raw_job_waits_for_retirement_and_explicit_reacquire(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    job = raw_job(audio_engine_mock)
    maps.analyze_original(0, expected_attempt=0)
    job.source_beatmap_async_status.return_value = {
        "stage": "failed",
        "detail": "injected QM failure",
        "terminal": True,
        "raw_review_retired": False,
    }
    maps.poll(frozenset())
    assert maps.raw_state(0).status == "cancel_retiring"
    job.source_beatmap_async_status.return_value["raw_review_retired"] = True
    maps.poll(frozenset())
    assert maps.raw_state(0).status == "failed"
    assert maps.raw_state(0).error == "injected QM failure"
    assert not maps.errors
    raw_job(audio_engine_mock)
    maps.analyze_original(0, expected_attempt=1)
    assert maps.raw_state(0).attempt == 2
    assert maps.raw_state(0).error is None


def test_raw_admission_refusal_preserves_attempt_intent_and_review_slot(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    raw_job(audio_engine_mock)
    audio_engine_mock.begin_source_beatmap_beat_this_raw_async.side_effect = RuntimeError(
        "cold queue full"
    )
    before = controller.project.model_dump()
    with pytest.raises(RuntimeError, match="cold queue full"):
        maps.analyze_original(0, expected_attempt=0)
    assert maps.raw_state(0).attempt == 0
    assert not maps.raw_state(0).occupied
    assert controller.project.model_dump() == before


@pytest.mark.parametrize("generation", [None, 0, False, -1, 1 << 64])
def test_raw_begin_requires_current_native_generation(
    controller: AppController, audio_engine_mock: Mock, generation: object
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    maps._source_generation = Mock(return_value=generation)
    raw_job(audio_engine_mock)
    with pytest.raises(ValueError, match="current Native source generation"):
        maps.analyze_original(0, expected_attempt=0)
    audio_engine_mock.begin_source_beatmap_beat_this_raw_async.assert_not_called()


def test_raw_begin_refuses_accepted_preparation_and_terminal_candidate_allows_edit(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    accepted = _job(audio_engine_mock)
    maps.select(0, "record")
    job = raw_job(audio_engine_mock)
    with pytest.raises(RuntimeError, match="Accepted source map preparation"):
        maps.analyze_original(0, expected_attempt=0)
    audio_engine_mock.begin_source_beatmap_beat_this_raw_async.assert_not_called()
    maps.completed(0, _event(accepted))
    geometry = accepted.resolve_source_beatmap_artifact_completion.return_value.map.return_value
    geometry.revision.return_value = "a" * 64
    maps.analyze_original(0, expected_attempt=0)
    candidate = FakeRawCandidate()
    maps.raw_completed(0, raw_event(job, candidate))
    job.source_beatmap_async_status.return_value.update(
        stage="completed", terminal=True, raw_review_retired=False
    )
    maps.poll(frozenset())
    edited = Mock()
    audio_engine_mock.begin_source_beatmap_edit_async = Mock(return_value=edited)
    maps.edit(0, {"schema_version": 1, "operation": "undo"}, expected_revision="a" * 64)
    assert maps.status(0) == "pending"
    assert maps.raw_state(0).status == "unreviewed"
    assert maps._raw is not None
    assert maps._raw.candidate is candidate
    assert job.source_beatmap_async_status.return_value["raw_review_retired"] is False
    audio_engine_mock.begin_source_beatmap_edit_async.assert_called_once()


def test_native_busy_edit_refusal_preserves_accepted_state_and_closes_reservation(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    accepted = _job(audio_engine_mock)
    maps.select(0, "record")
    maps.completed(0, _event(accepted))
    geometry = accepted.resolve_source_beatmap_artifact_completion.return_value.map.return_value
    geometry.revision.return_value = "b" * 64
    intent = controller.project.pad_source_beatmap_intent[0]
    delivery = maps.playback_delivery(0)
    before = controller.project.model_dump()
    saved = controller._persistence.config_path.read_bytes()
    changed = Mock()
    monkeypatch.setattr(maps, "changed", changed)
    job = raw_job(audio_engine_mock)
    maps.analyze_original(0, expected_attempt=0)
    job.source_beatmap_async_status.return_value["stage"] = "running"
    maps.poll(frozenset())
    audio_engine_mock.begin_source_beatmap_edit_async = Mock(
        side_effect=RuntimeError("original offline source is busy")
    )

    with pytest.raises(RuntimeError, match="offline source is busy"):
        maps.edit(0, {"schema_version": 1, "operation": "undo"}, expected_revision="b" * 64)

    assert controller.project.pad_source_beatmap_intent[0] is intent
    assert maps.effective(0) is geometry
    assert maps.playback_delivery(0) is delivery
    assert controller.project.model_dump() == before
    assert controller._persistence.config_path.read_bytes() == saved
    assert maps.status(0) == "effective"
    assert not maps._pending
    assert maps.raw_state(0).status == "running"
    assert controller._assets._reserved == 0
    changed.assert_not_called()
    job.cancel.assert_not_called()


@pytest.mark.parametrize(("start", "limit"), [(-1, 128), (261, 128), (0, 0), (0, 129)])
def test_raw_inspection_keeps_native_bounded_query_parameters(
    controller: AppController, audio_engine_mock: Mock, start: int, limit: int
) -> None:
    _job_owner, candidate = _review(controller, audio_engine_mock)
    maps = controller.loader.source_beatmaps
    with pytest.raises(ValueError, match="query bounds"):
        maps.inspect_raw(
            0,
            expected_attempt=1,
            expected_revision=candidate.revision,
            start_index=start,
            max_events=limit,
        )


def test_raw_pages_preserve_event_observations_and_valid_empty_exclusive_end(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _job_owner, candidate = _review(controller, audio_engine_mock)
    maps = controller.loader.source_beatmaps
    first = maps.inspect_raw(
        0, expected_attempt=1, expected_revision=candidate.revision, start_index=0
    )
    tail = maps.inspect_raw(
        0, expected_attempt=1, expected_revision=candidate.revision, start_index=256
    )
    end = maps.inspect_raw(
        0, expected_attempt=1, expected_revision=candidate.revision, start_index=260
    )
    assert first["returned_count"] == 128
    assert first["has_more"]
    assert tail["returned_count"] == 4
    assert not tail["has_more"]
    assert not end["events"]
    assert end["returned_count"] == 0
    assert first["events"][0]["odf_frame"] == 0.125
    assert "beat" not in first["events"][0]


def test_placement_pending_hides_rows_and_genuine_native_stale_cancels_review(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    job, candidate = _review(controller, audio_engine_mock)
    maps = controller.loader.source_beatmaps
    candidate.status = "placement_pending"
    page = maps.inspect_raw(
        0, expected_attempt=1, expected_revision=candidate.revision, start_index=0
    )
    assert page["status"] == "placement_pending"
    assert not page["events"]
    assert maps.raw_state(0).status == "unreviewed"
    job.cancel.assert_not_called()
    candidate.status = "stale"
    assert maps.raw_state(0).status == "cancel_retiring"
    job.cancel.assert_called_once()


@pytest.mark.parametrize("swap", [False, True], ids=["move", "swap"])
def test_raw_original_completion_follows_content_placement_and_copy_is_independent(
    controller: AppController, audio_engine_mock: Mock, *, swap: bool
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    job, candidate = raw_job(audio_engine_mock), FakeRawCandidate()
    maps.analyze_original(0, expected_attempt=0)
    event = raw_event(job, candidate)
    original = controller.project.pad_content[0]
    controller.project.pad_content[0] = PadContentIdentity(instance_id="b" * 32) if swap else None
    controller.project.pad_content[2] = original
    controller.project.sample_paths[2] = "samples/shared.wav"
    # This is the original Native relocation seam, not a receipt made from numeric PAD IDs.
    audio_engine_mock._resolve_loader_event.return_value = (2, False)
    audio_engine_mock.poll_loader_events.side_effect = [event, None]
    controller.loader.poll_loader_events()
    assert maps.raw_state(2).status == "unreviewed"
    assert maps.raw_state(0).status == "none"
    assert event["id"] == 2
    controller.project.pad_content[3] = PadContentIdentity(instance_id="c" * 32)
    controller.project.sample_paths[3] = "samples/shared.wav"
    assert maps.raw_state(3).status == "none"
    assert maps.raw_state(3).occupied
    with pytest.raises(RuntimeError, match="occupied or retiring"):
        maps.analyze_original(3, expected_attempt=1)
    job.cancel.assert_not_called()


@pytest.mark.parametrize("replacement", [None, "f" * 32], ids=["unload", "bank-replacement"])
def test_removal_cancels_only_original_raw_job_and_retains_retiring_handle(
    controller: AppController, audio_engine_mock: Mock, replacement: str | None
) -> None:
    job, _candidate = _review(controller, audio_engine_mock)
    maps = controller.loader.source_beatmaps
    controller.project.pad_content[0] = (
        None if replacement is None else PadContentIdentity(instance_id=replacement)
    )
    maps.poll(frozenset())
    assert maps.raw_state(0).status == "none"
    assert maps.raw_state(0).occupied
    assert maps._raw is not None
    assert maps._raw.status == "cancel_retiring"
    job.cancel.assert_called_once()
    job.source_beatmap_async_status.return_value.update(terminal=True, raw_review_retired=True)
    maps.poll(frozenset())
    assert maps._raw is None


def test_source_generation_aba_fences_actions_but_waveform_cache_does_not_grant_currentness(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    job, candidate = _review(controller, audio_engine_mock)
    maps = controller.loader.source_beatmaps
    audio_engine_mock.waveform_source_identity.return_value = (71, "f" * 64, 999, 44_100)
    maps.poll(frozenset())
    assert maps.inspect_raw(
        0, expected_attempt=1, expected_revision=candidate.revision, start_index=0
    )["events"]
    job.cancel.assert_not_called()
    maps._source_generation = Mock(return_value=72)
    with pytest.raises(ValueError, match="source or attempt changed"):
        maps.inspect_raw(0, expected_attempt=1, expected_revision=candidate.revision, start_index=0)
    job.is_cancelled.return_value = True
    maps.poll(frozenset())
    assert maps.raw_state(0).status == "cancel_retiring"


def test_shutdown_cancels_only_owned_raw_work_and_keeps_actual_retirement_handle(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    job, _candidate = _review(controller, audio_engine_mock)
    maps = controller.loader.source_beatmaps
    maps.shut_down()
    job.cancel.assert_called_once()
    assert maps._raw is not None
    assert maps._raw.status == "cancel_retiring"
    assert maps._raw.candidate is None
    assert maps._raw.delivery is None
    audio_engine_mock.shut_down.assert_not_called()
