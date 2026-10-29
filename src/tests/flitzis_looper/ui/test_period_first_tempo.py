"""Current automatic period guidance never changes manual controls or source authority."""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import app_tempo_periods
from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.analysis.execution import ExecutionReceipt, LegacyCpuExecutionReceipt
from flitzis_looper.bpm_range_options import BpmRangeOptions
from flitzis_looper.models import BeatGrid, SampleAnalysis
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import sidebar_left
from tests.flitzis_looper.controller.test_period_first_tempo import _perfect_data

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


@pytest.mark.parametrize("device", ["cpu", "cuda"])
@pytest.mark.parametrize("state", ["automatic", "loading", "analyzing", "manual", "tap"])
def test_period_unit_note_and_device_use_current_auto_without_rendering_raw_scans(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, device: str, state: str
) -> None:
    data = _perfect_data()
    data["execution"] = (
        LegacyCpuExecutionReceipt(1, "cpu", "cpu", None, "float32", "2.8.0+cpu", None, None)
        if device == "cpu"
        else ExecutionReceipt(
            1, "auto", "cuda:0", "Fixture GPU", "float32", "2.8.0+cu128", "12.8", None
        )
    )
    method = AppTempoMethod.model_validate(data, strict=True)
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=120.0, key="Am", beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]), bpm_method=method
    )
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

    def forbidden(*_args):
        message = "Rendering attempted to replay raw period evidence"
        raise AssertionError(message)

    monkeypatch.setattr(app_tempo_periods, "_assign", forbidden)
    monkeypatch.setattr(app_tempo_periods, "_bounded_fit", forbidden)
    ctx = UiContext(controller)
    rendered = []
    monkeypatch.setattr(sidebar_left.imgui, "text_wrapped", rendered.append)
    diagnostic = "Fixture independent key diagnostic"
    controller.session.sample_analysis_errors[0] = diagnostic
    sidebar_left._render_bpm_evidence(ctx, 0)
    guidance = sidebar_left.bpm_estimate_guidance(method)
    assert "Beat level may be half or double" in guidance
    assert "BPM /2 or x2" in guidance
    assert "200" not in guidance
    assert (guidance in rendered) is (state == "automatic")
    assert diagnostic in rendered
    label = "CPU" if device == "cpu" else "CUDA 0 · Fixture GPU"
    assert ctx.state.pads.bpm_execution_label(0) == (
        label if state == "automatic" else "Device unknown"
    )
    assert controller.project.sample_analysis[0].key == "Am"
