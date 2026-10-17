"""Native envelope orchestration tests; these fakes make no DSP or device claim."""

from dataclasses import dataclass
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.input_mapping.test_current_runtime import _configure_accepted_pad

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import CapturedPitchAction


@dataclass(slots=True)
class _CapturedPitch:
    accepted_sequence: int
    content_lifetime: int = 77
    content_is_current: bool = True
    base_shift: int = 0
    extra_shift: int = 2
    received_at_ns: int = 123
    retrigger: bool = False
    resets_key_correction: bool = False
    accepted: bool = False
    effective: bool = False
    state: str = "captured"
    reason: str | None = None
    current_control_slot: int | None = 0
    effective_output_frame: int | None = None


class _Binding:
    def __init__(self, lifetime: int) -> None:
        self._lifetime = lifetime

    def metadata(self) -> dict[str, object]:
        return {"content_lifetime": self._lifetime}


def _as_native(capture: _CapturedPitch) -> CapturedPitchAction:
    return cast("CapturedPitchAction", capture)


def _pitch_fixture(controller: AppController, audio: Mock) -> tuple[dict[int, int], Mock, Mock]:
    controller.transport.pitch.bind_runtime_refresh(lambda: True)
    controller.project.sample_paths[0] = "samples/synthetic-source-a.wav"
    controller.project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    controller.project.pad_key_intent[0] = PadKeyIntent(source=SourceKeyVersion(raw_key="Em"))
    controller.project.sample_paths[1] = "samples/synthetic-source-b.wav"
    controller.project.pad_content[1] = PadContentIdentity(instance_id="b" * 32)
    lifetimes = {0: 77, 1: 88}
    audio.current_input_runtime_pad_binding.side_effect = lambda slot: _Binding(lifetimes[slot])

    def publish(capture: _CapturedPitch) -> bool:
        capture.accepted = True
        capture.state = "preparing"
        return True

    def cancel(capture: _CapturedPitch) -> bool:
        if capture.state in {"applied", "rejected"}:
            return False
        capture.state = "rejected"
        capture.reason = "stale_content"
        return True

    audio.publish_pitch_action = Mock(side_effect=publish)
    audio.cancel_pitch_action = Mock(side_effect=cancel)
    audio.capture_pad_pitch_action = Mock()
    audio.capture_selected_pitch_action = Mock()
    return lifetimes, audio.publish_pitch_action, audio.cancel_pitch_action


def test_gui_capture_checks_source_receipt_first_and_persists_only_after_native_acceptance(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    capture = _CapturedPitch(1, base_shift=-4, extra_shift=12, retrigger=True)
    order: list[str] = []

    def ready(_slot: int) -> None:
        order.append("source-ready")

    def capture_pad(_slot: int, _key: str, *, received_at_ns: int | None) -> CapturedPitchAction:
        assert order
        assert received_at_ns == 123
        order.append("capture")
        return _as_native(capture)

    def publish(_capture: _CapturedPitch) -> bool:
        assert controller.project.pad_key_intent[0].base_shift == 0
        assert controller.project.pad_key_intent[0].extra_shift == 0
        order.append("publish")
        _capture.accepted = True
        _capture.state = "preparing"
        return True

    monkeypatch.setattr(controller.transport, "ensure_current_source_assignment_ready", ready)
    audio_engine_mock.capture_pad_pitch_action.side_effect = capture_pad
    audio_engine_mock.publish_pitch_action.side_effect = publish
    controller.project.selected_pad = 1
    assert controller.transport.pitch.choose_pad_action(
        0, "pitch.extra:12:set_retrigger", received_at_ns=123
    )
    assert order.index("source-ready") < order.index("capture") < order.index("publish")
    assert controller.project.pad_key_intent[0].base_shift == -4
    assert controller.project.pad_key_intent[0].extra_shift == 12
    assert controller.project.pad_key_intent[0].retrigger is False
    assert controller.project.pad_key_intent[1].extra_shift == 0
    assert controller.transport.pitch.status(0) == ("preparing", None)


def test_native_midi_capture_keeps_original_tuple_content_and_time_after_correction_and_selection(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _, publish, _ = _pitch_fixture(controller, audio_engine_mock)
    capture = _CapturedPitch(1, base_shift=-4, extra_shift=2, received_at_ns=777)
    controller.transport.pad.set_manual_key(0, "Fm")
    controller.project.selected_pad = 1
    assert controller.transport.pitch.accept_captured_event(_as_native(capture))
    publish.assert_called_once_with(capture)
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    audio_engine_mock.capture_selected_pitch_action.assert_not_called()
    intent = controller.project.pad_key_intent[0]
    assert (intent.base_shift, intent.extra_shift, intent.correction) == (-4, 2, "Fm")
    assert capture.received_at_ns == 777
    assert controller.project.pad_key_intent[1].extra_shift == 0


def test_keyboard_pitch_captures_native_selection_without_reading_python_selected_slot(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    capture = _CapturedPitch(1, received_at_ns=321)
    audio_engine_mock.capture_selected_pitch_action.return_value = _as_native(capture)
    controller.project.selected_pad = 1
    assert controller.transport.pitch.accept_selected_action(
        "pitch.extra:2:set", received_at_ns=321
    )
    audio_engine_mock.capture_selected_pitch_action.assert_called_once_with(
        "pitch.extra:2:set", received_at_ns=321
    )
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    assert controller.project.pad_key_intent[0].extra_shift == 2
    assert controller.project.pad_key_intent[1].extra_shift == 0


def test_repeated_actions_have_distinct_owners_and_late_effective_ack_cannot_revert_latest_choice(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _, publish, _ = _pitch_fixture(controller, audio_engine_mock)
    first = _CapturedPitch(1, extra_shift=2, retrigger=True)
    second = _CapturedPitch(2, extra_shift=-3, retrigger=True)
    controller.transport.pitch.accept_captured_event(_as_native(first))
    controller.transport.pitch.accept_captured_event(_as_native(second))
    assert len(controller.transport.pitch._owners) == 2
    assert controller.project.pad_key_intent[0].extra_shift == -3
    second.state = "applied"
    second.effective_output_frame = 2000
    controller.transport.pitch.poll()
    first.state = "applied"
    first.effective_output_frame = 3000
    controller.transport.pitch.poll()
    assert controller.project.pad_key_intent[0].extra_shift == -3
    projection = controller.transport.pitch._projections["a" * 32]
    assert projection.queued_sequence == 2
    assert projection.effective_sequence == 1
    assert projection.effective_pitch == (0, 2)
    controller.transport.pitch.accept_captured_event(_as_native(first))
    assert publish.call_count == 2
    assert controller.project.pad_key_intent[0].extra_shift == -3


def test_move_follows_original_content_and_overwrite_fences_its_lifetime(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    lifetimes, _, _ = _pitch_fixture(controller, audio_engine_mock)
    capture = _CapturedPitch(1)
    controller.transport.pitch.accept_captured_event(_as_native(capture))
    project = controller.project
    project.pad_content[0], project.pad_content[1] = project.pad_content[1], project.pad_content[0]
    project.pad_key_intent[0], project.pad_key_intent[1] = (
        project.pad_key_intent[1],
        project.pad_key_intent[0],
    )
    project.sample_paths[0], project.sample_paths[1] = (
        project.sample_paths[1],
        project.sample_paths[0],
    )
    lifetimes[0], lifetimes[1] = lifetimes[1], lifetimes[0]
    capture.current_control_slot = 1
    controller.transport.pitch.poll()
    assert controller.transport.pitch.status(1) == ("preparing", None)
    assert controller.transport.pitch.status(0) == (None, None)
    assert controller.project.pad_key_intent[1].extra_shift == 2
    controller.project.pad_content[1] = PadContentIdentity(instance_id="c" * 32)
    controller.project.pad_key_intent[1] = PadKeyIntent()
    lifetimes[1] = 99
    capture.current_control_slot = None
    capture.state = "rejected"
    capture.reason = "stale_content"
    controller.transport.pitch.poll()
    assert controller.project.pad_key_intent[1] == PadKeyIntent()
    assert controller.transport.pitch.status(1) == (None, None)
    assert not controller.transport.pitch._owners


def test_unaccepted_publication_preserves_numeric_intent_and_records_observed_highwater(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    capture = _CapturedPitch(5, extra_shift=18)

    def reject(_capture: _CapturedPitch) -> bool:
        _capture.state = "rejected"
        _capture.reason = "unsupported_pitch"
        msg = "unsupported_pitch"
        raise ValueError(msg)

    audio_engine_mock.publish_pitch_action.side_effect = reject
    audio_engine_mock.capture_pad_pitch_action.return_value = _as_native(capture)
    before = controller.project.pad_key_intent[0]
    assert not controller.transport.pitch.choose_pad_action(0, "pitch.extra:18:set")
    assert controller.project.pad_key_intent[0] == before
    assert controller.transport.pitch.projection_sequence(0) == 5
    phase, error = controller.transport.pitch.status(0)
    assert phase == "error"
    assert error is not None
    assert "not supported" in error
    assert not controller.transport.pitch._owners
    controller.transport.pitch.poll()
    assert audio_engine_mock.publish_pitch_action.call_count == 1


def test_accepted_midi_owner_waits_for_project_source_receipt_without_recapturing(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    capture = _CapturedPitch(1)
    controller.transport._incoming_load_request = lambda _slot: 123
    assert controller.transport.pitch.accept_captured_event(_as_native(capture))
    assert controller.project.pad_key_intent[0].extra_shift == 0
    assert controller.transport.pitch.projection_sequence(0) == 0
    controller.transport._incoming_load_request = lambda _slot: None
    controller.transport.pitch.poll()
    assert controller.project.pad_key_intent[0].extra_shift == 2
    assert controller.transport.pitch.projection_sequence(0) == 1
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    audio_engine_mock.capture_selected_pitch_action.assert_not_called()
    assert audio_engine_mock.publish_pitch_action.call_count == 1


def test_applied_owner_waits_for_late_incoming_association_and_projects_once(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, publish, _ = _pitch_fixture(controller, audio_engine_mock)
    capture = _CapturedPitch(9, extra_shift=7, received_at_ns=777)
    controller.transport._incoming_load_request = lambda _slot: 123
    controller.project.selected_pad = 1
    write = Mock(wraps=controller.transport.pad._set_key_intent)
    monkeypatch.setattr(controller.transport.pad, "_set_key_intent", write)
    assert controller.transport.pitch.accept_captured_event(_as_native(capture))
    capture.state = "applied"
    capture.effective = True
    capture.effective_output_frame = 24000
    for _ in range(3):
        controller.transport.pitch.poll()
    assert controller.project.pad_key_intent[0].extra_shift == 0
    assert controller.transport.pitch.status(0) == (None, None)
    assert len(controller.transport.pitch._owners) == 1
    controller.transport._incoming_load_request = lambda _slot: None
    for _ in range(3):
        controller.transport.pitch.poll()
    assert controller.project.pad_key_intent[0].extra_shift == 7
    assert controller.project.pad_key_intent[1].extra_shift == 0
    assert controller.project.selected_pad == 1
    assert controller.transport.pitch.status(0) == ("applied", None)
    projection = controller.transport.pitch._projections["a" * 32]
    assert (projection.queued_sequence, projection.effective_sequence) == (9, 9)
    assert (projection.effective_pitch, projection.effective_frame) == ((0, 7), 24000)
    assert not controller.transport.pitch._owners
    write.assert_called_once()
    publish.assert_called_once_with(capture)
    assert capture.received_at_ns == 777
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    audio_engine_mock.capture_selected_pitch_action.assert_not_called()


def test_applied_owner_with_known_content_waits_for_pair_projection_receipt(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    capture = _CapturedPitch(1, extra_shift=7)

    def publish_with_pair_barrier(original: _CapturedPitch) -> bool:
        controller.transport._pair_deferred_sample_ids = frozenset({0})
        original.accepted = True
        original.state = "applied"
        original.effective = True
        original.effective_output_frame = 24000
        return True

    audio_engine_mock.publish_pitch_action.side_effect = publish_with_pair_barrier
    assert controller.transport.pitch.accept_captured_event(_as_native(capture))
    for _ in range(3):
        controller.transport.pitch.poll()
    assert controller.project.pad_key_intent[0].extra_shift == 0
    assert len(controller.transport.pitch._owners) == 1
    controller.transport._pair_deferred_sample_ids = frozenset()
    controller.transport.pitch.poll()
    assert controller.project.pad_key_intent[0].extra_shift == 7
    assert controller.transport.pitch.status(0) == ("applied", None)
    assert not controller.transport.pitch._owners
    audio_engine_mock.publish_pitch_action.assert_called_once_with(capture)


@pytest.mark.parametrize("terminal_state", ["applied", "rejected"])
def test_terminal_unassociated_owner_cannot_project_into_replacement(
    controller: AppController, audio_engine_mock: Mock, terminal_state: str
) -> None:
    lifetimes, publish, _ = _pitch_fixture(controller, audio_engine_mock)
    capture = _CapturedPitch(1, extra_shift=7)
    controller.transport._incoming_load_request = lambda _slot: 123
    assert controller.transport.pitch.accept_captured_event(_as_native(capture))
    capture.state = terminal_state
    capture.effective = terminal_state == "applied"
    capture.effective_output_frame = 24000 if capture.effective else None
    capture.content_is_current = False
    capture.current_control_slot = None
    lifetimes[0] = 99
    controller.project.pad_content[0] = PadContentIdentity(instance_id="c" * 32)
    controller.project.pad_key_intent[0] = PadKeyIntent(extra_shift=-3)
    controller.transport._incoming_load_request = lambda _slot: None
    controller.transport.pitch.poll()
    assert not controller.transport.pitch._owners
    assert controller.project.pad_key_intent[0].extra_shift == -3
    assert controller.transport.pitch.status(0) == (None, None)
    publish.assert_called_once_with(capture)


def test_applied_unassociated_owner_survives_placement_gap_and_follows_content(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    lifetimes, publish, _ = _pitch_fixture(controller, audio_engine_mock)
    capture = _CapturedPitch(1, extra_shift=7)
    controller.transport._pair_deferred_sample_ids = frozenset({0, 1})
    assert controller.transport.pitch.accept_captured_event(_as_native(capture))
    capture.state = "applied"
    capture.effective = True
    capture.effective_output_frame = 24000
    capture.current_control_slot = None
    for _ in range(3):
        controller.transport.pitch.poll()
    assert len(controller.transport.pitch._owners) == 1
    project = controller.project
    project.pad_content[0], project.pad_content[1] = project.pad_content[1], project.pad_content[0]
    project.pad_key_intent[0], project.pad_key_intent[1] = (
        project.pad_key_intent[1],
        project.pad_key_intent[0],
    )
    lifetimes[0], lifetimes[1] = lifetimes[1], lifetimes[0]
    capture.current_control_slot = 1
    controller.transport._pair_deferred_sample_ids = frozenset()
    controller.transport.pitch.poll()
    assert project.pad_key_intent[1].extra_shift == 7
    assert project.pad_key_intent[0].extra_shift == 0
    assert controller.transport.pitch.status(1) == ("applied", None)
    assert controller.transport.pitch.status(0) == (None, None)
    assert not controller.transport.pitch._owners
    publish.assert_called_once_with(capture)


def test_capacity_cancels_only_unpublished_original_owner_and_shutdown_fences_retained_owner(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, publish, cancel = _pitch_fixture(controller, audio_engine_mock)
    monkeypatch.setattr(controller.transport.pitch, "_MAX_OWNERS", 1)
    first = _CapturedPitch(1)
    second = _CapturedPitch(2, extra_shift=3)
    controller.transport.pitch.accept_captured_event(_as_native(first))
    with pytest.raises(RuntimeError, match="capacity"):
        controller.transport.pitch.accept_captured_event(_as_native(second))
    cancel.assert_called_once_with(second)
    assert publish.call_count == 1
    assert controller.project.pad_key_intent[0].extra_shift == 2
    controller.transport.pitch.shut_down()
    assert cancel.call_args.args == (first,)
    assert first.state == "rejected"
    controller.transport.pitch.shut_down()
    with pytest.raises(RuntimeError, match="closed"):
        controller.transport.pitch.accept_captured_event(_as_native(_CapturedPitch(3)))


def test_old_native_unavailability_is_explicit_and_preserves_intent(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    audio_engine_mock.capture_pad_pitch_action = None
    before = controller.project.pad_key_intent[0]
    assert not controller.transport.pitch.choose_pad_action(0, "pitch.extra:2:set")
    assert controller.project.pad_key_intent[0] == before
    assert controller.transport.pitch.status(0)[1] == (
        "Pitch choices require the current audio runtime."
    )
    audio_engine_mock.publish_pitch_action.assert_not_called()


def test_selection_receipt_after_newer_gui_selection_cannot_roll_back_native_or_ui_selection(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    selection = [10, 0]

    def select(slot: int) -> int:
        selection[0] += 1
        selection[1] = slot
        return selection[0]

    audio_engine_mock.select_input_pad = Mock(side_effect=select)
    audio_engine_mock.input_selected_projection = Mock(side_effect=lambda: tuple(selection))
    controller.select_pad(1)
    assert controller.project.selected_pad == 1
    audio_engine_mock.select_input_pad.reset_mock()
    controller.select_pad(
        0, native_selection_already_applied=True, expected_native_selection_revision=10
    )
    assert controller.project.selected_pad == 1
    assert selection == [11, 1]
    audio_engine_mock.select_input_pad.assert_not_called()
    controller.select_pad(
        1, native_selection_already_applied=True, expected_native_selection_revision=11
    )
    assert controller.project.selected_pad == 1
    audio_engine_mock.select_input_pad.assert_not_called()


@pytest.mark.parametrize("revision", [None, True, -1])
def test_invalid_selection_receipts_cannot_create_selection_authority(
    controller: AppController, audio_engine_mock: Mock, revision: object
) -> None:
    audio_engine_mock.input_selected_projection = Mock(return_value=(1, 1))
    before = controller.project.selected_pad
    controller.select_pad(
        1,
        native_selection_already_applied=True,
        expected_native_selection_revision=revision,  # type: ignore[arg-type]
    )
    assert controller.project.selected_pad == before
    audio_engine_mock.input_selected_projection.assert_not_called()


def test_gui_and_matching_midi_empty_pad_selection_remain_available_for_loading(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    audio_engine_mock.select_input_pad = Mock(return_value=2)
    audio_engine_mock.input_selected_projection = Mock(return_value=(2, None))
    controller.select_pad(9)
    assert controller.project.selected_pad == 9
    audio_engine_mock.select_input_pad.assert_called_once_with(9)
    audio_engine_mock.select_input_pad.reset_mock()
    controller.select_pad(
        9, native_selection_already_applied=True, expected_native_selection_revision=2
    )
    assert controller.project.selected_pad == 9
    audio_engine_mock.select_input_pad.assert_not_called()
    controller.project.sample_paths[9] = "samples/later-source.wav"
    controller.project.pad_content[9] = PadContentIdentity(instance_id="d" * 32)
    controller.project.selected_pad = 1
    controller.select_pad(
        9, native_selection_already_applied=True, expected_native_selection_revision=2
    )
    assert controller.project.selected_pad == 1


def test_queued_owner_is_pending_until_real_native_admission_and_is_not_published_twice(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    capture = _CapturedPitch(1)
    audio_engine_mock.publish_pitch_action.side_effect = None
    audio_engine_mock.publish_pitch_action.return_value = True
    controller.transport.pitch.accept_captured_event(_as_native(capture))
    assert capture.accepted is False
    assert capture.effective is False
    assert controller.transport.pitch.status(0) == ("preparing", None)
    projection = controller.transport.pitch._projections["a" * 32]
    assert projection.queued_sequence == 1
    assert projection.effective_pitch is None
    controller.transport.pitch.accept_captured_event(_as_native(capture))
    assert audio_engine_mock.publish_pitch_action.call_count == 1


def test_loaded_gui_choice_resolves_once_after_atomic_runtime_projection_and_never_over_newer_midi(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _configure_accepted_pad(controller, audio_engine_mock)
    audio_engine_mock.capture_selected_pitch_action = Mock()
    selection: list[int | None] = [2, None]

    def select(slot: int) -> int:
        selection[:] = [3, slot]
        return 3

    audio_engine_mock.select_input_pad = Mock(side_effect=select)
    audio_engine_mock.input_selected_projection = Mock(side_effect=lambda: tuple(selection))
    controller._gui_native_selection = (2, 0)
    controller._refresh_native_gui_selection(0)
    audio_engine_mock.select_input_pad.assert_called_once_with(0)
    assert controller._gui_native_selection == (3, 0)
    assert audio_engine_mock.set_input_runtime_state.call_args.kwargs["pitch_rows"]
    controller._refresh_native_gui_selection(0)
    assert audio_engine_mock.select_input_pad.call_count == 1
    selection[:] = [4, 1]
    controller._refresh_native_gui_selection(0)
    assert audio_engine_mock.select_input_pad.call_count == 1
