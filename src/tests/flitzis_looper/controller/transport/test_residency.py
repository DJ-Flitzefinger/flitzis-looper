"""Controller intent/ACK proofs; native PCM and ownership use separate real tests."""

from dataclasses import replace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock, patch

import pytest

from tests.flitzis_looper.conftest import (
    FakeCapturedGlobalStart,
    FakeCapturedPadStart,
    FakeGlobalPlaybackBatchTicket,
    FakeInputRuntimePadBinding,
)

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import CapturedInputPadTrigger, ResidentWindowTicket


class Ticket:
    def __init__(self, status: str = "preparing") -> None:
        self.status = status
        self.current = True
        self.message: str | None = None
        self.effective_seek_seconds: float | None = None
        self.previous_window_revision = 1
        self.window_revision = 2
        self.key_lock_request_id: int | None = None
        self.launch_cancelled = False

    @property
    def native_ticket(self) -> ResidentWindowTicket:
        """Expose only the Native boundary view; retain this fake for state changes."""
        return cast("ResidentWindowTicket", self)

    def cancel_launch(self) -> bool:
        previous = self.launch_cancelled
        self.launch_cancelled = True
        return not previous

    def publication_status(self) -> str:
        return self.status

    def is_current(self) -> bool:
        return self.current

    def error(self) -> str | None:
        return self.message

    def cancel(self) -> bool:
        if self.status in {"adopting", "accepted"}:
            return False
        self.status = "cancelled"
        return True


class CapturedMidiGesture:
    """Explicit opaque Native facade fixture, with no numeric recapture authority."""

    def __init__(self, received_at_ns: int) -> None:
        self.owner_identity = object()
        self.native_intent = ("midi:note:1:60", received_at_ns, True, self.owner_identity)
        self.diagnostic_slot: int | None = 0
        self.owner_status = "current"

    @property
    def native_trigger(self) -> CapturedInputPadTrigger:
        """Keep the original fake handle alongside its opaque Native boundary view."""
        return cast("CapturedInputPadTrigger", self)

    def original_owner_status(self) -> str:
        return self.owner_status

    def is_current(self) -> bool:
        pytest.fail("Original-window currency cannot replace the own WindowACK launch route")

    def metadata(self) -> dict[str, object]:
        pytest.fail("Only Native reads the original MIDI mapping, timestamp and exclusive intent")


def captured_midi(audio: Mock, ticket: Ticket, received_at_ns: int) -> CapturedMidiGesture:
    original = CapturedMidiGesture(received_at_ns)
    audio.trigger_captured_input_pad = Mock(return_value=False)
    audio.prepare_captured_input_trigger = Mock(return_value=ticket)
    audio.trigger_prepared_input_pad = Mock(return_value=False)
    return original


def midi_event(original: CapturedMidiGesture) -> dict[str, object]:
    return {
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.trigger:0",
        "received_at_ns": "untrusted diagnostic metadata",
        "direct": True,
        "dispatched": False,
        "captured_trigger": original,
    }


def native_cancel_captured_launch(
    audio: Mock, original: CapturedMidiGesture, ticket: Ticket
) -> None:
    """Fake only the original Native cancellation result, never controller authority."""

    def cancel_owned(sample_id: int) -> bool:
        assert sample_id == original.diagnostic_slot
        original.owner_status = "superseded"
        ticket.cancel_launch()
        return True

    def cancel_all_owned() -> list[int]:
        cancel_owned(0)
        return [0]

    audio.cancel_pad_launches.side_effect = cancel_owned
    audio.cancel_all_launches.side_effect = cancel_all_owned
    audio.admitted_launch_ids.return_value = [0]


def resident(controller: AppController, audio: Mock) -> Ticket:
    controller.project.sample_paths[0] = "samples/source.wav"
    controller.project.sample_durations[0] = 12.0
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 3.0
    controller.project.pad_loop_end_s[0] = 4.0
    audio.loaded_residency.return_value = {"source_identity": 123, "window_revision": 1}
    audio.current_input_runtime_pad_binding.side_effect = None
    audio.current_input_runtime_pad_binding.return_value = FakeInputRuntimePadBinding()
    ticket = Ticket()
    audio.prepare_resident_control = Mock(return_value=ticket)
    audio.play_resident_control = Mock(return_value=True)
    audio.cancel_pad_launches = Mock(return_value=False)
    audio.cancel_all_launches = Mock(return_value=[])
    audio.admitted_launch_ids = Mock(return_value=[])
    audio.reset_mock()
    return ticket


def legacy_start(controller: AppController, audio: Mock, received_at_ns: int) -> None:
    """Exercise latest-per-pad Residency.start compatibility, outside GUI capture."""
    start_s, end_s = controller.transport.loop.requested_region(0)
    generation = controller.transport.capture_current_pad_edit_generation(0)
    controller.transport.residency.start(
        0,
        start_s,
        end_s,
        Mock(),
        prepared_play=lambda ticket: audio.play_resident_control(
            ticket, exclusive=True, received_at_ns=received_at_ns
        ),
        expected_source_generation=generation,
    )


def schedule_captured_start(
    start: FakeCapturedPadStart, ticket: ResidentWindowTicket | None = None
) -> bool:
    """Retain an explicitly queued receipt without claiming a first Native effect."""
    if not start.publish(ticket):
        return False
    start.status = "scheduled"
    return True


def test_legacy_repeated_starts_preserve_failed_admission_budget_and_deadline(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    resident(controller, audio_engine_mock)
    audio_engine_mock.prepare_resident_control.side_effect = RuntimeError("cold source queue full")
    residency = controller.transport.residency
    legacy_start(controller, audio_engine_mock, 1)
    pending = residency._pending[0]
    deadline = pending.deadline
    for attempt in range(1, 9):
        for gesture in range(10):
            legacy_start(controller, audio_engine_mock, attempt * 10 + gesture)
            assert residency._pending[0] is pending
            assert pending.attempts == attempt
            assert pending.deadline == deadline
        residency.poll()
    assert audio_engine_mock.prepare_resident_control.call_count == 8
    assert residency.status(0) == (None, "resident admission retry limit reached")
    audio_engine_mock.play_resident_control.assert_not_called()


def test_legacy_retired_pending_ticket_requires_fresh_ack_for_latest_start(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old = resident(controller, audio_engine_mock)
    new = Ticket()
    audio_engine_mock.prepare_resident_control.side_effect = [old, new]
    legacy_start(controller, audio_engine_mock, 1)
    old.current = False
    legacy_start(controller, audio_engine_mock, 2)
    assert audio_engine_mock.prepare_resident_control.call_count == 2
    old.status = "accepted"
    controller.transport.residency.poll()
    audio_engine_mock.play_resident_control.assert_not_called()
    new.status = "accepted"
    controller.transport.residency.poll()
    audio_engine_mock.play_resident_control.assert_called_once_with(
        new, exclusive=True, received_at_ns=2
    )


@pytest.mark.parametrize("ack_before_click", [False, True])
def test_legacy_retired_launch_preserves_actual_ack_geometry_after_failed_replacement(
    controller: AppController, audio_engine_mock: Mock, *, ack_before_click: bool
) -> None:
    old = resident(controller, audio_engine_mock)
    controller.transport.loop.set_end(0, 7.0)
    old.current = False
    if ack_before_click:
        old.status = "accepted"
    audio_engine_mock.loaded_residency.return_value["window_revision"] = old.window_revision
    audio_engine_mock.prepare_resident_control.side_effect = RuntimeError(
        "resident stem publication is pending"
    )
    residency = controller.transport.residency
    legacy_start(controller, audio_engine_mock, 2)
    old.status = "accepted"
    residency.poll()
    residency._pending[0].deadline = 0.0
    residency.poll()
    assert residency.status(0) == (None, "resident admission retry limit reached")
    assert controller.project.pad_loop_end_s[0] == 7.0
    audio_engine_mock.play_resident_control.assert_not_called()


@pytest.mark.parametrize("changed_owner", ["source", "authority", "window"])
def test_retired_launch_ack_cannot_project_geometry_into_another_native_owner(
    controller: AppController, audio_engine_mock: Mock, changed_owner: str
) -> None:
    ticket = resident(controller, audio_engine_mock)
    residency = controller.transport.residency
    owner = residency._owner(0)
    ticket.status, ticket.current = "accepted", False
    audio_engine_mock.loaded_residency.return_value["window_revision"] = ticket.window_revision
    if changed_owner == "source":
        audio_engine_mock.loaded_residency.return_value["source_identity"] = 456
    elif changed_owner == "authority":
        audio_engine_mock.current_input_runtime_pad_binding.return_value = (
            FakeInputRuntimePadBinding(authority_revision=2)
        )
    else:
        audio_engine_mock.loaded_residency.return_value["window_revision"] += 1
    assert not residency._acknowledged_window(
        0, audio_engine_mock.prepare_resident_control.return_value, owner
    )


def test_legacy_new_start_after_unconfirmed_adoption_requires_a_fresh_ticket_and_ack(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old = resident(controller, audio_engine_mock)
    fresh = Ticket()
    audio_engine_mock.prepare_resident_control.side_effect = [
        old,
        RuntimeError("resident native adoption is in progress"),
        fresh,
    ]
    residency = controller.transport.residency
    legacy_start(controller, audio_engine_mock, 1)
    original = residency._pending[0]
    old.status = "adopting"
    original.deadline = 0.0
    residency.poll()
    assert original.unconfirmed
    assert original.after_ack is None
    legacy_start(controller, audio_engine_mock, 2)
    assert residency._pending[0] is not original
    old.status = "accepted"
    residency.poll()
    assert residency._pending[0].ticket is not None
    assert residency._pending[0].ticket is not original.ticket
    audio_engine_mock.play_resident_control.assert_not_called()
    fresh.status = "accepted"
    residency.poll()
    audio_engine_mock.play_resident_control.assert_called_once_with(
        fresh, exclusive=True, received_at_ns=2
    )


def test_loop_requested_and_effective_separate_until_matching_ack(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.loop.set_start(0, 6.0)
    assert controller.project.pad_loop_start_s[0] == 6.0
    assert controller.transport.loop.effective_region(0) == (3.0, 4.0)
    assert controller.transport.residency.status(0) == ("preparing", None)
    audio_engine_mock.set_pad_loop_region.assert_not_called()
    controller.transport.residency.poll()
    assert controller.transport.loop.effective_region(0) == (3.0, 4.0)
    ticket.status = "accepted"
    controller.transport.residency.poll()
    assert controller.transport.loop.effective_region(0) == (6.0, 6.0 + 1.0 / 44_100)
    assert controller.transport.residency.status(0) == (None, None)


@pytest.mark.parametrize("status", ["failed", "rejected", "cancelled"])
def test_terminal_failure_restores_previous_intent_without_touching_timing_stems(
    controller: AppController, audio_engine_mock: Mock, status: str
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.loop.set_full_track_region(0)
    ticket.status, ticket.message = status, "complete-source preparation failed"
    controller.transport.residency.poll()
    assert controller.project.pad_loop_start_s[0] == 3.0
    assert controller.project.pad_loop_end_s[0] == 4.0
    assert controller.transport.residency.status(0) == (None, ticket.message)
    audio_engine_mock.set_pad_timing_metadata.assert_not_called()
    audio_engine_mock.set_pad_bpm.assert_not_called()
    audio_engine_mock.set_stem_mix_mode.assert_not_called()


def test_latest_intent_owns_ack_and_failure_baseline(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old = resident(controller, audio_engine_mock)
    new = Ticket()
    audio_engine_mock.prepare_resident_control.side_effect = [old, new]
    controller.transport.loop.set_end(0, 7.0)
    controller.transport.loop.set_end(0, 9.0)
    old.status = "accepted"
    controller.transport.residency.poll()
    assert controller.transport.loop.effective_region(0) == (3.0, 4.0)
    assert controller.project.pad_loop_end_s[0] == 9.0
    new.status = "failed"
    controller.transport.residency.poll()
    assert controller.project.pad_loop_end_s[0] == 4.0


@pytest.mark.parametrize("change", ["source", "authority", "manual", "unload", "new-path"])
def test_stale_ack_cannot_restore_or_project_into_new_owner(
    controller: AppController, audio_engine_mock: Mock, change: str
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.loop.set_end(0, 7.0)
    if change == "source":
        audio_engine_mock.loaded_residency.return_value["source_identity"] = 456
    elif change == "authority":
        audio_engine_mock.current_input_runtime_pad_binding.return_value = (
            FakeInputRuntimePadBinding(authority_revision=2)
        )
    elif change == "manual":
        controller.project.manual_bpm[0] = 123.0
    elif change == "unload":
        controller.project.sample_paths[0] = None
    else:
        controller.project.sample_paths[0] = "samples/new.wav"
    ticket.status = "accepted"
    controller.transport.residency.poll()
    assert controller.project.pad_loop_end_s[0] == 7.0
    assert controller.transport.residency.status(0) == (None, None)
    audio_engine_mock.play_sample.assert_not_called()


def test_cancel_unclaimed_restores_but_claimed_tail_keeps_pending(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.loop.set_end(0, 7.0)
    assert controller.transport.residency.cancel_requested(0)
    assert controller.project.pad_loop_end_s[0] == 4.0
    ticket.status = "adopting"
    controller.transport.loop.set_end(0, 9.0)
    assert not controller.transport.residency.cancel_requested(0)
    assert controller.project.pad_loop_end_s[0] == 9.0
    assert controller.transport.residency.status(0)[0] == "adopting"


@pytest.mark.parametrize(
    "error",
    ["resident command queue is full", "cold source queue full (2 workers, 32 queued jobs)"],
)
def test_capacity_retry_is_bounded_and_keeps_previous_effective_audio(
    controller: AppController, audio_engine_mock: Mock, error: str
) -> None:
    resident(controller, audio_engine_mock)
    audio_engine_mock.prepare_resident_control.side_effect = RuntimeError(error)
    controller.transport.loop.set_end(0, 7.0)
    for _ in range(20):
        controller.transport.residency.poll()
    assert audio_engine_mock.prepare_resident_control.call_count == 8
    assert controller.project.pad_loop_end_s[0] == 4.0
    assert controller.transport.residency.status(0) == (
        None,
        "resident admission retry limit reached",
    )


def test_claimed_previous_ack_becomes_baseline_before_latest_retry(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    first = resident(controller, audio_engine_mock)
    next_ticket = Ticket()
    audio_engine_mock.prepare_resident_control.side_effect = [
        first,
        RuntimeError("resident native adoption is in progress"),
        next_ticket,
    ]
    controller.transport.loop.set_end(0, 7.0)
    first.status = "adopting"
    controller.transport.loop.set_end(0, 9.0)
    controller.transport.residency.poll()
    assert audio_engine_mock.prepare_resident_control.call_count == 2
    first.status = "accepted"
    controller.transport.residency.poll()
    next_ticket.status = "failed"
    controller.transport.residency.poll()
    assert controller.project.pad_loop_end_s[0] == 7.0


def test_seek_preserves_markers_pause_and_uses_actual_ack_clamp(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.session.active_sample_ids.add(0)
    controller.session.paused_sample_ids.add(0)
    controller.session.pad_playhead_s[0] = 3.5
    controller.transport.playback.seek_pad(0, 1000.0)
    audio_engine_mock.prepare_resident_control.assert_called_once_with(
        0, start_s=None, end_s=None, position_s=1000.0, key_lock=None, expected_source_generation=1
    )
    assert controller.session.pad_playhead_s[0] == 3.5
    ticket.status, ticket.effective_seek_seconds = "accepted", 30.0
    controller.transport.residency.poll()
    assert controller.session.pad_playhead_s[0] == 30.0
    assert 0 in controller.session.paused_sample_ids
    assert controller.project.pad_loop_start_s[0] == 3.0
    assert controller.project.pad_loop_end_s[0] == 4.0


def test_stopped_seek_does_not_admit_complete_context(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    resident(controller, audio_engine_mock)
    controller.transport.playback.seek_pad(0, 0.0)
    audio_engine_mock.prepare_resident_control.assert_not_called()


def test_trigger_preserves_input_timestamp_and_launches_only_after_current_ack(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.playback.trigger_pad(0, received_at_ns=12345)
    original = audio_engine_mock.prepare_captured_pad_start.call_args.args[0]
    assert isinstance(original, FakeCapturedPadStart)
    assert original.received_at_ns == 12345
    assert original.exclusive is True
    assert original.loop_region == (3.0, 4.0)
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    audio_engine_mock.publish_pad_start.assert_not_called()
    ticket.status = "accepted"
    controller.transport.residency.poll()
    audio_engine_mock.publish_pad_start.assert_called_once_with(original, ticket)
    audio_engine_mock.capture_pad_start.assert_called_once_with(
        0,
        3.0,
        4.0,
        1.0,
        exclusive=True,
        received_at_ns=12345,
        expected_source_generation=1,
    )
    audio_engine_mock.play_resident_control.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()


@pytest.mark.parametrize("status", ["preparing", "pending", "adopting"])
def test_repeated_identical_legacy_start_keeps_preparation_and_latest_input_timestamp(
    controller: AppController, audio_engine_mock: Mock, status: str
) -> None:
    ticket = resident(controller, audio_engine_mock)
    ticket.status = status
    legacy_start(controller, audio_engine_mock, 123)
    pending = controller.transport.residency._pending[0]
    for timestamp in range(124, 144):
        legacy_start(controller, audio_engine_mock, timestamp)
    assert controller.transport.residency._pending[0] is pending
    assert audio_engine_mock.prepare_resident_control.call_count == 1
    audio_engine_mock.play_resident_control.assert_not_called()
    ticket.status = "accepted"
    controller.transport.residency.poll()
    audio_engine_mock.play_resident_control.assert_called_once_with(
        ticket, exclusive=True, received_at_ns=143
    )


def test_legacy_start_observes_received_ack_without_waiting_for_another_frame(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    legacy_start(controller, audio_engine_mock, 123)
    ticket.status = "accepted"
    legacy_start(controller, audio_engine_mock, 456)
    assert audio_engine_mock.prepare_resident_control.call_count == 1
    audio_engine_mock.play_resident_control.assert_called_once_with(
        ticket, exclusive=True, received_at_ns=456
    )


@pytest.mark.parametrize("change", ["region", "key-lock", "source", "authority"])
def test_changed_legacy_start_context_still_admits_new_preparation(
    controller: AppController, audio_engine_mock: Mock, change: str
) -> None:
    old = resident(controller, audio_engine_mock)
    legacy_start(controller, audio_engine_mock, 123)
    if change == "region":
        controller.project.pad_loop_end_s[0] = 5.0
    elif change == "key-lock":
        controller.project.pad_key_lock[0] = True
    elif change == "source":
        audio_engine_mock.loaded_residency.return_value["source_identity"] = 456
    else:
        audio_engine_mock.current_input_runtime_pad_binding.return_value = (
            FakeInputRuntimePadBinding(authority_revision=2)
        )
    new = Ticket()
    audio_engine_mock.prepare_resident_control.return_value = new
    legacy_start(controller, audio_engine_mock, 456)
    assert audio_engine_mock.prepare_resident_control.call_count == 2
    old.status = "accepted"
    controller.transport.residency.poll()
    audio_engine_mock.play_resident_control.assert_not_called()
    new.status = "accepted"
    if change == "key-lock":
        new.key_lock_request_id = 2
    controller.transport.residency.poll()
    if change == "key-lock":
        # This own Window ACK establishes geometry. An ON launch also needs the
        # actual current source's prepared Native mode acknowledgement.
        audio_engine_mock.play_resident_control.assert_not_called()
        assert controller.transport.residency.key_lock_status(0).pending
        audio_engine_mock.pad_key_lock_status.return_value = {
            "source_id": "loaded-0-1",
            "source_generation": 1,
            "source_identity": 123,
            "window_revision": new.window_revision,
            "request_id": new.key_lock_request_id,
            "effective": True,
            "ready": True,
        }
        controller.transport.residency.poll()
    audio_engine_mock.play_resident_control.assert_called_once_with(
        new, exclusive=True, received_at_ns=456
    )


def test_shutdown_invalidates_pending_launch_without_reopening_admission(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.playback.trigger_pad(0, received_at_ns=12345)
    controller.transport.residency.shut_down()
    ticket.status = "accepted"
    controller.transport.residency.poll()
    controller.transport.playback.trigger_pad(0, received_at_ns=6789)
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    audio_engine_mock.play_resident_control.assert_not_called()
    audio_engine_mock.publish_pad_start.assert_not_called()
    assert audio_engine_mock.prepare_resident_control.call_count == 1
    assert audio_engine_mock.prepare_captured_pad_start.call_count == 1


def test_key_lock_context_shares_loop_transaction_and_retains_rejected_intent(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    audio_engine_mock.prepare_resident_control.assert_called_once_with(
        0, start_s=None, end_s=None, position_s=None, key_lock=True, expected_source_generation=1
    )
    assert controller.project.pad_key_lock[0]
    ticket.status = "failed"
    controller.transport.residency.poll()
    assert controller.project.pad_key_lock[0] is True
    audio_engine_mock.set_pad_key_lock.assert_not_called()


@pytest.mark.parametrize("stop", ["pad", "global"])
@pytest.mark.parametrize("claimed", [False, True])
def test_stop_before_start_ack_revokes_launch_even_with_no_active_voice(
    controller: AppController, audio_engine_mock: Mock, stop: str, *, claimed: bool
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
    original = audio_engine_mock.prepare_captured_pad_start.call_args.args[0]
    if claimed:
        ticket.status = "adopting"
    if stop == "pad":
        controller.transport.playback.stop_pad(0)
    else:
        controller.transport.playback.stop_all_pads()
    ticket.status = "accepted"
    controller.transport.residency.poll()
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    audio_engine_mock.play_resident_control.assert_not_called()
    audio_engine_mock.publish_pad_start.assert_not_called()
    assert original.state() == "rejected"
    audio_engine_mock.capture_pad_start.assert_called_once()
    assert not controller.session.active_sample_ids


def test_ack_before_latest_admission_is_failure_baseline(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old = resident(controller, audio_engine_mock)
    new = Ticket()
    audio_engine_mock.prepare_resident_control.side_effect = [old, new]
    controller.transport.loop.set_end(0, 7.0)
    old.status = "accepted"
    controller.transport.loop.set_end(0, 9.0)
    new.status = "failed"
    controller.transport.residency.poll()
    assert controller.project.pad_loop_end_s[0] == 7.0


def test_third_intent_keeps_claimed_first_ack_baseline(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old = resident(controller, audio_engine_mock)
    new = Ticket()
    audio_engine_mock.prepare_resident_control.side_effect = [
        old,
        RuntimeError("resident native adoption is in progress"),
        RuntimeError("resident native adoption is in progress"),
        new,
    ]
    controller.transport.loop.set_end(0, 7.0)
    old.status = "adopting"
    controller.transport.loop.set_end(0, 9.0)
    controller.transport.loop.set_end(0, 11.0)
    old.status = "accepted"
    controller.transport.residency.poll()
    new.status = "failed"
    controller.transport.residency.poll()
    assert controller.project.pad_loop_end_s[0] == 7.0


def test_post_ack_launch_failure_reports_error_and_keeps_adopted_loop(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    audio_engine_mock.publish_pad_start.side_effect = RuntimeError("launch source retired")
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
    ticket.status = "accepted"
    controller.transport.residency.poll()
    assert controller.transport.residency.status(0) == (None, "launch source retired")
    original = audio_engine_mock.prepare_captured_pad_start.call_args.args[0]
    audio_engine_mock.publish_pad_start.assert_called_once_with(original, ticket)
    audio_engine_mock.play_resident_control.assert_not_called()
    assert controller.project.pad_loop_end_s[0] == 4.0


@pytest.mark.parametrize(
    "message",
    [
        "command queue is full",
        "Failed to send PlaySampleExclusive - buffer may be full",
        "Failed to send PlaySample - buffer may be full",
    ],
)
def test_post_ack_launch_capacity_retry_is_bounded_and_preserves_input_time(
    controller: AppController, audio_engine_mock: Mock, message: str
) -> None:
    ticket = resident(controller, audio_engine_mock)
    audio_engine_mock.publish_pad_start.side_effect = RuntimeError(message)
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
    ticket.status = "accepted"
    for _ in range(20):
        controller.transport.residency.poll()
    original = audio_engine_mock.prepare_captured_pad_start.call_args.args[0]
    # One preparation and seven publication attempts share the bounded budget.
    assert audio_engine_mock.publish_pad_start.call_count == 7
    assert all(
        attempt.args == (original, ticket)
        for attempt in audio_engine_mock.publish_pad_start.call_args_list
    )
    assert original.received_at_ns == 123
    audio_engine_mock.capture_pad_start.assert_called_once()
    assert controller.transport.residency.status(0) == (
        None,
        message,
    )
    audio_engine_mock.play_resident_control.assert_not_called()


def test_midi_unprepared_context_uses_own_ack_route_and_original_guard(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.input_mapping.set_enabled(enabled=True)
    original = captured_midi(audio_engine_mock, ticket, 456)
    audio_engine_mock.trigger_prepared_input_pad.return_value = True
    controller.input_mapping._handle_rust_input_event(midi_event(original))
    residency = controller.transport.residency
    retained = residency._captured_triggers[id(original)]
    assert retained.trigger is original.native_trigger
    residency.poll()
    audio_engine_mock.prepare_captured_input_trigger.assert_called_once_with(original)
    assert retained.ticket is ticket.native_ticket
    audio_engine_mock.prepare_resident_control.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    audio_engine_mock.play_resident_control.assert_not_called()
    audio_engine_mock.trigger_captured_input_pad.assert_called_once_with(original)
    audio_engine_mock.trigger_prepared_input_pad.assert_not_called()
    audio_engine_mock.trigger_input_runtime_pad.assert_not_called()
    ticket.status = "accepted"
    ticket.current = False  # Its own accepted window does not recapture an old original window.
    controller.transport.residency.poll()
    audio_engine_mock.trigger_prepared_input_pad.assert_called_once_with(original, ticket)
    assert original.native_intent == ("midi:note:1:60", 456, True, original.owner_identity)
    assert not residency._captured_triggers


def test_global_start_waits_for_pending_region_without_splitting_native_batch(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.session.active_sample_ids.add(0)
    controller.transport.loop.set_end(0, 7.0)
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=123)
    original = audio_engine_mock.publish_captured_global_start.call_args.args[0]
    assert isinstance(original, FakeCapturedGlobalStart)
    assert original.entries == tuple(audio_engine_mock.capture_global_start.call_args.args[0])
    assert original.entries[0][1:] == (3.0, 7.0, ticket)
    assert original.received_at_ns == 123
    assert original.state() == "captured"
    ticket.status = "accepted"
    controller.transport.residency.poll()
    controller.transport.playback._global_playback.poll()
    audio_engine_mock.capture_global_start.assert_called_once()
    assert audio_engine_mock.publish_captured_global_start.call_count == 2
    assert all(
        entry.args == (original,)
        for entry in audio_engine_mock.publish_captured_global_start.call_args_list
    )
    assert original.state() == "applied"
    audio_engine_mock.start_global_playback_batch.assert_not_called()


@pytest.mark.parametrize("ready", [False, True])
def test_midi_post_ack_pressure_retries_original_guard_with_same_timestamp(
    controller: AppController, audio_engine_mock: Mock, *, ready: bool
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.input_mapping.set_enabled(enabled=True)
    original = captured_midi(audio_engine_mock, ticket, 456)
    outcomes = iter([False, True]) if ready else None
    observed: list[tuple[object, ...]] = []

    def native_launch(token: object, own_window: object) -> bool:
        assert token is original
        assert own_window is ticket
        observed.append(original.native_intent)
        return next(outcomes) if outcomes is not None else False

    audio_engine_mock.trigger_prepared_input_pad.side_effect = native_launch
    controller.input_mapping._handle_rust_input_event(midi_event(original))
    controller.transport.residency.poll()
    pending = controller.transport.residency._captured_triggers[id(original)]
    assert pending.ticket is ticket.native_ticket
    ticket.status = "accepted"
    for _ in range(20):
        controller.transport.residency.poll()
    audio_engine_mock.trigger_captured_input_pad.assert_called_once_with(original)
    assert audio_engine_mock.trigger_prepared_input_pad.call_count == (2 if ready else 8)
    assert observed == [original.native_intent] * (2 if ready else 8)
    assert original.native_intent == ("midi:note:1:60", 456, True, original.owner_identity)
    if ready:
        assert not controller.transport.residency._captured_triggers
    else:
        assert controller.transport.residency._captured_triggers[id(original)] is pending
        assert pending.ticket is ticket.native_ticket
        assert pending.cancel_requested
        assert ticket.launch_cancelled
        original.owner_status = "superseded"
        controller.transport.residency.poll()
        assert not controller.transport.residency._captured_triggers
    audio_engine_mock.prepare_captured_input_trigger.assert_called_once_with(original)
    audio_engine_mock.trigger_input_runtime_pad.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    audio_engine_mock.play_resident_control.assert_not_called()


def test_claimed_predecessor_deadline_reports_unconfirmed_without_false_rollback(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old = resident(controller, audio_engine_mock)
    audio_engine_mock.prepare_resident_control.side_effect = [
        old,
        RuntimeError("resident native adoption is in progress"),
    ]
    controller.transport.loop.set_end(0, 7.0)
    old.status = "adopting"
    controller.transport.loop.set_end(0, 9.0)
    controller.transport.residency._pending[0].deadline = 0.0
    controller.transport.residency.poll()
    assert controller.project.pad_loop_end_s[0] == 9.0
    assert controller.transport.residency.status(0) == (
        "unconfirmed",
        "resident native adoption is unconfirmed",
    )
    old.status = "accepted"
    controller.transport.residency.poll()
    assert controller.project.pad_loop_end_s[0] == 7.0
    assert audio_engine_mock.prepare_resident_control.call_count == 2


@pytest.mark.parametrize("status", ["preparing", "pending"])
def test_unclaimed_current_ticket_deadline_cancels_and_restores_effective_region(
    controller: AppController, audio_engine_mock: Mock, status: str
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.loop.set_end(0, 7.0)
    ticket.status = status
    controller.transport.residency._pending[0].deadline = 0.0
    controller.transport.residency.poll()
    assert controller.project.pad_loop_end_s[0] == 4.0
    assert controller.transport.residency.status(0) == (
        None,
        "resident preparation deadline expired",
    )


def test_legacy_claimed_current_ticket_deadline_keeps_ownership_but_revokes_launch(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    legacy_start(controller, audio_engine_mock, 123)
    ticket.status = "adopting"
    controller.transport.residency._pending[0].deadline = 0.0
    controller.transport.residency.poll()
    assert controller.transport.residency.status(0) == (
        "unconfirmed",
        "resident native adoption is unconfirmed",
    )
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    audio_engine_mock.play_resident_control.assert_not_called()
    ticket.status = "accepted"
    controller.transport.residency.poll()
    assert controller.transport.residency.status(0) == (None, None)
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    audio_engine_mock.play_resident_control.assert_not_called()


@pytest.mark.parametrize("status", ["preparing", "pending"])
def test_unclaimed_predecessor_deadline_cancels_before_restoring_region(
    controller: AppController, audio_engine_mock: Mock, status: str
) -> None:
    old = resident(controller, audio_engine_mock)
    audio_engine_mock.prepare_resident_control.side_effect = [
        old,
        RuntimeError("resident native adoption is in progress"),
    ]
    controller.transport.loop.set_end(0, 7.0)
    old.status = status
    controller.transport.loop.set_end(0, 9.0)
    controller.transport.residency._pending[0].deadline = 0.0
    controller.transport.residency.poll()
    assert old.status == "cancelled"
    assert controller.project.pad_loop_end_s[0] == 4.0
    assert controller.transport.residency.status(0) == (
        None,
        "resident preparation deadline expired",
    )


def test_predecessor_claimed_during_deadline_cancel_does_not_roll_back(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old = resident(controller, audio_engine_mock)
    audio_engine_mock.prepare_resident_control.side_effect = [
        old,
        RuntimeError("resident native adoption is in progress"),
    ]
    controller.transport.loop.set_end(0, 7.0)
    controller.transport.loop.set_end(0, 9.0)

    def claim_in_cancel() -> bool:
        old.status = "adopting"
        return False

    controller.transport.residency._pending[0].deadline = 0.0
    with patch.object(old, "cancel", claim_in_cancel):
        controller.transport.residency.poll()
    assert controller.project.pad_loop_end_s[0] == 9.0
    assert controller.transport.residency.status(0) == (
        "unconfirmed",
        "resident native adoption is unconfirmed",
    )
    old.status = "accepted"
    controller.transport.residency.poll()
    assert controller.project.pad_loop_end_s[0] == 7.0


@pytest.mark.parametrize("succeeds", [False, True])
def test_deferred_global_queue_pressure_is_bounded_and_retains_input_timestamp(
    controller: AppController, audio_engine_mock: Mock, *, succeeds: bool
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.session.active_sample_ids.add(0)
    controller.transport.loop.set_end(0, 7.0)
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=123)
    original = audio_engine_mock.publish_captured_global_start.call_args.args[0]
    assert isinstance(original, FakeCapturedGlobalStart)
    message = "Failed to send global playback batch - buffer may be full"
    accepted = FakeGlobalPlaybackBatchTicket()
    attempts = 0

    def publish(capture: FakeCapturedGlobalStart) -> FakeGlobalPlaybackBatchTicket | None:
        nonlocal attempts
        assert capture is original
        attempts += 1
        if succeeds and attempts == 2:
            return capture.publish(accepted)
        raise RuntimeError(message)

    audio_engine_mock.publish_captured_global_start.side_effect = publish
    ticket.status = "accepted"
    controller.transport.residency.poll()
    for _ in range(20):
        controller.transport.playback._global_playback.poll()
    assert attempts == (2 if succeeds else 8)
    assert audio_engine_mock.publish_captured_global_start.call_count == attempts + 1
    assert all(
        entry.args == (original,)
        for entry in audio_engine_mock.publish_captured_global_start.call_args_list
    )
    audio_engine_mock.capture_global_start.assert_called_once()
    assert original.received_at_ns == 123
    assert original.entries[0][1:] == (3.0, 7.0, ticket)
    audio_engine_mock.start_global_playback_batch.assert_not_called()
    assert controller.transport.playback._global_playback._deferred is None
    if not succeeds:
        assert controller.transport.residency.status(0) == (None, message)
    assert controller.project.pad_loop_end_s[0] == 7.0


def test_new_global_gesture_supersedes_ready_deferred_timestamp(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.session.active_sample_ids.add(0)
    controller.transport.loop.set_end(0, 7.0)
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=123)
    first = audio_engine_mock.publish_captured_global_start.call_args.args[0]
    assert isinstance(first, FakeCapturedGlobalStart)
    ticket.status = "accepted"
    controller.transport.residency.poll()
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=456)
    second = audio_engine_mock.publish_captured_global_start.call_args.args[0]
    assert isinstance(second, FakeCapturedGlobalStart)
    assert second is not first
    assert first.cancelled is True
    assert first.received_at_ns == 123
    assert second.received_at_ns == 456
    assert audio_engine_mock.capture_global_start.call_count == 2
    assert audio_engine_mock.publish_captured_global_start.call_count == 2
    assert audio_engine_mock.capture_global_start.call_args.kwargs["received_at_ns"] == 456
    assert second.entries[0][1:] == (3.0, 7.0, None)
    audio_engine_mock.start_global_playback_batch.assert_not_called()


def test_deferred_global_deadline_exposes_error_without_launching(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.session.active_sample_ids.add(0)
    controller.transport.loop.set_end(0, 7.0)
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=123)
    global_controller = controller.transport.playback._global_playback
    deferred = global_controller._deferred
    assert deferred is not None
    original = deferred.capture
    global_controller._deferred = replace(deferred, deadline=0.0)
    global_controller.poll()
    ticket.status = "accepted"
    controller.transport.residency.poll()
    global_controller.poll()
    audio_engine_mock.start_global_playback_batch.assert_not_called()
    audio_engine_mock.capture_global_start.assert_called_once()
    audio_engine_mock.publish_captured_global_start.assert_called_once_with(original)
    assert isinstance(original, FakeCapturedGlobalStart)
    assert original.cancelled is True
    assert original.received_at_ns == 123
    assert global_controller._deferred is None
    assert controller.transport.residency.status(0) == (
        None,
        "global playback readiness deadline expired",
    )


def test_post_ack_ui_launch_keeps_opaque_ticket_and_rejects_retired_source(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)

    def retire_original(start: FakeCapturedPadStart, window: Ticket) -> bool:
        assert start.window is window.native_ticket
        start.status = "rejected"
        start.failure = "source_changed"
        return False

    audio_engine_mock.publish_pad_start.side_effect = retire_original
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
    ticket.status = "accepted"
    controller.transport.residency.poll()
    controller.transport.residency.poll()
    original = audio_engine_mock.prepare_captured_pad_start.call_args.args[0]
    audio_engine_mock.publish_pad_start.assert_called_once_with(original, ticket)
    assert original.received_at_ns == 123
    audio_engine_mock.capture_pad_start.assert_called_once()
    audio_engine_mock.play_resident_control.assert_not_called()
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    assert controller.transport.residency.status(0) == (
        None,
        "source_changed",
    )


@pytest.mark.parametrize("stop", ["pad", "global"])
@pytest.mark.parametrize("input_kind", ["ui", "midi"])
def test_stop_after_ack_revokes_queued_launch_before_active_feedback(
    controller: AppController, audio_engine_mock: Mock, stop: str, input_kind: str
) -> None:
    ticket = resident(controller, audio_engine_mock)
    if input_kind == "ui":
        audio_engine_mock.publish_pad_start.side_effect = schedule_captured_start
        audio_engine_mock.cancel_all_launches.return_value = [0]
        controller.transport.playback.trigger_pad(0, received_at_ns=123)
        original = audio_engine_mock.prepare_captured_pad_start.call_args.args[0]
    else:
        controller.input_mapping.set_enabled(enabled=True)
        original = captured_midi(audio_engine_mock, ticket, 123)
        audio_engine_mock.trigger_prepared_input_pad.return_value = True
        controller.input_mapping._handle_rust_input_event(midi_event(original))
        controller.transport.residency.poll()
        audio_engine_mock.prepare_captured_input_trigger.assert_called_once_with(original)
        native_cancel_captured_launch(audio_engine_mock, original, ticket)
    ticket.status = "accepted"
    controller.transport.residency.poll()
    assert controller.transport.residency.status(0) == (
        "pending" if input_kind == "ui" else None,
        None,
    )
    assert not controller.session.active_sample_ids
    if input_kind == "midi":
        audio_engine_mock.trigger_captured_input_pad.assert_called_once_with(original)
        audio_engine_mock.trigger_prepared_input_pad.assert_called_once_with(original, ticket)
        audio_engine_mock.trigger_input_runtime_pad.assert_not_called()
        audio_engine_mock.play_resident_control.assert_not_called()
        assert not controller.transport.residency._captured_triggers
    if stop == "pad":
        controller.transport.playback.stop_pad(0)
        audio_engine_mock.cancel_pad_launches.assert_called_once_with(0)
        audio_engine_mock.stop_sample.assert_called_once_with(0)
    else:
        controller.transport.playback.stop_all_pads()
        audio_engine_mock.cancel_all_launches.assert_called_once_with()
        audio_engine_mock.stop_global_playback_batch.assert_called_once()
        bindings = audio_engine_mock.stop_global_playback_batch.call_args.args[0]
        assert [binding.metadata()["pad_id"] for binding in bindings] == [0]
    assert ticket.launch_cancelled
    if input_kind == "midi":
        assert original.owner_status == "superseded"
    else:
        assert original.state() == "rejected"
    assert ticket.status == "accepted"
    assert ticket.is_current()
    assert not controller.transport.residency._launched
    assert controller.project.pad_loop_start_s[0] == 3.0


@pytest.mark.parametrize("retire", ["unload", "shutdown"])
def test_legacy_adopted_launch_retires_with_its_ticket_on_unload_and_all_on_shutdown(
    controller: AppController, audio_engine_mock: Mock, retire: str
) -> None:
    ticket = resident(controller, audio_engine_mock)
    legacy_start(controller, audio_engine_mock, 123)
    ticket.status = "accepted"
    controller.transport.residency.poll()
    audio_engine_mock.loaded_residency.return_value["source_identity"] = 456
    if retire == "unload":
        controller.transport.residency.cancel(0)
    else:
        controller.transport.residency.shut_down()
    assert ticket.launch_cancelled
    assert ticket.status == "accepted"
    assert not controller.transport.residency._launched
    audio_engine_mock.cancel_pad_launches.assert_not_called()
    if retire == "unload":
        audio_engine_mock.cancel_all_launches.assert_not_called()
    else:
        audio_engine_mock.cancel_all_launches.assert_called_once_with()


@pytest.mark.parametrize("status", ["pending", "accepted"])
def test_global_stop_targets_claimed_start_before_active_feedback(
    controller: AppController, audio_engine_mock: Mock, status: str
) -> None:
    resident(controller, audio_engine_mock)
    controller.session.active_sample_ids.add(0)
    start_ticket = FakeGlobalPlaybackBatchTicket(status)
    audio_engine_mock.publish_captured_global_start.side_effect = lambda capture: capture.publish(
        start_ticket
    )
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=123)
    original = audio_engine_mock.publish_captured_global_start.call_args.args[0]
    assert isinstance(original, FakeCapturedGlobalStart)
    assert original.received_at_ns == 123
    controller.session.active_sample_ids.clear()
    controller.transport.playback.stop_all_pads(received_at_ns=456)
    audio_engine_mock.cancel_all_launches.assert_called_once_with()
    audio_engine_mock.stop_global_playback_batch.assert_called_once()
    bindings = audio_engine_mock.stop_global_playback_batch.call_args.args[0]
    assert [binding.metadata()["pad_id"] for binding in bindings] == [0]
    assert audio_engine_mock.stop_global_playback_batch.call_args.kwargs["received_at_ns"] == 456


@pytest.mark.parametrize("stop", ["pad", "global"])
def test_stop_uses_native_direct_midi_target_before_input_or_active_feedback(
    controller: AppController, audio_engine_mock: Mock, stop: str
) -> None:
    resident(controller, audio_engine_mock)
    audio_engine_mock.cancel_pad_launches.return_value = True
    audio_engine_mock.cancel_all_launches.return_value = [0]
    assert not controller.session.active_sample_ids
    assert not controller.transport.residency._launched
    if stop == "pad":
        controller.transport.playback.stop_pad(0)
        audio_engine_mock.cancel_pad_launches.assert_called_once_with(0)
        audio_engine_mock.stop_sample.assert_called_once_with(0)
    else:
        controller.transport.playback.stop_all_pads()
        audio_engine_mock.cancel_all_launches.assert_called_once_with()
        audio_engine_mock.stop_global_playback_batch.assert_called_once()
        bindings = audio_engine_mock.stop_global_playback_batch.call_args.args[0]
        assert [binding.metadata()["pad_id"] for binding in bindings] == [0]
    audio_engine_mock.admitted_launch_ids.assert_not_called()
