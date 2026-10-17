from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.input_mapping.actions import (
    describe_pitch_rejection,
    parse_pitch_action,
    pitch_base_action,
    pitch_extra_action,
    pitch_reset_base_action,
)
from flitzis_looper.input_mapping.bindings import KeyboardBinding
from flitzis_looper.input_mapping.storage import (
    load_keyboard_mapping_file,
    load_midi_mapping_file,
)
from flitzis_looper.key_intent import MusicalKey, PadKeyIntent, SourceKeyVersion
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.controller.transport.test_pitch_actions import (
    _CapturedPitch,
    _pitch_fixture,
)
from tests.flitzis_looper.input_mapping.test_current_runtime import _configure_accepted_pad

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


class _RequestedPitchShadow:
    """Scalar Native journal double; real source-fenced rollback has Rust coverage."""

    def __init__(self) -> None:
        self.requested = (0, 0)
        self.latest: _CapturedPitch | None = None
        self.sequence = 0
        self.previous: dict[int, tuple[tuple[int, int], _CapturedPitch | None]] = {}

    def capture(self, base: int, extra: int, timestamp: int = 123) -> _CapturedPitch:
        self.sequence += 1
        receipt = _CapturedPitch(
            self.sequence, base_shift=base, extra_shift=extra, received_at_ns=timestamp
        )
        self.previous[receipt.accepted_sequence] = self.requested, self.latest
        self.requested, self.latest = (base, extra), receipt
        return receipt

    def capture_selected(
        self, action_key: str, *, received_at_ns: int | None = None
    ) -> _CapturedPitch:
        assert action_key == "pitch.base:7:minor:set"
        # The fixture source is E minor; G minor is +3, retaining Native extra.
        return self.capture(3, self.requested[1], received_at_ns or 0)

    def cancel(self, receipt: _CapturedPitch) -> bool:
        if receipt.state in {"applied", "rejected"}:
            return False
        receipt.state, receipt.reason = "rejected", "stale_content"
        predecessor = self.previous.pop(receipt.accepted_sequence)
        # Native removes canceled journal records and redirects their remaining
        # direct links immediately, even when this receipt is not the latest.
        for sequence, (_intent, previous) in tuple(self.previous.items()):
            if previous is receipt:
                self.previous[sequence] = predecessor
        if self.latest is receipt:
            self.requested, self.latest = predecessor
        return True


def _pitch_event(receipt: _CapturedPitch) -> dict[str, object]:
    return {
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": f"pitch.extra:{receipt.extra_shift}:set",
        "value": 100,
        "received_at_ns": receipt.received_at_ns,
        "captured_pitch": receipt,
        "direct": True,
        "dispatched": False,
    }


def _discard_pitch_event(
    controller: AppController, audio: Mock, event: dict[str, object], mode: str
) -> None:
    mapping = controller.input_mapping
    if mode == "disabled":
        mapping.set_enabled(enabled=False)
        mapping._handle_rust_input_event(event)
    elif mode == "learn":
        controller.session.input_learn_active = True
        mapping._handle_rust_input_event(event)
        assert controller.session.input_learn_pending_source == "midi"
        assert controller.session.input_learn_pending_binding_key == "midi:note:1:60"
    else:
        audio.poll_input_events.side_effect = [event, None]
        mapping.toggle_learn()
        assert controller.session.input_learn_active is True
        assert controller.session.input_learn_pending_binding_key is None
        mapping._handle_rust_input_event({
            "source": "midi",
            "binding_key": "midi:cc:1:71",
            "value": 64,
        })
        assert controller.session.input_learn_pending_binding_key == "midi:cc:1:71"


def _discard_pitch_burst(
    controller: AppController,
    audio: Mock,
    receipts: list[_CapturedPitch],
    mode: str,
    *,
    separate_polls: bool,
) -> None:
    mapping = controller.input_mapping
    if mode == "disabled":
        mapping.set_enabled(enabled=False)
    elif mode == "learn":
        controller.session.input_learn_active = True
    batches = [[receipt] for receipt in receipts] if separate_polls else [receipts]
    for index, batch in enumerate(batches):
        audio.poll_input_events.side_effect = [*(_pitch_event(receipt) for receipt in batch), None]
        if mode != "drain":
            mapping._poll_rust_input_events()
        elif index == 0:
            mapping.toggle_learn()
        else:
            mapping._drain_rust_input_events()


@pytest.mark.parametrize("mode", ["disabled", "learn", "drain"])
def test_discarded_native_pitch_is_cancelled_before_next_base_composes_requested_shadow(
    controller: AppController, audio_engine_mock: Mock, mode: str
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    controller.input_mapping.set_enabled(enabled=True)
    native = _RequestedPitchShadow()
    discarded = native.capture(0, 2)
    audio_engine_mock.cancel_pitch_action.side_effect = native.cancel
    audio_engine_mock.capture_selected_pitch_action.side_effect = native.capture_selected

    _discard_pitch_event(controller, audio_engine_mock, _pitch_event(discarded), mode)

    audio_engine_mock.cancel_pitch_action.assert_called_once_with(discarded)
    assert audio_engine_mock.cancel_pitch_action.call_args.args[0] is discarded
    assert discarded.state == "rejected"
    assert native.requested == (0, 0)
    audio_engine_mock.publish_pitch_action.assert_not_called()
    controller.session.input_learn_active = False
    controller.input_mapping.set_enabled(enabled=True)
    assert controller.input_mapping.execute_action(
        pitch_base_action(MusicalKey(root=7, mode="minor"), retrigger=False), received_at_ns=777
    )
    following = native.latest
    assert following is not None
    assert following is not discarded
    audio_engine_mock.publish_pitch_action.assert_called_once_with(following)
    assert (following.base_shift, following.extra_shift) == (3, 0)
    assert following.received_at_ns == 777
    assert following.accepted is True
    assert controller.project.pad_key_intent[0].extra_shift == 0


@pytest.mark.parametrize("mode", ["disabled", "learn", "drain"])
@pytest.mark.parametrize("extras", [(2, 5), (2, 5, 7)], ids=["two", "three"])
@pytest.mark.parametrize("separate_polls", [False, True], ids=["one-poll", "separate-polls"])
def test_discarded_fifo_burst_never_resurrects_an_earlier_requested_pitch(
    controller: AppController,
    audio_engine_mock: Mock,
    mode: str,
    extras: tuple[int, ...],
    *,
    separate_polls: bool,
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    controller.input_mapping.set_enabled(enabled=True)
    native = _RequestedPitchShadow()
    discarded = [native.capture(0, extra, 123 + index) for index, extra in enumerate(extras)]
    audio_engine_mock.cancel_pitch_action.side_effect = native.cancel
    audio_engine_mock.capture_selected_pitch_action.side_effect = native.capture_selected

    _discard_pitch_burst(
        controller, audio_engine_mock, discarded, mode, separate_polls=separate_polls
    )

    cancellations = audio_engine_mock.cancel_pitch_action.call_args_list
    assert len(cancellations) == len(discarded)
    assert all(
        call.args[0] is receipt for call, receipt in zip(cancellations, discarded, strict=True)
    )
    assert all(receipt.state == "rejected" for receipt in discarded)
    assert native.latest is None
    assert native.requested == (0, 0)
    audio_engine_mock.publish_pitch_action.assert_not_called()
    audio_engine_mock.capture_selected_pitch_action.assert_not_called()
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    controller.session.input_learn_active = False
    controller.input_mapping.set_enabled(enabled=True)
    assert controller.input_mapping.execute_action(
        pitch_base_action(MusicalKey(root=7, mode="minor"), retrigger=False), received_at_ns=777
    )
    following = native.latest
    assert following is not None
    assert all(following is not receipt for receipt in discarded)
    audio_engine_mock.publish_pitch_action.assert_called_once_with(following)
    assert (following.base_shift, following.extra_shift) == (3, 0)
    assert following.received_at_ns == 777
    assert following.accepted is True
    assert controller.project.pad_key_intent[0].extra_shift == 0
    assert audio_engine_mock.cancel_pitch_action.call_count == len(discarded)


@pytest.mark.parametrize("mode", ["disabled", "learn", "drain"])
def test_discarding_older_native_receipt_never_cancels_or_rolls_back_newer_owner(
    controller: AppController, audio_engine_mock: Mock, mode: str
) -> None:
    _pitch_fixture(controller, audio_engine_mock)
    controller.input_mapping.set_enabled(enabled=True)
    native = _RequestedPitchShadow()
    discarded = native.capture(0, 2)
    newer = native.capture(0, 5)
    audio_engine_mock.cancel_pitch_action.side_effect = native.cancel

    _discard_pitch_event(controller, audio_engine_mock, _pitch_event(discarded), mode)

    audio_engine_mock.cancel_pitch_action.assert_called_once_with(discarded)
    assert audio_engine_mock.cancel_pitch_action.call_args.args[0] is discarded
    assert discarded.state == "rejected"
    assert native.latest is newer
    assert native.requested == (0, 5)
    assert newer.state == "captured"
    assert newer.accepted is False
    audio_engine_mock.publish_pitch_action.assert_not_called()


@pytest.mark.parametrize("retrigger", [False, True])
def test_pitch_actions_keep_semantic_target_and_variant(*, retrigger: bool) -> None:
    variant = "set_retrigger" if retrigger else "set"
    key = pitch_base_action(MusicalKey(root=1, mode="minor"), retrigger=retrigger)
    assert key.key == f"pitch.base:1:minor:{variant}"
    assert key.source == "direct"
    parsed = parse_pitch_action(key.key)
    assert parsed is not None
    assert parsed.target == MusicalKey(root=1, mode="minor")
    assert parsed.retrigger is retrigger
    for value in range(-18, 19):
        extra = pitch_extra_action(value, retrigger=retrigger)
        assert extra.key == f"pitch.extra:{value}:{variant}"
        assert extra.source == "direct"
        parsed = parse_pitch_action(extra.key)
        assert parsed is not None
        assert parsed.target == value
        assert parsed.retrigger is retrigger
    reset = pitch_reset_base_action(retrigger=retrigger)
    assert reset.key == f"pitch.base.reset:{variant}"
    assert reset.source == "direct"


@pytest.mark.parametrize("value", [True, False, 2.0, "2", None])
def test_semantic_action_rejects_noninteger_extra_without_coercion(value: object) -> None:
    with pytest.raises(TypeError, match="integer"):
        pitch_extra_action(value, retrigger=False)  # type: ignore[arg-type]


@pytest.mark.parametrize("value", [-19, 19])
def test_semantic_action_rejects_out_of_range_without_clamping(value: int) -> None:
    with pytest.raises(ValueError, match="between"):
        pitch_extra_action(value, retrigger=False)


@pytest.mark.parametrize(
    "key",
    [
        "pitch.base:12:major:set",
        "pitch.base:0:dorian:set",
        "pitch.extra:2.0:set",
        "pitch.extra:19:set",
        "pitch.extra:0:toggle",
        "pitch.extra:00:set",
        "pitch.base.reset:toggle",
    ],
)
def test_invalid_pitch_keys_have_no_handler(key: str) -> None:
    assert parse_pitch_action(key) is None


@pytest.mark.parametrize("source", ["keyboard", "midi"])
def test_dual_zero_learn_restores_both_variants_after_checkbox_and_selection_changes(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, source: str
) -> None:
    ctx = UiContext(controller)
    mapping = controller.input_mapping
    mapping.set_enabled(enabled=True)
    execute = Mock()
    monkeypatch.setattr(controller.transport.pitch, "accept_selected_action", execute)
    bindings = (
        [KeyboardBinding(key_name=name).key for name in ("A", "B")]
        if source == "keyboard"
        else ["midi:note:1:60", "midi:note:1:61"]
    )
    for binding, retrigger in zip(bindings, (False, True), strict=True):
        ctx.audio.pads.set_pitch_retrigger_intent(0, enabled=retrigger)
        controller.session.input_learn_active = True
        controller.session.input_learn_pending_source = source
        controller.session.input_learn_pending_binding_key = binding
        ctx.audio.pads.choose_extra_shift(0, 0, retrigger=ctx.state.pads.key_intent(0).retrigger)
    execute.assert_not_called()
    restored = load_keyboard_mapping_file() if source == "keyboard" else load_midi_mapping_file()
    assert [entry.action.key for entry in restored.mappings] == [
        "pitch.extra:0:set",
        "pitch.extra:0:set_retrigger",
    ]
    assert all(entry.action.source == "direct" for entry in restored.mappings)
    ctx.audio.pads.set_pitch_retrigger_intent(0, enabled=False)
    controller.project.selected_pad = 37
    for entry in restored.mappings:
        assert mapping.execute_action(entry.action, received_at_ns=999)
    assert execute.call_args_list[0].args == ("pitch.extra:0:set",)
    assert execute.call_args_list[0].kwargs == {"received_at_ns": 999}
    assert execute.call_args_list[1].args == ("pitch.extra:0:set_retrigger",)
    assert execute.call_args_list[1].kwargs == {"received_at_ns": 999}


def test_native_pitch_fallback_passes_the_original_capture_after_selection_changes(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    original_capture = object()
    accept = Mock()
    choose = Mock()
    monkeypatch.setattr(controller.transport.pitch, "accept_captured_event", accept)
    monkeypatch.setattr(controller.transport.pad, "choose_extra_shift", choose)
    controller.project.selected_pad = 42
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pitch.extra:2:set_retrigger",
        "received_at_ns": 777,
        "captured_pitch": original_capture,
        "direct": True,
        "dispatched": False,
    })
    accept.assert_called_once_with(original_capture)
    choose.assert_not_called()


@pytest.mark.parametrize("rejection", [None, "unsupported_pitch", "stale_content"])
def test_missing_or_rejected_native_pitch_never_recaptures_the_new_selection(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, rejection: str | None
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    accept = Mock()
    choose = Mock()
    monkeypatch.setattr(controller.transport.pitch, "accept_captured_event", accept)
    monkeypatch.setattr(controller.transport.pad, "choose_extra_shift", choose)
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pitch.extra:2:set",
        "pitch_rejection": rejection,
        "direct": True,
        "dispatched": False,
    })
    accept.assert_not_called()
    choose.assert_not_called()
    assert controller.session.input_mapping_error
    if rejection:
        assert describe_pitch_rejection(rejection) in controller.session.input_mapping_error


@pytest.mark.parametrize("release", [{"value": 0}, {"velocity": 0}, {"kind": "note_off"}])
def test_pitch_release_has_no_attack_reset_hold_or_learn_semantics(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, release: dict[str, object]
) -> None:
    ctx = UiContext(controller)
    controller.input_mapping.set_enabled(enabled=True)
    ctx.ui.toggle_pitch_menu("extra")
    accept = Mock()
    hold_release = Mock()
    monkeypatch.setattr(controller.transport.pitch, "accept_captured_event", accept)
    monkeypatch.setattr(controller.transport.waveform, "request_hold_release", hold_release)
    controller.project.pad_key_intent[0] = controller.project.pad_key_intent[0].changed(
        extra_shift=2
    )
    before = ctx.state.pads.key_intent(0)
    event = {
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pitch.extra:0:set_retrigger",
        "captured_pitch": object(),
    } | release
    controller.input_mapping._handle_rust_input_event(event)
    controller.session.input_learn_active = True
    controller.input_mapping._handle_rust_input_event(event)
    accept.assert_not_called()
    hold_release.assert_not_called()
    assert controller.session.input_learn_pending_binding_key is None
    assert ctx.state.pads.key_intent(0) == before
    assert ctx.state.pitch_menus.extra_open


def test_repeated_native_attacks_remain_distinct_and_release_does_not_repeat_them(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    accept = Mock()
    monkeypatch.setattr(controller.transport.pitch, "accept_captured_event", accept)
    captures = [object(), object()]
    event: dict[str, object] = {
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pitch.extra:2:set_retrigger",
        "value": 100,
    }
    for capture in captures:
        controller.input_mapping._handle_rust_input_event(event | {"captured_pitch": capture})
    controller.input_mapping._handle_rust_input_event(event | {"value": 0})
    assert [call.args[0] for call in accept.call_args_list] == captures


def test_runtime_pitch_projection_keeps_unknown_correction_authoritative_and_strict(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(controller.transport.pitch, "projection_sequence", lambda _pad: 23)
    controller.project.sample_paths[0] = "samples/current.wav"
    controller.project.key_lock = True
    controller.project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(raw_key="Em"),
        correction="unrecognized legacy key",
        base_shift=-4,
        extra_shift=12,
    )
    controller.project.pad_key_lock[0] = True
    controller.project.trigger_quantization_enabled = True
    controller.project.trigger_quantization_step = "1_32"
    controller.project.bpm_lock = True
    payload = controller.input_mapping._input_pitch_projection(0).payload()
    assert payload == {
        "source_root": 4,
        "source_mode": "minor",
        "correction_root": None,
        "correction_mode": None,
        "correction_unknown": True,
        "base_shift": -4,
        "extra_shift": 12,
        "key_lock": True,
        "quantize_step_64ths": 2,
        "sync": True,
        "request_sequence": 23,
    }
    controller.project.pad_key_intent[0] = controller.project.pad_key_intent[0].corrected("Dm")
    payload = controller.input_mapping._input_pitch_projection(0).payload()
    assert payload["source_root"] == 4
    assert payload["correction_root"] == 2
    assert payload["source_mode"] == payload["correction_mode"] == "minor"
    assert payload["base_shift"] == -4
    assert payload["extra_shift"] == 12


def test_pitch_metadata_and_numeric_changes_republish_one_atomic_native_snapshot(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    _configure_accepted_pad(controller, audio_engine_mock)
    audio_engine_mock.capture_selected_pitch_action = Mock()
    controller.project.pad_key_intent[0] = PadKeyIntent(source=SourceKeyVersion(raw_key="Em"))
    controller.input_mapping._sync_rust_runtime_state()
    audio_engine_mock.reset_mock()
    controller.project.pad_key_intent[0] = controller.project.pad_key_intent[0].changed(
        extra_shift=2
    )
    controller.input_mapping._sync_rust_runtime_state()
    audio_engine_mock.set_input_runtime_state.assert_called_once()
    call = audio_engine_mock.set_input_runtime_state.call_args
    assert call.args[4][0] is not None

    assert call.kwargs["pitch_rows"][0]["extra_shift"] == 2
    assert len(call.kwargs["pitch_rows"]) == 216
    audio_engine_mock.reset_mock()
    controller.input_mapping._sync_rust_runtime_state()
    audio_engine_mock.set_input_runtime_state.assert_not_called()
    controller.project.pad_key_intent[0] = controller.project.pad_key_intent[0].corrected("Dm")
    controller.input_mapping._sync_rust_runtime_state()
    audio_engine_mock.set_input_runtime_state.assert_called_once()
    assert (
        audio_engine_mock.set_input_runtime_state.call_args.kwargs["pitch_rows"][0][
            "correction_root"
        ]
        == 2
    )


@pytest.mark.parametrize("quantized", [False, True])
def test_public_runtime_rows_encode_disabled_quantize_as_none_for_empty_and_loaded_pads(
    controller: AppController, audio_engine_mock: Mock, *, quantized: bool
) -> None:
    _configure_accepted_pad(controller, audio_engine_mock)
    controller.project.trigger_quantization_enabled = quantized
    controller.project.trigger_quantization_step = "1_32"
    controller.input_mapping._sync_rust_runtime_state()
    rows = audio_engine_mock.set_input_runtime_state.call_args.kwargs["pitch_rows"]
    assert rows[0]["quantize_step_64ths"] == (2 if quantized else None)
    assert all(row["quantize_step_64ths"] is None for row in rows[1:])


def test_midi_selection_receipt_projects_only_its_original_native_revision(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller.input_mapping.set_enabled(enabled=True)
    select = Mock()
    monkeypatch.setattr(controller, "select_pad", select)
    controller.input_mapping._handle_rust_input_event({
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "ui.select_pad:7",
        "value": 100,
        "selection_revision": 123,
    })
    select.assert_called_once_with(
        7, native_selection_already_applied=True, expected_native_selection_revision=123
    )
