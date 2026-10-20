"""Headless real ImGui gestures through Settings, dirty persistence and Config reopen."""

from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.settings import SettingsController
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper.recording_options import RecordingOptions
from flitzis_looper.ui.context import SettingsActions
from flitzis_looper.ui.render import settings
from tests.flitzis_looper.ui.test_key_lock_render import _imgui_context

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper.ui.context import UiContext

type _Point = tuple[float, float]


def _point() -> _Point:
    start, end = imgui.get_item_rect_min(), imgui.get_item_rect_max()
    return start.x + 100.0, (start.y + end.y) / 2.0


class _RecordingRender:
    def __init__(
        self, path: Path, monkeypatch: pytest.MonkeyPatch, options: RecordingOptions
    ) -> None:
        self.project = ProjectState(recording_options=options)
        self.project.sample_paths[37] = "samples/retained-source.wav"
        self.project.pad_gain_db[37] = -3.0
        self.audio = Mock()
        self.persistence = ProjectPersistence(self.project)
        self.persistence.config_path = path
        self.persistence.mark_dirty()
        self.persistence.flush()
        controller = SimpleNamespace(
            settings=SettingsController(
                self.project,
                SessionState(),
                self.audio,
                on_project_changed=self.persistence.mark_dirty,
            )
        )
        self.ctx = cast(
            "UiContext",
            SimpleNamespace(
                state=SimpleNamespace(project=self.project),
                ui=SimpleNamespace(settings=SettingsActions(cast("AppController", controller))),
                persistence=self.persistence,
            ),
        )
        self.flush = Mock(wraps=self.persistence.flush_if_dirty)
        monkeypatch.setattr(self.persistence, "flush_if_dirty", self.flush)
        self.combos: dict[str, _Point] = {}
        self.options: dict[str, _Point] = {}
        self.text: list[str] = []
        self._native_combo = imgui.begin_combo
        self._native_selectable = imgui.selectable
        self._native_text = imgui.text_wrapped
        monkeypatch.setattr(imgui, "begin_combo", self._combo)
        monkeypatch.setattr(imgui, "selectable", self._selectable)
        monkeypatch.setattr(imgui, "text_wrapped", self._text)

    def _combo(self, label: str, preview: str) -> bool:
        opened = self._native_combo(label, preview)
        self.combos[label] = _point()
        return opened

    def _selectable(self, label: str, *selection: bool) -> tuple[bool, bool]:
        (selected,) = selection
        result = self._native_selectable(label, selected)
        self.options[label] = _point()
        return result

    def _text(self, text: str) -> None:
        self.text.append(text)
        self._native_text(text)

    def frame(self) -> None:
        self.combos.clear()
        self.options.clear()
        self.text.clear()
        imgui.new_frame()
        imgui.set_next_window_pos((0, 0))
        imgui.set_next_window_size((590, 400))
        imgui.begin("Recording settings", flags=imgui.WindowFlags_.no_saved_settings)
        settings._recording_controls(self.ctx)
        imgui.end()
        imgui.render()
        assert imgui.get_draw_data().total_vtx_count > 0

    def click(self, point: _Point) -> None:
        io = imgui.get_io()
        io.add_mouse_pos_event(*point)
        down = True
        io.add_mouse_button_event(imgui.MouseButton_.left, down)
        self.frame()
        down = False
        io.add_mouse_button_event(imgui.MouseButton_.left, down)
        self.frame()

    def open(self, label: str) -> None:
        self.frame()
        self.frame()
        self.click(self.combos[label])
        self.frame()


@pytest.mark.parametrize(
    ("options", "combo", "field", "value", "label"),
    [
        (RecordingOptions(), "Recording format", "format", "flac", "FLAC"),
        (RecordingOptions(), "WAV precision", "wav_encoding", "pcm24", "24-bit PCM"),
        (RecordingOptions(), "WAV precision", "wav_encoding", "pcm16", "16-bit PCM"),
        (RecordingOptions(format="flac"), "FLAC precision", "flac_bit_depth", 16, "16-bit"),
        (RecordingOptions(format="mp3"), "MP3 bitrate", "mp3_bitrate_kbps", 128, "128 kbit/s CBR"),
        (RecordingOptions(format="mp3"), "MP3 bitrate", "mp3_bitrate_kbps", 256, "256 kbit/s CBR"),
    ],
)
def test_actual_headless_choice_persists_only_future_recording_defaults(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    options: RecordingOptions,
    combo: str,
    field: str,
    value: str | int,
    label: str,
) -> None:
    render = _RecordingRender(tmp_path / "project.json", monkeypatch, options)
    snapshot = render.project.recording_options
    before = render.project.model_dump(exclude={"recording_options", "config_revision"})
    with _imgui_context():
        render.open(combo)
        render.click(render.options[f"{label}##recording_{field}_{value}"])
        render.frame()
    assert snapshot == options
    expected = options.model_dump() | {field: value}
    assert render.project.recording_options.model_dump() == expected
    assert render.project.model_dump(exclude={"recording_options", "config_revision"}) == before
    render.flush.assert_called_once_with()
    assert not render.persistence._dirty
    assert render.audio.mock_calls == []
    reopened = ProjectState.model_validate_json(render.persistence.config_path.read_text())
    assert reopened.recording_options.model_dump() == expected
    assert reopened.model_dump(exclude={"recording_options", "config_revision"}) == before


def test_flac_compression_persists_on_edit_completion_and_notice_is_visible(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    render = _RecordingRender(
        tmp_path / "project.json", monkeypatch, RecordingOptions(format="flac")
    )
    notice = "Recording settings restored defaults for invalid FLAC compression."
    render.persistence.recording_settings_warning = notice
    slider = Mock(side_effect=[(True, 3), (False, 3)])
    monkeypatch.setattr(imgui, "slider_int", slider)
    monkeypatch.setattr(imgui, "is_item_deactivated_after_edit", lambda: True)
    with _imgui_context():
        render.frame()
        render.frame()
        assert notice in render.text
        assert any("Audio remains lossless" in line for line in render.text)
    assert render.project.recording_options.flac_compression == 3
    assert render.persistence.load_error is None
    assert render.audio.mock_calls == []
    reopened = ProjectState.model_validate_json(render.persistence.config_path.read_text())
    assert reopened.recording_options.flac_compression == 3
    assert not render.persistence._dirty
