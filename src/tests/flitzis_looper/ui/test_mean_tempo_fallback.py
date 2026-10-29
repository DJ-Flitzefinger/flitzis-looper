"""Approximate BPM warnings describe selected current automatic evidence only."""

from types import SimpleNamespace
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.analysis.execution import ExecutionReceipt, LegacyCpuExecutionReceipt
from flitzis_looper.bpm_range_options import BpmRangeOptions
from flitzis_looper.models import BeatGrid, SampleAnalysis
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import performance_view, sidebar_left
from tests.flitzis_looper.controller.test_mean_tempo_fallback import _mean_data

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


@pytest.mark.parametrize("device", ["cpu", "cuda"])
@pytest.mark.parametrize(
    "state", ["automatic", "loading", "analyzing", "manual", "tap", "new_source"]
)
def test_mean_labels_require_current_auto_and_keep_diagnostics_separate(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, device: str, state: str
) -> None:
    data = _mean_data()
    data["execution"] = (
        LegacyCpuExecutionReceipt(1, "cpu", "cpu", None, "float32", "2.8.0+cpu", None, None)
        if device == "cpu"
        else ExecutionReceipt(
            1, "auto", "cuda:0", "Fixture GPU", "float32", "2.8.0+cu128", "12.8", None
        )
    )
    data["tempo_fallback"]["range_constraint"] = {"minimum_bpm": 90.0, "maximum_bpm": 130.0}
    method = AppTempoMethod.model_validate(data, strict=True)
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=120.0, key="Am", beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]), bpm_method=method
    )
    # Current settings cannot turn historical selected evidence into a new admission.
    controller.project.automatic_bpm_range = BpmRangeOptions(
        enabled=True, minimum_bpm=200.0, maximum_bpm=250.0
    )
    if state == "loading":
        controller.session.loading_sample_ids.add(0)
    elif state == "analyzing":
        controller.session.analyzing_sample_ids.add(0)
    elif state in {"manual", "tap"}:
        controller.project.manual_bpm[0] = 94.0
        controller.project.pad_timing_intent[0] = state
    elif state == "new_source":
        controller.project.sample_analysis[0] = None
    ctx = UiContext(controller)
    rendered: list[str] = []
    controller.session.sample_analysis_errors[0] = "Independent key diagnostic"
    monkeypatch.setattr(sidebar_left.imgui, "text_wrapped", rendered.append)
    sidebar_left._render_bpm_evidence(ctx, 0)
    guidance = sidebar_left.bpm_estimate_guidance(method)
    assert "Possible BPM drift" in guidance
    assert "Mean estimate · middle 60%" in guidance
    assert "playback does not follow tempo changes" in guidance
    assert "recorded range (90-130 BPM)" in guidance
    assert (guidance in rendered) is (state == "automatic")
    assert "Independent key diagnostic" in rendered
    assert ctx.state.pads.bpm_drift_warning(0) == (
        "Possible BPM drift" if state == "automatic" else None
    )
    nn = "CPU" if device == "cpu" else "CUDA 0 · Fixture GPU"
    assert ctx.state.pads.bpm_execution_label(0) == (
        f"NN {nn}; QM CPU" if state == "automatic" else "Device unknown"
    )


@pytest.mark.parametrize("is_active", [False, True])
def test_per_pad_warning_renders_english_and_disappears_with_selected_precise_evidence(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, *, is_active: bool
) -> None:
    method = AppTempoMethod.model_validate(_mean_data(), strict=True)
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=120.0, key="Am", beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]), bpm_method=method
    )
    ctx = UiContext(controller)
    drawn: list[tuple[int, str]] = []
    used_colors: list[object] = []

    def color_u32(color: object) -> int:
        used_colors.append(color)
        return 11 if color == performance_view.TEXT_ACTIVE_RGBA else 22

    monkeypatch.setattr(
        performance_view.imgui, "get_item_rect_min", lambda: SimpleNamespace(x=0.0, y=0.0)
    )
    monkeypatch.setattr(
        performance_view.imgui, "get_item_rect_max", lambda: SimpleNamespace(x=300.0, y=120.0)
    )
    monkeypatch.setattr(
        performance_view.imgui,
        "calc_text_size",
        lambda text: SimpleNamespace(x=len(text) * 7.0, y=14.0),
    )
    monkeypatch.setattr(performance_view.imgui, "get_color_u32", color_u32)
    monkeypatch.setattr(
        performance_view.imgui,
        "get_window_draw_list",
        lambda: SimpleNamespace(add_text=lambda _pos, color, text: drawn.append((color, text))),
    )
    performance_view._pad_button_bpm_warning(ctx, 0, is_active=is_active)
    assert drawn == [(11 if is_active else 22, "Possible BPM drift")]
    assert used_colors == [
        performance_view.TEXT_ACTIVE_RGBA if is_active else performance_view.BANK_PRESSED_RGBA
    ]
    precise = AppTempoMethod.model_validate(_mean_data(precise=True), strict=True)
    controller.project.sample_analysis[0] = controller.project.sample_analysis[0].model_copy(
        update={"bpm_method": precise}
    )
    drawn.clear()
    performance_view._pad_button_bpm_warning(ctx, 0, is_active=is_active)
    assert drawn == []
    assert "Mean" not in sidebar_left.bpm_estimate_guidance(precise)


def test_mean_guidance_does_not_invent_a_request_range() -> None:
    method = AppTempoMethod.model_validate(_mean_data(), strict=True)
    assert "range" not in sidebar_left.bpm_estimate_guidance(method)
