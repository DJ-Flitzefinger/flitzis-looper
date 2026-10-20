"""Native ImGui geometry and pointer input without an App or AudioEngine."""

from contextlib import contextmanager
from dataclasses import dataclass, field
from types import SimpleNamespace
from typing import TYPE_CHECKING, Literal, NotRequired, TypedDict, Unpack, cast

import pytest
from imgui_bundle import imgui

from flitzis_looper.controller.transport.global_params import GlobalRateStatus
from flitzis_looper.controller.transport.key_lock_status import KeyLockStatus
from flitzis_looper.ui.contextmanager import default_style
from flitzis_looper.ui.render import render

if TYPE_CHECKING:
    from collections.abc import Iterator

    from flitzis_looper.ui.context import UiContext

type _Rect = tuple[float, float, float, float]


class _SliderOptions(TypedDict):
    format: NotRequired[str]
    flags: NotRequired[int]


@contextmanager
def _native_context(scale: float, height: float) -> Iterator[None]:
    previous = imgui.get_current_context()
    owned = imgui.create_context()
    imgui.set_current_context(owned)
    io = imgui.get_io()
    io.set_ini_filename(None)
    io.display_size = imgui.ImVec2(1010, height)
    io.delta_time = 1 / 60
    io.backend_flags |= imgui.BackendFlags_.renderer_has_textures
    imgui.get_style().font_scale_main = scale
    io.add_mouse_pos_event(-100, -100)
    try:
        yield
    finally:
        imgui.destroy_context(owned)
        if previous is not None:
            imgui.set_current_context(previous)


def _item_rect() -> _Rect:
    minimum, maximum = imgui.get_item_rect_min(), imgui.get_item_rect_max()
    return minimum.x, minimum.y, maximum.x, maximum.y


@dataclass
class _PitchSurface:
    height: float
    reference_bpm: float | None
    speed: float = 1.0
    rate: GlobalRateStatus = field(default_factory=lambda: GlobalRateStatus(1.0))
    key_lock: KeyLockStatus = field(
        default_factory=lambda: KeyLockStatus(requested=False, effective=False)
    )
    items: dict[str, _Rect] = field(default_factory=dict)
    speeds: list[float] = field(default_factory=list)
    project: SimpleNamespace = field(init=False)
    ctx: UiContext = field(init=False)

    def __post_init__(self) -> None:
        self.project = SimpleNamespace(
            speed=self.speed,
            bpm_lock=False,
            input_mapping_enabled=False,
            sidebar_left_expanded=True,
            sidebar_right_expanded=True,
        )
        self.ctx = cast(
            "UiContext",
            SimpleNamespace(
                state=SimpleNamespace(
                    project=self.project,
                    session=SimpleNamespace(global_bpm_edit_active=False),
                    global_=SimpleNamespace(
                        effective_bpm=lambda: (
                            None
                            if self.reference_bpm is None
                            else self.reference_bpm * self.project.speed
                        ),
                        speed_reference_bpm=lambda: self.reference_bpm,
                        global_rate_status=lambda: self.rate,
                        key_lock_status=lambda: self.key_lock,
                    ),
                ),
                audio=SimpleNamespace(
                    global_=SimpleNamespace(
                        set_speed=self._set_speed,
                        toggle_key_lock=lambda: None,
                    )
                ),
                ui=SimpleNamespace(
                    toggle_left_sidebar=lambda: None,
                    toggle_right_sidebar=lambda: None,
                ),
            ),
        )

    def _set_speed(self, value: float) -> None:
        self.project.speed = value
        self.speeds.append(value)

    def observe(self, monkeypatch: pytest.MonkeyPatch) -> None:
        native_slider = imgui.v_slider_float
        native_button = imgui.button

        def slider(
            label: str,
            size: imgui.ImVec2Like,
            v: float,
            v_min: float,
            v_max: float,
            **options: Unpack[_SliderOptions],
        ) -> tuple[bool, float]:
            result = native_slider(label, size, v, v_min, v_max, **options)
            self.items["Pitch"] = _item_rect()
            return result

        def button(label: str, size: imgui.ImVec2Like = (0, 0)) -> bool:
            result = native_button(label, size)
            self.items[label] = _item_rect()
            return result

        monkeypatch.setattr(imgui, "v_slider_float", slider)
        monkeypatch.setattr(imgui, "button", button)
        monkeypatch.setattr(render, "sidebar_left", lambda _ctx: imgui.button("Pad control"))
        monkeypatch.setattr(render, "_center_area", lambda _ctx: imgui.button("Center control"))

    def frame(self) -> dict[str, _Rect]:
        self.items.clear()
        imgui.new_frame()
        imgui.set_next_window_pos((0, 0))
        imgui.set_next_window_size((1010, self.height))
        imgui.begin(
            "Pitch drag geometry",
            flags=imgui.WindowFlags_.no_title_bar | imgui.WindowFlags_.no_saved_settings,
        )
        with default_style():
            render._main(self.ctx)
        imgui.end()
        imgui.render()
        assert imgui.get_draw_data().total_vtx_count > 0
        return dict(self.items)


def _feedback(surface: _PitchSurface, index: int) -> None:
    states: tuple[Literal["queued", "preparing", "applied", "rejected"] | None, ...] = (
        None,
        "queued",
        "preparing",
        "applied",
        "rejected",
    )
    state = states[index % len(states)]
    surface.rate = GlobalRateStatus(
        requested_speed=surface.project.speed,
        state=state,
        effective_speed=None if index % 2 else 1.0,
        effective_master_period=None if index % 2 else 0.5,
        error=("Synthetic rate diagnostic " * 30) if state == "rejected" else None,
    )
    surface.key_lock = KeyLockStatus(
        requested=bool(index % 2),
        effective=None if index % 2 else False,
        pending=bool(index % 2),
        mixed=bool(index % 3),
        unconfirmed=bool(index % 4),
        error=("Synthetic Key Lock diagnostic " * 30) if index % 5 == 4 else None,
    )


@pytest.mark.parametrize("reference_bpm", [None, 120.0], ids=["speed", "bpm"])
@pytest.mark.parametrize("scale", [1.0, 1.5, 2.0])
@pytest.mark.parametrize("height", [450.0, 630.0, 900.0])
def test_rate_and_key_lock_feedback_cannot_move_controls_under_stationary_drag(
    monkeypatch: pytest.MonkeyPatch,
    reference_bpm: float | None,
    scale: float,
    height: float,
) -> None:
    surface = _PitchSurface(height, reference_bpm)
    surface.observe(monkeypatch)
    with _native_context(scale, height):
        for _ in range(4):
            baseline = surface.frame()
        rectangle = baseline["Pitch"]
        pointer = (
            (rectangle[0] + rectangle[2]) / 2,
            rectangle[1] + 0.35 * (rectangle[3] - rectangle[1]),
        )
        io = imgui.get_io()
        io.add_mouse_pos_event(*pointer)
        surface.frame()
        down = True
        io.add_mouse_button_event(imgui.MouseButton_.left, down)
        surface.frame()
        assert imgui.is_any_item_active()
        assert surface.speeds
        held_speed = surface.project.speed
        surface.speeds.clear()
        for index in range(15):
            _feedback(surface, index)
            actual = surface.frame()
            assert surface.project.speed == held_speed
            assert actual == baseline
        assert not surface.speeds
        down = False
        io.add_mouse_button_event(imgui.MouseButton_.left, down)
        surface.frame()


@pytest.mark.parametrize("scale", [1.0, 1.5, 2.0])
def test_pitch_steps_and_lock_buttons_fit_normal_viewport(
    monkeypatch: pytest.MonkeyPatch, scale: float
) -> None:
    surface = _PitchSurface(630.0, 120.0)
    surface.observe(monkeypatch)
    with _native_context(scale, surface.height):
        for _ in range(4):
            items = surface.frame()
        for label in ("Pitch", "+", "Reset", "-", "KEY LOCK##key_lock", "BPM LOCK##bpm_lock"):
            assert items[label][1] >= 0
            assert items[label][3] <= surface.height - 8


def test_real_hover_keeps_rate_and_key_lock_details_outside_control_layout(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    surface = _PitchSurface(630.0, 120.0)
    surface.rate = GlobalRateStatus(
        requested_speed=1.0,
        effective_speed=0.75,
        effective_master_period=0.5,
        state="preparing",
        error="Synthetic rate diagnostic",
    )
    surface.key_lock = KeyLockStatus(
        requested=True,
        effective=False,
        pending=True,
        mixed=True,
        error="Synthetic Key Lock diagnostic",
    )
    tooltips: list[str] = []
    native_tooltip = imgui.set_tooltip

    def tooltip(text: str) -> None:
        tooltips.append(text)
        native_tooltip(text)

    monkeypatch.setattr(imgui, "set_tooltip", tooltip)
    surface.observe(monkeypatch)
    with _native_context(1.0, surface.height):
        for _ in range(4):
            baseline = surface.frame()
        key = baseline["KEY LOCK##key_lock"]
        imgui.get_io().add_mouse_pos_event((key[0] + key[2]) / 2, (key[1] + key[3]) / 2)
        assert surface.frame() == baseline
        assert "Requested ON\nAudio OFF\nPreparing ON…\nMixed pads" in tooltips
        # The BPM readout is directly below the sidebar close button and separator.
        pitch = baseline["Pitch"]
        imgui.get_io().add_mouse_pos_event((pitch[0] + pitch[2]) / 2, pitch[1] - 24)
        assert surface.frame() == baseline
        rate = next(text for text in tooltips if "Requested speed" in text)
        assert "Requested speed: 1.00x" in rate
        assert "Audio speed: 0.75x" in rate
        assert "Audio BPM: 120.00" in rate
        assert "Preparing rate change…" in rate
        assert all("diagnostic" not in text for text in tooltips)
