"""Execution diagnostics remain separate from source and cache authority."""

from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.bs_input_provenance import BS_INPUT_RESULT
from flitzis_looper.controller import stem_generation
from flitzis_looper.copy_snapshot import CopySnapshot, capture_copy_snapshot
from flitzis_looper.encoder_provenance import ENCODER_RESULT
from flitzis_looper.models import PadContentIdentity, StemCacheEntry
from flitzis_looper.stem_execution import (
    StemExecution,
    StemExecutionProgress,
    completed_execution,
    completed_execution_line,
    device_label,
    report_execution,
)

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def _receipt() -> StemExecution:
    return StemExecution(backend="demucs", model_name="htdemucs", device="cuda")


@pytest.mark.parametrize("device", ["cpu", "cuda"])
def test_completed_receipt_requires_one_exact_backend_model_and_device(device: str) -> None:
    line = completed_execution_line("demucs", "htdemucs", device)
    receipt = completed_execution(
        line, backend="demucs", model_name="htdemucs", device=device, cpu_fallback=device == "cpu"
    )
    assert receipt is not None
    assert receipt.device == device
    assert receipt.cpu_fallback is (device == "cpu")
    for stdout in ("", line + "\n" + line, line + "broken"):
        assert (
            completed_execution(
                stdout, backend="demucs", model_name="htdemucs", device=device, cpu_fallback=False
            )
            is None
        )
    assert (
        completed_execution(
            line, backend="bs-roformer", model_name="htdemucs", device=device, cpu_fallback=False
        )
        is None
    )
    assert (
        completed_execution(
            line, backend="demucs", model_name="other", device=device, cpu_fallback=False
        )
        is None
    )


def test_device_receipt_does_not_replace_independent_encoder_or_bs_input_receipt() -> None:
    execution = completed_execution_line("demucs", "htdemucs", "cuda")
    encoder = ENCODER_RESULT + "owned-encoder"
    bs_input = BS_INPUT_RESULT + "owned-input"
    stdout = stem_generation._command_stdout(f"diagnostic\n{execution}\n{encoder}\n{bs_input}")
    assert all(line in stdout.splitlines() for line in (execution, encoder, bs_input))
    duplicate = stem_generation._command_stdout(f"{execution}\n{execution}\n{encoder}")
    assert encoder in duplicate.splitlines()
    assert (
        completed_execution(
            duplicate, backend="demucs", model_name="htdemucs", device="cuda", cpu_fallback=False
        )
        is None
    )


def test_two_argument_progress_keeps_typed_execution_during_alignment() -> None:
    events = []
    progress = StemExecutionProgress(lambda p, s, e: events.append((p, s, e)))
    progress(0.05, "Starting")
    report_execution(progress, _receipt())
    progress(0.85, "Aligning stem cache")
    assert events[0][2] is None
    assert events[-1] == (0.85, "Aligning stem cache", _receipt())
    plain = []
    report_execution(lambda p, s: plain.append((p, s)), _receipt())
    assert plain == []


def test_cache_json_preserves_history_but_damaged_optional_metadata_keeps_pcm_selection() -> None:
    entry = StemCacheEntry(source_version="source-v1", cache_dir="samples/stems/#1", available=True)
    assert "execution" not in entry.model_dump()
    known = entry.model_copy(update={"execution": _receipt()})
    restored = StemCacheEntry.model_validate_json(known.model_dump_json())
    assert restored.execution == _receipt()
    damaged = known.model_dump()
    damaged["execution"] = {"device": "invented"}
    restored = StemCacheEntry.model_validate(damaged)
    assert restored.available
    assert restored.execution is None


def test_active_unknown_never_falls_back_to_old_cache_and_clear_removes_observation(
    controller: AppController,
) -> None:
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version="source-v1", cache_dir="samples/stems/#1", execution=_receipt()
    )
    assert controller.stems.stem_execution(0) == _receipt()
    controller.session.stem_generating_sample_ids.add(0)
    assert controller.stems.stem_execution(0) is None
    controller.stems._handle_stem_generation_progress(0, 0.85, "Aligning", execution=_receipt())
    assert controller.stems.stem_execution(0) == _receipt()
    controller.session.loading_sample_ids.add(0)
    assert controller.stems.stem_execution(0) is None
    controller.stems._clear_stem_generation_state(0)
    assert 0 not in controller.session.stem_generation_execution
    assert device_label(None) == "Device unknown"


def test_legacy_loader_error_clears_its_device_observation(controller: AppController) -> None:
    controller.loader._on_stem_generation_error = None
    controller.session.stem_generating_sample_ids.add(0)
    controller.session.stem_generation_execution[0] = _receipt()
    controller.session.stem_generation_stage[0] = "Aligning"
    controller.loader._handle_task_error(0, {"task": "stem_generation", "msg": "worker failed"})
    assert 0 not in controller.session.stem_generation_execution
    assert 0 not in controller.session.stem_generation_stage
    assert controller.session.stem_generation_errors[0] == "worker failed"


def test_copy_retains_only_historical_stem_device_without_availability(
    controller: AppController,
) -> None:
    controller.project.sample_paths[0] = "samples/source.wav"
    controller.project.pad_content[0] = PadContentIdentity(instance_id="a" * 32)
    controller.project.stem_cache[0] = StemCacheEntry(
        source_version="source-v1",
        cache_dir="samples/stems/#1",
        available=True,
        execution=_receipt(),
    )
    snapshot = capture_copy_snapshot(controller.project, 0)
    reopened = CopySnapshot.model_validate_json(snapshot.model_dump_json())
    copied = reopened.projected().stem_cache
    assert copied is not None
    assert copied.execution == _receipt()
    assert not copied.available


@pytest.mark.parametrize("schema", [False, True])
def test_boolean_protocol_is_rejected_without_erasing_cache_pcm(schema: object) -> None:
    raw = _receipt().model_dump() | {"schema_version": schema}
    with pytest.raises(ValidationError):
        StemExecution.model_validate(raw)
    entry = StemCacheEntry.model_validate({
        "source_version": "source-v1",
        "cache_dir": "samples/stems/#1",
        "available": True,
        "execution": raw,
    })
    assert entry.available
    assert entry.execution is None
