"""Original Demucs 4.0.1 host controls with explicit nonneural callbacks.

No learned model is constructed, no weights/storage or inference are loaded.
The one frozen executable is supervised through regular files. Full physical
outputs are compared, including every tail value. Neither these callbacks nor
independent reductions establish learned/CUDA/AMP or productive runtime parity.
"""

import argparse
import hashlib
import importlib.metadata
import json
import logging
import subprocess
from fractions import Fraction
from pathlib import Path

import numpy as np
import torch
from demucs.apply import TensorChunk, apply_model
from demucs.htdemucs import HTDemucs
from torch.nn import functional

ATOL = 2e-5
RTOL = 2e-5
logger = logging.getLogger(__name__)


class HostCallback(HTDemucs):
    """Only a deterministic callback, using HT type/valid_length host dispatch."""

    def __init__(self, training: int, channels: int, sources: int) -> None:
        torch.nn.Module.__init__(self)
        self.segment = Fraction(training, 44100)
        self.samplerate = 44100
        self.sources = [str(i) for i in range(sources)]
        self.audio_channels = channels
        self.use_train_segment = True

    def forward(self, mix: torch.Tensor) -> torch.Tensor:
        """Mirror right training padding/crop around context-sensitive arithmetic."""
        length = mix.shape[-1]
        training = int(self.segment * self.samplerate)
        padded = functional.pad(mix, (0, training - length))
        ramp = torch.arange(training, dtype=torch.float32) * 0.000001
        estimates = [
            padded * ((source + 1) * 0.25)
            + padded.roll(1, -1) * 0.125
            + (padded[..., :1] * 0.0625 + ramp)
            for source in range(len(self.sources))
        ]
        return torch.stack(estimates, 1)[..., :length]


def execute(exe: Path, folder: Path, arguments: list, pcm: np.ndarray) -> np.ndarray:
    """Close regular file owners after child return; no pipe-reader threads."""
    folder.mkdir()
    source, output, errors = (folder / n for n in ("input.f32", "output.f32", "stderr"))
    pcm.astype("<f4").tofile(source)
    with source.open("rb") as reader, output.open("xb") as writer, errors.open("xb") as stderr:
        subprocess.run(
            [str(exe), *map(str, arguments)],
            stdin=reader,
            stdout=writer,
            stderr=stderr,
            check=True,
        )
    return np.fromfile(output, dtype="<f4")


def compare(actual: np.ndarray, expected: np.ndarray) -> dict:
    """Check every value using original reference; fixed arithmetic criteria."""
    if actual.shape != expected.shape:
        msg = f"shape differs: {actual.shape} != {expected.shape}"
        raise ValueError(msg)
    delta = np.abs(actual.astype(np.float64) - expected.astype(np.float64))
    failed = (
        (~np.isfinite(actual)) | (~np.isfinite(expected)) | (delta > ATOL + RTOL * np.abs(expected))
    )
    return {
        "values": int(actual.size),
        "max_abs": float(delta.max()),
        "failed": int(failed.sum()),
        "passed": not bool(failed.any()),
    }


def signal(frames: int, channels: int = 2) -> np.ndarray:
    """Deterministic distinct nonzero channel means and complete edge impulses."""
    time = np.arange(frames, dtype=np.float32)
    pcm = np.stack([
        (channel + 1) * 0.13 + np.sin(time * (0.037 + channel * 0.014)) * 0.21
        for channel in range(channels)
    ]).astype(np.float32)
    pcm[:, 0] += 0.4
    pcm[:, -1] -= 0.3
    return pcm


def track_cases(exe: Path, folder: Path) -> list:
    """Complete physical tracks; explicit override vs default train context."""
    cases = [
        (1, 7, 9, True, 0.0, 1.0),
        (37, 7, 9, True, 0.6, 1.5),
        (38, 9, 9, False, 0.25, 1.0),
        (343980, 343980, 343980, False, 0.25, 1.0),
        (705601, 343980, 343980, False, 0.25, 1.0),
        (401003, 176400, 343980, True, 0.55, 2.5),
    ]
    results = []
    for index, (frames, segment, training, override, overlap, power) in enumerate(cases):
        pcm = signal(frames)
        model = HostCallback(training, 2, 4)
        expected = apply_model(
            model,
            torch.from_numpy(pcm)[None],
            shifts=0,
            split=True,
            overlap=overlap,
            transition_power=power,
            segment=float(Fraction(segment, 44100)) if override else None,
        )[0].numpy()
        valid = segment if override else training
        native = execute(
            exe,
            folder / f"track-{index}",
            ["track", frames, 2, 4, segment, overlap, power, valid, training],
            pcm,
        ).reshape(expected.shape)
        results.append({"kind": "track", "geometry": cases[index], **compare(native, expected)})
    # Direct full-context preparation exposes padding even where a model crop
    # would discard it: left/interior/right boundaries and asymmetric odd delta.
    pcm = signal(31)
    for index, (offset, length, valid, training) in enumerate([
        (0, 3, 8, 11),
        (9, 7, 10, 13),
        (29, 2, 9, 12),
    ]):
        expected = functional.pad(
            TensorChunk(torch.from_numpy(pcm), offset, length).padded(valid),
            (0, training - valid),
        ).numpy()
        native = execute(
            exe,
            folder / f"prepare-{index}",
            ["prepare", 31, 2, offset, length, valid, training],
            pcm,
        ).reshape(expected.shape)
        results.append({"kind": "prepare", **compare(native, expected)})
    return results


def statistics_cases(exe: Path, folder: Path) -> list:
    """Original reductions and elementwise expressions, including difficult DC."""
    results = []
    ordinary = signal(343980)
    dc = (np.float32(128) + signal(4097) * np.float32(0.001)).astype(np.float32)
    near = np.full((2, 1025), np.float32(0.125))
    near[0, 512] = np.nextafter(np.float32(0.125), np.float32(1))
    for index, pcm in enumerate([ordinary, dc, near, np.zeros((2, 1536), dtype=np.float32)]):
        for mode in ("branch", "cli"):
            tensor = torch.from_numpy(pcm.copy())
            reference = tensor.mean(0) if mode == "cli" else tensor
            mean, std = reference.mean(), reference.std()
            if mode == "cli" and (not bool(torch.isfinite(std)) or float(std) == 0):
                continue  # Undefined original CLI case is covered by Rust refusal.
            normalized = (
                (tensor - mean) / (std + 1e-5) if mode == "branch" else (tensor - mean) / std
            )
            restored = normalized * std + mean
            expected = torch.cat([
                mean.reshape(1),
                std.reshape(1),
                normalized.flatten(),
                restored.flatten(),
            ]).numpy()
            actual = execute(
                exe, folder / f"stats-{index}-{mode}", [mode, pcm.shape[1], pcm.shape[0]], pcm
            )
            supplied = execute(
                exe,
                folder / f"reference-{index}-{mode}",
                [f"{mode}-reference", pcm.shape[1], pcm.shape[0], float(mean), float(std)],
                pcm,
            )
            results.append({
                "kind": mode,
                "signal": index,
                **compare(supplied, expected),
                "independent_reduction_diagnostic": compare(actual, expected),
            })
    return results


def main() -> None:
    """Reuse one frozen executable for the one original-risk numerical set."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    if importlib.metadata.version("demucs") != "4.0.1":
        msg = "unchanged Demucs 4.0.1 required"
        raise ValueError(msg)
    torch.set_num_threads(1)
    torch.set_num_interop_threads(1)
    logging.basicConfig(level=logging.INFO, format="%(message)s")
    results = track_cases(args.executable, args.output) + statistics_cases(
        args.executable, args.output
    )
    report = {
        "scope": "nonneural host callbacks and independent statistics only; no learned/CUDA parity",
        "reduction": "supplied original stats; independent f64 diagnostic retains parity FAIL",
        "independent_reduction_parity": all(
            not r.get("independent_reduction_diagnostic", {}).get("failed", 0) for r in results
        ),
        "executable_sha256": hashlib.sha256(args.executable.read_bytes()).hexdigest(),
        "demucs": importlib.metadata.version("demucs"),
        "torch": torch.__version__,
        "atol": ATOL,
        "rtol": RTOL,
        "cases": results,
        "passed": all(not r["failed"] for r in results),
    }
    (args.output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    logger.info("%s", json.dumps(report))
    if not report["passed"]:
        msg = "original Demucs host arithmetic comparison failed"
        raise AssertionError(msg)


if __name__ == "__main__":
    main()
