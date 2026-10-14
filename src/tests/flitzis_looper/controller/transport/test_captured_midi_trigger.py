"""Real Controller façade retention; Native source/ACK checks have separate actual tests."""

from dataclasses import dataclass
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import CapturedInputPadTrigger


@dataclass(frozen=True)
class _NativeIntent:
    binding_key: str = "midi:note:1:60"
    received_at_ns: int = 123_456_789
    exclusive: bool = True


class _OpaqueTrigger:
    """Only Native reads the sealed mapping/time/exclusive values in this façade fake."""

    def __init__(self) -> None:
        self._native_intent = _NativeIntent()
        self.diagnostic_slot: int | None = None
        self.owner_status = "pending"
        self.observation_error: RuntimeError | ValueError | None = None

    def original_owner_status(self) -> str:
        if self.observation_error is not None:
            raise self.observation_error
        return self.owner_status

    def is_current(self) -> bool:
        pytest.fail("Own WindowACK must use the original token, not original-window currency")

    def metadata(self) -> dict[str, object]:
        pytest.fail("Controller must not reconstruct the original Native trigger")


class _Window:
    def __init__(self) -> None:
        self.status = "preparing"
        self.cancel_launch_calls = 0
        self.cancel_calls = 0

    def publication_status(self) -> str:
        return self.status

    def error(self) -> str | None:
        return None

    def is_current(self) -> bool:
        pytest.fail("Window acceptance belongs to the Native opaque trigger launch lane")

    def cancel_launch(self) -> bool:
        self.cancel_launch_calls += 1
        return self.cancel_launch_calls == 1

    def cancel(self) -> bool:
        self.cancel_calls += 1
        if self.status in {"adopting", "accepted"}:
            return False
        self.status = "cancelled"
        return True


def _event(trigger: object = None) -> dict[str, object]:
    return {
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.trigger:0",
        "received_at_ns": "untrusted Python timestamp",
        "direct": True,
        "dispatched": False,
        "captured_trigger": trigger,
    }


def _facade(controller: AppController, audio: Mock) -> Mock:
    controller.project.input_mapping_enabled = True
    controller.session.input_learn_active = False
    audio.reset_mock()
    gate = Mock()
    controller.transport.ensure_current_source_assignment_ready = gate
    return gate


def _assert_no_numeric_fallback(audio: Mock) -> None:
    audio.trigger_input_runtime_pad.assert_not_called()
    audio.current_input_runtime_pad_binding.assert_not_called()
    audio.prepare_resident_control.assert_not_called()
    audio.play_resident_control.assert_not_called()


def _original_window_launch(
    trigger: _OpaqueTrigger, window: _Window, seen: list[tuple[object, object, _NativeIntent]]
) -> Mock:
    outcomes = iter([RuntimeError("resident command queue is full"), False, True])

    def native_launch(original: object, own_window: object) -> bool:
        assert original is trigger
        assert own_window is window
        seen.append((original, own_window, trigger._native_intent))
        outcome = next(outcomes)
        if isinstance(outcome, RuntimeError):
            raise outcome
        if outcome:
            trigger.owner_status = "admitted"
        return outcome

    return Mock(side_effect=native_launch)


def test_original_event_keeps_exact_token_and_window_through_pair_and_queue_pressure(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    gate = _facade(controller, audio_engine_mock)
    trigger = _OpaqueTrigger()
    window = _Window()
    audio_engine_mock.trigger_captured_input_pad = Mock(
        side_effect=RuntimeError("captured MIDI command queue is full")
    )
    audio_engine_mock.prepare_captured_input_trigger = Mock(
        side_effect=[RuntimeError("resident command queue is full"), window]
    )
    seen: list[tuple[object, object, _NativeIntent]] = []
    audio_engine_mock.trigger_prepared_input_pad = _original_window_launch(trigger, window, seen)
    controller.input_mapping._handle_rust_input_event(_event(trigger))
    residency = controller.transport.residency
    pending = residency._captured_triggers[id(trigger)]
    for _ in range(3):
        residency.poll()
    audio_engine_mock.trigger_captured_input_pad.assert_not_called()
    audio_engine_mock.prepare_captured_input_trigger.assert_not_called()
    gate.assert_not_called()

    trigger.owner_status, trigger.diagnostic_slot = "current", 178
    residency.poll()
    assert residency._captured_triggers[id(trigger)] is pending
    assert pending.ticket is None
    controller.input_mapping._handle_rust_input_event(_event(trigger))
    assert len(residency._captured_triggers) == 1
    assert residency._captured_triggers[id(trigger)] is pending
    residency.poll()
    assert pending.ticket is window
    for _ in range(3):
        residency.poll()
    audio_engine_mock.trigger_prepared_input_pad.assert_not_called()

    window.status = "accepted"
    residency.poll()
    assert residency._captured_triggers[id(trigger)] is pending
    assert pending.ticket is window
    trigger.owner_status, trigger.diagnostic_slot = "pending", None
    residency.poll()
    assert pending.ticket is window
    trigger.owner_status, trigger.diagnostic_slot = "current", 36
    residency.poll()
    assert residency._captured_triggers[id(trigger)] is pending
    assert pending.ticket is window
    residency.poll()
    assert not residency._captured_triggers
    assert audio_engine_mock.prepare_captured_input_trigger.call_count == 2
    for prepared in audio_engine_mock.prepare_captured_input_trigger.call_args_list:
        assert prepared.args == (trigger,)
        assert prepared.kwargs == {}
    assert seen == [(trigger, window, _NativeIntent())] * 3
    assert all(call.args[0] in {178, 36} for call in gate.call_args_list)
    _assert_no_numeric_fallback(audio_engine_mock)


def test_missing_original_owner_rejects_event_without_numeric_or_native_recapture(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    gate = _facade(controller, audio_engine_mock)
    controller.input_mapping._handle_rust_input_event(_event())
    assert (
        controller.session.input_mapping_error
        == "MIDI trigger has no original Native gesture owner"
    )
    assert not controller.transport.residency._captured_triggers
    audio_engine_mock.trigger_captured_input_pad.assert_not_called()
    audio_engine_mock.prepare_captured_input_trigger.assert_not_called()
    audio_engine_mock.trigger_prepared_input_pad.assert_not_called()
    gate.assert_not_called()
    _assert_no_numeric_fallback(audio_engine_mock)


@pytest.mark.parametrize("terminal", ["retired", "superseded", "admitted"])
def test_only_known_terminal_original_owner_removes_retained_window(
    controller: AppController, audio_engine_mock: Mock, terminal: str
) -> None:
    _facade(controller, audio_engine_mock)
    trigger = _OpaqueTrigger()
    trigger.owner_status, trigger.diagnostic_slot = "current", 178
    window = _Window()
    audio_engine_mock.prepare_captured_input_trigger = Mock(return_value=window)
    residency = controller.transport.residency
    residency.prepare_captured_trigger(cast("CapturedInputPadTrigger", trigger))
    residency.poll()
    pending = residency._captured_triggers[id(trigger)]
    window.status = "accepted"
    trigger.observation_error = ValueError("original owner observation is temporarily unavailable")
    residency.poll()
    assert residency._captured_triggers[id(trigger)] is pending
    assert pending.ticket is window
    trigger.observation_error, trigger.owner_status = None, terminal
    residency.poll()
    assert not residency._captured_triggers
    audio_engine_mock.trigger_prepared_input_pad.assert_not_called()
    _assert_no_numeric_fallback(audio_engine_mock)


def test_shutdown_keeps_claimed_window_under_native_cancel_rules_and_admits_no_new_work(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _facade(controller, audio_engine_mock)
    trigger = _OpaqueTrigger()
    trigger.owner_status, trigger.diagnostic_slot = "current", 178
    window = _Window()
    audio_engine_mock.prepare_captured_input_trigger = Mock(return_value=window)
    residency = controller.transport.residency
    residency.prepare_captured_trigger(cast("CapturedInputPadTrigger", trigger))
    residency.poll()
    pending = residency._captured_triggers[id(trigger)]
    window.status = "adopting"
    residency.shut_down()
    assert window.cancel_launch_calls == 1
    assert window.cancel_calls == 1
    for _ in range(3):
        residency.poll()
    assert residency._captured_triggers[id(trigger)] is pending
    assert pending.ticket is window
    audio_engine_mock.prepare_captured_input_trigger.assert_called_once()
    audio_engine_mock.trigger_prepared_input_pad.assert_not_called()
    trigger.owner_status = "superseded"
    residency.poll()
    assert not residency._captured_triggers
    _assert_no_numeric_fallback(audio_engine_mock)


def test_original_captured_deadline_survives_pair_pending_redelivery_and_own_window(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _facade(controller, audio_engine_mock)
    now = [100.0]
    monkeypatch.setattr(
        "flitzis_looper.controller.transport.residency.time.monotonic", lambda: now[0]
    )
    trigger = _OpaqueTrigger()
    window = _Window()
    audio_engine_mock.prepare_captured_input_trigger = Mock(return_value=window)
    residency = controller.transport.residency
    controller.input_mapping._handle_rust_input_event(_event(trigger))
    pending = residency._captured_triggers[id(trigger)]
    assert pending.deadline == 130.0
    for _ in range(3):
        now[0] = 129.0
        residency.prepare_captured_trigger(cast("CapturedInputPadTrigger", trigger))
        residency.poll()
        assert residency._captured_triggers[id(trigger)] is pending
        assert pending.deadline == 130.0
    audio_engine_mock.prepare_captured_input_trigger.assert_not_called()
    trigger.owner_status, trigger.diagnostic_slot = "current", 178
    residency.poll()
    assert pending.ticket is window
    assert pending.deadline == 130.0
    window.status = "accepted"
    now[0] = 130.0
    residency.poll()
    assert residency._captured_triggers[id(trigger)] is pending
    assert pending.ticket is window
    assert pending.cancel_requested
    audio_engine_mock.prepare_captured_input_trigger.assert_called_once_with(trigger)
    audio_engine_mock.trigger_prepared_input_pad.assert_not_called()
    assert window.cancel_launch_calls == 1
    assert window.cancel_calls == 1
    trigger.owner_status = "superseded"
    residency.poll()
    assert not residency._captured_triggers
    _assert_no_numeric_fallback(audio_engine_mock)


def test_original_own_window_pressure_keeps_eight_attempt_limit_and_cancels_only_that_owner(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _facade(controller, audio_engine_mock)
    monkeypatch.setattr(
        "flitzis_looper.controller.transport.residency.time.monotonic", lambda: 100.0
    )
    trigger = _OpaqueTrigger()
    trigger.owner_status, trigger.diagnostic_slot = "current", 178
    window = _Window()
    audio_engine_mock.trigger_captured_input_pad = Mock(return_value=False)
    audio_engine_mock.prepare_captured_input_trigger = Mock(return_value=window)
    audio_engine_mock.trigger_prepared_input_pad = Mock(return_value=False)
    residency = controller.transport.residency
    controller.input_mapping._handle_rust_input_event(_event(trigger))
    pending = residency._captured_triggers[id(trigger)]
    residency.poll()
    assert pending.ticket is window
    window.status = "accepted"
    for _ in range(20):
        residency.poll()
        assert pending.deadline == 130.0
    audio_engine_mock.trigger_captured_input_pad.assert_called_once_with(trigger)
    audio_engine_mock.prepare_captured_input_trigger.assert_called_once_with(trigger)
    assert audio_engine_mock.trigger_prepared_input_pad.call_count == 8
    for call in audio_engine_mock.trigger_prepared_input_pad.call_args_list:
        assert call.args == (trigger, window)
        assert call.kwargs == {}
    assert residency._captured_triggers[id(trigger)] is pending
    assert pending.ticket is window
    assert pending.cancel_requested
    assert window.cancel_launch_calls >= 1
    assert window.cancel_calls >= 1
    trigger.owner_status = "superseded"
    residency.poll()
    assert not residency._captured_triggers
    _assert_no_numeric_fallback(audio_engine_mock)


@pytest.mark.parametrize("exhaustion", ["preparation", "deadline"])
def test_no_window_exhaustion_retains_original_budget_before_repeated_direct_delivery(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    exhaustion: str,
) -> None:
    _facade(controller, audio_engine_mock)
    now = [100.0]
    monkeypatch.setattr(
        "flitzis_looper.controller.transport.residency.time.monotonic", lambda: now[0]
    )
    trigger = _OpaqueTrigger()
    residency = controller.transport.residency
    audio_engine_mock.trigger_captured_input_pad = Mock(return_value=False)
    audio_engine_mock.prepare_captured_input_trigger = Mock(
        side_effect=RuntimeError("resident command queue is full")
    )
    if exhaustion == "preparation":
        trigger.owner_status, trigger.diagnostic_slot = "current", 178
    controller.input_mapping._handle_rust_input_event(_event(trigger))
    pending = residency._captured_triggers[id(trigger)]
    if exhaustion == "preparation":
        for _ in range(8):
            residency.poll()
        assert pending.attempts == 8
    else:
        now[0] = 130.0
        trigger.owner_status, trigger.diagnostic_slot = "current", 178
    residency.poll()
    assert pending.cancel_requested
    assert pending.ticket is None
    assert pending.deadline == 130.0
    assert trigger.original_owner_status() == "current"
    initial_dispatches = audio_engine_mock.trigger_captured_input_pad.call_count
    preparations = audio_engine_mock.prepare_captured_input_trigger.call_count
    assert initial_dispatches == (1 if exhaustion == "preparation" else 0)
    assert preparations == (8 if exhaustion == "preparation" else 0)
    audio_engine_mock.trigger_captured_input_pad.return_value = True
    for _ in range(3):
        controller.input_mapping._handle_rust_input_event(_event(trigger))
        residency.poll()
        assert residency._captured_triggers[id(trigger)] is pending
        assert pending.deadline == 130.0
        assert pending.cancel_requested
        assert pending.ticket is None
    assert audio_engine_mock.trigger_captured_input_pad.call_count == initial_dispatches
    assert audio_engine_mock.prepare_captured_input_trigger.call_count == preparations
    audio_engine_mock.trigger_prepared_input_pad.assert_not_called()
    trigger.owner_status = "superseded"
    residency.poll()
    assert not residency._captured_triggers
    _assert_no_numeric_fallback(audio_engine_mock)
