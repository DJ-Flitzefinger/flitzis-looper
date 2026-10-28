"""Saved controls settle before cold requests capture immutable startup authority.

These controller tests use a mocked engine. Genuine epoch cancellation and callback
acknowledgement are exercised by the separate device-free Native cold-worker tests.
"""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.input_mapping import KeyboardBinding, LooperAction
from flitzis_looper.models import BeatGrid, SampleAnalysis
from flitzis_looper.ui.context import UiContext

if TYPE_CHECKING:
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.models import TimingIntent


_STARTUP_CONTROLS = {
    "set_volume",
    "set_speed",
    "set_bpm_lock",
    "set_pad_key_lock",
    "set_pad_gain",
    "set_pad_eq",
    "set_pad_loop_region",
    "set_pad_bpm",
    "set_pad_timing_metadata",
    "set_pad_timing_intent",
    "set_master_bpm",
    "set_master_period",
}


def test_saved_scalar_loop_restores_with_its_full_requested_bars(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    relative = _configure_saved_source(controller, audio_engine_mock, tmp_path, "manual")
    project = controller.project
    project.multi_loop = True
    project.pad_loop_auto[0] = True
    project.pad_loop_bars[0] = 32.0
    before = (
        project.pad_loop_start_s[0],
        project.pad_loop_end_s[0],
        project.pad_loop_bars[0],
        project.pad_grid_offset_samples[0],
    )
    audio_engine_mock.load_sample_async.return_value = 37

    controller._restore_initial_runtime_state()

    assert audio_engine_mock.load_sample_async.call_args.args == (0, relative)
    assert audio_engine_mock.load_sample_async.call_args.kwargs["resident_loop_end_s"] == 65.0
    assert before == (
        project.pad_loop_start_s[0],
        project.pad_loop_end_s[0],
        project.pad_loop_bars[0],
        project.pad_grid_offset_samples[0],
    )
    assert project.pad_loop_auto[0] is True


def _configure_saved_source(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    intent: TimingIntent,
) -> str:
    samples = tmp_path / "samples"
    samples.mkdir(exist_ok=True)
    relative = "samples/restart-source.wav"
    (tmp_path / relative).write_bytes(b"owned controller source; no decoding or device")
    project = controller.project
    project.sample_paths[0] = relative
    project.sample_analysis[0] = SampleAnalysis(
        bpm=128.0,
        key="C",
        beat_grid=BeatGrid(beats=[0.0, 0.46875], downbeats=[0.0], bars=[0.0]),
    )
    project.manual_bpm[0] = 120.0 if intent in {"manual", "tap"} else None
    project.pad_timing_intent[0] = intent
    project.pad_grid_offset_samples[0] = 137
    project.pad_grid_anchor_s[0] = 0.25
    project.pad_loop_auto[0] = False
    project.pad_loop_start_s[0] = 1.0
    project.pad_loop_end_s[0] = 4.0
    project.key_lock = True
    project.pad_key_lock[0] = None
    project.pad_key_intent[0] = project.pad_key_intent[0].changed(base_shift=3, extra_shift=-2)
    project.pad_gain_db[0] = -6.0
    project.pad_eq_low_db[0] = -3.0
    project.volume = 0.75
    project.speed = 1.125
    project.bpm_lock = True
    audio_engine_mock.loaded_residency.side_effect = ValueError("sample is not loaded")
    audio_engine_mock.reset_mock()
    return relative


@pytest.mark.parametrize("intent", ["legacy", "manual", "tap", "automatic"])
def test_saved_controls_finish_before_original_restore_admission(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    intent: TimingIntent,
) -> None:
    relative = _configure_saved_source(controller, audio_engine_mock, tmp_path, intent)
    admitted: list[dict[str, object]] = []

    def admit(sample_id: int, path: str, **options: object) -> int:
        assert sample_id == 0
        assert path == relative
        # This is the actual asynchronous admission boundary, so the worker may
        # run immediately after it returns. No startup setter may follow it.
        audio_engine_mock.set_pad_gain.assert_called_once_with(0, -6.0)
        audio_engine_mock.set_pad_eq.assert_called_once_with(0, -3.0, 0.0, 0.0)
        assert audio_engine_mock.set_pad_key_lock.called
        if intent == "automatic":
            audio_engine_mock.set_pad_loop_region.assert_not_called()
            audio_engine_mock.set_pad_bpm.assert_not_called()
            audio_engine_mock.set_pad_timing_metadata.assert_not_called()
            audio_engine_mock.set_pad_timing_intent.assert_not_called()
            assert options["restore_automatic"] is True
        else:
            assert audio_engine_mock.set_pad_loop_region.called
            assert audio_engine_mock.set_pad_bpm.called
            assert audio_engine_mock.set_pad_timing_metadata.called
            assert "restore_automatic" not in options
            if intent in {"manual", "tap"}:
                audio_engine_mock.set_pad_timing_intent.assert_called_once_with(0, intent)
        admitted.append(options)
        return 27

    audio_engine_mock.load_sample_async.side_effect = admit

    controller._restore_initial_runtime_state()

    assert len(admitted) == 1
    (options,) = admitted
    assert options["source_intent"] == "restore"
    assert options["initial_base_shift"] == 3
    assert options["initial_extra_shift"] == -2
    assert options["initial_key_lock"] is True
    assert options["initial_gain_db"] == -6.0
    assert options["initial_eq_db"] == (-3.0, 0.0, 0.0)
    assert options["resident_loop_start_s"] == 1.0
    assert options["resident_loop_end_s"] == 4.0
    assert options["resident_key_lock"] is True
    calls = [item[0] for item in audio_engine_mock.mock_calls]
    admission = calls.index("load_sample_async")
    assert not _STARTUP_CONTROLS.intersection(calls[admission + 1 :])
    assert controller.loader._load_request_ids[0] == 27
    assert controller.session.loading_sample_ids == {0}
    assert controller.project.pad_timing_intent[0] == intent
    assert controller.project.pad_key_intent[0].base_shift == 3
    assert controller.project.pad_key_intent[0].extra_shift == -2
    audio_engine_mock.capture_saved_constant_timing.assert_not_called()
    audio_engine_mock.publish_pitch_action.assert_not_called()


def test_startup_still_clears_only_the_missing_saved_assignment(
    controller: AppController,
    audio_engine_mock: Mock,
) -> None:
    project = controller.project
    project.sample_paths[0] = "samples/missing-startup-source.wav"
    project.manual_bpm[0] = 120.0
    project.pad_timing_intent[0] = "manual"
    project.pad_loop_auto[0] = False
    project.pad_loop_start_s[0] = 1.0
    project.pad_loop_end_s[0] = 4.0
    audio_engine_mock.loaded_residency.side_effect = ValueError("sample is not loaded")
    audio_engine_mock.reset_mock()

    controller._restore_initial_runtime_state()

    assert project.sample_paths[0] is None
    assert project.manual_bpm[0] is None
    assert project.pad_timing_intent[0] == "legacy"
    assert not controller.session.loading_sample_ids
    audio_engine_mock.load_sample_async.assert_not_called()


def test_saved_auto_loop_clamps_before_a_restored_residency_guard_exists(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
) -> None:
    relative = _configure_saved_source(controller, audio_engine_mock, tmp_path, "manual")
    project = controller.project
    project.pad_loop_auto[0] = True
    project.pad_loop_bars[0] = 4.0
    project.sample_durations[0] = 3.0

    def admit(sample_id: int, path: str, **options: object) -> int:
        assert sample_id == 0
        assert path == relative
        # The complete Native source later owns physical EOF clamping. Before
        # that read, saved demand can exceed the historical physical duration.
        assert options["resident_loop_end_s"] == 9.0
        assert audio_engine_mock.set_pad_loop_region.call_args.args == (0, 1.0, 3.0)
        return 29

    audio_engine_mock.load_sample_async.side_effect = admit

    controller._restore_initial_runtime_state()

    calls = [item[0] for item in audio_engine_mock.mock_calls]
    admission = calls.index("load_sample_async")
    assert "set_pad_loop_region" not in calls[admission + 1 :]
    assert controller.loader._load_request_ids[0] == 29


def test_restore_explicitly_carries_neutral_mixing_in_its_original_load(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
) -> None:
    _configure_saved_source(controller, audio_engine_mock, tmp_path, "legacy")
    project = controller.project
    project.pad_gain_db[0] = 0.0
    project.pad_eq_low_db[0] = 0.0
    audio_engine_mock.load_sample_async.return_value = 31
    audio_engine_mock.load_sample_async.side_effect = None

    controller._restore_initial_runtime_state()

    options = audio_engine_mock.load_sample_async.call_args.kwargs
    assert options["initial_gain_db"] == 0.0
    assert options["initial_eq_db"] == (0.0, 0.0, 0.0)
    audio_engine_mock.set_pad_gain.assert_not_called()
    audio_engine_mock.set_pad_eq.assert_not_called()


def test_path_validation_neutralizes_missing_before_any_valid_restore_is_admitted(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
) -> None:
    _configure_saved_source(controller, audio_engine_mock, tmp_path, "manual")
    controller.project.sample_paths[1] = "samples/missing-before-admission.wav"
    controller.project.manual_bpm[1] = 99.0
    controller.project.pad_gain_db[1] = -9.0

    paths = controller.loader._validate_restored_sample_paths()

    assert list(paths) == [0]
    assert controller.project.sample_paths[1] is None
    assert controller.project.manual_bpm[1] is None
    assert controller.project.pad_gain_db[1] == 0.0
    assert not controller.session.loading_sample_ids
    audio_engine_mock.load_sample_async.assert_not_called()
    controller.transport.apply_project_state_to_audio(restored_sample_ids=frozenset(paths))
    audio_engine_mock.load_sample_async.side_effect = None
    audio_engine_mock.load_sample_async.return_value = 37

    controller.loader.restore_samples_from_project_state(validated_paths=paths)

    audio_engine_mock.load_sample_async.assert_called_once()
    assert controller.loader._load_request_ids == {0: 37}
    assert controller.session.loading_sample_ids == {0}


@pytest.mark.parametrize("gesture", ["gain", "eq", "key_lock", "loop", "timing"])
def test_pending_original_restore_rejects_current_source_edits_before_durable_mutation(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    gesture: str,
) -> None:
    _configure_saved_source(controller, audio_engine_mock, tmp_path, "manual")
    audio_engine_mock.load_sample_async.side_effect = None
    audio_engine_mock.load_sample_async.return_value = 41
    controller.loader.restore_samples_from_project_state()
    assert controller.loader._load_request_ids[0] == 41
    before = controller.project.model_dump()
    audio_engine_mock.reset_mock()
    gestures = {
        "gain": lambda: controller.transport.pad.set_pad_gain(0, -12.0),
        "eq": lambda: controller.transport.pad.set_pad_eq(0, -12.0, -11.0, -10.0),
        "key_lock": lambda: controller.transport.pad.set_pad_key_lock(0, enabled=False),
        "loop": lambda: controller.transport.loop.set_start(0, 2.0),
        "timing": lambda: controller.transport.bpm.set_manual_bpm(0, 99.0),
    }

    with pytest.raises(ValueError, match="Source assignment receipt is pending"):
        gestures[gesture]()

    assert controller.project.model_dump() == before
    assert controller.loader._load_request_ids[0] == 41
    assert controller.session.loading_sample_ids == {0}
    for method in (
        "set_pad_gain",
        "set_pad_eq",
        "set_pad_key_lock",
        "set_pad_loop_region",
        "set_pad_bpm",
        "set_pad_timing_metadata",
        "set_pad_timing_intent",
    ):
        getattr(audio_engine_mock, method).assert_not_called()


@pytest.mark.parametrize("gesture", ["gain", "eq"])
@pytest.mark.parametrize("route", ["gui", "keyboard"])
def test_pending_restore_mixing_reports_refusal_without_replay_then_accepts_fresh_gesture(
    controller: AppController,
    audio_engine_mock: Mock,
    tmp_path: Path,
    gesture: str,
    route: str,
) -> None:
    _configure_saved_source(controller, audio_engine_mock, tmp_path, "legacy")
    audio_engine_mock.load_sample_async.side_effect = None
    audio_engine_mock.load_sample_async.return_value = 43
    controller.loader.restore_samples_from_project_state()
    before = controller.project.model_dump()
    ctx = UiContext(controller)
    if route == "keyboard":
        action = "pad.gain_db:0:-120" if gesture == "gain" else "pad.eq:0:low:-12"
        controller.input_mapping.save_mapping(
            "keyboard", KeyboardBinding(key_name="K").key, LooperAction.from_key(action)
        )
        before = controller.project.model_dump()

    def execute() -> None:
        if route == "keyboard":
            ctx.input.capture_keyboard("K", received_at_ns=17)
        elif gesture == "gain":
            ctx.audio.pads.set_pad_gain(0, -12.0)
        else:
            ctx.audio.pads.set_pad_eq_band(0, "low", -12.0)

    audio_engine_mock.reset_mock()
    execute()

    error = (
        controller.session.input_mapping_error
        if route == "keyboard"
        else ctx.audio.pads.residency_status(0)[1]
    )
    assert "Source assignment receipt is pending" in (error or "")
    assert controller.project.model_dump() == before
    controller.transport.on_frame_render()
    controller.input_mapping.on_frame_render()
    audio_engine_mock.set_pad_gain.assert_not_called()
    audio_engine_mock.set_pad_eq.assert_not_called()
    controller.loader._settle_load_request(0)

    execute()

    if gesture == "gain":
        assert controller.project.pad_gain_db[0] == -12.0
        audio_engine_mock.set_pad_gain.assert_called_once_with(0, -12.0)
    else:
        assert controller.project.pad_eq_low_db[0] == -12.0
        audio_engine_mock.set_pad_eq.assert_called_once_with(0, -12.0, 0.0, 0.0)
