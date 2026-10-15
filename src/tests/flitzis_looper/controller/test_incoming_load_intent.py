from typing import TYPE_CHECKING
from unittest.mock import Mock, call, patch

import pytest

from flitzis_looper.models import BeatGrid, ProjectState, SampleAnalysis
from tests.flitzis_looper.conftest import (
    FakeCapturedGlobalStart,
    FakeCapturedPadStart,
    FakeInputRuntimePadBinding,
)

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def _success(*, revision: int = 0, key_lock: bool = False) -> dict[str, object]:
    return {
        "type": "success",
        "id": 0,
        "request_id": 7,
        "cached_path": "samples/incoming.wav",
        "duration_s": 600.0,
        "analysis": {
            "bpm": 120.0,
            "key": "C",
            "beat_grid": {"beats": [], "downbeats": [], "bars": []},
        },
        "incoming_defaults": {
            "revision": revision,
            "key_lock": key_lock,
            "start_s": 123.25,
            "end_s": 139.25,
            "period_seconds": 0.5,
            "grid_origin_s": 123.25,
            "loop_auto": True,
            "loop_bars": 8.0,
        },
    }


def _admit(controller: AppController, audio: Mock) -> None:
    audio.load_incoming_sample_async.return_value = 7
    controller.loader.load_sample_async(0, "incoming.wav")


def _generation_binding(generation: int, *, sample_id: int = 0, current: bool = True) -> Mock:
    metadata = FakeInputRuntimePadBinding(sample_id).metadata()
    metadata.update(source_generation=generation, source_id=f"loaded-{sample_id}-{generation}")
    return Mock(
        spec=["is_current", "metadata"],
        is_current=Mock(return_value=current),
        metadata=Mock(return_value=metadata),
    )


@pytest.mark.parametrize("global_mode", [False, True])
@pytest.mark.parametrize("old_override", [None, False, True])
def test_incoming_assignment_uses_same_resolver_without_transferring_old_override(
    controller: AppController,
    audio_engine_mock: Mock,
    *,
    global_mode: bool,
    old_override: bool | None,
) -> None:
    project = controller.project
    project.key_lock = global_mode
    project.sample_paths[0] = "samples/old.wav"
    project.pad_key_lock[0] = old_override
    previous = project.model_dump()

    _admit(controller, audio_engine_mock)

    audio_engine_mock.load_incoming_sample_async.assert_called_once_with(
        0, "incoming.wav", global_mode
    )
    audio_engine_mock.load_sample_async.assert_not_called()
    assert project.model_dump() == previous


def test_empty_incoming_assignment_projects_global_on_without_marking_current_pad_loaded() -> None:
    project = ProjectState(key_lock=True)
    assert not project.resolve_pad_key_lock(215)
    assert project.resolve_pad_key_lock(215, assignment_override=None)
    assert not project.resolve_pad_key_lock(215, assignment_override=False)
    assert project.sample_paths[215] is None


@pytest.mark.parametrize("previous_override", ["empty", None, False, True])
def test_global_refresh_updates_incoming_request_before_draining_events(
    controller: AppController, audio_engine_mock: Mock, *, previous_override: bool | str | None
) -> None:
    if previous_override != "empty":
        controller.project.sample_paths[0] = "samples/old.wav"
        assert previous_override is None or isinstance(previous_override, bool)
        controller.project.pad_key_lock[0] = previous_override
    _admit(controller, audio_engine_mock)
    controller.transport.global_params.set_key_lock(enabled=True)
    assert audio_engine_mock.update_incoming_load_intent.call_args.args == (0, 7, 1, True)
    audio_engine_mock.poll_loader_events.return_value = None

    controller.loader.poll_loader_events()

    update = audio_engine_mock.update_incoming_load_intent.call_args
    assert update.args == (0, 7, 1, True)
    assert update.kwargs["loop_start_s"] is None
    assert update.kwargs["loop_auto"] is True
    assert update.kwargs["timing_override"] is False
    if previous_override == "empty":
        assert controller.project.sample_paths[0] is None


def test_adopted_signed_incoming_origin_roundtrips_durable_base_and_offset(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.output_sample_rate.return_value = 48_000
    _admit(controller, audio_engine_mock)
    controller.loader.set_incoming_timing_intent(0, 7, period_seconds=0.75, grid_origin_s=-0.125)
    event = _success(revision=1)
    defaults = event["incoming_defaults"]
    assert isinstance(defaults, dict)
    defaults.update(period_seconds=0.75, grid_origin_s=-0.125)
    audio_engine_mock.poll_loader_events.side_effect = [event, None]

    controller.loader.poll_loader_events()

    assert controller.project.pad_grid_anchor_s[0] == 0.0
    assert controller.project.pad_grid_offset_samples[0] == -6_000
    assert controller.transport.loop.grid_anchor_sec(0) == -0.125
    assert controller.project.manual_bpm[0] == 80.0
    restored = ProjectState.model_validate_json(controller.project.model_dump_json())
    assert restored.pad_grid_offset_samples[0] == -6_000
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_explicit_incoming_origin_is_request_bound_and_preserves_predecessor(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.pad_key_lock[0] = False
    _admit(controller, audio_engine_mock)
    controller.loader.set_incoming_key_lock_override(0, 7, override=True)
    controller.loader.set_incoming_key_lock_override(0, 7, override=None)
    controller.loader.set_incoming_key_lock_override(0, 7, override=False)
    controller.transport.global_params.set_key_lock(enabled=True)
    audio_engine_mock.poll_loader_events.return_value = None

    controller.loader.poll_loader_events()

    assert [
        call.args[2] for call in audio_engine_mock.update_incoming_load_intent.call_args_list
    ] == [1, 2, 3]
    assert controller.project.pad_key_lock[0] is False
    assert controller.project.sample_paths[0] == "samples/old.wav"
    with pytest.raises(ValueError, match="no longer current"):
        controller.loader.set_incoming_key_lock_override(0, 6, override=True)
    assert audio_engine_mock.update_incoming_load_intent.call_count == 3


def test_own_receipt_projects_absolute_defaults_without_republishing_them(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.pad_key_lock[0] = True
    _admit(controller, audio_engine_mock)
    audio_engine_mock.reset_mock()
    audio_engine_mock.poll_loader_events.side_effect = [_success(), None]
    with patch.object(controller.stems, "reuse_compatible_stems_on_load") as reuse:
        controller.loader.poll_loader_events()

    project = controller.project
    assert project.sample_paths[0] == "samples/incoming.wav"
    assert project.pad_key_lock[0] is None
    assert project.pad_loop_start_s[0] == 123.25
    assert project.pad_loop_end_s[0] == 139.25
    assert project.pad_loop_auto[0]
    assert project.pad_loop_bars[0] == 8.0
    assert project.pad_grid_anchor_s[0] == 123.25
    assert project.sample_analysis[0] is not None
    assert project.sample_analysis[0].bpm == 120.0
    for method in (
        "set_pad_bpm",
        "set_pad_loop_region",
        "set_pad_timing_metadata",
        "set_pad_key_lock",
    ):
        getattr(audio_engine_mock, method).assert_not_called()
    reuse.assert_called_once_with(0)
    assert controller.loader.incoming_load_request_id(0) is None


def test_later_incoming_override_retains_requested_origin_after_older_adoption_receipt(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _admit(controller, audio_engine_mock)
    audio_engine_mock.update_incoming_load_intent.return_value = False
    controller.loader.set_incoming_key_lock_override(0, 7, override=True)
    audio_engine_mock.poll_loader_events.side_effect = [_success(), None]

    controller.loader.poll_loader_events()

    assert controller.project.pad_key_lock[0] is True
    assert controller.project.pad_loop_start_s[0] == 123.25
    requested = True
    audio_engine_mock.set_pad_key_lock.assert_called_with(0, requested)


@pytest.mark.parametrize("old_gesture", [False, True])
def test_generic_predecessor_pad_gesture_never_becomes_incoming_override(
    controller: AppController, audio_engine_mock: Mock, *, old_gesture: bool
) -> None:
    controller.project.key_lock = True
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.pad_key_lock[0] = False
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = _generation_binding(1)
    _admit(controller, audio_engine_mock)
    with pytest.raises(ValueError, match="receipt is pending"):
        controller.transport.pad.set_pad_key_lock(0, enabled=old_gesture)
    audio_engine_mock.set_pad_key_lock.assert_not_called()
    incoming = controller.loader._incoming_loads[0]
    assert "key_lock" not in incoming.current_source_revisions
    assert incoming.intent.override is None
    audio_engine_mock.poll_loader_events.side_effect = [_success(key_lock=True), None]

    controller.loader.poll_loader_events()

    assert controller.project.pad_key_lock[0] is None
    assert controller.project.resolve_pad_key_lock(0)


def test_generic_gesture_waits_for_own_receipt_then_uses_own_native_generation(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.key_lock = True
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.pad_key_lock[0] = False
    _admit(controller, audio_engine_mock)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = _generation_binding(7)
    with pytest.raises(ValueError, match="receipt is pending"):
        controller.transport.pad.set_pad_key_lock(0, enabled=False)
    assert controller.loader._incoming_loads[0].current_source_revisions == {}
    audio_engine_mock.set_pad_key_lock.assert_not_called()
    audio_engine_mock.poll_loader_events.side_effect = [_success(key_lock=True), None]

    controller.loader.poll_loader_events()

    assert controller.project.pad_key_lock[0] is None
    controller.transport.pad.set_pad_key_lock(0, enabled=False)
    requested = False
    audio_engine_mock.set_pad_key_lock.assert_called_with(
        0, requested, expected_source_generation=7
    )
    assert controller.project.pad_key_lock[0] is False
    assert controller.project.pad_loop_start_s[0] == 123.25


def test_incoming_completion_replaces_old_residency_baseline_without_publishing_mode_defaults(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.transport.residency.remember(0)
    assert controller.transport.residency._effective[0].path == "samples/old.wav"
    _admit(controller, audio_engine_mock)
    audio_engine_mock.reset_mock()
    audio_engine_mock.poll_loader_events.side_effect = [_success(), None]

    controller.loader.poll_loader_events()

    assert controller.transport.residency._effective[0].path == "samples/incoming.wav"
    audio_engine_mock.set_pad_key_lock.assert_not_called()


def test_incoming_loop_and_scalar_timing_share_monotone_request_revision(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _admit(controller, audio_engine_mock)
    controller.loader.set_incoming_loop_intent(
        0, 7, start_s=200.0, end_s=210.0, auto=False, bars=4.0
    )
    controller.loader.set_incoming_timing_intent(0, 7, period_seconds=0.75, grid_origin_s=-0.125)
    calls = audio_engine_mock.update_incoming_load_intent.call_args_list
    assert calls[0].args == (0, 7, 1, False)
    assert calls[1].args == (0, 7, 2, False)
    assert calls[1].kwargs["loop_start_s"] == 200.0
    assert calls[1].kwargs["loop_end_s"] == 210.0
    assert calls[1].kwargs["timing_override"] is True
    assert calls[1].kwargs["period_seconds"] == 0.75
    assert controller.project.manual_bpm[0] is None
    assert controller.project.pad_grid_anchor_s[0] is None


def test_newer_current_source_timing_is_preserved_when_receipt_is_drained_later(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _admit(controller, audio_engine_mock)
    controller.project.manual_bpm[0] = 123.456
    controller.project.pad_timing_intent[0] = "tap"
    controller.project.pad_grid_anchor_s[0] = -0.25
    event = _success()
    event["timing_stale"] = True
    event["analysis"] = None
    audio_engine_mock.poll_loader_events.side_effect = [event, None]

    controller.loader.poll_loader_events()

    assert controller.project.manual_bpm[0] == 123.456
    assert controller.project.pad_timing_intent[0] == "tap"
    assert controller.project.pad_grid_anchor_s[0] == -0.25
    assert controller.project.pad_loop_start_s[0] == 123.25
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()


def test_failed_update_keeps_last_admitted_overlay_and_failure_keeps_old_assignment(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.pad_key_lock[0] = True
    _admit(controller, audio_engine_mock)
    audio_engine_mock.update_incoming_load_intent.side_effect = ValueError("incoming stale")
    with pytest.raises(ValueError, match="incoming stale"):
        controller.loader.set_incoming_key_lock_override(0, 7, override=False)
    assert controller.loader._incoming_loads[0].intent.revision == 0
    audio_engine_mock.poll_loader_events.side_effect = [
        {"type": "error", "id": 0, "request_id": 7, "msg": "incoming stale"},
        None,
    ]

    controller.loader.poll_loader_events()

    assert controller.project.sample_paths[0] == "samples/old.wav"
    assert controller.project.pad_key_lock[0] is True
    assert controller.loader.incoming_load_request_id(0) is None


def test_missing_adopted_receipt_does_not_recompute_ordinary_defaults(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    _admit(controller, audio_engine_mock)
    event = _success()
    event.pop("incoming_defaults")
    audio_engine_mock.poll_loader_events.side_effect = [event, None]

    controller.loader.poll_loader_events()

    assert controller.project.sample_paths[0] == "samples/old.wav"
    assert "no adopted defaults receipt" in controller.session.sample_load_errors[0]
    audio_engine_mock.set_pad_loop_region.assert_not_called()


@pytest.mark.parametrize("generation", [1, 7])
def test_generic_loop_waits_for_receipt_for_predecessor_and_own_native_generation(
    controller: AppController, audio_engine_mock: Mock, generation: int
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    _admit(controller, audio_engine_mock)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = _generation_binding(
        generation
    )
    with pytest.raises(ValueError, match="receipt is pending"):
        controller.transport.loop.set_auto(0, enabled=False)
    with pytest.raises(ValueError, match="receipt is pending"):
        controller.transport.loop.set_start(0, 200.0)
    assert controller.project.pad_loop_start_s[0] == 0.0
    audio_engine_mock.reset_mock()
    audio_engine_mock.poll_loader_events.side_effect = [_success(), None]

    controller.loader.poll_loader_events()

    assert controller.project.pad_loop_start_s[0] == 123.25
    assert controller.project.pad_loop_auto[0]
    audio_engine_mock.set_pad_loop_region.assert_not_called()


@pytest.mark.parametrize("generation", [1, 7])
def test_generic_timing_waits_for_receipt_for_predecessor_and_own_native_generation(
    controller: AppController, audio_engine_mock: Mock, generation: int
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    _admit(controller, audio_engine_mock)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = _generation_binding(
        generation
    )
    with pytest.raises(ValueError, match="receipt is pending"):
        controller.transport.bpm.set_manual_bpm(0, 90.0)
    assert controller.project.manual_bpm[0] is None
    audio_engine_mock.reset_mock()
    audio_engine_mock.poll_loader_events.side_effect = [_success(), None]

    controller.loader.poll_loader_events()

    assert controller.project.manual_bpm[0] is None
    assert controller.project.pad_timing_intent[0] == "legacy"
    audio_engine_mock.set_pad_bpm.assert_not_called()


def test_unowned_signature_changes_do_not_transfer_predecessor_domains(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _admit(controller, audio_engine_mock)
    controller.project.pad_loop_start_s[0] = 200.0
    controller.project.pad_loop_auto[0] = False
    controller.project.manual_bpm[0] = 90.0
    controller.project.pad_timing_intent[0] = "manual"
    audio_engine_mock.poll_loader_events.side_effect = [_success(), None]

    controller.loader.poll_loader_events()

    assert controller.project.pad_loop_start_s[0] == 123.25
    assert controller.project.pad_loop_auto[0]
    assert controller.project.manual_bpm[0] is None
    assert controller.project.pad_timing_intent[0] == "legacy"


def test_later_addressed_timing_supersedes_earlier_addressed_timing(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    _admit(controller, audio_engine_mock)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = _generation_binding(7)
    controller.loader.set_incoming_timing_intent(
        0, 7, period_seconds=60.0 / 90.0, grid_origin_s=None
    )
    audio_engine_mock.update_incoming_load_intent.return_value = False
    controller.loader.set_incoming_timing_intent(0, 7, period_seconds=0.75, grid_origin_s=10.0)
    assert audio_engine_mock.update_incoming_load_intent.call_args.args[2] == 2
    event = _success()
    event["timing_stale"] = True
    audio_engine_mock.poll_loader_events.side_effect = [event, None]

    controller.loader.poll_loader_events()

    assert controller.project.manual_bpm[0] == 80.0
    assert controller.project.pad_grid_anchor_s[0] == 10.0
    audio_engine_mock.set_pad_bpm.assert_called_with(0, 80.0)


def test_addressed_override_keeps_incoming_origin_before_global_gesture(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    _admit(controller, audio_engine_mock)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = _generation_binding(7)
    controller.loader.set_incoming_key_lock_override(0, 7, override=False)
    controller.transport.global_params.set_key_lock(enabled=True)
    audio_engine_mock.poll_loader_events.side_effect = [_success(), None]

    controller.loader.poll_loader_events()

    assert controller.project.pad_key_lock[0] is False
    audio_engine_mock.update_incoming_load_intent.assert_called_once()


def test_short_source_receipt_keeps_auto_eight_bar_intent_and_effective_physical_end(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _admit(controller, audio_engine_mock)
    event = _success()
    event["duration_s"] = 130.0
    defaults = event["incoming_defaults"]
    assert isinstance(defaults, dict)
    defaults["end_s"] = 130.0
    audio_engine_mock.poll_loader_events.side_effect = [event, None]

    controller.loader.poll_loader_events()

    assert controller.project.pad_loop_auto[0]
    assert controller.project.pad_loop_bars[0] == 8.0
    assert controller.project.pad_loop_end_s[0] == 130.0
    assert controller.transport.loop.requested_region(0) == (123.25, 130.0)
    assert controller.transport.loop.effective_region(0) == (123.25, 130.0)
    audio_engine_mock.set_pad_loop_region.assert_not_called()


def test_old_loop_gesture_captures_generation_before_native_adoption_during_publication(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = _generation_binding(1)

    def adopt_during_publication(
        _sample_id: int, _start: float, _end: float | None, *, expected_source_generation: int
    ) -> None:
        audio_engine_mock.current_input_runtime_pad_binding.return_value = _generation_binding(7)
        assert expected_source_generation == 1
        message = "current source generation no longer matches captured gesture"
        raise ValueError(message)

    audio_engine_mock.set_pad_loop_region.side_effect = adopt_during_publication
    with pytest.raises(ValueError, match="generation no longer matches"):
        controller.transport.loop.set_start(0, 200.0)
    assert controller.project.sample_paths[0] == "samples/old.wav"
    assert controller.loader.incoming_load_request_id(0) is None


def test_later_addressed_override_supersedes_earlier_addressed_override(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    _admit(controller, audio_engine_mock)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = _generation_binding(7)
    controller.loader.set_incoming_key_lock_override(0, 7, override=False)
    audio_engine_mock.update_incoming_load_intent.return_value = False
    controller.loader.set_incoming_key_lock_override(0, 7, override=True)
    assert audio_engine_mock.update_incoming_load_intent.call_args.args[2] == 2
    audio_engine_mock.poll_loader_events.side_effect = [_success(), None]

    controller.loader.poll_loader_events()

    assert controller.project.pad_key_lock[0] is True
    requested = True
    audio_engine_mock.set_pad_key_lock.assert_called_with(0, requested)


def test_explicit_incoming_period_absence_rejects_without_native_or_overlay_mutation(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _admit(controller, audio_engine_mock)
    before = controller.loader._incoming_loads[0]

    with pytest.raises(ValueError, match="period must be positive and finite"):
        controller.loader.set_incoming_timing_intent(
            0,
            7,
            period_seconds=None,
            grid_origin_s=10.0,  # type: ignore[arg-type]
        )

    assert controller.loader._incoming_loads[0] is before
    audio_engine_mock.update_incoming_load_intent.assert_not_called()


@pytest.mark.parametrize(
    "operation",
    [
        "pad",
        "pad-clear",
        "toggle",
        "auto",
        "bars",
        "start",
        "end",
        "all",
        "grid",
        "manual",
        "clear",
        "tap",
    ],
)
@pytest.mark.parametrize("binding_state", ["missing", "future", "own"])
@pytest.mark.parametrize("source_present", [False, True])
def test_ordinary_pending_gesture_rejects_before_any_predecessor_read_or_mutation(
    controller: AppController,
    audio_engine_mock: Mock,
    operation: str,
    binding_state: str,
    *,
    source_present: bool,
) -> None:
    project = controller.project
    project.sample_paths[0] = "samples/old.wav" if source_present else None
    project.sample_durations[0] = 600.0
    project.pad_key_lock[0] = True
    project.manual_bpm[0] = 90.0
    project.sample_analysis[0] = SampleAnalysis(
        bpm=90.0, key="C", beat_grid=BeatGrid(beats=[], downbeats=[], bars=[])
    )
    project.pad_loop_auto[0] = False
    project.pad_loop_start_s[0] = 3.0
    project.pad_loop_end_s[0] = 4.0
    _admit(controller, audio_engine_mock)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    binding = _generation_binding(7, current=binding_state == "own")
    audio_engine_mock.current_input_runtime_pad_binding.return_value = (
        None if binding_state == "missing" else binding
    )
    before = project.model_dump()
    taps = list(controller.session.tap_bpm_timestamps)
    baseline = dict(controller.transport.residency._effective)
    gestures = {
        "pad": lambda: controller.transport.pad.set_pad_key_lock(0, enabled=False),
        "pad-clear": lambda: controller.transport.pad.clear_pad_key_lock_override(0),
        "toggle": lambda: controller.transport.pad.toggle_pad_key_lock(0),
        "auto": lambda: controller.transport.loop.set_auto(0, enabled=False),
        "bars": lambda: controller.transport.loop.set_bars(0, bars=4.0),
        "start": lambda: controller.transport.loop.set_start(0, 20.0),
        "end": lambda: controller.transport.loop.set_end(0, 30.0),
        "all": lambda: controller.transport.loop.set_full_track_region(0),
        "grid": lambda: controller.transport.loop.set_grid_offset_samples(0, 100),
        "manual": lambda: controller.transport.bpm.set_manual_bpm(0, 90.0),
        "clear": lambda: controller.transport.bpm.clear_manual_bpm(0),
        "tap": lambda: controller.transport.bpm.tap_bpm(0),
    }

    with pytest.raises(ValueError, match="current-source gesture rejected"):
        gestures[operation]()

    assert project.model_dump() == before
    assert controller.session.tap_bpm_timestamps == taps
    assert controller.transport.residency._effective == baseline
    assert controller.transport.residency._pending == {}
    binding.metadata.assert_not_called()
    for name in (
        "set_pad_key_lock",
        "set_pad_loop_region",
        "set_pad_bpm",
        "set_pad_timing_metadata",
        "set_pad_timing_intent",
        "prepare_resident_control",
    ):
        getattr(audio_engine_mock, name).assert_not_called()


def test_bpm_gesture_keeps_capture_through_derived_calls_and_rejects_adoption_race(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    generation = 3
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = lambda sample_id: (
        _generation_binding(generation, sample_id=sample_id)
    )

    def adopt_after_bpm(_sample_id: int, _bpm: float, *, expected_source_generation: int) -> None:
        nonlocal generation
        assert expected_source_generation == generation == 3
        generation = 7

    def guarded_metadata(
        _sample_id: int, _origin: float, *, expected_source_generation: int
    ) -> None:
        if expected_source_generation != generation:
            message = "current source generation no longer matches captured gesture"
            raise ValueError(message)

    audio_engine_mock.set_pad_bpm.side_effect = adopt_after_bpm
    audio_engine_mock.set_pad_timing_metadata.side_effect = guarded_metadata
    with pytest.raises(ValueError, match="generation no longer matches"):
        controller.transport.bpm.set_manual_bpm(0, 90.0)
    audio_engine_mock.set_pad_bpm.assert_called_once_with(0, 90.0, expected_source_generation=3)
    assert audio_engine_mock.set_pad_timing_metadata.call_args.kwargs == {
        "expected_source_generation": 3
    }
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    assert controller.project.sample_paths[0] == "samples/old.wav"
    assert controller.loader.incoming_load_request_id(0) is None


def test_global_incoming_update_skips_unavailable_predecessor_and_continues_other_targets(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.sample_paths[1] = "samples/other.wav"
    _admit(controller, audio_engine_mock)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = lambda sample_id: (
        None if sample_id == 0 else _generation_binding(1, sample_id=sample_id)
    )
    enabled = True

    controller.transport.global_params.set_key_lock(enabled=enabled)

    audio_engine_mock.update_incoming_load_intent.assert_called_once()
    assert audio_engine_mock.update_incoming_load_intent.call_args.args == (0, 7, 1, True)
    assert audio_engine_mock.set_pad_key_lock.call_args_list == [
        call(1, enabled, expected_source_generation=1)
    ]
    assert 0 not in controller.transport.residency._errors


@pytest.mark.parametrize("scalar", [False, True])
def test_residency_retry_retains_captured_generation_and_cannot_retarget_new_source(
    controller: AppController, audio_engine_mock: Mock, *, scalar: bool
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    generation = 3
    audio_engine_mock.loaded_residency.return_value = {
        "source_identity": 1,
        "window_revision": 1,
        "cache_backed": not scalar,
    }
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = lambda sample_id: (
        _generation_binding(generation, sample_id=sample_id)
    )
    method = (
        audio_engine_mock.set_pad_key_lock if scalar else audio_engine_mock.prepare_resident_control
    )
    admissions = 0

    def guard(*_args: object, expected_source_generation: int, **_kwargs: object) -> None:
        nonlocal admissions
        admissions += 1
        if expected_source_generation != generation:
            message = "current source generation no longer matches captured gesture"
            raise ValueError(message)
        message = "command queue is full"
        raise RuntimeError(message)

    method.side_effect = guard
    residency = controller.transport.residency
    residency.remember(0)
    residency._request(0, 0.0, 2.0, key_lock_only=True, scalar=scalar, expected_source_generation=3)
    pending = residency._pending[0]
    generation = 7

    residency._retry(0, pending)

    assert admissions == 2
    assert [entry.kwargs["expected_source_generation"] for entry in method.call_args_list] == [3, 3]
    assert 0 not in residency._pending
    assert "generation no longer matches" in residency._errors[0]


def test_generic_bpm_carries_one_source_capture_to_every_native_derived_publication(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/current.wav"
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = _generation_binding(5)

    controller.transport.bpm.set_manual_bpm(0, 90.0)

    for name in (
        "set_pad_bpm",
        "set_pad_timing_metadata",
        "set_pad_loop_region",
        "set_pad_timing_intent",
    ):
        method = getattr(audio_engine_mock, name)
        assert method.called
        assert all(
            entry.kwargs == {"expected_source_generation": 5} for entry in method.call_args_list
        )


def test_saved_pending_without_ordinary_incoming_cell_keeps_legacy_maintenance_calls(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/restored.wav"
    controller.session.loading_sample_ids.add(0)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = None

    controller.transport.bpm.set_manual_bpm(0, 90.0)

    assert controller.loader.incoming_load_request_id(0) is None
    audio_engine_mock.set_pad_bpm.assert_called_once_with(0, 90.0)
    assert audio_engine_mock.set_pad_timing_metadata.call_args.kwargs == {}


@pytest.mark.parametrize("operation", ["start", "exclusive", "seek"])
@pytest.mark.parametrize("binding_state", ["missing", "future", "own"])
def test_playback_pending_source_rejects_before_start_restore_or_seek_projection_mutation(
    controller: AppController, audio_engine_mock: Mock, operation: str, binding_state: str
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.session.active_sample_ids.add(0)
    controller.session.pad_playhead_s[0] = 3.0
    _admit(controller, audio_engine_mock)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = (
        None
        if binding_state == "missing"
        else _generation_binding(7, current=binding_state == "own")
    )
    playback = controller.transport.playback
    gestures = {
        "start": lambda: playback.trigger_pad_keep_others(0),
        "exclusive": lambda: playback.trigger_pad(0),
        "seek": lambda: playback.seek_pad(0, 20.0),
    }
    with patch.object(playback, "_forget_global_start_stop_restore") as forget:
        with pytest.raises(ValueError, match=r"current-source (gesture|start) rejected"):
            gestures[operation]()
        forget.assert_not_called()
    assert controller.session.pad_playhead_s[0] == 3.0
    assert controller.session.active_sample_ids == {0}
    assert controller.transport.residency._pending == {}
    for name in (
        "set_pad_loop_region",
        "play_sample",
        "play_sample_exclusive",
        "seek_sample",
        "prepare_resident_control",
    ):
        getattr(audio_engine_mock, name).assert_not_called()


def test_seek_adoption_race_keeps_captured_source_at_native_admission(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.session.active_sample_ids.add(0)
    controller.session.pad_playhead_s[0] = 3.0
    generation = 3
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = lambda sample_id: (
        _generation_binding(generation, sample_id=sample_id)
    )

    def reject_after_adoption(*_args: object, expected_source_generation: int) -> None:
        nonlocal generation
        generation = 7
        assert expected_source_generation == 3
        message = "current source generation no longer matches captured gesture"
        raise ValueError(message)

    method = audio_engine_mock.seek_sample
    method.side_effect = reject_after_adoption
    playback = controller.transport.playback
    with pytest.raises(ValueError, match="generation no longer matches"):
        playback.seek_pad(0, 20.0)
    assert method.call_args.kwargs == {"expected_source_generation": 3}
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    assert controller.session.pad_playhead_s[0] == 3.0
    assert controller.project.sample_paths[0] == "samples/old.wav"


@pytest.mark.parametrize("exclusive", [False, True])
def test_captured_start_adoption_race_rejects_original_without_recapturing_new_source(
    controller: AppController, audio_engine_mock: Mock, *, exclusive: bool
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.session.active_sample_ids.add(0)
    controller.session.pad_playhead_s[0] = 3.0
    generation = 3
    original = FakeCapturedPadStart(
        0, 0.0, None, 1.0, exclusive=exclusive, received_at_ns=99, expected_source_generation=3
    )
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = lambda sample_id: (
        _generation_binding(generation, sample_id=sample_id)
    )

    def capture(*_args: object, **options: object) -> FakeCapturedPadStart:
        nonlocal generation
        assert options["expected_source_generation"] == generation == 3
        generation = 7
        return original

    def reject_publication(start: FakeCapturedPadStart) -> bool:
        assert start is original
        assert start.expected_source_generation == 3
        assert generation == 7
        message = "current source generation no longer matches captured gesture"
        raise ValueError(message)

    audio_engine_mock.capture_pad_start.side_effect = capture
    audio_engine_mock.publish_pad_start.side_effect = reject_publication
    playback = controller.transport.playback
    if exclusive:
        playback.trigger_pad(0, received_at_ns=99)
    else:
        playback.trigger_pad_keep_others(0, received_at_ns=99)

    audio_engine_mock.capture_pad_start.assert_called_once_with(
        0, 0.0, None, 1.0, exclusive=exclusive, received_at_ns=99, expected_source_generation=3
    )
    audio_engine_mock.publish_pad_start.assert_called_once_with(original)
    assert original.cancelled
    assert controller.transport.residency._captured_starts == {}
    assert "generation no longer matches" in controller.transport.residency._launch_errors[0]
    assert controller.session.pad_playhead_s[0] == 3.0
    assert controller.session.active_sample_ids == {0}
    assert controller.project.sample_paths[0] == "samples/old.wav"
    for name in ("set_pad_loop_region", "play_sample", "play_sample_exclusive"):
        getattr(audio_engine_mock, name).assert_not_called()


def test_toggle_captures_source_before_derived_read_and_cannot_retarget_adoption(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    generation = 3
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = lambda sample_id: (
        _generation_binding(generation, sample_id=sample_id)
    )
    resolve = ProjectState.resolve_pad_key_lock

    def adopt_during_read(project: ProjectState, sample_id: int, **kwargs: object) -> bool:
        nonlocal generation
        generation = 7
        return resolve(project, sample_id, **kwargs)

    def guard(_sample_id: int, _enabled: object, *, expected_source_generation: int) -> None:
        assert expected_source_generation == 3
        assert generation == 7
        message = "current source generation no longer matches captured gesture"
        raise ValueError(message)

    audio_engine_mock.set_pad_key_lock.side_effect = guard
    transport = controller.transport
    with (
        patch.object(
            ProjectState, "resolve_pad_key_lock", side_effect=adopt_during_read, autospec=True
        ),
        patch.object(
            transport,
            "capture_current_pad_edit_generation",
            wraps=transport.capture_current_pad_edit_generation,
        ) as capture,
    ):
        transport.pad.toggle_pad_key_lock(0)
        capture.assert_called_once_with(0)
    requested = True
    audio_engine_mock.set_pad_key_lock.assert_called_once_with(
        0, requested, expected_source_generation=3
    )
    assert "generation no longer matches" in transport.residency._errors[0]
    assert controller.project.sample_paths[0] == "samples/old.wav"


def _own_binding(sample_id: int = 0) -> FakeInputRuntimePadBinding:
    binding = FakeInputRuntimePadBinding(sample_id)
    binding._metadata["source_generation"] = 7
    binding._metadata["source_id"] = f"incoming-{sample_id}-7"
    return binding


def test_global_change_cannot_publish_predecessor_inheritance_over_adopted_explicit_incoming_on(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.key_lock = True
    _admit(controller, audio_engine_mock)
    controller.loader.set_incoming_key_lock_override(0, 7, override=True)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = _own_binding()
    controller.transport.global_params.set_key_lock(enabled=False)

    audio_engine_mock.set_pad_key_lock.assert_not_called()
    audio_engine_mock.prepare_resident_control.assert_not_called()
    audio_engine_mock.update_incoming_load_intent.assert_called_once()
    audio_engine_mock.poll_loader_events.side_effect = [_success(revision=1, key_lock=True), None]
    controller.loader.poll_loader_events()
    assert controller.project.key_lock is False
    assert controller.project.pad_key_lock[0] is True
    assert controller.project.resolve_pad_key_lock(0)
    audio_engine_mock.set_pad_key_lock.assert_not_called()
    audio_engine_mock.prepare_resident_control.assert_not_called()


def test_midi_snapshot_withholds_pending_own_source_and_preserves_other_ready_pads(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.sample_paths[215] = "samples/peer.wav"
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 3.0
    controller.project.pad_loop_end_s[0] = 4.0
    _admit(controller, audio_engine_mock)
    own = _own_binding()
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = lambda sample_id: (
        own if sample_id == 0 else FakeInputRuntimePadBinding(sample_id)
    )
    controller.input_mapping._sync_rust_runtime_state()
    _, loaded, starts, ends, bindings = audio_engine_mock.set_input_runtime_state.call_args.args
    assert loaded[0] is False
    assert bindings[0] is None
    assert (starts[0], ends[0]) == (0.0, None)
    assert loaded[215] is True
    assert bindings[215].metadata()["pad_id"] == 215

    captured = Mock(spec=["diagnostic_slot", "original_owner_status"])
    captured.diagnostic_slot = 0
    captured.original_owner_status.return_value = "current"
    controller.input_mapping._retry_guarded_midi_trigger("pad.trigger:0", 123_456_789, captured)
    assert "receipt is pending" in controller.session.input_mapping_error
    audio_engine_mock.trigger_input_runtime_pad.assert_not_called()
    audio_engine_mock.prepare_resident_control.assert_not_called()
    audio_engine_mock.trigger_captured_input_pad.assert_not_called()
    audio_engine_mock.prepare_captured_input_trigger.assert_not_called()
    audio_engine_mock.trigger_prepared_input_pad.assert_not_called()
    controller.transport.residency.prepare_captured_trigger(captured)
    controller.transport.residency.poll()
    audio_engine_mock.prepare_captured_input_trigger.assert_not_called()
    audio_engine_mock.trigger_prepared_input_pad.assert_not_called()

    audio_engine_mock.poll_loader_events.side_effect = [_success(), None]
    controller.loader.poll_loader_events()
    audio_engine_mock.trigger_captured_input_pad.return_value = True
    controller.input_mapping._sync_rust_runtime_state()
    controller.input_mapping._retry_guarded_midi_trigger("pad.trigger:0", 123_456_789, captured)
    # The original readiness owner retains this gesture through its own WindowACK;
    # a duplicate delivery must not bypass it via a fresh direct launch.
    audio_engine_mock.trigger_captured_input_pad.assert_not_called()
    window = Mock(
        spec=["publication_status", "error", "cancel_launch", "cancel"],
        publication_status=Mock(return_value="preparing"),
    )
    audio_engine_mock.prepare_captured_input_trigger.return_value = window
    audio_engine_mock.trigger_prepared_input_pad.return_value = True
    controller.transport.residency.poll()
    audio_engine_mock.prepare_captured_input_trigger.assert_called_once_with(captured)
    audio_engine_mock.trigger_prepared_input_pad.assert_not_called()
    window.publication_status.return_value = "accepted"
    controller.transport.residency.poll()
    audio_engine_mock.trigger_prepared_input_pad.assert_called_once_with(captured, window)
    assert not controller.transport.residency.owns_captured_trigger(captured)
    audio_engine_mock.trigger_input_runtime_pad.assert_not_called()
    _, loaded, starts, ends, bindings = audio_engine_mock.set_input_runtime_state.call_args.args
    assert loaded[0] is True
    assert bindings[0] is own
    assert (starts[0], ends[0]) == (123.25, 139.25)


@pytest.mark.parametrize("deferred", [False, True])
def test_global_start_rejects_whole_batch_before_pending_own_project_loop_read(
    controller: AppController, audio_engine_mock: Mock, *, deferred: bool
) -> None:
    controller.project.sample_paths[0] = "samples/old.wav"
    controller.project.sample_paths[215] = "samples/peer.wav"
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 3.0
    controller.project.pad_loop_end_s[0] = 4.0
    controller.session.active_sample_ids = {0, 215}
    playback = controller.transport.playback
    global_playback = playback._global_playback
    original: FakeCapturedGlobalStart | None = None
    if deferred:
        window = Mock(
            spec=["publication_status", "is_current", "error", "cancel", "cancel_launch"],
            publication_status=Mock(return_value="preparing"),
            is_current=Mock(return_value=True),
        )
        audio_engine_mock.loaded_residency.return_value = {
            "cache_backed": True,
            "source_identity": 1,
            "window_revision": 1,
        }
        audio_engine_mock.prepare_resident_control.return_value = window
        controller.transport.residency.publish_loop(0, 3.0, 4.0)
        playback.start_or_restart_global_start_stop(received_at_ns=99)
        assert global_playback._deferred is not None
        original = global_playback._deferred.capture
        assert isinstance(original, FakeCapturedGlobalStart)
        assert original.entries[0][3] is window
    _admit(controller, audio_engine_mock)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = lambda sample_id: (
        _own_binding() if sample_id == 0 else FakeInputRuntimePadBinding(sample_id)
    )
    if original is not None:
        original.status = "rejected"
        original.failure = "original source generation changed"
    with patch.object(controller.transport.loop, "requested_region") as region:
        if deferred:
            global_playback.poll()
        else:
            playback.start_or_restart_global_start_stop(received_at_ns=99)
        region.assert_not_called()
    audio_engine_mock.start_global_playback_batch.assert_not_called()
    assert global_playback._deferred is None
    error = controller.transport.residency._launch_errors[0]
    assert ("source generation changed" if deferred else "receipt is pending") in error
    assert controller.session.active_sample_ids == {0, 215}
    audio_engine_mock.poll_loader_events.side_effect = [_success(), None]
    controller.loader.poll_loader_events()
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 215}
    playback.start_or_restart_global_start_stop(received_at_ns=99)
    expected_captures = 2 if deferred else 1
    assert audio_engine_mock.capture_global_start.call_count == expected_captures
    entries = audio_engine_mock.capture_global_start.call_args.args[0]
    assert [entry[0].metadata()["pad_id"] for entry in entries] == [0, 215]
    assert entries[0][1:] == (123.25, 139.25, None)
    assert audio_engine_mock.capture_global_start.call_args.kwargs == {"received_at_ns": 99}
    if original is not None:
        assert audio_engine_mock.publish_captured_global_start.call_args_list[0] == call(original)
    assert audio_engine_mock.publish_captured_global_start.call_args != call(original)
    audio_engine_mock.start_global_playback_batch.assert_not_called()


@pytest.mark.parametrize("operation", ["gain", "eq"])
def test_pending_own_source_mixing_gesture_rejects_before_publication_and_resumes_after_receipt(
    controller: AppController, audio_engine_mock: Mock, operation: str
) -> None:
    project = controller.project
    project.sample_paths[0] = "samples/old.wav"
    project.pad_gain_db[0] = -7.0
    project.pad_eq_low_db[0] = -4.0
    project.pad_eq_mid_db[0] = 1.0
    project.pad_eq_high_db[0] = 6.0
    _admit(controller, audio_engine_mock)
    audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
    audio_engine_mock.current_input_runtime_pad_binding.return_value = _own_binding()
    before = project.model_dump()
    gestures = {
        "gain": lambda: controller.transport.pad.set_pad_gain(0, -3.0),
        "eq": lambda: controller.transport.pad.set_pad_eq(0, 2.0, -1.0, 4.0),
    }
    with pytest.raises(ValueError, match="receipt is pending"):
        gestures[operation]()
    assert project.model_dump() == before
    audio_engine_mock.set_pad_gain.assert_not_called()
    audio_engine_mock.set_pad_eq.assert_not_called()

    audio_engine_mock.poll_loader_events.side_effect = [_success(), None]
    controller.loader.poll_loader_events()
    assert (
        project.pad_gain_db[0],
        project.pad_eq_low_db[0],
        project.pad_eq_mid_db[0],
        project.pad_eq_high_db[0],
    ) == (0.0, 0.0, 0.0, 0.0)
    audio_engine_mock.set_pad_gain.assert_not_called()
    audio_engine_mock.set_pad_eq.assert_not_called()
    gestures[operation]()
    if operation == "gain":
        assert project.pad_gain_db[0] == -3.0
        audio_engine_mock.set_pad_gain.assert_called_once_with(0, -3.0)
        audio_engine_mock.set_pad_eq.assert_not_called()
    else:
        assert (project.pad_eq_low_db[0], project.pad_eq_mid_db[0], project.pad_eq_high_db[0]) == (
            2.0,
            -1.0,
            4.0,
        )
        audio_engine_mock.set_pad_eq.assert_called_once_with(0, 2.0, -1.0, 4.0)
        audio_engine_mock.set_pad_gain.assert_not_called()
