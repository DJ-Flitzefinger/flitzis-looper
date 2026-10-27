"""Control-side automatic custody; Native source/music qualification is tested separately."""

import json
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.transport.source_sync import SourceSyncStatus
from flitzis_looper.models import PadContentIdentity
from flitzis_looper.source_beatmap_intent import next_source_beatmap_intent
from flitzis_looper.timing_policy import GlobalTimingPolicy
from tests.flitzis_looper.controller.test_source_beatmap_persistence import reference
from tests.flitzis_looper.controller.test_source_beatmap_selection import _assigned
from tests.flitzis_looper.controller.transport.test_source_map_loop import _bind_map, _geometry

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


@pytest.fixture
def automatic(controller: AppController, audio_engine_mock: Mock) -> Mock:
    audio = audio_engine_mock
    _assigned(controller, audio)
    audio.AUTOMATIC_SOURCE_BEATMAP_SELECTION_API_VERSION = 1
    audio.AUTOMATIC_SOURCE_BEATMAP_JOB_API_VERSION = 1
    audio.GRID_PHASE_ORIGIN_API_VERSION = 1
    audio._resolve_loader_event.return_value = (0, True)
    artifact = Mock()
    artifact.receipt_json.return_value = reference().model_dump_json()
    audio.consume_automatic_source_beatmap = Mock(return_value=(0, artifact, object()))
    audio.begin_automatic_source_beatmap_async = Mock()
    audio.begin_automatic_source_beatmap_restore_async = Mock()
    maps = controller.loader.source_beatmaps
    maps._source_generation = lambda _sample_id: 1
    maps._on_effective = Mock()
    return artifact


def _adopt(controller: AppController) -> None:
    maps = controller.loader.source_beatmaps
    captured = maps._capture_automatic({"id": 0, "_native_event_owner": object()})
    maps._adopt_automatic(0, "e" * 32, None, captured)


def test_automatic_receipt_is_durable_without_replacing_any_tempo_or_loop_intent(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
) -> None:
    before = controller.project.model_dump(exclude={"pad_source_beatmap_intent", "config_revision"})
    revision = controller.project.config_revision
    _adopt(controller)
    selected = controller.project.pad_source_beatmap_intent[0]
    assert selected.automatic is True
    assert selected.selected == reference()
    assert controller.loader.source_beatmaps.effective(0) is automatic.map.return_value
    assert (
        controller.project.model_dump(exclude={"pad_source_beatmap_intent", "config_revision"})
        == before
    )
    assert controller.project.config_revision == revision + 1
    audio_engine_mock.consume_automatic_source_beatmap.assert_called_once()
    controller.loader.source_beatmaps._on_effective.assert_called_once_with(0)


def test_durable_retry_reuses_consumed_tuple_and_does_not_repeat_analysis_or_revision(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    maps = controller.loader.source_beatmaps
    original = maps._commit
    monkeypatch.setattr(maps, "_commit", Mock(side_effect=OSError("sharing violation")))
    _adopt(controller)
    assert controller.project.pad_source_beatmap_intent[0] is None
    assert maps.effective(0) is None
    monkeypatch.setattr(maps, "_commit", original)
    maps._retry_after.clear()
    maps.poll(frozenset(), admit_restore=False)
    assert controller.project.pad_source_beatmap_intent[0].revision == 1
    assert maps.effective(0) is automatic.map.return_value
    audio_engine_mock.consume_automatic_source_beatmap.assert_called_once()


def test_derived_source_loss_inside_commit_never_installs_effective_map(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    maps = controller.loader.source_beatmaps

    def lose_source(*_args: object) -> None:
        audio_engine_mock._resolve_loader_event.return_value = None

    monkeypatch.setattr(maps, "_commit", lose_source)
    _adopt(controller)
    assert controller.project.pad_source_beatmap_intent[0] is None
    assert maps.effective(0) is None
    automatic.retire_unselected.assert_called_once()


def test_manual_clear_after_staging_prevents_late_automatic_selection(
    controller: AppController,
    automatic: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    maps = controller.loader.source_beatmaps
    monkeypatch.setattr(maps, "_blocked", lambda: True)
    _adopt(controller)
    monkeypatch.setattr(maps, "_blocked", lambda: False)
    maps.clear(0)
    maps.poll(frozenset())
    intent = controller.project.pad_source_beatmap_intent[0]
    assert intent.selected is None
    assert intent.automatic is False
    automatic.retire_unselected.assert_called_once()


def test_map_only_job_waits_for_actual_retirement_after_derived_export(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
) -> None:
    controller.project.multi_loop = controller.project.bpm_lock = True
    controller.project.global_timing_policy = GlobalTimingPolicy(
        schema_version=1,
        sync=True,
        realtime_bpm=False,
        revision=1,
    )
    maps = controller.loader.source_beatmaps
    job = Mock()
    job.is_cancelled.return_value = False
    job.source_beatmap_async_status.return_value = {"terminal": False, "stage": "ready"}
    audio_engine_mock.begin_automatic_source_beatmap_async.return_value = job
    maps.poll(frozenset())
    maps.completed(0, {"_native_event_owner": object()})
    job.is_cancelled.return_value = True  # Original timing epoch changed after export.
    maps.poll(frozenset(), admit_restore=False)
    assert maps.effective(0) is None
    assert controller.project.pad_source_beatmap_intent[0] is None
    job.source_beatmap_async_status.return_value = {"terminal": True, "stage": "ready"}
    maps.poll(frozenset(), admit_restore=False)
    assert maps.effective(0) is automatic.map.return_value
    job.cancel.assert_not_called()


def test_optional_map_only_export_failure_does_not_leave_ready_job_stuck(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
) -> None:
    controller.project.multi_loop = controller.project.bpm_lock = True
    controller.project.global_timing_policy = GlobalTimingPolicy(
        schema_version=1,
        sync=True,
        realtime_bpm=False,
        revision=1,
    )
    maps = controller.loader.source_beatmaps
    job = Mock()
    job.is_cancelled.return_value = False
    job.source_beatmap_async_status.return_value = {"terminal": True, "stage": "ready"}
    audio_engine_mock.begin_automatic_source_beatmap_async.return_value = job
    maps.poll(frozenset())
    audio_engine_mock.consume_automatic_source_beatmap.side_effect = RuntimeError(
        "optional export failed"
    )
    maps.completed(0, {"id": 0, "_native_event_owner": object()})
    maps.poll(frozenset())
    assert not maps._pending
    assert maps.effective(0) is None
    assert "optional export failed" in maps.errors[0]
    assert audio_engine_mock.begin_automatic_source_beatmap_async.call_count == 1
    job.cancel.assert_called_once()


def test_timing_edit_before_map_export_does_not_poison_source_retry(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
) -> None:
    controller.project.multi_loop = controller.project.bpm_lock = True
    controller.project.global_timing_policy = GlobalTimingPolicy(
        schema_version=1,
        sync=True,
        realtime_bpm=False,
        revision=1,
    )
    maps = controller.loader.source_beatmaps
    job = Mock()
    job.is_cancelled.return_value = False
    job.source_beatmap_async_status.return_value = None
    audio_engine_mock.begin_automatic_source_beatmap_async.return_value = job
    maps.poll(frozenset())
    assert "e" * 32 in maps._pending
    job.is_cancelled.return_value = True
    audio_engine_mock.consume_automatic_source_beatmap.return_value = None
    maps.completed(0, {"id": 0, "_native_event_owner": object()})
    assert "e" * 32 not in maps._failed
    job.is_cancelled.return_value = False
    maps.poll(frozenset())
    assert audio_engine_mock.begin_automatic_source_beatmap_async.call_count == 2
    assert "e" * 32 in maps._pending


def test_saved_automatic_map_restores_without_second_neural_analysis(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
) -> None:
    selected = next_source_beatmap_intent(None, reference(), automatic=True)
    controller.project.pad_source_beatmap_intent[0] = selected
    job = Mock()
    job.is_cancelled.return_value = False
    job.source_beatmap_async_status.return_value = {"terminal": True, "stage": "ready"}
    audio_engine_mock.begin_automatic_source_beatmap_restore_async.return_value = job
    maps = controller.loader.source_beatmaps
    maps.poll(frozenset())
    maps.completed(0, {"_native_event_owner": object()})
    maps.poll(frozenset(), admit_restore=False)
    assert controller.project.pad_source_beatmap_intent[0] is selected
    assert maps.effective(0) is automatic.map.return_value
    audio_engine_mock.begin_automatic_source_beatmap_async.assert_not_called()


@pytest.mark.parametrize("parent", [False, True])
def test_sync_off_never_starts_unrequested_full_source_analysis(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
    *,
    parent: bool,
) -> None:
    controller.project.multi_loop = controller.project.bpm_lock = parent
    controller.project.global_timing_policy = GlobalTimingPolicy(
        schema_version=1,
        sync=False,
        realtime_bpm=True,
        revision=1,
    )
    controller.loader.source_beatmaps.poll(frozenset())
    audio_engine_mock.begin_automatic_source_beatmap_async.assert_not_called()


@pytest.mark.parametrize("saved", [None, False, True])
def test_saved_tracker_preference_is_preserved_but_wire_always_projects_single_sync(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
    *,
    saved: bool | None,
) -> None:
    controller.project.global_timing_policy = GlobalTimingPolicy(
        schema_version=1,
        sync=None,
        realtime_bpm=saved,
        revision=0,
    )
    before = controller.project.global_timing_policy
    assert controller.input_mapping._refresh_rust_runtime_state()
    wire = json.loads(
        audio_engine_mock.set_input_runtime_state.call_args.kwargs["timing_policy_json"]
    )
    assert wire == {"schema_version": 1, "sync": None, "realtime_bpm": True, "revision": 0}
    assert controller.project.global_timing_policy is before


@pytest.mark.parametrize(
    "error",
    ["optional map failed", "loader event content placement pending: export target changed"],
)
def test_task_optional_failure_settles_core_and_pending_retries_before_callbacks(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
    monkeypatch: pytest.MonkeyPatch,
    error: str,
) -> None:
    loader = controller.loader
    loader._analysis_request_ids[0] = 7
    controller.session.analyzing_sample_ids.add(0)
    callback = Mock()
    monkeypatch.setattr(loader, "_store_analysis_completion", callback)
    event = {
        "type": "task_success",
        "task": "analysis",
        "id": 0,
        "request_id": 7,
        "_native_event_owner": object(),
    }
    audio_engine_mock.consume_automatic_source_beatmap.side_effect = RuntimeError(error)
    loader._dispatch_loader_event(event, {"task_success": loader._handle_task_success})
    if error.startswith("loader event content placement pending"):
        callback.assert_not_called()
        assert 0 in controller.session.analyzing_sample_ids
        assert loader._pair_held_loader_events == [event]
        audio_engine_mock.consume_automatic_source_beatmap.side_effect = None
        loader._pair_held_loader_events.clear()
        loader._dispatch_loader_event(event, {"task_success": loader._handle_task_success})
    callback.assert_called_once()
    assert 0 not in controller.session.analyzing_sample_ids


def test_cold_map_placement_pending_retries_before_settling_source_or_timing(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    loader = controller.loader
    controller.session.pending_sample_paths[0] = "samples/shared.wav"
    controller.session.loading_sample_ids.add(0)
    before = controller.project.model_dump()
    finish = Mock()
    monkeypatch.setattr(loader, "_finish_loaded_timing", finish)
    event = {
        "type": "success",
        "id": 0,
        "cached_path": "samples/shared.wav",
        "duration_s": 1.0,
        "_native_event_owner": object(),
    }
    audio_engine_mock.consume_automatic_source_beatmap.side_effect = RuntimeError(
        "loader event content placement pending: export target changed"
    )
    loader._dispatch_loader_event(event, {"success": loader._handle_loader_success})
    assert controller.project.model_dump() == before
    assert controller.session.pending_sample_paths[0] == "samples/shared.wav"
    assert 0 in controller.session.loading_sample_ids
    assert loader._pair_held_loader_events == [event]
    finish.assert_not_called()
    audio_engine_mock.consume_automatic_source_beatmap.side_effect = None
    loader._pair_held_loader_events.clear()
    loader._dispatch_loader_event(event, {"success": loader._handle_loader_success})
    finish.assert_called_once()
    assert 0 not in controller.session.pending_sample_paths
    assert controller.project.pad_source_beatmap_intent[0].automatic is True


def test_inactive_third_pad_is_excluded_consistently_without_borrowing_phase_ack(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    controller.project.sample_paths[1] = "samples/other.wav"
    controller.project.pad_content[1] = PadContentIdentity(instance_id="d" * 32)
    active = SourceSyncStatus(
        requested=True, active=True, effective=True, state="effective", grid_phase_confirmed=True
    )
    unknown = SourceSyncStatus(
        requested=True, active=None, state="unconfirmed", guidance="unconfirmed"
    )
    monkeypatch.setattr(
        controller.transport.source_sync, "status", Mock(side_effect=[active, unknown])
    )
    audio_engine_mock.admitted_launch_ids.return_value = []
    audio_engine_mock._current_ordinary_transport_observation.return_value = SimpleNamespace(
        sample_id=1, active=False
    )
    status = controller.transport.timing_policy.status("sync")
    assert (status.effective, status.pending, status.detail, status.grid_phase_confirmed) == (
        True,
        False,
        None,
        True,
    )


@pytest.mark.parametrize(
    "case",
    [
        "stopped",
        "fenced",
        "pending-launch",
        "new-launch",
        "wrong-slot",
        "unknown",
        "error",
        "changed-content",
    ],
)
def test_stopped_participant_proof_keeps_fences_and_unknowns(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
    case: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    audio = audio_engine_mock
    audio.admitted_launch_ids.return_value = [0] if case == "pending-launch" else []
    audio._current_ordinary_transport_observation.return_value = (
        None
        if case == "unknown"
        else SimpleNamespace(sample_id=1 if case == "wrong-slot" else 0, active=False)
    )
    if case == "fenced":
        controller.transport._pair_deferred_sample_ids = frozenset({0})
    elif case == "error":
        audio._current_ordinary_transport_observation.side_effect = RuntimeError(
            "placement pending"
        )
    elif case == "changed-content":

        def change_content(_sample_id: int) -> SimpleNamespace:
            controller.project.pad_content[0] = PadContentIdentity(instance_id="d" * 32)
            return SimpleNamespace(sample_id=0, active=False)

        audio._current_ordinary_transport_observation.side_effect = change_content
    elif case == "new-launch":

        def admit_during_observation(_sample_id: int) -> SimpleNamespace:
            audio.admitted_launch_ids.return_value = [0]
            return SimpleNamespace(sample_id=0, active=False)

        audio._current_ordinary_transport_observation.side_effect = admit_during_observation
    assert controller.transport.timing_policy._proven_inactive(0) is (case == "stopped")


def test_only_stopped_pads_grant_no_global_effective_or_phase_confirmation(
    controller: AppController,
    automatic: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        controller.transport.source_sync,
        "status",
        Mock(
            return_value=SourceSyncStatus(
                requested=True, active=False, effective=False, state="stopped"
            )
        ),
    )
    status = controller.transport.timing_policy.status("sync")
    assert (status.effective, status.pending, status.detail, status.grid_phase_confirmed) == (
        None,
        False,
        None,
        None,
    )


def test_pending_launch_stopped_feedback_remains_phase_participant(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    controller.project.sample_paths[1] = "samples/future.wav"
    active = SourceSyncStatus(
        requested=True, active=True, effective=True, state="effective", grid_phase_confirmed=True
    )
    future = SourceSyncStatus(
        requested=True, active=False, effective=False, state="pending", guidance="pending"
    )
    monkeypatch.setattr(
        controller.transport.source_sync, "status", Mock(side_effect=[active, future])
    )
    audio_engine_mock.admitted_launch_ids.return_value = [1]
    status = controller.transport.timing_policy.status("sync")
    assert status.effective is None
    assert status.pending is True
    assert status.grid_phase_confirmed is None


def test_unknown_meter_loops_change_8_to_64_bars_and_start_at_loaded_rate(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic: Mock,
) -> None:
    geometry = _geometry(meter="unknown", source_rate=44100, scale=32.0)
    _bind_map(controller, geometry)
    audio_engine_mock.loaded_sample_shape.return_value = (48000, 2, 15360000)
    controller.project.multi_loop = controller.project.bpm_lock = True
    controller.project.global_timing_policy = GlobalTimingPolicy(
        schema_version=1,
        sync=True,
        realtime_bpm=False,
        revision=1,
    )
    controller.project.sample_durations[0] = 320.0
    controller.project.pad_loop_start_s[0] = 0.0
    loop = controller.transport.loop
    assert geometry.editor_info()["sample_rate_hz"] == 44100
    assert loop._source_map_snapshot(0)[1] == 48000
    loop.set_bars(0, bars=8.0)
    assert controller.project.pad_loop_end_s[0] == 16.0
    loop.set_bars(0, bars=64.0)
    assert controller.project.pad_loop_end_s[0] == 160.0
    loop.set_start(0, 1.0)
    assert controller.project.pad_loop_start_s[0] == 1.0
    assert loop.requested_region(0) == (1.0, 162.5)
    assert loop._max_source_map_bars(0) == 95.5
