"""Explicit cancellation checkpoints for one owned offline inference child."""

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable

CANCEL_PATH_ENV = "FLITZIS_STEM_CANCEL_PATH"
CANCEL_ACK_PREFIX = "FLITZIS_STEM_CANCELLED_V1:"
CANCEL_EXIT_CODE = 130
TOOL_POLL_SECONDS = 0.02


class StemInferenceCancelledError(RuntimeError):
    """An own child observed its supervisor's marker at a named checkpoint."""

    def __init__(self, stage: str) -> None:
        self.stage = stage
        super().__init__(f"Stem inference cancelled at {stage}")


class StemCancellation:
    """Read only the explicit marker belonging to this process invocation."""

    def __init__(self, marker: Path | None) -> None:
        self.marker = marker

    @classmethod
    def from_environment(cls) -> StemCancellation:
        """Capture the own supervisor channel once, without model/source access."""
        marker = os.environ.get(CANCEL_PATH_ENV)
        return cls(Path(marker) if marker else None)

    def checkpoint(self, stage: str) -> None:
        """Cooperate at a Python boundary; active C/neural calls require escalation."""
        if self.marker is not None and self.marker.is_file():
            raise StemInferenceCancelledError(stage)


def check_cancelled(checkpoint: Callable[[str], None] | None, stage: str) -> None:
    """Invoke a supplied own-child checkpoint without changing unbound callers."""
    if checkpoint is not None:
        checkpoint(stage)


def tool_environment() -> dict[str, str]:
    """Keep the explicit inference channel out of subordinate foreign tools."""
    environment = dict(os.environ)
    environment.pop(CANCEL_PATH_ENV, None)
    return environment


def communicate_cancellable(
    process: subprocess.Popen[bytes], checkpoint: Callable[[str], None] | None
) -> tuple[bytes | None, bytes | None]:
    """Poll an exact owned tool handle and reap it before cooperative unwinding."""
    try:
        while True:
            check_cancelled(checkpoint, "input_tool_wait")
            try:
                return process.communicate(timeout=TOOL_POLL_SECONDS)
            except subprocess.TimeoutExpired:
                continue
    except StemInferenceCancelledError:
        process.kill()
        process.communicate()
        raise


def wait_cancellable(
    process: subprocess.Popen[bytes], checkpoint: Callable[[str], None] | None
) -> int:
    """Finish an own encoder, or reap that exact tool before cancellation unwinds."""
    try:
        while True:
            check_cancelled(checkpoint, "encoder_wait")
            try:
                return process.wait(timeout=TOOL_POLL_SECONDS)
            except subprocess.TimeoutExpired:
                continue
    except StemInferenceCancelledError:
        process.kill()
        process.wait()
        raise


def emit_cancellation_ack(error: StemInferenceCancelledError) -> None:
    """Report cooperative settlement after child contexts have unwound."""
    encoded = json.dumps({"stage": error.stage}, separators=(",", ":"))
    sys.stdout.write(CANCEL_ACK_PREFIX + encoded + "\n")
    sys.stdout.flush()
