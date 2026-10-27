"""Device labels describe recorded inference, separately from live audio controls."""

from typing import TYPE_CHECKING

from flitzis_looper.analysis.execution import LegacyCpuExecutionReceipt
from flitzis_looper.models import BeatGrid, SampleAnalysis, StemCacheEntry
from flitzis_looper.stem_execution import StemExecution
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.controller.test_adaptive_tempo_probe import _v4_method

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
