"""Source-owned completed separator observations, without playback authority."""

import json
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, Literal, cast

if TYPE_CHECKING:
    from collections.abc import Callable

STEM_EXECUTION_RESULT = "FLITZIS_STEM_EXECUTION_V1:"


def _integer(value: object, minimum: int, maximum: int) -> bool:
    return type(value) is int and minimum <= value <= maximum


@dataclass(frozen=True, slots=True, kw_only=True)
class ExecutionReceipt:
    """Strict internal V1 data used by producers and passive calibration."""

    schema_version: int = 1
    backend: Literal["demucs", "bs-roformer"]
    model_name: str
    device: Literal["cpu", "cuda"]
    cpu_fallback: bool = False
    intra_op_threads: int | None = None
    inter_op_threads: int | None = None
    cuda_peak_bytes: int | None = None
    cuda_device_identity: str | None = None

    def __post_init__(self) -> None:
        message = "Invalid completed execution fields"
        if (
            not _integer(self.schema_version, 1, 1)
            or type(self.backend) is not str
            or self.backend not in {"demucs", "bs-roformer"}
            or type(self.device) is not str
            or self.device not in {"cpu", "cuda"}
        ):
            raise ValueError(message)
        if (
            type(self.model_name) is not str
            or not 1 <= len(self.model_name) <= 256
            or type(self.cpu_fallback) is not bool
        ):
            raise ValueError(message)
        # The old JSON validator rejects unpaired Unicode surrogates.
        self.model_name.encode("utf-8")
        message = "Incoherent completed execution observations"
        if (
            (self.intra_op_threads is None) != (self.inter_op_threads is None)
            or (self.intra_op_threads is not None and not _integer(self.intra_op_threads, 1, 1))
            or (self.inter_op_threads is not None and not _integer(self.inter_op_threads, 1, 1))
        ):
            raise ValueError(message)
        if (self.cpu_fallback and self.device != "cpu") or (
            self.cuda_peak_bytes is not None
            and (
                self.device != "cuda" or not _integer(self.cuda_peak_bytes, 0, 9223372036854775807)
            )
        ):
            raise ValueError(message)
        if self.cuda_device_identity is not None and (
            self.device != "cuda"
            or type(self.cuda_device_identity) is not str
            or len(self.cuda_device_identity) != 64
            or any(char not in "0123456789abcdef" for char in self.cuda_device_identity)
        ):
            raise ValueError(message)


def execution_fields(receipt: ExecutionReceipt) -> dict[str, object]:
    """Keep the original V1 field order, including explicit null observations."""
    return {
        "schema_version": receipt.schema_version,
        "backend": receipt.backend,
        "model_name": receipt.model_name,
        "device": receipt.device,
        "cpu_fallback": receipt.cpu_fallback,
        "intra_op_threads": receipt.intra_op_threads,
        "inter_op_threads": receipt.inter_op_threads,
        "cuda_peak_bytes": receipt.cuda_peak_bytes,
        "cuda_device_identity": receipt.cuda_device_identity,
    }


def completed_execution_line(
    backend: str,
    model_name: str,
    device: str,
    *,
    threads: tuple[int, int] | None = None,
    cuda_peak_bytes: int | None = None,
    cuda_device_identity: str | None = None,
) -> str:
    """Encode only after the owned child's successful inference has completed."""
    receipt = ExecutionReceipt(
        backend=cast("Literal['demucs', 'bs-roformer']", backend),
        model_name=model_name,
        device=cast("Literal['cpu', 'cuda']", device),
        intra_op_threads=None if threads is None else threads[0],
        inter_op_threads=None if threads is None else threads[1],
        cuda_peak_bytes=cuda_peak_bytes,
        cuda_device_identity=cuda_device_identity,
    )
    fields = execution_fields(receipt)
    if receipt.cuda_device_identity is None:
        del fields["cuda_device_identity"]
    return STEM_EXECUTION_RESULT + json.dumps(fields, ensure_ascii=False, separators=(",", ":"))


def completed_execution(
    stdout: str, *, backend: str, model_name: str, device: str, cpu_fallback: bool
) -> ExecutionReceipt | None:
    """Missing, duplicate, malformed or mismatched diagnostics stay unknown."""
    lines = [
        line.rstrip("\r") for line in stdout.split("\n") if line.startswith(STEM_EXECUTION_RESULT)
    ]
    if len(lines) != 1 or len(lines[0]) > 1000:
        return None
    try:
        value = json.loads(lines[0][len(STEM_EXECUTION_RESULT) :])
        if type(value) is not dict:
            return None
        receipt = ExecutionReceipt(**value)
        if (
            receipt.backend != backend
            or receipt.model_name != model_name
            or receipt.device != device
            or receipt.cpu_fallback
        ):
            return None
        return replace(receipt, cpu_fallback=cpu_fallback)
    except TypeError, ValueError:
        return None


def report_execution(
    progress: Callable[[float, str], None], receipt: ExecutionReceipt | None
) -> None:
    """Keep ordinary two-argument callbacks and third-party backends compatible."""
    receiver = getattr(progress, "execution", None)
    if receipt is not None and callable(receiver):
        receiver(receipt)


class ExecutionProgress:
    """Latch internal observations until the existing control event boundary."""

    def __init__(
        self,
        report: Callable[[float | None, str | None, ExecutionReceipt | None], None],
    ) -> None:
        self._report = report
        self._execution: ExecutionReceipt | None = None

    def __call__(self, percent: float, stage: str) -> None:
        self._report(percent, stage, self._execution)

    def execution(self, receipt: ExecutionReceipt) -> None:
        self._execution = receipt
        self._report(None, None, receipt)
