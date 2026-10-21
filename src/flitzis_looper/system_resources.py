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
from functools import cache


class HostMemory(ctypes.Structure):
    """Win32 MEMORYSTATUSEX layout, including caller-specific commit bytes."""

    _fields_ = [
        ("length", wintypes.DWORD),
        ("load", wintypes.DWORD),
        ("total_physical", ctypes.c_ulonglong),
        ("available_physical", ctypes.c_ulonglong),
        ("commit_limit", ctypes.c_ulonglong),
        ("available_commit", ctypes.c_ulonglong),
        ("total_virtual", ctypes.c_ulonglong),
        ("available_virtual", ctypes.c_ulonglong),
        ("available_extended_virtual", ctypes.c_ulonglong),
    ]


class PerformanceInformation(ctypes.Structure):
    """Win32 PERFORMANCE_INFORMATION layout; commit quantities are page counts."""

    _fields_ = [
        ("size", wintypes.DWORD),
        *[
            (name, ctypes.c_size_t)
            for name in (
                "commit_total",
                "commit_limit",
                "commit_peak",
                "physical_total",
                "physical_available",
                "system_cache",
                "kernel_total",
                "kernel_paged",
                "kernel_nonpaged",
                "page_size",
            )
        ],
        ("handles", wintypes.DWORD),
        ("processes", wintypes.DWORD),
        ("threads", wintypes.DWORD),
    ]


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


@cache
def _windows_kernel() -> ctypes.WinDLL:
    # cache publishes only fully configured objects. Concurrent initial calls may
    # configure independent WinDLL wrappers; each wrapper has identical signatures.
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.GlobalMemoryStatusEx.argtypes = [ctypes.POINTER(HostMemory)]
    kernel.GlobalMemoryStatusEx.restype = wintypes.BOOL
    kernel.K32GetPerformanceInfo.argtypes = [
        ctypes.POINTER(PerformanceInformation),
        wintypes.DWORD,
    ]
    kernel.K32GetPerformanceInfo.restype = wintypes.BOOL
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
