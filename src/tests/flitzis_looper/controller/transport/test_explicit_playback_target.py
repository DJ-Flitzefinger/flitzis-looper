"""Controller routing only; Native separately proves source/order/audio effects."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from tests.flitzis_looper.conftest import FakeCapturedPadStart, FakeGlobalPlaybackBatchTicket
from tests.flitzis_looper.controller.transport.test_global_batches import _accepted_sources

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


class _Target:
    """Opaque identity stand-in; it does not synthesize Native playback history."""

    def __init__(self, slot: int | None) -> None:
        self.slot = slot
        self.error: str | None = None

    def current_control_slot(self) -> int | None:
        if self.error is not None:
            raise RuntimeError(self.error)
        return self.slot


def _loaded(controller: AppController, *slots: int) -> None:
    for slot in slots:
        controller.project.sample_paths[slot] = f"samples/source-{slot}.wav"
        controller.project.sample_durations[slot] = 10.0
        controller.project.pad_loop_auto[slot] = False
        controller.project.pad_loop_start_s[slot] = slot / 10
        controller.project.pad_loop_end_s[slot] = 2.0 + slot / 10
    controller.project.multi_loop = False


def test_off_start_keeps_native_target_across_stop_runtime_updates_and_editor_focus(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _loaded(controller, 0, 1, 2)
    target = _Target(2)
    audio_engine_mock.current_explicit_playback_target.return_value = target
    controller.project.selected_pad = 0
    controller.session.active_sample_ids = {0, 1, 2}
    playback = controller.transport.playback

    playback.stop_global_start_stop()
    controller.transport.on_frame_render()
    controller.project.pad_key_intent[2] = controller.project.pad_key_intent[2].changed(
        extra_shift=1
    )
    controller.input_mapping._refresh_rust_runtime_state()
    playback.start_or_restart_global_start_stop(received_at_ns=37)

    audio_engine_mock.stop_explicit_playback.assert_called_once_with()
    captured = audio_engine_mock.capture_pad_start.call_args
    assert captured.args == (2, 0.2, 2.2, 1.0)
    assert captured.kwargs["explicit_target"] is target
    assert captured.kwargs["exclusive"] is True
    assert captured.kwargs["received_at_ns"] == 37
    original = audio_engine_mock.publish_pad_start.call_args.args[0]
    assert isinstance(original, FakeCapturedPadStart)
    assert original.explicit_target is target
    audio_engine_mock.capture_global_start.assert_not_called()
    audio_engine_mock.stop_global_playback_batch.assert_not_called()
    assert controller.project.selected_pad == 0


@pytest.mark.parametrize("route", ["direct", "midi"])
def test_start_routes_follow_native_content_placement_without_using_editor_selection(
    controller: AppController, audio_engine_mock: Mock, route: str
) -> None:
    _loaded(controller, 0, 2, 8)
    controller.input_mapping.set_enabled(enabled=True)
    controller.project.selected_pad = 0
    target = _Target(2)
    audio_engine_mock.current_explicit_playback_target.return_value = target
    target.slot = 8  # The fixture represents Native's completed Move/Swap resolution.
    if route == "direct":
        controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=17)
    else:
        controller.input_mapping._handle_rust_input_event({
            "source": "midi",
            "binding_key": "midi:note:1:63",
            "action_key": "global.start_stop",
            "received_at_ns": 17,
            "direct": False,
            "dispatched": True,
        })
    assert audio_engine_mock.capture_pad_start.call_args.args[0] == 8
    assert audio_engine_mock.capture_pad_start.call_args.kwargs["explicit_target"] is target
    assert controller.project.selected_pad == 0
    audio_engine_mock.capture_global_start.assert_not_called()


@pytest.mark.parametrize(
    "failure", ["Hold is pending", "content placement publication is pending", "source is busy"]
)
def test_valid_unavailable_target_never_falls_back_or_clears_original_intent(
    controller: AppController, audio_engine_mock: Mock, failure: str
) -> None:
    _loaded(controller, 0, 2)
    target = _Target(2)
    target.error = failure
    audio_engine_mock.current_explicit_playback_target.return_value = target
    controller.project.selected_pad = 0
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {1, 2}
    before = controller.session.model_dump()
    with pytest.raises(RuntimeError, match=failure):
        controller.transport.playback.start_or_restart_global_start_stop()
    assert controller.session.model_dump() == before
    audio_engine_mock.capture_pad_start.assert_not_called()
    audio_engine_mock.capture_global_start.assert_not_called()


def test_target_invalidated_after_query_does_not_retarget_the_same_press(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _loaded(controller, 0)
    audio_engine_mock.current_explicit_playback_target.return_value = _Target(None)
    controller.transport.playback.start_or_restart_global_start_stop()
    audio_engine_mock.capture_pad_start.assert_not_called()
    audio_engine_mock.capture_global_start.assert_not_called()


def test_only_absent_history_can_start_loaded_selection(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _loaded(controller, 1)
    controller.project.selected_pad = 1
    controller.transport.playback.start_or_restart_global_start_stop()
    assert audio_engine_mock.capture_pad_start.call_args.args[0] == 1
    assert "explicit_target" not in audio_engine_mock.capture_pad_start.call_args.kwargs
    assert audio_engine_mock.capture_pad_start.call_args.kwargs["exclusive"] is True


def test_target_capture_replacement_failure_never_retries_selected_pad(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _loaded(controller, 0, 2)
    target = _Target(2)
    audio_engine_mock.current_explicit_playback_target.return_value = target
    audio_engine_mock.capture_pad_start.side_effect = ValueError("explicit source was replaced")
    with pytest.raises(ValueError, match="explicit source was replaced"):
        controller.transport.playback.start_or_restart_global_start_stop()
    audio_engine_mock.capture_pad_start.assert_called_once()
    assert audio_engine_mock.capture_pad_start.call_args.args[0] == 2
    audio_engine_mock.publish_pad_start.assert_not_called()


@pytest.mark.parametrize("active", [False, True])
def test_off_stop_is_always_one_native_fence_and_keeps_telemetry_unchanged(
    controller: AppController, audio_engine_mock: Mock, *, active: bool
) -> None:
    _loaded(controller, 0, 1, 2)
    controller.session.active_sample_ids = {0, 1, 2} if active else set()
    controller.session.paused_sample_ids = {1} if active else set()
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0, 1, 2}
    ticket = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.stop_explicit_playback.return_value = ticket
    controller.transport.playback.stop_global_start_stop()
    owner = controller.transport.playback._global_playback
    controller.transport.on_frame_render()
    assert owner._fenced_batches == [ticket]
    assert controller.session.active_sample_ids == ({0, 1, 2} if active else set())
    assert controller.session.paused_sample_ids == ({1} if active else set())
    assert controller.session.global_stop_restore_sample_ids == set()
    assert controller.session.global_stop_engaged is False
    audio_engine_mock.stop_explicit_playback.assert_called_once_with()
    audio_engine_mock.current_explicit_playback_target.assert_not_called()
    audio_engine_mock.cancel_all_launches.assert_not_called()
    audio_engine_mock.cancel_pad_launches.assert_not_called()
    audio_engine_mock.stop_all.assert_not_called()
    audio_engine_mock.stop_sample.assert_not_called()
    ticket.status = "accepted"
    owner.poll()
    assert not owner._fenced_batches


def test_refused_off_stop_retains_old_restore_and_deferred_owners(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0, 2}
    owner = controller.transport.playback._global_playback
    gate = owner.capture_recording_start()
    original = owner._deferred
    controller.project.multi_loop = False
    audio_engine_mock.stop_explicit_playback.side_effect = RuntimeError("queue is full")
    before = controller.session.model_dump()
    with pytest.raises(RuntimeError, match="queue is full"):
        controller.transport.playback.stop_global_start_stop()
    assert controller.session.model_dump() == before
    assert owner._deferred is original
    assert gate is not None
    assert gate.is_current()


def test_fenced_stop_keeps_prior_accepted_pending_owner_without_group_revival(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0, 2}
    previous = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.stop_global_playback_batch.return_value = previous
    owner = controller.transport.playback._global_playback
    owner.stop(remember=True)
    original = owner._pending
    controller.project.multi_loop = False
    controller.transport.playback.stop_global_start_stop()
    assert owner._pending is original
    previous.status = "accepted"
    owner.poll()
    assert owner._pending is None
    assert controller.session.global_stop_restore_sample_ids == set()
    assert controller.session.global_stop_engaged is False


def test_recording_still_captures_group_with_multi_loop_off(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock)
    controller.project.multi_loop = False
    controller.session.active_sample_ids = {0, 2}
    gate = controller.transport.playback.capture_recording_start(received_at_ns=81)
    assert gate is not None
    entries = audio_engine_mock.capture_global_start.call_args.args[0]
    assert [binding.metadata()["pad_id"] for binding, *_rest in entries] == [0, 2]
    audio_engine_mock.current_explicit_playback_target.assert_not_called()
    audio_engine_mock.capture_pad_start.assert_not_called()


def test_off_stop_capacity_is_checked_before_native_admission(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    owner = controller.transport.playback._global_playback
    owner._fenced_batches = [FakeGlobalPlaybackBatchTicket("pending") for _ in range(256)]
    with pytest.raises(RuntimeError, match="observation capacity"):
        controller.transport.playback.stop_global_start_stop()
    audio_engine_mock.stop_explicit_playback.assert_not_called()
    assert len(owner._fenced_batches) == 256


@pytest.mark.parametrize("pending_group", [False, True])
def test_off_left_closes_original_different_cohort_before_capturing_new_singleton(
    controller: AppController, audio_engine_mock: Mock, *, pending_group: bool
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0, 1, 3))
    controller.session.active_sample_ids = {0, 1}
    owner = controller.transport.playback._global_playback
    if pending_group:
        receipt = FakeGlobalPlaybackBatchTicket("pending")
        audio_engine_mock.publish_captured_global_start.side_effect = lambda capture: (
            capture.publish(receipt)
        )
        owner.start()
        pending = owner._pending
        assert pending is not None
        assert pending.capture is not None
        original = pending.capture
    else:
        owner.capture_recording_start()
        assert owner._deferred is not None
        original = owner._deferred.capture
        pending = None
    order = []
    original.cancel = Mock(side_effect=lambda: order.append("cancel") or False)

    def capture(*args: object, **kwargs: object) -> FakeCapturedPadStart:
        order.append("capture")
        return FakeCapturedPadStart(*args, **kwargs)

    audio_engine_mock.capture_pad_start.side_effect = capture
    controller.project.multi_loop = False
    audio_engine_mock.current_explicit_playback_target.return_value = _Target(3)
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=97)
    assert order == ["cancel", "capture"]
    assert audio_engine_mock.capture_pad_start.call_args.args[0] == 3
    assert audio_engine_mock.capture_pad_start.call_args.kwargs["received_at_ns"] == 97
    assert owner._pending is pending
    assert owner._deferred is None
    assert controller.session.global_stop_restore_sample_ids == set()


def test_off_left_cancellation_refusal_keeps_original_and_admits_no_singleton(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0, 1, 3))
    controller.session.active_sample_ids = {0, 1}
    owner = controller.transport.playback._global_playback
    owner.capture_recording_start()
    original = owner._deferred
    assert original is not None
    original.capture.cancel = Mock(side_effect=RuntimeError("original group cancellation refused"))
    controller.project.multi_loop = False
    audio_engine_mock.current_explicit_playback_target.return_value = _Target(3)
    before = controller.session.model_dump()
    with pytest.raises(RuntimeError, match="original group cancellation refused"):
        controller.transport.playback.start_or_restart_global_start_stop()
    assert owner._deferred is original
    assert controller.session.model_dump() == before
    audio_engine_mock.capture_pad_start.assert_not_called()
