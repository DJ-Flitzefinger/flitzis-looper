"""Actual RECORD draws and press routing; no App, capture worker or audio device."""

from dataclasses import dataclass, field, replace
from itertools import combinations
from pathlib import Path
from typing import TYPE_CHECKING, Literal
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.controller.recording import RecordingStatus
from flitzis_looper.recording_finalize import RecordingCompletion
from flitzis_looper.ui.contextmanager import default_style
from flitzis_looper.ui.render import bottom_bar
from flitzis_looper.ui.render.settings import settings_toggle_button_label
from tests.flitzis_looper.ui import test_performance_action_layout as action_layout
from tests.flitzis_looper.ui import test_waveform_render_stability as render_stability

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.ui.context import UiContext

headless_plot = action_layout.headless_plot
pytestmark = pytest.mark.usefixtures("headless_plot")


@dataclass
class _RecordingFacade:
    """Only the recording action boundary is replaced; ImGui routing stays productive."""

    status: RecordingStatus = field(default_factory=RecordingStatus)
    intents: list[tuple[str, int | None]] = field(default_factory=list)
    acknowledgements: int = 0

    def record_mix(self) -> None:
        self.intents.append(("mix", None))

    def record_group(self, *, received_at_ns: int | None = None) -> None:
        self.intents.append(("group", received_at_ns))

    def acknowledge_failure(self) -> bool:
        self.acknowledgements += 1
        self.status = RecordingStatus(last_completion=self.status.last_completion)
        return True


def _context(
    controller: AppController, audio: Mock, monkeypatch: pytest.MonkeyPatch
) -> tuple[UiContext, _RecordingFacade, action_layout._Observer]:
    ctx = render_stability._context(controller, audio)[0]
    facade = _RecordingFacade()
    monkeypatch.setattr(ctx, "recording", facade)
    action_layout._fonts(ctx, 1.0)
    observer = action_layout._Observer()
    observer.install(monkeypatch)
    return ctx, facade, observer


def _frame(ctx: UiContext, observer: action_layout._Observer, width: float = 1100.0) -> None:
    observer.items.clear()
    io = imgui.get_io()
    io.display_size = imgui.ImVec2(1400, 900)
    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size((width + 16, 500))
    imgui.begin("productive recording controls", flags=imgui.WindowFlags_.no_title_bar)
    with default_style():
        height = bottom_bar.bottom_bar_layout(imgui.get_content_region_avail().x).height
        imgui.begin_child("bottom_bar", (-1, height))
        bottom_bar.bottom_bar(ctx)
        imgui.end_child()
    imgui.end()
    imgui.render()


def _press(
    ctx: UiContext,
    observer: action_layout._Observer,
    label: str,
    buttons: tuple[int, ...],
) -> None:
    io = imgui.get_io()
    io.add_mouse_pos_event(*observer.one(label).center)
    _frame(ctx, observer)
    for button in buttons:
        io.add_mouse_button_event(button, down=True)
    _frame(ctx, observer)


def _release(ctx: UiContext, observer: action_layout._Observer, buttons: tuple[int, ...]) -> None:
    for button in buttons:
        imgui.get_io().add_mouse_button_event(button, down=False)
    _frame(ctx, observer)


@pytest.mark.parametrize("scale", [1.0, 1.25, 1.5, 2.0])
@pytest.mark.parametrize("width", [540.0, 680.0, 1100.0])
def test_actual_center_reserves_wrapped_controls_and_full_record_transport_labels(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    scale: float,
    width: float,
) -> None:
    ctx, facade, observer = _context(controller, audio_engine_mock, monkeypatch)
    action_layout._fonts(ctx, scale)
    ctx.ui.toggle_pitch_menu("base")
    imgui.get_io().display_size = imgui.ImVec2(1400, 900)
    for _ in range(4):
        action_layout._editor_frame(ctx, observer, width)

    labels = [
        "##master_volume",
        "MULTI LOOP",
        "L",
        "Q",
        "V",
        "D",
        "M",
        "B",
        "I",
        "A",
        "RECORD",
        "START/STOP",
        settings_toggle_button_label(settings_open=False).split("##")[0],
    ]
    controls = [observer.one(label) for label in labels]
    assert all(action_layout._contained(item.rectangle, item.clip) for item in controls)
    for first, second in combinations(controls, 2):
        a, b = first.rectangle, second.rectangle
        assert a[2] <= b[0] or b[2] <= a[0] or a[3] <= b[1] or b[3] <= a[1], (
            first.label,
            a,
            second.label,
            b,
        )
    master = observer.one("##master_volume")
    assert master.rectangle[2] - master.rectangle[0] >= 300.0
    record, transport = observer.one("RECORD"), observer.one("START/STOP")
    assert record.rectangle[1] == transport.rectangle[1]
    assert transport.rectangle[0] - record.rectangle[2] >= bottom_bar.CONTROL_GROUP_GAP
    for label in ("RECORD", "START/STOP", "MULTI LOOP"):
        item = observer.one(label)
        assert len(item.glyphs) == len(label.replace(" ", ""))
        assert all(action_layout._contained(glyph, item.rectangle) for glyph in item.glyphs)
    status = observer.one("Recording: Idle")
    assert action_layout._contained(status.rectangle, status.clip)
    assert facade.intents == []


@pytest.mark.parametrize(
    ("buttons", "expected"),
    [((0,), [("group", 73)]), ((1,), [("mix", None)]), ((0, 1), [("mix", None)])],
    ids=["left-original-time", "right-native-time", "simultaneous-right-wins"],
)
def test_native_press_edges_dispatch_once_and_hold_never_repeats(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    buttons: tuple[int, ...],
    expected: list[tuple[str, int | None]],
) -> None:
    ctx, facade, observer = _context(controller, audio_engine_mock, monkeypatch)
    timestamp = Mock(return_value=73)
    monkeypatch.setattr(ctx.input, "capture_timestamp_ns", timestamp)
    start, stop, mute = Mock(), Mock(), Mock()
    monkeypatch.setattr(ctx.audio.global_, "start_or_restart_start_stop", start)
    monkeypatch.setattr(ctx.audio.global_, "stop_start_stop", stop)
    monkeypatch.setattr(ctx.audio.global_, "set_momentary_output_mute", mute)
    for _ in range(4):
        _frame(ctx, observer)
    _press(ctx, observer, "RECORD", buttons)
    assert facade.intents == expected
    for _ in range(4):
        _frame(ctx, observer)
    _release(ctx, observer, buttons)
    assert facade.intents == expected
    assert timestamp.call_count == (1 if buttons == (0,) else 0)
    start.assert_not_called()
    stop.assert_not_called()
    mute.assert_not_called()
    assert not controller.session.global_start_stop_left_pressed


def test_held_drag_into_record_and_middle_press_are_not_record_intents(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx, facade, observer = _context(controller, audio_engine_mock, monkeypatch)
    for _ in range(4):
        _frame(ctx, observer)
    io = imgui.get_io()
    io.add_mouse_pos_event(450, 450)
    io.add_mouse_button_event(0, down=True)
    _frame(ctx, observer)
    io.add_mouse_pos_event(*observer.one("RECORD").center)
    for _ in range(3):
        _frame(ctx, observer)
    _release(ctx, observer, (0,))
    _press(ctx, observer, "RECORD", (2,))
    _release(ctx, observer, (2,))
    assert facade.intents == []


def test_pad_owned_pointer_suppresses_both_record_press_edges(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx, facade, observer = _context(controller, audio_engine_mock, monkeypatch)
    monkeypatch.setattr(type(ctx.ui.arrangement), "owns_pointer", property(lambda _self: True))
    for _ in range(4):
        _frame(ctx, observer)
    _press(ctx, observer, "RECORD", (0, 1))
    _release(ctx, observer, (0, 1))
    assert facade.intents == []
    assert facade.acknowledgements == 0


@pytest.mark.parametrize("phase", ["Preparing", "Recording", "Finalizing", "Failed"])
def test_pending_and_failed_projection_never_earns_effective_record_color(
    phase: Literal["Preparing", "Recording", "Finalizing", "Failed"],
) -> None:
    status = replace(RecordingStatus(), phase=phase, busy=True, retired=False, pending="start")
    assert bottom_bar.recording_button_style(status) == "regular"
    assert "Capture active: No" in bottom_bar.recording_status_tooltip(status)
    assert bottom_bar.recording_button_style(replace(status, effective_recording=True)) == "active"


@pytest.mark.parametrize("uncertain", [False, True])
def test_native_failure_ack_preserves_error_without_rendering_technical_details(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    uncertain: bool,
) -> None:
    ctx, facade, observer = _context(controller, audio_engine_mock, monkeypatch)
    facade.status = RecordingStatus(
        phase="Failed",
        busy=uncertain,
        retired=not uncertain,
        retirement_uncertain=uncertain,
        pending="retire" if uncertain else None,
        error="Writer failed",
        incomplete_directory=Path("record/.incomplete/original-take"),
    )
    for _ in range(4):
        _frame(ctx, observer)
    failure = observer.one("Recording: Failed")
    assert action_layout._contained(failure.rectangle, failure.clip)
    assert "Writer failed" not in bottom_bar.recording_status_tooltip(facade.status)
    assert "Incomplete take: record" in bottom_bar.recording_status_tooltip(facade.status)
    _press(ctx, observer, "RECORD", (0, 1))
    _release(ctx, observer, (0, 1))
    assert facade.intents == []
    assert facade.status.error == "Writer failed"
    _press(ctx, observer, "ACK", (0,))
    _release(ctx, observer, (0,))
    assert facade.acknowledgements == (0 if uncertain else 1)
    if uncertain:
        assert facade.status.error == "Writer failed"
        assert facade.status.retirement_uncertain
    else:
        assert facade.status.phase == "Idle"


def test_recording_does_not_own_ordinary_start_stop_gestures(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx, facade, observer = _context(controller, audio_engine_mock, monkeypatch)
    facade.status = RecordingStatus(
        phase="Recording", effective_recording=True, busy=True, retired=False
    )
    start, stop = Mock(), Mock()
    monkeypatch.setattr(ctx.audio.global_, "start_or_restart_start_stop", start)
    monkeypatch.setattr(ctx.audio.global_, "stop_start_stop", stop)
    for _ in range(4):
        _frame(ctx, observer)
    _press(ctx, observer, "START/STOP", (0,))
    _release(ctx, observer, (0,))
    start.assert_called_once()
    _press(ctx, observer, "START/STOP", (1,))
    _release(ctx, observer, (1,))
    stop.assert_called_once_with()
    assert facade.intents == []
    assert facade.status.effective_recording


def test_closed_completion_and_warning_are_visible_from_original_status_metadata(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx, facade, observer = _context(controller, audio_engine_mock, monkeypatch)
    completion = RecordingCompletion(
        path=Path("record/verified-take.wav"),
        extent=Mock(),
        pcm=Mock(),
        codec=Mock(),
        warning="Overrange samples were saturated",
    )
    facade.status = RecordingStatus(last_completion=completion)
    for _ in range(4):
        _frame(ctx, observer)
    saved = observer.one("Saved (warning): verified-take.wav")
    assert action_layout._contained(saved.rectangle, saved.clip)
    tooltip = bottom_bar.recording_status_tooltip(facade.status)
    assert str(completion.path) in tooltip
    assert completion.warning is not None
    assert completion.warning in tooltip
    assert facade.status.last_completion is completion
    assert facade.intents == []


@pytest.mark.parametrize("phase", ["Preparing", "Finalizing"])
def test_busy_phase_notice_and_pending_metadata_survive_a_single_record_gesture(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    phase: Literal["Idle", "Preparing", "Recording", "Finalizing", "Failed"],
) -> None:
    ctx, facade, observer = _context(controller, audio_engine_mock, monkeypatch)
    facade.status = replace(
        RecordingStatus(),
        phase=phase,
        busy=True,
        retired=False,
        pending="encode",
        notice="Recording owner is busy",
    )
    for _ in range(4):
        _frame(ctx, observer)
    busy = observer.one(f"Recording {phase}: Recording owner is busy")
    assert action_layout._contained(busy.rectangle, busy.clip)
    tooltip = bottom_bar.recording_status_tooltip(facade.status)
    assert "Pending: encode" in tooltip
    assert "Capture active: No" in tooltip
    _press(ctx, observer, "RECORD", (1,))
    for _ in range(3):
        _frame(ctx, observer)
    _release(ctx, observer, (1,))
    assert facade.intents == [("mix", None)]
    assert facade.status.busy
    assert facade.status.phase == phase
