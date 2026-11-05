"""Inactive PAD presentation using only saved project state, never Native selectors."""

from pathlib import PurePath
from typing import TYPE_CHECKING

from imgui_bundle import imgui

from flitzis_looper.constants import GRID_SIZE, NUM_PADS
from flitzis_looper.ui.constants import PAD_GRID_GAP, SPACING
from flitzis_looper.ui.contextmanager import style_var
from flitzis_looper.ui.render.performance_view import wrap_pad_title

if TYPE_CHECKING:
    from flitzis_looper.ui.context import UiContext


def startup_performance(ctx: UiContext) -> None:
    """Keep PADs visible while the original startup/restore owner admits no input."""
    output = ctx.ui.settings.audio_output
    imgui.text_unformatted(f"Audio output: {output.phase.replace('_', ' ')}")
    if output.error:
        imgui.text_wrapped(output.error)
    imgui.text_wrapped("PADs become available when audio and saved sources are ready.")
    if imgui.button("Open settings"):
        ctx.ui.settings.open()
    imgui.dummy((0, SPACING))
    avail = imgui.get_content_region_avail()
    spacing = (GRID_SIZE - 1) * PAD_GRID_GAP
    width = max(1.0, (avail.x - spacing) / GRID_SIZE)
    height = max(1.0, (avail.y - spacing) / GRID_SIZE)
    first = ctx.state.project.selected_bank * NUM_PADS
    imgui.begin_disabled()
    try:
        with style_var(imgui.StyleVar_.item_spacing, (0, 0)):
            for row in range(GRID_SIZE):
                for col in range(GRID_SIZE):
                    if col:
                        imgui.same_line(spacing=PAD_GRID_GAP)
                    pad_id = first + row * GRID_SIZE + col
                    path = ctx.state.project.sample_paths[pad_id]
                    label = PurePath(path).stem if path else "Empty"
                    label = wrap_pad_title(label, pad_width=width)
                    imgui.button(f"#{pad_id + 1} {label}##startup_pad_{pad_id}", (width, height))
                if row < GRID_SIZE - 1:
                    imgui.dummy((0, PAD_GRID_GAP))
    finally:
        imgui.end_disabled()
