"""Validate historical and current Beat This execution receipts."""

from dataclasses import dataclass
from typing import Annotated, Final, Literal

from pydantic import ConfigDict, Field, TypeAdapter

from flitzis_looper.analysis.contracts import (
    ProtocolOne,  # noqa: TC001 -- Pydantic validates this alias.
)

type ExecutionPolicy = Literal["auto", "cpu", "cuda"]
type FallbackReason = Literal[
    "cuda_unavailable", "cuda_probe_error", "cuda_placement_error", "cuda_inference_error"
]


class _StrictDto:
    __slots__ = ()
    __pydantic_config__ = ConfigDict(extra="forbid")


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


@dataclass(frozen=True, slots=True)
class NativeCpuExecutionReceipt(_StrictDto):
    """Actual execution on the pinned CPU-only Native runtime."""

    schema_version: ProtocolOne
    requested_policy: Literal["cpu"]
    device: Literal["cpu"]
    device_name: None
    precision: Literal["float32"]
    ort_version: Literal["1.24.4"]
    ort_api: Literal[24]
    fallback_reason: None


NATIVE_CUDA_PACKAGE_SHA: Final = "d62a363030d5c75db02f357eb2fd9936fc9edc26160f84b83ef19be231103f4b"
NATIVE_CPU_PACKAGE_SHA: Final = "7a5e216d9cc35418e8c38afa293ece04e86abc63153553fe56e81aa901cf2319"
type NativeCudaPolicy = Literal["cuda", "auto"]
type NativeCudaError = Literal[2, 214, 700, 702, 719]
type NativeCudaStartupReason = Literal["cuda_unavailable", "cuda_startup_error"]
type DeviceName = Annotated[str, Field(strict=True, min_length=1, max_length=256)]


@dataclass(frozen=True, slots=True)
class NativeCudaStartupError(_StrictDto):
    """A qualified driver or provider failure before CUDA inference."""

    stage: Literal[
        "driver_load",
        "driver_symbol",
        "driver_init",
        "driver_device",
        "provider_registration",
        "session_initialization",
    ]
    code: Annotated[int, Field(strict=True)] | None

    def fallback_reason(self) -> NativeCudaStartupReason | None:
        """Classify the finite startup evidence also admitted by the Native App."""
        match self.stage, self.code:
            case (
                ("driver_load", 126)
                | ("driver_init", 100)
                | ("driver_device" | "session_initialization", 100 | 101)
                | ("provider_registration", None)
            ):
                return "cuda_unavailable"
            case (
                ("driver_load" | "driver_symbol", 127)
                | ("driver_init" | "driver_device", 3 | 46 | 802 | 803 | 804)
                | (
                    "session_initialization",
                    2 | 3 | 35 | 46 | 214 | 700 | 702 | 719 | 802 | 803 | 804,
                )
            ):
                return "cuda_startup_error"
            case _:
                return None


@dataclass(frozen=True, slots=True)
class NativeCudaRuntime(_StrictDto):
    """The driver's ordinal zero and the pinned ORT CUDA package observed in the child."""

    device_name: DeviceName
    driver_api_version: Annotated[int, Field(strict=True, gt=0, le=2**31 - 1)]
    package_sha256: Literal["d62a363030d5c75db02f357eb2fd9936fc9edc26160f84b83ef19be231103f4b"]
    provider_sha256: Literal["110929d2552bc775828e7a5c18c5ef9ab90898fc57a57868623fe2fe8c82d494"]


@dataclass(frozen=True, slots=True)
class NativeCudaExecutionReceipt(_StrictDto):
    """Effective Native execution and evidence from the original CUDA attempt."""

    schema_version: ProtocolOne
    requested_policy: NativeCudaPolicy
    device: Literal["cpu", "cuda:0"]
    device_name: DeviceName | None
    precision: Literal["float32"]
    ort_version: Literal["1.24.4"]
    ort_api: Literal[24]
    ort_package_sha256: Literal[
        "d62a363030d5c75db02f357eb2fd9936fc9edc26160f84b83ef19be231103f4b",
        "7a5e216d9cc35418e8c38afa293ece04e86abc63153553fe56e81aa901cf2319",
    ]
    cuda: NativeCudaRuntime | None
    cuda_kernel_events: Annotated[int, Field(strict=True, gt=0, le=100_000)] | None
    fallback_reason: (
        Literal["cuda_device_error", "cuda_cache_unavailable"] | NativeCudaStartupReason | None
    )
    cuda_error: NativeCudaError | None
    cuda_startup_error: NativeCudaStartupError | None = None
    cuda_cache_error: Annotated[str, Field(strict=True, min_length=1, max_length=512)] | None = None


type ActualExecutionReceipt = (
    ExecutionReceipt
    | LegacyCpuExecutionReceipt
    | NativeCpuExecutionReceipt
    | NativeCudaExecutionReceipt
)


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
    if isinstance(validated, (LegacyCpuExecutionReceipt, NativeCpuExecutionReceipt)):
        return
    if isinstance(validated, NativeCudaExecutionReceipt):
        if not _native_cuda_execution_valid(validated):
            msg = "incoherent_native_cuda_execution_receipt"
            raise ValueError(msg)
        return
    if not _device_fields_valid(validated) or not _policy_fields_valid(validated):
        msg = "incoherent_execution_receipt"
        raise ValueError(msg)


def _native_cuda_execution_valid(receipt: NativeCudaExecutionReceipt) -> bool:
    name = receipt.cuda.device_name if receipt.cuda is not None else None
    if name is not None and (
        not name.strip() or any(ord(char) < 32 or 127 <= ord(char) <= 159 for char in name)
    ):
        return False
    if receipt.device == "cuda:0":
        return (
            name is not None
            and receipt.device_name == name
            and receipt.ort_package_sha256 == NATIVE_CUDA_PACKAGE_SHA
            and receipt.cuda_kernel_events is not None
            and receipt.fallback_reason is None
            and receipt.cuda_error is None
            and receipt.cuda_startup_error is None
            and receipt.cuda_cache_error is None
        )
    if not (
        receipt.requested_policy == "auto"
        and receipt.device_name is None
        and receipt.ort_package_sha256 == NATIVE_CPU_PACKAGE_SHA
        and receipt.cuda_kernel_events is None
    ):
        return False
    if receipt.cuda_cache_error is not None:
        return (
            name is None
            and receipt.cuda_error is None
            and receipt.cuda_startup_error is None
            and receipt.fallback_reason == "cuda_cache_unavailable"
        )
    if receipt.cuda_startup_error is None:
        return (
            name is not None
            and receipt.fallback_reason == "cuda_device_error"
            and receipt.cuda_error is not None
        )
    reason = receipt.cuda_startup_error.fallback_reason()
    return (
        name is None
        and receipt.cuda_error is None
        and reason is not None
        and receipt.fallback_reason == reason
    )


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


def device_label(receipt: ActualExecutionReceipt | None) -> str:
    """Display confirmed execution, including a coded CPU fallback, independently of stage."""
    if receipt is None:
        return "Device unknown"
    if receipt.device == "cpu":
        if (
            isinstance(receipt, NativeCudaExecutionReceipt)
            and receipt.fallback_reason == "cuda_unavailable"
        ):
            return "CPU (CUDA unavailable)"
        return "CPU (CUDA fallback)" if receipt.fallback_reason is not None else "CPU"
    return f"CUDA 0 · {receipt.device_name}"
