"""Actual owned command/tree/output settlement without model inference."""

import ctypes
import os
import subprocess
import sys
import time
import venv
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from ctypes import wintypes
from pathlib import Path
from threading import Event, Thread
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import process
from flitzis_looper.analysis.contracts import WorkerLimits
from flitzis_looper.analysis.windows_job import WindowsJob

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator


def _wait_file(path: Path) -> None:
    deadline = time.monotonic() + 10
    while not path.is_file() or not path.stat().st_size:
        assert time.monotonic() < deadline, "owned fixture failed to become ready"
        Event().wait(0.01)


def _run(
    args: list[str],
    tmp_path: Path,
    *,
    limits: WorkerLimits | None = None,
    receipt_prefixes: tuple[bytes, ...] = (),
) -> process.OwnedCommandResult:
    return process.run_owned_command(
        args,
        cwd=tmp_path,
        env=dict(os.environ),
        cancel=Event(),
        retired=Event(),
        limits=limits,
        receipt_prefixes=receipt_prefixes,
    )


def _record_processes(monkeypatch: pytest.MonkeyPatch) -> list[process._OwnedTreeProcess]:
    launches: list[process._OwnedTreeProcess] = []
    original = process._OwnedTreeProcess.__init__

    def create(
        child: process._OwnedTreeProcess,
        arguments: list[str],
        *,
        cwd: Path,
        env: dict[str, str],
        capture_stderr: bool = False,
    ) -> None:
        original(child, arguments, cwd=cwd, env=env, capture_stderr=capture_stderr)
        launches.append(child)

    monkeypatch.setattr(process._OwnedTreeProcess, "__init__", create)
    return launches


@pytest.mark.parametrize("failure_index", [1, 2])
def test_output_reader_construction_failure_keeps_suspended_root_owned_until_reaped(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, failure_index: int
) -> None:
    launches = _record_processes(monkeypatch)
    real_thread = Thread
    constructed = 0
    retired = Event()
    marker = tmp_path / "must-not-run"

    def thread(*, target: Callable[[], None], name: str, daemon: bool) -> Thread:
        nonlocal constructed
        if name == "owned-command-output":
            constructed += 1
            if constructed == failure_index:
                msg = "fixture reader constructor failure"
                raise RuntimeError(msg)
        return real_thread(target=target, name=name, daemon=daemon)

    monkeypatch.setattr(process, "Thread", thread)
    result = process.run_owned_command(
        [sys.executable, "-I", "-c", f"from pathlib import Path; Path({str(marker)!r}).touch()"],
        cwd=tmp_path,
        env=dict(os.environ),
        cancel=Event(),
        retired=retired,
    )
    assert result.reason == "worker_supervision_failed"
    assert retired.is_set()
    assert launches[0].poll() is not None
    assert launches[0].stdout is not None
    assert launches[0].stdout.closed
    assert launches[0].stderr is not None
    assert launches[0].stderr.closed
    if os.name == "nt":
        assert not marker.exists()


def test_stderr_reader_failure_triggers_visible_failure_and_reaps_child(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    launches = _record_processes(monkeypatch)
    original = process._CommandReader._read

    def read(reader: process._CommandReader) -> None:
        if reader.stream is launches[0].stderr:
            reader.failed.set()
        else:
            original(reader)

    monkeypatch.setattr(process._CommandReader, "_read", read)
    result = _run(
        [
            sys.executable,
            "-I",
            "-c",
            "import sys,time; sys.stderr.write('x' * 1000000); time.sleep(60)",
        ],
        tmp_path,
    )
    assert result.reason == "response_read_failed"
    assert launches[0].poll() is not None
    assert launches[0].stdout is not None
    assert launches[0].stdout.closed
    assert launches[0].stderr is not None
    assert launches[0].stderr.closed


def test_stdout_stderr_flood_drains_with_bounded_tail_and_early_receipt(tmp_path: Path) -> None:
    prefix = b"OWNED-RECEIPT:"
    body = (
        "import sys\n"
        "print('OWNED-RECEIPT:verified', flush=True)\n"
        "sys.stdout.write('diagnostic\\n' * 40000)\n"
        "sys.stderr.write('progress\\r' * 40000 + 'last error')\n"
    )
    result = _run(
        [sys.executable, "-I", "-c", body],
        tmp_path,
        limits=WorkerLimits(max_response_bytes=4096),
        receipt_prefixes=(prefix,),
    )
    assert result.reason == "ok"
    assert result.returncode == 0
    assert len(result.stdout) <= 4096 + 1001
    assert len(result.stderr) <= 4096
    assert result.stdout.count(prefix) == 1
    assert result.stdout.endswith(b"OWNED-RECEIPT:verified")
    assert result.stderr.endswith(b"last error")


@pytest.mark.parametrize("receipt", ["ONE\\nONE", "x" * 1001])
def test_tail_trimming_cannot_make_duplicate_or_oversized_receipt_valid(
    tmp_path: Path, receipt: str
) -> None:
    lines = "\n".join("OWNED-RECEIPT:" + part for part in receipt.split("\\n"))
    body = f"import sys; print({lines!r}); sys.stdout.write('log\\n' * 40000)"
    result = _run(
        [sys.executable, "-I", "-c", body],
        tmp_path,
        limits=WorkerLimits(max_response_bytes=4096),
        receipt_prefixes=(b"OWNED-RECEIPT:",),
    )
    assert result.reason == "ok"
    receipts = [line for line in result.stdout.splitlines() if line.startswith(b"OWNED-RECEIPT:")]
    assert len(receipts) == 2


def test_reader_end_retains_caller_after_finite_reap_until_actual_reader_returns(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    reader_drained = Event()
    allow_read_end = Event()
    retired = Event()
    original = process._CommandReader._read

    def read(reader: process._CommandReader) -> None:
        original(reader)
        reader_drained.set()
        assert allow_read_end.wait(10)

    monkeypatch.setattr(process._CommandReader, "_read", read)
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(
            process.run_owned_command,
            [sys.executable, "-I", "-c", "print('read fixture')"],
            cwd=tmp_path,
            env=dict(os.environ),
            cancel=Event(),
            retired=retired,
            limits=WorkerLimits(reap_timeout_seconds=0.05),
            abort_grace_seconds=0.0,
        )
        try:
            assert reader_drained.wait(10)
            assert not retired.is_set()
            assert not future.done()
        finally:
            allow_read_end.set()
        assert future.result(10).reason == "ok"
    assert retired.is_set()


def test_failed_finite_wait_keeps_caller_until_actual_background_reap(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    reaping = Event()
    allow_reap = Event()
    retired = Event()
    original = process._OwnedTreeProcess.wait

    def wait(child: process._OwnedTreeProcess, timeout: float | None = None) -> int:
        if timeout is not None:
            command_name = "owned command fixture"
            raise subprocess.TimeoutExpired(command_name, timeout)
        reaping.set()
        assert allow_reap.wait(10)
        return original(child)

    monkeypatch.setattr(process._OwnedTreeProcess, "wait", wait)
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(
            process.run_owned_command,
            [sys.executable, "-I", "-c", "print('reap fixture')"],
            cwd=tmp_path,
            env=dict(os.environ),
            cancel=Event(),
            retired=retired,
            limits=WorkerLimits(reap_timeout_seconds=0.05),
            abort_grace_seconds=0.0,
        )
        try:
            assert reaping.wait(10)
            assert not retired.is_set()
            assert not future.done()
        finally:
            allow_reap.set()
        assert future.result(10).reason == "ok"
    assert retired.is_set()


def _blocked_body(ready: Path, release: Path) -> str:
    return (
        "import time\nfrom pathlib import Path\n"
        f"Path({str(ready)!r}).write_text('ready')\n"
        f"while not Path({str(release)!r}).exists(): time.sleep(0.005)\n"
        "print('{}')\n"
    )


def test_analysis_slot_and_two_owned_commands_have_independent_admission(tmp_path: Path) -> None:
    beat_ready = tmp_path / "beat.ready"
    first_ready = tmp_path / "first.ready"
    second_ready = tmp_path / "second.ready"
    release = tmp_path / "release"
    script = tmp_path / "beat.py"
    script.write_text(_blocked_body(beat_ready, release), encoding="utf-8")
    command = process.WorkerCommand(Path(sys.executable), script, tmp_path / "unused", tmp_path)
    cancel = Event()
    beat_retired = Event()
    with ThreadPoolExecutor(max_workers=3) as executor:
        beat = executor.submit(
            process.run_process, command, b"{}", cancel, WorkerLimits(), beat_retired
        )
        first = executor.submit(
            _run,
            [sys.executable, "-I", "-c", _blocked_body(first_ready, release)],
            tmp_path,
        )
        second = executor.submit(
            _run,
            [sys.executable, "-I", "-c", _blocked_body(second_ready, release)],
            tmp_path,
        )
        try:
            for ready in (beat_ready, first_ready, second_ready):
                _wait_file(ready)
            assert not beat_retired.is_set()
            assert not first.done()
            assert not second.done()
            busy = process.run_process(command, b"{}", Event(), WorkerLimits(), Event())
            assert busy.reason == "worker_busy"
        finally:
            release.touch()
            cancel.set()
        assert beat.result(10).resources_released
        assert first.result(10).reason == "ok"
        assert second.result(10).reason == "ok"


def test_optional_owned_abort_hook_can_settle_child_before_escalation(tmp_path: Path) -> None:
    ready = tmp_path / "ready"
    abort = tmp_path / "abort"
    acknowledged = tmp_path / "acknowledged"
    body = (
        "import time\nfrom pathlib import Path\n"
        f"Path({str(ready)!r}).write_text('ready')\n"
        f"while not Path({str(abort)!r}).exists(): time.sleep(0.005)\n"
        f"Path({str(acknowledged)!r}).write_text('cooperative exit')\n"
    )
    cancel = Event()
    retired = Event()
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
        )
        try:
            _wait_file(ready)
        finally:
            cancel.set()
        result = future.result(10)
    assert result.reason == "cancelled"
    assert result.returncode == 0
    assert retired.is_set()
    assert acknowledged.read_text() == "cooperative exit"


def test_unexpected_abort_hook_failure_still_retires_actual_command(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    ready, release = (tmp_path / name for name in ("ready", "never-release"))
    cancel = Event()
    retired = Event()
    launches = _record_processes(monkeypatch)

    def fail_abort() -> None:
        msg = "fixture unexpected abort error"
        raise ValueError(msg)

    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(
            process.run_owned_command,
            [sys.executable, "-I", "-c", _blocked_body(ready, release)],
            cwd=tmp_path,
            env=dict(os.environ),
            cancel=cancel,
            retired=retired,
            request_abort=fail_abort,
        )
        try:
            _wait_file(ready)
        finally:
            cancel.set()
        with pytest.raises(ValueError, match="unexpected abort error"):
            future.result(10)
    assert retired.is_set()
    assert launches[0].poll() is not None


_TREE_SCRIPT = """
import os, subprocess, sys, time
from pathlib import Path
ready, marker, release, mode, pipe = sys.argv[1:]
child_code = (
    "import os,time; from pathlib import Path; "
    f"Path({marker!r}).write_text(str(os.getpid())); time.sleep(60)"
)
options = {} if pipe == 'pipe' else {'stdout': subprocess.DEVNULL, 'stderr': subprocess.DEVNULL}
subprocess.Popen(
    [sys._base_executable, '-I', '-c', child_code],
    creationflags=subprocess.CREATE_NO_WINDOW, **options,
)
while not Path(marker).exists(): time.sleep(0.005)
Path(ready).write_text(str(os.getpid()))
while not Path(release).exists(): time.sleep(0.005)
print('tree complete')
raise SystemExit(7 if mode == 'crash' else 0)
"""


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


@pytest.mark.skipif(os.name != "nt", reason="actual Windows launcher/descendant ownership")
@pytest.mark.parametrize("pipe", ["silent", "pipe"])
@pytest.mark.parametrize("mode", ["success", "crash", "cancel", "timeout"])
def test_windows_owned_command_retires_launcher_descendant_and_both_pipes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, mode: str, pipe: str
) -> None:
    environment = tmp_path / "venv"
    venv.EnvBuilder(with_pip=False).create(environment)
    script = tmp_path / "owned_tree.py"
    script.write_text(_TREE_SCRIPT, encoding="utf-8")
    ready, marker, release = (tmp_path / name for name in ("ready", "child", "release"))
    cancel = Event()
    retired = Event()
    launches: list[process._OwnedTreeProcess] = []
    original = process._OwnedTreeProcess.activate

    def activate(child: process._OwnedTreeProcess) -> None:
        launches.append(child)
        original(child)

    monkeypatch.setattr(process._OwnedTreeProcess, "activate", activate)
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(
            process.run_owned_command,
            [
                str(environment / "Scripts/python.exe"),
                "-I",
                str(script),
                str(ready),
                str(marker),
                str(release),
                mode,
                pipe,
            ],
            cwd=tmp_path,
            env=dict(os.environ),
            cancel=cancel,
            retired=retired,
            limits=WorkerLimits(timeout_seconds=3 if mode == "timeout" else 15),
        )
        try:
            _wait_file(ready)
            worker_pid = int(ready.read_text())
            assert worker_pid != launches[0].pid
            with (
                _observed_process(int(marker.read_text())) as (kernel, child_handle),
                _observed_process(worker_pid) as (_, worker_handle),
            ):
                assert kernel.WaitForSingleObject(child_handle, 0) == 258
                assert not retired.is_set()
                if mode == "cancel":
                    cancel.set()
                elif mode in {"success", "crash"}:
                    release.touch()
                result = future.result(15)
                assert (
                    result.reason
                    == {
                        "success": "ok",
                        "crash": "worker_crashed",
                        "cancel": "cancelled",
                        "timeout": "worker_timeout",
                    }[mode]
                )
                assert retired.is_set()
                assert launches[0].poll() is not None
                assert kernel.WaitForSingleObject(child_handle, 0) == 0
                assert kernel.WaitForSingleObject(worker_handle, 0) == 0
                assert launches[0].stdout is not None
                assert launches[0].stdout.closed
                assert launches[0].stderr is not None
                assert launches[0].stderr.closed
        finally:
            cancel.set()
            release.touch()
            future.result(15)


@pytest.mark.skipif(os.name != "nt", reason="suspended Windows process ownership")
@pytest.mark.parametrize("failure", ["assign_suspended", "_resume_initial_thread"])
def test_windows_owned_command_admission_failure_reaps_suspended_root(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    marker = tmp_path / "must-not-run"
    launches: list[process._OwnedTreeProcess] = []
    original = process._OwnedTreeProcess.activate

    def activate(child: process._OwnedTreeProcess) -> None:
        launches.append(child)
        original(child)

    def fail(job: WindowsJob, pid: int) -> None:
        msg = "fixture suspended admission failure"
        raise OSError(msg)

    monkeypatch.setattr(process._OwnedTreeProcess, "activate", activate)
    monkeypatch.setattr(WindowsJob, failure, fail)
    result = _run(
        [sys.executable, "-I", "-c", f"from pathlib import Path; Path({str(marker)!r}).touch()"],
        tmp_path,
    )
    assert result.reason == "worker_supervision_failed"
    assert launches[0].poll() is not None
    assert not marker.exists()
