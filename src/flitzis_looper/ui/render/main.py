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
from flitzis_looper.ui.render.sidebar_left import sidebar_left
from flitzis_looper.ui.render.sidebar_right import sidebar_right

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

    from flitzis_looper.ui.context import UiContext


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

    with style_var(imgui.StyleVar_.item_spacing, (0, SPACING * 2)):
        imgui.begin_child(
            "performance_view",
            (-1, avail.y - BOTTOM_BAR_HEIGHT - SPACING),
        )
        performance_view(ctx)
        imgui.end_child()

        imgui.begin_child("bottom_bar", (-1, -1))
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


def render_ui(ctx: UiContext) -> None:
    """Main application render entrypoint."""
    ctx.audio.poll.poll_loader_events()
    ctx.audio.poll.poll_audio_messages()

    with default_style():
        _main(ctx)
        _file_dialog(ctx)

    ctx.persistence.maybe_flush()
