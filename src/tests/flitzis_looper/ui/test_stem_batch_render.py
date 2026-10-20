"""Real Settings/modal frames with control-only metadata and batch boundary doubles."""

from dataclasses import dataclass
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.controller.performance_confirmation import StemBatchConfirmation
from flitzis_looper.controller.stem_batch import StemBatchItemStatus, StemBatchSummary
from flitzis_looper.models import STEM_SEPARATOR_LABELS, PadContentIdentity
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import settings
from tests.flitzis_looper.ui import test_performance_confirmation_render as modal_frames

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.models import StemSeparator

headless_plot = modal_frames.headless_plot
pytestmark = pytest.mark.usefixtures("headless_plot")
_GENERATE = "Generate Stems of all loaded PADs"


def _context(controller: AppController, audio: Mock) -> UiContext:
    for pad_id in (0, 215):
        controller.project.sample_paths[pad_id] = f"samples/original-{pad_id}.wav"
        controller.project.pad_content[pad_id] = PadContentIdentity(
            instance_id=f"{pad_id + 1:032x}", material_id=f"{pad_id + 1000:032x}"
        )
    audio.waveform_source_identity.return_value = (7, "a" * 64, 128, 44_100)
    audio.loaded_sample_shape.return_value = (44_100, 1, 128)
    return UiContext(controller)


def _settings_frame(ctx: UiContext) -> None:
    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size(imgui.get_io().display_size)
    imgui.begin("Stem batch Settings", flags=imgui.WindowFlags_.no_saved_settings)
    settings._stem_generation_controls(ctx)
    settings._stem_batch_controls(ctx)
    imgui.end()
    imgui.render()
    assert imgui.get_draw_data().total_vtx_count > 0


def _settings_click(ctx: UiContext, observed: modal_frames._DrawObservation, label: str) -> None:
    left, top, right, bottom = observed.buttons[label]
    io = imgui.get_io()
    io.add_mouse_pos_event((left + right) / 2, (top + bottom) / 2)
    _settings_frame(ctx)
    pressed = True
    io.add_mouse_button_event(imgui.MouseButton_.left, pressed)
    _settings_frame(ctx)
    pressed = False
    io.add_mouse_button_event(imgui.MouseButton_.left, pressed)
    _settings_frame(ctx)


@pytest.mark.parametrize("separator", ["demucs:htdemucs", "bs-roformer:musdb18hq"])
@pytest.mark.parametrize(("width", "scale"), [(600, 1.0), (600, 2.0), (1200, 1.5)])
def test_settings_mouse_request_and_cancel_preserve_captured_all_bank_scope(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    separator: StemSeparator,
    width: int,
    scale: float,
) -> None:
    ctx = _context(controller, audio_engine_mock)
    imgui.get_io().display_size = imgui.ImVec2(width, 900)
    imgui.get_style().font_scale_main = scale
    controller.project.stem_separator = separator
    controller.project.demucs_shifts = 2
    controller.project.demucs_overlap = 0.75
    start = Mock(return_value=True)
    monkeypatch.setattr(controller.stem_batch, "start", start)
    observed = modal_frames._observe(monkeypatch)
    _settings_frame(ctx)
    _settings_frame(ctx)
    _settings_click(ctx, observed, _GENERATE)
    intent = ctx.ui.confirmation.pending
    assert isinstance(intent, StemBatchConfirmation)
    assert len(intent.inventory.items) == 216
    assert [item.pad_id for item in intent.inventory.items if item is not None] == [0, 215]
    controller.project.selected_bank = 5
    controller.project.selected_pad = 215
    controller.session.settings_open = True
    before = controller.project.model_dump(), controller.session.model_dump()
    observed.texts.clear()
    modal_frames._frame(ctx)
    modal_frames._frame(ctx)
    assert observed.texts[0] == "Generate stems for 2 loaded PADs?"
    assert any("all 6 banks (216 slots)" in text for text in observed.texts)
    assert any(STEM_SEPARATOR_LABELS[separator] in text for text in observed.texts)
    assert any("Actual producer identity" in text for text in observed.texts)
    if separator == "demucs:htdemucs":
        assert any("shifts 2, overlap 0.75" in text for text in observed.texts)
    for label in ("GENERATE STEMS", "CANCEL"):
        left, top, right, bottom = observed.buttons[label]
        assert 0 <= left < right <= width
        assert 0 <= top < bottom <= 900
    modal_frames._click(ctx, observed, "CANCEL")
    assert ctx.ui.confirmation.pending is None
    assert (controller.project.model_dump(), controller.session.model_dump()) == before
    start.assert_not_called()


def test_changed_original_does_not_disable_batch_accept_or_retarget_its_inventory(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = _context(controller, audio_engine_mock)
    start = Mock(return_value=True)
    monkeypatch.setattr(controller.stem_batch, "start", start)
    ctx.ui.confirmation.request_all_loaded_stems()
    intent = ctx.ui.confirmation.pending
    assert isinstance(intent, StemBatchConfirmation)
    controller.project.pad_content[215] = PadContentIdentity(
        instance_id="f" * 32, material_id="e" * 32
    )
    controller.project.sample_paths[1] = "samples/newly-loaded.wav"
    controller.project.selected_bank = 5
    controller.project.selected_pad = 215
    assert ctx.ui.confirmation.is_current(intent)
    observed = modal_frames._observe(monkeypatch)
    modal_frames._frame(ctx)
    modal_frames._frame(ctx)
    modal_frames._click(ctx, observed, "GENERATE STEMS")
    modal_frames._frame(ctx)
    assert ctx.ui.confirmation.pending is None
    start.assert_called_once_with(intent.inventory)
    assert start.call_args.args[0] is intent.inventory
    assert intent.inventory.items[1] is None
    original = intent.inventory.items[215]
    assert original is not None
    assert original.content != controller.project.pad_content[215]
    assert controller.project.selected_pad == 215


def test_batch_escape_uses_common_modal_without_cancelling_existing_work(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, audio_engine_mock)
    start, cancel = Mock(), Mock()
    monkeypatch.setattr(controller.stem_batch, "start", start)
    monkeypatch.setattr(controller.stem_batch, "cancel", cancel)
    ctx.ui.confirmation.request_all_loaded_stems()
    modal_frames._frame(ctx)
    modal_frames._frame(ctx)
    pressed = True
    imgui.get_io().add_key_event(imgui.Key.escape, pressed)
    modal_frames._frame(ctx)
    assert ctx.ui.confirmation.pending is None
    pressed = False
    imgui.get_io().add_key_event(imgui.Key.escape, pressed)
    modal_frames._frame(ctx)
    start.assert_not_called()
    cancel.assert_not_called()


@dataclass
class _ChildGeometry:
    scrollbar: bool = False
    height: float = 0
    scroll_max: float = 0


def _observe_child(monkeypatch: pytest.MonkeyPatch) -> _ChildGeometry:
    geometry = _ChildGeometry()
    original = imgui.end_child

    def end_child() -> None:
        window = imgui.internal.get_current_window()
        geometry.scrollbar = window.scrollbar_y
        geometry.height = window.size.y
        geometry.scroll_max = window.scroll_max.y
        original()

    monkeypatch.setattr(imgui, "end_child", end_child)
    return geometry


def _mixed_summary() -> StemBatchSummary:
    items = (
        StemBatchItemStatus(0, "done"),
        StemBatchItemStatus(1, "queued"),
        StemBatchItemStatus(2, "running"),
        StemBatchItemStatus(3, "skipped", "Original content changed"),
        StemBatchItemStatus(4, "failed", "Resource unavailable"),
        StemBatchItemStatus(5, "cancelling"),
        StemBatchItemStatus(6, "cancelled"),
        *(StemBatchItemStatus(pad_id, "queued") for pad_id in range(7, 216)),
    )
    return StemBatchSummary(
        items=items,
        total=216,
        queued=210,
        running=1,
        done=1,
        skipped=1,
        failed=1,
        cancelling=1,
        cancelled=1,
        active=True,
    )


def test_settings_reports_original_216_rows_in_bounded_scroll_and_cancel_uses_owner(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, audio_engine_mock)
    batch = Mock(summary=_mixed_summary())
    monkeypatch.setattr(controller, "stem_batch", batch)
    observed = modal_frames._observe(monkeypatch)
    geometry = _observe_child(monkeypatch)
    controller.project.selected_bank = 5
    controller.project.selected_pad = 215
    before = controller.project.model_dump()
    _settings_frame(ctx)
    _settings_frame(ctx)
    _settings_frame(ctx)
    assert "Settled 4 of 216 loaded PADs." in observed.texts
    assert "PAD 1: Done - stems saved" in observed.texts
    assert "PAD 216: Queued" in observed.texts
    assert "PAD 4: Skipped - Original content changed" in observed.texts
    assert "PAD 5: Failed" in observed.texts
    assert not any("Resource unavailable" in text for text in observed.texts)
    assert "PAD 6: Cancelling" in observed.texts
    assert "PAD 7: Cancelled" in observed.texts
    assert any(
        "Playback readiness is reported separately for each PAD" in text for text in observed.texts
    )
    assert {
        int(text.split(":", 1)[0].removeprefix("PAD "))
        for text in observed.texts
        if text.startswith("PAD ")
    } == set(range(1, 217))
    assert geometry.scrollbar
    assert geometry.scroll_max > geometry.height
    assert geometry.height <= 6 * imgui.get_text_line_height_with_spacing() + 0.01
    _settings_click(ctx, observed, "Cancel batch")
    batch.cancel.assert_called_once_with()
    batch.start.assert_not_called()
    batch.dismiss_result.assert_not_called()
    assert controller.project.model_dump() == before


def test_settled_partial_results_remain_visible_until_explicit_dismissal(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, audio_engine_mock)
    summary = StemBatchSummary(
        items=(StemBatchItemStatus(0, "done"), StemBatchItemStatus(215, "failed", "Disk full")),
        total=2,
        queued=0,
        running=0,
        done=1,
        skipped=0,
        failed=1,
        cancelling=0,
        cancelled=0,
        active=False,
    )
    batch = Mock(summary=summary)
    monkeypatch.setattr(controller, "stem_batch", batch)
    observed = modal_frames._observe(monkeypatch)
    _settings_frame(ctx)
    _settings_frame(ctx)
    assert "Settled 2 of 2 loaded PADs." in observed.texts
    assert "PAD 1: Done - stems saved" in observed.texts
    assert "PAD 216: Failed" in observed.texts
    assert not any("Disk full" in text for text in observed.texts)
    assert "Cancel batch" not in observed.buttons
    batch.dismiss_result.assert_not_called()
    _settings_click(ctx, observed, "Dismiss batch results")
    batch.dismiss_result.assert_called_once_with()
    batch.cancel.assert_not_called()


def test_settings_batch_keeps_failed_status_without_private_technical_detail(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, audio_engine_mock)
    detail = r"Native refused 'D:\Private Music\unreleased.wav': source generation mismatch"
    summary = StemBatchSummary(
        items=(StemBatchItemStatus(215, "failed", detail),),
        total=1,
        queued=0,
        running=0,
        done=0,
        skipped=0,
        failed=1,
        cancelling=0,
        cancelled=0,
        active=False,
    )
    batch = Mock(summary=summary)
    monkeypatch.setattr(controller, "stem_batch", batch)
    observed = modal_frames._observe(monkeypatch)
    _settings_frame(ctx)
    _settings_frame(ctx)
    assert "PAD 216: Failed" in observed.texts
    assert not any(
        "Private Music" in text or "source generation mismatch" in text for text in observed.texts
    )
    assert "Dismiss batch results" in observed.buttons
    assert batch.summary is summary
    assert summary.items[0].detail == detail
    batch.poll.assert_not_called()
