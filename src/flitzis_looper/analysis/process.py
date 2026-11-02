"""Owned subprocess trees and bounded output supervision for offline workers."""

import os
import subprocess
import sys
import tempfile
import time
from contextlib import contextmanager, suppress
from contextvars import ContextVar
from dataclasses import dataclass
from pathlib import Path
from threading import Event, Semaphore, Thread, get_ident
from typing import IO, TYPE_CHECKING

from flitzis_looper.analysis.contracts import WorkerLimits
from flitzis_looper.analysis.linux_process import LinuxProcess
from flitzis_looper.analysis.windows_job import CREATE_SUSPENDED, WindowsJob
from flitzis_looper.offline_runtime import child_environment
from flitzis_looper_audio import OfflineChildMemoryReservation, OfflineWorkTicket

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

_PROCESS_SLOT = Semaphore(1)
_ANALYSIS_ADMISSION = Semaphore(1)
_POLL_SECONDS = 0.02
_READ_CHUNK_BYTES = 16384


class OfflineWorkCancelledError(RuntimeError):
    """A waiting offline owner was cancelled before acquiring heavy capacity."""


class OfflineWorkAdmissionError(RuntimeError):
    """Native heavy admission failed before this scope began any payload work."""


@dataclass(slots=True)
class _WorkOwner:
    thread: int
    ticket: OfflineWorkTicket
    active: bool = True
    process_slot: bool = False


_WORK_OWNER: ContextVar[_WorkOwner | None] = ContextVar("offline-work-owner", default=None)


@contextmanager
def heavy_work_scope(
    cancel: Event, on_wait: Callable[[], None] | None = None
) -> Iterator[_WorkOwner]:
    """Hold whole-job capacity without taking the subordinate child-process slot."""
    owner = _WORK_OWNER.get()
    if owner is not None and owner.active and owner.thread == get_ident():
        yield owner
        return
    try:
        ticket = OfflineWorkTicket()
    except RuntimeError as error:
        raise OfflineWorkAdmissionError(str(error)) from error
    reported = False
    try:
        while True:
            try:
                acquired = ticket.try_acquire()
            except RuntimeError as error:
                raise OfflineWorkAdmissionError(str(error)) from error
            if acquired:
                break
            if not reported and on_wait is not None:
                on_wait()
                reported = True
            if cancel.wait(_POLL_SECONDS):
                message = "Offline work cancelled before resource admission"
                raise OfflineWorkCancelledError(message)
        if cancel.is_set():
            message = "Offline work cancelled before resource admission"
            raise OfflineWorkCancelledError(message)
        owner = _WorkOwner(get_ident(), ticket)
        token = _WORK_OWNER.set(owner)
        try:
            yield owner
        finally:
            owner.active = False
            _WORK_OWNER.reset(token)
    finally:
        ticket.close()


@contextmanager
def offline_work_scope(cancel: Event, on_wait: Callable[[], None] | None = None) -> Iterator[None]:
    """Acquire whole-job permission BEFORE the child slot; reuse only this owner."""
    with heavy_work_scope(cancel, on_wait) as owner:
        if owner.process_slot:
            yield
            return
        reported = False
        while not _PROCESS_SLOT.acquire(blocking=False):
            if not reported and on_wait is not None:
                on_wait()
                reported = True
            if cancel.wait(_POLL_SECONDS):
                message = "Offline work cancelled before resource admission"
                raise OfflineWorkCancelledError(message)
        owner.process_slot = True
        try:
            if cancel.is_set():
                message = "Offline work cancelled before resource admission"
                raise OfflineWorkCancelledError(message)
            yield
        finally:
            owner.process_slot = False
            _PROCESS_SLOT.release()


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


class _OwnedTreeProcess:
    """Payload-compatible process with independent ownership of its descendants."""

    def __init__(
        self,
        arguments: list[str],
        *,
        cwd: Path,
        env: dict[str, str],
        capture_stderr: bool = False,
        commit_limit_bytes: int | None = None,
    ) -> None:
        if os.name != "nt" and sys.platform != "linux":
            message = f"Offline worker process-tree supervision is unavailable on {sys.platform}"
            raise OSError(message)
        self._job = (
            WindowsJob(
                **(
                    {"commit_limit_bytes": commit_limit_bytes}
                    if commit_limit_bytes is not None
                    else {}
                )
            )
            if os.name == "nt"
            else None
        )
        self._linux: LinuxProcess | None = None
        started = False
        try:
            if sys.platform == "linux":
                self._linux = LinuxProcess(
                    arguments, cwd=cwd, env=env, capture_stderr=capture_stderr
                )
                self._process: subprocess.Popen[bytes] | LinuxProcess = self._linux
            else:
                self._process = subprocess.Popen(
                    arguments,
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE if capture_stderr else subprocess.DEVNULL,
                    cwd=cwd,
                    env=env,
                    creationflags=(
                        subprocess.CREATE_NO_WINDOW | CREATE_SUSPENDED
                        if self._job is not None
                        else 0
                    ),
                )
            self.stdout: IO[bytes] | None = self._process.stdout
            self.stderr: IO[bytes] | None = self._process.stderr
            started = True
        finally:
            if not started and self._job is not None:
                self._job.close()

    @property
    def pid(self) -> int:
        """Expose the gated payload PID to observers, never the Linux broker PID."""
        return self._process.pid

    @property
    def returncode(self) -> int | None:
        """Expose payload status independently of complete descendant retirement."""
        return self._process.returncode

    def poll(self) -> int | None:
        """Observe payload exit without certifying descendant or reader retirement."""
        return self._process.poll()

    def wait(self, timeout: float | None = None) -> int:
        """Wait for the payload; tree and reader settlement remain separate."""
        return self._process.wait(timeout=timeout)

    def activate(self) -> None:
        """Associate and resume only after the supervisor owns teardown on every failure."""
        if self._job is not None:
            self._job.assign_suspended(self.pid)
        elif self._linux is not None:
            self._linux.activate()

    def kill(self) -> None:
        """Terminate the owned tree while retaining its independent retirement proof."""
        if self._job is not None:
            try:
                self._job.terminate()
            finally:
                # Assignment failures leave the newly created root suspended outside
                # the job. Keep its direct Popen handle covered by the same teardown.
                self._process.kill()
        else:
            self._process.kill()

    def tree_active(self) -> bool:
        """Return actual tree ownership after launcher exit, including silent descendants."""
        if self._job is not None:
            return self._job.active()
        if self._linux is not None:
            return self._linux.active()
        return self.poll() is None

    def observe_tree(self) -> None:
        """Retain actual descendant handles while the worker is running."""
        if self._job is not None:
            self._job.observe_members()
        elif self._linux is not None:
            self._linux.poll()

    def close_tree(self) -> None:
        """Release the owning job handle after the supervisor has observed an empty tree."""
        if self._job is not None:
            self._job.close()
        elif self._linux is not None:
            self._linux.close()


class _WorkerProcess(_OwnedTreeProcess):
    """Beat-worker launch adapter; analysis retains its independent global admission."""

    def __init__(self, arguments: list[str], command: WorkerCommand) -> None:
        self._memory_budget = OfflineChildMemoryReservation() if os.name == "nt" else None
        try:
            if self._memory_budget is None:
                super().__init__(arguments, cwd=command.scratch_dir, env=_environment())
            else:
                super().__init__(
                    arguments,
                    cwd=command.scratch_dir,
                    env=_environment(),
                    commit_limit_bytes=self._memory_budget.limit_bytes,
                )
        except OSError, RuntimeError, ValueError:
            if self._memory_budget is not None:
                self._memory_budget.close()
            raise

    def close_tree(self) -> None:
        """Release prospective commit only after all real tree/pipe readers end."""
        super().close_tree()
        if self._memory_budget is not None:
            self._memory_budget.close()


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
        process: subprocess.Popen[bytes] | _OwnedTreeProcess,
        reader: _ResponseReader | None,
        temporary: tempfile.TemporaryDirectory[str] | None,
        retired: Event,
        stderr_reader: _ResponseReader | None = None,
        whole_work: OfflineWorkTicket | None = None,
    ) -> None:
        self.process = process
        self.reader = reader
        self.temporary = temporary
        self.retired = retired
        self.readers = tuple(item for item in (reader, stderr_reader) if item is not None)
        self.whole_work = whole_work

    def add_reader(self, reader: _ResponseReader) -> None:
        """Attach each allocated reader while the suspended process already has an owner."""
        if self.reader is None:
            self.reader = reader
        self.readers += (reader,)

    def release(self) -> bool:
        return _release_process_resources(
            self.process, self.temporary, self.retired, self.whole_work
        )

    def retire(self) -> None:
        # A failed finite reap must retain ownership; never pretend the PCM reader is gone.
        _retire_process_tree(self.process)
        for reader in self.readers:
            if reader.thread.ident is not None:
                reader.thread.join()
        while not self.release():
            # An OS-level file lock must not release the process/temporary-resource slot.
            Event().wait(_POLL_SECONDS)

    def terminate_tree(self) -> None:
        """Also terminate descendants when the launcher already returned success or crashed."""
        _terminate_process_tree(self.process)

    def wait_tree(self, timeout: float) -> bool:
        """Retain admission until the job's active-process count is zero."""
        return _wait_process_tree(self.process, timeout)


def _terminate_process_tree(process: subprocess.Popen[bytes] | _OwnedTreeProcess) -> None:
    if isinstance(process, _OwnedTreeProcess) or process.poll() is None:
        process.kill()


def _wait_process_tree(
    process: subprocess.Popen[bytes] | _OwnedTreeProcess, timeout: float
) -> bool:
    if not isinstance(process, _OwnedTreeProcess):
        return process.poll() is not None
    deadline = time.monotonic() + timeout
    while process.tree_active():
        if time.monotonic() >= deadline:
            return False
        Event().wait(_POLL_SECONDS)
    return True


def _retire_process_tree(process: subprocess.Popen[bytes] | _OwnedTreeProcess) -> None:
    while True:
        try:
            _terminate_process_tree(process)
            process.wait()
            if _wait_process_tree(process, _POLL_SECONDS):
                return
        except OSError:
            pass
        Event().wait(_POLL_SECONDS)


def _release_process_resources(
    process: subprocess.Popen[bytes] | _OwnedTreeProcess,
    temporary: tempfile.TemporaryDirectory[str] | None,
    retired: Event,
    whole_work: OfflineWorkTicket | None,
) -> bool:
    try:
        if isinstance(process, _OwnedTreeProcess):
            process.close_tree()
        for stream in (process.stdout, process.stderr):
            if stream is not None:
                stream.close()
    except OSError:
        return False
    if temporary is None:
        if whole_work is not None:
            whole_work.close()
        retired.set()
        return True
    return _release_temporary(temporary, retired, whole_work)


def _own_process(
    process: subprocess.Popen[bytes] | _OwnedTreeProcess,
    temporary: tempfile.TemporaryDirectory[str] | None,
    retired: Event,
    whole_work: OfflineWorkTicket | None = None,
) -> _OwnedProcess:
    """Retain the returned child even if its supervisor cannot be allocated."""
    try:
        return _OwnedProcess(process, None, temporary, retired, whole_work=whole_work)
    except MemoryError:
        # No output reader has been allocated or started at this boundary. Keep
        # raw custody here instead of trying to allocate a replacement owner.
        _retire_process_tree(process)
        while not _release_process_resources(process, temporary, retired, whole_work):
            Event().wait(_POLL_SECONDS)
        raise


def _environment() -> dict[str, str]:
    env = child_environment(dict(os.environ))
    env["HF_HUB_OFFLINE"] = "1"
    env["TRANSFORMERS_OFFLINE"] = "1"
    return env


def _start(command: WorkerCommand, request_path: Path) -> _OwnedTreeProcess:
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
    if reason is None or reason in {"resource_pressure", "resource_unavailable", "audio_headroom"}:
        return reason
    return "resource_unavailable"


def _pending_stop_reason(
    owned: _OwnedProcess,
    cancel: Event,
    stop_reason: Callable[[], str | None] | None,
    *,
    observe_before_exit: bool,
) -> str | None:
    if cancel.is_set():
        return "cancelled"
    if any(reader.overflow.is_set() for reader in owned.readers):
        return "response_limit"
    if any(reader.failed.is_set() for reader in owned.readers):
        return "response_read_failed"
    return _resource_stop_reason(stop_reason) if observe_before_exit else None


def _completed_reason(
    returncode: int,
    cancel: Event,
    stop_reason: Callable[[], str | None] | None,
    *,
    observe_before_exit: bool,
) -> str:
    if observe_before_exit:
        if cancel.is_set():
            return "cancelled"
        # The peak can change between the periodic query and the poll that sees exit.
        resource_reason = _resource_stop_reason(stop_reason)
        if cancel.is_set():
            return "cancelled"
        if resource_reason is not None:
            return resource_reason
    return "ok" if returncode == 0 else "worker_crashed"


def _monitor(
    owned: _OwnedProcess,
    cancel: Event,
    limits: WorkerLimits,
    stop_reason: Callable[[], str | None] | None = None,
    *,
    observe_before_exit: bool = False,
) -> str:
    deadline = time.monotonic() + limits.timeout_seconds
    while True:
        if isinstance(owned.process, _OwnedTreeProcess):
            owned.process.observe_tree()
        if pending_reason := _pending_stop_reason(
            owned, cancel, stop_reason, observe_before_exit=observe_before_exit
        ):
            return pending_reason
        returncode = owned.process.poll()
        if returncode is not None:
            return _completed_reason(
                returncode, cancel, stop_reason, observe_before_exit=observe_before_exit
            )
        if not observe_before_exit and (resource_reason := _resource_stop_reason(stop_reason)):
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


def _readers_for_command(
    owned: _OwnedProcess,
    limits: WorkerLimits,
    receipt_prefixes: tuple[bytes, ...] | IndependentReceipts,
) -> tuple[_CommandReader, _CommandReader]:
    if isinstance(receipt_prefixes, IndependentReceipts):
        prefixes, independent = receipt_prefixes.prefixes, True
    else:
        prefixes, independent = receipt_prefixes, False
    return _command_readers(owned, limits, prefixes, independent_receipts=independent)


class _ObserverSetupError(Exception):
    """Route an opt-in observer admission failure through the existing owner teardown."""


def _bind_command_observer(
    process: _OwnedTreeProcess, cancel: Event, on_process_started: Callable[[int], None] | None
) -> None:
    if on_process_started is not None:
        try:
            on_process_started(process.pid)
        except OSError, RuntimeError, ValueError:
            reason = "cancelled" if cancel.is_set() else "resource_unavailable"
            raise _ObserverSetupError(reason) from None


def _supervise_command(
    owned: _OwnedProcess,
    process: _OwnedTreeProcess,
    cancel: Event,
    limits: WorkerLimits,
    stop_reason: Callable[[], str | None] | None,
    on_process_started: Callable[[int], None] | None,
    request_abort: Callable[[], None] | None,
    abort_grace_seconds: float,
) -> str:
    _bind_command_observer(process, cancel, on_process_started)
    if on_process_started is None:
        _ = process.pid
    if cancel.is_set():
        return "cancelled"
    process.activate()
    for reader in owned.readers:
        reader.thread.start()
    reason = _monitor(
        owned, cancel, limits, stop_reason, observe_before_exit=on_process_started is not None
    )
    if reason in {"cancelled", "resource_pressure", "resource_unavailable", "audio_headroom"} and (
        request_abort is not None
    ):
        request_abort()
        with suppress(subprocess.TimeoutExpired):
            process.wait(timeout=abort_grace_seconds)
    return reason


# Preserve the existing keyword API and optional observer hook.
def run_owned_command(  # noqa: PLR0913
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
    on_process_started: Callable[[int], None] | None = None,
    commit_limit_bytes: int | None = None,
) -> OwnedCommandResult:
    """Run under caller admission, retaining the worker through actual read end.

    This function creates no executor and acquires no analysis process slot.
    Cancellation and optional resource pressure are observed on the existing
    offline worker. An optional owned child abort hook receives finite grace before
    tree termination. Legacy commands win over a later resource observation; the
    opt-in observer checks before accepting completion.
    A finite reap failure keeps this worker and all caller resources occupied until
    retirement succeeds.
    """
    limits = limits or WorkerLimits(
        timeout_seconds=21600.0,
        reap_timeout_seconds=0.5,
        max_response_bytes=65536,
    )
    if commit_limit_bytes is not None and (
        os.name != "nt"
        or type(commit_limit_bytes) is not int
        or not 0 < commit_limit_bytes <= 2**63 - 1
    ):
        retired.set()
        message = "Invalid or unsupported owned job commit limit"
        raise ValueError(message)
    if not 0 <= abort_grace_seconds <= limits.reap_timeout_seconds:
        msg = "abort grace must fit within the finite reap timeout"
        raise ValueError(msg)
    retired.clear()
    if cancel.is_set():
        retired.set()
        return OwnedCommandResult("cancelled", -1, b"", b"")
    try:
        process = _OwnedTreeProcess(
            arguments,
            cwd=cwd,
            env=child_environment(env),
            capture_stderr=True,
            **(
                {"commit_limit_bytes": commit_limit_bytes} if commit_limit_bytes is not None else {}
            ),
        )
    except OSError:
        retired.set()
        raise
    owned = _own_process(process, None, retired)
    stdout = None
    stderr = None
    reason = "worker_supervision_failed"
    try:
        stdout, stderr = _readers_for_command(owned, limits, receipt_prefixes)
        reason = _supervise_command(
            owned,
            process,
            cancel,
            limits,
            stop_reason,
            on_process_started,
            request_abort,
            abort_grace_seconds,
        )
    except _ObserverSetupError as error:
        reason = str(error)
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
    if on_process_started is not None and cancel.is_set():
        final_reason = "cancelled"
    assert process.returncode is not None
    return OwnedCommandResult(
        final_reason,
        process.returncode,
        stdout.captured() if stdout is not None else b"",
        stderr.captured() if stderr is not None else b"",
    )


def _release_temporary(
    temporary: tempfile.TemporaryDirectory[str],
    retired: Event,
    whole_work: OfflineWorkTicket | None = None,
) -> bool:
    try:
        temporary.cleanup()
    except OSError:
        return False
    _PROCESS_SLOT.release()
    _ANALYSIS_ADMISSION.release()
    if whole_work is not None:
        whole_work.close()
    retired.set()
    return True


def _retire_temporary(
    temporary: tempfile.TemporaryDirectory[str],
    retired: Event,
    whole_work: OfflineWorkTicket | None = None,
) -> None:
    while not _release_temporary(temporary, retired, whole_work):
        Event().wait(_POLL_SECONDS)


def _start_failed(
    temporary: tempfile.TemporaryDirectory[str],
    retired: Event,
    whole_work: OfflineWorkTicket | None = None,
) -> ProcessResult:
    if _release_temporary(temporary, retired, whole_work):
        return ProcessResult("worker_start_failed")
    try:
        Thread(
            target=_retire_temporary,
            args=(temporary, retired, whole_work),
            name="beat-worker-cleanup",
            daemon=True,
        ).start()
    except OSError, RuntimeError, MemoryError:
        _retire_temporary(temporary, retired, whole_work)
        return ProcessResult("worker_start_failed")
    return ProcessResult("worker_retiring", resources_released=False)


def run_process(
    command: WorkerCommand, request: bytes, cancel: Event, limits: WorkerLimits, retired: Event
) -> ProcessResult:
    """Queue one whole owner; a retiring child retains its independent borrow."""
    # Reject an occupied Beat child before waiting on a later heavy ticket.
    # Never hold this subordinate guard during that wait: an earlier native
    # owner may still need to invoke its own Beat branch.
    if not _ANALYSIS_ADMISSION.acquire(blocking=False):
        return ProcessResult("worker_busy")
    _ANALYSIS_ADMISSION.release()
    retired.clear()
    try:
        with heavy_work_scope(cancel) as owner:
            result = _run_process_admitted(command, request, cancel, limits, retired, owner)
            if result.reason == "worker_busy":
                # This invocation owned the pending Event, but launched nothing.
                retired.set()
            return result
    except OfflineWorkCancelledError:
        retired.set()
        return ProcessResult("cancelled")
    except OfflineWorkAdmissionError:
        # This scope never yielded: no child, pipe or temporary owner exists.
        retired.set()
        return ProcessResult("worker_supervision_failed")


def _beat_response_reader(owned: _OwnedProcess, limit: int) -> _ResponseReader:
    if owned.process.stdout is None:
        message = "worker stdout pipe was not created"
        raise RuntimeError(message)
    reader = _ResponseReader(owned.process.stdout, limit)
    owned.add_reader(reader)
    return reader


def _activate_beat_process(
    process: subprocess.Popen[bytes] | _OwnedTreeProcess, cancel: Event
) -> bool:
    """Resolve gated identity before the final pre-execution cancellation check."""
    if isinstance(process, _WorkerProcess):
        _ = process.pid
    if cancel.is_set():
        return False
    if isinstance(process, _WorkerProcess):
        process.activate()
    return True


def _acquire_beat_process_slot(cancel: Event, retired: Event) -> bool:
    """Keep existing Beat admission while waiting for its subordinate process slot."""
    while not _PROCESS_SLOT.acquire(blocking=False):
        if cancel.wait(_POLL_SECONDS):
            retired.set()
            _ANALYSIS_ADMISSION.release()
            return False
    if cancel.is_set():
        retired.set()
        _PROCESS_SLOT.release()
        _ANALYSIS_ADMISSION.release()
        return False
    return True


def _run_process_admitted(
    command: WorkerCommand,
    request: bytes,
    cancel: Event,
    limits: WorkerLimits,
    retired: Event,
    whole_owner: _WorkOwner,
) -> ProcessResult:
    """Run one process globally; a retiring process continues to occupy its slot.

    The caller must run this on its background supervisor. It owns the borrowed PCM
    until ``retired`` is set. The existing global admission owns the wait; no process is launched
    while a previous process is active or retiring.
    """
    if not _ANALYSIS_ADMISSION.acquire(blocking=False):
        return ProcessResult("worker_busy")
    retired.clear()
    if not _acquire_beat_process_slot(cancel, retired):
        return ProcessResult("cancelled")
    temporary = None
    try:
        temporary = tempfile.TemporaryDirectory(prefix="beat-request-", dir=command.scratch_dir)
    except OSError:
        return ProcessResult("worker_scratch_unavailable")
    finally:
        if temporary is None:
            retired.set()
            _PROCESS_SLOT.release()
            _ANALYSIS_ADMISSION.release()
    whole_work = None
    try:
        request_path = Path(temporary.name) / "request.json"
        whole_work = whole_owner.ticket.borrow()
        request_path.write_bytes(request)
    except MemoryError:
        _start_failed(temporary, retired, whole_work)
        raise
    except OSError, RuntimeError:
        return _start_failed(temporary, retired, whole_work)
    try:
        process = _start(command, request_path)
    except OSError, RuntimeError:
        return _start_failed(temporary, retired, whole_work)
    owned = _own_process(process, temporary, retired, whole_work)
    reason = "worker_supervision_failed"
    try:
        reader = _beat_response_reader(owned, limits.max_response_bytes)
        if _activate_beat_process(process, cancel):
            reader.thread.start()
            reason = _monitor(owned, cancel, limits)
        else:
            reason = "cancelled"
    except OSError, RuntimeError:
        reason = "worker_supervision_failed"
    finally:
        outcome = _finish(owned, reason, limits)
    return outcome
