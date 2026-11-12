"""Caller and system memory headroom for non-realtime control and offline work.

The two Win32 queries are sequential observations, not an atomic OS snapshot.
Caller commit headroom can differ from system headroom; neither is process RSS,
a model-size estimate, or a guarantee about a future allocation. No process/job
handles, telemetry thread, or observation history are created here.
"""

import ctypes
import os
import time
from ctypes import wintypes
from threading import Lock


class HostMemory(ctypes.Structure):
    """Win32 MEMORYSTATUSEX layout, including caller-specific commit bytes."""

    _fields_ = (
        ("length", wintypes.DWORD),
        ("load", wintypes.DWORD),
        ("total_physical", ctypes.c_uint64),
        ("available_physical", ctypes.c_uint64),
        ("commit_limit", ctypes.c_uint64),
        ("available_commit", ctypes.c_uint64),
        ("total_virtual", ctypes.c_uint64),
        ("available_virtual", ctypes.c_uint64),
        ("available_extended_virtual", ctypes.c_uint64),
    )


class PerformanceInformation(ctypes.Structure):
    """Win32 PERFORMANCE_INFORMATION layout; commit quantities are page counts."""

    _fields_ = (
        ("size", wintypes.DWORD),
        ("commit_total", ctypes.c_size_t),
        ("commit_limit", ctypes.c_size_t),
        ("commit_peak", ctypes.c_size_t),
        ("physical_total", ctypes.c_size_t),
        ("physical_available", ctypes.c_size_t),
        ("system_cache", ctypes.c_size_t),
        ("kernel_total", ctypes.c_size_t),
        ("kernel_paged", ctypes.c_size_t),
        ("kernel_nonpaged", ctypes.c_size_t),
        ("page_size", ctypes.c_size_t),
        ("handles", wintypes.DWORD),
        ("processes", wintypes.DWORD),
        ("threads", wintypes.DWORD),
    )


def system_commit_bytes(total: int, limit: int, page_size: int) -> tuple[int, int, int]:
    """Convert one PERFORMANCE_INFORMATION commit image to exact unsigned bytes.

    Args:
        total: System committed page count.
        limit: System commit page limit from the same query.
        page_size: Page size in bytes from the same query.

    Returns:
        Total, limit, and available commit bytes, without coercion or saturation.

    Raises:
        ValueError: The geometry is malformed or any byte extent exceeds uint64.
    """
    if (
        any(type(value) is not int for value in (total, limit, page_size))
        or page_size <= 0
        or total < 0
        or total > limit
    ):
        message = "GetPerformanceInfo returned invalid commit geometry"
        raise ValueError(message)
    values = total * page_size, limit * page_size, (limit - total) * page_size
    if any(value > 2**64 - 1 for value in values):
        message = "GetPerformanceInfo commit byte extent overflow"
        raise ValueError(message)
    return values


class _WindowsKernelCache:
    """One lazy configured-wrapper owner, independent of live memory observations."""

    __slots__ = ("_kernel", "_lock")

    def __init__(self) -> None:
        self._lock = Lock()
        self._kernel: ctypes.WinDLL | None = None

    def get(self) -> ctypes.WinDLL:
        """Publish successful initialization; cold callers return their own wrapper."""
        with self._lock:
            kernel = self._kernel
        if kernel is not None:
            return kernel
        kernel = _configure_windows_kernel()
        with self._lock:
            previous = self._kernel
            self._kernel = kernel
        # Release a displaced wrapper outside the cache lock.
        del previous
        return kernel

    def clear(self) -> None:
        """Empty the cache; an already running initializer may publish afterward."""
        with self._lock:
            previous = self._kernel
            self._kernel = None
        del previous


_WINDOWS_KERNEL = _WindowsKernelCache()


def _windows_kernel() -> ctypes.WinDLL:
    return _WINDOWS_KERNEL.get()


def _configure_windows_kernel() -> ctypes.WinDLL:
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.GlobalMemoryStatusEx.argtypes = [ctypes.POINTER(HostMemory)]
    kernel.GlobalMemoryStatusEx.restype = wintypes.BOOL
    kernel.K32GetPerformanceInfo.argtypes = [
        ctypes.POINTER(PerformanceInformation),
        wintypes.DWORD,
    ]
    kernel.K32GetPerformanceInfo.restype = wintypes.BOOL
    kernel.CreateMemoryResourceNotification.argtypes = [ctypes.c_int]
    kernel.CreateMemoryResourceNotification.restype = wintypes.HANDLE
    kernel.QueryMemoryResourceNotification.argtypes = [
        wintypes.HANDLE,
        ctypes.POINTER(wintypes.BOOL),
    ]
    kernel.QueryMemoryResourceNotification.restype = wintypes.BOOL
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.CloseHandle.restype = wintypes.BOOL
    return kernel


def windows_host_memory_snapshot(kernel: ctypes.WinDLL) -> dict[str, int]:
    """Read timestamped headroom using an injected, signature-configured kernel32.

    Args:
        kernel: Kernel32 wrapper with HostMemory and PerformanceInformation query
            signatures configured before use.

    Returns:
        The established caller/system byte fields and observation timestamps as
        plain integers. Values from the two APIs need not describe the same moment.

    Raises:
        OSError: Either Win32 query fails.
        ValueError: A successful query returns malformed memory geometry.
    """
    started = time.perf_counter_ns()
    observed_at = time.time_ns()
    status = HostMemory()
    status.length = ctypes.sizeof(status)
    if not kernel.GlobalMemoryStatusEx(ctypes.byref(status)):
        raise ctypes.WinError(ctypes.get_last_error())
    system = PerformanceInformation()
    system.size = ctypes.sizeof(system)
    if not kernel.K32GetPerformanceInfo(ctypes.byref(system), system.size):
        raise ctypes.WinError(ctypes.get_last_error())
    total, limit, available = system_commit_bytes(
        system.commit_total, system.commit_limit, system.page_size
    )
    if (
        not status.total_physical
        or status.available_physical > status.total_physical
        or not status.commit_limit
        or status.available_commit > status.commit_limit
        or not limit
    ):
        message = "Windows memory query returned invalid headroom geometry"
        raise ValueError(message)
    caller = {
        name: int(getattr(status, name))
        for name in ("total_physical", "available_physical", "commit_limit", "available_commit")
    }
    return caller | {
        "system_commit_total_bytes": total,
        "system_commit_limit_bytes": limit,
        "system_available_commit_bytes": available,
        "observed_at_unix_ns": observed_at,
        "observation_started_perf_counter_ns": started,
        "observation_span_ns": time.perf_counter_ns() - started,
    }


def host_memory_snapshot() -> dict[str, int] | None:
    """Read current Windows headroom; return None only on unsupported platforms.

    Query/configuration errors propagate to the caller, which owns its admission
    policy. A failed observation never becomes fabricated zero/unlimited headroom.

    Returns:
        A fresh timestamped caller/system image, or None when os.name is not nt.
    """
    if os.name != "nt":
        return None
    return windows_host_memory_snapshot(_windows_kernel())


def stem_memory_snapshot() -> dict[str, int] | None:
    """Read host headroom and the system-wide Windows low-memory signal."""
    if os.name != "nt":
        return None
    kernel = _windows_kernel()
    image = windows_host_memory_snapshot(kernel)
    # LowMemoryResourceNotification. Each sample closes its own handle.
    handle = kernel.CreateMemoryResourceNotification(0)
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    low_memory = wintypes.BOOL()
    try:
        if not kernel.QueryMemoryResourceNotification(handle, ctypes.byref(low_memory)):
            raise ctypes.WinError(ctypes.get_last_error())
    finally:
        closed = kernel.CloseHandle(handle)
    if not closed:
        raise ctypes.WinError(ctypes.get_last_error())
    return image | {"low_memory": int(bool(low_memory.value))}
