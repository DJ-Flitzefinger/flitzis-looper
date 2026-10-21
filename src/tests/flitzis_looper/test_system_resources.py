"""Headroom query contracts without model launches, process admission, or devices."""

import ctypes
import os
import time
from ctypes import wintypes
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

from flitzis_looper import system_resources
from flitzis_looper.system_resources import (
    HostMemory,
    PerformanceInformation,
    host_memory_snapshot,
    system_commit_bytes,
    windows_host_memory_snapshot,
)


def _kernel(*, caller: dict[str, int] | None = None, system: dict[str, int] | None = None) -> Mock:
    caller_fields = {
        "total_physical": 32_000,
        "available_physical": 12_000,
        "commit_limit": 40_000,
        "available_commit": 8_000,
    } | (caller or {})
    system_fields = {"commit_total": 3, "commit_limit": 9, "page_size": 4096} | (system or {})

    def read_caller(pointer: ctypes.c_void_p) -> int:
        status = ctypes.cast(pointer, ctypes.POINTER(HostMemory)).contents
        assert status.length == ctypes.sizeof(HostMemory)
        for name, value in caller_fields.items():
            setattr(status, name, value)
        return 1

    def read_system(pointer: ctypes.c_void_p, size: int) -> int:
        status = ctypes.cast(pointer, ctypes.POINTER(PerformanceInformation)).contents
        assert status.size == size == ctypes.sizeof(PerformanceInformation)
        for name, value in system_fields.items():
            setattr(status, name, value)
        return 1

    kernel = Mock(spec=["GlobalMemoryStatusEx", "K32GetPerformanceInfo"])
    kernel.GlobalMemoryStatusEx.side_effect = read_caller
    kernel.K32GetPerformanceInfo.side_effect = read_system
    return kernel


def test_snapshot_keeps_distinct_metrics_and_sequential_observation_span(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    kernel = _kernel()
    monkeypatch.setattr(time, "perf_counter_ns", Mock(side_effect=[100, 145]))
    monkeypatch.setattr(time, "time_ns", lambda: 200)
    image = windows_host_memory_snapshot(kernel)
    assert image == {
        "total_physical": 32_000,
        "available_physical": 12_000,
        "commit_limit": 40_000,
        "available_commit": 8_000,
        "system_commit_total_bytes": 12_288,
        "system_commit_limit_bytes": 36_864,
        "system_available_commit_bytes": 24_576,
        "observed_at_unix_ns": 200,
        "observation_started_perf_counter_ns": 100,
        "observation_span_ns": 45,
    }
    assert all(type(value) is int for value in image.values())
    assert [call[0] for call in kernel.mock_calls] == [
        "GlobalMemoryStatusEx",
        "K32GetPerformanceInfo",
    ]


def test_sequential_queries_do_not_assume_simultaneous_caller_system_ordering() -> None:
    # The caller API was observed first; another process can consume commit before
    # the system API is observed. This discrepancy is not malformed geometry.
    image = windows_host_memory_snapshot(_kernel(caller={"available_commit": 30_000}))
    assert image["available_commit"] > image["system_available_commit_bytes"]


@pytest.mark.parametrize(
    "caller",
    [
        {"total_physical": 0},
        {"available_physical": 32_001},
        {"commit_limit": 0},
        {"available_commit": 40_001},
    ],
)
def test_malformed_successful_caller_image_is_not_headroom(caller: dict[str, int]) -> None:
    with pytest.raises(ValueError, match="invalid headroom geometry"):
        windows_host_memory_snapshot(_kernel(caller=caller))


@pytest.mark.parametrize(
    "system",
    [
        {"commit_total": 10},
        {"page_size": 0},
        {"commit_total": 0, "commit_limit": 0},
    ],
)
def test_malformed_successful_system_image_is_not_headroom(system: dict[str, int]) -> None:
    with pytest.raises(ValueError, match=r"invalid .* geometry"):
        windows_host_memory_snapshot(_kernel(system=system))


def test_commit_conversion_preserves_maximum_uint64_extent_without_rounding() -> None:
    maximum = 2**64 - 1
    assert system_commit_bytes(maximum - 1, maximum, 1) == (maximum - 1, maximum, 1)
    with pytest.raises(ValueError, match="byte extent overflow"):
        system_commit_bytes(maximum, maximum, 2)


def test_non_windows_is_the_only_unsupported_result(monkeypatch: pytest.MonkeyPatch) -> None:
    query = Mock(side_effect=AssertionError("unsupported platform queried kernel32"))
    monkeypatch.setattr(system_resources, "os", SimpleNamespace(name="posix"))
    monkeypatch.setattr(system_resources, "_windows_kernel", query)
    assert host_memory_snapshot() is None
    query.assert_not_called()


def test_kernel_configuration_failure_propagates(monkeypatch: pytest.MonkeyPatch) -> None:
    failed = Mock(side_effect=OSError("kernel32 unavailable"))
    monkeypatch.setattr(system_resources, "os", SimpleNamespace(name="nt"))
    monkeypatch.setattr(system_resources, "_windows_kernel", failed)
    with pytest.raises(OSError, match="kernel32 unavailable"):
        host_memory_snapshot()
    failed.assert_called_once()


@pytest.mark.skipif(os.name != "nt", reason="actual Windows error translation")
@pytest.mark.parametrize("api", ["GlobalMemoryStatusEx", "K32GetPerformanceInfo"])
def test_query_failure_retains_last_error_and_stops_at_failed_api(
    monkeypatch: pytest.MonkeyPatch, api: str
) -> None:
    kernel = _kernel()
    failed = getattr(kernel, api)
    failed.side_effect = None
    failed.return_value = 0
    monkeypatch.setattr(ctypes, "get_last_error", lambda: 5)
    with pytest.raises(OSError, match=r"\[WinError 5\]"):
        windows_host_memory_snapshot(kernel)
    failed.assert_called_once()
    if api == "GlobalMemoryStatusEx":
        kernel.K32GetPerformanceInfo.assert_not_called()


@pytest.mark.skipif(os.name != "nt", reason="actual Windows kernel32 configuration")
def test_default_query_lazily_reuses_configured_functions(monkeypatch: pytest.MonkeyPatch) -> None:
    kernel = _kernel()
    load = Mock(return_value=kernel)
    system_resources._windows_kernel.cache_clear()
    monkeypatch.setattr(ctypes, "WinDLL", load)
    try:
        first = host_memory_snapshot()
        second = host_memory_snapshot()
        assert first is not None
        assert second is not None
        assert first is not second
        load.assert_called_once_with("kernel32", use_last_error=True)
        assert kernel.GlobalMemoryStatusEx.argtypes == [ctypes.POINTER(HostMemory)]
        assert kernel.GlobalMemoryStatusEx.restype is wintypes.BOOL
        assert kernel.K32GetPerformanceInfo.argtypes == [
            ctypes.POINTER(PerformanceInformation),
            wintypes.DWORD,
        ]
        assert kernel.K32GetPerformanceInfo.restype is wintypes.BOOL
    finally:
        system_resources._windows_kernel.cache_clear()


@pytest.mark.skipif(os.name != "nt", reason="actual Windows query failure")
def test_default_query_failure_is_not_unsupported_or_cached_headroom(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    kernel = _kernel()
    monkeypatch.setattr(system_resources, "_windows_kernel", lambda: kernel)
    assert host_memory_snapshot() is not None
    kernel.K32GetPerformanceInfo.side_effect = None
    kernel.K32GetPerformanceInfo.return_value = 0
    monkeypatch.setattr(ctypes, "get_last_error", lambda: 8)
    with pytest.raises(OSError, match=r"\[WinError 8\]"):
        host_memory_snapshot()


@pytest.mark.skipif(os.name != "nt", reason="actual Windows handle lifetime")
def test_actual_default_queries_preserve_host_fields_without_opening_process_handles() -> None:
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.GetCurrentProcess.argtypes = []
    kernel.GetCurrentProcess.restype = wintypes.HANDLE
    kernel.GetProcessHandleCount.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
    kernel.GetProcessHandleCount.restype = wintypes.BOOL
    process = kernel.GetCurrentProcess()
    before = wintypes.DWORD()
    after = wintypes.DWORD()
    image = host_memory_snapshot()
    assert image is not None
    assert image["system_commit_limit_bytes"] > 0
    assert image["total_physical"] > 0
    assert image["observed_at_unix_ns"] > 0
    assert image["observation_span_ns"] >= 0
    assert (
        image["system_commit_total_bytes"] + image["system_available_commit_bytes"]
        == (image["system_commit_limit_bytes"])
    )
    assert kernel.GetProcessHandleCount(process, ctypes.byref(before))
    for _ in range(64):
        assert host_memory_snapshot() is not None
    assert kernel.GetProcessHandleCount(process, ctypes.byref(after))
    assert before.value == after.value
