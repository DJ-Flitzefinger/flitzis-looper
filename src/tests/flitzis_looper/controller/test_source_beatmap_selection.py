"""Productive controller/config/assignment fault routing; actual Native proof is separate."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.app import AppController
from flitzis_looper.controller.asset_lifecycle import AssignmentPublicationPlan
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import PadContentIdentity, ProjectState
from flitzis_looper.source_beatmap_intent import next_source_beatmap_intent
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import FakeProjectAssetLease
from tests.flitzis_looper.controller.test_source_beatmap_persistence import reference

if TYPE_CHECKING:
    from pathlib import Path

    from tests.flitzis_looper.conftest import FakeStemGenerationBackend


def _assigned(controller: AppController, audio: Mock) -> None:
    controller.project.sample_paths[0] = "samples/shared.wav"
    controller.project.pad_content[0] = PadContentIdentity(instance_id="e" * 32)
    audio.waveform_source_identity.return_value = (1, "f" * 64, 4096, 48000)
    audio._resolve_loader_event.return_value = (0, False)
    controller._assets.sync_assignments()
    controller._persistence.flush()


def _job(audio: Mock) -> Mock:
    job = Mock()
    job.source_beatmap_async_status.return_value = None
    job.is_cancelled.return_value = False
    audio.begin_source_beatmap_store_async.return_value = job
    audio.begin_source_beatmap_restore_async.return_value = job
    return job


def _event(job: Mock, generation: str = "a") -> dict[str, object]:
    artifact = Mock()
    artifact.receipt_json.return_value = reference(generation).model_dump_json()
    job.resolve_source_beatmap_artifact_completion.return_value = artifact
    return {
        "type": "source_beatmap_completed",
        "id": 0,
        "_native_event_owner": object(),
        "artifact_receipt": "spoofed-unused",
    }


def test_original_job_delivery_commits_compact_selection_and_publishes_saved_owner(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    audio = audio_engine_mock
    _assigned(controller, audio)
    job = _job(audio)
    maps = controller.loader.source_beatmaps
    maps.select(0, "Native validates the real record")
    assert maps.status(0) == "pending"
    assert controller.project.pad_source_beatmap_intent[0].selected is None
    event = _event(job)
    maps.completed(0, event)
    selected = controller.project.pad_source_beatmap_intent[0]
    assert selected.revision == 1
    assert selected.selected == reference()
    saved = ProjectPersistence.from_config_path(controller._persistence.config_path)
    assert saved.project.pad_source_beatmap_intent[0] == selected
    assert maps.status(0) == "effective"
    job.resolve_source_beatmap_artifact_completion.assert_called_once_with(
        event["_native_event_owner"]
    )
    lease = controller._assets._assignments["source_beatmap", 0][1]
    assert lease.source_beatmap_acknowledgements > 0
    assert controller.project.sample_analysis[0] is None
    assert controller.project.sample_paths[0] == "samples/shared.wav"


def test_unowned_dictionary_never_resolves_or_selects_candidate(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    job = _job(audio_engine_mock)
    maps = controller.loader.source_beatmaps
    maps.select(0, "record")
    maps.completed(0, {"artifact_receipt": reference().model_dump_json(), "map": Mock()})
    job.resolve_source_beatmap_artifact_completion.assert_not_called()
    assert maps.status(0) == "pending"


def test_failed_selection_commit_keeps_old_selection_and_retires_only_candidate(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _assigned(controller, audio_engine_mock)
    maps = controller.loader.source_beatmaps
    previous = next_source_beatmap_intent(None, reference())
    controller.project.pad_source_beatmap_intent[0] = previous
    controller._assets.sync_assignments()
    controller._persistence.flush()
    before = controller._persistence.config_path.read_bytes()
    job = _job(audio_engine_mock)
    maps.select(0, "record")
    requested = controller.project.pad_source_beatmap_intent[0]

    def refuse(*_args: object, **_kwargs: object) -> None:
        message = "injected Config failure"
        raise OSError(message)

    monkeypatch.setattr(controller._persistence, "_atomic_write_text", refuse)
    maps.completed(0, _event(job, "b"))
    assert controller.project.pad_source_beatmap_intent[0] is requested
    assert requested.selected == previous.selected
    assert controller._persistence.config_path.read_bytes() == before
    assert controller._assets._assignments["source_beatmap", 0][0].name == "map.json"
    assert "injected Config failure" in maps.errors[0]
    assert controller._assets._reserved == 0


def test_clear_advances_revision_and_late_completion_cannot_revive_previous_request(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    job = _job(audio_engine_mock)
    maps = controller.loader.source_beatmaps
    maps.select(0, "record")
    maps.clear(0)
    current = controller.project.pad_source_beatmap_intent[0]
    assert current.revision == 2
    assert current.selected is None
    maps.completed(0, _event(job))
    job.resolve_source_beatmap_artifact_completion.assert_not_called()
    job.cancel.assert_called_once()
    assert controller.project.pad_source_beatmap_intent[0] is current
    saved = ProjectPersistence.from_config_path(controller._persistence.config_path)
    assert saved.project.pad_source_beatmap_intent[0] == current


def test_restore_is_independent_of_manual_bpm_and_requires_fresh_genuine_completion(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    project = controller.project
    project.manual_bpm[0] = 123.0
    project.pad_timing_intent[0] = "tap"
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, reference())
    controller._persistence.flush()
    job = _job(audio_engine_mock)
    maps = controller.loader.source_beatmaps
    assert maps.effective(0) is None
    maps.poll(frozenset())
    audio_engine_mock.begin_source_beatmap_restore_async.assert_called_once_with(
        0, reference().model_dump_json()
    )
    assert maps.status(0) == "restoring"
    assert maps.effective(0) is None
    maps.completed(0, _event(job))
    assert maps.status(0) == "effective"
    assert project.manual_bpm[0] == 123.0
    assert project.pad_timing_intent[0] == "tap"


def test_postcommit_assignment_fault_retains_original_plan_and_settles_on_poll(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _assigned(controller, audio_engine_mock)
    job = _job(audio_engine_mock)
    maps = controller.loader.source_beatmaps
    original = AssignmentPublicationPlan.apply
    attempts = 0

    def interrupted(plan: AssignmentPublicationPlan) -> None:
        nonlocal attempts
        original(plan)
        attempts += 1
        if attempts == 1:
            message = "injected signal after actual assignment publication"
            raise OSError(message)

    monkeypatch.setattr(AssignmentPublicationPlan, "apply", interrupted)
    maps.select(0, "record")
    maps.completed(0, _event(job))
    assert len(maps._publications) == 1
    saved = ProjectPersistence.from_config_path(controller._persistence.config_path)
    assert saved.project.pad_source_beatmap_intent[0].selected == reference()
    maps.poll(frozenset())
    assert not maps._publications
    assert maps.status(0) == "effective"
    assert controller._assets._reserved == 0


def test_failed_clear_keeps_current_selection_and_pending_original_job(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _assigned(controller, audio_engine_mock)
    maps = controller.loader.source_beatmaps
    previous = next_source_beatmap_intent(None, reference())
    controller.project.pad_source_beatmap_intent[0] = previous
    controller._assets.sync_assignments()
    controller._persistence.flush()
    before = controller._persistence.config_path.read_bytes()
    job = _job(audio_engine_mock)
    maps.select(0, "record")
    requested = controller.project.pad_source_beatmap_intent[0]

    def refuse(*_args: object, **_kwargs: object) -> None:
        message = "injected failed clear"
        raise OSError(message)

    monkeypatch.setattr(controller._persistence, "_atomic_write_text", refuse)
    with pytest.raises(OSError, match="failed clear"):
        maps.clear(0)
    assert controller.project.pad_source_beatmap_intent[0] is requested
    assert requested.selected == previous.selected
    assert controller._persistence.config_path.read_bytes() == before
    job.cancel.assert_not_called()
    assert maps.status(0) == "pending"


def test_native_admission_refusal_does_not_mutate_intent_or_reservation(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    previous = next_source_beatmap_intent(None, reference())
    controller.project.pad_source_beatmap_intent[0] = previous
    audio_engine_mock.begin_source_beatmap_store_async.side_effect = RuntimeError("queue full")
    with pytest.raises(RuntimeError, match="queue full"):
        controller.loader.source_beatmaps.select(0, "record")
    assert controller.project.pad_source_beatmap_intent[0] is previous
    assert controller._assets._reserved == 0


def test_native_source_supersession_releases_pending_owner_even_if_event_was_stale(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    job = _job(audio_engine_mock)
    maps = controller.loader.source_beatmaps
    maps.select(0, "record")
    audio_engine_mock.waveform_source_identity.return_value = (2, "d" * 64, 8192, 48000)
    job.is_cancelled.return_value = True
    maps.poll(frozenset())
    job.cancel.assert_called_once()
    assert maps.status(0) == "none"
    assert controller._assets._reserved == 0


def test_failed_restore_retries_only_after_source_context_changes(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    controller.project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, reference())
    job = _job(audio_engine_mock)
    maps = controller.loader.source_beatmaps
    maps.poll(frozenset())
    job.source_beatmap_async_status.return_value = {
        "terminal": True,
        "stage": "failed",
        "detail": "actual source mismatch",
    }
    maps.poll(frozenset())
    maps.poll(frozenset())
    assert audio_engine_mock.begin_source_beatmap_restore_async.call_count == 1
    assert maps.status(0) == "failed"
    audio_engine_mock.waveform_source_identity.return_value = (2, "f" * 64, 4096, 48000)
    maps.poll(frozenset())
    assert audio_engine_mock.begin_source_beatmap_restore_async.call_count == 2


def test_candidate_owner_admission_failure_retires_through_original_artifact_proof(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _assigned(controller, audio_engine_mock)
    maps = controller.loader.source_beatmaps
    job = _job(audio_engine_mock)
    maps.select(0, "record")
    event = _event(job)

    def refuse(*_args: object) -> None:
        message = "project asset owner registry full"
        raise RuntimeError(message)

    monkeypatch.setattr(controller._assets, "prepare_assignment_image", refuse)
    acquired = audio_engine_mock.acquire_project_asset_lease.call_count
    maps.completed(0, event)
    artifact = job.resolve_source_beatmap_artifact_completion.return_value
    artifact.retire_unselected.assert_called_once()
    assert audio_engine_mock.acquire_project_asset_lease.call_count == acquired
    assert controller.project.pad_source_beatmap_intent[0].selected is None
    assert "registry full" in maps.errors[0]
    assert controller._assets._reserved == 0


def test_actual_app_initialization_keeps_project_when_saved_map_backing_is_missing(
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(tmp_path)
    source = tmp_path / "samples" / "shared.wav"
    source.parent.mkdir()
    write_mono_pcm16_wav(source, 44_100)
    project = ProjectState(volume=0.61)
    project.sample_paths[0] = "samples/shared.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="e" * 32)
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, reference())
    persistence = ProjectPersistence(project)
    persistence.flush()
    before = persistence.config_path.read_bytes()

    class MissingMapLease(FakeProjectAssetLease):
        def bind_source_beatmap(self, _receipt: str) -> None:
            message = "map backing file identity unavailable"
            raise FileNotFoundError(message)

    audio_engine_mock.acquire_project_asset_lease.side_effect = MissingMapLease
    audio_engine_mock.waveform_source_identity.return_value = None
    app = AppController(stem_backend=stem_backend, stem_task_runner=lambda work: work())
    try:
        assert app.project.volume == 0.61
        assert app.project.sample_paths[0] == "samples/shared.wav"
        assert app.project.pad_source_beatmap_intent[0] == project.pad_source_beatmap_intent[0]
        assert app.loader.source_beatmaps.effective(0) is None
        assert persistence.config_path.read_bytes() == before
        lease = app._assets._assignments["source_beatmap", 0][1]
        assert not lease.released
        assert not lease.source_beatmap_bound
        assert lease.source_beatmap_acknowledgements == 0
    finally:
        app.shut_down()


def test_restore_waits_for_complete_control_side_loader_ack(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _assigned(controller, audio_engine_mock)
    project = controller.project
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, reference())
    job = _job(audio_engine_mock)
    controller.session.loading_sample_ids.add(0)
    audio_engine_mock.poll_loader_events.side_effect = [None]
    controller.loader.poll_loader_events()
    audio_engine_mock.begin_source_beatmap_restore_async.assert_not_called()

    def deliver(*_args: object) -> None:
        audio_engine_mock.begin_source_beatmap_restore_async.assert_not_called()
        # Routing-only stand-in for the actual productive success handler. Its
        # complete source/timing ACK is exercised by the real Native vertical.
        controller.session.loading_sample_ids.discard(0)

    monkeypatch.setattr(controller.loader, "_deliver_loader_event", deliver)
    audio_engine_mock.poll_loader_events.side_effect = [{"type": "success", "id": 0}, None]
    controller.loader.poll_loader_events()
    audio_engine_mock.begin_source_beatmap_restore_async.assert_called_once()
    job.cancel.assert_not_called()


def test_waveform_cache_changes_do_not_replace_native_job_currentness(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    _assigned(controller, audio_engine_mock)
    job = _job(audio_engine_mock)
    maps = controller.loader.source_beatmaps
    maps.select(0, "record")
    audio_engine_mock.waveform_source_identity.return_value = (1, "d" * 64, 2048, 48000)
    maps.poll(frozenset())
    job.cancel.assert_not_called()
    assert maps.status(0) == "pending"
