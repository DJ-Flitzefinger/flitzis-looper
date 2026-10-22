"""Real ImGui manual insertion gestures with Native-owned captured-map readback."""

from typing import TYPE_CHECKING, cast

import pytest
from imgui_bundle import imgui

from flitzis_looper.ui.render import source_beatmap_editor, source_beatmap_manual_beat
from tests.flitzis_looper.ui.test_source_beatmap_editor import _expose
from tests.flitzis_looper.ui.test_source_beatmap_review_window import _bits, _captured_map
from tests.flitzis_looper.ui.test_waveform_render_stability import _context, _plot_context

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from flitzis_looper.source_beatmap_manual import ManualBeatMode
    from flitzis_looper.ui.context import UiContext
    from flitzis_looper_audio import SourceBeatmapReviewPage

type _Buttons = dict[str, tuple[float, float, float, float]]
_NATIVE_HEADER = imgui.collapsing_header
_INSERT = "Insert manual beat##source-manual-insert"


def _frame(
    ctx: UiContext,
    *,
    mouse: tuple[float, float] | None = None,
    down: bool | None = None,
    manual_open: bool = True,
    parent_open: bool = True,
    full_editor: bool = False,
    draw: bool = True,
    headers: list[str] | None = None,
) -> _Buttons:
    io = imgui.get_io()
    io.display_size = imgui.ImVec2(1600, 2000)
    if mouse is not None:
        io.add_mouse_pos_event(*mouse)
    if down is not None:
        io.add_mouse_button_event(imgui.MouseButton_.left, down)
    buttons: _Buttons = {}
    original_button = imgui.button

    def observe(label: str, size: imgui.ImVec2Like | None = None) -> bool:
        result = original_button(label, size)
        low, high = imgui.get_item_rect_min(), imgui.get_item_rect_max()
        buttons[label] = low.x, low.y, high.x, high.y
        return result

    def header(label: str) -> bool:
        if headers is not None:
            headers.append(label)
        is_open = manual_open if "source-manual-beat" in label else parent_open
        imgui.set_next_item_open(is_open=is_open, cond=imgui.Cond_.always)
        return _NATIVE_HEADER(label)

    imgui.new_frame()
    imgui.set_next_window_pos((0, 0))
    imgui.set_next_window_size((1500, 1900))
    imgui.begin("productive manual beat gestures")
    with pytest.MonkeyPatch.context() as hooks:
        hooks.setattr(imgui, "button", observe)
        hooks.setattr(imgui, "collapsing_header", header)
        try:
            if draw:
                pad = ctx.state.session.waveform_editor_pad_id
                assert pad is not None
                frame = source_beatmap_editor.capture_source_map(ctx, pad)
                if full_editor:
                    source_beatmap_editor.render_source_map_editor(ctx, frame, None)
                else:
                    source_beatmap_manual_beat.render_manual_beat(ctx, frame)
        finally:
            imgui.end()
            imgui.render()
    return buttons


def _click(ctx: UiContext) -> None:
    rectangle = _frame(ctx)[_INSERT]
    mouse = (rectangle[0] + rectangle[2]) / 2, (rectangle[1] + rectangle[3]) / 2
    _frame(ctx, mouse=mouse)
    _frame(ctx, mouse=mouse, down=True)
    _frame(ctx, mouse=mouse, down=False)


def _ready(
    controller: AppController, audio: Mock, monkeypatch: pytest.MonkeyPatch
) -> tuple[UiContext, Mock, Mock, Mock]:
    ctx, _worker = _context(controller, audio)
    ctx.ui.waveform.open(0)
    geometry = _captured_map(190)
    edit, identity = _expose(ctx, monkeypatch, geometry)
    for _ in range(3):
        _frame(ctx)
    return ctx, geometry, edit, identity


def _draft(ctx: UiContext) -> None:
    state = source_beatmap_manual_beat._INPUTS[ctx.ui.waveform]
    state.sample, state.quarter = "12001", "0.5"


def _change_selection(
    ctx: UiContext, geometry: Mock, identity: Mock, monkeypatch: pytest.MonkeyPatch, changed: str
) -> None:
    if changed == "pad":
        ctx.ui.waveform.open(1)
    elif changed == "source":
        identity.return_value = ("new source", 9)
    elif changed == "map":
        cast("Mock", ctx.ui.source_map.selection.effective).return_value = _captured_map(190, "b")
    elif changed in {"capture", "rate", "extent"}:
        field, value = {
            "capture": ("capture_revision", "e" * 64),
            "rate": ("source_sample_rate_hz", 44_100),
            "extent": ("source_frame_count", 28_799_999),
        }[changed]
        geometry.editor_review_info.return_value[field] = value
    elif changed == "busy":
        monkeypatch.setattr(ctx.ui.source_map.selection, "status", lambda _pad: "pending")
    else:
        pytest.fail(f"Unhandled selection change: {changed}")


def _change_draft(ctx: UiContext, changed: str) -> None:
    state = source_beatmap_manual_beat._INPUTS[ctx.ui.waveform]
    field, value = {
        "left": ("first_anchor", "1"),
        "mode": ("mode", "extend_end"),
        "sample": ("sample", "12002"),
        "quarter": ("quarter", "0.5000000000000001"),
    }[changed]
    setattr(state, field, value)


def _change_endpoint(geometry: Mock) -> None:
    original = geometry.editor_review_page.side_effect

    def moved_endpoint(*args: int) -> SourceBeatmapReviewPage:
        page: SourceBeatmapReviewPage = original(*args)
        page["anchors"][0]["beat_bits"] = _bits(-0.0)
        return page

    geometry.editor_review_page.side_effect = moved_endpoint


def test_actual_manual_interior_uses_exact_sample_and_label_without_interpolation(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch)
        _draft(ctx)
        geometry.evaluate_source.reset_mock()
        _click(ctx)
        assert edit.call_args.args == (
            0,
            {
                "schema_version": 1,
                "operation": "insert_manual_beat",
                "map_revision": "a" * 64,
                "capture_revision": "c" * 64,
                "mode": "interior",
                "first_anchor_index": 0,
                "last_anchor_index": 1,
                "sample_index": 12001,
                "sample_rate_hz": 48_000,
                "quarter_bits": _bits(0.5),
            },
        )
        assert edit.call_args.kwargs == {"expected_revision": "a" * 64}
        geometry.evaluate_source.assert_not_called()
        geometry.record_json.assert_not_called()
        assert all(call.args[-1] == 1 for call in geometry.editor_review_page.call_args_list)
        audio_engine_mock.seek_sample.assert_not_called()


def test_uncaptured_map_has_no_manual_insertion_header_or_redundant_layout(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch)
        _draft(ctx)
        geometry.editor_review_info.return_value.update(
            status="unavailable",
            capture_revision=None,
            reason="This legacy map has no complete captured observations.",
        )
        geometry.editor_review_page.reset_mock()
        headers: list[str] = []
        buttons = _frame(ctx, headers=headers)
        assert "Insert detector-missed beat##source-manual-beat" not in headers
        assert _INSERT not in buttons
        assert ctx.ui.waveform not in source_beatmap_manual_beat._INPUTS
        assert geometry.editor_review_info.return_value["status"] == "unavailable"
        geometry.editor_review_page.assert_not_called()
        edit.assert_not_called()


@pytest.mark.parametrize("mode", ["extend_start", "extend_end"])
def test_outer_modes_use_actual_outer_anchor_without_a_valid_existing_viewport_span(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    mode: str,
) -> None:
    with _plot_context():
        ctx, _geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch)
        _frame(ctx, full_editor=True)
        editor = source_beatmap_editor._INPUTS[ctx.ui.waveform]
        editor.first_anchor, editor.last_anchor = "outside", "accepted coverage"
        state = source_beatmap_manual_beat._INPUTS[ctx.ui.waveform]
        state.mode = cast("ManualBeatMode", mode)
        assert _INSERT in _frame(ctx, full_editor=True)
        state.sample, state.quarter = (
            ("0", "-0.5") if mode == "extend_start" else ("5000000", "190.5")
        )
        _click(ctx)
        operation = edit.call_args.args[1]
        assert operation["mode"] == mode
        endpoint = 0 if mode == "extend_start" else 189
        assert operation["first_anchor_index"] == operation["last_anchor_index"] == endpoint


@pytest.mark.parametrize(
    "changed",
    [
        "pad",
        "source",
        "map",
        "capture",
        "rate",
        "extent",
        "left",
        "mode",
        "sample",
        "quarter",
        "endpoint",
        "busy",
    ],
)
def test_press_release_refuses_every_changed_manual_gesture(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    changed: str,
) -> None:
    with _plot_context():
        ctx, geometry, edit, identity = _ready(controller, audio_engine_mock, monkeypatch)
        _draft(ctx)
        rectangle = _frame(ctx)[_INSERT]
        mouse = (rectangle[0] + rectangle[2]) / 2, (rectangle[1] + rectangle[3]) / 2
        _frame(ctx, mouse=mouse)
        _frame(ctx, mouse=mouse, down=True)
        if changed in {"left", "mode", "sample", "quarter"}:
            _change_draft(ctx, changed)
        elif changed == "endpoint":
            _change_endpoint(geometry)
        else:
            _change_selection(ctx, geometry, identity, monkeypatch, changed)
        _frame(ctx, mouse=mouse, down=False)
        edit.assert_not_called()
        if changed not in {"sample", "quarter"}:
            fresh = source_beatmap_manual_beat._INPUTS[ctx.ui.waveform]
            assert (fresh.sample, fresh.quarter) == ("", "")
        audio_engine_mock.seek_sample.assert_not_called()


@pytest.mark.parametrize("hidden", ["manual", "parent", "undrawn"])
def test_hidden_pad_roundtrip_discards_draft_even_when_original_key_returns(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    hidden: str,
) -> None:
    with _plot_context():
        ctx, _geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch)
        _draft(ctx)
        ctx.ui.waveform.open(1)
        _frame(
            ctx,
            manual_open=hidden != "manual",
            parent_open=hidden != "parent",
            full_editor=hidden == "parent",
            draw=hidden != "undrawn",
        )
        ctx.ui.waveform.open(0)
        _frame(ctx)
        state = source_beatmap_manual_beat._INPUTS[ctx.ui.waveform]
        assert (state.sample, state.quarter) == ("", "")
        _click(ctx)
        edit.assert_not_called()


def test_pending_failure_roundtrip_under_collapsed_parent_discards_old_draft(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, _geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch)
        _draft(ctx)
        status = ["pending"]
        monkeypatch.setattr(ctx.ui.source_map.selection, "status", lambda _pad: status[0])
        _frame(ctx, full_editor=True, parent_open=False)
        status[0] = "failed"
        _frame(ctx, full_editor=True, parent_open=False)
        status[0] = "effective"
        _frame(ctx)
        state = source_beatmap_manual_beat._INPUTS[ctx.ui.waveform]
        assert (state.sample, state.quarter) == ("", "")
        _click(ctx)
        edit.assert_not_called()


@pytest.mark.parametrize("invalid", ["index", "page"])
def test_unavailable_endpoint_roundtrip_cannot_restore_an_old_draft(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    invalid: str,
) -> None:
    with _plot_context():
        ctx, geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch)
        _draft(ctx)
        state = source_beatmap_manual_beat._INPUTS[ctx.ui.waveform]
        original = geometry.editor_review_page.side_effect
        if invalid == "index":
            state.first_anchor = "not an index"
        else:
            geometry.editor_review_page.side_effect = ValueError("endpoints unavailable")
        assert _INSERT not in _frame(ctx)
        geometry.editor_review_page.side_effect = original
        _frame(ctx)
        fresh = source_beatmap_manual_beat._INPUTS[ctx.ui.waveform]
        assert fresh is not state
        assert (fresh.sample, fresh.quarter) == ("", "")
        _click(ctx)
        edit.assert_not_called()


@pytest.mark.parametrize("sample", ["-1", "28800000", "18446744073709551616", "1.5"])
def test_invalid_physical_sample_never_admits(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    sample: str,
) -> None:
    with _plot_context():
        ctx, _geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch)
        _draft(ctx)
        source_beatmap_manual_beat._INPUTS[ctx.ui.waveform].sample = sample
        _click(ctx)
        edit.assert_not_called()
        assert cast("Mock", ctx.ui.source_map.report_error).call_args.args[1]


def test_native_refusal_retains_manual_draft_project_and_markers(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, _geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch)
        _draft(ctx)
        before = controller.project.model_dump()
        edit.side_effect = ValueError("manual beat conflicts with a corrected anchor")
        _click(ctx)
        state = source_beatmap_manual_beat._INPUTS[ctx.ui.waveform]
        assert (state.sample, state.quarter) == ("12001", "0.5")
        assert controller.project.model_dump() == before
        assert "corrected anchor" in cast("Mock", ctx.ui.source_map.report_error).call_args.args[1]


@pytest.mark.parametrize("quarter", ["", "nan", "inf", "-inf"])
def test_invalid_quarter_text_cannot_admit_or_mutate_the_project(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    quarter: str,
) -> None:
    with _plot_context():
        ctx, _geometry, edit, _identity = _ready(controller, audio_engine_mock, monkeypatch)
        _draft(ctx)
        source_beatmap_manual_beat._INPUTS[ctx.ui.waveform].quarter = quarter
        before = controller.project.model_dump()
        _click(ctx)
        edit.assert_not_called()
        assert controller.project.model_dump() == before
        assert cast("Mock", ctx.ui.source_map.report_error).call_args.args[1]
