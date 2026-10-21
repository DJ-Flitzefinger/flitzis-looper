"""Resource stops retain the existing command tree, reader and caller ownership."""

import ctypes
import os
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from ctypes import wintypes
from threading import Event
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.analysis import process
from flitzis_looper.analysis.contracts import WorkerLimits
from flitzis_looper.controller.stem_workers import STEM_QUEUED_JOBS, STEM_WORKERS, StemWorkerPool

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path


def _wait_file(path: Path) -> None:
    deadline = time.monotonic() + 10
    while not path.is_file() or not path.stat().st_size:
        assert time.monotonic() < deadline, "resource fixture failed to become ready"
        Event().wait(0.01)


def _record_processes(monkeypatch: pytest.MonkeyPatch) -> list[process._OwnedTreeProcess]:
    launches: list[process._OwnedTreeProcess] = []
    original = process._OwnedTreeProcess.activate

    def activate(child: process._OwnedTreeProcess) -> None:
        launches.append(child)
        original(child)

    monkeypatch.setattr(process._OwnedTreeProcess, "activate", activate)
    return launches


def _assert_settled(child: process._OwnedTreeProcess, retired: Event) -> None:
    assert retired.is_set()
    assert child.poll() is not None
    assert child.stdout is not None
    assert child.stdout.closed
    assert child.stderr is not None
    assert child.stderr.closed


@pytest.mark.parametrize("returncode", [0, 7])
def test_completed_command_wins_before_fresh_pressure_query(returncode: int) -> None:
    child = Mock(spec=subprocess.Popen)
    child.poll.return_value = returncode
    owned = process._OwnedProcess(child, None, None, Event())
    pressure = Mock(return_value="resource_pressure")
    assert process._monitor(owned, Event(), WorkerLimits(), pressure) == (
        "ok" if returncode == 0 else "worker_crashed"
    )
    pressure.assert_not_called()


def test_user_cancel_wins_before_active_resource_query_or_completion() -> None:
    child = Mock(spec=subprocess.Popen)
    child.poll.return_value = 0
    owned = process._OwnedProcess(child, None, None, Event())
    pressure = Mock(return_value="resource_pressure")
    cancel = Event()
    cancel.set()
    assert process._monitor(owned, cancel, WorkerLimits(), pressure) == "cancelled"
    child.poll.assert_not_called()
    pressure.assert_not_called()


@pytest.mark.parametrize("failure", [OSError, RuntimeError, ValueError])
def test_failed_resource_query_cannot_fabricate_clear_headroom(failure: type[Exception]) -> None:
    pressure = Mock(side_effect=failure("fixture resource query failed"))
    child = Mock(spec=subprocess.Popen)
    child.poll.return_value = None
    owned = process._OwnedProcess(child, None, None, Event())
    assert process._monitor(owned, Event(), WorkerLimits(), pressure) == "resource_unavailable"
    pressure.assert_called_once_with()


def test_unsupported_resource_reason_cannot_look_like_success() -> None:
    assert process._resource_stop_reason(lambda: "ok") == "resource_unavailable"


@pytest.mark.parametrize("reason", ["resource_pressure", "resource_unavailable"])
def test_resource_stop_uses_owned_cooperative_abort_then_retires_both_readers(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, reason: str
) -> None:
    launches = _record_processes(monkeypatch)
    ready, abort, acknowledged = (tmp_path / name for name in ("ready", "abort", "ack"))
    pressure, cancel, retired = Event(), Event(), Event()
    body = (
        "import time\nfrom pathlib import Path\n"
        f"Path({str(ready)!r}).write_text('ready')\n"
        f"while not Path({str(abort)!r}).exists(): time.sleep(0.005)\n"
        f"Path({str(acknowledged)!r}).write_text('cooperative exit')\n"
        "print('output settled')\n"
    )
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(
            process.run_owned_command,
            [sys.executable, "-I", "-c", body],
            cwd=tmp_path,
            env=dict(os.environ),
            cancel=cancel,
            retired=retired,
            request_abort=abort.touch,
            abort_grace_seconds=0.5,
            limits=WorkerLimits(reap_timeout_seconds=0.7),
            stop_reason=lambda: reason if pressure.is_set() else None,
        )
        try:
            _wait_file(ready)
            pressure.set()
            result = future.result(10)
            assert not cancel.is_set()
        finally:
            cancel.set()
            future.result(10)
    assert result.reason == reason
    assert result.returncode == 0
    assert result.stdout.strip() == b"output settled"
    assert acknowledged.read_text() == "cooperative exit"
    _assert_settled(launches[0], retired)


def test_failed_resource_abort_preserves_supervision_failure_after_actual_settlement(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    launches = _record_processes(monkeypatch)
    ready = tmp_path / "ready"
    pressure, cancel, retired = Event(), Event(), Event()
    body = (
        "import time; from pathlib import Path; "
        f"Path({str(ready)!r}).write_text('ready'); time.sleep(60)"
    )
    abort = Mock(side_effect=OSError("fixture abort marker unavailable"))
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(
            process.run_owned_command,
            [sys.executable, "-I", "-c", body],
            cwd=tmp_path,
            env=dict(os.environ),
            cancel=cancel,
            retired=retired,
            request_abort=abort,
            stop_reason=lambda: "resource_pressure" if pressure.is_set() else None,
        )
        try:
            _wait_file(ready)
            pressure.set()
            result = future.result(10)
            assert not cancel.is_set()
        finally:
            cancel.set()
            future.result(10)
    assert result.reason == "worker_supervision_failed"
    abort.assert_called_once_with()
    _assert_settled(launches[0], retired)


@contextmanager
def _observed_process(pid: int) -> Iterator[tuple[ctypes.WinDLL, int]]:
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
    kernel.WaitForSingleObject.restype = wintypes.DWORD
    kernel.CloseHandle.argtypes = (wintypes.HANDLE,)
    kernel.CloseHandle.restype = wintypes.BOOL
    handle = kernel.OpenProcess(0x00100000, 0, pid)
    assert handle, ctypes.WinError(ctypes.get_last_error())
    try:
        yield kernel, int(handle)
    finally:
        assert kernel.CloseHandle(handle)


@pytest.mark.skipif(os.name != "nt", reason="actual Windows owned descendant and pipe retirement")
@pytest.mark.parametrize("pipe", ["silent", "pipe"])
def test_pressure_terminates_only_the_owned_tree_before_releasing_output_custody(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, pipe: str
) -> None:
    launches = _record_processes(monkeypatch)
    ready = tmp_path / "ready"
    pressure, cancel, retired = Event(), Event(), Event()
    body = (
        "import subprocess,sys,time\nfrom pathlib import Path\n"
        f"options = {{}} if {pipe!r} == 'pipe' else "
        "{'stdout': subprocess.DEVNULL, 'stderr': subprocess.DEVNULL}\n"
        "child = subprocess.Popen([sys.executable, '-I', '-c', 'import time; time.sleep(60)'], "
        "creationflags=subprocess.CREATE_NO_WINDOW, **options)\n"
        f"Path({str(ready)!r}).write_text(str(child.pid))\n"
        "time.sleep(60)\n"
    )
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(
            process.run_owned_command,
            [sys.executable, "-I", "-c", body],
            cwd=tmp_path,
            env=dict(os.environ),
            cancel=cancel,
            retired=retired,
            stop_reason=lambda: "resource_pressure" if pressure.is_set() else None,
        )
        try:
            _wait_file(ready)
            with _observed_process(int(ready.read_text())) as (kernel, child_handle):
                assert kernel.WaitForSingleObject(child_handle, 0) == 258
                pressure.set()
                result = future.result(10)
                assert result.reason == "resource_pressure"
                assert not cancel.is_set()
                assert kernel.WaitForSingleObject(child_handle, 0) == 0
                _assert_settled(launches[0], retired)
        finally:
            cancel.set()
            future.result(10)


def test_pressure_retirement_pins_existing_pool_slot_until_the_actual_readers_end(  # noqa: PLR0915
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    drained, allow_read_end, retiring = Event(), Event(), Event()
    pressure, cancel, retired, target_done = Event(), Event(), Event(), Event()
    original_read = process._CommandReader._read
    original_retire = process._retire_later
    results: list[process.OwnedCommandResult] = []
    ready = tmp_path / "ready"

    def read(reader: process._CommandReader) -> None:
        original_read(reader)
        drained.set()
        assert allow_read_end.wait(10)

    def retire(owned: process._OwnedProcess) -> process.ProcessResult:
        result = original_retire(owned)
        retiring.set()
        return result

    def target() -> None:
        try:
            results.append(
                process.run_owned_command(
                    [
                        sys.executable,
                        "-I",
                        "-c",
                        (
                            "import time; from pathlib import Path; "
                            f"Path({str(ready)!r}).write_text('ready'); time.sleep(60)"
                        ),
                    ],
                    cwd=tmp_path,
                    env=dict(os.environ),
                    cancel=cancel,
                    retired=retired,
                    limits=WorkerLimits(reap_timeout_seconds=0.05),
                    abort_grace_seconds=0.0,
                    stop_reason=lambda: "resource_pressure" if pressure.is_set() else None,
                )
            )
        finally:
            target_done.set()

    monkeypatch.setattr(process._CommandReader, "_read", read)
    monkeypatch.setattr(process, "_retire_later", retire)
    pool = StemWorkerPool()
    reservation = pool.try_reserve()
    assert reservation is not None
    held = [pool.try_reserve() for _ in range(STEM_WORKERS + STEM_QUEUED_JOBS - 1)]
    assert all(item is not None for item in held)
    try:
        reservation.submit(target)
        _wait_file(ready)
        pressure.set()
        assert drained.wait(10)
        assert retiring.wait(10)
        assert not retired.is_set()
        assert not target_done.is_set()
        assert pool.try_reserve() is None
        assert not cancel.is_set()
    finally:
        cancel.set()
        allow_read_end.set()
        assert target_done.wait(10)
        deadline = time.monotonic() + 10
        while (returned := pool.try_reserve()) is None:
            assert time.monotonic() < deadline, "actual command retirement did not return its slot"
            Event().wait(0.01)
        returned.close()
        for item in held:
            assert item is not None
            item.close()
        reservation.close()
        pool.shutdown()
    assert retired.is_set()
    assert len(results) == 1
    assert results[0].reason == "resource_pressure"
