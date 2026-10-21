"""Actual label/build gestures; Native geometry and custody have separate proof."""

import json
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.models import PadContentIdentity
from flitzis_looper.ui.render import source_beatmap_editor
from tests.flitzis_looper.controller.raw_source_beatmap_fakes import (
    FakeRawCandidate,
    raw_event,
    raw_job,
    review_job,
)
from tests.flitzis_looper.ui.test_source_beatmap_editor import _expose
from tests.flitzis_looper.ui.test_source_beatmap_raw_editor import _assign_raw_ui
from tests.flitzis_looper.ui.test_waveform_render_stability import (
    _click_toolbar,
    _context,
    _full_frame,
    _observe_draw,
    _plot_context,
)

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.ui.context import UiContext


def _ready(ctx: UiContext, audio: Mock) -> tuple[Mock, FakeRawCandidate]:
    raw, candidate = raw_job(audio), FakeRawCandidate()
    ctx.ui.source_map.selection.analyze_original(0, expected_attempt=0)
    ctx.ui.source_map.selection.raw_completed(0, raw_event(raw, candidate))
    raw.source_beatmap_async_status.return_value.update(
        stage="completed", terminal=True, raw_review_retired=False
    )
    review_job(audio)
    return raw, candidate


def test_actual_use_detection_leaves_quarter_blank_then_explicit_nonordinal_labels_build(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = source_beatmap_editor.imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        _expose(ctx, monkeypatch, None)
        _assign_raw_ui(ctx, controller)
        _ready(ctx, audio_engine_mock)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        draft = source_beatmap_editor._RAW_INPUTS[ctx.ui.waveform]
        assert not draft.raw_index
        assert not draft.quarter_coordinate
        assert not draft.labels
        _click_toolbar("Inspect detections##source-raw-inspect", ctx, worker, observer, frames)
        draft.quarter_coordinate = "99"

        _click_toolbar("Use detection #0##source-raw-use-0", ctx, worker, observer, frames)

        assert draft.raw_index == "0"
        assert not draft.quarter_coordinate
        assert not draft.labels
        _click_toolbar(
            "Build source map from labels##source-raw-build", ctx, worker, observer, frames
        )
        audio_engine_mock.begin_source_beatmap_review_async.assert_not_called()
        draft.quarter_coordinate = "0.25"
        _click_toolbar("Add/update label##source-raw-label", ctx, worker, observer, frames)
        draft.raw_index, draft.quarter_coordinate = "2", "3.5"
        _click_toolbar("Add/update label##source-raw-label", ctx, worker, observer, frames)
        assert draft.labels == [(0, 0.25), (2, 3.5)]

        _click_toolbar(
            "Build source map from labels##source-raw-build", ctx, worker, observer, frames
        )

        encoded = audio_engine_mock.begin_source_beatmap_review_async.call_args.args[2]
        assert json.loads(encoded)["anchors"] == [
            {"raw_index": 0, "quarter_bits": "3fd0000000000000"},
            {"raw_index": 2, "quarter_bits": "400c000000000000"},
        ]
        assert controller.project.pad_source_beatmap_intent[0].selected is None
        audio_engine_mock.seek_sample.assert_not_called()


@pytest.mark.parametrize("quarter", ["", "nan", "inf"])
def test_actual_missing_or_nonfinite_label_gesture_never_fills_draft_or_admits(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    quarter: str,
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = source_beatmap_editor.imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        _expose(ctx, monkeypatch, None)
        _assign_raw_ui(ctx, controller)
        _ready(ctx, audio_engine_mock)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        draft = source_beatmap_editor._RAW_INPUTS[ctx.ui.waveform]
        draft.raw_index, draft.quarter_coordinate = "0", quarter

        _click_toolbar("Add/update label##source-raw-label", ctx, worker, observer, frames)

        assert not draft.labels
        assert controller.project.pad_source_beatmap_intent[0] is None
        audio_engine_mock.begin_source_beatmap_review_async.assert_not_called()


def test_actual_label_update_remove_and_limit_remain_explicit_bounded_intent(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = source_beatmap_editor.imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        _expose(ctx, monkeypatch, None)
        _assign_raw_ui(ctx, controller)
        _ready(ctx, audio_engine_mock)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        draft = source_beatmap_editor._RAW_INPUTS[ctx.ui.waveform]
        draft.labels = [(index, float(index)) for index in range(64)]
        draft.raw_index, draft.quarter_coordinate = "65", "65"
        _click_toolbar("Add/update label##source-raw-label", ctx, worker, observer, frames)
        assert len(draft.labels) == 64
        draft.raw_index, draft.quarter_coordinate = "63", "63.5"
        _click_toolbar("Add/update label##source-raw-label", ctx, worker, observer, frames)
        assert draft.labels[-1] == (63, 63.5)
        assert len(draft.labels) == 64

        _click_toolbar("Remove label##source-raw-remove-label", ctx, worker, observer, frames)

        assert len(draft.labels) == 63
        assert not draft.quarter_coordinate
        assert controller.project.pad_source_beatmap_intent[0] is None
        audio_engine_mock.begin_source_beatmap_review_async.assert_not_called()


@pytest.mark.parametrize("index_text", ["9223372036854775808", "-9223372036854775809"])
@pytest.mark.parametrize(
    "action",
    ["Add/update label##source-raw-label", "Remove label##source-raw-remove-label"],
)
def test_actual_huge_index_gestures_refuse_before_native_without_mutating_review(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    index_text: str,
    action: str,
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = source_beatmap_editor.imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        _expose(ctx, monkeypatch, None)
        _assign_raw_ui(ctx, controller)
        candidate = _ready(ctx, audio_engine_mock)[1]
        native_inspect = Mock(wraps=candidate.inspect)
        monkeypatch.setattr(candidate, "inspect", native_inspect)
        reported = Mock()
        monkeypatch.setattr(ctx.ui.source_map, "report_raw_error", reported)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        draft = source_beatmap_editor._RAW_INPUTS[ctx.ui.waveform]
        draft.labels = [(0, 0.25), (4, 1.5)]
        draft.raw_index, draft.quarter_coordinate = index_text, "2.5"
        frames.append(_full_frame(ctx, worker, observer))
        controller._persistence.flush()
        before = (controller.project.model_dump(), controller._persistence.config_path.read_bytes())

        _click_toolbar(action, ctx, worker, observer, frames)

        native_inspect.assert_not_called()
        assert not candidate.queries
        assert draft.labels == [(0, 0.25), (4, 1.5)]
        assert draft.raw_index == index_text
        assert draft.quarter_coordinate == "2.5"
        assert draft.start_index == 0
        assert not draft.inspecting
        assert controller.project.model_dump() == before[0]
        assert controller._persistence.config_path.read_bytes() == before[1]
        reported.assert_called_once_with(0, "Raw event index must satisfy 0 <= index < 260.")
        audio_engine_mock.begin_source_beatmap_review_async.assert_not_called()


def test_actual_build_release_cannot_transfer_draft_to_equal_digest_new_attempt(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = source_beatmap_editor.imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        _expose(ctx, monkeypatch, None)
        _assign_raw_ui(ctx, controller)
        raw, candidate = _ready(ctx, audio_engine_mock)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        draft = source_beatmap_editor._RAW_INPUTS[ctx.ui.waveform]
        draft.labels = [(0, 0.0), (4, 1.5)]
        frames.append(_full_frame(ctx, worker, observer))
        rect = frames[-1]["buttons"]["Build source map from labels##source-raw-build"]
        mouse = ((rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2)
        _full_frame(ctx, worker, observer, mouse=mouse)
        _full_frame(ctx, worker, observer, mouse=mouse, down=True)
        ctx.ui.source_map.selection.discard_raw(
            0, expected_attempt=1, expected_revision=candidate.revision
        )
        raw.source_beatmap_async_status.return_value["raw_review_retired"] = True
        ctx.ui.source_map.selection.poll(frozenset())
        replacement = raw_job(audio_engine_mock)
        ctx.ui.source_map.selection.analyze_original(0, expected_attempt=1)
        ctx.ui.source_map.selection.raw_completed(0, raw_event(replacement, FakeRawCandidate()))

        _full_frame(ctx, worker, observer, mouse=mouse, down=False)

        fresh = source_beatmap_editor._RAW_INPUTS[ctx.ui.waveform]
        assert fresh is not draft
        assert not fresh.labels
        assert not fresh.raw_index
        assert not fresh.quarter_coordinate
        audio_engine_mock.begin_source_beatmap_review_async.assert_not_called()


def test_original_move_keeps_single_draft_while_copy_bank_target_and_new_attempt_do_not_inherit(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = source_beatmap_editor.imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        _expose(ctx, monkeypatch, None)
        _assign_raw_ui(ctx, controller)
        _ready(ctx, audio_engine_mock)
        observer = _observe_draw(monkeypatch)
        _full_frame(ctx, worker, observer)
        draft = source_beatmap_editor._RAW_INPUTS[ctx.ui.waveform]
        draft.labels = [(0, 0.25), (4, 2.5)]
        controller.project.pad_content[2] = controller.project.pad_content[0]
        controller.project.sample_paths[2] = controller.project.sample_paths[0]
        controller.project.pad_content[0] = None
        moved = source_beatmap_editor.capture_source_map(ctx, 2)
        assert moved.raw.status == "unreviewed"
        assert source_beatmap_editor._raw_inputs(ctx, moved) is draft
        controller.project.pad_content[3] = PadContentIdentity(instance_id="c" * 32)
        controller.project.sample_paths[3] = controller.project.sample_paths[2]
        copied = source_beatmap_editor.capture_source_map(ctx, 3)
        assert copied.raw.status == "none"
        controller.project.pad_content[2] = PadContentIdentity(instance_id="b" * 32)
        bank = source_beatmap_editor.capture_source_map(ctx, 2)
        assert bank.raw.status == "none"
        assert draft.labels == [(0, 0.25), (4, 2.5)]
        audio_engine_mock.begin_source_beatmap_review_async.assert_not_called()
