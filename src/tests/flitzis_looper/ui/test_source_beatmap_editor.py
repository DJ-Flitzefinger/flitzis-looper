"""Actual Loop Editor gestures over the qualified map/controller boundary."""

import math
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from imgui_bundle import imgui

from flitzis_looper.controller.current_timing import CurrentPadTiming
from flitzis_looper.models import PadContentIdentity
from flitzis_looper.ui.render import source_beatmap_editor, waveform_editor
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
    from flitzis_looper_audio import (
        SourceBeatmapEditorSpan,
        SourceBeatmapEditorSpanAnchor,
        SourceBeatmapEvaluation,
    )


def _evaluate(source: float) -> SourceBeatmapEvaluation:
    """Supply the complete Native DTO for a two-segment 120/110 BPM UI fixture."""
    beat = source * 2.0 if source < 0.5 else 1.0 + (source - 0.5) * 11.0 / 6.0
    return {
        "map_revision": "a" * 64,
        "source_seconds": source,
        "beat": beat,
        "local_bpm": 120.0 if source < 0.5 else 110.0,
        "coordinate_status": "anchor" if source in {0.0, 0.5, 600.0} else "supported",
        "interval_status": "trusted",
        "segment_index": 0 if source < 0.5 else 1,
        "physical_domain": "exclusive_source_end" if source == 600.0 else "within_source",
        "meter_status": "quarter_note_4_4_verified",
        "downbeat_label": 0,
    }


def _source_at_beat(beat: float) -> float:
    return beat / 2.0 if beat < 1.0 else 0.5 + (beat - 1.0) * 6.0 / 11.0


def _source_after_beats(
    source: float, beats: float, *, require_four_four: bool = True
) -> SourceBeatmapEvaluation:
    current = _evaluate(source)["beat"]
    assert current is not None
    result = _evaluate(_source_at_beat(current + beats))
    if require_four_four:
        assert result["meter_status"] == "quarter_note_4_4_verified"
    return result


def _nearest_grid(source: float, step_beats: float = 0.0625) -> SourceBeatmapEvaluation:
    beat = _evaluate(source)["beat"]
    assert beat is not None
    return _evaluate(_source_at_beat(math.floor(beat / step_beats + 0.5) * step_beats))


def _span_anchor(index: int) -> SourceBeatmapEditorSpanAnchor:
    source = (0.0, 0.5, 600.0)[index]
    beat = _evaluate(source)["beat"]
    assert beat is not None
    return {
        "index": index,
        "beat": beat,
        "source_seconds": source,
        "sample_index": None if source == 600.0 else round(source * 48_000),
    }


def _span(first: int, last: int) -> SourceBeatmapEditorSpan:
    left, right = _span_anchor(first), _span_anchor(last)
    return {
        "map_revision": "a" * 64,
        "status": "complete",
        "first_anchor": left,
        "last_anchor": right,
        "quarter_notes": right["beat"] - left["beat"],
        "interval_count": last - first,
        "coverage": "trusted",
        "meter_status": "quarter_note_4_4_verified",
        "downbeat_status": "known",
        "downbeat_label": 0,
    }


def _map() -> Mock:
    geometry = Mock()
    geometry.revision.return_value = "a" * 64
    geometry.editor_info.return_value = {
        "map_revision": "a" * 64,
        "sample_rate_hz": 48_000,
        "source_duration_seconds": 600.0,
        "anchor_count": 3,
        "can_undo": True,
    }
    geometry.evaluate_source.side_effect = _evaluate
    geometry.local_tempo_at_source.side_effect = _evaluate
    geometry.source_after_beats.side_effect = _source_after_beats
    geometry.nearest_grid_source.side_effect = _nearest_grid
    geometry.editor_span.side_effect = _span
    geometry.editor_view.return_value = {
        "map_revision": "a" * 64,
        "status": "complete",
        "step_beats": 1.0,
        "lines": [
            {
                "source_seconds": 0.5,
                "loop_beat": 2.0,
                "major": True,
                "reference": False,
                "meter_status": "quarter_note_4_4_verified",
                "downbeat": False,
            }
        ],
        "anchors": [],
    }
    return geometry


def _expose(
    ctx: UiContext,
    monkeypatch: pytest.MonkeyPatch,
    geometry: Mock | None,
) -> tuple[Mock, Mock]:
    maps = ctx.ui.source_map.selection
    monkeypatch.setattr(maps, "effective", Mock(return_value=geometry))
    monkeypatch.setattr(maps, "is_selected", lambda _pad: geometry is not None)
    monkeypatch.setattr(maps, "status", lambda _pad: "effective" if geometry else "none")
    identity = Mock(return_value=("content", 3, (7, "a" * 64, 28_800_000, 48_000)))
    monkeypatch.setattr(ctx.ui.source_map, "identity", identity)
    monkeypatch.setattr(ctx.ui.source_map, "report_error", Mock())
    edit = Mock()
    monkeypatch.setattr(maps, "edit", edit)
    original_header = imgui.collapsing_header

    def expanded(label: str) -> bool:
        imgui.set_next_item_open(is_open=True, cond=imgui.Cond_.always)
        return original_header(label)

    monkeypatch.setattr(imgui, "collapsing_header", expanded)
    return edit, identity


def _observe_status_windows(monkeypatch: pytest.MonkeyPatch) -> list[tuple[int, int, float, float]]:
    """Inspect real Native child identity and layout, rather than text-call hooks."""
    children: list[tuple[int, int, float, float]] = []
    original = imgui.begin_child

    def begin(label: str, *args: object, **kwargs: object) -> bool:
        visible = original(label, *args, **kwargs)
        if label == "source-map-status":
            window = imgui.internal.get_current_window()
            assert window.parent_window is not None
            children.append((window.id_, window.parent_window.id_, window.pos.y, window.size.y))
        return visible

    monkeypatch.setattr(imgui, "begin_child", begin)
    return children


def test_full_editor_manual_creation_captures_source_scalar_intent(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        edit, _identity = _expose(ctx, monkeypatch, None)
        create = Mock()
        monkeypatch.setattr(ctx.ui.source_map.selection, "create_manual", create)
        before = controller.project.pad_loop_start_s[0], controller.project.pad_loop_end_s[0]
        timing = controller.transport.bpm.current_timing(0)
        assert timing is not None
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]

        _click_toolbar("Create manual 4/4 map##source-map-create", ctx, worker, observer, frames)

        create.assert_called_once_with(0, timing.origin_seconds, timing.period_seconds)
        edit.assert_not_called()
        assert before == (
            controller.project.pad_loop_start_s[0],
            controller.project.pad_loop_end_s[0],
        )
        audio_engine_mock.seek_sample.assert_not_called()


def test_actual_sync_click_changes_request_and_keeps_readiness_and_refusal_truthful(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        controller.project.pad_content[0] = PadContentIdentity(instance_id="e" * 32)
        ctx.ui.waveform.open(0)
        _edit, _identity = _expose(ctx, monkeypatch, _map())
        binding = audio_engine_mock.current_input_runtime_pad_binding(0)
        binding._metadata["window_revision"] = 0
        audio_engine_mock.current_input_runtime_pad_binding.side_effect = None
        audio_engine_mock.current_input_runtime_pad_binding.return_value = binding
        # A real UI/controller test with a callback boundary double; map readiness
        # cannot manufacture audio adoption from this explicit stopped observation.
        audio_engine_mock.pad_source_sync_status = Mock(
            return_value={
                "source_generation": 1,
                "source_identity": 1234,
                "window_revision": 0,
                "active": False,
                "effective": False,
                "intent_revision": None,
                "map_revision": None,
            }
        )
        original_status, original_wrapped = source_beatmap_editor._status_line, imgui.text_wrapped
        rendered: list[str] = []

        def status(label: str) -> None:
            rendered.append(label)
            original_status(label)

        def wrapped(label: str) -> None:
            rendered.append(label)
            original_wrapped(label)

        monkeypatch.setattr(source_beatmap_editor, "_status_line", status)
        monkeypatch.setattr(imgui, "text_wrapped", wrapped)
        children = _observe_status_windows(monkeypatch)
        controls = (
            controller.project.bpm_lock,
            controller.project.key_lock,
            controller.project.multi_loop,
            controller.project.pad_loop_start_s[0],
            controller.project.pad_loop_end_s[0],
        )
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]

        _click_toolbar("SYNC PAD OFF##source-sync-toggle", ctx, worker, observer, frames)
        frames.append(_full_frame(ctx, worker, observer))

        assert controller.project.pad_source_sync_intent[0].requested
        assert controller.project.pad_source_sync_intent[0].revision == 1
        assert ctx.audio.pads.source_sync_status(0).state == "stopped"
        assert any("SYNC requested ON | Audio OFF | stopped" in label for label in rendered)
        technical = "mapped Native h/r tuple unavailable: diagnostic ID42"
        controller.transport.residency.report_launch_error(0, technical)
        frames.append(_full_frame(ctx, worker, observer))
        assert any("SYNC requested ON | Audio OFF | refused" in label for label in rendered)
        assert all(technical not in label and "ID42" not in label for label in rendered)
        assert technical in capsys.readouterr().err
        assert len(children) >= 10
        for sync, source_map in zip(children[::2], children[1::2], strict=True):
            assert sync[0] != source_map[0]
            assert sync[1] == source_map[1]
            assert sync[3] == source_map[3]
            assert source_map[2] >= sync[2] + sync[3]
        assert controls == (
            controller.project.bpm_lock,
            controller.project.key_lock,
            controller.project.multi_loop,
            controller.project.pad_loop_start_s[0],
            controller.project.pad_loop_end_s[0],
        )
        audio_engine_mock.seek_sample.assert_not_called()


def test_full_editor_moves_one_sample_translates_and_undoes_with_same_revision(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        geometry = _map()
        edit, _identity = _expose(ctx, monkeypatch, geometry)
        observer = _observe_draw(monkeypatch)
        before = controller.project.model_dump()
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        state = source_beatmap_editor._INPUTS[ctx.ui.waveform]
        state.beat, state.sample, state.translation = "1", "24001", "-1"

        _click_toolbar("Move beat##source-map-move", ctx, worker, observer, frames)
        _click_toolbar("Translate map##source-map-translate", ctx, worker, observer, frames)
        _click_toolbar("Undo map edit##source-map-undo", ctx, worker, observer, frames)

        assert [call.args[1] for call in edit.call_args_list] == [
            {
                "schema_version": 1,
                "operation": "move_beat",
                "beat": 1.0,
                "sample_index": 24001,
                "sample_rate_hz": 48000,
            },
            {
                "schema_version": 1,
                "operation": "translate",
                "sample_offset": -1,
                "sample_rate_hz": 48000,
            },
            {"schema_version": 1, "operation": "undo"},
        ]
        assert all(call.kwargs == {"expected_revision": "a" * 64} for call in edit.call_args_list)
        assert all(call.args[0] == 0 for call in edit.call_args_list)
        assert controller.project.model_dump() == before
        assert frames[-1]["bands"], "Native variable coordinates reach the actual grid renderer"
        geometry.editor_view.assert_called()
        geometry.record_json.assert_not_called()
        audio_engine_mock.seek_sample.assert_not_called()


def test_source_replacement_between_press_and_release_cannot_edit_new_content(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        edit, identity = _expose(ctx, monkeypatch, _map())
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        old_inputs = source_beatmap_editor._INPUTS[ctx.ui.waveform]
        old_inputs.sample = "24001"
        rect = frames[-1]["buttons"]["Move beat##source-map-move"]
        mouse = ((rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2)
        _full_frame(ctx, worker, observer, mouse=mouse)
        _full_frame(ctx, worker, observer, mouse=mouse, down=True)
        identity.return_value = ("replacement", 5, (9, "b" * 64, 28_800_000, 48_000))

        _full_frame(ctx, worker, observer, mouse=mouse, down=False)

        edit.assert_not_called()
        assert source_beatmap_editor._INPUTS[ctx.ui.waveform] is not old_inputs
        assert source_beatmap_editor._INPUTS[ctx.ui.waveform].sample != "24001"


@pytest.mark.parametrize(
    ("label", "method"),
    [
        ("Apply bars##source-map-bars", "apply_source_map_bars"),
        ("Snap markers##source-map-snap", "snap_source_map_markers"),
    ],
)
@pytest.mark.parametrize("changed", ["source", "revision"])
def test_map_toolbar_release_cannot_transfer_to_replacement_target(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    label: str,
    method: str,
    changed: str,
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        geometry = _map()
        _edit, identity = _expose(ctx, monkeypatch, geometry)
        action = Mock()
        monkeypatch.setattr(ctx.audio.pads, method, action)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        rect = frames[-1]["buttons"][label]
        mouse = ((rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2)
        _full_frame(ctx, worker, observer, mouse=mouse)
        _full_frame(ctx, worker, observer, mouse=mouse, down=True)
        if changed == "source":
            identity.return_value = ("replacement", 6, (9, "b" * 64, 28_800_000, 48_000))
        else:
            geometry.revision.return_value = "b" * 64
        _full_frame(ctx, worker, observer, mouse=mouse, down=False)
        action.assert_not_called()


def test_selected_unavailable_map_never_draws_a_scalar_substitute(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, Mock())[0]
    _expose(ctx, monkeypatch, None)
    monkeypatch.setattr(ctx.ui.source_map.selection, "is_selected", lambda _pad: True)
    monkeypatch.setattr(ctx.ui.source_map.selection, "status", lambda _pad: "restoring")
    scalar = Mock(side_effect=AssertionError("selected map must not fall back to scalar"))
    monkeypatch.setattr(waveform_editor, "visible_grid_lines", scalar)

    waveform_editor._plot_musical_grid(
        ctx,
        0,
        Mock(),
        0.0,
        4.0,
        timing=CurrentPadTiming(period_seconds=0.5, origin_seconds=0.0, sample_rate_hz=48000),
    )

    scalar.assert_not_called()


@pytest.mark.parametrize(("position", "expected_bpm"), [(0.5, 110), (1.5, 110), (0.25, 120)])
def test_local_source_tempo_uses_retained_playhead_after_seek_or_wrap(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    position: float,
    expected_bpm: int,
) -> None:
    ctx = _context(controller, Mock())[0]
    geometry = _map()
    _expose(ctx, monkeypatch, geometry)
    controller.session.pad_playhead_s[0] = position
    frame = source_beatmap_editor.capture_source_map(ctx, 0)

    assert source_beatmap_editor._local_tempo_label(ctx, frame) == f"Source BPM {expected_bpm:.3f}"
    geometry.local_tempo_at_source.assert_called_once_with(position)
    assert controller.session.active_sample_ids == set()


def test_failed_or_stale_edit_reports_error_without_changing_selection(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = _context(controller, Mock())[0]
    _edit, identity = _expose(ctx, monkeypatch, _map())
    frame = source_beatmap_editor.capture_source_map(ctx, 0)
    state = source_beatmap_editor._inputs(ctx, frame)
    failure = Mock(side_effect=ValueError("monotone anchors required"))

    source_beatmap_editor._run(ctx, frame, state, failure)
    assert state.error == "monotone anchors required"
    identity.return_value = ("replacement", 6)
    action = Mock()
    source_beatmap_editor._run(ctx, frame, state, action)

    action.assert_not_called()
    assert state.error is not None
    assert "source changed" in state.error


def test_index_navigation_and_playhead_select_an_unsupported_interval(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        geometry = _map()
        edit, _identity = _expose(ctx, monkeypatch, geometry)
        gap = _evaluate(1.0)
        gap.update(
            beat=None,
            local_bpm=None,
            coordinate_status="gap",
            interval_status="gap_unknown_count",
        )
        geometry.evaluate_source.return_value = gap
        geometry.evaluate_source.side_effect = None
        geometry.editor_view.return_value.update(status="unsupported_reference", lines=[])

        def gap_span(first: int, last: int) -> SourceBeatmapEditorSpan:
            span = _span(first, last)
            if first == 1:
                span["coverage"] = "gap_unknown_count"
            return span

        geometry.editor_span.side_effect = gap_span
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        state = source_beatmap_editor._INPUTS[ctx.ui.waveform]

        _click_toolbar("Next span##source-map-next", ctx, worker, observer, frames)
        assert (state.first_anchor, state.last_anchor) == ("1", "2")
        _click_toolbar("Previous span##source-map-previous", ctx, worker, observer, frames)
        assert (state.first_anchor, state.last_anchor) == ("0", "1")
        _click_toolbar(
            "Select interval at playhead##source-map-select-playhead", ctx, worker, observer, frames
        )

        assert (state.first_anchor, state.last_anchor) == ("1", "2")
        geometry.editor_span.assert_called_with(1, 2)
        assert state.inspected_span == (1, 2)
        assert state.coverage == "gap_unknown_count"
        edit.assert_not_called()
        geometry.record_json.assert_not_called()
        audio_engine_mock.seek_sample.assert_not_called()


@pytest.mark.parametrize(
    ("field", "value", "button", "operation", "expected"),
    [
        ("quarter_notes", "2.5", "count", "set_count", {"quarter_notes": 2.5}),
        ("meter_status", "unknown", "meter", "set_meter", {"meter_status": "unknown"}),
        (
            "meter_status",
            "quarter_note_4_4_verified",
            "meter",
            "set_meter",
            {"meter_status": "quarter_note_4_4_verified"},
        ),
        (
            "downbeat_label",
            "-9223372036854775808",
            "downbeat",
            "set_downbeat",
            {"downbeat_label": -(1 << 63)},
        ),
        ("downbeat_label", "", "downbeat", "set_downbeat", {"downbeat_label": None}),
        (
            "coverage",
            "trusted",
            "coverage",
            "set_coverage",
            {"coverage": "trusted", "reviewed_quarter_notes": 1.0},
        ),
        (
            "coverage",
            "uncertain",
            "coverage",
            "set_coverage",
            {"coverage": "uncertain", "reviewed_quarter_notes": 1.0},
        ),
        (
            "coverage",
            "gap_unknown_count",
            "coverage",
            "set_coverage",
            {"coverage": "gap_unknown_count", "reviewed_quarter_notes": None},
        ),
    ],
)
def test_distinct_interval_buttons_send_only_their_explicit_intent(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    field: str,
    value: str,
    button: str,
    operation: str,
    expected: dict[str, object],
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        geometry = _map()
        edit, _identity = _expose(ctx, monkeypatch, geometry)
        before = controller.project.model_dump()
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        state = source_beatmap_editor._INPUTS[ctx.ui.waveform]
        state.quarter_notes = "123.5"
        setattr(state, field, value)
        suffix = operation.removeprefix("set_")

        _click_toolbar(f"Set {button}##source-map-set-{suffix}", ctx, worker, observer, frames)

        edit.assert_called_once_with(
            0,
            {
                "schema_version": 1,
                "operation": operation,
                "first_anchor_index": 0,
                "last_anchor_index": 1,
                **expected,
            },
            expected_revision="a" * 64,
        )
        assert controller.project.model_dump() == before
        geometry.record_json.assert_not_called()
        audio_engine_mock.seek_sample.assert_not_called()


@pytest.mark.parametrize(
    ("field", "value", "button"),
    [
        ("quarter_notes", "nan", "Set count##source-map-set-count"),
        ("quarter_notes", "inf", "Set count##source-map-set-count"),
        ("quarter_notes", "0", "Set count##source-map-set-count"),
        ("quarter_notes", "-1", "Set count##source-map-set-count"),
        ("downbeat_label", "1.5", "Set downbeat##source-map-set-downbeat"),
        ("downbeat_label", "9223372036854775808", "Set downbeat##source-map-set-downbeat"),
        ("meter_status", "3/4", "Set meter##source-map-set-meter"),
        ("first_anchor", "1.5", "Previous span##source-map-previous"),
        ("first_anchor", "-1", "Previous span##source-map-previous"),
        ("last_anchor", "3", "Previous span##source-map-previous"),
        ("last_anchor", "0", "Previous span##source-map-previous"),
    ],
)
def test_invalid_explicit_inputs_refuse_at_actual_buttons(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    field: str,
    value: str,
    button: str,
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        edit, _identity = _expose(ctx, monkeypatch, _map())
        reported = Mock()
        monkeypatch.setattr(ctx.ui.source_map, "report_error", reported)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        state = source_beatmap_editor._INPUTS[ctx.ui.waveform]
        setattr(state, field, value)
        frames.append(_full_frame(ctx, worker, observer))

        _click_toolbar(button, ctx, worker, observer, frames)

        edit.assert_not_called()
        assert state.error
        reported.assert_called_with(0, state.error)


@pytest.mark.parametrize("label", ["1", ""])
def test_downbeat_does_not_implicitly_enable_unknown_meter(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    label: str,
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        geometry = _map()
        span = _span(0, 1)
        span.update(meter_status="unknown", downbeat_status="unknown", downbeat_label=None)
        geometry.editor_span.side_effect = None
        geometry.editor_span.return_value = span
        edit, _identity = _expose(ctx, monkeypatch, geometry)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        state = source_beatmap_editor._INPUTS[ctx.ui.waveform]
        state.downbeat_label = label

        _click_toolbar("Set downbeat##source-map-set-downbeat", ctx, worker, observer, frames)

        if label:
            edit.assert_not_called()
            assert state.error
        else:
            assert edit.call_args.args[1]["operation"] == "set_downbeat"
            assert edit.call_args.args[1]["downbeat_label"] is None


@pytest.mark.parametrize("changed", ["range", "source", "revision"])
@pytest.mark.parametrize("operation", ["count", "meter", "downbeat", "coverage"])
def test_interval_release_never_transfers_to_another_target(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    changed: str,
    operation: str,
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        geometry = _map()
        edit, identity = _expose(ctx, monkeypatch, geometry)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]
        rect = frames[-1]["buttons"][f"Set {operation}##source-map-set-{operation}"]
        mouse = ((rect[0] + rect[2]) / 2, (rect[1] + rect[3]) / 2)
        _full_frame(ctx, worker, observer, mouse=mouse)
        _full_frame(ctx, worker, observer, mouse=mouse, down=True)
        if changed == "range":
            state = source_beatmap_editor._INPUTS[ctx.ui.waveform]
            state.first_anchor, state.last_anchor = "1", "2"
        elif changed == "source":
            identity.return_value = ("replacement", 5, (9, "b" * 64, 28_800_000, 48_000))
        else:
            geometry.revision.return_value = "b" * 64

        _full_frame(ctx, worker, observer, mouse=mouse, down=False)

        edit.assert_not_called()


@pytest.mark.parametrize("status", ["pending", "restoring", "settling"])
def test_busy_map_disables_interval_mutations(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    status: str,
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        edit, _identity = _expose(ctx, monkeypatch, _map())
        monkeypatch.setattr(ctx.ui.source_map.selection, "status", lambda _pad: status)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]

        _click_toolbar("Set count##source-map-set-count", ctx, worker, observer, frames)

        edit.assert_not_called()


def test_detailed_failure_reaches_console_facade_without_waveform_text_or_tooltip(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    with _plot_context():
        ctx, worker = _context(controller, audio_engine_mock)
        ctx.bold_font = imgui.get_io().fonts.fonts[1]
        ctx.ui.waveform.open(0)
        edit, _identity = _expose(ctx, monkeypatch, _map())
        detail = "private-path/secret.wav failed with internal source descriptor"
        edit.side_effect = ValueError(detail)
        reported = Mock()
        monkeypatch.setattr(ctx.ui.source_map, "report_error", reported)
        text: list[str] = []
        original = source_beatmap_editor._status_line

        def status(label: str) -> None:
            text.append(label)
            original(label)

        monkeypatch.setattr(source_beatmap_editor, "_status_line", status)
        tooltip = Mock()
        monkeypatch.setattr(imgui, "set_tooltip", tooltip)
        observer = _observe_draw(monkeypatch)
        frames = [_full_frame(ctx, worker, observer) for _ in range(5)]

        _click_toolbar("Set count##source-map-set-count", ctx, worker, observer, frames)
        frames.append(_full_frame(ctx, worker, observer))

        reported.assert_called_with(0, detail)
        assert text
        assert all(detail not in label and "secret.wav" not in label for label in text)
        assert all(detail not in str(call) for call in tooltip.call_args_list)


def test_native_downbeat_flag_adds_a_stronger_line_without_changing_quarter_emphasis(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with _plot_context():
        lines = _map().editor_view.return_value["lines"]
        lines[0]["downbeat"] = True
        lines.append({**lines[0], "source_seconds": 1.0, "downbeat": False})
        draw = Mock()
        monkeypatch.setattr(waveform_editor.implot, "plot_to_pixels", lambda x, y: (x, y))

        waveform_editor._draw_source_map_downbeats(draw, lines)

        assert [line["major"] for line in lines] == [True, True]
        draw.add_line.assert_called_once()
        assert draw.add_line.call_args.args[:2] == ((0.5, 1.0), (0.5, -1.0))
        assert draw.add_line.call_args.kwargs == {"thickness": 2.0}
