"""Disposable CUDA capability query; its result is never an execution receipt."""

import ctypes
import hashlib
import json
import os
import platform
import sys
from contextlib import suppress

import torch


def _driver_release() -> str | None:
    """Read the installed NVIDIA driver release, not its supported CUDA API level."""
    nvml = ctypes.CDLL("nvml.dll" if os.name == "nt" else "libnvidia-ml.so.1")
    nvml.nvmlInit_v2.argtypes = []
    nvml.nvmlInit_v2.restype = ctypes.c_int
    nvml.nvmlShutdown.argtypes = []
    nvml.nvmlShutdown.restype = ctypes.c_int
    nvml.nvmlSystemGetDriverVersion.argtypes = [ctypes.c_char_p, ctypes.c_uint]
    nvml.nvmlSystemGetDriverVersion.restype = ctypes.c_int
    if nvml.nvmlInit_v2() != 0:
        return None
    try:
        release = ctypes.create_string_buffer(80)
        if nvml.nvmlSystemGetDriverVersion(release, len(release)) != 0:
            return None
        value = release.value.decode("ascii")
        return value if value and all(char in "0123456789." for char in value) else None
    finally:
        nvml.nvmlShutdown()


def _device_identity() -> dict[str, object]:
    """Return no identity when a required driver/UUID observation is missing."""
    driver = ctypes.CDLL("nvcuda.dll" if os.name == "nt" else "libcuda.so.1")
    driver.cuDriverGetVersion.argtypes = [ctypes.POINTER(ctypes.c_int)]
    driver.cuDriverGetVersion.restype = ctypes.c_int
    version = ctypes.c_int()
    if driver.cuDriverGetVersion(ctypes.byref(version)) != 0 or version.value <= 0:
        return {}
    release = _driver_release()
    if release is None:
        return {}
    properties = torch.cuda.get_device_properties(0)
    uuid = str(getattr(properties, "uuid", ""))
    if not uuid:
        return {}
    free, total = torch.cuda.mem_get_info(0)
    identity = (
        platform.platform(),
        platform.machine(),
        platform.python_version(),
        torch.__version__,
        torch.version.cuda,
        version.value,
        release,
        uuid,
        properties.name,
        properties.major,
        properties.minor,
        total,
    )
    return {
        "identity": hashlib.sha256(repr(identity).encode()).hexdigest(),
        "free_bytes": free,
    }


def main() -> None:
    """Report actual driver/device identity and current free device bytes if known."""
    result: dict[str, object] = {"available": torch.cuda.is_available()}
    if result["available"]:
        with suppress(OSError, ValueError, RuntimeError, AttributeError):
            result |= _device_identity()
    sys.stdout.write(json.dumps(result, separators=(",", ":")) + "\n")
    sys.stdout.flush()


if __name__ == "__main__":
    main()
