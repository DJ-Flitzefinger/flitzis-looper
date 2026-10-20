"""GLOBAL START/STOP keeps failed native admission inside the GUI boundary."""

from contextlib import contextmanager
from typing import TYPE_CHECKING, cast
from unittest.mock import call

import pytest
from imgui_bundle import imgui

from flitzis_looper.models import BeatGrid, SampleAnalysis
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import bottom_bar
from tests.flitzis_looper.conftest import (
    FakeCapturedGlobalStart,
    FakeGlobalPlaybackBatchTicket,
    FakeInputRuntimePadBinding,
    current_timing_metadata,
)
from tests.flitzis_looper.controller.transport.test_residency import Ticket

if TYPE_CHECKING:
    from collections.abc import Iterator
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


PENDING_PLACEMENT = "content placement publication is pending"


def _context(
    controller: AppController, audio: Mock, *, busy: str = "loading"
) -> tuple[UiContext, dict[int, FakeInputRuntimePadBinding]]:
    """Represent loaded PAD1 and a busy PAD2 without opening files or a device."""
    project = controller.project
    metadata = {sample_id: current_timing_metadata(sample_id=sample_id) for sample_id in (0, 2)}
    bindings = {
        sample_id: FakeInputRuntimePadBinding(sample_id, accepted_timing=timing, intent="automatic")
        for sample_id, timing in metadata.items()
    }
    for sample_id in (0, 2):
        project.sample_paths[sample_id] = f"samples/synthetic-pad-{sample_id}.wav"
        project.sample_durations[sample_id] = 60.0
        project.sample_analysis[sample_id] = SampleAnalysis(
            bpm=120.0,
            key="C",
            beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
        )
        project.pad_loop_auto[sample_id] = False
        project.pad_loop_start_s[sample_id] = 0.0
        project.pad_loop_end_s[sample_id] = 2.0
    project.sample_paths[1] = "samples/synthetic-incoming-pad.wav"
    session = controller.session
    if busy == "loading":
        session.loading_sample_ids.add(1)
    else:
        session.analyzing_sample_ids.add(1)
    session.active_sample_ids = {0, 2}
    session.paused_sample_ids = {2}
    session.global_stop_engaged = True
    session.global_stop_restore_sample_ids = {0, 2}
    session.pad_playhead_s[0] = 1.25
    audio.output_sample_rate.return_value = 48_000
    audio.current_constant_timing.side_effect = metadata.get
    audio.pad_timing_intent.return_value = "automatic"
    audio.current_input_runtime_pad_binding.side_effect = bindings.get
    audio.loaded_residency.return_value = None
    ctx = UiContext(controller)
    audio.reset_mock()
    return ctx, bindings


def _assert_no_raw_transport(audio: Mock) -> None:
    """A failed native transaction has no older unguarded playback fallback."""
    audio.start_global_playback_batch.assert_not_called()
    audio.stop_all.assert_not_called()
    audio.stop_sample.assert_not_called()
    audio.play_sample.assert_not_called()
    audio.play_sample_exclusive.assert_not_called()
    audio.publish_pad_start.assert_not_called()
    audio.cancel_pad_launches.assert_not_called()


@pytest.mark.parametrize("busy", ["loading", "analyzing"])
def test_global_stop_pending_placement_preserves_state_owners_and_next_gesture(
    controller: AppController, audio_engine_mock: Mock, busy: str
) -> None:
    ctx, bindings = _context(controller, audio_engine_mock, busy=busy)
    launched = Ticket("accepted")
    controller.transport.residency._launched[0] = launched.native_ticket
    before_session = controller.session.model_dump()
    before_project = controller.project.model_dump()
    audio_engine_mock.cancel_all_launches.side_effect = RuntimeError(PENDING_PLACEMENT)

    ctx.audio.global_.stop_start_stop()

    assert ctx.audio.global_.playback_error == PENDING_PLACEMENT
    assert controller.session.model_dump() == before_session
    assert controller.project.model_dump() == before_project
    assert cast("object", controller.transport.residency._launched[0]) is launched
    assert not launched.launch_cancelled
    audio_engine_mock.cancel_all_launches.assert_called_once_with()
    audio_engine_mock.stop_global_playback_batch.assert_not_called()
    audio_engine_mock.capture_global_start.assert_not_called()
    audio_engine_mock.publish_captured_global_start.assert_not_called()
    _assert_no_raw_transport(audio_engine_mock)

    controller.transport.on_frame_render()
    controller.transport.on_frame_render()

    assert controller.session.model_dump() == before_session
    assert controller.project.model_dump() == before_project
    assert ctx.audio.global_.playback_error == PENDING_PLACEMENT
    audio_engine_mock.cancel_all_launches.assert_called_once_with()
    audio_engine_mock.stop_global_playback_batch.assert_not_called()
    assert not launched.launch_cancelled

    stop = FakeGlobalPlaybackBatchTicket("pending")
    audio_engine_mock.cancel_all_launches.side_effect = None
    audio_engine_mock.cancel_all_launches.return_value = [0]
    audio_engine_mock.stop_global_playback_batch.return_value = stop

    ctx.audio.global_.stop_start_stop()

    assert ctx.audio.global_.playback_error is None
    assert controller.session.model_dump() == before_session
    assert controller.project.model_dump() == before_project
    assert launched.launch_cancelled
    assert audio_engine_mock.cancel_all_launches.call_count == 2
    audio_engine_mock.stop_global_playback_batch.assert_called_once_with(
        [bindings[0], bindings[2]], received_at_ns=None
    )
    stop.status = "accepted"
    controller.transport.on_frame_render()
    assert controller.session.active_sample_ids == {0, 2}
    assert controller.session.paused_sample_ids == {2}
    assert controller.session.global_stop_restore_sample_ids == {0}
    assert controller.session.global_stop_engaged
    assert controller.project.model_dump() == before_project
    _assert_no_raw_transport(audio_engine_mock)


def test_failed_native_global_cancel_does_not_cancel_original_deferred_start(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ctx, _ = _context(controller, audio_engine_mock)
    audio_engine_mock.publish_captured_global_start.side_effect = lambda _capture: None
    ctx.audio.global_.start_or_restart_start_stop(received_at_ns=17)
    original = audio_engine_mock.publish_captured_global_start.call_args.args[0]
    assert isinstance(original, FakeCapturedGlobalStart)
    assert not original.cancelled
    before_session = controller.session.model_dump()
    before_project = controller.project.model_dump()
    audio_engine_mock.cancel_all_launches.side_effect = RuntimeError(PENDING_PLACEMENT)

    ctx.audio.global_.stop_start_stop()

    assert ctx.audio.global_.playback_error == PENDING_PLACEMENT
    assert not original.cancelled
    assert original.state() == "captured"
    assert controller.session.model_dump() == before_session
    assert controller.project.model_dump() == before_project
    audio_engine_mock.stop_global_playback_batch.assert_not_called()
    assert audio_engine_mock.capture_global_start.call_count == 1
    _assert_no_raw_transport(audio_engine_mock)


def test_global_stop_batch_error_keeps_telemetry_truth_without_background_retry(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ctx, bindings = _context(controller, audio_engine_mock)
    before_session = controller.session.model_dump()
    before_project = controller.project.model_dump()
    failure = "Failed to send GlobalPlaybackBatch - buffer may be full"
    audio_engine_mock.stop_global_playback_batch.side_effect = RuntimeError(failure)

    ctx.audio.global_.stop_start_stop()

    assert ctx.audio.global_.playback_error == failure
    assert controller.session.model_dump() == before_session
    assert controller.project.model_dump() == before_project
    audio_engine_mock.cancel_all_launches.assert_called_once_with()
    audio_engine_mock.stop_global_playback_batch.assert_called_once_with(
        [bindings[0], bindings[2]], received_at_ns=None
    )
    controller.transport.on_frame_render()
    controller.transport.on_frame_render()
    assert controller.session.model_dump() == before_session
    assert controller.project.model_dump() == before_project
    assert ctx.audio.global_.playback_error == failure
    assert audio_engine_mock.stop_global_playback_batch.call_count == 1
    audio_engine_mock.capture_global_start.assert_not_called()
    audio_engine_mock.publish_captured_global_start.assert_not_called()
    _assert_no_raw_transport(audio_engine_mock)

    audio_engine_mock.stop_global_playback_batch.side_effect = None

    ctx.audio.global_.stop_start_stop()

    assert ctx.audio.global_.playback_error is None
    assert audio_engine_mock.stop_global_playback_batch.call_args_list == [
        call([bindings[0], bindings[2]], received_at_ns=None),
        call([bindings[0], bindings[2]], received_at_ns=None),
    ]
    assert controller.session.active_sample_ids == {0, 2}
    assert controller.session.paused_sample_ids == {2}
    assert controller.project.model_dump() == before_project
    _assert_no_raw_transport(audio_engine_mock)


@pytest.mark.parametrize("phase", ["capture", "admission"])
@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
def test_global_start_terminal_failure_is_nonfatal_and_needs_new_explicit_gesture(
    controller: AppController,
    audio_engine_mock: Mock,
    phase: str,
    error_type: type[Exception],
) -> None:
    ctx, _ = _context(controller, audio_engine_mock)
    before_session = controller.session.model_dump()
    before_project = controller.project.model_dump()
    failure = "current complete global source/placement unavailable"
    operation = (
        audio_engine_mock.capture_global_start
        if phase == "capture"
        else audio_engine_mock.publish_captured_global_start
    )
    original_operation = operation.side_effect
    operation.side_effect = error_type(failure)

    ctx.audio.global_.start_or_restart_start_stop(received_at_ns=17)

    assert ctx.audio.global_.playback_error == failure
    assert controller.session.model_dump() == before_session
    assert controller.project.model_dump() == before_project
    audio_engine_mock.capture_global_start.assert_called_once()
    if phase == "capture":
        audio_engine_mock.publish_captured_global_start.assert_not_called()
    else:
        audio_engine_mock.publish_captured_global_start.assert_called_once()
        captured = audio_engine_mock.publish_captured_global_start.call_args.args[0]
        assert isinstance(captured, FakeCapturedGlobalStart)
        assert captured.cancelled
    controller.transport.on_frame_render()
    controller.transport.on_frame_render()
    assert controller.session.model_dump() == before_session
    assert controller.project.model_dump() == before_project
    assert ctx.audio.global_.playback_error == failure
    assert operation.call_count == 1
    audio_engine_mock.stop_global_playback_batch.assert_not_called()
    audio_engine_mock.cancel_all_launches.assert_not_called()
    _assert_no_raw_transport(audio_engine_mock)
    operation.side_effect = original_operation

    ctx.audio.global_.start_or_restart_start_stop(received_at_ns=29)

    assert ctx.audio.global_.playback_error is None
    assert audio_engine_mock.capture_global_start.call_count == 2
    assert audio_engine_mock.capture_global_start.call_args.kwargs == {"received_at_ns": 29}
    assert audio_engine_mock.publish_captured_global_start.call_count == (
        1 if phase == "capture" else 2
    )
    assert controller.session.active_sample_ids == {0, 2}
    assert controller.session.paused_sample_ids == {2}
    assert controller.project.model_dump() == before_project
    _assert_no_raw_transport(audio_engine_mock)


@contextmanager
def _imgui_context() -> Iterator[None]:
    previous = imgui.get_current_context()
    owned = imgui.create_context()
    imgui.set_current_context(owned)
    io = imgui.get_io()
    io.set_ini_filename(None)
    io.display_size = imgui.ImVec2(400, 160)
    io.delta_time = 1 / 60
    io.backend_flags |= imgui.BackendFlags_.renderer_has_textures
    try:
        yield
    finally:
        imgui.destroy_context(owned)
        if previous is not None:
            imgui.set_current_context(previous)


def _button_frame(ctx: UiContext) -> tuple[tuple[int, ...], tuple[float, float]]:
    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size((400, 160))
    imgui.begin("GLOBAL START/STOP failure", flags=imgui.WindowFlags_.no_saved_settings)
    bottom_bar._start_stop_button(ctx)
    minimum, maximum = imgui.get_item_rect_min(), imgui.get_item_rect_max()
    midpoint = ((minimum.x + maximum.x) / 2, (minimum.y + maximum.y) / 2)
    colors = tuple(
        vertex.col for vertex in imgui.internal.get_current_window().draw_list.vtx_buffer
    )
    imgui.end()
    imgui.render()
    assert imgui.get_draw_data().total_vtx_count > 0
    return colors, midpoint


def test_global_start_stop_keeps_failed_native_admission_outside_control_and_hover(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx, _ = _context(controller, audio_engine_mock)
    audio_engine_mock.cancel_all_launches.side_effect = RuntimeError(PENDING_PLACEMENT)
    ctx.audio.global_.stop_start_stop()
    tooltips: list[str] = []
    original_tooltip = imgui.set_tooltip

    def tooltip(text: str) -> None:
        tooltips.append(text)
        original_tooltip(text)

    monkeypatch.setattr(imgui, "set_tooltip", tooltip)
    with _imgui_context():
        _, midpoint = _button_frame(ctx)
        imgui.get_io().add_mouse_pos_event(*midpoint)
        colors, _ = _button_frame(ctx)

        assert imgui.color_convert_float4_to_u32((1.0, 0.25, 0.2, 1.0)) not in colors
        assert not any(PENDING_PLACEMENT in text for text in tooltips)
        assert "Start/restart loops; right stop; hold mouse wheel for mute" in tooltips
    audio_engine_mock.cancel_all_launches.assert_called_once_with()
    audio_engine_mock.stop_global_playback_batch.assert_not_called()


def test_held_right_press_never_retries_failed_stop_against_replacement_source(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    ctx, bindings = _context(controller, audio_engine_mock)
    audio_engine_mock.cancel_all_launches.side_effect = RuntimeError(PENDING_PLACEMENT)
    with _imgui_context():
        _, midpoint = _button_frame(ctx)
        io = imgui.get_io()
        io.add_mouse_pos_event(*midpoint)
        _button_frame(ctx)
        io.add_mouse_button_event(imgui.MouseButton_.right, down=True)
        _button_frame(ctx)
        audio_engine_mock.cancel_all_launches.assert_called_once_with()
        assert ctx.audio.global_.playback_error == PENDING_PLACEMENT

        replacement = FakeInputRuntimePadBinding(
            0, accepted_timing=current_timing_metadata(sample_id=0), intent="automatic"
        )
        bindings[0] = replacement
        audio_engine_mock.cancel_all_launches.side_effect = None
        audio_engine_mock.cancel_all_launches.return_value = []
        for _ in range(3):
            _button_frame(ctx)
        audio_engine_mock.cancel_all_launches.assert_called_once_with()
        audio_engine_mock.stop_global_playback_batch.assert_not_called()
        assert ctx.audio.global_.playback_error == PENDING_PLACEMENT

        io.add_mouse_button_event(imgui.MouseButton_.right, down=False)
        _button_frame(ctx)
        io.add_mouse_button_event(imgui.MouseButton_.right, down=True)
        _button_frame(ctx)
        assert audio_engine_mock.cancel_all_launches.call_count == 2
        audio_engine_mock.stop_global_playback_batch.assert_called_once_with(
            [replacement, bindings[2]], received_at_ns=None
        )
        assert ctx.audio.global_.playback_error is None
