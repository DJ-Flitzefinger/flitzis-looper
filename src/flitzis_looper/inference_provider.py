"""Receipt gate for an owned inference child's CUDA device failure."""

CUDA_FAILURE_EXIT = 70
CUDA_FAILURE_RECEIPT = "FLITZIS_CUDA_FAILURE_V1:placement_or_inference"


def is_cuda_failure(returncode: int, stdout: str) -> bool:
    """Require both the dedicated exit and exactly one complete child marker."""
    lines = [line.rstrip("\r") for line in stdout.split("\n")]
    return returncode == CUDA_FAILURE_EXIT and lines.count(CUDA_FAILURE_RECEIPT) == 1
