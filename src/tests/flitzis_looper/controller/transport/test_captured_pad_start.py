"""Original GUI owner/readiness routing; actual Native/FIFO proof is Rust-owned."""

from typing import TYPE_CHECKING, cast
from unittest.mock import Mock, call

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import ResidentWindowTicket


class _Start:
    """Opaque original native owner without reconstructible source/timing metadata."""

    def __init__(self) -> None:
        self.owner_identity = object()
        self.diagnostic_slot: int | None = 0
        self.status = "captured"
        self.failure: str | None = None
        self.cancelled = False

    def state(self) -> str:
        return self.status

    def same_owner(self, other: object) -> bool:
        return getattr(other, "owner_identity", None) is self.owner_identity

    def reason(self) -> str | None:
        return self.failure

    def original_owner_status(self) -> str:
        return "superseded" if self.cancelled else "current"

    def cancel(self) -> bool:
        self.cancelled = True
        self.status = "rejected"
        return True


class _StartAlias:
    """A second Python facade for the same opaque native owner."""

    def __init__(self, original: _Start) -> None:
        self.original = original
        self.owner_identity = original.owner_identity

    @property
    def diagnostic_slot(self) -> int | None:
        return self.original.diagnostic_slot

    def state(self) -> str:
        return self.original.state()

    def same_owner(self, other: object) -> bool:
        return self.original.same_owner(other)

    def reason(self) -> str | None:
        return self.original.reason()

    def original_owner_status(self) -> str:
        return self.original.original_owner_status()

    def cancel(self) -> bool:
        return self.original.cancel()


class _ObservedTrigger:
    def __init__(self, original: _Start) -> None:
        self.original = original

    @property
    def captured_start(self) -> _StartAlias:
        return _StartAlias(self.original)


class _Window:
    def __init__(self) -> None:
        self.status = "preparing"
        self.launch_cancelled = False

    @property
    def native_ticket(self) -> ResidentWindowTicket:
        """Expose only the Native boundary view; retain this fake for state changes."""
        return cast("ResidentWindowTicket", self)

    def publication_status(self) -> str:
        return self.status

    def is_current(self) -> bool:
        return True

    def error(self) -> str | None:
        return None

    def cancel_launch(self) -> bool:
        self.launch_cancelled = True
        return True

    def cancel(self) -> bool:
        self.status = "cancelled"
        return True


class _AckRaceStart(_Start):
    def __init__(self) -> None:
        super().__init__()
        self.joined = False

    def original_owner_status(self) -> str:
        return "current" if self.joined else "superseded"


def _ready(controller: AppController, audio: Mock, *, resident: bool) -> None:
    controller.project.sample_paths[0] = "samples/original.wav"
    controller.project.sample_durations[0] = 12.0
    controller.project.pad_loop_auto[0] = False
    controller.project.pad_loop_start_s[0] = 3.0
    controller.project.pad_loop_end_s[0] = 4.0
    audio.loaded_residency.return_value = {
        "source_identity": 123,
        "cache_backed": resident,
    }
    audio.capture_pad_start = Mock()
    audio.prepare_captured_pad_start = Mock()
    audio.publish_pad_start = Mock(return_value=True)


def test_gui_retains_exact_independent_edit_for_native_member_and_cancels_only_attack(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=True)
    edit = _Window()
    member = _Window()
    original = _Start()
    audio.prepare_resident_control.return_value = edit
    controller.transport.residency.publish_loop(0, 3.0, 4.0)
    assert controller.transport.residency._pending[0].ticket is edit.native_ticket
    audio.capture_pad_start.return_value = original
    audio.prepare_captured_pad_start.return_value = member

    controller.transport.playback.trigger_pad_keep_others(0, received_at_ns=777)

    (pending,) = controller.transport.residency._captured_starts.values()
    assert pending.dependency is edit.native_ticket
    assert pending.ticket is member.native_ticket
    audio.prepare_captured_pad_start.assert_called_once_with(original, edit)
    controller.transport.residency._cancel_captured_start(pending)
    assert original.cancelled is True
    assert member.launch_cancelled is True
    assert member.status == "cancelled"
    assert edit.launch_cancelled is False
    assert edit.status == "preparing"
    audio.publish_pad_start.assert_not_called()


def test_ack_race_defers_old_window_status_to_native_prepare_or_join(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=True)
    original = _AckRaceStart()
    window = _Window()
    audio.capture_pad_start.return_value = original

    def native_join(start: _AckRaceStart) -> _Window:
        assert start is original
        assert not start.cancelled
        start.joined = True
        return window

    audio.prepare_captured_pad_start.side_effect = native_join
    controller.transport.playback.trigger_pad_keep_others(0)
    assert original.joined is True
    assert original.cancelled is False
    audio.prepare_captured_pad_start.assert_called_once_with(original)
    window.status = "accepted"
    controller.transport.residency.poll()
    audio.publish_pad_start.assert_called_once_with(original, window)


def test_captured_gui_rejects_pending_edit_without_genuine_native_dependency(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=True)
    audio.prepare_resident_control.side_effect = RuntimeError("resident command queue is full")
    controller.transport.residency.publish_loop(0, 3.0, 4.0)
    assert controller.transport.residency._pending[0].ticket is None
    original = _Start()
    audio.capture_pad_start.return_value = original

    controller.transport.playback.trigger_pad_keep_others(0)

    assert original.cancelled is True
    assert not controller.transport.residency._captured_starts
    assert "no native request owner" in controller.transport.residency._launch_errors[0]
    audio.prepare_captured_pad_start.assert_not_called()
    audio.publish_pad_start.assert_not_called()


def test_gui_capture_precedes_readiness_and_publishes_only_original_owner(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=False)
    original = _Start()
    residency = controller.transport.residency

    def capture(*_args: object, **_kwargs: object) -> _Start:
        assert not residency._captured_starts
        audio.publish_pad_start.assert_not_called()
        return original

    audio.capture_pad_start.side_effect = capture
    controller.transport.playback.trigger_pad_keep_others(0, received_at_ns=777)

    arguments = audio.capture_pad_start.call_args
    assert arguments.args == (0, 3.0, 4.0, 1.0)
    assert arguments.kwargs["received_at_ns"] == 777
    assert arguments.kwargs["exclusive"] is False
    audio.publish_pad_start.assert_called_once_with(original)
    audio.prepare_captured_pad_start.assert_not_called()
    audio.play_sample.assert_not_called()
    audio.play_sample_exclusive.assert_not_called()
    assert len(residency._captured_starts) == 1
    original.status = "applied"
    residency.poll()
    assert not residency._captured_starts


def test_equal_timestamp_gui_attacks_retain_each_exact_own_window(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=True)
    first, second = _Start(), _Start()
    first_window, second_window = _Window(), _Window()
    audio.capture_pad_start.side_effect = [first, second]
    audio.prepare_captured_pad_start.side_effect = [first_window, second_window]

    controller.transport.playback.trigger_pad(0, received_at_ns=123)
    controller.transport.playback.trigger_pad(0, received_at_ns=123)

    assert len(controller.transport.residency._captured_starts) == 2
    assert audio.prepare_captured_pad_start.call_args_list == [call(first), call(second)]
    audio.publish_pad_start.assert_not_called()
    first_window.status = "accepted"
    controller.transport.residency.poll()
    audio.publish_pad_start.assert_called_once_with(first, first_window)
    second_window.status = "accepted"
    controller.transport.residency.poll()
    assert audio.publish_pad_start.call_args_list == [
        call(first, first_window),
        call(second, second_window),
    ]
    assert audio.capture_pad_start.call_count == 2


def test_stop_revokes_original_gui_owner_before_its_window_ack(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=True)
    original, window = _Start(), _Window()
    audio.capture_pad_start.return_value = original
    audio.prepare_captured_pad_start.return_value = window
    controller.transport.playback.trigger_pad(0, received_at_ns=123)

    controller.transport.playback.stop_pad(0)
    window.status = "accepted"
    controller.transport.residency.poll()

    assert original.cancelled
    assert window.launch_cancelled
    assert not controller.transport.residency._captured_starts
    audio.publish_pad_start.assert_not_called()


def test_queue_retry_reuses_original_capture_and_acknowledged_window(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=True)
    original, window = _Start(), _Window()
    audio.capture_pad_start.return_value = original
    audio.prepare_captured_pad_start.return_value = window
    audio.publish_pad_start.side_effect = [RuntimeError("ordinary start queue is full"), True]
    controller.transport.playback.trigger_pad(0, received_at_ns=777)
    window.status = "accepted"

    controller.transport.residency.poll()
    controller.transport.residency.poll()

    audio.capture_pad_start.assert_called_once()
    audio.prepare_captured_pad_start.assert_called_once_with(original)
    assert audio.publish_pad_start.call_args_list == [
        call(original, window),
        call(original, window),
    ]
    assert not original.cancelled


def test_queued_native_preparation_failure_remains_visible_to_controller(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=False)
    original = _Start()
    audio.capture_pad_start.return_value = original
    controller.transport.playback.trigger_pad(0, received_at_ns=777)
    assert len(controller.transport.residency._captured_starts) == 1

    original.status = "rejected"
    original.failure = "preparation_failed"
    controller.transport.residency.poll()

    assert not controller.transport.residency._captured_starts
    assert controller.transport.residency.status(0) == (None, "preparation_failed")


def test_captured_start_status_stays_pending_until_first_effect(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=True)
    original, window = _Start(), _Window()
    audio.capture_pad_start.return_value = original
    audio.prepare_captured_pad_start.return_value = window
    residency = controller.transport.residency

    controller.transport.playback.trigger_pad(0, received_at_ns=123)
    assert residency.status(0) == ("preparing", None)
    window.status = "adopting"
    assert residency.status(0) == ("adopting", None)
    window.status = "accepted"
    residency.poll()
    assert residency.status(0) == ("pending", None)
    original.status = "prepared"
    assert residency.status(0) == ("pending", None)
    original.status = "applied"
    residency.poll()
    assert residency.status(0) == (None, None)


def test_own_window_ack_preserves_captured_geometry_without_inventing_effective_mode(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=True)
    controller.project.pad_key_lock[0] = True
    original, window = _Start(), _Window()
    audio.capture_pad_start.return_value = original
    audio.prepare_captured_pad_start.return_value = window
    residency = controller.transport.residency
    controller.transport.playback.trigger_pad(0, received_at_ns=123)

    controller.project.pad_loop_start_s[0] = 6.0
    controller.project.pad_loop_end_s[0] = 7.0
    window.status = "accepted"
    residency.poll()
    assert 0 not in residency._effective
    assert residency.key_lock_status(0).effective is None

    audio.prepare_resident_control = Mock(side_effect=ValueError("loop edit rejected"))
    residency.publish_loop(0, 6.0, 7.0)
    assert controller.project.pad_loop_start_s[0] == 3.0
    assert controller.project.pad_loop_end_s[0] == 4.0
    assert controller.project.pad_key_lock[0] is True
    assert residency.key_lock_status(0).effective is None


def test_captured_geometry_uses_actual_mode_feedback_when_desired_mode_differs(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=True)
    controller.project.pad_key_lock[0] = True
    metadata = audio.current_input_runtime_pad_binding(0).metadata()
    audio.pad_key_lock_status.return_value = {
        "source_id": metadata["source_id"],
        "source_generation": metadata["source_generation"],
        "source_identity": 123,
        "effective": False,
        "ready": True,
    }
    original, window = _Start(), _Window()
    audio.capture_pad_start.return_value = original
    audio.prepare_captured_pad_start.return_value = window
    residency = controller.transport.residency
    controller.transport.playback.trigger_pad(0, received_at_ns=123)
    window.status = "accepted"
    residency.poll()

    assert residency._effective[0].key_lock is False
    audio.pad_key_lock_status.return_value = None
    residency.remember(0)
    assert residency._effective[0].key_lock is False
    assert controller.project.pad_key_lock[0] is True


def _direct_midi_event(start: _Start) -> dict[str, object]:
    return {
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pad.trigger:0",
        "value": 37,
        "received_at_ns": "diagnostic metadata cannot reconstruct the owner",
        "direct": True,
        "dispatched": True,
        "captured_trigger": _ObservedTrigger(start),
    }


def _assert_no_observer_launch_writes(audio: Mock) -> None:
    audio.capture_pad_start.assert_not_called()
    audio.prepare_captured_pad_start.assert_not_called()
    audio.publish_pad_start.assert_not_called()
    audio.trigger_captured_input_pad.assert_not_called()
    audio.prepare_captured_input_trigger.assert_not_called()
    audio.trigger_prepared_input_pad.assert_not_called()


def test_direct_dispatched_midi_observes_worker_failure_without_republishing(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=False)
    controller.input_mapping.set_enabled(enabled=True)
    original = _Start()
    original.status = "preparing"
    residency = controller.transport.residency

    controller.input_mapping._handle_rust_input_event(_direct_midi_event(original))

    assert len(residency._captured_starts) == 1
    assert residency.status(0) == ("pending", None)
    original.status = "rejected"
    original.failure = "preparation_failed"
    residency.poll()

    assert not residency._captured_starts
    assert residency.status(0) == (None, "preparation_failed")
    _assert_no_observer_launch_writes(audio)


def test_direct_midi_cloned_observations_preserve_deadline_and_never_write_geometry(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=False)
    controller.input_mapping.set_enabled(enabled=True)
    original = _Start()
    original.status = "scheduled"
    residency = controller.transport.residency
    previous_effective = dict(residency._effective)
    previous_geometry = dict(residency._acknowledged_geometry)
    event = _direct_midi_event(original)
    controller.input_mapping._handle_rust_input_event(event)
    pending = next(iter(residency._captured_starts.values()))
    deadline = pending.deadline

    controller.input_mapping._handle_rust_input_event(event)

    assert len(residency._captured_starts) == 1
    assert next(iter(residency._captured_starts.values())) is pending
    assert pending.deadline == deadline
    assert pending.requested is None
    controller.project.pad_loop_start_s[0] = 8.0
    controller.project.pad_loop_end_s[0] = 9.0
    original.status = "applied"
    residency.poll()
    assert not residency._captured_starts
    assert residency._effective == previous_effective
    assert residency._acknowledged_geometry == previous_geometry
    # Late duplicate delivery is already terminal and never recreates readiness work.
    controller.input_mapping._handle_rust_input_event(event)
    assert not residency._captured_starts
    assert residency.status(0) == (None, None)
    _assert_no_observer_launch_writes(audio)


def test_direct_midi_observer_cancellation_uses_current_original_cell_projection(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio = audio_engine_mock
    _ready(controller, audio, resident=False)
    controller.input_mapping.set_enabled(enabled=True)
    original = _Start()
    original.status = "preparing"
    controller.input_mapping._handle_rust_input_event(_direct_midi_event(original))
    original.diagnostic_slot = 3

    assert controller.transport.residency.cancel_launch(3)

    assert original.cancelled
    assert not controller.transport.residency._captured_starts
    audio.cancel_pad_launches.assert_called_once_with(3)
    _assert_no_observer_launch_writes(audio)
