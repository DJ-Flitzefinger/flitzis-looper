"""Compare complete Native PCM to pinned HTDemucs.forward with synthetic decoders."""

import argparse
import hashlib
import importlib.metadata
import importlib.util
import json
import logging
import subprocess
from fractions import Fraction
from pathlib import Path

import numpy as np
import torch
from demucs.htdemucs import HTDemucs
from torch import nn

ATOL = 2e-5
RTOL = 2e-5
RATE = 44100
TORCH_GIT = "7661cd9c6b841b62b7f411aa52ec51f05457263b"
SOURCE_PINS = {
    "htdemucs.py": "3404ed68d35b946f9c54b54201851bb4171dd7861a498945ce53b1df1144b034",
    "hdemucs.py": "5b98a5ee262bf6b872a0db23eeb60d024debc4be1ed237c551284af305343864",
    "remote/htdemucs.yaml": "239c445d0b14454d541ad8bd9bb271c9e536d267e8a4625208744cbb2e7bb66c",
    "spec.py": "8a63153a012703fd5ed2e3d012e91ad624597c7296e9d82f40a26c4249f2bff7",
}
logger = logging.getLogger(__name__)


class Encoder(nn.Module):
    """Identity at the normalized branch input."""

    empty = False

    def forward(self, x: torch.Tensor, inject: torch.Tensor | None = None) -> torch.Tensor:
        assert inject is None
        return x


class Decoder(nn.Module):
    """Produce finite parameter-free signals in the original decoder layouts."""

    empty = False

    def __init__(self, sources: int) -> None:
        super().__init__()
        self.sources = sources
        self.prediction: torch.Tensor | None = None

    def forward(
        self, x: torch.Tensor, skip: torch.Tensor, length: int
    ) -> tuple[torch.Tensor, None]:
        assert x is skip
        assert x.shape[-1] == length
        b = torch.arange(x.shape[0], dtype=torch.float32)[:, None, None]
        s = torch.arange(self.sources, dtype=torch.float32)[None, :, None]
        channels = x.shape[1] // 2 if x.ndim == 4 else x.shape[1]
        c = torch.arange(channels, dtype=torch.float32)[None, None, :]
        label = (1 + b * 16 + s * 3 + c) * 0.0625
        t = torch.arange(length, dtype=torch.float32)
        if x.ndim == 4:
            bins = x.shape[-2]
            marks = torch.zeros((2, bins))
            marks[0, 3], marks[1, 7], marks[0, -1] = 8, -6, 3
            pattern = label[..., None] + (t % 5) * 0.125
            shaped = x.unflatten(1, (channels, 2))
            y = shaped[:, None] * ((s + 1) * 0.25)[..., None, None, None]
            y += marks[None, None, None, ..., None] * pattern[..., None, None, :]
        else:
            y = x[:, None] * ((s + 1) * -0.125)[..., None]
            y += (t % 17) * 0.03125 + label[..., None]
        self.prediction = y
        return y.flatten(1, y.ndim - (3 if x.ndim == 4 else 2)).contiguous(), None


class Fixture(HTDemucs):
    """Use original forward, padding, transforms and restore with synthetic layers."""

    def __init__(self, frames: int, sources: int) -> None:
        # Bypass the original learned-layer constructor.
        nn.Module.__init__(self)
        self.segment = Fraction(frames, RATE)
        self.samplerate = RATE
        self.use_train_segment = True
        self.sources = [str(index) for index in range(sources)]
        self.nfft, self.hop_length = 4096, 1024
        self.cac, self.wiener_iters, self.depth = True, 0, 1
        self.freq_emb = self.crosstransformer = None
        self.encoder, self.tencoder = nn.ModuleList([Encoder()]), nn.ModuleList([Encoder()])
        self.decoder = nn.ModuleList([Decoder(sources)])
        self.tdecoder = nn.ModuleList([Decoder(sources)])
        self.stats: dict[str, tuple[torch.Tensor, torch.Tensor]] = {}
        self.spectral_pcm: torch.Tensor | None = None

    def _spec(self, x: torch.Tensor) -> torch.Tensor:
        self.stats["time"] = (x.mean(dim=(1, 2)), x.std(dim=(1, 2)))
        return super()._spec(x)

    def _magnitude(self, z: torch.Tensor) -> torch.Tensor:
        values = super()._magnitude(z)
        self.stats["spectral"] = (values.mean(dim=(1, 2, 3)), values.std(dim=(1, 2, 3)))
        return values

    def _ispec(self, z: torch.Tensor, length: int, scale: int = 0) -> torch.Tensor:
        self.spectral_pcm = super()._ispec(z, length, scale)
        return self.spectral_pcm


def signal(batches: int, channels: int, frames: int, kind: str) -> torch.Tensor:
    """Mark both ends and distinguish batch and channel ordering."""
    if kind in {"zero", "constant"}:
        return torch.full((batches, channels, frames), 0.0 if kind == "zero" else 0.5)
    t = torch.arange(frames, dtype=torch.float32)[None, None, :]
    b = torch.arange(batches, dtype=torch.float32)[:, None, None]
    c = torch.arange(channels, dtype=torch.float32)[None, :, None]
    pcm = 0.13 * (b + 1) - 0.07 * c + 0.25 * torch.sin(t * (0.03 + 0.01 * c + 0.02 * b))
    pcm[..., 0] += 0.6
    pcm[..., -1] -= 0.53
    return pcm.contiguous()


def original_forward(
    pcm: torch.Tensor, training: int, sources: int, kind: str
) -> tuple[torch.Tensor, torch.Tensor, dict]:
    frames = pcm.shape[-1]
    model = Fixture(training, sources).eval()
    with torch.inference_mode():
        expected = model(pcm)
        time = model.tdecoder[0].prediction
        assert time is not None
        assert model.spectral_pcm is not None
        mean, std = model.stats["time"]
        time_pcm = time * std[:, None, None, None] + mean[:, None, None, None]
        spec_pcm = model.spectral_pcm
        summed = (time_pcm + spec_pcm)[..., :frames]
        assert torch.equal(summed, expected)
        branches = {
            "min_lane_spectral_peak": float(spec_pcm[..., :frames].abs().amax(-1).min()),
            "min_lane_time_peak": float(time_pcm[..., :frames].abs().amax(-1).min()),
            "min_lane_final_window_peak": float(
                expected[..., -min(frames, 64) :].abs().amax(-1).min()
            ),
            "min_lane_spectral_final_window_peak": float(
                spec_pcm[..., max(0, frames - 64) : frames].abs().amax(-1).min()
            ),
            "min_lane_time_final_window_peak": float(
                time_pcm[..., max(0, frames - 64) : frames].abs().amax(-1).min()
            ),
            "discarded_spectral_peak": float(spec_pcm[..., frames:].abs().max())
            if frames < training
            else None,
            "discarded_time_peak": float(time_pcm[..., frames:].abs().max())
            if frames < training
            else None,
        }
        if kind == "mixed":
            for name, peak in branches.items():
                assert peak is None or peak > 0.001, (name, peak)
        else:
            assert torch.count_nonzero(std) == 0
            if kind == "zero":
                assert torch.count_nonzero(model.stats["spectral"][1]) == 0
            else:
                assert torch.all(time_pcm == 0.5)
    stats = torch.stack([*model.stats["spectral"], *model.stats["time"]], dim=1)
    return expected, stats, branches


def execute(
    exe: Path, case_dir: Path, geometry: list[int], stats: torch.Tensor, pcm: torch.Tensor
) -> np.ndarray:
    """Close the regular file readers and writers after the owned process exits."""
    case_dir.mkdir()
    source, output, errors = (case_dir / n for n in ("input.f32", "output.f32", "stderr"))
    with source.open("xb") as writer:
        stats.numpy().astype("<f4", copy=False).tofile(writer)
        pcm.numpy().astype("<f4", copy=False).tofile(writer)
    with source.open("rb") as reader, output.open("xb") as writer, errors.open("xb") as stderr:
        subprocess.run(
            [str(exe), *map(str, geometry)],
            stdin=reader,
            stdout=writer,
            stderr=stderr,
            check=True,
        )
    return np.fromfile(output, dtype="<f4")


def compare(
    exe: Path,
    case_dir: Path,
    pcm: torch.Tensor,
    training: int,
    sources: int,
    kind: str,
) -> dict:
    batches, channels, frames = pcm.shape
    geometry = [batches, channels, sources, frames, training]
    expected, stats, branches = original_forward(pcm, training, sources, kind)
    actual = execute(exe, case_dir, geometry, stats, pcm).reshape(expected.shape)
    reference = expected.numpy()
    diff = np.abs(actual.astype(np.float64) - reference.astype(np.float64))
    threshold = 0.0 if kind == "zero" else ATOL + RTOL * np.abs(reference)
    failed = (~np.isfinite(actual)) | (~np.isfinite(reference)) | (diff > threshold)
    return {
        "geometry": geometry,
        "kind": kind,
        "values": int(actual.size),
        "max_abs": float(diff.max()),
        "tail_max_abs": float(diff[..., -1].max()),
        "exact_zero_required": kind == "zero",
        "failed": int(failed.sum()),
        "passed": not bool(failed.any()),
        "branch_controls": branches,
        "original_stats": stats.tolist(),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if importlib.metadata.version("demucs") != "4.0.1":
        msg = "original Demucs 4.0.1 required"
        raise ValueError(msg)
    if torch.__version__ != "2.12.0+cu130" or torch.version.git_version != TORCH_GIT:
        msg = "pinned original Torch 2.12.0+cu130 required"
        raise ValueError(msg)
    module = importlib.util.find_spec("demucs.htdemucs")
    assert module is not None
    assert module.origin is not None
    source = Path(module.origin).parent
    for name, pin in SOURCE_PINS.items():
        if hashlib.sha256((source / name).read_bytes()).hexdigest() != pin:
            msg = f"original source pin mismatch for {name}"
            raise ValueError(msg)
    args.output.mkdir()
    torch.set_default_device("cpu")
    torch.set_num_threads(1)
    torch.set_num_interop_threads(1)
    cases = [
        (2, 2, 3, 3073, 4097, "mixed"),
        (1, 2, 4, 343041, 343980, "mixed"),
        (1, 2, 4, 343980, 343980, "mixed"),
        (1, 2, 3, 29, 29, "constant"),
        (1, 1, 2, 1, 3, "zero"),
    ]
    results = [
        compare(
            args.executable,
            args.output / f"case-{i}",
            signal(b, c, frames, kind),
            training,
            s,
            kind,
        )
        for i, (b, c, s, frames, training, kind) in enumerate(cases)
    ]
    report = {
        "oracle": "Demucs4.0.1 original HTDemucs.forward, pinned Torch CPU f32, synthetic layers",
        "torch": torch.__version__,
        "torch_git": torch.version.git_version,
        "sources_sha256": SOURCE_PINS,
        "executable_sha256": hashlib.sha256(args.executable.read_bytes()).hexdigest(),
        "atol": ATOL,
        "rtol": RTOL,
        "cases": results,
        "passed": all(r["passed"] for r in results),
    }
    (args.output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    logging.basicConfig(level=logging.INFO, format="%(message)s")
    logger.info("%s", json.dumps(report))
    if not report["passed"]:
        msg = "original HTDemucs hybrid PCM comparison failed"
        raise AssertionError(msg)


if __name__ == "__main__":
    main()
