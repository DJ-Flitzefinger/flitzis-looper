from typing import TYPE_CHECKING, Literal
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.key_intent import MusicalKey, PadKeyIntent, SourceKeyVersion
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import performance_view
from flitzis_looper.ui.render.pitch_keyboard import (
    base_key_choices,
    extra_shift_choices,
    format_extra_shift,
    keyboard_geometry,
)
from tests.flitzis_looper.ui.test_key_lock_render import _imgui_context

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


@pytest.mark.parametrize("root", [0, 2])
def test_all_37_signed_keys_have_centered_zero_and_distinct_octave_targets(root: int) -> None:
    keys = extra_shift_choices(MusicalKey(root=root, mode="minor"))
    cells = keyboard_geometry(keys)
    assert [key.value for key in keys] == list(range(-18, 19))
    assert len({key.value for key in keys}) == 37
    assert cells[18].key.value == 0
    assert cells[18].center_x == (cells[-1].x + cells[-1].width) / 2.0
    octaves = [keys[value + 18] for value in (-12, 0, 12)]
    assert len({key.label for key in octaves}) == 3
    assert {key.label.splitlines()[1] for key in octaves} == {"C" if root == 0 else "D"}
    assert "-1 oct" in octaves[0].label
    assert "+1 oct" in octaves[2].label


def test_keyboard_colors_follow_base_root_and_use_unique_sharp_names() -> None:
    c = extra_shift_choices(MusicalKey(root=0, mode="major"))
    d = extra_shift_choices(MusicalKey(root=2, mode="major"))
    assert c[18].black is False
    assert c[19].black is True
    assert d[18].black is False
    assert d[19].black is True
    assert c[21].black is True
    assert d[21].black is False
    assert c[19].label.splitlines()[1] == "C#"
    assert d[19].label.splitlines()[1] == "D#"
    for keys in (c, d):
        assert len({key.label.splitlines()[1] for key in keys[18:30]}) == 12
        assert all("b" not in key.label.splitlines()[1] for key in keys)


def test_unknown_source_retains_all_signed_targets_without_guessing_key_color() -> None:
    keys = extra_shift_choices(None)
    assert [key.value for key in keys] == list(range(-18, 19))
    assert all(key.black is None for key in keys)
    assert keys[18].label == "0 st"


@pytest.mark.parametrize("mode", ["major", "minor"])
def test_absolute_keyboard_has_twelve_same_mode_roots(mode: str) -> None:
    musical_mode: Literal["major", "minor"] = "major" if mode == "major" else "minor"
    keys = base_key_choices(musical_mode)
    assert len(keys) == 12
    assert [key.value for key in keys] == list(range(12))
    assert keys[0].label == ("C" if mode == "major" else "Cm")
    assert keys[1].label == ("C#" if mode == "major" else "C#m")


def test_piano_expansion_is_global_across_pad_selection_and_context_local(
    controller: AppController,
) -> None:
    ctx = UiContext(controller)
    other = UiContext(controller)
    ctx.ui.toggle_pitch_menu("base")
    assert ctx.state.pitch_menus.base_open
    assert not other.state.pitch_menus.base_open
    controller.project.selected_pad = 1
    assert ctx.state.pitch_menus.base_open
    controller.project.selected_pad = 0
    assert ctx.state.pitch_menus.base_open
    ctx.ui.toggle_pitch_menu("base")
    assert not ctx.state.pitch_menus.base_open
    assert not other.state.pitch_menus.base_open


@pytest.mark.parametrize("source", ["Em", "unrecognized"])
def test_shared_piano_routes_signed_octave_target_and_selected_pad_intent(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, source: str
) -> None:
    ctx = UiContext(controller)
    intent = PadKeyIntent(
        source=SourceKeyVersion(raw_key=source), base_shift=-4, extra_shift=2, retrigger=True
    )
    controller.project.selected_pad = 1
    controller.project.pad_key_intent[1] = intent
    ctx.ui.toggle_pitch_menu("base")
    chosen_note = Mock()
    monkeypatch.setattr(ctx.audio.pads, "choose_piano_note", chosen_note)
    monkeypatch.setattr(ctx.input, "capture_timestamp_ns", lambda: 123)
    surfaces: list[tuple[str, int | None, int, float]] = []

    def choose(
        _id: str,
        *,
        source_root: int | None,
        selected_shift: int,
        height: float,
        original_root: int | None,
    ) -> int:
        surfaces.append((_id, source_root, selected_shift, height))
        return -12

    monkeypatch.setattr(performance_view, "render_piano_keyboard", choose)
    with _imgui_context():
        imgui.new_frame()
        imgui.begin("Piano routing", flags=imgui.WindowFlags_.no_saved_settings)
        expected_height = 120.0 - imgui.get_text_line_height_with_spacing()
        performance_view._render_piano(ctx, 120.0)
        imgui.end()
        imgui.render()
    assert surfaces == [("performance_piano", 4 if source == "Em" else None, -4, expected_height)]
    chosen_note.assert_called_once_with(1, -12, retrigger=True, received_at_ns=123)
    assert ctx.state.pitch_menus.base_open
    assert controller.project.pad_key_intent[1] == intent
    assert format_extra_shift(-12) != format_extra_shift(0) != format_extra_shift(12)


@pytest.mark.parametrize("phase", ["rejected", "failed", "preparing"])
def test_shared_piano_keeps_technical_errors_out_of_its_status_line(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, phase: str
) -> None:
    ctx = UiContext(controller)
    reason = "Playback context unavailable: prepared_mismatch"
    monkeypatch.setattr(controller.transport.pitch, "status", lambda _pad: (phase, reason))
    monkeypatch.setattr(performance_view, "render_piano_keyboard", lambda *_args, **_kwargs: None)
    labels: list[str] = []
    monkeypatch.setattr(imgui, "text", labels.append)
    with _imgui_context():
        imgui.new_frame()
        imgui.begin("Piano diagnostics", flags=imgui.WindowFlags_.no_saved_settings)
        performance_view._render_piano(ctx, 120.0)
        imgui.end()
        imgui.render()
    assert len(labels) == 1
    assert reason not in labels[0]
    if phase == "preparing":
        assert phase in labels[0]
    else:
        assert phase not in labels[0]
        assert "Nominal note" in labels[0]
