"""Controller feedback/ownership tests; actual Native output is covered by productive tests."""

from typing import TYPE_CHECKING
from unittest.mock import Mock, call

import pytest

from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding
from tests.flitzis_looper.controller.transport.test_residency import Ticket, resident

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def _feedback(
    pad: int = 0, *, effective: bool = False, ready: bool = True, request: int = 1, window: int = 1
) -> dict[str, object]:
    return {
        "source_id": f"loaded-{pad}-1",
        "source_generation": 1,
        "source_identity": 123 + pad,
        "window_revision": window,
        "request_id": request,
        "effective": effective,
        "ready": ready,
        "state": "wet" if effective else "dry",
        "error": None,
    }


def _mode_source(controller: AppController, audio: Mock) -> tuple[Ticket, dict[str, object]]:
    ticket = resident(controller, audio)
    ticket.key_lock_request_id = 2
    feedback = _feedback()
    audio.pad_key_lock_status.return_value = feedback
    return ticket, feedback


def test_window_ack_waits_for_own_actual_processing_feedback(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket, feedback = _mode_source(controller, audio_engine_mock)
    residency = controller.transport.residency
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    audio_engine_mock.prepare_resident_control.assert_called_once_with(
        0, start_s=None, end_s=None, position_s=None, key_lock=True
    )
    assert residency.key_lock_status(0).requested is True
    assert residency.key_lock_status(0).effective is False
    ticket.status = "accepted"
    feedback.update(effective=True, ready=False, request_id=2, window_revision=2)
    residency.poll()
    assert residency.key_lock_status(0).pending
    assert residency.key_lock_status(0).effective is False
    feedback["ready"] = True
    residency.poll()
    assert not residency.key_lock_status(0).pending
    assert residency.key_lock_status(0).effective is True
    assert controller.project.pad_loop_start_s[0] == 3.0
    assert controller.project.pad_loop_end_s[0] == 4.0
    audio_engine_mock.seek_sample.assert_not_called()
    audio_engine_mock.play_resident_control.assert_not_called()


def test_initial_on_intent_with_unknown_baseline_waits_for_own_native_ready_before_launch(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket, feedback = _mode_source(controller, audio_engine_mock)
    controller.project.pad_key_lock[0] = True
    # No actual previous mode feedback exists yet; saved ON alone cannot grant
    # effective ON or bypass this launch's own mode request acknowledgement.
    audio_engine_mock.pad_key_lock_status.return_value = None
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
    ticket.status = "accepted"
    audio_engine_mock.pad_key_lock_status.return_value = feedback
    feedback.update(effective=True, ready=False, request_id=2, window_revision=2)
    residency = controller.transport.residency
    residency.poll()
    assert residency.key_lock_status(0).effective is None
    assert residency.key_lock_status(0).pending
    audio_engine_mock.play_resident_control.assert_not_called()
    feedback["ready"] = True
    residency.poll()
    assert residency.key_lock_status(0).effective is True
    assert not residency.key_lock_status(0).pending
    audio_engine_mock.play_resident_control.assert_called_once_with(
        ticket, exclusive=True, received_at_ns=123
    )


@pytest.mark.parametrize("terminal", ["wet", "error"])
def test_armed_first_live_readiness_is_visible_without_new_controller_transaction(
    controller: AppController, audio_engine_mock: Mock, terminal: str
) -> None:
    _ticket, feedback = _mode_source(controller, audio_engine_mock)
    residency = controller.transport.residency
    controller.project.pad_key_lock[0] = True
    controller.project.key_lock = True
    feedback.update(effective=True, state="armed")
    residency.remember(0)
    assert residency.key_lock_status(0).effective is True
    assert not residency.key_lock_status(0).pending
    # Start/resume changes real Native readiness, not requested intent or ownership.
    feedback.update(effective=False, ready=False, state="waiting")
    state = residency.key_lock_status(0)
    assert state.requested
    assert state.pending
    assert state.effective is False
    assert not state.unconfirmed
    assert not residency._pending
    residency.remember(0)
    assert residency._effective[0].key_lock is False
    assert controller.transport.global_params.key_lock_status().pending
    feedback.update(
        effective=terminal == "wet",
        ready=terminal == "wet",
        state=terminal,
        error="native worker failed" if terminal == "error" else None,
    )
    settled = residency.key_lock_status(0)
    assert settled.requested
    assert not settled.pending
    assert settled.effective is (terminal == "wet")
    assert settled.error == ("native worker failed" if terminal == "error" else None)
    audio_engine_mock.prepare_resident_control.assert_not_called()


@pytest.mark.parametrize("scalar", [False, True])
@pytest.mark.parametrize("actual_mode", [False, True])
def test_terminal_own_mode_error_reconciles_actual_audio_even_when_ready_was_not_polled(
    controller: AppController, audio_engine_mock: Mock, *, scalar: bool, actual_mode: bool
) -> None:
    ticket, feedback = _mode_source(controller, audio_engine_mock)
    if scalar:
        audio_engine_mock.loaded_residency.return_value["cache_backed"] = False
        audio_engine_mock.set_pad_key_lock.return_value = 2
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    ticket.status = "accepted"
    feedback.update(
        effective=actual_mode,
        ready=False,
        request_id=2,
        window_revision=2,
        state="error",
        error="native worker failed",
    )
    residency = controller.transport.residency
    assert residency.key_lock_status(0).effective is actual_mode
    residency.poll()
    state = residency.key_lock_status(0)
    assert state.requested is actual_mode
    assert state.effective is actual_mode
    assert state.error == "native worker failed"
    assert not state.pending
    if scalar:
        requested_mode = True
        audio_engine_mock.set_pad_key_lock.assert_called_once_with(0, requested_mode)
    else:
        audio_engine_mock.set_pad_key_lock.assert_not_called()


def test_accepted_coupled_geometry_survives_own_mode_failure(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old, feedback = _mode_source(controller, audio_engine_mock)
    latest = Ticket()
    latest.key_lock_request_id = 3
    latest.window_revision = 3
    audio_engine_mock.prepare_resident_control.side_effect = [old, latest]
    controller.transport.loop.set_end(0, 7.0)
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    latest.status = "accepted"
    feedback.update(
        effective=False,
        ready=False,
        request_id=3,
        window_revision=3,
        state="error",
        error="native worker failed",
    )
    controller.transport.residency.poll()
    assert controller.project.pad_loop_end_s[0] == 7.0
    assert not controller.project.pad_key_lock[0]
    assert controller.transport.residency.key_lock_status(0).error == "native worker failed"


@pytest.mark.parametrize("stale", ["window", "request", "source", "generation"])
def test_old_feedback_never_completes_latest_mode(
    controller: AppController, audio_engine_mock: Mock, stale: str
) -> None:
    ticket, feedback = _mode_source(controller, audio_engine_mock)
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    ticket.status = "accepted"
    feedback.update(effective=True, request_id=2, window_revision=2)
    field = {
        "window": "window_revision",
        "request": "request_id",
        "source": "source_id",
        "generation": "source_generation",
    }[stale]
    feedback[field] = "previous-source" if stale == "source" else 9
    controller.transport.residency.poll()
    assert controller.transport.residency.key_lock_status(0).pending
    assert controller.project.pad_key_lock[0]


def test_identical_pending_mode_preserves_ticket_retry_budget_and_deadline(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _mode_source(controller, audio_engine_mock)
    audio_engine_mock.prepare_resident_control.side_effect = RuntimeError("resident queue is full")
    residency = controller.transport.residency
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    pending = residency._pending[0]
    deadline = pending.deadline
    for attempt in range(1, 9):
        for _ in range(5):
            controller.transport.pad.set_pad_key_lock(0, enabled=True)
            assert residency._pending[0] is pending
            assert pending.deadline == deadline
            assert pending.attempts == attempt
        residency.poll()
    assert audio_engine_mock.prepare_resident_control.call_count == 8
    assert not controller.project.pad_key_lock[0]
    assert residency.key_lock_status(0).error == "resident admission retry limit reached"


def test_identical_mode_with_retired_ticket_obtains_fresh_native_work(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old, _ = _mode_source(controller, audio_engine_mock)
    fresh = Ticket()
    fresh.key_lock_request_id = 3
    fresh.window_revision = 3
    audio_engine_mock.prepare_resident_control.side_effect = [old, fresh]
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    old.current = False
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    assert audio_engine_mock.prepare_resident_control.call_count == 2
    assert controller.transport.residency._pending[0].ticket is not None
    assert controller.transport.residency._pending[0].ticket.key_lock_request_id == 3


@pytest.mark.parametrize("coupled", ["loop", "seek"])
def test_mode_keeps_actual_pending_loop_or_seek_coupled(
    controller: AppController, audio_engine_mock: Mock, coupled: str
) -> None:
    old, _ = _mode_source(controller, audio_engine_mock)
    new = Ticket()
    audio_engine_mock.prepare_resident_control.side_effect = [old, new]
    if coupled == "loop":
        controller.transport.loop.set_end(0, 7.0)
    else:
        controller.transport.residency.seek(0, 3.25)
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    audio_engine_mock.prepare_resident_control.assert_called_with(
        0,
        start_s=3.0,
        end_s=7.0 if coupled == "loop" else 4.0,
        position_s=3.25 if coupled == "seek" else None,
        key_lock=True,
    )


@pytest.mark.parametrize("operation_id", [None, 0, 41])
def test_scalar_enqueue_never_manufactures_ack_and_cannot_be_cancelled_as_window(
    controller: AppController, audio_engine_mock: Mock, operation_id: int | None
) -> None:
    _, feedback = _mode_source(controller, audio_engine_mock)
    audio_engine_mock.loaded_residency.return_value["cache_backed"] = False
    audio_engine_mock.set_pad_key_lock.return_value = operation_id
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    residency = controller.transport.residency
    feedback.update(effective=True, request_id=40, window_revision=1)
    residency.poll()
    assert residency.key_lock_status(0).pending
    assert not residency.cancel_requested(0)
    assert controller.project.pad_key_lock[0]
    if operation_id == 41:
        feedback["request_id"] = 41
        residency.poll()
        assert not residency.key_lock_status(0).pending
    else:
        residency._pending[0].deadline = 0.0
        residency.poll()
        assert residency.key_lock_status(0).unconfirmed


def test_claimed_mode_deadline_retains_effective_baseline_and_later_real_confirmation(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket, feedback = _mode_source(controller, audio_engine_mock)
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    residency = controller.transport.residency
    ticket.status = "adopting"
    residency._pending[0].deadline = 0.0
    residency.poll()
    state = residency.key_lock_status(0)
    assert state.unconfirmed
    assert state.pending
    assert state.effective is False
    assert not residency.cancel_requested(0)
    ticket.status = "accepted"
    feedback.update(effective=True, request_id=2, window_revision=2)
    residency.poll()
    state = residency.key_lock_status(0)
    assert state.effective is True
    assert not state.pending
    assert state.error is None


def test_three_way_supersession_keeps_claimed_predecessor_confirmed_baseline(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    old, feedback = _mode_source(controller, audio_engine_mock)
    final = Ticket()
    final.key_lock_request_id = 4
    final.window_revision = 4
    final.previous_window_revision = 2
    audio_engine_mock.prepare_resident_control.side_effect = [
        old,
        RuntimeError("resident native adoption is in progress"),
        RuntimeError("resident native adoption is in progress"),
        final,
    ]
    pad = controller.transport.pad
    pad.set_pad_key_lock(0, enabled=True)
    old.status = "adopting"
    pad.set_pad_key_lock(0, enabled=False)
    pad.set_pad_key_lock(0, enabled=True)
    old.status = "accepted"
    feedback.update(effective=True, request_id=2, window_revision=2)
    residency = controller.transport.residency
    residency.poll()
    final.status = "failed"
    final.message = "reserve rejected"
    residency.poll()
    assert controller.project.pad_key_lock[0] is True
    assert residency.key_lock_status(0).effective is True
    assert residency.key_lock_status(0).error == "reserve rejected"


def test_new_source_and_selection_never_inherit_old_mode_feedback(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ticket, feedback = _mode_source(controller, audio_engine_mock)
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    controller.project.selected_pad = 37
    assert UiContext(controller).state.pads.key_lock_status(37).effective is False
    controller.transport.residency.cancel(0)
    controller.project.sample_paths[0] = "samples/reloaded.wav"
    controller.project.pad_key_lock[0] = False
    audio_engine_mock.current_input_runtime_pad_binding.return_value = FakeInputRuntimePadBinding(
        accepted_timing={"source_id": "loaded-0-2", "source_generation": 2}
    )
    ticket.status = "accepted"
    feedback.update(effective=True, request_id=2, window_revision=2)
    controller.transport.residency.poll()
    state = UiContext(controller).state.pads.key_lock_status(0)
    assert state.effective is None
    assert not state.pending
    assert not state.requested


@pytest.mark.parametrize("same_path", [False, True])
def test_terminal_mode_error_does_not_follow_reloaded_content_without_explicit_cancel(
    controller: AppController, audio_engine_mock: Mock, *, same_path: bool
) -> None:
    ticket, feedback = _mode_source(controller, audio_engine_mock)
    controller.transport.pad.set_pad_key_lock(0, enabled=True)
    ticket.status = "failed"
    ticket.message = "old source reserve rejected"
    residency = controller.transport.residency
    residency.poll()
    assert residency.key_lock_status(0).error == "old source reserve rejected"
    # Reproduce native replacement accepted by the loader, including a same-path
    # replacement, rather than relying on the explicit Unload cancel path.
    if not same_path:
        controller.project.sample_paths[0] = "samples/replacement.wav"
    audio_engine_mock.current_input_runtime_pad_binding.return_value = FakeInputRuntimePadBinding(
        accepted_timing={"source_id": "loaded-0-2", "source_generation": 2}
    )
    feedback.update(source_id="loaded-0-2", source_generation=2, source_identity=456)
    assert residency.key_lock_status(0).error is None
    assert residency.status(0) == (None, None)
    assert controller.transport.global_params.key_lock_status().error is None


def _all_bank_sources(
    controller: AppController, audio: Mock
) -> tuple[dict[int, Ticket], dict[int, dict[str, object]]]:
    _mode_source(controller, audio)
    targets = (0, 37, 215)
    tickets = {pad: Ticket() for pad in targets}
    for ticket in tickets.values():
        ticket.key_lock_request_id = 2
    feedback = {pad: _feedback(pad) for pad in targets}
    for pad in targets:
        controller.project.sample_paths[pad] = f"samples/source-{pad}.wav"
        controller.project.sample_durations[pad] = 12.0
        controller.project.pad_loop_auto[pad] = False
        controller.project.pad_loop_start_s[pad] = 3.0
        controller.project.pad_loop_end_s[pad] = 4.0
    audio.current_input_runtime_pad_binding.side_effect = FakeInputRuntimePadBinding
    audio.loaded_residency.side_effect = lambda pad: {
        "source_identity": 123 + pad,
        "window_revision": feedback[pad]["window_revision"],
    }
    audio.pad_key_lock_status.side_effect = feedback.get
    audio.prepare_resident_control.side_effect = lambda pad, **_kwargs: tickets[pad]
    return tickets, feedback


def test_global_continues_after_target_reject_and_reports_actual_mixed_outcome(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    tickets, feedback = _all_bank_sources(controller, audio_engine_mock)

    def prepare(pad: int, **_kwargs: object) -> Ticket:
        if pad == 0:
            message = "unsupported finite processing"
            raise ValueError(message)
        return tickets[pad]

    audio_engine_mock.prepare_resident_control.side_effect = prepare
    global_params = controller.transport.global_params
    global_params.set_key_lock(enabled=True)
    assert audio_engine_mock.prepare_resident_control.call_args_list == [
        call(pad, start_s=None, end_s=None, position_s=None, key_lock=True) for pad in (0, 37, 215)
    ]
    for pad in (37, 215):
        tickets[pad].status = "accepted"
        feedback[pad].update(effective=True, request_id=2, window_revision=2)
    controller.transport.residency.poll()
    state = global_params.key_lock_status()
    assert state.requested
    assert state.mixed
    assert state.effective is None
    assert not state.pending
    assert state.error == "1 pad(s): unsupported finite processing"
    assert [controller.project.pad_key_lock[pad] for pad in (0, 37, 215)] == [False, True, True]


def test_local_override_and_same_global_rebroadcast_keep_other_targets_and_pending_work(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    tickets, feedback = _all_bank_sources(controller, audio_engine_mock)
    global_params = controller.transport.global_params
    global_params.set_key_lock(enabled=True)
    unchanged = controller.transport.residency._pending[215]
    local = Ticket()
    local.key_lock_request_id = 3
    local.window_revision = 3
    audio_engine_mock.prepare_resident_control.side_effect = lambda pad, **_kwargs: (
        local if pad == 37 else tickets[pad]
    )
    controller.transport.pad.toggle_pad_key_lock(37)
    assert not controller.project.pad_key_lock[37]
    assert controller.transport.residency._pending[215] is unchanged
    # Native ACK of the old ON cannot complete the newer local OFF.
    tickets[37].status = "accepted"
    feedback[37].update(effective=True, request_id=2, window_revision=2)
    controller.transport.residency.poll()
    assert controller.transport.residency._pending[37].requested.key_lock is False
    global_params.set_key_lock(enabled=True)
    assert all(controller.project.pad_key_lock[pad] for pad in (0, 37, 215))
    assert controller.transport.residency._pending[215] is unchanged


def test_synchronous_target_failure_does_not_abort_global_broadcast(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _all_bank_sources(controller, audio_engine_mock)
    audio_engine_mock.loaded_residency.side_effect = [
        RuntimeError("source descriptor unavailable"),
        {"source_identity": 160, "window_revision": 1},
        {"source_identity": 160, "window_revision": 1},
        {"source_identity": 338, "window_revision": 1},
        {"source_identity": 338, "window_revision": 1},
    ]
    controller.transport.global_params.set_key_lock(enabled=True)
    assert (
        controller.transport.residency.key_lock_status(0).error == "source descriptor unavailable"
    )
    assert controller.project.pad_key_lock[37]
    assert controller.project.pad_key_lock[215]
