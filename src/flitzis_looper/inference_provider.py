"""Narrow device-failure boundary shared by disposable inference children."""

import sys
from contextlib import contextmanager
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterator

CUDA_FAILURE_EXIT = 70
CUDA_FAILURE_RECEIPT = "FLITZIS_CUDA_FAILURE_V1:placement_or_inference"


class CudaProviderError(RuntimeError):
    """A recognized CUDA failure inside an explicit device operation."""


@contextmanager
def cuda_operation(device: str) -> Iterator[None]:
    """Classify device faults without retrying arbitrary model or data failures."""
    try:
        yield
    except (RuntimeError, AssertionError) as error:
        diagnostic = str(error).lower()
        if device == "cuda" and diagnostic.startswith((
            "cuda error:",
            "cuda out of memory",
            "cuda unavailable",
            "cuda driver",
            "cudnn error:",
            "cublas_status_",
            "found no nvidia driver on your system",
            "torch not compiled with cuda enabled",
        )):
            raise CudaProviderError(str(error)) from error
        raise


def emit_cuda_failure(error: CudaProviderError) -> None:
    """Emit a separate bounded marker only from the device-operation boundary."""
    sys.stderr.write(str(error)[:1000] + "\n")
    sys.stderr.flush()
    sys.stdout.write(CUDA_FAILURE_RECEIPT + "\n")
    sys.stdout.flush()


def is_cuda_failure(returncode: int, stdout: str) -> bool:
    """Require both the dedicated exit and exactly one complete child marker."""
    return returncode == CUDA_FAILURE_EXIT and stdout.splitlines().count(CUDA_FAILURE_RECEIPT) == 1
