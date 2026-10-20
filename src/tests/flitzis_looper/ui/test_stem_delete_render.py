"""Real Settings/modal gestures with logical deletion and cleanup boundary doubles."""

from dataclasses import FrozenInstanceError, dataclass
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.controller.performance_confirmation import StemDeleteConfirmation
from flitzis_looper.controller.stem_delete import StemDeleteItemStatus, StemDeleteSummary
from flitzis_looper.models import PadContentIdentity
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import settings
from tests.flitzis_looper.controller.test_stem_delete_inventory import _entry
from tests.flitzis_looper.ui import test_performance_confirmation_render as modal_frames

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController

headless_plot = modal_frames.headless_plot
pytestmark = pytest.mark.usefixtures("headless_plot")
_DELETE = "Delete Stems of all loaded PADs"
_GENERATE = "Generate Stems of all loaded PADs"


def _assign_field(target: object, name: str, value: object) -> None:
    setattr(target, name, value)


def _context(controller: AppController, audio: Mock) -> UiContext:
    shared = _entry()
    for pad_id in (0, 35, 215):
        controller.project.sample_paths[pad_id] = f"samples/original-{pad_id}.wav"
        controller.project.pad_content[pad_id] = PadContentIdentity(
            instance_id=f"{pad_id + 1:032x}", material_id="b" * 32
        )
        controller.project.stem_cache[pad_id] = _entry(2) if pad_id == 35 else shared
    audio.waveform_source_identity.return_value = (7, "a" * 64, 128, 44_100)
    audio.loaded_sample_shape.return_value = (44_100, 1, 128)
    return UiContext(controller)


def _settings_frame(ctx: UiContext) -> None:
    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size(imgui.get_io().display_size)
    imgui.begin("Stem deletion Settings", flags=imgui.WindowFlags_.no_saved_settings)
    settings._stem_batch_controls(ctx)
    settings._stem_delete_controls(ctx)
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


@pytest.mark.parametrize(("width", "scale"), [(600, 1.0), (600, 2.0), (1200, 1.5)])
def test_settings_mouse_warning_counts_shared_versions_and_cancel_preserves_originals(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    width: int,
    scale: float,
) -> None:
    ctx = _context(controller, audio_engine_mock)
    io = imgui.get_io()
    io.display_size = imgui.ImVec2(width, 900)
    io.display_framebuffer_scale = imgui.ImVec2(scale, scale)
    imgui.get_style().font_scale_main = scale
    start = Mock(return_value=True)
    monkeypatch.setattr(controller.stem_delete, "start", start)
    observed = modal_frames._observe(monkeypatch)
    _settings_frame(ctx)
    _settings_frame(ctx)
    _settings_click(ctx, observed, _DELETE)
    intent = ctx.ui.confirmation.pending
    assert isinstance(intent, StemDeleteConfirmation)
    assert len(intent.inventory.items) == 216
    assert [item.pad_id for item in intent.inventory.items if item is not None] == [0, 35, 215]
    assert len(intent.inventory.set_identities) == 2
    with pytest.raises(FrozenInstanceError):
        _assign_field(intent.inventory, "items", ())
    controller.project.selected_bank = 5
    controller.project.selected_pad = 215
    controller.session.settings_open = False
    before = controller.project.model_dump(), controller.session.model_dump()
    observed.texts.clear()
    modal_frames._frame(ctx)
    modal_frames._frame(ctx)
    assert observed.texts[0] == "Delete stems for 3 loaded PADs?"
    assert any("all 6 banks (216 slots), 2 unique stem set versions" in t for t in observed.texts)
    assert any("pending stem work is withdrawn" in text for text in observed.texts)
    assert any("Original audio and FullMix sources are kept" in text for text in observed.texts)
    assert any("playback or other readers still need them" in text for text in observed.texts)
    assert set(observed.buttons) >= {"DELETE STEMS", "CANCEL"}
    for label in ("DELETE STEMS", "CANCEL"):
        left, top, right, bottom = observed.buttons[label]
        assert right - left >= observed.text_widths[label] + 2 * imgui.get_style().frame_padding.x
        assert 0 <= left < right <= width
        assert 0 <= top < bottom <= 900
    modal_frames._click(ctx, observed, "CANCEL")
    assert ctx.ui.confirmation.pending is None
    assert (controller.project.model_dump(), controller.session.model_dump()) == before
    start.assert_not_called()
    audio_engine_mock.retire_project_asset.assert_not_called()


def test_accept_keeps_exact_inventory_when_original_source_or_set_changes(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ctx = _context(controller, audio_engine_mock)
    start = Mock(return_value=True)
    monkeypatch.setattr(controller.stem_delete, "start", start)
    ctx.ui.confirmation.request_all_loaded_stem_deletion()
    intent = ctx.ui.confirmation.pending
    assert isinstance(intent, StemDeleteConfirmation)
    controller.project.pad_content[215] = PadContentIdentity(
        instance_id="f" * 32, material_id="e" * 32
    )
    controller.project.stem_cache[35] = _entry(3)
    controller.project.sample_paths[1] = "samples/newly-loaded.wav"
    controller.project.selected_bank = 5
    controller.project.selected_pad = 215
    assert ctx.ui.confirmation.is_current(intent)
    observed = modal_frames._observe(monkeypatch)
    modal_frames._frame(ctx)
    modal_frames._frame(ctx)
    modal_frames._click(ctx, observed, "DELETE STEMS")
    modal_frames._frame(ctx)
    assert ctx.ui.confirmation.pending is None
    start.assert_called_once_with(intent.inventory)
    assert start.call_args.args[0] is intent.inventory
    assert intent.inventory.items[1] is None
    original = intent.inventory.items[215]
    assert original is not None
    assert original.content != controller.project.pad_content[215]
    captured = intent.inventory.items[35]
    assert captured is not None
    assert captured.current_set is not None
    current = controller.project.stem_cache[35]
    assert current is not None
    assert captured.current_set.cache_dir != current.cache_dir
    assert (controller.project.selected_bank, controller.project.selected_pad) == (5, 215)
    audio_engine_mock.retire_project_asset.assert_not_called()


def test_escape_dismisses_deletion_warning_without_start_or_generation_cancel(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, audio_engine_mock)
    start, cancel = Mock(), Mock()
    monkeypatch.setattr(controller.stem_delete, "start", start)
    monkeypatch.setattr(controller.stem_batch, "cancel", cancel)
    ctx.ui.confirmation.request_all_loaded_stem_deletion()
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
    audio_engine_mock.retire_project_asset.assert_not_called()


@dataclass
class _ChildGeometry:
    scrollbar: bool = False
    height: float = 0
    scroll_max: float = 0


def _observe_delete_child(monkeypatch: pytest.MonkeyPatch) -> _ChildGeometry:
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


def _mixed_summary() -> StemDeleteSummary:
    return StemDeleteSummary(
        items=(
            StemDeleteItemStatus(0, "removed"),
            StemDeleteItemStatus(1, "noop", "No captured stems"),
            StemDeleteItemStatus(2, "skipped", "Original target changed"),
            StemDeleteItemStatus(3, "failed", "FullMix unavailable"),
            *(StemDeleteItemStatus(pad_id, "queued") for pad_id in range(4, 216)),
        ),
        total=216,
        queued=212,
        removed=1,
        noop=1,
        skipped=1,
        failed=1,
        active=True,
    )


def test_original_216_results_are_bounded_and_active_operation_guards_both_buttons(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, audio_engine_mock)
    deletion = Mock(summary=_mixed_summary())
    monkeypatch.setattr(controller, "stem_delete", deletion)
    capture = Mock()
    monkeypatch.setattr(controller.stems, "capture_deletion_inventory", capture)
    observed = modal_frames._observe(monkeypatch)
    geometry = _observe_delete_child(monkeypatch)
    controller.project.selected_bank = 5
    controller.project.selected_pad = 215
    before = controller.project.model_dump()
    _settings_frame(ctx)
    _settings_frame(ctx)
    _settings_frame(ctx)
    assert "Deletion settled 4 of 216 loaded PADs." in observed.texts
    assert "Removed 1; Noop 1; Skipped 1; Failed 1; Queued 212." in observed.texts
    assert "PAD 1: Removed" in observed.texts
    assert "PAD 2: Noop - No captured stems" in observed.texts
    assert "PAD 3: Skipped - Original target changed" in observed.texts
    assert "PAD 4: Failed" in observed.texts
    assert not any("FullMix unavailable" in text for text in observed.texts)
    assert "PAD 216: Queued" in observed.texts
    assert any("File cleanup is separate" in text for text in observed.texts)
    assert {
        int(text.split(":", 1)[0].removeprefix("PAD "))
        for text in observed.texts
        if text.startswith("PAD ")
    } == set(range(1, 217))
    assert geometry.scrollbar
    assert geometry.scroll_max > geometry.height
    assert geometry.height <= 6 * imgui.get_text_line_height_with_spacing() + 0.01
    _settings_click(ctx, observed, _DELETE)
    _settings_click(ctx, observed, _GENERATE)
    assert ctx.ui.confirmation.pending is None
    assert "Dismiss deletion results" not in observed.buttons
    capture.assert_not_called()
    deletion.start.assert_not_called()
    deletion.poll.assert_not_called()
    deletion.dismiss_result.assert_not_called()
    assert controller.project.model_dump() == before


def test_terminal_partial_result_can_be_dismissed_while_global_cleanup_stays_visible(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, audio_engine_mock)
    summary = StemDeleteSummary(
        items=(
            StemDeleteItemStatus(0, "removed"),
            StemDeleteItemStatus(215, "failed", "FullMix unavailable"),
        ),
        total=2,
        queued=0,
        removed=1,
        noop=0,
        skipped=0,
        failed=1,
        active=False,
    )
    deletion = Mock(summary=summary)

    def dismiss() -> None:
        deletion.summary = None

    deletion.dismiss_result.side_effect = dismiss
    monkeypatch.setattr(controller, "stem_delete", deletion)
    audio_engine_mock.project_asset_cleanup_status.return_value = (
        2,
        33,
        999999,
        ["A retained stem file could not be removed"],
    )
    controller._assets.retirement_errors[Path("samples/owned-stems")] = "Cleanup queue full"
    observed = modal_frames._observe(monkeypatch)
    _settings_frame(ctx)
    _settings_frame(ctx)
    assert "Deletion settled 2 of 2 loaded PADs." in observed.texts
    assert "PAD 1: Removed" in observed.texts
    assert "PAD 216: Failed" in observed.texts
    assert any("Global file cleanup: Pending 2; Errors 2." in text for text in observed.texts)
    assert any("These counts include other cleanup" in text for text in observed.texts)
    assert "A retained stem file could not be removed" not in observed.texts
    assert "Cleanup queue full" not in observed.texts
    assert not any("FullMix unavailable" in text for text in observed.texts)
    assert not any("999999" in text for text in observed.texts)
    status = ctx.ui.stem_delete.cleanup_status
    assert status.pending_targets == 2
    assert status.retained_native_records == 33
    deletion.dismiss_result.assert_not_called()
    _settings_click(ctx, observed, "Dismiss deletion results")
    deletion.dismiss_result.assert_called_once_with()
    observed.texts.clear()
    _settings_frame(ctx)
    assert not any("Deletion settled" in text for text in observed.texts)
    assert any("Global file cleanup: Pending 2; Errors 2." in text for text in observed.texts)
    assert "Cleanup queue full" not in observed.texts
    audio_engine_mock.retire_project_asset.assert_not_called()


def test_cleanup_query_failure_keeps_unknown_and_control_errors_visible_without_a_batch(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, audio_engine_mock)
    audio_engine_mock.project_asset_cleanup_status.side_effect = RuntimeError("Worker unavailable")
    controller._assets.retirement_errors[Path("samples/owned-stems")] = "Cleanup admission failed"
    observed = modal_frames._observe(monkeypatch)
    _settings_frame(ctx)
    _settings_frame(ctx)
    assert any("Global file cleanup: Pending unknown; Errors 2." in t for t in observed.texts)
    assert "Cleanup status unavailable: Worker unavailable" not in observed.texts
    assert "Cleanup admission failed" not in observed.texts
    assert ctx.ui.stem_delete.summary is None
    audio_engine_mock.retire_project_asset.assert_not_called()
