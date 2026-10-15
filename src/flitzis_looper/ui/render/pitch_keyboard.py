"""Small scrollable pitch surfaces with persistent, content-derived selection."""

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from imgui_bundle import imgui

from flitzis_looper.key_intent import MusicalKey
from flitzis_looper.ui.contextmanager import style_var

if TYPE_CHECKING:
    from collections.abc import Callable

_ROOT_NAMES = ("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")
_BLACK_ROOTS = frozenset({1, 3, 6, 8, 10})
KEY_WIDTH = 42.0
KEY_HEIGHT = 88.0
KEYBOARD_HEIGHT = 112.0
_CHOSEN_RGBA = (0.24, 0.65, 1.0, 1.0)


@dataclass(frozen=True)
class PitchKeyboardKey:
    """One semantic target; octave-separated values never share a widget identity."""

    value: int
    label: str
    black: bool | None


@dataclass(frozen=True)
class PitchKeyGeometry:
    """Equal semitone cells keep zero centered, including non-C root references."""

    key: PitchKeyboardKey
    x: float
    width: float

    @property
    def center_x(self) -> float:
        """Return this target's horizontal center for scroll positioning."""
        return self.x + self.width / 2.0


def base_key_choices(mode: Literal["major", "minor"]) -> tuple[PitchKeyboardKey, ...]:
    """Offer each of the twelve roots in the source's mode without mode conversion."""
    return tuple(
        PitchKeyboardKey(
            root,
            MusicalKey(root=root, mode=mode).label,
            root in _BLACK_ROOTS,
        )
        for root in range(12)
    )


def extra_shift_choices(base: MusicalKey | None) -> tuple[PitchKeyboardKey, ...]:
    """Offer all 37 signed targets using the base key root as the note reference."""
    return tuple(_extra_key(value, base) for value in range(-18, 19))


def _extra_key(value: int, base: MusicalKey | None) -> PitchKeyboardKey:
    signed = "0" if value == 0 else f"{value:+d}"
    if base is None:
        return PitchKeyboardKey(value, f"{signed} st", None)
    root = (base.root + value) % 12
    octave = ""
    if value in {-12, 0, 12}:
        octave = "\nbase" if value == 0 else f"\n{value // 12:+d} oct"
    return PitchKeyboardKey(
        value, f"{signed} st\n{_ROOT_NAMES[root]}{octave}", root in _BLACK_ROOTS
    )


def keyboard_geometry(
    keys: tuple[PitchKeyboardKey, ...], *, key_width: float = KEY_WIDTH
) -> tuple[PitchKeyGeometry, ...]:
    """Arrange each signed semitone in its own cell without octave overlap."""
    return tuple(
        PitchKeyGeometry(key, index * key_width, key_width) for index, key in enumerate(keys)
    )


def format_extra_shift(value: int) -> str:
    """Show the signed numerical intent, including explicit whole-octave distance."""
    signed = "0" if value == 0 else f"{value:+d}"
    if value in {-12, 12}:
        return f"{signed} st ({value // 12:+d} octave)"
    return f"{signed} st"


def render_pitch_keyboard(
    menu_id: str,
    keys: tuple[PitchKeyboardKey, ...],
    *,
    selected_value: int | None,
    reveal_selection: Callable[[], bool],
) -> int | None:
    """Draw a scrollable keyboard and return one mouse attack without closing the menu."""
    chosen: int | None = None
    text_sizes = [imgui.calc_text_size(key.label) for key in keys]
    width = max(KEY_WIDTH, max((size.x for size in text_sizes), default=0.0) + 10.0)
    height = max(KEY_HEIGHT, max((size.y for size in text_sizes), default=0.0) + 40.0)
    style = imgui.get_style()
    child_height = max(
        KEYBOARD_HEIGHT, height + style.scrollbar_size + style.window_padding.y * 2.0
    )
    visible = imgui.begin_child(
        menu_id,
        (-1, child_height),
        window_flags=imgui.WindowFlags_.horizontal_scrollbar,
    )
    try:
        if visible:
            with style_var(imgui.StyleVar_.item_spacing, (0.0, 0.0)):
                chosen = _render_keys(
                    keyboard_geometry(keys, key_width=width),
                    selected_value=selected_value,
                    reveal_selection=reveal_selection(),
                    height=height,
                )
    finally:
        imgui.end_child()
    return chosen


def _render_keys(
    geometry: tuple[PitchKeyGeometry, ...],
    *,
    selected_value: int | None,
    reveal_selection: bool,
    height: float,
) -> int | None:
    chosen: int | None = None
    for index, item in enumerate(geometry):
        if index:
            imgui.same_line()
        key = item.key
        imgui.invisible_button(f"##pitch_{key.value}", (item.width, height))
        hovered = imgui.is_item_hovered()
        _draw_key(key, selected=key.value == selected_value, hovered=hovered)
        if hovered and imgui.is_mouse_clicked(imgui.MouseButton_.left):
            chosen = key.value
        if reveal_selection and key.value == selected_value:
            imgui.set_scroll_here_x(0.5)
    return chosen


def _draw_key(key: PitchKeyboardKey, *, selected: bool, hovered: bool) -> None:
    position = imgui.get_item_rect_min()
    maximum = imgui.get_item_rect_max()
    draw = imgui.get_window_draw_list()
    dark = key.black is not False
    background = (0.12, 0.12, 0.14, 1.0) if dark else (0.86, 0.86, 0.88, 1.0)
    text = (0.95, 0.95, 0.97, 1.0) if dark else (0.09, 0.09, 0.11, 1.0)
    border = _CHOSEN_RGBA if selected else (0.43, 0.43, 0.46, 1.0)
    bottom = maximum.y - (12.0 if key.black else 0.0)
    left_top = (position.x + 1.0, position.y + 1.0)
    right_bottom = (maximum.x - 1.0, bottom - 1.0)
    draw.add_rect_filled(left_top, right_bottom, imgui.get_color_u32(background))
    draw.add_rect(
        left_top, right_bottom, imgui.get_color_u32(border), thickness=3.0 if selected else 1.0
    )
    if selected or hovered:
        draw.add_rect_filled(
            (position.x + 3.0, position.y + 3.0),
            (maximum.x - 3.0, position.y + 7.0),
            imgui.get_color_u32(border if selected else (0.65, 0.65, 0.69, 1.0)),
        )
    size = imgui.calc_text_size(key.label)
    draw.add_text(
        (position.x + (maximum.x - position.x - size.x) / 2.0, position.y + 14.0),
        imgui.get_color_u32(text),
        key.label,
    )
