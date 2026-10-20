"""Actual ImGui labels and mouse choices; Native admission is an explicit control double."""

from dataclasses import dataclass, field
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.settings import SettingsController
from flitzis_looper.models import (
    KEY_LOCK_START_BEHAVIOR_LABELS,
    KEY_LOCK_START_BEHAVIORS,
    ProjectState,
    SessionState,
)
from flitzis_looper.ui.context import SettingsActions
from flitzis_looper.ui.render import settings
from tests.flitzis_looper.controller.test_key_lock_start_settings import (
    _assert_policy,
    _NativeStartPolicy,
)
from tests.flitzis_looper.ui.test_key_lock_render import _imgui_context

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper.models import KeyLockStartBehavior
    from flitzis_looper.ui.context import UiContext
    from flitzis_looper_audio import AudioEngine


type _Point = tuple[float, float]


def _item_point() -> _Point:
    minimum, maximum = imgui.get_item_rect_min(), imgui.get_item_rect_max()
    return minimum.x + 100.0, (minimum.y + maximum.y) / 2.0


@dataclass
class _Frame:
    label: str = ""
    preview: str = ""
    combo: _Point | None = None
    options: dict[str, _Point] = field(default_factory=dict)
    selected: set[str] = field(default_factory=set)
    disabled: bool = False
    text: list[str] = field(default_factory=list)


class _SettingsRender:
    """Render the real Settings action path without constructing AppController or AudioEngine."""

    def __init__(
        self, path: Path, monkeypatch: pytest.MonkeyPatch, behavior: KeyLockStartBehavior
    ) -> None:
        self.project = ProjectState(key_lock_start_behavior=behavior)
        self.native = _NativeStartPolicy(behavior=behavior)
        self.persistence = ProjectPersistence(self.project)
        self.persistence.config_path = path
        self.persistence.mark_dirty()
        self.persistence.flush()
        controller = SimpleNamespace(
            settings=SettingsController(
                self.project,
                SessionState(),
                cast("AudioEngine", self.native),
                on_project_changed=self.persistence.mark_dirty,
            )
        )
        self.actions = SettingsActions(cast("AppController", controller))
        self.ctx = cast(
            "UiContext",
            SimpleNamespace(
                state=SimpleNamespace(project=self.project),
                ui=SimpleNamespace(settings=self.actions),
                persistence=self.persistence,
            ),
        )
        self.flush = Mock(wraps=self.persistence.flush_if_dirty)
        monkeypatch.setattr(self.persistence, "flush_if_dirty", self.flush)
        self.observed = _Frame()
        self._native_combo = imgui.begin_combo
        self._native_selectable = imgui.selectable
        self._native_disabled = imgui.begin_disabled
        self._native_text = imgui.text_wrapped
        monkeypatch.setattr(imgui, "begin_combo", self._combo)
        monkeypatch.setattr(imgui, "selectable", self._selectable)
        monkeypatch.setattr(imgui, "begin_disabled", self._disabled)
        monkeypatch.setattr(imgui, "text_wrapped", self._text)

    def _combo(self, label: str, preview: str) -> bool:
        opened = self._native_combo(label, preview)
        self.observed.label = label
        self.observed.preview = preview
        self.observed.combo = _item_point()
        return opened

    def _selectable(self, label: str, *selection: bool) -> tuple[bool, bool]:
        (selected,) = selection
        result = self._native_selectable(label, selected)
        self.observed.options[label] = _item_point()
        if selected:
            self.observed.selected.add(label)
        return result

    def _disabled(self, *values: bool) -> None:
        self.observed.disabled = values[0]
        self._native_disabled(*values)

    def _text(self, value: str) -> None:
        self.observed.text.append(value)
        self._native_text(value)

    def frame(self) -> _Frame:
        self.observed = _Frame()
        imgui.new_frame()
        imgui.set_next_window_pos((0, 0))
        imgui.set_next_window_size((590, 400))
        imgui.begin("Key Lock start Settings", flags=imgui.WindowFlags_.no_saved_settings)
        settings._key_lock_start_behavior_controls(self.ctx)
        imgui.end()
        imgui.render()
        assert imgui.get_draw_data().total_vtx_count > 0
        return self.observed

    def click(self, point: _Point) -> _Frame:
        io = imgui.get_io()
        io.add_mouse_pos_event(*point)
        down = True
        io.add_mouse_button_event(imgui.MouseButton_.left, down)
        self.frame()
        down = False
        io.add_mouse_button_event(imgui.MouseButton_.left, down)
        return self.frame()

    def open(self) -> _Frame:
        self.frame()
        frame = self.frame()
        assert frame.combo is not None
        self.click(frame.combo)
        return self.frame()


def _option_label(behavior: KeyLockStartBehavior) -> str:
    return f"{KEY_LOCK_START_BEHAVIOR_LABELS[behavior]}##key_lock_start_behavior_{behavior}"


@pytest.mark.parametrize("behavior", KEY_LOCK_START_BEHAVIORS)
def test_real_settings_combo_displays_both_policies_and_persists_an_accepted_mouse_choice(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, behavior: KeyLockStartBehavior
) -> None:
    previous: KeyLockStartBehavior = (
        "attack_at_grid" if behavior == "first_sound_at_grid" else "first_sound_at_grid"
    )
    render = _SettingsRender(tmp_path / "project.json", monkeypatch, previous)
    with _imgui_context():
        frame = render.open()
        assert frame.label == "Key Lock start behavior"
        assert frame.preview == KEY_LOCK_START_BEHAVIOR_LABELS[previous]
        assert set(frame.options) == {_option_label(option) for option in KEY_LOCK_START_BEHAVIORS}
        assert frame.selected == {_option_label(previous)}
        assert not frame.disabled
        assert "Stop playback to change" not in frame.text
        render.click(frame.options[_option_label(behavior)])
        frame = render.frame()
        assert frame.preview == KEY_LOCK_START_BEHAVIOR_LABELS[behavior]
    assert render.native.requests == [behavior]
    assert render.project.key_lock_start_behavior == render.native.behavior == behavior
    render.flush.assert_called_once_with()
    reopened = ProjectPersistence.from_config_path(render.persistence.config_path).project
    assert reopened.key_lock_start_behavior == behavior


def test_real_disabled_combo_shows_stop_hint_and_rejects_mouse_and_bypassed_action(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    render = _SettingsRender(tmp_path / "project.json", monkeypatch, "first_sound_at_grid")
    render.native.stopped = False
    original_bytes = render.persistence.config_path.read_bytes()
    with _imgui_context():
        frame = render.open()
        assert frame.disabled
        assert frame.label == "Key Lock start behavior"
        assert frame.preview == "First sound at grid"
        assert "Stop playback to change" in frame.text
        assert not frame.options
        assert render.native.requests == []
        assert not render.actions.set_key_lock_start_behavior("attack_at_grid")
        assert render.native.requests == ["attack_at_grid"]
        render.native.stopped = True
        frame = render.frame()
        assert not frame.disabled
        assert frame.preview == "First sound at grid"
        assert "Stop playback to change" not in frame.text
    assert render.project.key_lock_start_behavior == render.native.behavior == "first_sound_at_grid"
    assert render.persistence.config_path.read_bytes() == original_bytes
    render.flush.assert_not_called()
    assert not render.persistence._dirty


def test_real_mouse_race_keeps_old_preview_and_json_without_a_deferred_selection(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    render = _SettingsRender(tmp_path / "project.json", monkeypatch, "first_sound_at_grid")
    original_bytes = render.persistence.config_path.read_bytes()
    with _imgui_context():
        frame = render.open()
        assert not frame.disabled
        # Native's control double admits playback precisely at setter admission.
        render.native.admission_hook = lambda: setattr(render.native, "stopped", False)
        render.click(frame.options[_option_label("attack_at_grid")])
        frame = render.frame()
        assert frame.disabled
        assert frame.preview == "First sound at grid"
        assert "Stop playback to change" in frame.text
        assert render.native.requests == ["attack_at_grid"]
        assert (
            render.project.key_lock_start_behavior
            == render.native.behavior
            == ("first_sound_at_grid")
        )
        assert render.persistence.config_path.read_bytes() == original_bytes
        render.flush.assert_not_called()
        assert not render.persistence._dirty

        render.native.admission_hook = None
        render.native.stopped = True
        frame = render.open()
        assert frame.preview == "First sound at grid"
        assert frame.selected == {_option_label("first_sound_at_grid")}
        assert render.native.requests == ["attack_at_grid"]
        assert render.persistence.config_path.read_bytes() == original_bytes
        render.click(frame.options[_option_label("attack_at_grid")])
    _assert_policy(render.project, render.native, "attack_at_grid")
    assert render.native.requests == ["attack_at_grid", "attack_at_grid"]
    render.flush.assert_called_once_with()
