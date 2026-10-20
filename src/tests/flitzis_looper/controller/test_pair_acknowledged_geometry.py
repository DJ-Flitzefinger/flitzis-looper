"""Accepted captured-start geometry follows the surviving Pair's control owners."""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.pair_control_mirror import prepare_pair_control_mirror
from flitzis_looper.models import PadContentIdentity
from flitzis_looper.pair_snapshot import capture_pair_snapshot
from tests.flitzis_looper.conftest import FakeCapturedPadStart, FakeInputRuntimePadBinding
from tests.flitzis_looper.controller.transport.test_residency import Ticket

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.transport.residency import _Intent


def _acknowledge_start_geometry(
    controller: AppController, audio: Mock, sample_id: int, start: float, end: float
) -> _Intent:
    """Populate geometry through the ordinary capture, window ACK and launch observer."""
    controller.project.sample_paths[sample_id] = f"samples/source-{sample_id}.wav"
    controller.project.pad_content[sample_id] = PadContentIdentity(
        instance_id=("a" if sample_id == 0 else "b") * 32
    )
    controller.project.sample_durations[sample_id] = 12.0
    controller.project.pad_loop_auto[sample_id] = False
    controller.project.pad_loop_start_s[sample_id] = start
    controller.project.pad_loop_end_s[sample_id] = end
    controller.transport.residency.remember(sample_id)
    ticket = Ticket()
    audio.prepare_resident_control.return_value = ticket

    controller.transport.playback.trigger_pad_keep_others(sample_id, received_at_ns=17 + sample_id)

    captured = audio.capture_pad_start.call_args_list[-1]
    original = audio.prepare_captured_pad_start.call_args.args[0]
    assert isinstance(original, FakeCapturedPadStart)
    assert captured.args[:3] == (sample_id, start, end)
    assert original.state() == "captured"
    ticket.status = "accepted"
    controller.transport.residency.poll()
    controller.transport.residency.poll()
    assert original.state() == "applied"
    assert not controller.transport.residency._captured_starts
    geometry = controller.transport.residency._acknowledged_geometry[sample_id]
    assert (geometry.start, geometry.end) == (start, end)
    return geometry


def _prepare_acknowledged_pair(
    controller: AppController, audio: Mock, *, occupied: bool
) -> tuple[dict[int, _Intent], dict[int, FakeInputRuntimePadBinding]]:
    bindings = {sample_id: FakeInputRuntimePadBinding(sample_id) for sample_id in (0, 215)}
    audio.current_input_runtime_pad_binding.side_effect = bindings.get
    audio.pad_timing_intent.return_value = "legacy"
    audio.current_constant_timing.return_value = None
    audio.loaded_residency.return_value = {"source_identity": 123, "window_revision": 1}
    audio.pad_key_lock_status.return_value = None
    geometry = {0: _acknowledge_start_geometry(controller, audio, 0, 3.0, 4.0)}
    if occupied:
        geometry[215] = _acknowledge_start_geometry(controller, audio, 215, 6.0, 9.0)
    return geometry, bindings


@pytest.mark.parametrize("occupied", [False, True], ids=["move", "swap"])
def test_pair_mirror_moves_only_its_acknowledged_geometry_with_content(
    controller: AppController, audio_engine_mock: Mock, *, occupied: bool
) -> None:
    geometry, _ = _prepare_acknowledged_pair(controller, audio_engine_mock, occupied=occupied)
    snapshot = capture_pair_snapshot(
        controller.project, 0, 215, operation="swap" if occupied else "move"
    )
    acknowledgement = object()
    plan = prepare_pair_control_mirror(
        controller, snapshot, validate_acknowledgement=lambda value: value is acknowledgement
    )
    audio_engine_mock.reset_mock()

    plan.apply(acknowledgement)

    assert plan.applied
    actual = controller.transport.residency._acknowledged_geometry
    assert actual[215] is geometry[0]
    if occupied:
        assert actual[0] is geometry[215]
    else:
        assert 0 not in actual
    assert not audio_engine_mock.mock_calls


@pytest.mark.parametrize("edited_slot", [0, 215])
def test_failed_edit_after_swap_restores_own_captured_ack_geometry(
    controller: AppController, audio_engine_mock: Mock, edited_slot: int
) -> None:
    geometry, bindings = _prepare_acknowledged_pair(controller, audio_engine_mock, occupied=True)
    snapshot = capture_pair_snapshot(controller.project, 0, 215, operation="swap")
    acknowledgement = object()
    plan = prepare_pair_control_mirror(
        controller, snapshot, validate_acknowledgement=lambda value: value is acknowledgement
    )
    plan.apply(acknowledgement)
    # Simulate Native's actual surviving placement in the explicit binding double.
    bindings[0], bindings[215] = bindings[215], bindings[0]
    for sample_id, binding in bindings.items():
        binding._metadata["pad_id"] = sample_id
    expected = geometry[215 if edited_slot == 0 else 0]
    assert expected.end is not None
    assert controller.project.pad_loop_start_s[edited_slot] == expected.start
    assert controller.project.pad_loop_end_s[edited_slot] == expected.end
    audio_engine_mock.prepare_resident_control.side_effect = RuntimeError("invalid geometry")

    controller.transport.loop.set_end(edited_slot, expected.end + 1.0)

    assert controller.transport.residency.status(edited_slot) == (None, "invalid geometry")
    assert controller.project.pad_loop_start_s[edited_slot] == expected.start
    assert controller.project.pad_loop_end_s[edited_slot] == expected.end
    assert not controller.transport.residency._pending
    audio_engine_mock.play_sample.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    audio_engine_mock.set_pad_loop_region.assert_not_called()
