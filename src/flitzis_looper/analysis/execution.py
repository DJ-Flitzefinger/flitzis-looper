"""Protocol-two execution receipts; legacy CPU DTOs remain byte-for-byte separate."""

import importlib
from dataclasses import dataclass
from typing import Annotated, Literal

from pydantic import ConfigDict, Field, TypeAdapter

from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatPredictions,
    ComponentStatus,
    IdentityText,
    MonoPcmInput,
    ProtocolOne,
    WorkerLimits,
    validate_predictions,
)

CUDA_LOCK_SHA256 = "0a662b8a7b4058899bc8eb0b775d186bc1e5acf6f101ec2186d3f998c84d0f30"
CUDA_ENVIRONMENT_ID = f"uv-lock-sha256:{CUDA_LOCK_SHA256}"
type ExecutionPolicy = Literal["auto", "cpu", "cuda"]
type FallbackReason = Literal[
    "cuda_unavailable", "cuda_probe_error", "cuda_placement_error", "cuda_inference_error"
]
type ProtocolTwo = Annotated[int, Field(strict=True, ge=2, le=2)]


class _StrictDto:
    __slots__ = ()
    __pydantic_config__ = ConfigDict(extra="forbid")


@dataclass(frozen=True, slots=True)
class CudaBeatModelIdentity(_StrictDto):
    """Known CUDA-capable profile; requested policy is distinct from actual execution."""

    sha256: Annotated[str, Field(strict=True, pattern=r"^[0-9a-f]{64}$")]
    frontend_id: IdentityText
    environment_id: Literal[
        "uv-lock-sha256:0a662b8a7b4058899bc8eb0b775d186bc1e5acf6f101ec2186d3f998c84d0f30"
    ] = "uv-lock-sha256:0a662b8a7b4058899bc8eb0b775d186bc1e5acf6f101ec2186d3f998c84d0f30"
    package_version: Literal["1.1.0"] = "1.1.0"
    checkpoint: Literal["final0"] = "final0"
    postprocessor: Literal["minimal"] = "minimal"
    device: Literal["auto"] = "auto"
    precision: Literal["float32"] = "float32"


@dataclass(frozen=True, slots=True)
class ExecutionReceipt(_StrictDto):
    """Model and inference tensor placement observed by the executing worker."""

    schema_version: ProtocolOne
    requested_policy: ExecutionPolicy
    device: Literal["cpu", "cuda:0"]
    device_name: Annotated[str, Field(strict=True, min_length=1, max_length=256)] | None
    precision: Literal["float32"]
    torch_version: Literal["2.8.0+cu128"]
    cuda_build_version: Literal["12.8"]
    fallback_reason: FallbackReason | None


@dataclass(frozen=True, slots=True)
class LegacyCpuExecutionReceipt(_StrictDto):
    """New App diagnostic derived from validated CPU-only raw, never added to old raw."""

    schema_version: ProtocolOne
    requested_policy: Literal["cpu"]
    device: Literal["cpu"]
    device_name: None
    precision: Literal["float32"]
    torch_version: Literal["2.8.0+cpu"]
    cuda_build_version: None
    fallback_reason: None


type ActualExecutionReceipt = ExecutionReceipt | LegacyCpuExecutionReceipt


@dataclass(frozen=True, slots=True)
class CudaBeatWorkerRequest(_StrictDto):
    """One borrowed full-source request to the finite CUDA-capable profile."""

    identity: AnalysisIdentity
    pcm: MonoPcmInput
    model: CudaBeatModelIdentity
    execution_policy: ExecutionPolicy = "auto"
    schema_version: ProtocolTwo = 2


@dataclass(frozen=True, slots=True)
class CudaBeatWorkerResponse(_StrictDto):
    identity: AnalysisIdentity
    model: CudaBeatModelIdentity
    predictions: BeatPredictions
    execution: ExecutionReceipt
    schema_version: ProtocolTwo = 2


@dataclass(frozen=True, slots=True)
class CudaBeatComponentResult(_StrictDto):
    identity: AnalysisIdentity
    model: CudaBeatModelIdentity
    status: ComponentStatus
    reason: str
    predictions: BeatPredictions | None = None
    resources_released: bool = True
    execution: ExecutionReceipt | None = None


CUDA_REQUEST_ADAPTER = TypeAdapter(CudaBeatWorkerRequest)
CUDA_RESPONSE_ADAPTER = TypeAdapter(CudaBeatWorkerResponse)
CUDA_COMPONENT_ADAPTER = TypeAdapter(CudaBeatComponentResult)
EXECUTION_ADAPTER = TypeAdapter(ExecutionReceipt)
ACTUAL_EXECUTION_ADAPTER: TypeAdapter[ActualExecutionReceipt] = TypeAdapter(ActualExecutionReceipt)


def validate_execution(
    receipt: ActualExecutionReceipt, policy: ExecutionPolicy | None = None
) -> None:
    """Availability and attempted placement cannot masquerade as confirmed execution."""
    validated = ACTUAL_EXECUTION_ADAPTER.validate_json(
        ACTUAL_EXECUTION_ADAPTER.dump_json(receipt), strict=True
    )
    if policy is not None and validated.requested_policy != policy:
        msg = "execution_policy_mismatch"
        raise ValueError(msg)
    if isinstance(validated, LegacyCpuExecutionReceipt):
        return
    if not _device_fields_valid(validated) or not _policy_fields_valid(validated):
        msg = "incoherent_execution_receipt"
        raise ValueError(msg)


def _device_fields_valid(receipt: ExecutionReceipt) -> bool:
    if receipt.device == "cpu":
        return receipt.device_name is None
    return receipt.device_name is not None and receipt.fallback_reason is None


def _policy_fields_valid(receipt: ExecutionReceipt) -> bool:
    cpu = receipt.device == "cpu"
    if receipt.requested_policy == "cpu":
        return cpu and receipt.fallback_reason is None
    if receipt.requested_policy == "cuda":
        return not cpu
    return not cpu or receipt.fallback_reason is not None


def encode_cuda_request(request: CudaBeatWorkerRequest, limits: WorkerLimits) -> bytes:
    encoded = CUDA_REQUEST_ADAPTER.dump_json(request)
    CUDA_REQUEST_ADAPTER.validate_json(encoded, strict=True)
    if len(encoded) > limits.max_request_bytes:
        msg = "request_limit"
        raise ValueError(msg)
    if request.pcm.frame_count * 4 > limits.max_pcm_bytes:
        msg = "pcm_limit"
        raise ValueError(msg)
    if not request.pcm.path.is_absolute():
        msg = "pcm_path_must_be_absolute"
        raise ValueError(msg)
    if request.pcm.path.stat().st_size != request.pcm.frame_count * 4:
        msg = "pcm_size_mismatch"
        raise ValueError(msg)
    return encoded


def decode_cuda_response(
    encoded: bytes, request: CudaBeatWorkerRequest, limits: WorkerLimits
) -> CudaBeatWorkerResponse:
    if len(encoded) > limits.max_response_bytes:
        msg = "response_limit"
        raise ValueError(msg)
    response = CUDA_RESPONSE_ADAPTER.validate_json(encoded, strict=True)
    if response.identity != request.identity or response.model != request.model:
        msg = "response_identity_mismatch"
        raise ValueError(msg)
    validate_execution(response.execution, request.execution_policy)
    validate_predictions(response.predictions, request.pcm, limits)
    return response


def validate_cuda_component_result(
    result: CudaBeatComponentResult, request: CudaBeatWorkerRequest, limits: WorkerLimits
) -> None:
    """Injected and production adapters retain exactly the original request owner."""
    if not isinstance(result, CudaBeatComponentResult):
        message = "invalid_cuda_component_type"
        raise TypeError(message)
    encoded = CUDA_COMPONENT_ADAPTER.dump_json(result)
    if len(encoded) > limits.max_response_bytes:
        message = "cuda_component_limit"
        raise ValueError(message)
    validated = CUDA_COMPONENT_ADAPTER.validate_json(encoded, strict=True)
    if validated.identity != request.identity or validated.model != request.model:
        message = "cuda_component_identity_mismatch"
        raise ValueError(message)
    if validated.status != "ready":
        if validated.predictions is not None or validated.execution is not None:
            message = "unsuccessful_cuda_component_has_payload"
            raise ValueError(message)
        return
    if validated.predictions is None or validated.execution is None:
        message = "ready_cuda_component_missing_receipt"
        raise ValueError(message)
    response = CudaBeatWorkerResponse(
        validated.identity, validated.model, validated.predictions, validated.execution
    )
    decode_cuda_response(CUDA_RESPONSE_ADAPTER.dump_json(response), request, limits)


def device_label(receipt: ActualExecutionReceipt | None) -> str:
    """Display confirmed execution, including a coded CPU fallback, independently of stage."""
    if receipt is None:
        return "Device unknown"
    if receipt.device == "cpu":
        return "CPU (CUDA fallback)" if receipt.fallback_reason is not None else "CPU"
    return f"CUDA 0 · {receipt.device_name}"


def require_cuda_native_capability(engine_type: object | None = None) -> None:
    """Reject a CUDA pointer/runtime pairing before inference or pointer publication."""
    if engine_type is None:
        engine_type = importlib.import_module("flitzis_looper_audio").AudioEngine
    version = getattr(engine_type, "BEAT_THIS_EXECUTION_API_VERSION", None)
    if type(version) is not int or version != 2:
        message = (
            "CUDA Beat This requires BEAT_THIS_EXECUTION_API_VERSION=2; "
            "restore the paired CPU pointer or update Native while the App is closed"
        )
        raise ValueError(message)
