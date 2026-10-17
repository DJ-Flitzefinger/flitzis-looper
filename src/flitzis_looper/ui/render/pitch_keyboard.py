"""Piano geometry and pitch surfaces with content-derived selection."""

import math
from dataclasses import dataclass
from typing import Literal

from imgui_bundle import imgui

from flitzis_looper.key_intent import MusicalKey

_ROOT_NAMES = ("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")
_BLACK_ROOTS = frozenset({1, 3, 6, 8, 10})
KEY_WIDTH = 42.0
_CHOSEN_RGBA = (0.24, 0.65, 1.0, 1.0)
_ORIGINAL_RGBA = (0.92, 0.57, 0.19, 1.0)
_PIANO_WHITE_COUNT = 21
_PIANO_BLACK_WIDTH = 0.64
_PIANO_BLACK_HEIGHT = 0.62


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


@dataclass(frozen=True)
class PianoKeyGeometry:
    """One chromatic target, with black keys overlaying neighboring white keys."""

    key: PitchKeyboardKey
    semitone_index: int
    x: float
    width: float
    height: float
    is_original: bool

    @property
    def center_x(self) -> float:
        """Return the key's horizontal center in piano-local coordinates."""
        return self.x + self.width / 2.0

    def contains(self, x: float, y: float) -> bool:
        """Use half-open bounds so shared white-key edges have one owner."""
        return self.x <= x < self.x + self.width and 0.0 <= y < self.height


def piano_geometry(
    source_root: int | None,
    *,
    width: float,
    height: float,
    original_root: int | Literal["source"] | None = "source",
) -> tuple[PianoKeyGeometry, ...]:
    """Fit three C-based chromatic octaves with the source in the middle octave.

    Values are signed semitones from the original source. An unknown source uses
    the same physical layout with zero at its middle C position, but labels only
    relative intervals and does not claim any absolute note or original key.
    """
    if source_root is not None and (type(source_root) is not int or not 0 <= source_root < 12):
        message = "Source root must be a chromatic index from 0 through 11."
        raise ValueError(message)
    detected = source_root if original_root == "source" else original_root
    if detected is not None and (type(detected) is not int or not 0 <= detected < 12):
        message = "Original root must be a chromatic index from 0 through 11."
        raise ValueError(message)
    if not math.isfinite(width) or not math.isfinite(height) or min(width, height) <= 0.0:
        message = "Piano dimensions must be finite and positive."
        raise ValueError(message)
    white_width = width / _PIANO_WHITE_COUNT
    original_index = 12 + (source_root if source_root is not None else 0)
    white_count = 0
    geometry: list[PianoKeyGeometry] = []
    for index in range(36):
        black = index % 12 in _BLACK_ROOTS
        x = white_count * width / _PIANO_WHITE_COUNT
        if black:
            key_width = white_width * _PIANO_BLACK_WIDTH
            x -= key_width / 2.0
        else:
            key_width = (white_count + 1) * width / _PIANO_WHITE_COUNT - x
        shift = index - original_index
        label = _piano_key_label(index, shift, source_root)
        geometry.append(
            PianoKeyGeometry(
                key=PitchKeyboardKey(shift, label, black),
                semitone_index=index,
                x=x,
                width=key_width,
                height=height * _PIANO_BLACK_HEIGHT if black else height,
                is_original=source_root is not None
                and detected is not None
                and index == 12 + detected,
            )
        )
        white_count += not black
    return tuple(geometry)


def _piano_key_label(index: int, shift: int, source_root: int | None) -> str:
    if source_root is None:
        return format_extra_shift(shift)
    octave = index // 12 - 1
    octave_label = "original octave" if octave == 0 else f"{octave:+d} octave"
    return f"{_ROOT_NAMES[index % 12]}\n{octave_label}"


def piano_hit_test(
    geometry: tuple[PianoKeyGeometry, ...], x: float, y: float
) -> PianoKeyGeometry | None:
    """Resolve a piano-local point, giving raised black keys first priority."""
    for black in (True, False):
        for item in geometry:
            if item.key.black is black and item.contains(x, y):
                return item
    return None


def render_piano_keyboard(
    menu_id: str,
    *,
    source_root: int | None,
    selected_shift: int,
    height: float = 136.0,
    original_root: int | Literal["source"] | None = "source",
) -> int | None:
    """Draw a width-fitted piano and emit one signed target on each mouse press.

    A single item owns interaction. Black/white overlap is resolved geometrically,
    and pressing the selected note again emits the same action for retriggering.
    """
    width = max(1.0, imgui.get_content_region_avail().x)
    geometry = piano_geometry(source_root, width=width, height=height, original_root=original_root)
    imgui.invisible_button(menu_id, (width, height))
    origin = imgui.get_item_rect_min()
    hovered = None
    if imgui.is_item_hovered():
        mouse = imgui.get_mouse_pos()
        hovered = piano_hit_test(geometry, mouse.x - origin.x, mouse.y - origin.y)
    for black in (False, True):
        for item in geometry:
            if item.key.black is black:
                _draw_piano_key(
                    item,
                    origin=(origin.x, origin.y),
                    selected=item.key.value == selected_shift,
                    hovered=item is hovered,
                    known_source=source_root is not None,
                )
    _draw_piano_range_feedback(
        source_root=source_root,
        selected_shift=selected_shift,
        origin=(origin.x, origin.y),
        width=width,
        height=height,
    )
    if hovered is not None:
        description = hovered.key.label.replace("\n", " Â· ")
        if source_root is not None:
            description += f"\n{format_extra_shift(hovered.key.value)}"
        else:
            description += "\nSource key unknown: relative note"
        if hovered.is_original:
            description += "\nOriginal Key"
        imgui.set_tooltip(description)
        if imgui.is_mouse_clicked(imgui.MouseButton_.left):
            return hovered.key.value
    return None


def _draw_piano_range_feedback(
    *,
    source_root: int | None,
    selected_shift: int,
    origin: tuple[float, float],
    width: float,
    height: float,
) -> None:
    """Keep an undisplayed exact selection visible without changing piano bounds."""
    selected_index = 12 + (source_root if source_root is not None else 0) + selected_shift
    if 0 <= selected_index < 36:
        return
    direction = "below" if selected_index < 0 else "above"
    note = _ROOT_NAMES[selected_index % 12] if source_root is not None else "relative note"
    description = (
        f"Selected: {note} | {selected_shift:+d} st | "
        f"{selected_index // 12 - 1:+d} octave | {direction} displayed range"
    )
    size = imgui.calc_text_size(description)
    scale = min(1.0, max(0.0, width - 16.0) / size.x, max(0.0, height - 8.0) / size.y)
    position = (origin[0] + (width - size.x * scale) / 2.0, origin[1] + 4.0)
    draw = imgui.get_window_draw_list()
    draw.add_rect_filled(
        (position[0] - 4.0, position[1] - 2.0),
        (position[0] + size.x * scale + 4.0, position[1] + size.y * scale + 2.0),
        imgui.get_color_u32((0.09, 0.10, 0.12, 0.96)),
        rounding=2.0,
    )
    draw.add_text(
        imgui.get_font(),
        imgui.get_font_size() * scale,
        position,
        imgui.get_color_u32((0.88, 0.92, 0.97, 1.0)),
        description,
    )


def _draw_piano_key(
    item: PianoKeyGeometry,
    *,
    origin: tuple[float, float],
    selected: bool,
    hovered: bool,
    known_source: bool,
) -> None:
    draw = imgui.get_window_draw_list()
    left, top = origin[0] + item.x, origin[1]
    right, bottom = left + item.width, top + item.height
    background = (0.09, 0.10, 0.12, 1.0) if item.key.black else (0.88, 0.89, 0.91, 1.0)
    if hovered:
        background = (0.18, 0.23, 0.29, 1.0) if item.key.black else (0.77, 0.86, 0.94, 1.0)
    border = _CHOSEN_RGBA if selected else (0.20, 0.22, 0.26, 1.0)
    inset = min(0.5, item.width / 8.0)
    minimum, maximum = (left + inset, top + inset), (right - inset, bottom - inset)
    draw.add_rect_filled(minimum, maximum, imgui.get_color_u32(background), rounding=2.0)
    draw.add_rect(
        minimum,
        maximum,
        imgui.get_color_u32(border),
        rounding=2.0,
        thickness=2.5 if selected else 1.0,
    )
    if selected:
        draw.add_rect_filled(
            (left + inset, top + 2.0),
            (right - inset, top + 6.0),
            imgui.get_color_u32(_CHOSEN_RGBA),
        )
    if item.is_original:
        draw.add_circle_filled(
            (item.center_x + origin[0], bottom - 9.0),
            min(3.0, item.width / 5.0),
            imgui.get_color_u32(_ORIGINAL_RGBA),
        )
    _draw_piano_key_label(item, origin=origin, known_source=known_source)


def _draw_piano_key_label(
    item: PianoKeyGeometry, *, origin: tuple[float, float], known_source: bool
) -> None:
    if item.width < 8.0 or item.height < 31.0:
        return
    if known_source:
        octave = item.semitone_index // 12 - 1
        lines = (_ROOT_NAMES[item.semitone_index % 12], "0" if octave == 0 else f"{octave:+d}")
    else:
        lines = ("0" if item.key.value == 0 else f"{item.key.value:+d}", "st")
    sizes = [imgui.calc_text_size(line) for line in lines]
    scale = min(
        1.0,
        (item.width - 4.0) / max(size.x for size in sizes),
        (item.height - 23.0) / (2.0 * imgui.get_font_size()),
    )
    font_size = imgui.get_font_size() * scale
    line_height = font_size + 2.0
    color = (0.92, 0.93, 0.96, 1.0) if item.key.black else (0.14, 0.16, 0.20, 1.0)
    draw = imgui.get_window_draw_list()
    for index, (line, size) in enumerate(zip(lines, sizes, strict=True)):
        position = (
            origin[0] + item.center_x - size.x * scale / 2.0,
            origin[1] + item.height - 17.0 - (2 - index) * line_height,
        )
        draw.add_text(imgui.get_font(), font_size, position, imgui.get_color_u32(color), line)


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
