from typing import TYPE_CHECKING, Literal
from unittest.mock import Mock

import pytest

from flitzis_looper.key_intent import MusicalKey, PadKeyIntent, SourceKeyVersion
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import sidebar_left
from flitzis_looper.ui.render.pitch_keyboard import (
    base_key_choices,
    extra_shift_choices,
    format_extra_shift,
    keyboard_geometry,
)

if TYPE_CHECKING:
    from collections.abc import Callable

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


def test_pitch_menu_visibility_and_scroll_requests_are_independent_and_context_local(
    controller: AppController,
) -> None:
    ctx = UiContext(controller)
    other = UiContext(controller)
    ctx.ui.toggle_pitch_menu("base")
    ctx.ui.toggle_pitch_menu("extra")
    assert ctx.state.pitch_menus.base_open
    assert ctx.state.pitch_menus.extra_open
    assert not other.state.pitch_menus.base_open
    assert not other.state.pitch_menus.extra_open
    assert ctx.ui.consume_pitch_scroll_request("extra", 0, 0)
    assert not ctx.ui.consume_pitch_scroll_request("extra", 0, 0)
    assert ctx.ui.consume_pitch_scroll_request("extra", 0, 2)
    assert ctx.ui.consume_pitch_scroll_request("base", 0, 0)
    assert ctx.ui.consume_pitch_scroll_request("extra", 4, 2)
    assert ctx.state.pitch_menus.base_open
    assert ctx.state.pitch_menus.extra_open
    ctx.ui.toggle_pitch_menu("base")
    assert not ctx.state.pitch_menus.base_open
    assert ctx.state.pitch_menus.extra_open


def test_mouse_choices_keep_both_menus_open_and_highlight_base_separately_from_result(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = UiContext(controller)
    intent = PadKeyIntent(
        source=SourceKeyVersion(raw_key="Em"), base_shift=-4, extra_shift=2, retrigger=True
    )
    controller.project.pad_key_intent[0] = intent
    ctx.ui.toggle_pitch_menu("base")
    ctx.ui.toggle_pitch_menu("extra")
    chosen_base = Mock()
    chosen_extra = Mock()
    monkeypatch.setattr(ctx.audio.pads, "choose_base_key", chosen_base)
    monkeypatch.setattr(ctx.audio.pads, "choose_extra_shift", chosen_extra)
    monkeypatch.setattr(ctx.input, "capture_timestamp_ns", lambda: 123)
    monkeypatch.setattr(sidebar_left, "_render_pitch_octaves", lambda *_args: None)
    surfaces: list[tuple[str, int | None]] = []

    def choose(
        _id: str,
        _keys: object,
        *,
        selected_value: int | None,
        reveal_selection: Callable[[], bool],
    ) -> int:
        assert reveal_selection()
        surfaces.append((_id, selected_value))
        return 7 if _id == "pitch_base_keyboard" else -12

    monkeypatch.setattr(sidebar_left, "render_pitch_keyboard", choose)
    info = sidebar_left._SidebarPadInfo(0, 220.0, is_loaded=True, is_loading=False)
    sidebar_left._render_base_keyboard(ctx, info, intent)
    sidebar_left._render_extra_keyboard(ctx, info, intent)
    assert surfaces == [("pitch_base_keyboard", 0), ("pitch_extra_keyboard", 2)]
    chosen_base.assert_called_once_with(
        0, MusicalKey(root=7, mode="minor"), retrigger=True, received_at_ns=123
    )
    chosen_extra.assert_called_once_with(0, -12, retrigger=True, received_at_ns=123)
    assert ctx.state.pitch_menus.base_open
    assert ctx.state.pitch_menus.extra_open
    assert format_extra_shift(-12) != format_extra_shift(0) != format_extra_shift(12)
