from contextlib import contextmanager
from typing import TYPE_CHECKING

from imgui_bundle import imgui, implot

from flitzis_looper.ui.constants import (
    BG_FRAME_ACTIVE_RGBA,
    BG_FRAME_HOVERED_RGBA,
    BG_FRAME_RGBA,
    CONTROL_BORDER_RGBA,
    CONTROL_PRESSED_RGBA,
    SPACING,
    TEXT_RGBA,
)
from flitzis_looper.ui.styles import BUTTON_STYLES, ButtonStyleName

if TYPE_CHECKING:
    from collections.abc import Iterable, Iterator

type ColorItem = tuple[int, imgui.ImVec4Like]
type StyleVarValue = float | imgui.ImVec2Like
type StyleItem = tuple[int, StyleVarValue]


@contextmanager
def default_style() -> Iterator[None]:
    """Apply some foundation styles."""
    button_style = BUTTON_STYLES["regular"]
    with (
        style_var(imgui.StyleVar_.item_spacing, (SPACING, SPACING)),
        style_colors((
            (imgui.Col_.button, button_style[imgui.Col_.button]),
            (imgui.Col_.button_active, button_style[imgui.Col_.button_active]),
            (imgui.Col_.button_hovered, button_style[imgui.Col_.button_hovered]),
            (imgui.Col_.text, TEXT_RGBA),
            (imgui.Col_.border, CONTROL_BORDER_RGBA),
            (imgui.Col_.frame_bg, BG_FRAME_RGBA),
            (imgui.Col_.frame_bg_active, BG_FRAME_ACTIVE_RGBA),
            (imgui.Col_.frame_bg_hovered, BG_FRAME_HOVERED_RGBA),
            (imgui.Col_.slider_grab, CONTROL_BORDER_RGBA),
            (imgui.Col_.slider_grab_active, CONTROL_PRESSED_RGBA),
        )),
    ):
        yield


@contextmanager
def button_style(name: ButtonStyleName) -> Iterator[None]:
    """Push a button style and pop on exit."""
    style = BUTTON_STYLES[name]
    with style_colors((
        (imgui.Col_.button, style[imgui.Col_.button]),
        (imgui.Col_.button_active, style[imgui.Col_.button_active]),
        (imgui.Col_.button_hovered, style[imgui.Col_.button_hovered]),
        (imgui.Col_.text, style[imgui.Col_.text]),
        (imgui.Col_.border, style[imgui.Col_.border]),
    )):
        yield


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


@contextmanager
def item_width(value: float) -> Iterator[None]:
    """Push item width and pop on exit."""
    imgui.push_item_width(value)
    try:
        yield
    finally:
        imgui.pop_item_width()


@contextmanager
def implot_style_color(idx: implot.Col, val: imgui.ImU32 | imgui.ImVec4Like) -> Iterator[None]:
    """Push implot style color and pop on exit.

    Usage:
        with implot_style_color(implot.Col_.fill, GREEN_RGBA):
            ...
    """
    implot.push_style_color(idx, val)
    try:
        yield
    finally:
        implot.pop_style_color()


@contextmanager
def implot_style_var(idx: implot.StyleVar, val: StyleVarValue) -> Iterator[None]:
    """Push implot style var and pop on exit.

    Usage:
        with implot_style_var(implot.StyleVar_.fill_alpha, 0.5):
            ...
    """
    implot.push_style_var(idx, val)
    try:
        yield
    finally:
        implot.pop_style_var()
