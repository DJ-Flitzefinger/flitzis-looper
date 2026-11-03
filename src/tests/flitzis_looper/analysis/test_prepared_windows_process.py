"""Real suspended Windows roots and actual retirement, without model inference.

The controlled post-start exception is not an allocator-OOM or upstream-CPython
binary defect reproduction. The child and its observed kernel handle are real.
"""

import ctypes
import json
import os
import subprocess
import sys
import uuid
from contextlib import ExitStack, contextmanager
from ctypes import wintypes
from pathlib import Path
from threading import Event
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import process, windows_job
from flitzis_looper.offline_runtime import child_environment
from flitzis_looper_audio import (
    OfflineChildMemoryReservation,
    PreparedWindowsProcess,
    offline_process_identity,
)

if TYPE_CHECKING:
    from collections.abc import Iterator
    from types import FrameType

pytestmark = pytest.mark.skipif(os.name != "nt", reason="native prepared Windows child owner")


@contextmanager
def _observed(pid: int) -> Iterator[tuple[ctypes.WinDLL, int]]:
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


def test_public_owned_command_preserves_windows_arguments_environment_and_both_pipes(
    tmp_path: Path,
) -> None:
    tmp_path /= "prepared \U0001f4be"
    tmp_path.mkdir()
    arguments = ['space "quoted" tail\\', "\U0001f4be", "\ud800"]
    env = child_environment(dict(os.environ))
    env.update({"FLITZIS_CASE": "first", "flitzis_case": "last", "=C:": "C:\\fixture"})
    retired = Event()
    source = """
import ctypes,json,os,sys
from ctypes import wintypes as w
k=ctypes.WinDLL('kernel32',use_last_error=True)
k.GetEnvironmentStringsW.restype=ctypes.c_void_p
k.FreeEnvironmentStringsW.argtypes=(ctypes.c_void_p,)
k.FreeEnvironmentStringsW.restype=w.BOOL
block=k.GetEnvironmentStringsW()
assert block
cursor=block
drive=None
try:
    # The CRT/Python environment mapping omits hidden drive entries. Observe
    # the actual Windows block rather than that narrower Python projection.
    while entry:=ctypes.wstring_at(cursor):
        if entry.startswith('=C:='): drive=entry[4:]
        cursor+=len(entry.encode('utf-16-le','surrogatepass'))+2
finally: assert k.FreeEnvironmentStringsW(block)
print(json.dumps([sys.argv[1:],os.getcwd(),os.environ['FLITZIS_CASE'],drive,sys.stdin.read()]))
sys.stderr.write('separate stderr')
"""

    result = process.run_owned_command(
        [sys.executable, "-I", "-c", source, *arguments],
        cwd=tmp_path,
        env=env,
        cancel=Event(),
        retired=retired,
    )

    assert result.reason == "ok", result.stderr
    assert result.returncode == 0
    assert json.loads(result.stdout) == [arguments, str(tmp_path), "last", "C:\\fixture", ""]
    assert result.stderr == b"separate stderr"
    assert retired.is_set()


def test_prepared_root_cannot_run_before_activation_and_abort_waits_actual_end(
    tmp_path: Path,
) -> None:
    marker = tmp_path / "must-not-run"
    prepared = PreparedWindowsProcess(capture_stderr=True)
    try:
        prepared.start(
            subprocess.list2cmdline([
                sys.executable,
                "-I",
                "-c",
                f"from pathlib import Path; Path({str(marker)!r}).touch()",
            ]),
            str(tmp_path),
            child_environment(dict(os.environ)),
        )
        with _observed(prepared.pid) as (kernel, handle):
            assert kernel.WaitForSingleObject(handle, 0) == 258
            _assert_initial_thread_suspended(prepared.pid)
            assert prepared.wait(0.0) is None
            assert not marker.exists()
            prepared.abort_startup()
            assert kernel.WaitForSingleObject(handle, 0) == 0
            assert prepared.poll() is not None
            assert not marker.exists()
        with pytest.raises(ValueError, match="only be started once"):
            prepared.start("unused", str(tmp_path), {})
    finally:
        prepared.abort_startup()


def _assert_initial_thread_suspended(pid: int) -> None:
    """Observe the prior kernel suspend count, restoring it before leaving."""
    kernel = windows_job._kernel()
    kernel.SuspendThread.argtypes = (wintypes.HANDLE,)
    kernel.SuspendThread.restype = wintypes.DWORD
    snapshot = kernel.CreateToolhelp32Snapshot(4, 0)
    assert snapshot != ctypes.c_void_p(-1).value
    entry = windows_job._ThreadEntry()
    entry.size = ctypes.sizeof(entry)
    matched = 0
    try:
        more = kernel.Thread32First(snapshot, ctypes.byref(entry))
        while more:
            if entry.process_id == pid:
                thread = kernel.OpenThread(2, 0, entry.thread_id)
                assert thread
                try:
                    before = kernel.SuspendThread(thread)
                    assert before != 0xFFFFFFFF
                    restored = kernel.ResumeThread(thread)
                    assert restored == before + 1
                    assert before == 1
                    matched += 1
                finally:
                    assert kernel.CloseHandle(thread)
            more = kernel.Thread32Next(snapshot, ctypes.byref(entry))
        assert matched == 1
    finally:
        assert kernel.CloseHandle(snapshot)


def test_actual_createprocess_refusal_preserves_zero_child_and_closes_prepared_pipes(
    tmp_path: Path,
) -> None:
    prepared = PreparedWindowsProcess(capture_stderr=True)
    with pytest.raises(OSError, match="create suspended offline process"):
        prepared.start(str(tmp_path / "missing-worker.exe"), str(tmp_path), {})
    assert prepared.pid == 0
    assert prepared.poll() is None
    prepared.abort_startup()
    with pytest.raises(ValueError, match="closed offline pipe"):
        prepared.stdout.read(1)


@pytest.mark.parametrize("failure_point", ["poststart", "base_return", "worker_return"])
def test_real_poststart_python_error_waits_root_before_releasing_beat_budget(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, failure_point: str
) -> None:
    observed: list[tuple[ctypes.WinDLL, int]] = []
    scopes = ExitStack()
    releases = []

    class ObservedBudget:
        def __init__(self) -> None:
            self.actual = OfflineChildMemoryReservation()

        @property
        def limit_bytes(self) -> int:
            return self.actual.limit_bytes

        def close(self) -> None:
            assert observed
            kernel, handle = observed[0]
            assert kernel.WaitForSingleObject(handle, 0) == 0
            releases.append("actual root ended before reservation close")
            self.actual.close()

    def trace(frame: FrameType, event: str, _arg: object) -> object:
        target = (
            process._WorkerProcess.__init__
            if failure_point == "worker_return"
            else process._OwnedTreeProcess.__init__
        )
        if failure_point != "poststart" and frame.f_code is target.__code__ and event == "return":
            prepared = getattr(frame.f_locals["self"], "_process", None)
            if isinstance(prepared, PreparedWindowsProcess) and prepared.pid and not observed:
                observed.append(scopes.enter_context(_observed(prepared.pid)))
                message = "controlled Python error after real native start"
                raise RuntimeError(message)
        return trace

    def profile(_frame: FrameType, event: str, argument: object) -> None:
        prepared = getattr(argument, "__self__", None)
        if (
            event == "c_return"
            and getattr(argument, "__name__", None) == "start"
            and isinstance(prepared, PreparedWindowsProcess)
            and not observed
        ):
            observed.append(scopes.enter_context(_observed(prepared.pid)))
            message = "controlled Python error after real native start"
            raise RuntimeError(message)

    monkeypatch.setattr(process, "OfflineChildMemoryReservation", ObservedBudget)
    command = process.WorkerCommand(
        Path(sys.executable), tmp_path / "unused.py", tmp_path / "unused.ckpt", tmp_path
    )
    previous = sys.gettrace()
    previous_profile = sys.getprofile()
    try:
        sys.settrace(trace)
        if failure_point == "poststart":
            sys.setprofile(profile)
        with pytest.raises(RuntimeError, match="controlled Python error"):
            process._start(
                command,
                process._WorkerLaunch(
                    [sys.executable, "-I", "-c", "raise SystemExit(7)"],
                    child_environment(dict(os.environ)),
                ),
            )
    finally:
        sys.settrace(previous)
        sys.setprofile(previous_profile)
        scopes.close()
    # Nested startup guards may idempotently close the already retired budget.
    assert releases
    assert set(releases) == {"actual root ended before reservation close"}
    # The real reservation can be freshly admitted after the failed constructor.
    successor = OfflineChildMemoryReservation()
    successor.close()


def test_shared_constructor_return_error_waits_root_before_retired_signal(tmp_path: Path) -> None:
    scopes = ExitStack()
    observed = []
    marker = tmp_path / "must-not-run"

    class ObservedRetired(Event):
        def set(self) -> None:
            assert observed
            kernel, handle = observed[0]
            assert kernel.WaitForSingleObject(handle, 0) == 0
            super().set()

    def trace(frame: FrameType, event: str, _arg: object) -> object:
        if frame.f_code is process._OwnedTreeProcess.__init__.__code__ and event == "return":
            child = frame.f_locals["self"]
            observed.append(scopes.enter_context(_observed(child.pid)))
            message = "controlled constructor return error"
            raise RuntimeError(message)
        return trace

    retired = ObservedRetired()
    previous = sys.gettrace()
    try:
        sys.settrace(trace)
        with pytest.raises(RuntimeError, match="controlled constructor return"):
            process.run_owned_command(
                [
                    sys.executable,
                    "-I",
                    "-c",
                    f"from pathlib import Path; Path({str(marker)!r}).touch()",
                ],
                cwd=tmp_path,
                env=dict(os.environ),
                cancel=Event(),
                retired=retired,
            )
    finally:
        sys.settrace(previous)
        scopes.close()
    assert retired.is_set()
    assert not marker.exists()


def test_native_drop_waits_root_before_releasing_its_prebirth_budget_keeper(tmp_path: Path) -> None:
    observations = []
    released = []

    class BudgetKeeper:
        def __init__(self) -> None:
            self.actual = OfflineChildMemoryReservation()

        def __del__(self) -> None:
            kernel, handle = observations[0]
            released.append(kernel.WaitForSingleObject(handle, 0))
            self.actual.close()

    keeper = BudgetKeeper()
    prepared = PreparedWindowsProcess(budget_keeper=keeper)
    del keeper
    prepared.start(
        subprocess.list2cmdline([sys.executable, "-I", "-c", "raise SystemExit(9)"]),
        str(tmp_path),
        child_environment(dict(os.environ)),
    )
    with _observed(prepared.pid) as observation:
        observations.append(observation)
        assert observation[0].WaitForSingleObject(observation[1], 0) == 258
        assert not released
        del prepared
        assert released == [0]


def test_compiled_process_identity_requires_its_actual_constructor() -> None:
    policy, digest = offline_process_identity(PreparedWindowsProcess)
    assert policy == "prepared-windows-process-v1"
    assert len(digest) == 64
    with pytest.raises(ValueError, match="unsupported prepared Windows process binding"):
        offline_process_identity(type("Unrelated", (), {}))


def test_ambient_inheritable_event_is_not_inherited_by_the_offline_child(tmp_path: Path) -> None:
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateEventW.argtypes = (ctypes.c_void_p, wintypes.BOOL, wintypes.BOOL, wintypes.LPCWSTR)
    kernel.CreateEventW.restype = wintypes.HANDLE
    kernel.SetHandleInformation.argtypes = (wintypes.HANDLE, wintypes.DWORD, wintypes.DWORD)
    kernel.SetHandleInformation.restype = wintypes.BOOL
    kernel.CloseHandle.argtypes = (wintypes.HANDLE,)
    kernel.CloseHandle.restype = wintypes.BOOL
    name = "Local\\FLITZIS_prepared_" + uuid.uuid4().hex
    handle = kernel.CreateEventW(None, 0, 0, name)
    assert handle, ctypes.WinError(ctypes.get_last_error())
    source = """
import ctypes,json,sys
from ctypes import wintypes as w
k=ctypes.WinDLL('kernel32',use_last_error=True)
k.DuplicateHandle.argtypes=(w.HANDLE,w.HANDLE,w.HANDLE,ctypes.POINTER(w.HANDLE),w.DWORD,w.BOOL,w.DWORD)
k.DuplicateHandle.restype=w.BOOL
k.OpenEventW.argtypes=(w.DWORD,w.BOOL,w.LPCWSTR)
k.OpenEventW.restype=w.HANDLE
compare=ctypes.WinDLL('kernelbase',use_last_error=True).CompareObjectHandles
compare.argtypes=(w.HANDLE,w.HANDLE)
compare.restype=w.BOOL
k.CloseHandle.argtypes=(w.HANDLE,)
k.CloseHandle.restype=w.BOOL
candidate=w.HANDLE()
# Capture the original slot BEFORE opening the named event: a newly opened
# handle can reuse that slot and must not be mistaken for inherited identity.
duplicated=k.DuplicateHandle(-1,int(sys.argv[1]),-1,ctypes.byref(candidate),0,0,2)
target=k.OpenEventW(0x100000,0,sys.argv[2])
control=k.OpenEventW(0x100000,0,sys.argv[2])
assert target and control
try:
    same=bool(compare(target,control))
    inherited=bool(compare(candidate,target)) if duplicated else False
finally:
    assert k.CloseHandle(control); assert k.CloseHandle(target)
    if duplicated: assert k.CloseHandle(candidate)
print(json.dumps([inherited,same]))
"""
    try:
        assert kernel.SetHandleInformation(handle, 1, 1)
        result = process.run_owned_command(
            [sys.executable, "-I", "-c", source, str(int(handle)), name],
            cwd=tmp_path,
            env=child_environment(dict(os.environ)),
            cancel=Event(),
            retired=Event(),
        )
        assert result.reason == "ok", result.stderr
        assert json.loads(result.stdout) == [False, True]
    finally:
        assert kernel.CloseHandle(handle)
