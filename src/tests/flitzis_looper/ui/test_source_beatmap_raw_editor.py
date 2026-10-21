"""Real ImGui mouse gestures over the transient raw review controller boundary."""

from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.models import PadContentIdentity
from flitzis_looper.ui.render import source_beatmap_editor
from tests.flitzis_looper.controller.raw_source_beatmap_fakes import (
    FakeRawCandidate,
    raw_event,
    raw_job,
)
from tests.flitzis_looper.ui.test_source_beatmap_editor import _expose
from tests.flitzis_looper.ui.test_waveform_render_stability import (
    _click_toolbar,
    _context,
    _full_frame,
    _observe_draw,
    _plot_context,
)

if TYPE_CHECKING:
    from imgui_bundle import imgui

    from flitzis_looper.controller import AppController
    from flitzis_looper.ui.context import UiContext


def _assign_raw_ui(ctx: UiContext, controller: AppController) -> None:
    controller.project.pad_content[0] = PadContentIdentity(instance_id="e" * 32)
    ctx.ui.source_map.selection._source_generation = Mock(return_value=71)


def test_actual_analyze_click_uses_original_generation_without_selecting_map_or_audio(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = source_beatmap_editor.imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        _edit, _identity = _expose(ctx, monkeypatch, None)
        _assign_raw_ui(ctx, controller)
        job = raw_job(audio_engine_mock)
        before = controller.project.model_dump()
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]

        _click_toolbar(
            "Analyze original full mix##source-raw-analyze", ctx, worker, observer, frames
        )

        audio_engine_mock.begin_source_beatmap_raw_async.assert_called_once_with(
            0, expected_source_generation=71
        )
        assert ctx.ui.source_map.selection.raw_state(0).status == "queued"
        assert controller.project.model_dump() == before
        assert controller.project.pad_source_beatmap_intent[0] is None
        job.resolve_source_beatmap_raw_completion.assert_not_called()
        audio_engine_mock.seek_sample.assert_not_called()


def test_actual_raw_inspection_and_paging_use_only_bounded_native_event_windows(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = source_beatmap_editor.imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        _edit, _identity = _expose(ctx, monkeypatch, None)
        _assign_raw_ui(ctx, controller)
        maps = ctx.ui.source_map.selection
        job, candidate = raw_job(audio_engine_mock), FakeRawCandidate()
        maps.analyze_original(0, expected_attempt=0)
        maps.raw_completed(0, raw_event(job, candidate))
        before = controller.project.model_dump()
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]

        _click_toolbar("Inspect detections##source-raw-inspect", ctx, worker, observer, frames)
        _click_toolbar("Next detections##source-raw-next", ctx, worker, observer, frames)
        _click_toolbar("Next detections##source-raw-next", ctx, worker, observer, frames)
        _click_toolbar("Previous detections##source-raw-previous", ctx, worker, observer, frames)
        frames.append(_full_frame(ctx, worker, observer))

        assert {(0, 128), (128, 128), (256, 128)} <= set(candidate.queries)
        assert all(count == 128 for _start, count in candidate.queries)
        assert source_beatmap_editor._RAW_INPUTS[ctx.ui.waveform].start_index == 128
        assert controller.project.model_dump() == before
        assert frames[-1]["bands"], (
            "The existing scalar grid remains available beside raw evidence."
        )
        assert "Apply bars##source-map-bars" not in frames[-1]["buttons"]
        audio_engine_mock.seek_sample.assert_not_called()


@pytest.mark.parametrize("action", ["source-raw-discard", "source-raw-next"])
def test_identical_raw_digest_new_attempt_cannot_receive_an_old_mouse_release(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    action: str,
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = source_beatmap_editor.imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        _edit, _identity = _expose(ctx, monkeypatch, None)
        _assign_raw_ui(ctx, controller)
        maps = ctx.ui.source_map.selection
        first, candidate = raw_job(audio_engine_mock), FakeRawCandidate()
        maps.analyze_original(0, expected_attempt=0)
        maps.raw_completed(0, raw_event(first, candidate))
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        if action == "source-raw-next":
            _click_toolbar("Inspect detections##source-raw-inspect", ctx, worker, observer, frames)
        label = (
            "Discard candidate##source-raw-discard"
            if action == "source-raw-discard"
            else "Next detections##source-raw-next"
        )
        rect = frames[-1]["buttons"][label]
        mouse = ((rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2)
        _full_frame(ctx, worker, observer, mouse=mouse)
        _full_frame(ctx, worker, observer, mouse=mouse, down=True)
        old_inputs = source_beatmap_editor._RAW_INPUTS[ctx.ui.waveform]
        maps.discard_raw(0, expected_attempt=1, expected_revision=candidate.revision)
        first.source_beatmap_async_status.return_value.update(
            terminal=True, raw_review_retired=True
        )
        maps.poll(frozenset())
        second, same_bytes = raw_job(audio_engine_mock), FakeRawCandidate()
        maps.analyze_original(0, expected_attempt=1)
        maps.raw_completed(0, raw_event(second, same_bytes))

        _full_frame(ctx, worker, observer, mouse=mouse, down=False)

        assert same_bytes.revision == candidate.revision
        assert maps.raw_state(0).attempt == 2
        assert maps.raw_state(0).status == "unreviewed"
        second.cancel.assert_not_called()
        assert not same_bytes.queries
        assert source_beatmap_editor._RAW_INPUTS[ctx.ui.waveform] is not old_inputs


def test_analyze_mouse_release_does_not_transfer_to_a_replaced_original(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = source_beatmap_editor.imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        _edit, identity = _expose(ctx, monkeypatch, None)
        _assign_raw_ui(ctx, controller)
        raw_job(audio_engine_mock)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        rect = frames[-1]["buttons"]["Analyze original full mix##source-raw-analyze"]
        mouse = ((rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2)
        _full_frame(ctx, worker, observer, mouse=mouse)
        _full_frame(ctx, worker, observer, mouse=mouse, down=True)
        identity.return_value = ("replacement", 5, (9, "b" * 64, 28_800_000, 48_000))

        _full_frame(ctx, worker, observer, mouse=mouse, down=False)

        audio_engine_mock.begin_source_beatmap_raw_async.assert_not_called()


@pytest.mark.parametrize("status", ["pending", "restoring", "settling"])
def test_raw_analyze_is_disabled_during_accepted_map_preparation(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    status: str,
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = source_beatmap_editor.imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        _edit, _identity = _expose(ctx, monkeypatch, None)
        _assign_raw_ui(ctx, controller)
        monkeypatch.setattr(ctx.ui.source_map.selection, "status", lambda _pad: status)
        raw_job(audio_engine_mock)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]

        _click_toolbar(
            "Analyze original full mix##source-raw-analyze", ctx, worker, observer, frames
        )

        audio_engine_mock.begin_source_beatmap_raw_async.assert_not_called()


def test_raw_admission_details_reach_console_facade_without_waveform_error_text(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = source_beatmap_editor.imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        _edit, _identity = _expose(ctx, monkeypatch, None)
        _assign_raw_ui(ctx, controller)
        raw_job(audio_engine_mock)
        detail = "private/source.wav failed with injected descriptor detail"
        audio_engine_mock.begin_source_beatmap_raw_async.side_effect = ValueError(detail)
        reported = Mock()
        monkeypatch.setattr(ctx.ui.source_map, "report_raw_error", reported)
        displayed: list[str] = []
        original_text = source_beatmap_editor.imgui.text_colored

        def text(color: imgui.ImVec4, label: str) -> None:
            displayed.append(label)
            original_text(color, label)

        monkeypatch.setattr(source_beatmap_editor.imgui, "text_colored", text)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]

        _click_toolbar(
            "Analyze original full mix##source-raw-analyze", ctx, worker, observer, frames
        )

        reported.assert_called_with(0, detail)
        assert displayed
        assert all(detail not in label for label in displayed)
        assert ctx.ui.source_map.selection.raw_state(0).status == "none"
