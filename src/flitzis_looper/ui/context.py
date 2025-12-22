from contextlib import contextmanager
from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.ui.styles import BUTTON_STYLES, ButtonStyleName

if TYPE_CHECKING:
    from collections.abc import Iterable, Iterator


@contextmanager
def button_style(name: ButtonStyleName) -> Iterator[None]:
    style = BUTTON_STYLES[name]
    with style_colors((
        (imgui.Col_.button, style[imgui.Col_.button]),
        (imgui.Col_.button_active, style[imgui.Col_.button_active]),
        (imgui.Col_.button_hovered, style[imgui.Col_.button_hovered]),
        (imgui.Col_.text, style[imgui.Col_.text]),
        (imgui.Col_.border, style[imgui.Col_.border]),
    )):
        yield


type ColorItem = tuple[int, imgui.ImVec4Like]


@contextmanager
def style_color(idx: int, col: imgui.ImVec4Like) -> Iterator[None]:
    """Push style color and pop on exit.

    Usage:
        with style_var(imgui.ColorVar_.text, TEXT_COLOR_RGBA):
            ...
    """
    with style_colors(((idx, col),)):
        yield


@contextmanager
def style_colors(items: Iterable[ColorItem]) -> Iterator[None]:
    """Push color vars and pop them on exit.

    Usage:
        with color_vars([
            (imgui.ColorVar_.text, TEXT_COLOR_RGBA),
            (imgui.ColorVar_.border, BORDER_COLOR_RGBA)
        ]):
            ...
    """
    count = 0
    for idx, col in items:
        imgui.push_style_color(idx, col)
        count += 1
    try:
        yield
    finally:
        imgui.pop_style_color(count)


type StyleVarValue = float | imgui.ImVec2Like
type StyleItem = tuple[int, StyleVarValue]


@contextmanager
def style_var(idx: int, val: StyleVarValue) -> Iterator[None]:
    """Push style var and pop on exit.

    Usage:
        with style_var(imgui.StyleVar_.item_spacing, (0.0, 0.0)):
            ...
    """
    with style_vars(((idx, val),)):
        yield


@contextmanager
def style_vars(items: Iterable[StyleItem]) -> Iterator[None]:
    """Push style vars and pop them on exit.

    Usage:
        with style_vars([
            (imgui.StyleVar_.item_spacing, (0.0, 0.0)),
            (imgui.StyleVar_.frame_padding, (5, 5))
        ]):
            ...
    """
    count = 0
    for style, value in items:
        imgui.push_style_var(style, value)
        count += 1
    try:
        yield
    finally:
        imgui.pop_style_var(count)
