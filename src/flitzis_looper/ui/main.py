from contextlib import contextmanager
from typing import TYPE_CHECKING

from imgui_bundle import hello_imgui, imgui

from flitzis_looper.ui.bottom_bar import bottom_bar
from flitzis_looper.ui.constants import (
    BG_FRAME_ACTIVE_RGBA,
    BG_FRAME_HOVERED_RGBA,
    BG_FRAME_RGBA,
    BOTTOM_BAR_HEIGHT,
    CONTROL_BORDER_RGBA,
    CONTROL_PRESSED_RGBA,
    SIDEBAR_COLLAPSED_PX,
    SIDEBAR_LEFT_PX,
    SIDEBAR_RIGHT_PX,
    SPACING,
    TEXT_RGBA,
)
from flitzis_looper.ui.context import style_colors, style_var
from flitzis_looper.ui.file_dialog import check_file_dialog, open_file_dialog
from flitzis_looper.ui.performance_view import performance_view
from flitzis_looper.ui.sidebar_left import sidebar_left
from flitzis_looper.ui.sidebar_right import sidebar_right
from flitzis_looper.ui.styles import BUTTON_STYLES

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

    from flitzis_looper.app import FlitzisLooperApp


@contextmanager
def _sidebar(
    str_id: str,
    expand_label: str,
    collapse_label: str,
    toggle: Callable[[], None],
    size: imgui.ImVec2Like,
    *,
    expanded: bool,
) -> Iterator[bool]:
    imgui.begin_child(str_id, size)
    try:
        if expanded:
            if imgui.button(collapse_label, (-1, 20)):
                toggle()
            imgui.separator()
            yield True
        else:
            if imgui.button(
                f"{expand_label}##btn_expand_{str_id}", (SIDEBAR_COLLAPSED_PX, size[1])
            ):
                toggle()
            yield False
    finally:
        imgui.end_child()


def _center_area(app: FlitzisLooperApp) -> None:
    avail = imgui.get_content_region_avail()

    with style_var(imgui.StyleVar_.item_spacing, (0, SPACING * 2)):
        imgui.begin_child(
            "performance_view",
            (-1, avail.y - BOTTOM_BAR_HEIGHT - SPACING),
        )
        performance_view(app)
        imgui.end_child()

        imgui.begin_child("bottom_bar", (-1, -1))
        bottom_bar(app)
        imgui.end_child()


def _main_gui(app: FlitzisLooperApp) -> None:
    avail = imgui.get_content_region_avail()
    left_sidebar_width = (
        SIDEBAR_LEFT_PX if app.state.sidebar_left_expanded else SIDEBAR_COLLAPSED_PX
    )
    right_sidebar_width = (
        SIDEBAR_RIGHT_PX if app.state.sidebar_right_expanded else SIDEBAR_COLLAPSED_PX
    )

    # Left sidebar
    with _sidebar(
        "left_sidebar",
        ">",
        "« Close",
        app.toggle_left_sidebar,
        (left_sidebar_width, avail.y),
        expanded=app.state.sidebar_left_expanded,
    ) as expanded:
        if expanded:
            sidebar_left(app)

    # Center
    main_width = avail.x - left_sidebar_width - right_sidebar_width - 2 * SPACING
    imgui.same_line(spacing=SPACING)
    imgui.begin_child("center_area", (main_width, -1))
    _center_area(app)
    imgui.end_child()

    # Right sidebar
    imgui.same_line(spacing=SPACING)
    with _sidebar(
        "right_sidebar",
        "<",
        "Close »",
        app.toggle_right_sidebar,
        (right_sidebar_width, avail.y),
        expanded=app.state.sidebar_right_expanded,
    ) as expanded:
        if expanded:
            sidebar_right(app)


def main_gui(app: FlitzisLooperApp) -> None:
    io = imgui.get_io()
    if io.display_size.x < 800 or io.display_size.y < 600:
        hello_imgui.get_runner_params().app_window_params.window_geometry.size = (800, 600)

    # Apply some foundation styles
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
        _main_gui(app)

    if app.state.pending_file_dialog is not None:
        pad_id = app.state.pending_file_dialog
        open_file_dialog(pad_id)
        check_file_dialog(app, pad_id)
