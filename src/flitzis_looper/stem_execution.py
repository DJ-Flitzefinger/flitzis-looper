"""Completed separator execution observations; these grant no playback authority."""

from typing import TYPE_CHECKING, Annotated, Literal, Self

from pydantic import BaseModel, ConfigDict, Field, ValidationError, model_validator

if TYPE_CHECKING:
    from collections.abc import Callable

STEM_EXECUTION_RESULT = "FLITZIS_STEM_EXECUTION_V1:"


class StemExecution(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="forbid", strict=True, revalidate_instances="always"
    )

    schema_version: Annotated[int, Field(strict=True, ge=1, le=1)] = 1
    backend: Literal["demucs", "bs-roformer"]
    model_name: Annotated[str, Field(min_length=1, max_length=256)]
    device: Literal["cpu", "cuda"]
    cpu_fallback: bool = False

    @model_validator(mode="after")
    def _coherent_fallback(self) -> Self:
        if self.cpu_fallback and self.device != "cpu":
            message = "CPU fallback requires completed CPU inference"
            raise ValueError(message)
        return self


def completed_execution_line(backend: str, model_name: str, device: str) -> str:
    """Encode only after the owned child's successful inference has completed."""
    receipt = StemExecution.model_validate({
        "backend": backend,
        "model_name": model_name,
        "device": device,
    })
    return STEM_EXECUTION_RESULT + receipt.model_dump_json()


def completed_execution(
    stdout: str, *, backend: str, model_name: str, device: str, cpu_fallback: bool
) -> StemExecution | None:
    """Missing, duplicate, malformed or mismatched diagnostics stay unknown."""
    lines = [line for line in stdout.splitlines() if line.startswith(STEM_EXECUTION_RESULT)]
    if len(lines) != 1 or len(lines[0]) > 1000:
        return None
    try:
        receipt = StemExecution.model_validate_json(
            lines[0][len(STEM_EXECUTION_RESULT) :], strict=True
        )
    except ValidationError:
        return None
    if (
        receipt.backend != backend
        or receipt.model_name != model_name
        or receipt.device != device
        or receipt.cpu_fallback
    ):
        return None
    return receipt.model_copy(update={"cpu_fallback": cpu_fallback})


def report_execution(progress: Callable[[float, str], None], receipt: StemExecution | None) -> None:
    """Keep ordinary two-argument callbacks and third-party backends compatible."""
    receiver = getattr(progress, "execution", None)
    if receipt is not None and callable(receiver):
        receiver(receipt)


class StemExecutionProgress:
    """Latch a completed receipt into the existing progress event stream."""

    def __init__(
        self,
        report: Callable[[float | None, str | None, StemExecution | None], None],
    ) -> None:
        self._report = report
        self._execution: StemExecution | None = None

    def __call__(self, percent: float, stage: str) -> None:
        self._report(percent, stage, self._execution)

    def execution(self, receipt: StemExecution) -> None:
        self._execution = receipt
        self._report(None, None, receipt)


def device_label(receipt: StemExecution | None) -> str:
    if receipt is None:
        return "Device unknown"
    if receipt.cpu_fallback:
        return "CPU (CUDA fallback)"
    return "GPU (CUDA)" if receipt.device == "cuda" else "CPU"
