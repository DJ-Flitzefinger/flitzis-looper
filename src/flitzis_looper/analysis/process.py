"""Owned subprocess trees and bounded output supervision for offline workers."""

import os
import subprocess
import tempfile
import time
from contextlib import suppress
from dataclasses import dataclass
from pathlib import Path
from threading import Event, Semaphore, Thread
from typing import IO, TYPE_CHECKING

from flitzis_looper.analysis.contracts import WorkerLimits
from flitzis_looper.analysis.windows_job import CREATE_SUSPENDED, WindowsJob

if TYPE_CHECKING:
    from collections.abc import Callable

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


@dataclass(frozen=True, slots=True)
class OwnedCommandResult:
    """Bounded command output returned only after its actual tree and readers retire."""

    reason: str
    returncode: int
    stdout: bytes
    stderr: bytes


@dataclass(frozen=True, slots=True)
class IndependentReceipts:
    """Opt in to bounded per-prefix receipts without changing legacy tuple behavior."""

    prefixes: tuple[bytes, ...]


class _OwnedTreeProcess(subprocess.Popen[bytes]):
    """Popen-compatible process with explicit ownership of Windows descendants."""

    def __init__(
        self,
        arguments: list[str],
        *,
        cwd: Path,
        env: dict[str, str],
        capture_stderr: bool = False,
    ) -> None:
        self._job = WindowsJob() if os.name == "nt" else None
        started = False
        try:
            super().__init__(
                arguments,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE if capture_stderr else subprocess.DEVNULL,
                cwd=cwd,
                env=env,
                creationflags=(
                    subprocess.CREATE_NO_WINDOW | CREATE_SUSPENDED if self._job is not None else 0
                ),
            )
            started = True
        finally:
            if not started and self._job is not None:
                self._job.close()

    def activate(self) -> None:
        """Associate and resume only after the supervisor owns teardown on every failure."""
        if self._job is not None:
            self._job.assign_suspended(self.pid)

    def kill(self) -> None:
        """Terminate the whole owned Windows job, or the directly owned POSIX process."""
        if self._job is not None:
            try:
                self._job.terminate()
            finally:
                # Assignment failures leave the newly created root suspended outside
                # the job. Keep its direct Popen handle covered by the same teardown.
                super().kill()
        else:
            super().kill()

    def tree_active(self) -> bool:
        """Return actual tree ownership after launcher exit, including silent descendants."""
        return self._job.active() if self._job is not None else self.poll() is None

    def observe_tree(self) -> None:
        """Retain actual descendant handles while the worker is running."""
        if self._job is not None:
            self._job.observe_members()

    def close_tree(self) -> None:
        """Release the owning job handle after the supervisor has observed an empty tree."""
        if self._job is not None:
            self._job.close()


class _WorkerProcess(_OwnedTreeProcess):
    """Beat-worker launch adapter; analysis retains its independent global admission."""

    def __init__(self, arguments: list[str], command: WorkerCommand) -> None:
        super().__init__(arguments, cwd=command.scratch_dir, env=_environment())


class _ResponseReader:
    def __init__(
        self, stream: IO[bytes], limit: int, *, thread_name: str = "beat-worker-output"
    ) -> None:
        self.stream = stream
        self.limit = limit
        self.data = bytearray()
        self.failed = Event()
        self.overflow = Event()
        self.thread = Thread(target=self._read, name=thread_name, daemon=True)

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


class _CommandReader(_ResponseReader):
    """Drain arbitrary diagnostics while retaining a finite tail and receipt evidence."""

    def __init__(
        self,
        stream: IO[bytes],
        limit: int,
        receipt_prefixes: tuple[bytes, ...] = (),
        *,
        independent_receipts: bool = False,
    ) -> None:
        super().__init__(stream, limit, thread_name="owned-command-output")
        self.receipt_prefixes = receipt_prefixes
        self._line = bytearray()
        self._line_truncated = False
        self._receipt_count = 0
        self._receipt: bytes | None = None
        self.independent_receipts = independent_receipts
        self._receipts: dict[bytes, tuple[int, bytes | None]] = {}

    def _read(self) -> None:
        try:
            while chunk := self.stream.read(_READ_CHUNK_BYTES):
                self.data.extend(chunk)
                del self.data[: max(0, len(self.data) - self.limit)]
                if self.receipt_prefixes:
                    self._observe_lines(chunk)
            self._finish_line()
        except OSError:
            self.failed.set()

    def _observe_lines(self, chunk: bytes) -> None:
        for index, part in enumerate(chunk.split(b"\n")):
            if index:
                self._finish_line()
            remaining = 1001 - len(self._line)
            self._line.extend(part[:remaining])
            self._line_truncated |= len(part) > remaining

    def _finish_line(self) -> None:
        line = bytes(self._line).rstrip(b"\r")
        if self.independent_receipts:
            for prefix in self.receipt_prefixes:
                if line.startswith(prefix):
                    count, _ = self._receipts.get(prefix, (0, None))
                    retained = line if not self._line_truncated and len(line) <= 1000 else None
                    self._receipts[prefix] = (min(2, count + 1), retained)
        elif line.startswith(self.receipt_prefixes):
            self._receipt_count = min(2, self._receipt_count + 1)
            if not self._line_truncated and len(line) <= 1000:
                self._receipt = line
        self._line.clear()
        self._line_truncated = False

    def captured(self) -> bytes:
        """Preserve receipt validity even when its original line left the bounded tail."""
        if not self.receipt_prefixes:
            return bytes(self.data)
        diagnostics = b"\n".join(
            line for line in self.data.splitlines() if not line.startswith(self.receipt_prefixes)
        )
        if self.independent_receipts:
            retained = []
            for prefix in self.receipt_prefixes:
                count, receipt = self._receipts.get(prefix, (0, None))
                if count == 1 and receipt is not None:
                    retained.append(receipt)
                elif count:
                    retained.extend([prefix + b"invalid receipt"] * 2)
            return b"\n".join([diagnostics, *retained])
        if self._receipt_count == 1 and self._receipt is not None:
            return diagnostics + b"\n" + self._receipt
        if self._receipt_count:
            # Two bounded marker lines retain invalidity for callers that require
            # exactly one receipt. Never turn an oversized/duplicate line valid.
            invalid = self.receipt_prefixes[0] + b"invalid receipt"
            return diagnostics + b"\n" + invalid + b"\n" + invalid
        return diagnostics


class _OwnedProcess:
    def __init__(
        self,
        process: subprocess.Popen[bytes],
        reader: _ResponseReader | None,
        temporary: tempfile.TemporaryDirectory[str] | None,
        retired: Event,
        stderr_reader: _ResponseReader | None = None,
    ) -> None:
        self.process = process
        self.reader = reader
        self.temporary = temporary
        self.retired = retired
        self.readers = tuple(item for item in (reader, stderr_reader) if item is not None)

    def add_reader(self, reader: _ResponseReader) -> None:
        """Attach each allocated reader while the suspended process already has an owner."""
        if self.reader is None:
            self.reader = reader
        self.readers += (reader,)

    def release(self) -> bool:
        try:
            if isinstance(self.process, _OwnedTreeProcess):
                self.process.close_tree()
            for stream in (self.process.stdout, self.process.stderr):
                if stream is not None:
                    stream.close()
        except OSError:
            return False
        if self.temporary is None:
            self.retired.set()
            return True
        return _release_temporary(self.temporary, self.retired)

    def retire(self) -> None:
        # A failed finite reap must retain ownership; never pretend the PCM reader is gone.
        while True:
            try:
                self.terminate_tree()
                self.process.wait()
                if self.wait_tree(_POLL_SECONDS):
                    break
            except OSError:
                pass
            Event().wait(_POLL_SECONDS)
        for reader in self.readers:
            if reader.thread.ident is not None:
                reader.thread.join()
        while not self.release():
            # An OS-level file lock must not release the process/temporary-resource slot.
            Event().wait(_POLL_SECONDS)

    def terminate_tree(self) -> None:
        """Also terminate descendants when the launcher already returned success or crashed."""
        if isinstance(self.process, _OwnedTreeProcess) or self.process.poll() is None:
            self.process.kill()

    def wait_tree(self, timeout: float) -> bool:
        """Retain admission until the job's active-process count is zero."""
        if not isinstance(self.process, _OwnedTreeProcess):
            return self.process.poll() is not None
        deadline = time.monotonic() + timeout
        while self.process.tree_active():
            if time.monotonic() >= deadline:
                return False
            Event().wait(_POLL_SECONDS)
        return True


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
    return _WorkerProcess(
        [
            str(command.interpreter),
            "-I",
            str(command.script),
            "--request",
            str(request_path),
            "--checkpoint",
            str(command.checkpoint),
        ],
        command,
    )


def _resource_stop_reason(stop_reason: Callable[[], str | None] | None) -> str | None:
    if stop_reason is None:
        return None
    try:
        reason = stop_reason()
    except OSError, RuntimeError, ValueError:
        return "resource_unavailable"
    if reason is None or reason in {"resource_pressure", "resource_unavailable"}:
        return reason
    return "resource_unavailable"


def _monitor(
    owned: _OwnedProcess,
    cancel: Event,
    limits: WorkerLimits,
    stop_reason: Callable[[], str | None] | None = None,
) -> str:
    deadline = time.monotonic() + limits.timeout_seconds
    while True:
        if isinstance(owned.process, _OwnedTreeProcess):
            owned.process.observe_tree()
        if cancel.is_set():
            return "cancelled"
        if any(reader.overflow.is_set() for reader in owned.readers):
            return "response_limit"
        if any(reader.failed.is_set() for reader in owned.readers):
            return "response_read_failed"
        returncode = owned.process.poll()
        if returncode is not None:
            return "ok" if returncode == 0 else "worker_crashed"
        if resource_reason := _resource_stop_reason(stop_reason):
            return resource_reason
        if time.monotonic() >= deadline:
            return "worker_timeout"
        cancel.wait(_POLL_SECONDS)


def _finish(owned: _OwnedProcess, reason: str, limits: WorkerLimits) -> ProcessResult:
    try:
        owned.terminate_tree()
        owned.process.wait(timeout=limits.reap_timeout_seconds)
        if not owned.wait_tree(limits.reap_timeout_seconds):
            return _retire_later(owned)
    except OSError, subprocess.TimeoutExpired:
        return _retire_later(owned)
    for reader in owned.readers:
        if reader.thread.ident is not None:
            reader.thread.join(timeout=limits.reap_timeout_seconds)
        if reader.thread.is_alive():
            return _retire_later(owned)
    if any(reader.overflow.is_set() for reader in owned.readers):
        reason = "response_limit"
    elif any(reader.failed.is_set() for reader in owned.readers):
        reason = "response_read_failed"
    response = bytes(owned.reader.data) if reason == "ok" and owned.reader is not None else b""
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


def _command_readers(
    owned: _OwnedProcess,
    limits: WorkerLimits,
    receipt_prefixes: tuple[bytes, ...],
    *,
    independent_receipts: bool = False,
) -> tuple[_CommandReader, _CommandReader]:
    if owned.process.stdout is None or owned.process.stderr is None:
        msg = "owned command output pipes were not created"
        raise RuntimeError(msg)
    stdout = _CommandReader(
        owned.process.stdout,
        limits.max_response_bytes,
        receipt_prefixes,
        independent_receipts=independent_receipts,
    )
    owned.add_reader(stdout)
    stderr = _CommandReader(owned.process.stderr, limits.max_response_bytes)
    owned.add_reader(stderr)
    return stdout, stderr


def run_owned_command(
    arguments: list[str],
    *,
    cwd: Path,
    env: dict[str, str],
    cancel: Event,
    retired: Event,
    limits: WorkerLimits | None = None,
    receipt_prefixes: tuple[bytes, ...] | IndependentReceipts = (),
    request_abort: Callable[[], None] | None = None,
    abort_grace_seconds: float = 0.2,
    stop_reason: Callable[[], str | None] | None = None,
) -> OwnedCommandResult:
    """Run under caller admission, retaining the worker through actual read end.

    This function creates no executor and acquires no analysis process slot.
    Cancellation and optional resource pressure are observed on the existing
    offline worker. An optional owned child abort hook receives finite grace before
    tree termination. Completed commands win over a later resource observation.
    A finite reap failure keeps this worker and all caller resources occupied until
    retirement succeeds.
    """
    limits = limits or WorkerLimits(
        timeout_seconds=21600.0,
        reap_timeout_seconds=0.5,
        max_response_bytes=65536,
    )
    if not 0 <= abort_grace_seconds <= limits.reap_timeout_seconds:
        msg = "abort grace must fit within the finite reap timeout"
        raise ValueError(msg)
    retired.clear()
    if cancel.is_set():
        retired.set()
        return OwnedCommandResult("cancelled", -1, b"", b"")
    try:
        process = _OwnedTreeProcess(arguments, cwd=cwd, env=env, capture_stderr=True)
    except OSError:
        retired.set()
        raise
    owned = _OwnedProcess(process, None, None, retired)
    stdout = None
    stderr = None
    reason = "worker_supervision_failed"
    try:
        if isinstance(receipt_prefixes, IndependentReceipts):
            prefixes, independent = receipt_prefixes.prefixes, True
        else:
            prefixes, independent = receipt_prefixes, False
        stdout, stderr = _command_readers(owned, limits, prefixes, independent_receipts=independent)
        process.activate()
        for reader in owned.readers:
            reader.thread.start()
        reason = _monitor(owned, cancel, limits, stop_reason)
        if reason in {"cancelled", "resource_pressure", "resource_unavailable"} and (
            request_abort is not None
        ):
            request_abort()
            with suppress(subprocess.TimeoutExpired):
                process.wait(timeout=abort_grace_seconds)
    except OSError, RuntimeError:
        reason = "worker_supervision_failed"
    finally:
        outcome = _finish(owned, reason, limits)
        if not outcome.resources_released:
            # Do not leave a backend's temporary/source context under a surviving
            # child or output read. The already admitted worker retains its slot.
            retired.wait()
    final_reason = reason if outcome.reason == "worker_retiring" else outcome.reason
    if any(reader.failed.is_set() for reader in owned.readers):
        final_reason = "response_read_failed"
    assert process.returncode is not None
    return OwnedCommandResult(
        final_reason,
        process.returncode,
        stdout.captured() if stdout is not None else b"",
        stderr.captured() if stderr is not None else b"",
    )


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
        if isinstance(process, _WorkerProcess):
            process.activate()
        reader.thread.start()
        reason = _monitor(owned, cancel, limits)
    except OSError, RuntimeError:
        reason = "worker_supervision_failed"
    return _finish(owned, reason, limits)
