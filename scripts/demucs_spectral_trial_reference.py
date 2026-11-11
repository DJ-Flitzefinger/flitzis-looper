"""Compare independent Rust HTDemucs arithmetic to unchanged Demucs 4.0.1.

No model is constructed and no weights or inference are loaded. The executable
is built once by the caller. Private regular input/output files remain owned by
this supervisor through child return and all Python file readers. It creates no
pipe readers and grants no application/producer, runtime or cutover acceptance.
"""

import argparse
import hashlib
import importlib.metadata
import json
import logging
import subprocess
from pathlib import Path
from types import SimpleNamespace

import numpy as np
import torch
from demucs.htdemucs import HTDemucs

# Existing independent FFT arithmetic ceilings, declared before execution.
# These do not replace the original full-model learned-mask/PCM criteria.
SPEC_ATOL = 2e-4
SPEC_RTOL = 2e-5
PCM_ATOL = 2e-5
PCM_RTOL = 2e-5
LENGTHS = (1, 1024, 1025, 1536, 2048, 4097, 343040, 343980, 344065, 705601)
logger = logging.getLogger(__name__)


def compare(actual: np.ndarray, original: np.ndarray, atol: float, rtol: float) -> dict:
    """Compare every scalar with the original as the relative reference."""
    if actual.shape != original.shape:
        msg = f"geometry differs: {actual.shape} != {original.shape}"
        raise ValueError(msg)
    delta = np.abs(actual.astype(np.float64) - original.astype(np.float64))
    failed = (~np.isfinite(actual)) | (delta > atol + rtol * np.abs(original))
    return {
        "scalars": int(actual.size),
        "max_abs": float(delta.max()),
        "failed": int(failed.sum()),
        "edge_max_abs": float(delta[..., : min(2048, delta.shape[-1])].max()),
        "tail_max_abs": float(delta[..., -min(2048, delta.shape[-1]) :].max()),
        "atol": atol,
        "rtol": rtol,
    }


def execute(exe: Path, folder: Path, mode: str, values: np.ndarray, length: int) -> np.ndarray:
    """Use regular files; close every supervisor owner after actual child return."""
    source = folder / f"{mode}.input.f32"
    output = folder / f"{mode}.output.f32"
    errors = folder / f"{mode}.stderr"
    values.astype("<f4").tofile(source)
    with source.open("rb") as reader, output.open("xb") as writer, errors.open("xb") as stderr:
        subprocess.run(
            [str(exe), mode, str(length), str(values.shape[0])],
            stdin=reader,
            stdout=writer,
            stderr=stderr,
            check=True,
        )
    return np.fromfile(output, dtype="<f4")


def reconstruction(
    exe: Path, folder: Path, shape: tuple, length: int, rng: np.random.Generator
) -> dict:
    """Test arbitrary complex outputs, independently of the forward arithmetic."""
    holder = SimpleNamespace(nfft=4096, hop_length=1024)
    # Every frequency/time/component/lane is active, including imaginary DC.
    independent = rng.normal(0, 0.2, shape).astype(np.float32)
    original_z = torch.view_as_complex(
        torch.from_numpy(independent).permute(0, 2, 3, 1).contiguous()
    )
    expected = HTDemucs._ispec(holder, original_z[None], length)[0].numpy()
    inverse = execute(exe, folder, "reconstruct", independent, length).reshape(expected.shape)
    return compare(inverse, expected, PCM_ATOL, PCM_RTOL)


def case(exe: Path, folder: Path, length: int, rng: np.random.Generator) -> dict:
    """Compare all samples/bins for one physical length and its original tail."""
    folder.mkdir()
    time = np.arange(length, dtype=np.float32)
    pcm = np.stack([
        0.15 + 0.18 * np.sin(time * 0.037) + 0.07 * (-1.0) ** time,
        -0.07 + 0.11 * np.cos(time * 0.071) + rng.normal(0, 0.08, length),
    ]).astype(np.float32)
    if length == 1536:
        pcm.fill(0)
    else:
        pcm[:, 0] += [0.5, -0.3]
        pcm[:, -1] += [-0.2, 0.4]
    original = HTDemucs._spec(
        SimpleNamespace(nfft=4096, hop_length=1024), torch.from_numpy(pcm)[None]
    )
    cac = torch.view_as_real(original).permute(0, 1, 4, 2, 3).contiguous()[0].numpy()
    native = execute(exe, folder, "analyze", pcm, length).reshape(cac.shape)
    inverse_shape = (8, *cac.shape[1:]) if length == 343980 else cac.shape
    result = {
        "frames": length,
        "steps": int(cac.shape[-1]),
        "inverse_lanes": inverse_shape[0],
        "forward": compare(native, cac, SPEC_ATOL, SPEC_RTOL),
        "inverse_independent": reconstruction(exe, folder, inverse_shape, length, rng),
    }
    logger.info("%s", json.dumps(result))
    return result


def main() -> None:
    """Run one risk set with the same exact executable and original methods."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    if importlib.metadata.version("demucs") != "4.0.1":
        msg = "the unchanged pinned Demucs 4.0.1 is required"
        raise ValueError(msg)
    torch.set_num_threads(1)
    torch.set_num_interop_threads(1)
    logging.basicConfig(level=logging.INFO, format="%(message)s")
    rng = np.random.default_rng(935717)
    results = [case(args.executable, args.output / str(n), n, rng) for n in LENGTHS]
    report = {
        "scope": "HTDemucs scale-zero spectral arithmetic only, no model/weights/inference",
        "executable_sha256": hashlib.sha256(args.executable.read_bytes()).hexdigest(),
        "demucs": importlib.metadata.version("demucs"),
        "torch": torch.__version__,
        "cases": results,
        "passed": all(
            not r[phase]["failed"] for r in results for phase in ("forward", "inverse_independent")
        ),
    }
    (args.output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    if not report["passed"]:
        msg = "HTDemucs original arithmetic comparison failed"
        raise AssertionError(msg)


if __name__ == "__main__":
    main()
