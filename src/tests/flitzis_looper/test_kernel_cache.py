"""Deterministic cache publication races, independent of live resource policy."""

import ctypes
from concurrent.futures import ThreadPoolExecutor
from threading import Event
from unittest.mock import Mock

import pytest

from flitzis_looper import system_resources


@pytest.fixture(autouse=True)
def _empty_cache():
    system_resources._WINDOWS_KERNEL.clear()
    yield
    system_resources._WINDOWS_KERNEL.clear()


def _kernel() -> Mock:
    return Mock(spec=["GlobalMemoryStatusEx", "K32GetPerformanceInfo"])


@pytest.mark.parametrize("failure", ["load", "signature"])
def test_failed_initialization_never_publishes_and_later_call_retries(
    monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    broken = _kernel()
    if failure == "signature":
        broken.K32GetPerformanceInfo = None
    good = _kernel()
    loader = Mock(side_effect=[OSError("load failed") if failure == "load" else broken, good])
    monkeypatch.setattr(ctypes, "WinDLL", loader, raising=False)
    with pytest.raises(OSError if failure == "load" else AttributeError):
        system_resources._windows_kernel()
    assert system_resources._windows_kernel() is good
    assert system_resources._windows_kernel() is good
    assert loader.call_count == 2
    assert good.GlobalMemoryStatusEx.argtypes == [ctypes.POINTER(system_resources.HostMemory)]
    assert good.K32GetPerformanceInfo.argtypes[0] == ctypes.POINTER(
        system_resources.PerformanceInformation
    )


def test_concurrent_cold_callers_return_own_complete_wrappers_and_last_publishes(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    entered = [Event(), Event()]
    release = [Event(), Event()]
    wrappers = [_kernel(), _kernel()]
    calls = iter(range(2))

    def load(*_args, **_kwargs):
        index = next(calls)
        entered[index].set()
        assert release[index].wait(5)
        return wrappers[index]

    monkeypatch.setattr(ctypes, "WinDLL", load, raising=False)
    with ThreadPoolExecutor(max_workers=2) as workers:
        try:
            first = workers.submit(system_resources._windows_kernel)
            assert entered[0].wait(5)
            second = workers.submit(system_resources._windows_kernel)
            assert entered[1].wait(5)
            release[0].set()
            assert first.result(timeout=5) is wrappers[0]
            release[1].set()
            assert second.result(timeout=5) is wrappers[1]
            assert system_resources._windows_kernel() is wrappers[1]
            for kernel in wrappers:
                assert kernel.K32GetPerformanceInfo.argtypes[1] is system_resources.wintypes.DWORD
                assert kernel.GlobalMemoryStatusEx.restype is system_resources.wintypes.BOOL
        finally:
            for event in release:
                event.set()


def test_clear_during_initialization_does_not_cancel_later_publication(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    entered, release = Event(), Event()
    old, newer = _kernel(), _kernel()
    calls = iter((old, newer))

    def load(*_args, **_kwargs):
        kernel = next(calls)
        if kernel is old:
            entered.set()
            assert release.wait(5)
        return kernel

    monkeypatch.setattr(ctypes, "WinDLL", load, raising=False)
    with ThreadPoolExecutor(max_workers=2) as workers:
        try:
            pending = workers.submit(system_resources._windows_kernel)
            assert entered.wait(5)
            workers.submit(system_resources._WINDOWS_KERNEL.clear).result(timeout=5)
            assert system_resources._windows_kernel() is newer
            release.set()
            assert pending.result(timeout=5) is old
            assert system_resources._windows_kernel() is old
            system_resources._WINDOWS_KERNEL.clear()
        finally:
            release.set()
