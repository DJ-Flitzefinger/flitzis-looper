"""Original-press control routing only; no Native effect, codec or device proof.

The existing controller fixtures supply explicit capture/receipt doubles. These
tests check immutable routing and owner fencing, not manufactured audio success.
"""

import time
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock, call

import pytest

from tests.flitzis_looper.conftest import (
    FakeCapturedGlobalStart,
    FakeGlobalPlaybackBatchTicket,
)
from tests.flitzis_looper.controller.transport.test_global_batches import (
    _accepted_sources,
    _assert_no_unguarded_batch_effect,
    _pending_loop_edit,
)

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.recording import RecordingGroupGate
    from flitzis_looper_audio import MixCaptureTicket


def _token() -> MixCaptureTicket:
    """An identity-only argument; it provides no synthetic Native status or ACK."""
    return cast("MixCaptureTicket", object())


def _gate(
    controller: AppController, audio: Mock, *, received_at_ns: int = 123
) -> tuple[RecordingGroupGate, FakeCapturedGlobalStart]:
    control = controller.transport.playback._global_playback
    gate = control.capture_recording_start(received_at_ns=received_at_ns)
    assert gate is not None
    assert control._deferred is not None
    original = control._deferred.capture
    assert isinstance(original, FakeCapturedGlobalStart)
    audio.publish_captured_global_start.assert_not_called()
    return gate, original


def _publisher(
    audio: Mock, token: MixCaptureTicket, receipt: FakeGlobalPlaybackBatchTicket
) -> None:
    def publish(
        original: FakeCapturedGlobalStart, *, capture: MixCaptureTicket
    ) -> FakeGlobalPlaybackBatchTicket | None:
        assert capture is token
        return original.publish(receipt)

    audio.publish_captured_global_start.side_effect = publish


def _no_capture_side_effect(audio: Mock) -> None:
    _assert_no_unguarded_batch_effect(audio)
    audio.start_mix_capture.assert_not_called()
    audio.stop_mix_capture.assert_not_called()
    audio.cancel_mix_capture.assert_not_called()


def test_original_press_is_captured_once_and_poll_never_joins_without_writer(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, bindings = _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.active_sample_ids = {0}
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 0.25
    controller.project.pad_loop_end_s[0] = 0.75
    monkeypatch.setattr(time, "monotonic", lambda: 10.0)
    gate, original = _gate(controller, audio_engine_mock, received_at_ns=987)
    original.frame = 24_000  # Control-only stand-in for the original target receipt.
    control = controller.transport.playback._global_playback
    deferred = control._deferred
    source_reads = audio_engine_mock.current_input_runtime_pad_binding.call_count
    timing_reads = audio_engine_mock.current_constant_timing.call_count
    assert deferred is not None
    assert deferred.deadline == 40.0
    assert original.entries == ((bindings[0], 0.25, 0.75, None),)
    controller.project.pad_loop_start_s[0] = 0.5
    controller.project.pad_loop_end_s[0] = 1.0
    for _ in range(4):
        control.poll()
        assert gate.is_current()
    assert control._deferred is deferred
    assert original.received_at_ns == 987
    assert original.original_frame() == 24_000
    assert original.entries == ((bindings[0], 0.25, 0.75, None),)
    audio_engine_mock.capture_global_start.assert_called_once_with(
        list(original.entries), received_at_ns=987
    )
    audio_engine_mock.publish_captured_global_start.assert_not_called()
    assert audio_engine_mock.current_input_runtime_pad_binding.call_count == source_reads
    assert audio_engine_mock.current_constant_timing.call_count == timing_reads
    _no_capture_side_effect(audio_engine_mock)


def test_same_original_ticket_queues_once_and_existing_pending_owner_observes_ack(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {0}
    gate, original = _gate(controller, audio_engine_mock)
    token = _token()
    receipt = FakeGlobalPlaybackBatchTicket("pending")
    _publisher(audio_engine_mock, token, receipt)
    assert gate.publish(token)
    control = controller.transport.playback._global_playback
    pending = control._pending
    assert pending is not None
    assert id(pending.capture) == id(original)
    assert id(pending.ticket) == id(receipt)
    assert gate.publish(token)
    control.poll()
    assert control._pending is pending
    audio_engine_mock.publish_captured_global_start.assert_called_once_with(original, capture=token)
    with pytest.raises(ValueError, match=r"replace.*original.*ticket"):
        gate.publish(_token())
    assert control._pending is pending
    receipt.status = "accepted"
    original.status = "applied"
    control.poll()
    assert control._pending is None
    assert controller.session.global_stop_restore_sample_ids == set()
    assert controller.session.global_stop_engaged is False
    assert controller.session.active_sample_ids == set()
    audio_engine_mock.capture_global_start.assert_called_once()
    _no_capture_side_effect(audio_engine_mock)


def test_original_resident_readiness_none_retains_gate_and_never_recaptures(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    window = _pending_loop_edit(controller, audio_engine_mock)
    gate, original = _gate(controller, audio_engine_mock, received_at_ns=456)
    original.frame = 8192
    token = _token()
    receipt = FakeGlobalPlaybackBatchTicket("pending")
    _publisher(audio_engine_mock, token, receipt)
    assert not gate.publish(token)
    control = controller.transport.playback._global_playback
    deferred = control._deferred
    source_reads = audio_engine_mock.current_input_runtime_pad_binding.call_count
    timing_reads = audio_engine_mock.current_constant_timing.call_count
    assert deferred is not None
    original_entries = original.entries
    window.status = "accepted"
    for _ in range(3):
        control.poll()
    assert control._deferred is deferred
    audio_engine_mock.publish_captured_global_start.assert_called_once_with(original, capture=token)
    assert gate.publish(token)
    assert original.entries is original_entries
    assert original.entries[0][3] is window
    assert original.received_at_ns == 456
    assert original.original_frame() == 8192
    assert window.cancelled is False
    assert audio_engine_mock.current_input_runtime_pad_binding.call_count == source_reads
    assert audio_engine_mock.current_constant_timing.call_count == timing_reads
    audio_engine_mock.capture_global_start.assert_called_once()
    assert audio_engine_mock.publish_captured_global_start.call_args_list == [
        call(original, capture=token),
        call(original, capture=token),
    ]
    _no_capture_side_effect(audio_engine_mock)


@pytest.mark.parametrize("remembered", [False, True])
def test_recording_selection_reuses_active_unpaused_or_remembered_set(
    controller: AppController, audio_engine_mock: Mock, *, remembered: bool
) -> None:
    _, bindings = _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0, 2}
    controller.session.paused_sample_ids = {2}
    controller.session.global_stop_engaged = remembered
    controller.session.global_stop_restore_sample_ids = {2} if remembered else set()
    _, original = _gate(controller, audio_engine_mock)
    expected = 2 if remembered else 0
    assert [entry[0] for entry in original.entries] == [bindings[expected]]
    _no_capture_side_effect(audio_engine_mock)


def test_empty_or_paused_group_creates_no_original_or_recording_publication(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    control = controller.transport.playback._global_playback
    assert control.capture_recording_start(received_at_ns=7) is None
    controller.session.active_sample_ids = {0}
    controller.session.paused_sample_ids = {0}
    assert control.capture_recording_start(received_at_ns=8) is None
    audio_engine_mock.capture_global_start.assert_not_called()
    audio_engine_mock.publish_captured_global_start.assert_not_called()
    _no_capture_side_effect(audio_engine_mock)


@pytest.mark.parametrize("timestamp", [True, -1, 1.5])
def test_invalid_press_timestamp_preserves_previous_original_before_any_mutation(
    controller: AppController, audio_engine_mock: Mock, timestamp: object
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.active_sample_ids = {0}
    gate, original = _gate(controller, audio_engine_mock)
    control = controller.transport.playback._global_playback
    deferred = control._deferred
    with pytest.raises(ValueError, match="received_at_ns"):
        control.capture_recording_start(received_at_ns=cast("int", timestamp))
    assert control._deferred is deferred
    assert gate.is_current()
    assert original.cancelled is False
    audio_engine_mock.capture_global_start.assert_called_once()
    _no_capture_side_effect(audio_engine_mock)


def test_gate_deadline_uses_original_deferred_deadline_without_ungated_retry(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.active_sample_ids = {0}
    gate, original = _gate(controller, audio_engine_mock)
    control = controller.transport.playback._global_playback
    assert control._deferred is not None
    deadline = control._deferred.deadline
    monkeypatch.setattr(time, "monotonic", lambda: deadline)
    control.poll()
    assert not gate.is_current()
    assert original.cancelled
    assert control._deferred is None
    assert (
        controller.transport.residency.status(0)[1] == "global playback readiness deadline expired"
    )
    audio_engine_mock.publish_captured_global_start.assert_not_called()
    _no_capture_side_effect(audio_engine_mock)


def test_pair_placement_hold_needs_same_gate_and_ticket_after_it_becomes_ready(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.active_sample_ids = {0}
    gate, original = _gate(controller, audio_engine_mock)
    original.slots = [5]
    controller.transport._pair_deferred_sample_ids = frozenset({5})
    token = _token()
    _publisher(audio_engine_mock, token, FakeGlobalPlaybackBatchTicket("pending"))
    assert not gate.publish(token)
    controller.transport.playback._global_playback.poll()
    audio_engine_mock.publish_captured_global_start.assert_not_called()
    controller.transport._pair_deferred_sample_ids = frozenset()
    assert gate.publish(token)
    audio_engine_mock.capture_global_start.assert_called_once()
    audio_engine_mock.publish_captured_global_start.assert_called_once_with(original, capture=token)
    _no_capture_side_effect(audio_engine_mock)


def test_pair_hold_does_not_extend_original_recording_press_deadline(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.active_sample_ids = {0}
    gate, original = _gate(controller, audio_engine_mock)
    control = controller.transport.playback._global_playback
    assert control._deferred is not None
    deadline = control._deferred.deadline
    controller.transport._pair_deferred_sample_ids = frozenset({0})
    monkeypatch.setattr(time, "monotonic", lambda: deadline)
    control.poll()
    assert control._deferred is None
    assert original.cancelled
    assert not gate.is_current()
    audio_engine_mock.publish_captured_global_start.assert_not_called()
    _no_capture_side_effect(audio_engine_mock)


def test_queue_pressure_budget_is_shared_but_frame_poll_cannot_retry_the_gate(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.active_sample_ids = {0}
    gate, original = _gate(controller, audio_engine_mock)
    original.frame = 700
    token = _token()
    audio_engine_mock.publish_captured_global_start.side_effect = RuntimeError(
        "ordinary start queue is full"
    )
    control = controller.transport.playback._global_playback
    for attempt in range(7):
        assert not gate.publish(token)
        control.poll()
        assert audio_engine_mock.publish_captured_global_start.call_count == attempt + 1
    with pytest.raises(ValueError, match=r"launch_cancelled|no longer current"):
        gate.publish(token)
    assert not gate.is_current()
    assert original.cancelled
    assert original.original_frame() == 700
    assert control._deferred is None
    assert (
        audio_engine_mock.publish_captured_global_start.call_args_list
        == [call(original, capture=token)] * 8
    )
    audio_engine_mock.capture_global_start.assert_called_once()
    _no_capture_side_effect(audio_engine_mock)


def test_native_source_rejection_retires_original_without_new_source_or_time_capture(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.active_sample_ids = {0}
    gate, original = _gate(controller, audio_engine_mock)

    def reject(_original: FakeCapturedGlobalStart, *, capture: MixCaptureTicket) -> None:
        original.status = "rejected"
        original.failure = "source_changed"
        message = "original source changed"
        raise ValueError(message)

    audio_engine_mock.publish_captured_global_start.side_effect = reject
    with pytest.raises(ValueError, match="original source changed"):
        gate.publish(_token())
    assert not gate.is_current()
    assert controller.transport.playback._global_playback._deferred is None
    audio_engine_mock.capture_global_start.assert_called_once()
    audio_engine_mock.publish_captured_global_start.assert_called_once()
    assert controller.session.active_sample_ids == {0}
    _no_capture_side_effect(audio_engine_mock)


def test_cancellation_failure_keeps_exact_original_until_its_real_fence_can_retry(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.active_sample_ids = {0}
    gate, original = _gate(controller, audio_engine_mock)
    control = controller.transport.playback._global_playback
    deferred = control._deferred
    original_cancel = original.cancel
    allow = False

    def cancel() -> bool:
        if not allow:
            message = "original Native fence unavailable"
            raise RuntimeError(message)
        return original_cancel()

    monkeypatch.setattr(original, "cancel", cancel)
    with pytest.raises(RuntimeError, match="fence unavailable"):
        gate.cancel()
    assert control._deferred is deferred
    assert gate.is_current()
    assert original.cancelled is False
    allow = True
    gate.cancel()
    gate.cancel()
    assert control._deferred is None
    assert not gate.is_current()
    assert original.cancelled
    _no_capture_side_effect(audio_engine_mock)


def test_cancel_after_original_claim_retains_pending_ack_and_does_not_stop_playback(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.active_sample_ids = {0}
    gate, original = _gate(controller, audio_engine_mock)
    token = _token()
    receipt = FakeGlobalPlaybackBatchTicket("pending")
    _publisher(audio_engine_mock, token, receipt)
    assert gate.publish(token)
    control = controller.transport.playback._global_playback
    pending = control._pending
    original.status = "applied"  # Control-only interleaving of an already won claim.
    receipt.status = "accepted"
    before_project = controller.project.model_dump()
    before_session = controller.session.model_dump()
    gate.cancel()
    assert control._pending is pending
    assert original.cancelled is False
    assert receipt.publication_status() == "accepted"
    assert controller.project.model_dump() == before_project
    assert controller.session.model_dump() == before_session
    control.poll()
    assert control._pending is None
    assert controller.session.active_sample_ids == {0}
    _no_capture_side_effect(audio_engine_mock)


def test_new_ordinary_gesture_keeps_one_argument_route_and_old_gate_cannot_touch_it(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0}
    old_gate, old_original = _gate(controller, audio_engine_mock)
    controller.session.active_sample_ids = {2}
    receipt = FakeGlobalPlaybackBatchTicket("pending")
    # Deliberately accepts no capture keyword, as the ordinary installed ABI does.
    audio_engine_mock.publish_captured_global_start.side_effect = lambda original: original.publish(
        receipt
    )
    control = controller.transport.playback._global_playback
    control.start(received_at_ns=999)
    pending = control._pending
    assert pending is not None
    assert id(pending.capture) != id(old_original)
    assert isinstance(pending.capture, FakeCapturedGlobalStart)
    assert pending.capture.received_at_ns == 999
    assert old_original.cancelled
    old_gate.cancel()
    assert not old_gate.is_current()
    with pytest.raises(ValueError, match=r"launch_cancelled|no longer current"):
        old_gate.publish(_token())
    assert control._pending is pending
    assert receipt.publication_status() == "pending"
    audio_engine_mock.publish_captured_global_start.assert_called_once_with(pending.capture)
    assert audio_engine_mock.capture_global_start.call_count == 2
    _no_capture_side_effect(audio_engine_mock)


def test_ordinary_stop_invalidates_only_original_pending_group_without_capture_stop(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _, bindings = _accepted_sources(controller, audio_engine_mock, (0,))
    controller.session.active_sample_ids = {0}
    gate, original = _gate(controller, audio_engine_mock)
    control = controller.transport.playback._global_playback
    control.stop(remember=True, received_at_ns=456)
    assert original.cancelled
    assert not gate.is_current()
    with pytest.raises(ValueError, match=r"launch_cancelled|no longer current"):
        gate.publish(_token())
    audio_engine_mock.stop_global_playback_batch.assert_called_once_with(
        [bindings[0]], received_at_ns=456
    )
    audio_engine_mock.publish_captured_global_start.assert_not_called()
    _no_capture_side_effect(audio_engine_mock)


def test_old_accepted_pending_ack_cannot_overwrite_new_restore_revision(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _accepted_sources(controller, audio_engine_mock)
    controller.session.active_sample_ids = {0}
    gate, original = _gate(controller, audio_engine_mock)
    token = _token()
    receipt = FakeGlobalPlaybackBatchTicket("pending")
    _publisher(audio_engine_mock, token, receipt)
    assert gate.publish(token)
    control = controller.transport.playback._global_playback
    original.status = "applied"
    receipt.status = "accepted"
    control.forget_restore()
    controller.session.global_stop_engaged = True
    controller.session.global_stop_restore_sample_ids = {2}
    assert not gate.is_current()
    gate.cancel()
    control.poll()
    assert controller.session.global_stop_engaged
    assert controller.session.global_stop_restore_sample_ids == {2}
    assert receipt.publication_status() == "accepted"
    _no_capture_side_effect(audio_engine_mock)
