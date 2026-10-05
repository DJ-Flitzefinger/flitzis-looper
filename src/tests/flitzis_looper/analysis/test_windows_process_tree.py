"""Real Windows venv-launcher and descendant retirement, without model inference."""

import ctypes
import json
import os
import time
import venv
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from ctypes import wintypes
from threading import Event
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import process
from flitzis_looper.analysis.contracts import WorkerLimits
from flitzis_looper.analysis.windows_job import WindowsJob

if TYPE_CHECKING:
    import subprocess
    from collections.abc import Iterator
    from pathlib import Path

pytestmark = pytest.mark.skipif(os.name != "nt", reason="Windows process-tree ownership")

_TREE_SCRIPT = """
import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path
p = argparse.ArgumentParser()
p.add_argument('--request')
p.add_argument('--checkpoint')
args = p.parse_args()
request = json.loads(Path(args.request).read_text())
marker = Path(request['marker'])
child_code = f'''
import os, subprocess, sys, time
from pathlib import Path
marker = Path({str(marker)!r})
marker.write_text(str(os.getpid()))
while not marker.with_suffix('.attempt').exists():
    time.sleep(0.005)
try:
    nested = subprocess.Popen(
        [sys.executable, '-I', '-c', 'import time; time.sleep(60)'],
        creationflags=subprocess.CREATE_NO_WINDOW,
    )
except OSError:
    marker.with_suffix('.spawn').write_text('denied')
else:
    marker.with_suffix('.spawn').write_text('spawned')
time.sleep(60)
'''
# A quiet child cannot hold stdout open as an accidental retirement signal.
child = subprocess.Popen(
    [sys._base_executable, '-I', '-c', child_code],
    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    creationflags=subprocess.CREATE_NO_WINDOW,
)
while not marker.exists():
    time.sleep(0.005)
Path(request['ready']).write_text(str(os.getpid()))
if request['mode'] in {'normal', 'crash'}:
    while not Path(request['release']).exists():
        time.sleep(0.005)
    if request['mode'] == 'crash':
        raise SystemExit(7)
    sys.stdout.write('{}')
else:
    time.sleep(60)
"""


@pytest.fixture
def command(tmp_path: Path) -> process.WorkerCommand:
    environment = tmp_path / "venv"
    venv.EnvBuilder(with_pip=False).create(environment)
    script = tmp_path / "worker.py"
    script.write_text(_TREE_SCRIPT, encoding="utf-8")
    checkpoint = tmp_path / "fixture.ckpt"
    checkpoint.write_bytes(b"not used by process fixture")
    return process.WorkerCommand(environment / "Scripts/python.exe", script, checkpoint, tmp_path)


def _wait_file(path: Path) -> None:
    deadline = time.monotonic() + 10
    while not path.is_file() or not path.stat().st_size:
        assert time.monotonic() < deadline, "worker descendant failed to start"
        Event().wait(0.01)


@contextmanager
def _observed_process(pid: int) -> Iterator[tuple[ctypes.WinDLL, int]]:
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
    kernel.WaitForSingleObject.restype = wintypes.DWORD
    kernel.CloseHandle.argtypes = (wintypes.HANDLE,)
    kernel.CloseHandle.restype = wintypes.BOOL
    handle = kernel.OpenProcess(0x00100000, 0, pid)  # SYNCHRONIZE, no process control rights.
    assert handle, ctypes.WinError(ctypes.get_last_error())
    try:
        yield kernel, int(handle)
    finally:
        assert kernel.CloseHandle(handle)


def _record_launchers(monkeypatch: pytest.MonkeyPatch) -> list[subprocess.Popen[bytes]]:
    launches: list[subprocess.Popen[bytes]] = []
    original = process._start

    def start(command: process.WorkerCommand, path: Path) -> subprocess.Popen[bytes]:
        child = original(command, path)
        launches.append(child)
        return child

    monkeypatch.setattr(process, "_start", start)
    return launches


@pytest.mark.parametrize(
    ("mode", "reason"),
    [
        ("cancel", "cancelled"),
        ("timeout", "worker_timeout"),
        ("normal", "ok"),
        ("crash", "worker_crashed"),
    ],
)
def test_real_venv_launcher_and_silent_child_retire_together(
    command: process.WorkerCommand, monkeypatch: pytest.MonkeyPatch, mode: str, reason: str
) -> None:
    marker = command.scratch_dir / "child.pid"
    ready = command.scratch_dir / "worker.pid"
    release = command.scratch_dir / "release-root"
    request = json.dumps({
        "marker": str(marker),
        "ready": str(ready),
        "release": str(release),
        "mode": mode,
    }).encode()
    launches = _record_launchers(monkeypatch)
    cancel = Event()
    retired = Event()
    limits = WorkerLimits(timeout_seconds=3 if mode == "timeout" else 15)
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(process.run_process, command, request, cancel, limits, retired)
        try:
            _wait_file(ready)
            child_pid = int(marker.read_text())
            worker_pid = int(ready.read_text())
            # Keep kernel handles: PID reuse cannot turn an exit assertion into a false pass.
            with (
                _observed_process(child_pid) as (kernel, child_handle),
                _observed_process(worker_pid) as (_, worker_handle),
            ):
                assert kernel.WaitForSingleObject(child_handle, 0) == 258
                assert worker_pid != launches[0].pid  # Real Windows venv launcher exercised.
                assert not retired.is_set()
                assert (
                    process.run_process(command, request, Event(), limits, Event()).reason
                    == "worker_busy"
                )
                if mode == "cancel":
                    cancel.set()
                elif mode in {"normal", "crash"}:
                    release.touch()
                result = future.result(timeout=15)
                assert result.reason == reason
                assert result.resources_released
                assert retired.is_set()
                assert kernel.WaitForSingleObject(child_handle, 0) == 0
                assert kernel.WaitForSingleObject(worker_handle, 0) == 0
        finally:
            cancel.set()
            release.touch()
            future.result(timeout=15)
    assert launches[0].poll() is not None
    assert list(command.scratch_dir.glob("beat-request-*")) == []
    command.script.write_text("print('{}')\n", encoding="utf-8")
    assert process.run_process(command, b"{}", Event(), limits, Event()).reason == "ok"


@pytest.mark.parametrize("failure", ["assign_suspended", "_resume_initial_thread"])
def test_failed_windows_admission_reaps_the_suspended_root(
    command: process.WorkerCommand, monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    launches = _record_launchers(monkeypatch)
    retired = Event()

    def fail(job: WindowsJob, pid: int) -> None:
        msg = "fixture Windows admission failure"
        raise OSError(msg)

    with monkeypatch.context() as patch:
        patch.setattr(WindowsJob, failure, fail)
        result = process.run_process(command, b"{}", Event(), WorkerLimits(), retired)
    assert result.reason == "worker_supervision_failed"
    assert result.resources_released
    assert retired.is_set()
    assert launches[0].poll() is not None
    assert not (command.scratch_dir / "worker.pid").exists()
    assert list(command.scratch_dir.glob("beat-request-*")) == []
    command.script.write_text("print('{}')\n", encoding="utf-8")
    assert process.run_process(command, b"{}", Event(), WorkerLimits(), Event()).reason == "ok"


def test_terminal_admission_limit_keeps_existing_members_but_prevents_new_children(
    command: process.WorkerCommand, monkeypatch: pytest.MonkeyPatch
) -> None:
    marker = command.scratch_dir / "child.pid"
    ready = command.scratch_dir / "worker.pid"
    release = command.scratch_dir / "release-root"
    request = json.dumps({
        "marker": str(marker),
        "ready": str(ready),
        "release": str(release),
        "mode": "normal",
    }).encode()
    launches = _record_launchers(monkeypatch)
    cancel = Event()
    with ThreadPoolExecutor(max_workers=1) as executor:
        future = executor.submit(
            process.run_process, command, request, cancel, WorkerLimits(), Event()
        )
        try:
            _wait_file(ready)
            child = launches[0]
            assert isinstance(child, process._WorkerProcess)
            assert child._job is not None
            child._job._set_process_limit(1)
            marker.with_suffix(".attempt").touch()
            _wait_file(marker.with_suffix(".spawn"))
            assert marker.with_suffix(".spawn").read_text() == "denied"
            assert child._job.active()
            release.touch()
            assert future.result(timeout=15).resources_released
        finally:
            cancel.set()
            release.touch()
            future.result(timeout=15)


def test_retirement_retries_a_failed_tree_termination(
    command: process.WorkerCommand, monkeypatch: pytest.MonkeyPatch
) -> None:
    command.script.write_text("print('{}')\n", encoding="utf-8")
    original = WindowsJob.terminate
    attempts = 0

    def terminate(job: WindowsJob) -> None:
        nonlocal attempts
        attempts += 1
        if attempts == 1:
            msg = "fixture transient kernel termination failure"
            raise OSError(msg)
        original(job)

    monkeypatch.setattr(WindowsJob, "terminate", terminate)
    retired = Event()
    result = process.run_process(command, b"{}", Event(), WorkerLimits(), retired)
    assert result.reason == "worker_retiring"
    assert not result.resources_released
    assert retired.wait(5)
    assert attempts >= 2
    assert list(command.scratch_dir.glob("beat-request-*")) == []
