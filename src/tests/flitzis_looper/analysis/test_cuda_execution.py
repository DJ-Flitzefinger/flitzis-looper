"""Read historical Beat This execution receipts in saved projects."""

import json
from dataclasses import asdict, replace
from typing import Literal

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis.execution import (
    ACTUAL_EXECUTION_ADAPTER,
    ExecutionReceipt,
    LegacyCpuExecutionReceipt,
    device_label,
    validate_execution,
)


def receipt(device: Literal["cpu", "cuda:0"] = "cuda:0") -> ExecutionReceipt:
    return ExecutionReceipt(
        1,
        "auto",
        device,
        "Fixture GPU" if device != "cpu" else None,
        "float32",
        "2.8.0+cu128",
        "12.8",
        None if device != "cpu" else "cuda_unavailable",
    )


@pytest.mark.parametrize(
    "change",
    [
        {"device": "cuda:1"},
        {"requested_policy": "cpu"},
        {"torch_version": "2.8.0+cpu"},
        {"cuda_build_version": None},
        {"fallback_reason": "cuda_probe_error"},
        {"device_name": None},
    ],
)
def test_impossible_cuda_receipts_fail(change: dict[str, object]) -> None:
    with pytest.raises((ValidationError, ValueError)):
        validate_execution(replace(receipt(), **change))


@pytest.mark.parametrize("version", [True, False, 1.0])
def test_execution_receipt_version_requires_an_actual_integer(version: object) -> None:
    data = {**asdict(receipt()), "schema_version": version}
    with pytest.raises(ValidationError):
        ACTUAL_EXECUTION_ADAPTER.validate_json(json.dumps(data), strict=True)


def test_cpu_fallback_and_gpu_are_separate_confirmed_receipts() -> None:
    validate_execution(receipt())
    validate_execution(receipt("cpu"))
    assert device_label(receipt()).startswith("CUDA 0")
    assert device_label(receipt("cpu")) == "CPU (CUDA fallback)"
    assert device_label(None) == "Device unknown"
    with pytest.raises(ValueError, match="incoherent_execution_receipt"):
        validate_execution(replace(receipt("cpu"), fallback_reason=None))


def test_new_legacy_cpu_receipt_is_distinct_from_cuda_runtime_cpu_fallback() -> None:
    legacy = LegacyCpuExecutionReceipt(1, "cpu", "cpu", None, "float32", "2.8.0+cpu", None, None)
    validate_execution(legacy)
    assert device_label(legacy) == "CPU"
    encoded = ACTUAL_EXECUTION_ADAPTER.dump_json(legacy)
    assert ACTUAL_EXECUTION_ADAPTER.validate_json(encoded, strict=True) == legacy
    with pytest.raises(ValueError, match="execution_policy_mismatch"):
        validate_execution(legacy, "auto")
    forged = asdict(legacy)
    forged["cuda_build_version"] = "12.8"
    with pytest.raises(ValueError, match="cuda_build_version"):
        ACTUAL_EXECUTION_ADAPTER.validate_python(forged)
