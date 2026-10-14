"""Retired native timing events settle bookkeeping without replaying legacy timing."""

from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.models import SampleAnalysis
from flitzis_looper.stem_intent import PadStemIntent
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding, current_timing_metadata
from tests.flitzis_looper.controller.transport.test_key_lock_transactions import _mode_source

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.models import ProjectState


def _seed_musical_state_before_replacement(
    project: ProjectState, analysis: SampleAnalysis
) -> PadStemIntent:
    intent = PadStemIntent(enabled_mask=0, last_custom_mask=5, display_mode="custom")
    project.sample_analysis[0] = analysis
    project.pad_stem_intent[0] = intent
    project.pad_stem_intent[36] = intent
    return intent


def _assert_stem_intent_after_replacement(
    project: ProjectState, previous: PadStemIntent, *, restored: bool
) -> None:
    assert project.pad_stem_intent[0] == (previous if restored else PadStemIntent())
    assert project.pad_stem_intent[36] is previous


def _legacy_analysis() -> dict[str, object]:
    return {
        "bpm": 120.0,
        "key": "C#m",
        "beat_grid": {"beats": [0.0, 0.5], "downbeats": [0.0], "bars": [0.0]},
    }


def _schedule_maintenance_load(
    controller: AppController, audio: Mock, path: str, *, request_id: int
) -> None:
    """Exercise the existing saved/maintenance source admission contract."""
    audio.load_sample_async.return_value = request_id
    assert controller.loader._schedule_restored_load(0, Path(path), run_analysis=False)
    call = audio.load_sample_async.call_args
    assert call.args == (0, Path(path).as_posix())
    assert call.kwargs["source_intent"] == "restore"
    assert call.kwargs["replace_assignment"] is True
    assert call.kwargs["run_analysis"] is False


def test_timing_stale_maintenance_replacement_retires_old_geometry_before_new_mode_failure(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old_ticket, feedback = _mode_source(controller, audio_engine_mock)
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    controller.project.key_lock = True
    controller.project.pad_loop_end_s[0] = 7.0
    _schedule_maintenance_load(controller, audio_engine_mock, "samples/new.wav", request_id=9)
    audio_engine_mock.current_input_runtime_pad_binding.return_value = FakeInputRuntimePadBinding(
        accepted_timing={"source_id": "new-source", "source_generation": 2}
    )
    audio_engine_mock.loaded_residency.return_value = {"source_identity": 456}
    feedback.update(source_id="new-source", source_generation=2, source_identity=456)
    audio_engine_mock.prepare_resident_control.side_effect = RuntimeError("new mode unavailable")
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 9,
            "duration_s": 12.0,
            "cached_path": "samples/new.wav",
            "timing_stale": True,
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert old_ticket.status == "cancelled"
    assert controller.project.sample_paths[0] == "samples/new.wav"
    assert (controller.project.pad_loop_start_s[0], controller.project.pad_loop_end_s[0]) == (
        3.0,
        7.0,
    )
    assert controller.project.pad_key_lock[0] is None
    state = controller.transport.residency.key_lock_status(0)
    assert state.requested is True
    assert state.effective is False
    assert state.error == "new mode unavailable"


def test_failed_cold_load_settles_superseded_analysis_without_changing_effective_source(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    project = controller.project
    session = controller.session
    project.sample_paths[0] = "samples/old.wav"
    project.sample_durations[0] = 32.0
    project.sample_analysis[0] = SampleAnalysis.model_validate(_legacy_analysis())
    project.manual_bpm[0] = 123.456
    project.pad_gain_db[0] = -6.0
    session.active_sample_ids.add(0)
    session.pad_playhead_s[0] = 4.25
    audio_engine_mock.analyze_sample_async.return_value = 3
    controller.loader.analyze_sample_async(0)
    session.sample_analysis_progress[0] = 0.5
    old_project = project.model_dump()
    audio_engine_mock.load_incoming_sample_async.return_value = 4
    controller.loader.load_sample_async(0, "new.wav")
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "error", "id": 0, "request_id": 4, "msg": "cold decode failed"},
        {
            "type": "task_error",
            "id": 0,
            "request_id": 3,
            "task": "analysis",
            "timing_stale": True,
            "msg": "Analysis superseded by another pad request",
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert project.model_dump() == old_project
    assert session.active_sample_ids == {0}
    assert session.pad_playhead_s[0] == 4.25
    assert session.analyzing_sample_ids == set()
    assert 0 not in session.sample_analysis_progress
    assert 0 not in controller.loader._analysis_request_ids
    assert session.sample_analysis_errors[0] == "Analysis superseded by another pad request"
    assert session.sample_load_errors[0] == "cold decode failed"
    audio_engine_mock.unload_sample.assert_not_called()


@pytest.mark.parametrize("restored", [False, True])
@pytest.mark.parametrize("include_analysis", [False, True])
@pytest.mark.parametrize("global_default", [False, True])
def test_maintenance_source_completion_preserves_timing_and_gates_analysis_until_loaded(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    *,
    restored: bool,
    include_analysis: bool,
    global_default: bool,
) -> None:
    project, session = controller.project, controller.session
    cached_path = "samples/current.wav"
    project.sample_paths[0] = cached_path if restored else "samples/predecessor.wav"
    project.key_lock = global_default
    project.pad_key_lock[0] = not global_default
    old_analysis = SampleAnalysis.model_validate(_legacy_analysis())
    old_stem_intent = _seed_musical_state_before_replacement(project, old_analysis)
    project.manual_bpm[0] = 123.456
    project.pad_grid_anchor_s[0] = -0.125
    project.pad_loop_auto[0] = False
    project.pad_loop_start_s[0] = 3.25
    project.pad_loop_end_s[0] = 5.75
    current_source = (7, "b" * 64, 48_000 * 32, 48_000)
    audio_engine_mock.waveform_source_identity.return_value = current_source
    audio_engine_mock.load_sample_async.return_value = 7
    if restored:
        cached_file = tmp_path / cached_path
        cached_file.parent.mkdir()
        write_mono_pcm16_wav(cached_file, 48_000)
        controller.loader.restore_samples_from_project_state()
    else:
        _schedule_maintenance_load(controller, audio_engine_mock, cached_path, request_id=7)
    assert controller.loader._load_request_ids[0] == 7
    assert 0 in session.loading_sample_ids
    audio_engine_mock.analyze_sample_async.return_value = 9
    controller.loader.analyze_sample_async(0)
    audio_engine_mock.analyze_sample_async.assert_not_called()
    assert 0 not in controller.loader._analysis_request_ids
    audio_engine_mock.reset_mock()
    completed_load = {
        "type": "success",
        "id": 0,
        "request_id": 7,
        "duration_s": 32.0,
        "cached_path": cached_path,
        "detected_loop_start_s": 1.0,
        "timing_stale": True,
        "analysis": _legacy_analysis() if include_analysis else None,
    }
    audio_engine_mock.poll_loader_events.side_effect = [
        completed_load,
        None,
    ]

    controller.loader.poll_loader_events()

    assert (project.sample_paths[0], project.sample_durations[0]) == (cached_path, 32.0)
    _assert_stem_intent_after_replacement(project, old_stem_intent, restored=restored)
    assert (
        session.loading_sample_ids,
        session.pending_sample_paths,
        controller.loader._load_request_ids,
    ) == (set(), {}, {})
    if restored:
        assert (
            project.sample_analysis[0] is old_analysis,
            project.pad_key_lock[0],
            project.resolve_pad_key_lock(0),
        ) == (True, not global_default, not global_default)
    else:
        assert (
            project.sample_analysis[0],
            project.pad_key_lock[0],
            project.resolve_pad_key_lock(0),
        ) == (None, None, global_default)
    requested_key_lock = not global_default if restored else global_default
    audio_engine_mock.set_pad_key_lock.assert_called_with(0, requested_key_lock)
    assert (
        project.manual_bpm[0],
        project.pad_grid_anchor_s[0],
        project.pad_loop_auto[0],
        project.pad_loop_start_s[0],
        project.pad_loop_end_s[0],
    ) == (123.456, -0.125, False, 3.25, 5.75)
    assert (0 in controller.loader._analysis_request_ids, 0 in session.analyzing_sample_ids) == (
        False,
        False,
    )
    controller.loader.analyze_sample_async(0)
    audio_engine_mock.analyze_sample_async.assert_called_once_with(0)
    session.sample_analysis_progress[0] = 0.5
    # Delayed feedback for the completed load cannot retire the later public admission.
    audio_engine_mock.poll_loader_events.side_effect = [completed_load, None]
    controller.loader.poll_loader_events()
    assert (
        controller.loader._analysis_request_ids[0],
        0 in session.analyzing_sample_ids,
        session.sample_analysis_progress[0],
    ) == (9, True, 0.5)
    for publisher in (
        audio_engine_mock.set_pad_bpm,
        audio_engine_mock.set_pad_timing_metadata,
        audio_engine_mock.set_pad_loop_region,
    ):
        publisher.assert_not_called()


def test_retired_analysis_completion_settles_its_task_without_legacy_publication(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old_analysis = SampleAnalysis.model_validate(_legacy_analysis())
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.sample_analysis[0] = old_analysis
    controller.loader._analysis_request_ids[0] = 7
    controller.session.analyzing_sample_ids.add(0)
    controller.session.sample_analysis_progress[0] = 0.5
    audio_engine_mock.reset_mock()
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "task_success",
            "id": 0,
            "request_id": 7,
            "task": "analysis",
            "timing_stale": True,
            "analysis": _legacy_analysis(),
        },
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_analysis[0] is old_analysis
    assert 0 not in controller.session.analyzing_sample_ids
    assert 0 not in controller.session.sample_analysis_progress
    assert 0 not in controller.loader._analysis_request_ids
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()


@pytest.fixture
def automatic_pad(controller: AppController, audio_engine_mock: Mock) -> dict[str, object]:
    """Model the native current resolver and authority edits without a device stream."""
    controller.project.sample_paths[0] = "C:\\fixture\\accepted.wav"
    controller.project.sample_durations[0] = 600.0
    controller.project.sample_analysis[0] = SampleAnalysis.model_validate(_legacy_analysis())
    metadata = current_timing_metadata(period=0.2245048325556449)
    audio_engine_mock.current_constant_timing.return_value = metadata
    audio_engine_mock.pad_timing_intent.return_value = "automatic"

    def legacy_edit(*_args: object) -> None:
        audio_engine_mock.current_constant_timing.return_value = None
        audio_engine_mock.pad_timing_intent.return_value = "legacy"

    audio_engine_mock.set_pad_bpm.side_effect = legacy_edit
    audio_engine_mock.set_pad_timing_metadata.side_effect = legacy_edit
    return metadata


@pytest.mark.parametrize("failure", ["admission", "worker", "stale-callback"])
def test_failed_cold_replacement_retains_acknowledged_automatic_authority(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    failure: str,
) -> None:
    previous = controller.project.model_dump()
    previous_timing = controller.transport.bpm.current_timing(0)
    assert previous_timing is not None
    audio_engine_mock.reset_mock()
    if failure == "admission":
        audio_engine_mock.load_incoming_sample_async.side_effect = RuntimeError(
            "bounded source queue full"
        )
        controller.loader.load_sample_async(0, "new.wav")
    else:
        audio_engine_mock.load_incoming_sample_async.return_value = 7
        controller.loader.load_sample_async(0, "new.wav")
        audio_engine_mock.poll_loader_events.side_effect = [
            {"type": "error", "id": 0, "request_id": 7, "msg": failure},
            None,
        ]
        controller.loader.poll_loader_events()

    audio_engine_mock.load_incoming_sample_async.assert_called_once_with(
        0, "new.wav", controller.project.key_lock
    )
    assert controller.session.sample_load_errors[0] == (
        "bounded source queue full" if failure == "admission" else failure
    )
    assert controller.project.model_dump() == previous
    assert controller.transport.bpm.current_timing(0) == previous_timing
    assert audio_engine_mock.current_constant_timing.return_value is automatic_pad
    assert audio_engine_mock.pad_timing_intent(0) == "automatic"
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    audio_engine_mock.set_pad_timing_intent.assert_not_called()
    audio_engine_mock.unload_sample.assert_not_called()


def test_accepted_unload_then_maintenance_reload_restores_legacy_timing(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    assert controller.transport.bpm.current_timing(0) is not None
    controller.loader.unload_sample(0)
    audio_engine_mock.set_pad_bpm.assert_called_with(0, None)
    assert audio_engine_mock.pad_timing_intent(0) == "legacy"
    assert controller.transport.bpm.current_timing(0) is None

    _schedule_maintenance_load(controller, audio_engine_mock, "samples/ordinary.wav", request_id=7)
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 7,
            "duration_s": 32.0,
            "cached_path": "samples/ordinary.wav",
            "analysis": _legacy_analysis(),
        },
        None,
    ]
    controller.loader.poll_loader_events()
    assert controller.project.sample_paths[0] == "samples/ordinary.wav"
    assert controller.transport.bpm.effective_bpm(0) == 120.0
    audio_engine_mock.set_pad_bpm.assert_called_with(0, 120.0)
    assert audio_engine_mock.pad_timing_intent(0) == "legacy"


def test_successful_restored_load_can_leave_unacknowledged_automatic_intent(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    controller.project.sample_paths[0] = "samples/restored.wav"
    controller.session.pending_sample_paths[0] = "samples/restored.wav"
    controller.session.loading_sample_ids.add(0)
    controller.loader._load_request_ids[0] = 7
    audio_engine_mock.current_constant_timing.return_value = None
    audio_engine_mock.reset_mock()
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "success",
            "id": 0,
            "request_id": 7,
            "duration_s": 32.0,
            "cached_path": "samples/restored.wav",
        },
        None,
    ]
    controller.loader.poll_loader_events()
    assert audio_engine_mock.pad_timing_intent(0) == "legacy"
    assert controller.transport.bpm.effective_bpm(0) == 120.0
    audio_engine_mock.set_pad_bpm.assert_called_with(0, 120.0)


def test_pending_ordinary_analysis_preserves_current_until_valid_completion(
    controller: AppController, audio_engine_mock: Mock, automatic_pad: dict[str, object]
) -> None:
    audio_engine_mock.analyze_sample_async.return_value = 9
    controller.loader.analyze_sample_async(0)
    timing = controller.transport.bpm.current_timing(0)
    assert timing is not None
    assert timing.accepted_revision == automatic_pad["revision"]
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": "task_success",
            "id": 0,
            "request_id": 9,
            "task": "analysis",
            "analysis": _legacy_analysis(),
        },
        None,
    ]
    controller.loader.poll_loader_events()
    assert controller.transport.bpm.effective_bpm(0) == 120.0
    assert audio_engine_mock.pad_timing_intent(0) == "legacy"
    audio_engine_mock.set_pad_bpm.assert_called_with(0, 120.0)


@pytest.mark.parametrize("event_type", ["success", "task_success"])
def test_retired_source_or_analysis_completion_cannot_clear_current_automatic(
    controller: AppController,
    audio_engine_mock: Mock,
    automatic_pad: dict[str, object],
    event_type: str,
) -> None:
    controller.loader._load_request_ids[0] = 7
    controller.loader._analysis_request_ids[0] = 7
    controller.session.loading_sample_ids.add(0)
    controller.session.analyzing_sample_ids.add(0)
    audio_engine_mock.poll_loader_events.side_effect = [
        {
            "type": event_type,
            "id": 0,
            "request_id": 7,
            "task": "analysis",
            "duration_s": 32.0,
            "cached_path": "samples/current.wav",
            "timing_stale": True,
            "analysis": _legacy_analysis(),
        },
        None,
    ]
    controller.loader.poll_loader_events()
    timing = controller.transport.bpm.current_timing(0)
    assert timing is not None
    assert timing.accepted_revision == automatic_pad["revision"]
    assert audio_engine_mock.pad_timing_intent(0) == "automatic"
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
