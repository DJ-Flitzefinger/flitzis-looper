"""Bounded source-position inspection marks, independent of musical grid support."""

from typing import TYPE_CHECKING

from imgui_bundle import ImVec4, imgui, implot

from flitzis_looper.ui.constants import TEXT_MUTED_RGBA

if TYPE_CHECKING:
    from flitzis_looper_audio import SourceBeatmapEditorAnchor


def draw_source_map_anchors(
    draw_list: imgui.ImDrawList, anchors: list[SourceBeatmapEditorAnchor]
) -> None:
    """Draw physical Native anchor positions without beat labels or support claims."""
    base = TEXT_MUTED_RGBA
    color = imgui.get_color_u32(ImVec4(base.x, base.y, base.z, 0.65))
    for anchor in anchors:
        if anchor["sample_index"] is None:
            continue
        point = implot.plot_to_pixels(anchor["source_seconds"], 1.0)
        draw_list.add_line(
            (point.x, point.y + 2.0), (point.x, point.y + 10.0), color, thickness=2.0
        )
