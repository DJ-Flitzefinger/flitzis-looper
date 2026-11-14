"""Validate BS CUDA worker memory observations and query selected-device free bytes."""

import ctypes
import os
import re
from dataclasses import dataclass
from typing import Annotated, Final, Literal

from pydantic import BeforeValidator

if os.name == "nt":
    from _ctypes import FreeLibrary  # noqa: PLC2701 -- Release the explicitly loaded DLL.

_KIND: Final = "dxgi_wddm_local_sampled"
_MAX_BYTES = (1 << 63) - 1
_SYSTEM32 = 0x800
_CUDA_UUID = re.compile(r"GPU-[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}")


@dataclass(frozen=True, slots=True)
class CudaMemoryObservation:
    """Process-local WDDM usage and budget sampled while the model was live."""

    kind: Literal["dxgi_wddm_local_sampled"]
    observed_peak_bytes: int
    minimum_budget_bytes: int
    first_usage_bytes: int
    last_usage_bytes: int
    samples: int


def parse_cuda_memory(value: object) -> CudaMemoryObservation | None:
    """Missing, unsupported or inconsistent optional telemetry stays unknown."""
    if not isinstance(value, dict) or len(value) != 6 or value.get("kind") != _KIND:
        return None
    peak = value.get("observed_peak_bytes")
    budget = value.get("minimum_budget_bytes")
    first = value.get("first_usage_bytes")
    last = value.get("last_usage_bytes")
    samples = value.get("samples")
    if (
        type(peak) is not int
        or type(budget) is not int
        or type(first) is not int
        or type(last) is not int
        or type(samples) is not int
    ):
        return None
    if (
        0 < peak <= _MAX_BYTES
        and 0 <= budget <= _MAX_BYTES
        and 0 <= first <= peak
        and 0 <= last <= peak
        and 2 <= samples <= _MAX_BYTES
    ):
        return CudaMemoryObservation(_KIND, peak, budget, first, last, samples)
    return None


CudaMemoryReceipt = Annotated[CudaMemoryObservation | None, BeforeValidator(parse_cuda_memory)]


class _NvmlMemoryV2(ctypes.Structure):
    _fields_ = (
        ("version", ctypes.c_uint),
        ("total", ctypes.c_uint64),
        ("reserved", ctypes.c_uint64),
        ("free", ctypes.c_uint64),
        ("used", ctypes.c_uint64),
    )


def nvml_free_bytes(device_uuid: str) -> int | None:
    """Sample NVML's current device-wide free bytes by UUID, without a CUDA context."""
    if os.name != "nt" or _CUDA_UUID.fullmatch(device_uuid) is None:
        return None
    try:
        nvml = ctypes.CDLL("nvml.dll", winmode=_SYSTEM32)
        try:
            return _query_free_bytes(nvml, device_uuid)
        finally:
            FreeLibrary(nvml._handle)
    except AttributeError, OSError:
        return None


def _query_free_bytes(nvml: ctypes.CDLL, device_uuid: str) -> int | None:
    initialize = nvml.nvmlInit_v2
    initialize.argtypes = ()
    initialize.restype = ctypes.c_int
    shutdown = nvml.nvmlShutdown
    shutdown.argtypes = ()
    shutdown.restype = ctypes.c_int
    device_by_uuid = nvml.nvmlDeviceGetHandleByUUID
    device_by_uuid.argtypes = (ctypes.c_char_p, ctypes.POINTER(ctypes.c_void_p))
    device_by_uuid.restype = ctypes.c_int
    get_memory = nvml.nvmlDeviceGetMemoryInfo_v2
    get_memory.argtypes = (ctypes.c_void_p, ctypes.POINTER(_NvmlMemoryV2))
    get_memory.restype = ctypes.c_int
    device = ctypes.c_void_p()
    # NVIDIA's NVML_STRUCT_VERSION macro combines sizeof and the version.
    memory = _NvmlMemoryV2(ctypes.sizeof(_NvmlMemoryV2) | (2 << 24))
    if initialize() != 0:
        return None
    try:
        sampled = (
            device_by_uuid(device_uuid.encode("ascii"), ctypes.byref(device)) == 0
            and device.value is not None
            and get_memory(device, ctypes.byref(memory)) == 0
        )
    finally:
        stopped = shutdown() == 0
    if sampled and stopped and 0 <= memory.free <= memory.total <= _MAX_BYTES and memory.total > 0:
        return int(memory.free)
    return None
