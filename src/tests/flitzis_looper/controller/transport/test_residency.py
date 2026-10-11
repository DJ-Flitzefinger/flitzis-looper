"""Controller intent/ACK proofs; native PCM and ownership use separate real tests."""

from dataclasses import replace
from typing import TYPE_CHECKING
from unittest.mock import Mock, patch

import pytest

from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


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


def test_repeated_starts_preserve_failed_admission_budget_and_deadline(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    resident(controller, audio_engine_mock)
    audio_engine_mock.prepare_resident_control.side_effect = RuntimeError("cold source queue full")
    residency = controller.transport.residency
    controller.transport.playback.trigger_pad(0, received_at_ns=1)
    pending = residency._pending[0]
    deadline = pending.deadline
    for attempt in range(1, 9):
        for gesture in range(10):
            controller.transport.playback.trigger_pad(0, received_at_ns=attempt * 10 + gesture)
            assert residency._pending[0] is pending
            assert pending.attempts == attempt
            assert pending.deadline == deadline
        residency.poll()
    assert audio_engine_mock.prepare_resident_control.call_count == 8
    assert residency.status(0) == (None, "resident admission retry limit reached")
    audio_engine_mock.play_resident_control.assert_not_called()


def test_retired_pending_ticket_requires_fresh_ack_for_latest_start(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old = resident(controller, audio_engine_mock)
    new = Ticket()
    audio_engine_mock.prepare_resident_control.side_effect = [old, new]
    controller.transport.playback.trigger_pad(0, received_at_ns=1)
    old.current = False
    controller.transport.playback.trigger_pad(0, received_at_ns=2)
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
def test_retired_launch_preserves_actual_ack_geometry_after_failed_replacement(
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
    controller.transport.playback.trigger_pad(0, received_at_ns=2)
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


def test_new_click_after_unconfirmed_adoption_requires_a_fresh_ticket_and_ack(
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
    controller.transport.playback.trigger_pad(0, received_at_ns=1)
    original = residency._pending[0]
    old.status = "adopting"
    original.deadline = 0.0
    residency.poll()
    assert original.unconfirmed
    assert original.after_ack is None
    controller.transport.playback.trigger_pad(0, received_at_ns=2)
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
        0, start_s=None, end_s=None, position_s=1000.0, key_lock=None
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
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    audio_engine_mock.play_resident_control.assert_not_called()
    ticket.status = "accepted"
    controller.transport.residency.poll()
    audio_engine_mock.play_resident_control.assert_called_once_with(
        ticket, exclusive=True, received_at_ns=12345
    )
    audio_engine_mock.play_sample_exclusive.assert_not_called()


@pytest.mark.parametrize("status", ["preparing", "pending", "adopting"])
def test_repeated_identical_trigger_keeps_preparation_and_latest_input_timestamp(
    controller: AppController, audio_engine_mock: Mock, status: str
) -> None:
    ticket = resident(controller, audio_engine_mock)
    ticket.status = status
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
    pending = controller.transport.residency._pending[0]
    for timestamp in range(124, 144):
        controller.transport.playback.trigger_pad(0, received_at_ns=timestamp)
    assert controller.transport.residency._pending[0] is pending
    assert audio_engine_mock.prepare_resident_control.call_count == 1
    audio_engine_mock.play_resident_control.assert_not_called()
    ticket.status = "accepted"
    controller.transport.residency.poll()
    audio_engine_mock.play_resident_control.assert_called_once_with(
        ticket, exclusive=True, received_at_ns=143
    )


def test_trigger_observes_already_received_ack_without_waiting_for_another_frame(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
    ticket.status = "accepted"
    controller.transport.playback.trigger_pad(0, received_at_ns=456)
    assert audio_engine_mock.prepare_resident_control.call_count == 1
    audio_engine_mock.play_resident_control.assert_called_once_with(
        ticket, exclusive=True, received_at_ns=456
    )


@pytest.mark.parametrize("change", ["region", "key-lock", "source", "authority"])
def test_changed_trigger_context_still_admits_new_preparation(
    controller: AppController, audio_engine_mock: Mock, change: str
) -> None:
    old = resident(controller, audio_engine_mock)
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
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
    controller.transport.playback.trigger_pad(0, received_at_ns=456)
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
    assert audio_engine_mock.prepare_resident_control.call_count == 1


def test_key_lock_context_shares_loop_transaction_and_rolls_back_failure(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    audio_engine_mock.prepare_resident_control.assert_called_once_with(
        0, start_s=None, end_s=None, position_s=None, key_lock=True
    )
    assert controller.project.pad_key_lock[0]
    ticket.status = "failed"
    controller.transport.residency.poll()
    assert not controller.project.pad_key_lock[0]
    audio_engine_mock.set_pad_key_lock.assert_not_called()


@pytest.mark.parametrize("stop", ["pad", "global"])
@pytest.mark.parametrize("claimed", [False, True])
def test_stop_before_start_ack_revokes_launch_even_with_no_active_voice(
    controller: AppController, audio_engine_mock: Mock, stop: str, *, claimed: bool
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
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
    audio_engine_mock.play_resident_control.side_effect = RuntimeError("launch source retired")
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
    ticket.status = "accepted"
    controller.transport.residency.poll()
    assert controller.transport.residency.status(0) == (None, "launch source retired")
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
    audio_engine_mock.play_resident_control.side_effect = RuntimeError(message)
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
    ticket.status = "accepted"
    for _ in range(20):
        controller.transport.residency.poll()
    assert audio_engine_mock.play_resident_control.call_count == 8
    assert all(
        call.kwargs["received_at_ns"] == 123
        for call in audio_engine_mock.play_resident_control.call_args_list
    )
    assert controller.transport.residency.status(0) == (None, message)


def test_midi_unprepared_context_uses_same_ack_route_and_fresh_guard(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.input_mapping.set_enabled(enabled=True)
    audio_engine_mock.trigger_input_runtime_pad.side_effect = [False, True]
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.trigger:0",
        "received_at_ns": 456,
        "direct": True,
        "dispatched": False,
    })
    assert audio_engine_mock.prepare_resident_control.call_count == 1
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    audio_engine_mock.play_resident_control.assert_not_called()
    assert audio_engine_mock.trigger_input_runtime_pad.call_count == 1
    ticket.status = "accepted"
    controller.transport.residency.poll()
    assert audio_engine_mock.trigger_input_runtime_pad.call_count == 2
    audio_engine_mock.trigger_input_runtime_pad.assert_called_with(0, received_at_ns=456)


def test_global_start_waits_for_pending_region_without_splitting_native_batch(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.session.active_sample_ids.add(0)
    controller.transport.loop.set_end(0, 7.0)
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=123)
    audio_engine_mock.start_global_playback_batch.assert_not_called()
    ticket.status = "accepted"
    controller.transport.residency.poll()
    controller.transport.playback._global_playback.poll()
    audio_engine_mock.start_global_playback_batch.assert_called_once()
    entry = audio_engine_mock.start_global_playback_batch.call_args.args[0][0]
    assert entry[1:] == (3.0, 7.0)
    assert audio_engine_mock.start_global_playback_batch.call_args.kwargs["received_at_ns"] == 123


@pytest.mark.parametrize("ready", [False, True])
def test_midi_post_ack_pressure_retries_fresh_guard_with_same_timestamp(
    controller: AppController, audio_engine_mock: Mock, *, ready: bool
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.input_mapping.set_enabled(enabled=True)
    audio_engine_mock.trigger_input_runtime_pad.side_effect = (
        [False, False, True] if ready else None
    )
    audio_engine_mock.trigger_input_runtime_pad.return_value = False
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.trigger:0",
        "received_at_ns": 456,
        "direct": True,
        "dispatched": False,
    })
    ticket.status = "accepted"
    for _ in range(20):
        controller.transport.residency.poll()
    assert audio_engine_mock.trigger_input_runtime_pad.call_count == (3 if ready else 9)
    assert all(
        call.kwargs["received_at_ns"] == 456
        for call in audio_engine_mock.trigger_input_runtime_pad.call_args_list
    )
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


def test_claimed_current_ticket_deadline_keeps_ownership_but_revokes_launch(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
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
    message = "Failed to send global playback batch - buffer may be full"
    accepted = audio_engine_mock.start_global_playback_batch.return_value
    audio_engine_mock.start_global_playback_batch.side_effect = (
        [RuntimeError(message), accepted] if succeeds else RuntimeError(message)
    )
    ticket.status = "accepted"
    controller.transport.residency.poll()
    for _ in range(20):
        controller.transport.playback._global_playback.poll()
    assert audio_engine_mock.start_global_playback_batch.call_count == (2 if succeeds else 8)
    assert all(
        call.kwargs["received_at_ns"] == 123
        for call in audio_engine_mock.start_global_playback_batch.call_args_list
    )
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
    ticket.status = "accepted"
    controller.transport.residency.poll()
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=456)
    audio_engine_mock.start_global_playback_batch.assert_called_once()
    assert audio_engine_mock.start_global_playback_batch.call_args.kwargs["received_at_ns"] == 456


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
    global_controller._deferred = replace(deferred, deadline=0.0)
    global_controller.poll()
    ticket.status = "accepted"
    controller.transport.residency.poll()
    global_controller.poll()
    audio_engine_mock.start_global_playback_batch.assert_not_called()
    assert global_controller._deferred is None
    assert controller.transport.residency.status(0) == (
        None,
        "global playback readiness deadline expired",
    )


def test_post_ack_ui_launch_keeps_opaque_ticket_and_rejects_retired_source(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket = resident(controller, audio_engine_mock)
    audio_engine_mock.play_resident_control.return_value = False
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
    ticket.status = "accepted"
    controller.transport.residency.poll()
    audio_engine_mock.play_resident_control.assert_called_once_with(
        ticket, exclusive=True, received_at_ns=123
    )
    audio_engine_mock.play_sample_exclusive.assert_not_called()
    assert controller.transport.residency.status(0) == (
        None,
        "resident launch ownership retired",
    )


@pytest.mark.parametrize("stop", ["pad", "global"])
@pytest.mark.parametrize("input_kind", ["ui", "midi"])
def test_stop_after_ack_revokes_queued_launch_before_active_feedback(
    controller: AppController, audio_engine_mock: Mock, stop: str, input_kind: str
) -> None:
    ticket = resident(controller, audio_engine_mock)
    if input_kind == "ui":
        controller.transport.playback.trigger_pad(0, received_at_ns=123)
    else:
        controller.input_mapping.set_enabled(enabled=True)
        audio_engine_mock.trigger_input_runtime_pad.side_effect = [False, True]
        controller.input_mapping._handle_rust_input_event({
            "source": "midi",
            "binding_key": "midi:note:1:60",
            "action_key": "pad.trigger:0",
            "received_at_ns": 123,
            "direct": True,
            "dispatched": False,
        })
    ticket.status = "accepted"
    controller.transport.residency.poll()
    assert controller.transport.residency.status(0) == (None, None)
    assert not controller.session.active_sample_ids
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
    assert ticket.status == "accepted"
    assert ticket.is_current()
    assert not controller.transport.residency._launched
    assert controller.project.pad_loop_start_s[0] == 3.0


@pytest.mark.parametrize("retire", ["unload", "shutdown"])
def test_adopted_launch_retires_with_only_its_ticket_on_unload_and_all_on_shutdown(
    controller: AppController, audio_engine_mock: Mock, retire: str
) -> None:
    ticket = resident(controller, audio_engine_mock)
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
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
    start_ticket = audio_engine_mock.start_global_playback_batch.return_value
    start_ticket.status = status
    controller.transport.playback.start_or_restart_global_start_stop(received_at_ns=123)
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
