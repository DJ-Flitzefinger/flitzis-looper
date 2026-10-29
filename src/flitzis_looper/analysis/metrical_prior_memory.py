"""Query one owned worker while its original Popen handle prevents PID reuse."""

import ctypes
import os
from ctypes import wintypes


class _Counters(ctypes.Structure):
    _fields_ = (
        ("cb", wintypes.DWORD),
        ("faults", wintypes.DWORD),
        ("peak_working_set", ctypes.c_size_t),
        ("working_set", ctypes.c_size_t),
        ("peak_paged", ctypes.c_size_t),
        ("paged", ctypes.c_size_t),
        ("peak_nonpaged", ctypes.c_size_t),
        ("nonpaged", ctypes.c_size_t),
        ("pagefile", ctypes.c_size_t),
        ("peak_pagefile", ctypes.c_size_t),
        ("private", ctypes.c_size_t),
    )


class PriorMemoryObserver:
    """No new process or sampler; the existing command owner calls bind/check."""

    def __init__(self, native_live_bytes: int, limit_bytes: int = 536870912) -> None:
        if type(native_live_bytes) is not int or not 0 <= native_live_bytes <= limit_bytes:
            message = "Invalid prior Native ownership charge"
            raise ValueError(message)
        self.native_live_bytes = native_live_bytes
        self.limit_bytes = limit_bytes
        self.handle: int | None = None
        self.peak_bytes = 0
        self.checks = 0
        self.max_working_set_bytes = 0
        self.max_private_bytes = 0
        self.max_peak_working_set_bytes = 0
        self.max_peak_pagefile_bytes = 0

    def bind(self, process_id: int) -> None:
        """Called before activation while the original owned process handle is retained."""
        if os.name != "nt" or self.handle is not None:
            message = "Prior memory observation requires one owned Windows process"
            raise RuntimeError(message)
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
        kernel.OpenProcess.restype = wintypes.HANDLE
        handle = kernel.OpenProcess(0x0410, wintypes.BOOL(0), process_id)
        if not handle:
            raise ctypes.WinError(ctypes.get_last_error())
        self.handle = int(handle)

    def check(self) -> str | None:
        if self.handle is None:
            message = "Prior memory query handle is unavailable"
            raise RuntimeError(message)
        psapi = ctypes.WinDLL("psapi", use_last_error=True)
        psapi.GetProcessMemoryInfo.argtypes = (wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD)
        psapi.GetProcessMemoryInfo.restype = wintypes.BOOL
        counters = _Counters()
        counters.cb = ctypes.sizeof(counters)
        if not psapi.GetProcessMemoryInfo(self.handle, ctypes.byref(counters), counters.cb):
            raise ctypes.WinError(ctypes.get_last_error())
        self.checks += 1
        self.max_working_set_bytes = max(self.max_working_set_bytes, counters.working_set)
        self.max_private_bytes = max(self.max_private_bytes, counters.private)
        self.max_peak_working_set_bytes = max(
            self.max_peak_working_set_bytes, counters.peak_working_set
        )
        self.max_peak_pagefile_bytes = max(self.max_peak_pagefile_bytes, counters.peak_pagefile)
        self.peak_bytes = max(
            self.max_working_set_bytes,
            self.max_private_bytes,
            self.max_peak_working_set_bytes,
            self.max_peak_pagefile_bytes,
        )
        return (
            "resource_pressure"
            if self.native_live_bytes + self.peak_bytes > self.limit_bytes
            else None
        )

    def close(self) -> None:
        if self.handle is not None:
            kernel = ctypes.WinDLL("kernel32", use_last_error=True)
            kernel.CloseHandle.argtypes = (wintypes.HANDLE,)
            kernel.CloseHandle.restype = wintypes.BOOL
            handle = self.handle
            if not kernel.CloseHandle(handle):
                raise ctypes.WinError(ctypes.get_last_error())
            self.handle = None

    def receipt(self) -> dict[str, int]:
        return {
            "checks": self.checks,
            "observed_child_peak_bytes": self.peak_bytes,
            "working_set_bytes": self.max_working_set_bytes,
            "private_bytes": self.max_private_bytes,
            "peak_working_set_bytes": self.max_peak_working_set_bytes,
            "peak_pagefile_bytes": self.max_peak_pagefile_bytes,
            "native_live_bytes": self.native_live_bytes,
            "combined_peak_bytes": self.native_live_bytes + self.peak_bytes,
        }
