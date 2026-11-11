"""Compare the explicit Native CPU reduction profile on identical input values."""

import argparse
import hashlib
import importlib.metadata
import importlib.util
import json
import logging
import os
import platform
import subprocess
import sys
from pathlib import Path

import numpy as np
import torch
from demucs.htdemucs import HTDemucs
from demucs_host_trial_reference import ATOL, RTOL, compare, execute, signal
from demucs_hybrid_trial_reference import SOURCE_PINS, TORCH_GIT
from torch.nn import functional

TORCH_VERSION = "2.12.0+cu130"
SEPARATE_SHA = "3ca74c33421850682ceac3582305f4d46e2c296de96f8db69ddb2cf0568d9f79"
logger = logging.getLogger(__name__)


def identity() -> list:
    """Check the pinned original environment without mutating Torch dispatch."""
    assert importlib.metadata.version("demucs") == "4.0.1"
    assert torch.__version__ == TORCH_VERSION
    assert torch.version.git_version == TORCH_GIT
    assert sys.platform == "win32"
    assert platform.machine() == "AMD64"
    assert os.environ.get("ATEN_CPU_CAPABILITY") is None
    assert torch.backends.cpu.get_cpu_capability() == "AVX512"
    assert torch.get_num_threads() == 1
    parallel = torch.__config__.parallel_info()
    assert "ATen parallel backend: OpenMP" in parallel
    assert "omp_get_max_threads() : 1" in parallel
    module = importlib.util.find_spec("demucs.htdemucs")
    assert module is not None
    assert module.origin is not None
    source = Path(module.origin).parent
    for name, pin in {**SOURCE_PINS, "separate.py": SEPARATE_SHA}.items():
        assert hashlib.sha256((source / name).read_bytes()).hexdigest() == pin, name
    return [torch.__version__, torch.version.git_version, "AVX512", torch.get_num_threads()]


def native_order(tensor: torch.Tensor, *, interleaved: bool) -> np.ndarray:
    if interleaved:
        tensor = tensor.transpose(-1, -2)
    assert tensor.is_contiguous()
    return tensor.flatten().numpy()


def reference_values(tensor: torch.Tensor, *, cli: bool, interleaved: bool) -> np.ndarray:
    """Keep the original reduction axes and separate arithmetic operations."""
    if cli:
        ref = tensor.mean(0)
        mean, std = ref.mean(), ref.std()
        assert float(std) > 0.0
    else:
        dims = tuple(range(1, tensor.ndim))
        mean = tensor.mean(dim=dims, keepdim=True)
        std = tensor.std(dim=dims, keepdim=True)
    denominator = std if cli else std + 1e-5
    normalized = (tensor - mean) / denominator
    restored = normalized * std + mean
    return np.concatenate([
        torch.stack([mean.flatten(), std.flatten()], dim=1).numpy().flatten(),
        native_order(normalized, interleaved=interleaved),
        native_order(restored, interleaved=interleaved),
    ])


def trial(exe: Path, folder: Path, profile: list, mode: str, tensor: torch.Tensor) -> dict:
    """Compare exact scalar bits, then every normalized and restored value."""
    strides = list(tensor.stride())
    interleaved = mode != "cac" and strides[-2:] == [1, 2]
    shape = [1, *tensor.shape] if mode == "cli" else list(tensor.shape)
    expected = reference_values(tensor, cli=mode == "cli", interleaved=interleaved)
    actual = execute(
        exe,
        folder,
        [f"cpu-{mode}", *profile, shape[0], shape[1], shape[-1], *strides],
        native_order(tensor, interleaved=interleaved),
    )
    assert actual.size == expected.size
    scalars = 2 * shape[0]
    count = tensor.numel()
    scalar_mismatches = int(
        np.count_nonzero(expected[:scalars].view(np.uint32) != actual[:scalars].view(np.uint32))
    )
    normalized = compare(actual[scalars : scalars + count], expected[scalars : scalars + count])
    restored = compare(actual[-count:], expected[-count:])
    return {
        "case": folder.name,
        "mode": mode,
        "shape": list(tensor.shape),
        "strides": strides,
        "expected_scalar_bits": expected[:scalars].view(np.uint32).reshape(-1, 2).tolist(),
        "actual_scalar_bits": actual[:scalars].view(np.uint32).reshape(-1, 2).tolist(),
        "scalar_mismatches": scalar_mismatches,
        "normalized": normalized,
        "restored": restored,
        "passed": scalar_mismatches == 0 and normalized["passed"] and restored["passed"],
    }


def actual_branches(training: int, pcm: torch.Tensor) -> tuple[torch.Tensor, torch.Tensor]:
    """Keep original right-pad and CaC reshape, without constructing learned layers."""
    model = object.__new__(HTDemucs)
    model.cac, model.nfft, model.hop_length = True, 4096, 1024
    if pcm.shape[-1] < training:
        pcm = functional.pad(pcm, (0, training - pcm.shape[-1]))
    features = model._magnitude(model._spec(pcm))
    return pcm, features


def boundaries():
    """Signed cancellation makes changes in lane grouping and scalar tail visible."""
    for frames in (
        2,
        7,
        8,
        9,
        15,
        16,
        17,
        31,
        32,
        33,
        511,
        512,
        513,
        8191,
        8192,
        8193,
        32767,
        32768,
        32769,
        131071,
        131072,
        131073,
    ):
        pcm = torch.ones((1, 1, frames), dtype=torch.float32)
        pcm[..., 0] = 1e8
        if frames > 2:
            pcm[..., min(8, frames - 1)] = -1e8
            pcm[..., -1] += 2
        yield f"boundary-{frames}", "time", pcm


def cases():
    """Use distinct B, C and F/T axes and the original failed High-DC signal."""
    ordinary = torch.from_numpy(signal(343980))
    old_dc = torch.from_numpy(
        (np.float32(128) + signal(4097) * np.float32(0.001)).astype(np.float32)
    )
    near = torch.full((1, 2, 1025), 0.125, dtype=torch.float32)
    near[0, 0, 512] = torch.nextafter(near[0, 0, 512], torch.tensor(1.0))
    yield "high-dc-time-b1", "time", old_dc[None]
    yield "high-dc-time-b2", "time", torch.stack([old_dc, old_dc + 16])
    yield "high-dc-cli-planar", "cli", old_dc
    yield "high-dc-cli-interleaved", "cli", old_dc.T.contiguous().T
    yield "ordinary-cli-stereo", "cli", ordinary
    yield "ordinary-cli-mono", "cli", ordinary[:1]
    yield "near-constant", "time", near
    yield "near-constant-b2", "time", torch.cat([near, near + 2], dim=0)
    yield "zero", "time", torch.zeros((2, 1, 2), dtype=torch.float32)
    yield "constant", "time", torch.full((1, 2, 29), 0.5, dtype=torch.float32)
    yield "full-time-interleaved-b1", "time", ordinary.T.contiguous().T[None]
    yield (
        "high-dc-time-interleaved-b2",
        "time",
        torch.stack([old_dc, old_dc + 16]).transpose(-1, -2).contiguous().transpose(-1, -2),
    )
    for tag, pcm, training in (
        ("release", ordinary[None], 343980),
        ("release-short", ordinary[None, :, :343041].contiguous(), 343980),
        ("training-pad-b2", torch.stack([old_dc[:, :3073], old_dc[:, :3073] - 150]), 4097),
        ("one-spectral-frame", ordinary[None, :, :29].contiguous(), 29),
    ):
        time, cac = actual_branches(training, pcm)
        yield f"{tag}-time", "time", time
        yield f"{tag}-cac", "cac", cac
        if tag == "training-pad-b2":
            # Observe small variance near a large offset on an actual CaC layout.
            # Feed the same modified tensor to both reducers.
            scaled = cac / cac.abs().amax() * 0.003
            offset = torch.tensor([128.0, -64.0], dtype=torch.float32)[:, None, None, None]
            yield "high-dc-cac-b2", "cac", scaled + offset
    yield from boundaries()


def refusals(exe: Path, folder: Path, profile: list) -> list:
    folder.mkdir()
    output = []
    test = [
        ("multithread", [*profile[:3], 16], ["cpu-time", 1, 2, 2, 4, 2, 1], 4),
        ("mono-cac-layout", profile, ["cpu-cac", 1, 2, 1, 2, 1, 2, 4098], 4096),
        ("cli-zero-std", profile, ["cpu-cli", 1, 2, 2, 2, 1], 4),
        ("cli-singleton", profile, ["cpu-cli", 1, 2, 1, 1, 1], 2),
    ]
    for name, dispatch, args, count in test:
        source, stdout, stderr = (folder / f"{name}.{ext}" for ext in ("f32", "out", "err"))
        np.zeros(count, dtype="<f4").tofile(source)
        with source.open("rb") as reader, stdout.open("xb") as writer, stderr.open("xb") as errors:
            result = subprocess.run(
                [str(exe), args[0], *map(str, dispatch), *map(str, args[1:])],
                stdin=reader,
                stdout=writer,
                stderr=errors,
                check=False,
            )
        output.append({
            "case": name,
            "exit_code": result.returncode,
            "diagnostic": stderr.read_text(encoding="utf-8").strip(),
            "passed": result.returncode == 1 and stdout.stat().st_size == 0,
        })
    return output


def large_cascade(exe: Path, folder: Path, profile: list) -> list:
    """Cross the first cascade level-power change with a scalar-only bridge."""
    output = []
    for size in (16777216, 16777248):
        tensor = torch.ones((1, 1, size), dtype=torch.float32)
        tensor[..., 0], tensor[..., 8], tensor[..., -1] = 1e8, -1e8, 3.0
        mean, std = tensor.mean(dim=(1, 2)), tensor.std(dim=(1, 2))
        expected = torch.stack([mean, std], dim=1).numpy().flatten().view(np.uint32)
        actual = execute(
            exe,
            folder / f"large-{size}",
            ["cpu-time-stats", *profile, 1, 1, size, size, size, 1],
            tensor.numpy(),
        )
        assert actual.size == 2
        got_bits = actual.view(np.uint32)
        output.append({
            "case": f"large-cascade-{size}",
            "scalar_only": True,
            "values_reduced": size,
            "expected_scalar_bits": expected.tolist(),
            "actual_scalar_bits": got_bits.tolist(),
            "passed": bool(np.array_equal(got_bits, expected)),
        })
    return output


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    profile = identity()
    args.output.mkdir()
    with torch.inference_mode():
        results = [
            trial(args.executable, args.output / name, profile, mode, tensor)
            for name, mode, tensor in cases()
        ]
    results += large_cascade(args.executable, args.output, profile)
    negative = refusals(args.executable, args.output / "refusals", profile)
    report = {
        "scope": "same-tensor CPU f32 statistics and complete normalize/restore arithmetic",
        "python": sys.version,
        "profile": profile,
        "interop_threads": torch.get_num_interop_threads(),
        "parallel": torch.__config__.parallel_info(),
        "source_pins": {**SOURCE_PINS, "separate.py": SEPARATE_SHA},
        "executable_sha256": hashlib.sha256(args.executable.read_bytes()).hexdigest(),
        "atol": ATOL,
        "rtol": RTOL,
        "cases": results,
        "refusals": negative,
        "passed": all(r["passed"] for r in results + negative),
    }
    (args.output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    failures = [r["case"] for r in results + negative if not r["passed"]]
    logger.info(
        "%d numerical cases, %d refusals, failures %s", len(results), len(negative), failures
    )
    if failures:
        msg = f"original CPU reduction comparison failed: {failures}"
        raise AssertionError(msg)


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(message)s")
    main()
