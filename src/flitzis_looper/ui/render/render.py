from contextlib import contextmanager
from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.ui.constants import (
    BOTTOM_BAR_HEIGHT,
    SIDEBAR_COLLAPSED_PX,
    SIDEBAR_LEFT_PX,
    SIDEBAR_RIGHT_PX,
    SPACING,
)
from flitzis_looper.ui.contextmanager import default_style, style_var
from flitzis_looper.ui.render.bottom_bar import bottom_bar
from flitzis_looper.ui.render.file_dialog import check_file_dialog, open_file_dialog
from flitzis_looper.ui.render.performance_view import performance_view
from flitzis_looper.ui.render.settings import settings_overlay, settings_surface_child_id
from flitzis_looper.ui.render.sidebar_left import sidebar_left
from flitzis_looper.ui.render.sidebar_right import sidebar_right
from flitzis_looper.ui.render.waveform_editor import waveform_editor

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

    from flitzis_looper.ui.context import UiContext

KEYBOARD_INPUT_KEYS = (
    ("A", imgui.Key.a),
    ("B", imgui.Key.b),
    ("C", imgui.Key.c),
    ("D", imgui.Key.d),
    ("E", imgui.Key.e),
    ("F", imgui.Key.f),
    ("G", imgui.Key.g),
    ("H", imgui.Key.h),
    ("I", imgui.Key.i),
    ("J", imgui.Key.j),
    ("K", imgui.Key.k),
    ("L", imgui.Key.l),
    ("M", imgui.Key.m),
    ("N", imgui.Key.n),
    ("O", imgui.Key.o),
    ("P", imgui.Key.p),
    ("Q", imgui.Key.q),
    ("R", imgui.Key.r),
    ("S", imgui.Key.s),
    ("T", imgui.Key.t),
    ("U", imgui.Key.u),
    ("V", imgui.Key.v),
    ("W", imgui.Key.w),
    ("X", imgui.Key.x),
    ("Y", imgui.Key.y),
    ("Z", imgui.Key.z),
    ("0", imgui.Key._0),
    ("1", imgui.Key._1),
    ("2", imgui.Key._2),
    ("3", imgui.Key._3),
    ("4", imgui.Key._4),
    ("5", imgui.Key._5),
    ("6", imgui.Key._6),
    ("7", imgui.Key._7),
    ("8", imgui.Key._8),
    ("9", imgui.Key._9),
    ("SPACE", imgui.Key.space),
    ("ENTER", imgui.Key.enter),
    ("ESCAPE", imgui.Key.escape),
    ("LEFT", imgui.Key.left_arrow),
    ("RIGHT", imgui.Key.right_arrow),
    ("UP", imgui.Key.up_arrow),
    ("DOWN", imgui.Key.down_arrow),
    ("F1", imgui.Key.f1),
    ("F2", imgui.Key.f2),
    ("F3", imgui.Key.f3),
    ("F4", imgui.Key.f4),
    ("F5", imgui.Key.f5),
    ("F6", imgui.Key.f6),
    ("F7", imgui.Key.f7),
    ("F8", imgui.Key.f8),
    ("F9", imgui.Key.f9),
    ("F10", imgui.Key.f10),
    ("F11", imgui.Key.f11),
    ("F12", imgui.Key.f12),
)


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


def _center_area(ctx: UiContext) -> None:
    avail = imgui.get_content_region_avail()
    surface_height = max(0.0, avail.y - BOTTOM_BAR_HEIGHT - SPACING)

    with style_var(imgui.StyleVar_.item_spacing, (0.0, 0.0)):
        imgui.begin_child(
            settings_surface_child_id(settings_open=ctx.state.session.settings_open),
            (-1, surface_height),
        )
        if ctx.state.session.settings_open:
            settings_overlay(ctx)
        else:
            performance_view(ctx)
        imgui.end_child()

        imgui.dummy((0.0, SPACING))

        imgui.begin_child("bottom_bar", (-1, BOTTOM_BAR_HEIGHT))
        bottom_bar(ctx)
        imgui.end_child()


def _main(ctx: UiContext) -> None:
    avail = imgui.get_content_region_avail()
    left_sidebar_width = (
        SIDEBAR_LEFT_PX if ctx.state.project.sidebar_left_expanded else SIDEBAR_COLLAPSED_PX
    )
    right_sidebar_width = (
        SIDEBAR_RIGHT_PX if ctx.state.project.sidebar_right_expanded else SIDEBAR_COLLAPSED_PX
    )

    # Left sidebar
    with _sidebar(
        "left_sidebar",
        ">",
        "« Close",
        ctx.ui.toggle_left_sidebar,
        (left_sidebar_width, avail.y),
        expanded=ctx.state.project.sidebar_left_expanded,
    ) as expanded:
        if expanded:
            sidebar_left(ctx)

    # Center
    main_width = avail.x - left_sidebar_width - right_sidebar_width - 2 * SPACING
    imgui.same_line(spacing=SPACING)
    imgui.begin_child("center_area", (main_width, -1))
    _center_area(ctx)
    imgui.end_child()

    # Right sidebar
    imgui.same_line(spacing=SPACING)
    with _sidebar(
        "right_sidebar",
        "<",
        "Close »",
        ctx.ui.toggle_right_sidebar,
        (right_sidebar_width, avail.y),
        expanded=ctx.state.project.sidebar_right_expanded,
    ) as expanded:
        if expanded:
            sidebar_right(ctx)


def _file_dialog(ctx: UiContext) -> None:
    """Show file dialog if requested."""
    if ctx.state.session.file_dialog_pad_id is not None:
        pad_id = ctx.state.session.file_dialog_pad_id
        open_file_dialog(pad_id)
        check_file_dialog(ctx, pad_id)


def _poll_keyboard_input(ctx: UiContext) -> None:
    if not ctx.state.project.input_mapping_enabled:
        return

    io = imgui.get_io()
    text_input_focused = bool(getattr(io, "want_text_input", False)) or imgui.is_any_item_active()
    ctrl = imgui.is_key_down(imgui.Key.mod_ctrl)
    alt = imgui.is_key_down(imgui.Key.mod_alt)
    shift = imgui.is_key_down(imgui.Key.mod_shift)
    super_ = imgui.is_key_down(imgui.Key.mod_super)

    for key_name, key in KEYBOARD_INPUT_KEYS:
        if imgui.is_key_pressed(key, repeat=False) and ctx.input.capture_keyboard(
            key_name,
            ctrl=ctrl,
            alt=alt,
            shift=shift,
            super_=super_,
            text_input_focused=text_input_focused,
        ):
            break


def render_ui(ctx: UiContext) -> None:
    """Main application render entrypoint."""
    ctx.on_frame_render()
    _poll_keyboard_input(ctx)
    ctx.audio.poll.poll()

    with default_style():
        _main(ctx)
        if not ctx.state.session.settings_open:
            waveform_editor(ctx)
            _file_dialog(ctx)

    ctx.persistence.maybe_flush()
