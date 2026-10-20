"""Real source-editor mouse frames; Native identities and input envelopes are synthetic."""

import json
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.persistence import PROJECT_CONFIG_PATH, ProjectPersistence
from flitzis_looper.key_intent import MAX_KEY_EPOCH, PadKeyIntent, SourceKeyVersion, recognize_key
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.controller.test_key_metadata_persistence import (
    _non_key_settings,
    _performer_project,
)
from tests.flitzis_looper.controller.transport.test_pitch_actions import _as_native, _CapturedPitch
from tests.flitzis_looper.ui import test_pitch_render as pitch_render_tests
from tests.flitzis_looper.ui.test_key_lock_render import _imgui_context

if TYPE_CHECKING:
    from unittest.mock import Mock

    from tests.flitzis_looper.ui.test_pitch_render import _Frame, _PitchRender

_ROOTS = ("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")
_SOURCE_LABELS = tuple(root + suffix for root in _ROOTS for suffix in ("", "m"))
pitch_render = pitch_render_tests.pitch_render


def _open_source(render: _PitchRender) -> _Frame:
    render.frame()
    frame = render.frame()
    render.click(frame.controls["pitch_source_edit"])
    # Give the real popup its final auto-sized geometry before choosing a key.
    return render.frame()


def _assert_no_audio_commands(audio: Mock) -> None:
    assert {call[0] for call in audio.mock_calls} <= {
        "waveform_source_identity",
        "current_constant_timing",
        "pad_timing_intent",
    }
    audio.capture_input_timestamp_ns.assert_not_called()
    audio.capture_pad_pitch_action.assert_not_called()
    audio.capture_selected_pitch_action.assert_not_called()
    audio.publish_pitch_action.assert_not_called()
    audio.cancel_pitch_action.assert_not_called()


def test_source_editor_keeps_legacy_label_punctuation_visible_and_editable(
    pitch_render: _PitchRender, audio_engine_mock: Mock
) -> None:
    controller = pitch_render.controller
    raw = "unknown ## detected reference"
    correction = "manual ## performer reference"
    controller.project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(raw_key=raw), correction=correction, base_shift=12, extra_shift=2
    )
    audio_engine_mock.reset_mock()
    with _imgui_context():
        frame = pitch_render.frame()
        frame = pitch_render.frame()
        assert "pitch_source_edit" in frame.controls
        assert f"{correction} (manual)" in frame.text
        frame = _open_source(pitch_render)
        pitch_render.click(frame.controls["pitch_source_clear"])
        frame = pitch_render.frame()
        assert raw in frame.text
        assert controller.project.pad_key_intent[0].source == SourceKeyVersion(raw_key=raw)
        assert controller.project.pad_key_intent[0].correction is None
    _assert_no_audio_commands(audio_engine_mock)


@pytest.mark.parametrize("label", _SOURCE_LABELS)
def test_actual_source_choice_preserves_playback_intent_and_one_central_piano(
    pitch_render: _PitchRender, audio_engine_mock: Mock, label: str
) -> None:
    controller = pitch_render.controller
    before = PadKeyIntent(
        source=SourceKeyVersion(version=7, raw_key="Em"),
        correction="unknown legacy label",
        analysis_epoch=11,
        correction_epoch=13,
        base_shift=12,
        extra_shift=2,
        retrigger=True,
        extra_retrigger=False,
    )
    controller.project.pad_key_intent[0] = before
    controller.project.speed = 1.25
    controller.project.multi_loop = True
    controller.project.manual_bpm[0] = 127.5
    controller.project.pad_loop_start_s[0] = 3.5
    controller.project.pad_loop_end_s[0] = 9.0
    controller.session.master_bpm = 112.0
    controller.session.active_sample_ids.add(0)
    controller.session.pad_playhead_s[0] = 12.5
    settings = _non_key_settings(controller.project)
    other = controller.project.pad_key_intent[1]
    with _imgui_context():
        frame = pitch_render.open()
        assert frame.piano is not None
        bounds, grid = frame.piano.bounds, frame.children["pad_grid"]
        audio_engine_mock.reset_mock()
        frame = _open_source(pitch_render)
        assert frame.labels[f"pitch_source_key_{label}"].rstrip() == label
        pitch_render.click(frame.controls[f"pitch_source_key_{label}"])
        frame = pitch_render.frame()
        chosen = controller.project.pad_key_intent[0]
        assert chosen == before.changed(correction=label, correction_epoch=14)
        assert chosen.source_key == recognize_key(label)
        assert f"{label} (manual)" in frame.text
        assert frame.piano is not None
        assert frame.piano.bounds == bounds
        assert frame.children["pad_grid"] == grid
        frame.piano.assert_markers(12, original_value=4 - _ROOTS.index(label.removesuffix("m")))
        assert frame.piano_items == 1
        assert pitch_render.ctx.state.pitch_menus.base_open
    assert controller.project.pad_key_intent[1] == other
    assert _non_key_settings(controller.project) == settings
    assert controller.session.master_bpm == 112.0
    assert controller.session.active_sample_ids == {0}
    assert controller.session.pad_playhead_s[0] == 12.5
    assert controller.persistence._dirty
    _assert_no_audio_commands(audio_engine_mock)


@pytest.mark.parametrize("source", ["Em", "unrecognized source", None])
def test_actual_use_detected_source_clears_only_correction_and_keeps_unknown_metadata(
    pitch_render: _PitchRender, audio_engine_mock: Mock, source: str | None
) -> None:
    controller = pitch_render.controller
    before = PadKeyIntent(
        source=SourceKeyVersion(version=7, raw_key=source) if source is not None else None,
        correction="Dm",
        analysis_epoch=11,
        correction_epoch=13,
        base_shift=-12,
        extra_shift=5,
        retrigger=True,
        extra_retrigger=False,
    )
    controller.project.pad_key_intent[0] = before
    with _imgui_context():
        pitch_render.open()
        audio_engine_mock.reset_mock()
        frame = _open_source(pitch_render)
        assert frame.labels["pitch_source_clear"] == "Use detected source"
        pitch_render.click(frame.controls["pitch_source_clear"])
        frame = pitch_render.frame()
        cleared = controller.project.pad_key_intent[0]
        assert cleared == before.changed(correction=None, correction_epoch=14)
        assert (source or "Unknown") in frame.text
        assert frame.piano is not None
        frame.piano.assert_markers(-12, original_value=0 if source == "Em" else None)
        revision = controller.persistence.revision
        frame = _open_source(pitch_render)
        pitch_render.click(frame.controls["pitch_source_clear"])
        assert controller.project.pad_key_intent[0] == cleared
        assert controller.persistence.revision == revision
    _assert_no_audio_commands(audio_engine_mock)


def test_actual_same_root_mode_change_and_independent_clear_survive_real_json_roundtrip(
    pitch_render: _PitchRender, audio_engine_mock: Mock
) -> None:
    controller = pitch_render.controller
    before = _performer_project().pad_key_intent[0].changed(correction="E")
    controller.project.pad_key_intent[0] = before
    settings = _non_key_settings(controller.project)
    with _imgui_context():
        frame = _open_source(pitch_render)
        audio_engine_mock.reset_mock()
        pitch_render.click(frame.controls["pitch_source_key_Em"])
        changed = controller.project.pad_key_intent[0]
        assert changed == before.changed(correction="Em", correction_epoch=14)
        assert changed.source_key is not None
        assert changed.source_key.mode == "minor"
        assert _non_key_settings(controller.project) == settings
        _assert_no_audio_commands(audio_engine_mock)
        controller.persistence.flush(now=0.0)
        saved = json.loads(PROJECT_CONFIG_PATH.read_text(encoding="utf-8"))
        assert saved["manual_key"][0] == "Em"
        assert saved["pad_key_intent"][0] == changed.model_dump(mode="json")
        reopened = ProjectPersistence.from_config_path().project
        assert reopened.pad_key_intent == controller.project.pad_key_intent
        assert reopened.pad_content == controller.project.pad_content
        frame = _open_source(pitch_render)
        audio_engine_mock.reset_mock()
        pitch_render.click(frame.controls["pitch_source_clear"])
        cleared = controller.project.pad_key_intent[0]
        assert cleared == changed.changed(correction=None, correction_epoch=15)
        _assert_no_audio_commands(audio_engine_mock)
        controller.persistence.flush(now=1.0)
    saved = json.loads(PROJECT_CONFIG_PATH.read_text(encoding="utf-8"))
    assert saved["manual_key"][0] is None
    assert saved["pad_key_intent"][0]["correction"] is None
    assert ProjectPersistence.from_config_path().project.pad_key_intent[0] == cleared


def test_captured_midi_envelope_keeps_tuple_owner_and_time_after_rendered_source_edit(
    pitch_render: _PitchRender, audio_engine_mock: Mock
) -> None:
    """The captured Python fake proves orchestration, not Native audio effectiveness."""
    controller = pitch_render.controller
    controller.input_mapping.set_enabled(enabled=True)
    original = _CapturedPitch(1, base_shift=-4, extra_shift=2, received_at_ns=777)
    event: dict[str, object] = {
        "source": "midi",
        "binding_key": "midi:note:1:60",
        "action_key": "pitch.note:-4:set",
        "value": 100,
        "captured_pitch": _as_native(original),
        "received_at_ns": 777,
        "direct": True,
        "dispatched": False,
    }
    with _imgui_context():
        frame = _open_source(pitch_render)
        audio_engine_mock.reset_mock()
        pitch_render.click(frame.controls["pitch_source_key_Fm"])
        _assert_no_audio_commands(audio_engine_mock)
        controller.project.selected_pad = 1
        controller.input_mapping._handle_rust_input_event(event)
        controller.input_mapping._handle_rust_input_event(event | {"value": 0})
        pitch_render.frame()
    intent = controller.project.pad_key_intent[0]
    assert (intent.base_shift, intent.extra_shift, intent.correction) == (-4, 2, "Fm")
    assert intent.source == SourceKeyVersion(raw_key="C")
    assert controller.project.pad_key_intent[1].base_shift == -2
    assert controller.project.selected_pad == 1
    assert (original.base_shift, original.extra_shift, original.received_at_ns) == (-4, 2, 777)
    assert original.content_lifetime == 77
    audio_engine_mock.publish_pitch_action.assert_called_once_with(original)
    audio_engine_mock.capture_pad_pitch_action.assert_not_called()
    audio_engine_mock.capture_selected_pitch_action.assert_not_called()


def test_rendered_source_edit_retains_already_published_pending_pitch_owner(
    pitch_render: _PitchRender, audio_engine_mock: Mock
) -> None:
    """Synthetic Native preparation stays pending; this makes no effectiveness claim."""
    controller = pitch_render.controller
    original = _CapturedPitch(1, base_shift=-4, extra_shift=2, received_at_ns=777)
    native_capture = _as_native(original)
    assert controller.transport.pitch.accept_captured_event(native_capture)
    owner = controller.transport.pitch._owners[1]
    assert owner.capture is native_capture
    assert controller.transport.pitch.status(0) == ("preparing", None)
    with _imgui_context():
        frame = _open_source(pitch_render)
        audio_engine_mock.reset_mock()
        pitch_render.click(frame.controls["pitch_source_key_Fm"])
        assert controller.transport.pitch._owners == {1: owner}
        assert owner.capture is native_capture
        assert original.state == "preparing"
        assert not original.effective
        assert controller.transport.pitch.status(0) == ("preparing", None)
        intent = controller.project.pad_key_intent[0]
        assert (intent.base_shift, intent.extra_shift, intent.correction) == (-4, 2, "Fm")
    _assert_no_audio_commands(audio_engine_mock)


@pytest.mark.parametrize("replacement", ["selection", "content", "path", "native_generation"])
def test_open_source_editor_rejects_old_choice_after_owner_replacement(
    pitch_render: _PitchRender, audio_engine_mock: Mock, replacement: str
) -> None:
    controller = pitch_render.controller
    with _imgui_context():
        frame = _open_source(pitch_render)
        old_choice = frame.controls["pitch_source_key_F#m"]
        if replacement == "selection":
            controller.project.selected_pad = 1
        elif replacement == "content":
            controller.project.pad_content[0] = PadContentIdentity(instance_id="c" * 32)
            controller.project.pad_key_intent[0] = PadKeyIntent(
                source=SourceKeyVersion(raw_key="G"), correction="A", base_shift=5, extra_shift=-2
            )
        elif replacement == "path":
            controller.project.sample_paths[0] = "samples/synthetic-replacement.wav"
        else:
            # Identical digest, shape and path do not hide a Native generation ABA.
            audio_engine_mock.waveform_source_identity.return_value = (8, "b" * 64, 128, 44_100)
        expected = tuple(controller.project.pad_key_intent)
        audio_engine_mock.reset_mock()
        frame = pitch_render.click(old_choice)
        assert tuple(controller.project.pad_key_intent) == expected
        assert not any(identifier.startswith("pitch_source_key_") for identifier in frame.controls)
        _assert_no_audio_commands(audio_engine_mock)
        frame = _open_source(pitch_render)
        pitch_render.click(frame.controls["pitch_source_key_D#"])
        selected = controller.project.selected_pad
        assert controller.project.pad_key_intent[selected] == expected[selected].corrected("D#")
        assert controller.project.pad_key_intent[1 - selected] == expected[1 - selected]
    _assert_no_audio_commands(audio_engine_mock)


def test_open_source_editor_uses_latest_analysis_without_treating_it_as_source_replacement(
    pitch_render: _PitchRender, audio_engine_mock: Mock
) -> None:
    controller = pitch_render.controller
    with _imgui_context():
        frame = _open_source(pitch_render)
        choice = frame.controls["pitch_source_key_F#m"]
        current = (
            controller.project
            .pad_key_intent[0]
            .changed(source=SourceKeyVersion(version=8, raw_key="Bm"), analysis_epoch=12)
            .corrected("D")
        )
        controller.project.pad_key_intent[0] = current
        controller.project.pad_key_lock[0] = True
        audio_engine_mock.reset_mock()
        pitch_render.click(choice)
        assert controller.project.pad_key_intent[0] == current.corrected("F#m")
        assert controller.project.pad_key_lock[0] is True
    _assert_no_audio_commands(audio_engine_mock)


@pytest.mark.parametrize(
    ("failure", "reason"),
    [
        ("epoch", "key metadata epoch capacity exhausted"),
        ("reserved", "reserved arrangement target"),
    ],
)
def test_source_edit_guard_failure_retains_popup_and_state_then_allows_fresh_retry(
    pitch_render: _PitchRender, audio_engine_mock: Mock, failure: str, reason: str
) -> None:
    controller = pitch_render.controller
    before = PadKeyIntent(
        source=SourceKeyVersion(version=7, raw_key="Em"),
        correction="D",
        analysis_epoch=11,
        correction_epoch=MAX_KEY_EPOCH if failure == "epoch" else 13,
        base_shift=12,
        extra_shift=2,
        retrigger=True,
        extra_retrigger=False,
    )
    controller.project.pad_key_intent[0] = before
    previous_guard = controller.transport._pad_edit_guard
    blocked = True

    def check_edit(slot: int) -> None:
        if failure == "reserved" and blocked and slot == 0:
            raise ValueError(reason)
        if previous_guard is not None:
            previous_guard(slot)

    controller.transport.bind_pad_edit_guard(check_edit)
    intents = tuple(controller.project.pad_key_intent)
    settings = _non_key_settings(controller.project)
    persistence = controller.persistence.revision, controller.persistence._dirty
    with _imgui_context():
        frame = _open_source(pitch_render)
        audio_engine_mock.reset_mock()
        pitch_render.click(frame.controls["pitch_source_key_F#m"])
        pitch_render.frame()
        frame = pitch_render.frame()
        assert not any(reason in text for text in frame.text)
        assert pitch_render.ctx.ui.source_key.error == reason
        assert pitch_render.ctx.ui.source_key.pending is not None
        assert {
            identifier
            for identifier in frame.controls
            if identifier.startswith("pitch_source_key_")
        } == {f"pitch_source_key_{label}" for label in _SOURCE_LABELS}
        assert tuple(controller.project.pad_key_intent) == intents
        assert _non_key_settings(controller.project) == settings
        assert (controller.persistence.revision, controller.persistence._dirty) == persistence
        _assert_no_audio_commands(audio_engine_mock)
        blocked = False
        retry = before.changed(correction_epoch=13)
        controller.project.pad_key_intent[0] = retry
        pitch_render.click(frame.controls["pitch_source_key_F#m"])
        assert controller.project.pad_key_intent[0] == retry.changed(
            correction="F#m", correction_epoch=14
        )
        assert controller.persistence.revision == persistence[0] + 1
        assert controller.persistence._dirty
        assert pitch_render.ctx.ui.source_key.pending is None
        assert pitch_render.ctx.ui.source_key.error is None
        frame = _open_source(pitch_render)
        assert "pitch_source_key_F#m" in frame.controls
        assert not any(text.startswith("Source correction unavailable:") for text in frame.text)
    _assert_no_audio_commands(audio_engine_mock)
