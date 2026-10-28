"""Range settings and recorded automatic guidance use the existing current-intent guard."""

from contextlib import nullcontext
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.analysis.execution import LegacyCpuExecutionReceipt
from flitzis_looper.bpm_range_options import BpmRangeOptions
from flitzis_looper.models import BeatGrid, SampleAnalysis
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import settings, sidebar_left
from tests.flitzis_looper.controller.test_bpm_range import _range_data

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


@pytest.mark.parametrize("refused", [False, True])
def test_range_checkbox_configures_before_flush_and_shows_admission_failure(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    refused: bool,
) -> None:
    ctx = UiContext(controller)
    rendered = []
    audio_engine_mock.reset_mock()
    if refused:
        audio_engine_mock.configure_automatic_bpm_range.side_effect = RuntimeError(
            "Range configuration unavailable"
        )
    monkeypatch.setattr(settings, "item_width", lambda _width: nullcontext())
    monkeypatch.setattr(settings.imgui, "checkbox", lambda _label, _current: (True, True))
    monkeypatch.setattr(
        settings.imgui, "input_double", lambda _label, value, *_args: (False, value)
    )
    monkeypatch.setattr(settings.imgui, "text_wrapped", rendered.append)
    flush = Mock()
    monkeypatch.setattr(ctx.persistence, "flush_if_dirty", flush)
    controller.project.manual_bpm[0] = 94.0
    controller.project.pad_timing_intent[0] = "tap"

    settings._automatic_bpm_range_controls(ctx)

    audio_engine_mock.configure_automatic_bpm_range.assert_called_once()
    assert audio_engine_mock.configure_automatic_bpm_range.call_args.args == (True, 50.0, 170.0)
    assert controller.project.automatic_bpm_range.enabled is not refused
    assert controller.project.manual_bpm[0] == 94.0
    assert controller.project.pad_timing_intent[0] == "tap"
    assert flush.call_count == (0 if refused else 1)
    assert any("future loads" in text and "unchanged" in text for text in rendered)
    if refused:
        assert "Range configuration unavailable" in rendered


def test_invalid_range_editor_retains_saved_bounds_and_reports_error(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx = UiContext(controller)
    rendered = []
    audio_engine_mock.reset_mock()
    monkeypatch.setattr(settings, "item_width", lambda _width: nullcontext())
    monkeypatch.setattr(settings.imgui, "checkbox", lambda _label, value: (False, value))
    monkeypatch.setattr(
        settings.imgui,
        "input_double",
        lambda label, value, *_args: (True, 200.0) if label == "Minimum BPM" else (False, value),
    )
    monkeypatch.setattr(settings.imgui, "text_wrapped", rendered.append)
    flush = Mock()
    monkeypatch.setattr(ctx.persistence, "flush_if_dirty", flush)
    settings._automatic_bpm_range_controls(ctx)
    assert controller.project.automatic_bpm_range == BpmRangeOptions()
    assert controller.session.automatic_bpm_range_settings_error is not None
    assert controller.session.automatic_bpm_range_settings_error in rendered
    audio_engine_mock.configure_automatic_bpm_range.assert_not_called()
    flush.assert_not_called()


@pytest.mark.parametrize("selected", [False, True])
@pytest.mark.parametrize("state", ["automatic", "loading", "analyzing", "manual", "tap"])
def test_range_guidance_and_cpu_support_describe_recorded_current_auto_only(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, state: str, *, selected: bool
) -> None:
    data = _range_data(selected=selected)
    data["execution"] = LegacyCpuExecutionReceipt(
        1, "cpu", "cpu", None, "float32", "2.8.0+cpu", None, None
    )
    method = AppTempoMethod.model_validate(data, strict=True)
    controller.project.automatic_bpm_range = BpmRangeOptions(
        enabled=False, minimum_bpm=200, maximum_bpm=250
    )
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=60.0 if selected else 0.0,
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
    ctx = UiContext(controller)
    diagnostic = "No new automatic BPM within the configured request range"
    controller.session.sample_analysis_errors[0] = diagnostic
    rendered = []
    monkeypatch.setattr(sidebar_left.imgui, "text_wrapped", rendered.append)
    sidebar_left._render_bpm_evidence(ctx, 0)
    assert diagnostic in rendered
    guidance = sidebar_left.bpm_estimate_guidance(method)
    assert "recorded range (50-100 BPM)" in guidance
    assert (guidance in rendered) is (state == "automatic")
    expected = "NN CPU; QM CPU" if selected else "CPU"
    assert ctx.state.pads.bpm_execution_label(0) == (
        expected if state == "automatic" else "Device unknown"
    )
