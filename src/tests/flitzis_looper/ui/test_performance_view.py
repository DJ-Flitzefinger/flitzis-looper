from contextlib import contextmanager
from typing import TYPE_CHECKING, cast

from flitzis_looper.ui.render import performance_view
from flitzis_looper.ui.render.performance_view import stem_grid_indicator_label, wrap_pad_title

if TYPE_CHECKING:
    from collections.abc import Iterator

    import pytest

    from flitzis_looper.ui.context import UiContext


class _Point:
    __slots__ = ("x", "y")

    def __init__(self, x: float, y: float) -> None:
        self.x = x
        self.y = y


class _DrawList:
    __slots__ = ("texts",)

    def __init__(self) -> None:
        self.texts: list[str] = []

    def add_rect_filled(
        self,
        _pos_min: tuple[float, float],
        _pos_max: tuple[float, float],
        _color: int,
    ) -> None:
        return

    def add_text(self, _pos: tuple[float, float], _color: int, text: str) -> None:
        self.texts.append(text)


class _Pads:
    __slots__ = ()

    def is_loaded(self, _pad_id: int) -> bool:
        return True

    def is_loading(self, _pad_id: int) -> bool:
        return False

    def is_active(self, _pad_id: int) -> bool:
        return False

    def is_selected(self, _pad_id: int) -> bool:
        return False

    def is_analyzing(self, _pad_id: int) -> bool:
        return False

    def label(self, _pad_id: int) -> str:
        return "Track.wav"

    def effective_bpm(self, _pad_id: int) -> float | None:
        return 94.0

    def effective_key(self, _pad_id: int) -> str | None:
        return "D#"

    def peak(self, _pad_id: int) -> float:
        raise AssertionError


class _Stems:
    __slots__ = ()

    def stem_grid_indicator_state(self, _pad_id: int) -> None:
        return None


class _State:
    __slots__ = ("pads", "stems")

    def __init__(self) -> None:
        self.pads = _Pads()
        self.stems = _Stems()


class _IndicatorStems:
    __slots__ = ("_state",)

    def __init__(self, state: str) -> None:
        self._state = state

    def stem_grid_indicator_state(self, _pad_id: int) -> str:
        return self._state


class _StemIndicatorState:
    __slots__ = ("stems",)

    def __init__(self, state: str) -> None:
        self.stems = _IndicatorStems(state)


class _StemIndicatorContext:
    __slots__ = ("state",)

    def __init__(self, state: str) -> None:
        self.state = _StemIndicatorState(state)


class _Context:
    __slots__ = ("state",)

    def __init__(self) -> None:
        self.state = _State()


@contextmanager
def _button_style(_style_name: object) -> Iterator[None]:
    yield


def test_stem_grid_indicator_labels_are_compact() -> None:
    assert stem_grid_indicator_label(None) is None
    assert stem_grid_indicator_label("available") == "ST"
    assert stem_grid_indicator_label("generating") == "..."
    assert stem_grid_indicator_label("blocked") is None
    assert stem_grid_indicator_label("error") == "!"


def test_pad_grid_stem_indicator_does_not_set_hover_tooltip(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    draw_list = _DrawList()

    def item_rect_min() -> _Point:
        return _Point(10.0, 20.0)

    def item_rect_max() -> _Point:
        return _Point(210.0, 120.0)

    def window_draw_list() -> _DrawList:
        return draw_list

    def color_u32(_rgba: object) -> int:
        return 1

    def calc_text_size(text: str) -> _Point:
        return _Point(float(len(text) * 8), 14.0)

    def set_tooltip(_tooltip: str) -> None:
        raise AssertionError

    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.get_item_rect_min",
        item_rect_min,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.get_item_rect_max",
        item_rect_max,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.get_window_draw_list",
        window_draw_list,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.get_color_u32",
        color_u32,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.calc_text_size",
        calc_text_size,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.is_item_hovered",
        lambda: True,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.set_tooltip",
        set_tooltip,
    )

    performance_view._pad_button_stem_indicator(
        cast("UiContext", _StemIndicatorContext("available")),
        0,
    )

    assert draw_list.texts == ["ST"]


def test_pad_grid_blocked_stem_indicator_is_hidden(monkeypatch: pytest.MonkeyPatch) -> None:
    def item_rect_min() -> _Point:
        msg = "blocked indicator should not render"
        raise AssertionError(msg)

    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.get_item_rect_min",
        item_rect_min,
    )

    performance_view._pad_button_stem_indicator(
        cast("UiContext", _StemIndicatorContext("blocked")),
        0,
    )


def test_pad_title_wraps_to_three_padded_lines(monkeypatch: pytest.MonkeyPatch) -> None:
    def calc_text_size(text: str) -> _Point:
        return _Point(float(len(text) * 8), 14.0)

    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.calc_text_size",
        calc_text_size,
    )

    title = wrap_pad_title("VeryLongSampleNameWithNoSpaces.wav", pad_width=80.0)
    lines = title.splitlines()

    assert len(lines) == 3
    assert lines[-1].endswith("...")
    assert all(len(line) * 8 <= 64 for line in lines)


def test_loaded_pad_renders_bpm_key_metadata_but_not_vertical_peak_meter(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    draw_list = _DrawList()
    button_labels: list[str] = []

    def item_rect_min() -> _Point:
        return _Point(10.0, 20.0)

    def item_rect_max() -> _Point:
        return _Point(210.0, 120.0)

    def window_draw_list() -> _DrawList:
        return draw_list

    def color_u32(_rgba: object) -> int:
        return 1

    def calc_text_size(text: str) -> _Point:
        return _Point(float(len(text) * 8), 14.0)

    def button(label: str, _size: tuple[float, float]) -> bool:
        button_labels.append(label)
        return False

    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.get_item_rect_min",
        item_rect_min,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.get_item_rect_max",
        item_rect_max,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.get_window_draw_list",
        window_draw_list,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.get_color_u32",
        color_u32,
    )
    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.calc_text_size",
        calc_text_size,
    )
    monkeypatch.setattr("flitzis_looper.ui.render.performance_view.imgui.button", button)
    monkeypatch.setattr(
        "flitzis_looper.ui.render.performance_view.imgui.is_item_hovered",
        lambda: False,
    )
    monkeypatch.setattr("flitzis_looper.ui.render.performance_view.button_style", _button_style)

    performance_view._pad_button(cast("UiContext", _Context()), 0, (200.0, 100.0))

    assert button_labels == ["Track.wav##pad_btn_0"]
    assert draw_list.texts == ["#1", "94.0 D#"]
