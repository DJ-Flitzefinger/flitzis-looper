"""Device labels describe recorded inference, separately from live audio controls."""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
from flitzis_looper.analysis.execution import ExecutionReceipt, LegacyCpuExecutionReceipt
from flitzis_looper.models import BeatGrid, SampleAnalysis, StemCacheEntry
from flitzis_looper.stem_execution import StemExecution
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.controller.test_adaptive_tempo_probe import _v4_method
from tests.flitzis_looper.controller.test_corroborated_tempo import _v6_method
from tests.flitzis_looper.controller.test_inferred_tempo_probe import _v5_method

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def test_bpm_label_is_unknown_for_old_or_pending_analysis_and_independent_of_manual_bpm(
    controller: AppController,
) -> None:
    context = UiContext(controller)
    method = _v4_method(short=True)
    analysis = SampleAnalysis(
        bpm=120.0, key="Am", beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]), bpm_method=method
    )
    controller.project.sample_analysis[0] = analysis
    assert context.state.pads.bpm_execution_label(0) == "Device unknown"
    execution = LegacyCpuExecutionReceipt(1, "cpu", "cpu", None, "float32", "2.8.0+cpu", None, None)
    controller.project.sample_analysis[0] = analysis.model_copy(
        update={"bpm_method": method.model_copy(update={"execution": execution})}
    )
    controller.project.manual_bpm[0] = 94.0
    assert context.state.pads.bpm_execution_label(0) == "CPU"
    controller.session.analyzing_sample_ids.add(0)
    assert context.state.pads.bpm_execution_label(0) == "Device unknown"
    controller.session.analyzing_sample_ids.clear()
    controller.session.loading_sample_ids.add(0)
    assert context.state.pads.bpm_execution_label(0) == "Device unknown"


def test_stem_history_is_distinct_from_unconfirmed_active_attempt(
    controller: AppController,
) -> None:
    context = UiContext(controller)
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version="source-v1",
        cache_dir="samples/stems/#1",
        execution=StemExecution(backend="demucs", model_name="htdemucs", device="cuda"),
    )
    assert context.state.stems.execution_label(0) == "GPU (CUDA)"
    controller.session.stem_generating_sample_ids.add(0)
    assert context.state.stems.execution_label(0) == "Device unknown"


def test_v5_restore_keeps_confirmed_inference_device_without_making_it_a_live_job(
    controller: AppController,
) -> None:
    context = UiContext(controller)
    execution = LegacyCpuExecutionReceipt(1, "cpu", "cpu", None, "float32", "2.8.0+cpu", None, None)
    data = _v5_method().model_dump(mode="python")
    data["execution"] = execution
    method = AppTempoMethod.model_validate(data, strict=True)
    restored = AppTempoMethod.model_validate_json(method.model_dump_json())
    assert restored.execution == execution
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=120.0,
        key="Am",
        beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]),
        bpm_method=restored,
    )
    assert context.state.pads.bpm_execution_label(0) == "CPU"
    controller.session.analyzing_sample_ids.add(0)
    assert context.state.pads.bpm_execution_label(0) == "Device unknown"


@pytest.mark.parametrize("device", ["cpu", "cuda"])
@pytest.mark.parametrize(
    "state", ["automatic", "loading", "analyzing", "manual_value", "manual_intent", "tap"]
)
def test_joint_device_label_separates_cpu_acoustic_support_only_for_current_auto(
    controller: AppController, device: str, state: str
) -> None:
    context = UiContext(controller)
    execution = (
        LegacyCpuExecutionReceipt(1, "cpu", "cpu", None, "float32", "2.8.0+cpu", None, None)
        if device == "cpu"
        else ExecutionReceipt(
            1, "cuda", "cuda:0", "Fixture GPU", "float32", "2.8.0+cu128", "12.8", None
        )
    )
    method = _v6_method().model_copy(update={"execution": execution})
    restored = AppTempoMethod.model_validate_json(method.model_dump_json())
    assert restored.execution == execution
    controller.project.sample_analysis[0] = SampleAnalysis(
        bpm=120.0,
        key="Am",
        beat_grid=BeatGrid(beats=[], downbeats=[], bars=[]),
        bpm_method=restored,
    )
    if state == "loading":
        controller.session.loading_sample_ids.add(0)
    elif state == "analyzing":
        controller.session.analyzing_sample_ids.add(0)
    elif state == "manual_value":
        controller.project.manual_bpm[0] = 94.0
    elif state in {"manual_intent", "tap"}:
        controller.project.pad_timing_intent[0] = "tap" if state == "tap" else "manual"
    expected = "NN CPU; QM CPU" if device == "cpu" else "NN CUDA 0 · Fixture GPU; QM CPU"
    assert context.state.pads.bpm_execution_label(0) == (
        expected if state == "automatic" else "Device unknown"
    )
