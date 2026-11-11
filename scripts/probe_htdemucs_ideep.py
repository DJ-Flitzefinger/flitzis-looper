"""Observe iDeep allocations for one exact, checkpoint-free HTDemucs convolution."""

import argparse
import ctypes
import hashlib
import json
import os
import platform
import sys
import sysconfig
from dataclasses import dataclass
from importlib import import_module
from pathlib import Path

TORCH_COMMIT = "7661cd9c6b841b62b7f411aa52ec51f05457263b"
TORCH_CPU_DLL = "de412dd735a2d880bd40f0ef88f2333af846e698e28065912026b650ae34fd1a"
VERBOSE = "profile_create,profile_exec"


@dataclass(frozen=True)
class Geometry:
    """Dense F32 shapes and spatial options at the ordinary ATen entry point."""

    source: tuple[int, ...]
    weights: tuple[int, ...]
    output: tuple[int, ...]
    stride: tuple[int, ...]
    padding: tuple[int, ...]
    dilation: tuple[int, ...]
    transpose: bool = False
    fold_spectral: bool = False


GEOMETRIES = {
    "spectral-encoder": Geometry(
        (1, 4, 2048, 336), (48, 4, 8, 1), (1, 48, 512, 336), (4, 1), (2, 0), (1, 1)
    ),
    "spectral-rewrite": Geometry(
        (1, 48, 512, 336), (96, 48, 3, 3), (1, 96, 512, 336), (1, 1), (1, 1), (1, 1)
    ),
    "spectral-transpose": Geometry(
        (1, 48, 512, 336),
        (48, 16, 8, 1),
        (1, 16, 2052, 336),
        (4, 1),
        (0, 0),
        (1, 1),
        transpose=True,
    ),
    "folded-dconv-k3": Geometry(
        (512, 48, 336), (6, 48, 3), (512, 6, 336), (1,), (1,), (1,), fold_spectral=True
    ),
    "folded-dconv-d2": Geometry(
        (512, 48, 336), (6, 48, 3), (512, 6, 336), (1,), (2,), (2,), fold_spectral=True
    ),
    "folded-dconv-k1": Geometry((512, 6, 336), (96, 6, 1), (512, 96, 336), (1,), (0,), (1,)),
    "waveform-encoder": Geometry((1, 2, 343980), (48, 2, 8), (1, 48, 85995), (4,), (2,), (1,)),
    "waveform-rewrite": Geometry((1, 48, 85995), (96, 48, 3), (1, 96, 85995), (1,), (1,), (1,)),
    "waveform-transpose": Geometry(
        (1, 48, 85995), (48, 8, 8), (1, 8, 343984), (4,), (0,), (1,), transpose=True
    ),
}


class Event(ctypes.Structure):
    """One allocation or free from the recorder's fixed-size event buffer."""

    _fields_ = [
        ("id", ctypes.c_uint64),
        ("requested", ctypes.c_uint64),
        ("usable", ctypes.c_uint64),
        ("allocated", ctypes.c_uint64),
    ]


def sha256(path: Path) -> str:
    """Hash a pinned binary without allocating a buffer of its file size."""
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def emit(**record) -> None:
    """Write a receipt record between the corresponding oneDNN verbose rows."""
    sys.stdout.write(json.dumps(record) + "\n")
    sys.stdout.flush()


def recorder_for(path: Path, expected: str, cpu_dll: Path):
    """Bind the recorder to the already loaded, pinned wheel CPU image."""
    if sha256(path) != expected:
        msg = "unexpected HTDemucs recorder bytes"
        raise ValueError(msg)
    recorder = ctypes.WinDLL(str(path))
    recorder.install_ideep_recorder.argtypes = [ctypes.c_wchar_p]
    recorder.install_ideep_recorder.restype = ctypes.c_int
    recorder.read_ideep_events.argtypes = [ctypes.POINTER(Event), ctypes.c_size_t]
    recorder.read_ideep_events.restype = ctypes.c_int
    if status := recorder.install_ideep_recorder(str(cpu_dll)):
        msg = f"iDeep recorder installation failed: {status}"
        raise RuntimeError(msg)
    return recorder


def allocation_receipt(recorder, cursor: int) -> tuple[int, dict]:
    """Check the complete per-call stream and compute live requested and usable maxima."""
    events = (Event * 512)()
    count = recorder.read_ideep_events(events, len(events))
    if count < cursor:
        msg = "iDeep recorder lost allocation events"
        raise RuntimeError(msg)
    live = {}
    requested = usable = max_requested = max_usable = 0
    records = []
    for event in events[cursor:count]:
        allocation = (event.requested, event.usable)
        if event.allocated == 1 and event.id not in live:
            live[event.id] = allocation
            requested += event.requested
            usable += event.usable
            max_requested = max(max_requested, requested)
            max_usable = max(max_usable, usable)
        elif event.allocated == 0 and live.pop(event.id, None) == allocation:
            requested -= event.requested
            usable -= event.usable
        else:
            msg = "unexpected iDeep allocation/free pairing"
            raise RuntimeError(msg)
        records.append([event.id, event.allocated, *allocation])
    if live:
        msg = "iDeep allocations remain live at the geometry boundary"
        raise RuntimeError(msg)
    return count, {
        "events": records,
        "peakRequestedBytes": max_requested,
        "peakUsableBytes": max_usable,
    }


def run_geometry(torch, recorder, name: str, geometry: Geometry, *, cold_only: bool) -> None:
    """Run stock dispatch twice with the exact dense or folded source layout."""
    if geometry.fold_spectral:
        batch, channels, frames = geometry.source
        dense = torch.full((1, channels, batch, frames), 0.125, dtype=torch.float32)
        source = dense.permute(0, 2, 1, 3).view(geometry.source)
    else:
        source = torch.full(geometry.source, 0.125, dtype=torch.float32)
    weight = torch.full(geometry.weights, 0.0625, dtype=torch.float32)
    bias = torch.zeros(geometry.output[1], dtype=torch.float32)
    output_padding = (0,) * len(geometry.stride)
    emit(
        geometry=name,
        source=list(source.shape),
        sourceStrides=list(source.stride()),
        weights=list(weight.shape),
        weightStrides=list(weight.stride()),
        stride=geometry.stride,
        padding=geometry.padding,
        dilation=geometry.dilation,
    )
    cursor, baseline = allocation_receipt(recorder, 0)
    if baseline["events"]:
        msg = "iDeep allocation stream was not empty before stock execution"
        raise RuntimeError(msg)
    for iteration in ("cold",) if cold_only else ("cold", "warm"):
        emit(geometry=name, iteration=iteration, started=True)
        output = torch.ops.aten.convolution.default(
            source,
            weight,
            bias,
            geometry.stride,
            geometry.padding,
            geometry.dilation,
            geometry.transpose,
            output_padding,
            1,
        )
        if tuple(output.shape) != geometry.output or output.dtype != torch.float32:
            msg = "stock convolution produced an unexpected geometry"
            raise RuntimeError(msg)
        result = {
            "output": list(output.shape),
            "outputStrides": list(output.stride()),
            "denseOutputBytes": output.untyped_storage().nbytes(),
            "first": output[(0,) * output.ndim].item(),
        }
        del output
        cursor, allocations = allocation_receipt(recorder, cursor)
        emit(geometry=name, iteration=iteration, **result, **allocations)
    # ATen's query itself packs strided 1D inputs. Keep it out of the cold/warm envelope.
    backend = torch._C._select_conv_backend(
        source,
        weight,
        bias,
        geometry.stride,
        geometry.padding,
        geometry.dilation,
        geometry.transpose,
        output_padding,
        1,
    )
    emit(geometry=name, backend=backend.name, backendCode=backend.value)


def main() -> None:
    """Run in a disposable Windows Job with no checkpoint or release artifact."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("geometry", choices=GEOMETRIES)
    parser.add_argument("--cpu-overlay", type=Path, required=True)
    parser.add_argument("--recorder", type=Path, required=True)
    parser.add_argument("--recorder-sha256", required=True)
    parser.add_argument("--cold-only", action="store_true")
    args = parser.parse_args()
    if (
        sys.platform != "win32"
        or sys.version_info[:2] != (3, 14)
        or platform.machine() != "AMD64"
        or sysconfig.get_config_var("Py_GIL_DISABLED")
        or os.environ.get("ONEDNN_VERBOSE") != VERBOSE
    ):
        msg = "requires Windows x64 CP314 and exact oneDNN creation/execution verbosity"
        raise ValueError(msg)
    overlay = args.cpu_overlay.resolve(strict=True)
    cpu_dll = overlay / "torch/lib/torch_cpu.dll"
    if sha256(cpu_dll) != TORCH_CPU_DLL:
        msg = "unexpected Torch CPU DLL bytes before import"
        raise ValueError(msg)
    sys.path.insert(0, str(overlay))
    torch = import_module("torch")

    if (
        Path(torch.__file__).resolve(strict=True) != overlay / "torch/__init__.py"
        or torch.__version__ != "2.12.0+cpu"
        or torch.version.git_version != TORCH_COMMIT
        or any(getattr(torch.version, name) is not None for name in ("cuda", "hip", "xpu"))
    ):
        msg = "unexpected Torch CPU wheel"
        raise ValueError(msg)
    torch.set_num_threads(1)
    torch.set_num_interop_threads(1)
    if (torch.get_num_threads(), torch.get_num_interop_threads()) != (1, 1):
        msg = "could not establish serial stock dispatch"
        raise RuntimeError(msg)
    recorder = recorder_for(args.recorder.resolve(strict=True), args.recorder_sha256, cpu_dll)
    emit(
        processor=platform.processor(),
        torchCpuCapability=torch.backends.cpu.get_cpu_capability(),
        config=torch.__config__.show(),
        intraop=1,
        interop=1,
    )
    with torch.inference_mode():
        run_geometry(
            torch, recorder, args.geometry, GEOMETRIES[args.geometry], cold_only=args.cold_only
        )


if __name__ == "__main__":
    main()
