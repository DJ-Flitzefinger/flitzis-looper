"""Bounded subprocess supervision, used only by an offline analysis owner."""

import os
import subprocess
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from threading import Event, Semaphore, Thread
from typing import IO, TYPE_CHECKING

if TYPE_CHECKING:
    from flitzis_looper.analysis.contracts import WorkerLimits

_PROCESS_SLOT = Semaphore(1)
_POLL_SECONDS = 0.02
_READ_CHUNK_BYTES = 16384


@dataclass(frozen=True, slots=True)
class ProcessResult:
    """Bounded worker bytes or a failure, with actual process ownership state."""

    reason: str
    response: bytes = b""
    resources_released: bool = True


@dataclass(frozen=True, slots=True)
class WorkerCommand:
    """Explicit local paths; never resolve/download packages or model shortnames."""

    interpreter: Path
    script: Path
    checkpoint: Path
    scratch_dir: Path


class _ResponseReader:
    def __init__(self, stream: IO[bytes], limit: int) -> None:
        self.stream = stream
        self.limit = limit
        self.data = bytearray()
        self.failed = Event()
        self.overflow = Event()
        self.thread = Thread(target=self._read, name="beat-worker-output", daemon=True)

    def _read(self) -> None:
        try:
            while chunk := self.stream.read(
                min(_READ_CHUNK_BYTES, self.limit + 1 - len(self.data))
            ):
                self.data.extend(chunk)
                if len(self.data) > self.limit:
                    self.overflow.set()
                    break
        except OSError:
            self.failed.set()


class _OwnedProcess:
    def __init__(
        self,
        process: subprocess.Popen[bytes],
        reader: _ResponseReader,
        temporary: tempfile.TemporaryDirectory[str],
        retired: Event,
    ) -> None:
        self.process = process
        self.reader = reader
        self.temporary = temporary
        self.retired = retired

    def release(self) -> bool:
        try:
            self.reader.stream.close()
        except OSError:
            return False
        return _release_temporary(self.temporary, self.retired)

    def retire(self) -> None:
        # A failed finite reap must retain ownership; never pretend the PCM reader is gone.
        while True:
            try:
                self.process.wait()
                break
            except OSError:
                Event().wait(_POLL_SECONDS)
        if self.reader.thread.ident is not None:
            self.reader.thread.join()
        while not self.release():
            # An OS-level file lock must not release the process/temporary-resource slot.
            Event().wait(_POLL_SECONDS)


def _environment() -> dict[str, str]:
    env = dict(os.environ)
    for name in (
        "OMP_NUM_THREADS",
        "MKL_NUM_THREADS",
        "OPENBLAS_NUM_THREADS",
        "NUMEXPR_NUM_THREADS",
    ):
        env[name] = "1"
    env["HF_HUB_OFFLINE"] = "1"
    env["TRANSFORMERS_OFFLINE"] = "1"
    return env


def _start(command: WorkerCommand, request_path: Path) -> subprocess.Popen[bytes]:
    return subprocess.Popen(
        [
            str(command.interpreter),
            "-I",
            str(command.script),
            "--request",
            str(request_path),
            "--checkpoint",
            str(command.checkpoint),
        ],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        cwd=command.scratch_dir,
        env=_environment(),
        creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
    )


def _monitor(owned: _OwnedProcess, cancel: Event, limits: WorkerLimits) -> str:
    deadline = time.monotonic() + limits.timeout_seconds
    while True:
        if cancel.is_set():
            return "cancelled"
        if owned.reader.overflow.is_set():
            return "response_limit"
        if owned.reader.failed.is_set():
            return "response_read_failed"
        returncode = owned.process.poll()
        if returncode is not None:
            return "ok" if returncode == 0 else "worker_crashed"
        if time.monotonic() >= deadline:
            return "worker_timeout"
        cancel.wait(_POLL_SECONDS)


def _finish(owned: _OwnedProcess, reason: str, limits: WorkerLimits) -> ProcessResult:
    if owned.process.poll() is None:
        try:
            owned.process.kill()
        except OSError:
            return _retire_later(owned)
    try:
        owned.process.wait(timeout=limits.reap_timeout_seconds)
    except OSError, subprocess.TimeoutExpired:
        return _retire_later(owned)
    if owned.reader.thread.ident is not None:
        owned.reader.thread.join(timeout=limits.reap_timeout_seconds)
    if owned.reader.thread.is_alive():
        return _retire_later(owned)
    if owned.reader.overflow.is_set():
        reason = "response_limit"
    elif owned.reader.failed.is_set():
        reason = "response_read_failed"
    response = bytes(owned.reader.data) if reason == "ok" else b""
    if not owned.release():
        return _retire_later(owned)
    return ProcessResult(reason, response)


def _retire_later(owned: _OwnedProcess) -> ProcessResult:
    try:
        Thread(target=owned.retire, name="beat-worker-retiring", daemon=True).start()
    except OSError, RuntimeError:
        # This caller already is the offline supervisor. Retain its ownership until
        # teardown finishes if the OS cannot allocate the fallback retirement thread.
        owned.retire()
        return ProcessResult("worker_supervision_failed")
    return ProcessResult("worker_retiring", resources_released=False)


def _release_temporary(temporary: tempfile.TemporaryDirectory[str], retired: Event) -> bool:
    try:
        temporary.cleanup()
    except OSError:
        return False
    retired.set()
    _PROCESS_SLOT.release()
    return True


def _retire_temporary(temporary: tempfile.TemporaryDirectory[str], retired: Event) -> None:
    while not _release_temporary(temporary, retired):
        Event().wait(_POLL_SECONDS)


def _start_failed(temporary: tempfile.TemporaryDirectory[str], retired: Event) -> ProcessResult:
    if _release_temporary(temporary, retired):
        return ProcessResult("worker_start_failed")
    try:
        Thread(
            target=_retire_temporary,
            args=(temporary, retired),
            name="beat-worker-cleanup",
            daemon=True,
        ).start()
    except OSError, RuntimeError:
        _retire_temporary(temporary, retired)
        return ProcessResult("worker_start_failed")
    return ProcessResult("worker_retiring", resources_released=False)


def run_process(
    command: WorkerCommand, request: bytes, cancel: Event, limits: WorkerLimits, retired: Event
) -> ProcessResult:
    """Run one process globally; a retiring process continues to occupy its slot.

    The caller must run this on its background supervisor. It owns the borrowed PCM
    until ``retired`` is set. No queue is maintained and no extra process is launched
    while a previous process is active or retiring.
    """
    if not _PROCESS_SLOT.acquire(blocking=False):
        return ProcessResult("worker_busy")
    retired.clear()
    try:
        temporary = tempfile.TemporaryDirectory(prefix="beat-request-", dir=command.scratch_dir)
    except OSError:
        retired.set()
        _PROCESS_SLOT.release()
        return ProcessResult("worker_scratch_unavailable")
    try:
        request_path = Path(temporary.name) / "request.json"
        request_path.write_bytes(request)
        process = _start(command, request_path)
    except OSError:
        return _start_failed(temporary, retired)
    if process.stdout is None:
        msg = "worker stdout pipe was not created"
        raise RuntimeError(msg)
    reader = _ResponseReader(process.stdout, limits.max_response_bytes)
    owned = _OwnedProcess(process, reader, temporary, retired)
    try:
        reader.thread.start()
        reason = _monitor(owned, cancel, limits)
    except OSError, RuntimeError:
        reason = "worker_supervision_failed"
    return _finish(owned, reason, limits)
