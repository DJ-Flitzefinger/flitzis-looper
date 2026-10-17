"""Piano control orchestration only; fake captures prove no Native DSP or mouse behavior."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.input_mapping.actions import PitchCcMode, parse_pitch_action
from flitzis_looper.input_mapping.bindings import KeyboardBinding
from flitzis_looper.input_mapping.storage import (
    load_keyboard_mapping_file,
    load_midi_mapping_file,
)
from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import PadContentIdentity
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.controller.transport.test_pitch_actions import (
    _as_native,
    _CapturedPitch,
    _pitch_fixture,
)

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def _modified_intent() -> PadKeyIntent:
    return PadKeyIntent(
        source=SourceKeyVersion(version=3, raw_key="C"),
        correction="D",
        correction_epoch=7,
        analysis_epoch=5,
        base_shift=14,
        extra_shift=1,
        retrigger=True,
        extra_retrigger=False,
    )


def test_accepted_original_key_persists_one_complete_reset_for_its_original_content(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    before = _modified_intent()
    other = before.changed(base_shift=-12, correction="Fm")
    controller.project.pad_key_intent[0] = before
    controller.project.pad_key_intent[1] = other
    controller.project.selected_pad = 1
    protected = (
        controller.project.key_lock,
        controller.project.bpm_lock,
        controller.project.multi_loop,
        controller.project.speed,
    )
    capture = _CapturedPitch(
        1, base_shift=0, extra_shift=0, retrigger=True, resets_key_correction=True
    )
    audio_engine_mock.capture_pad_pitch_action.return_value = _as_native(capture)
    write = Mock(wraps=controller.transport.pad._set_key_intent)
    monkeypatch.setattr(controller.transport.pad, "_set_key_intent", write)

    controller.transport.pitch.original_key(0, retrigger=True, received_at_ns=123)

    write.assert_called_once_with(0, before.original())
    assert controller.project.pad_key_intent[0] == before.original()
    assert controller.project.pad_key_intent[1] == other
    assert controller.project.selected_pad == 1
    assert protected == (
        controller.project.key_lock,
        controller.project.bpm_lock,
        controller.project.multi_loop,
        controller.project.speed,
    )
    audio_engine_mock.capture_pad_pitch_action.assert_called_once_with(
        0, "pitch.original:set_retrigger", received_at_ns=123
    )
    audio_engine_mock.publish_pitch_action.assert_called_once_with(capture)
    audio_engine_mock.set_speed.assert_not_called()
    audio_engine_mock.set_master_bpm.assert_not_called()
    audio_engine_mock.set_key_lock.assert_not_called()
    assert controller.transport.pitch.status(0) == ("preparing", None)


@pytest.mark.parametrize("failure", ["rejected_capture", "publication_refused", "stale_source"])
def test_original_key_failure_does_not_clear_any_metadata_or_numeric_choice(
    controller: AppController, audio_engine_mock: Mock, failure: str
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    before = _modified_intent()
    controller.project.pad_key_intent[0] = before
    controller.project.pad_key_intent[1] = before.changed(correction="Fm")
    other = controller.project.pad_key_intent[1]
    capture = _CapturedPitch(1, base_shift=0, extra_shift=0, resets_key_correction=True)
    audio_engine_mock.capture_pad_pitch_action.return_value = _as_native(capture)
    if failure == "rejected_capture":
        capture.state = "rejected"
        capture.reason = "unsupported_pitch"
    elif failure == "publication_refused":
        audio_engine_mock.publish_pitch_action.side_effect = None
        audio_engine_mock.publish_pitch_action.return_value = False
    else:
        audio_engine_mock.capture_pad_pitch_action.side_effect = ValueError("stale_content")

    controller.transport.pitch.original_key(0, retrigger=False, received_at_ns=123)

    assert controller.project.pad_key_intent[0] == before
    assert controller.project.pad_key_intent[1] == other
    phase, error = controller.transport.pitch.status(0)
    assert phase == "error"
    assert error
    assert controller.session.input_mapping_error
    if failure != "publication_refused":
        audio_engine_mock.publish_pitch_action.assert_not_called()


def test_original_reset_receipt_cannot_clear_a_replacement_with_another_lifetime(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    lifetimes, _, _ = _pitch_fixture(controller, audio_engine_mock)
    replacement = _modified_intent()
    controller.project.pad_content[0] = PadContentIdentity(instance_id="c" * 32)
    controller.project.pad_key_intent[0] = replacement
    lifetimes[0] = 99
    stale = _CapturedPitch(
        1,
        content_lifetime=77,
        base_shift=0,
        extra_shift=0,
        resets_key_correction=True,
        accepted=True,
    )

    # The facade test deliberately supplies an already accepted old receipt.
    # Actual stale Native admission is covered by its own ownership tests.
    controller.transport.pitch.accept_captured_event(_as_native(stale))
    assert controller.project.pad_key_intent[0] == replacement
    stale.state = "rejected"
    stale.reason = "stale_content"
    stale.content_is_current = False
    controller.transport.pitch.poll()
    assert controller.project.pad_key_intent[0] == replacement
    assert not controller.transport.pitch._owners
    audio_engine_mock.publish_pitch_action.assert_not_called()


def test_offset_reset_keeps_the_entire_piano_octave_correction_and_preferences(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    before = _modified_intent()
    controller.project.pad_key_intent[0] = before
    capture = _CapturedPitch(1, base_shift=14, extra_shift=0, retrigger=False)
    audio_engine_mock.capture_pad_pitch_action.return_value = _as_native(capture)

    UiContext(controller).audio.pads.reset_extra_pitch(0, retrigger=False, received_at_ns=123)

    assert controller.project.pad_key_intent[0] == before.changed(extra_shift=0)
    audio_engine_mock.capture_pad_pitch_action.assert_called_once_with(
        0, "pitch.extra:0:set", received_at_ns=123
    )
    audio_engine_mock.publish_pitch_action.assert_called_once_with(capture)


def test_runtime_refresh_failure_is_visible_and_captures_no_piano_action(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    controller.project.pad_key_intent[0] = _modified_intent()
    before = controller.project.pad_key_intent[0]
    refresh = Mock(return_value=False)
    controller.transport.pitch.bind_runtime_refresh(refresh)

    controller.transport.pitch.choose_piano_note(0, 12, retrigger=True, received_at_ns=123)

    refresh.assert_called_once_with()
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    audio_engine_mock.publish_pitch_action.assert_not_called()
    assert controller.project.pad_key_intent[0] == before
    phase, error = controller.transport.pitch.status(0)
    assert phase == "error"
    assert error is not None
    assert "not ready" in error
    assert controller.session.input_mapping_error is not None
    assert "not ready" in controller.session.input_mapping_error


def test_same_frame_source_then_runtime_refresh_precedes_one_original_piano_capture(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    controller.project.pad_key_intent[0] = _modified_intent()
    order: list[str] = []
    capture = _CapturedPitch(1, base_shift=-12, extra_shift=1, retrigger=True)

    def ready(slot: int) -> None:
        assert slot == 0
        order.append("source")

    def refresh() -> bool:
        assert controller.project.pad_key_intent[0].correction == "D"
        order.append("refresh")
        return True

    def capture_pad(slot: int, action: str, *, received_at_ns: int | None) -> object:
        assert (slot, action, received_at_ns) == (0, "pitch.note:-12:set_retrigger", 123)
        assert order[:2] == ["source", "refresh"]
        order.append("capture")
        return _as_native(capture)

    monkeypatch.setattr(controller.transport, "ensure_current_source_assignment_ready", ready)
    controller.transport.pitch.bind_runtime_refresh(refresh)
    audio_engine_mock.capture_pad_pitch_action.side_effect = capture_pad

    UiContext(controller).audio.pads.choose_piano_note(0, -12, retrigger=True, received_at_ns=123)

    assert order.index("refresh") < order.index("capture")
    audio_engine_mock.capture_pad_pitch_action.assert_called_once()
    audio_engine_mock.publish_pitch_action.assert_called_once_with(capture)
    assert controller.project.pad_key_intent[0].base_shift == -12
    assert controller.project.pad_key_intent[0].correction == "D"


@pytest.mark.parametrize("source", ["keyboard", "midi"])
def test_piano_original_and_step_learn_variants_survive_preference_and_pad_changes(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, source: str
) -> None:
    context = UiContext(controller)
    mapping = controller.input_mapping
    mapping.set_enabled(enabled=True)
    execute = Mock()
    monkeypatch.setattr(controller.transport.pitch, "accept_selected_action", execute)
    bindings = (
        [KeyboardBinding(key_name=name).key for name in ("A", "B", "C")]
        if source == "keyboard"
        else ["midi:note:1:60", "midi:cc:1:71", "midi:note:1:62"]
    )
    for index, binding in enumerate(bindings):
        controller.session.input_learn_active = True
        controller.session.input_learn_pending_source = source
        controller.session.input_learn_pending_binding_key = binding
        if index == 0:
            context.audio.pads.choose_piano_note(0, 12, retrigger=True)
        elif index == 1:
            context.audio.pads.original_key(0, retrigger=False)
        else:
            context.audio.pads.step_pitch(0, -1, retrigger=True)
    execute.assert_not_called()
    restored = load_keyboard_mapping_file() if source == "keyboard" else load_midi_mapping_file()
    expected = [
        "pitch.note:12:set_retrigger",
        "pitch.original:set",
        "pitch.extra.step:-1:set_retrigger",
    ]
    assert [entry.action.key for entry in restored.mappings] == expected
    context.audio.pads.set_pitch_retrigger_intent(0, enabled=False)
    context.audio.pads.set_extra_retrigger_intent(0, enabled=False)
    controller.project.selected_pad = 37
    for entry in restored.mappings:
        assert mapping.execute_action(entry.action, received_at_ns=999)
    assert [call.args[0] for call in execute.call_args_list] == expected
    assert all(call.kwargs == {"received_at_ns": 999} for call in execute.call_args_list)


@pytest.mark.parametrize("mode", ["button", "absolute", "twos_complement", "offset_binary"])
def test_cc_step_learn_saves_explicit_encoding_independently_of_later_ui_preference(
    controller: AppController, mode: PitchCcMode
) -> None:
    context = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)
    context.ui.set_pitch_cc_mode(mode)
    controller.session.input_learn_active = True
    controller.session.input_learn_pending_source = "midi"
    controller.session.input_learn_pending_binding_key = "midi:cc:1:71"
    context.audio.pads.step_pitch(
        0, 1, retrigger=True, cc_mode=context.state.pitch_menus.midi_cc_mode
    )
    entry = load_midi_mapping_file().mappings[0]
    action = parse_pitch_action(entry.action.key)
    assert action is not None
    assert action.cc_mode == mode
    assert action.target == 1
    assert action.retrigger
    context.ui.set_pitch_cc_mode("offset_binary" if mode == "button" else "button")
    assert load_midi_mapping_file().mappings[0] == entry


def test_encoder_preference_does_not_change_a_note_learn_or_normal_gui_step(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    context = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)
    controller.session.input_learn_active = True
    controller.session.input_learn_pending_source = "midi"
    controller.session.input_learn_pending_binding_key = "midi:note:1:60"
    context.audio.pads.step_pitch(0, -1, retrigger=False, cc_mode="twos_complement")
    assert load_midi_mapping_file().mappings[0].action.key == "pitch.extra.step:-1:set"
    audio_engine_mock.capture_pad_pitch_action.return_value = _as_native(_CapturedPitch(1))
    context.audio.pads.step_pitch(0, 1, retrigger=False, cc_mode="twos_complement")
    audio_engine_mock.capture_pad_pitch_action.assert_called_once()
    assert audio_engine_mock.capture_pad_pitch_action.call_args.args[1] == "pitch.extra.step:1:set"
