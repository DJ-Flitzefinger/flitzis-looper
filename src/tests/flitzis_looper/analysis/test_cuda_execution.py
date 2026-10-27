"""Finite worker profiles and actual-device receipts, without inference or CUDA probes."""

import json
import runpy
import tomllib
from dataclasses import asdict, replace
from types import SimpleNamespace
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis import artifacts, setup
from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatModelIdentity,
    BeatPredictions,
    BeatWorkerRequest,
    MonoPcmInput,
    WorkerLimits,
    encode_request,
)
from flitzis_looper.analysis.cuda_artifacts import (
    CUDA_MANIFEST_ADAPTER,
    CudaInstalledManifest,
    cuda_source_directory,
    validate_cuda_environment,
)
from flitzis_looper.analysis.execution import (
    ACTUAL_EXECUTION_ADAPTER,
    CUDA_LOCK_SHA256,
    CUDA_REQUEST_ADAPTER,
    CUDA_RESPONSE_ADAPTER,
    CudaBeatComponentResult,
    CudaBeatModelIdentity,
    CudaBeatWorkerRequest,
    CudaBeatWorkerResponse,
    ExecutionReceipt,
    LegacyCpuExecutionReceipt,
    decode_cuda_response,
    device_label,
    require_cuda_native_capability,
    validate_cuda_component_result,
    validate_execution,
)
from flitzis_looper.analysis.worker_profile_markers import expected_packages, marker_applies

if TYPE_CHECKING:
    from pathlib import Path


def receipt(device: str = "cuda:0") -> ExecutionReceipt:
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


def test_old_request_serialization_has_no_new_optional_fields(tmp_path: Path) -> None:
    path = tmp_path / "source.f32le"
    path.write_bytes(bytes(400))
    request = BeatWorkerRequest(
        AnalysisIdentity(0, 2, "source", 3), MonoPcmInput(path, 48000, 100), BeatModelIdentity()
    )
    encoded = encode_request(request, WorkerLimits())
    assert json.loads(encoded) == {
        **asdict(request),
        "pcm": {**asdict(request.pcm), "path": str(path)},
    }
    assert b"execution" not in encoded
    assert b"auto" not in encoded


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


def test_protocol_two_response_preserves_exact_expected_model_and_owner(tmp_path: Path) -> None:
    request = CudaBeatWorkerRequest(
        AnalysisIdentity(0, 2, "source", 3),
        MonoPcmInput(tmp_path / "source", 48000, 48000),
        CudaBeatModelIdentity(artifacts.ACCEPTED_ARTIFACT.sha256, artifacts.FRONTEND_ID),
    )
    predictions = BeatPredictions((0.1, 0.5), (0.1,), (0.25,), (0.5,))
    response = CudaBeatWorkerResponse(request.identity, request.model, predictions, receipt())
    encoded = CUDA_RESPONSE_ADAPTER.dump_json(response)
    assert decode_cuda_response(encoded, request, WorkerLimits()) == response
    for changed in [
        replace(request, execution_policy="cpu"),
        replace(request, identity=replace(request.identity, source_generation=4)),
    ]:
        with pytest.raises(
            ValueError, match=r"execution_policy_mismatch|response_identity_mismatch"
        ):
            decode_cuda_response(encoded, changed, WorkerLimits())
    wrong = json.loads(CUDA_REQUEST_ADAPTER.dump_json(request))
    wrong["model"]["environment_id"] = "arbitrary"
    with pytest.raises(ValueError, match="environment_id"):
        CUDA_REQUEST_ADAPTER.validate_python(wrong)


def test_lock_markers_use_worker_target_and_exclude_linux_only_edges() -> None:
    assert marker_applies("python_version == '3.12' and sys_platform == 'win32'")
    assert not marker_applies("sys_platform == 'linux' or python_version >= '3.14'")
    lock = {
        "package": [
            {
                "name": "root",
                "version": "1",
                "dependencies": [
                    {"name": "win", "marker": "sys_platform == 'win32'"},
                    {"name": "linux", "marker": "sys_platform == 'linux'"},
                ],
            },
            {"name": "win", "version": "2"},
            {"name": "linux", "version": "3"},
        ]
    }
    assert expected_packages(lock, "root") == {"win": "2"}
    with pytest.raises(ValueError, match="unsupported_worker_lock_marker"):
        marker_applies("__import__('os').name == 'nt'")


def test_known_cuda_lock_closure_is_bound_to_specific_source_project() -> None:
    project = cuda_source_directory()
    assert artifacts.file_digest(project / "uv.lock") == CUDA_LOCK_SHA256
    with (project / "uv.lock").open("rb") as stream:
        packages = expected_packages(tomllib.load(stream), "flitzis-beat-this-cuda-worker")
    assert packages["torch"] == packages["torchaudio"] == "2.8.0+cu128"
    assert not any(name.startswith("nvidia-") for name in packages)
    assert "pytest" not in packages
    env = artifacts.EnvironmentManifest(
        1,
        artifacts.FRONTEND_ID,
        f"uv-lock-sha256:{CUDA_LOCK_SHA256}",
        "3.12.13",
        packages,
        CUDA_LOCK_SHA256,
        artifacts.file_digest(project / "worker.py"),
    )
    validate_cuda_environment(env, project)
    with pytest.raises(ValueError, match="worker_environment_mismatch"):
        validate_cuda_environment(
            replace(env, packages={**packages, "nvidia-cublas-cu12": "1"}), project
        )


def test_cuda_manifest_version_requires_an_actual_integer() -> None:
    manifest = CudaInstalledManifest(
        2,
        "cuda-auto-v2",
        artifacts.ACCEPTED_ARTIFACT,
        CudaBeatModelIdentity(artifacts.ACCEPTED_ARTIFACT.sha256, artifacts.FRONTEND_ID),
        {},
        artifacts.EnvironmentManifest(
            1,
            artifacts.FRONTEND_ID,
            f"uv-lock-sha256:{CUDA_LOCK_SHA256}",
            "3.12.13",
            {},
            CUDA_LOCK_SHA256,
            "0" * 64,
        ),
        {},
    )
    encoded = CUDA_MANIFEST_ADAPTER.dump_json(manifest)
    assert CUDA_MANIFEST_ADAPTER.validate_json(encoded, strict=True) == manifest
    forged = json.loads(encoded)
    forged["schema_version"] = 2.0
    with pytest.raises(ValidationError, match="schema_version"):
        CUDA_MANIFEST_ADAPTER.validate_json(json.dumps(forged), strict=True)


def test_cuda_component_cannot_borrow_request_identity_from_its_validator(tmp_path: Path) -> None:
    request = CudaBeatWorkerRequest(
        AnalysisIdentity(0, 2, "source", 3),
        MonoPcmInput(tmp_path / "source", 48000, 48000),
        CudaBeatModelIdentity(artifacts.ACCEPTED_ARTIFACT.sha256, artifacts.FRONTEND_ID),
    )
    result = CudaBeatComponentResult(
        request.identity,
        request.model,
        "ready",
        "ready",
        BeatPredictions((0.1,), (0.1,), (0.2,), (0.3,)),
        execution=receipt(),
    )
    validate_cuda_component_result(result, request, WorkerLimits())
    with pytest.raises(ValueError, match="cuda_component_identity_mismatch"):
        validate_cuda_component_result(
            replace(result, identity=replace(result.identity, source_generation=4)),
            request,
            WorkerLimits(),
        )
    with pytest.raises(ValueError, match="missing_receipt"):
        validate_cuda_component_result(replace(result, execution=None), request, WorkerLimits())


def test_only_cuda_configuration_requires_the_exact_native_capability() -> None:
    class OldNative:
        pass

    class CurrentNative:
        BEAT_THIS_EXECUTION_API_VERSION = 2

    require_cuda_native_capability(CurrentNative)
    with pytest.raises(ValueError, match="BEAT_THIS_EXECUTION_API_VERSION=2"):
        require_cuda_native_capability(OldNative)


def test_portable_gate_keeps_old_native_usable_with_cpu_pointer(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    check = runpy.run_path(
        str(cuda_source_directory().parents[1] / "scripts" / "portable" / "bootstrap.py")
    )["check_beat_this_execution_api"]
    (tmp_path / "current.json").write_text("{}", encoding="utf-8")
    monkeypatch.setattr(
        setup, "load_worker_configuration", lambda *_: SimpleNamespace(model=BeatModelIdentity())
    )
    check(object(), tmp_path)
    monkeypatch.setattr(
        setup,
        "load_worker_configuration",
        lambda *_: SimpleNamespace(
            model=CudaBeatModelIdentity(artifacts.ACCEPTED_ARTIFACT.sha256, artifacts.FRONTEND_ID)
        ),
    )
    with pytest.raises(ValueError, match="BEAT_THIS_EXECUTION_API_VERSION=2"):
        check(object(), tmp_path)
