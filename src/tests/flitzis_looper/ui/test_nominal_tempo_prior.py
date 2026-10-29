"""Prior guidance describes saved current automatic evidence and actual CPU components."""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.analysis.execution import ExecutionReceipt, LegacyCpuExecutionReceipt
from flitzis_looper.bpm_range_options import BpmRangeOptions
from flitzis_looper.models import BeatGrid, SampleAnalysis
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import sidebar_left
from tests.flitzis_looper.controller.test_nominal_tempo_prior import _prior_data

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


@pytest.mark.parametrize("device", ["cpu", "cuda"])
@pytest.mark.parametrize("state", ["automatic", "loading", "analyzing", "manual", "tap"])
def test_prior_labels_and_guidance_require_current_auto_and_keep_errors_separate(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
    device: str,
    state: str,
) -> None:
    data = _prior_data()
    data["execution"] = (
        LegacyCpuExecutionReceipt(1, "cpu", "cpu", None, "float32", "2.8.0+cpu", None, None)
        if device == "cpu"
        else ExecutionReceipt(
            1, "auto", "cuda:0", "Fixture GPU", "float32", "2.8.0+cu128", "12.8", None
        )
    )
    data["nominal_selection"]["range_constraint"] = {
        "minimum_bpm": 50.0,
        "maximum_bpm": 170.0,
    }
    method = AppTempoMethod.model_validate(data, strict=True)
    # Saved guidance must not be reinterpreted using today's range settings.
    controller.project.automatic_bpm_range = BpmRangeOptions(
        enabled=True, minimum_bpm=200, maximum_bpm=250
    )
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=60.0,
        key="Am",
        beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]),
        bpm_method=method,
    )
    if state == "loading":
        controller.session.loading_sample_ids.add(0)
    elif state == "analyzing":
        controller.session.analyzing_sample_ids.add(0)
    elif state in {"manual", "tap"}:
        controller.project.manual_bpm[0] = 94.0
        controller.project.pad_timing_intent[0] = state
    diagnostic = "Fixture independent key analysis diagnostic"
    controller.session.sample_analysis_errors[0] = diagnostic
    ctx = UiContext(controller)
    rendered = []
    monkeypatch.setattr(sidebar_left.imgui, "text_wrapped", rendered.append)
    sidebar_left._render_bpm_evidence(ctx, 0)
    guidance = sidebar_left.bpm_estimate_guidance(method)
    assert "learned beat-level prior" in guidance
    assert "recorded range (50-170 BPM)" in guidance
    assert (guidance in rendered) is (state == "automatic")
    assert diagnostic in rendered
    nn = "CPU" if device == "cpu" else "CUDA 0 · Fixture GPU"
    assert ctx.state.pads.bpm_execution_label(0) == (
        f"NN {nn}; QM CPU; prior CPU" if state == "automatic" else "Device unknown"
    )


def test_prior_guidance_without_captured_range_does_not_invent_bounds() -> None:
    method = AppTempoMethod.model_validate(_prior_data(), strict=True)
    guidance = sidebar_left.bpm_estimate_guidance(method)
    assert "learned beat-level prior" in guidance
    assert "range" not in guidance
    assert "Verify with /2, x2 or Tap BPM" in guidance
