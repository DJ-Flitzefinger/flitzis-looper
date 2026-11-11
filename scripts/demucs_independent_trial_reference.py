"""Independent CPU normalization through full original forward and apply paths."""

import argparse
import hashlib
import json
from fractions import Fraction
from pathlib import Path
from unittest.mock import patch

import numpy as np
import torch
from demucs.apply import BagOfModels, apply_model
from demucs_host_trial_reference import compare, execute
from demucs_hybrid_trial_reference import Decoder, Fixture, signal
from demucs_reduction_trial_reference import identity
from torch import nn


class Probe(Fixture):
    """Observe real raw/normalized branch inputs without replacing forward math."""

    def __init__(self, training: int, sources: int, channels: int) -> None:
        super().__init__(training, sources)
        self.audio_channels = channels
        self.register_parameter("placeholder", nn.Parameter(torch.zeros(1)))
        self.raw: torch.Tensor | None = None
        self.normalized: dict[str, torch.Tensor] = {}
        self.encoder[0].register_forward_pre_hook(self.spectral_input)
        self.tencoder[0].register_forward_pre_hook(self.time_input)

    def spectral_input(self, _module: nn.Module, args: tuple) -> None:
        self.normalized["spectral"] = args[0].clone()

    def time_input(self, _module: nn.Module, args: tuple) -> None:
        self.normalized["time"] = args[0].clone()

    def _magnitude(self, z: torch.Tensor) -> torch.Tensor:
        values = super()._magnitude(z)
        self.raw = values.clone()
        return values


def material(batches: int, channels: int, frames: int, kind: str) -> torch.Tensor:
    x = signal(batches, channels, frames, "mixed")
    if kind == "dc":
        return 128 + x * 0.001
    if kind == "tiny":
        return x * 1e-7
    if kind == "zero":
        return x * 0
    return x


def same_tensor(examples: Path, folder: Path, original: torch.Tensor) -> dict:
    shape = original.shape
    actual = execute(
        examples / "demucs_host.exe",
        folder,
        ["cpu-planar-cac", *identity(), shape[0], shape[1], shape[-1], shape[1] // 2],
        original.contiguous().numpy(),
    )
    mean = original.mean((1, 2, 3), keepdim=True)
    std = original.std((1, 2, 3), keepdim=True)
    normalized = (original - mean) / (std + 1e-5)
    expected = np.concatenate([
        torch.stack([mean.flatten(), std.flatten()], 1).numpy().flatten(),
        normalized.contiguous().numpy().flatten(),
        (normalized * std + mean).contiguous().numpy().flatten(),
    ])
    count = shape[0] * 2
    return {
        "statistic_mismatches": int(
            np.count_nonzero(actual[:count].view(np.uint32) != expected[:count].view(np.uint32))
        ),
        **compare(actual[count:], expected[count:]),
    }


def independent(
    examples: Path, folder: Path, geometry: list, pcm: torch.Tensor, steps: int
) -> dict:
    values = execute(
        examples / "demucs_hybrid.exe", folder, ["independent", *geometry], pcm.numpy()
    )
    b, c, sources, physical, training = geometry
    spectral = b * c * 2 * 2048 * steps
    time_end = spectral + b * c * training
    return {
        "spectral": values[:spectral].reshape(b, c * 2, 2048, steps),
        "time": values[spectral:time_end].reshape(b, c, training),
        "statistics": values[time_end : time_end + b * 4].reshape(b, 4),
        "pcm": values[time_end + b * 4 :].reshape(b, sources, c, physical),
    }


def direct(examples: Path, folder: Path, spec: tuple) -> dict:
    batches, channels, frames, training, sources, kind = spec
    pcm = material(batches, channels, frames, kind)
    model = Probe(training, sources, channels).eval()
    with torch.inference_mode():
        reference = model(pcm).numpy()
    assert model.raw is not None
    native = independent(
        examples,
        folder / "independent",
        [batches, channels, sources, frames, training],
        pcm,
        model.raw.shape[-1],
    )
    padded = torch.nn.functional.pad(pcm, (0, training - frames))
    same = same_tensor(examples, folder / "same-tensor", model._magnitude(model._spec(padded)))
    result = {
        "geometry": [batches, channels, sources, frames, training],
        "kind": kind,
        "pcm": compare(native["pcm"], reference),
        "normalized_spectral": compare(native["spectral"], model.normalized["spectral"].numpy()),
        "normalized_time": compare(native["time"], model.normalized["time"].numpy()),
        "raw_fft_difference": compare(
            execute(
                examples / "demucs_spectral.exe",
                folder / "native-fft",
                ["analyze", training, batches * channels],
                padded.numpy(),
            ).reshape(model.raw.shape),
            model.raw.numpy(),
        ),
        "same_tensor": same,
        "independent_time_statistics_mismatches": int(
            np.count_nonzero(
                native["statistics"][:, 2:].copy().view(np.uint32)
                != torch.stack(model.stats["time"], 1).numpy().view(np.uint32)
            )
        ),
        "native_statistics": native["statistics"].tolist(),
    }
    result["passed"] = (
        all(
            result[k]["passed"]
            for k in ("pcm", "normalized_spectral", "normalized_time", "same_tensor")
        )
        and same["statistic_mismatches"] == result["independent_time_statistics_mismatches"] == 0
    )
    if kind == "zero":
        result["exact_zero"] = not np.count_nonzero(native["pcm"])
        result["passed"] &= result["exact_zero"]
    return result


class IndexedDecoder(Decoder):
    """Distinct model outputs before original raw-statistic restoration."""

    def __init__(self, sources: int, index: int) -> None:
        super().__init__(sources)
        self.index = index

    def forward(self, x: torch.Tensor, skip: torch.Tensor, length: int) -> tuple:
        out, _ = super().forward(x, skip, length)
        return out * (1 + self.index * 0.125) + self.index * 0.007, None


class Application:
    """One whole CLI/apply composition with explicit per-model controls."""

    def __init__(self, channels: int, *, cli: bool, split: bool, shifted: bool, kind: str) -> None:
        self.channels, self.cli, self.split, self.shifted, self.kind = (
            channels,
            cli,
            split,
            shifted,
            kind,
        )
        self.frames, self.sources = (43 if split else 31), 2
        self.training = 22091 if shifted else (19 if split else 37)
        self.segment = 22083 if shifted else (13 if split else 37)
        self.models = [
            Probe(self.training + i * 2, self.sources, channels).eval() for i in range(2)
        ]
        self.events: list = []
        for i, model in enumerate(self.models):
            model.decoder = nn.ModuleList([IndexedDecoder(self.sources, i)])
            model.tdecoder = nn.ModuleList([IndexedDecoder(self.sources, i)])
            model.register_forward_pre_hook(
                lambda _m, args, index=i: self.events.append((index, args[0].shape[-1]))
            )
        self.weights = [[0.25, -0.25], [0.75, 1.25]]
        self.draws = [0, 22050, 101, 22049] if shifted else []
        self.original = material(1, channels, self.frames, kind)[0]

    def reference(self) -> np.ndarray:
        pcm = self.original
        mean, std = 0.0, 1.0
        if self.cli:
            ref = pcm.mean(0)
            mean, std = ref.mean(), ref.std()
            pcm = (pcm - mean) / std
        bag = BagOfModels(self.models, weights=self.weights)
        offsets = iter(self.draws)
        with (
            torch.inference_mode(),
            patch("demucs.apply.random.randint", lambda low, high: next(offsets)),
        ):
            expected = apply_model(
                bag,
                pcm[None],
                shifts=2 if self.shifted else 0,
                split=self.split,
                overlap=0.4,
                transition_power=1.5,
                segment=float(Fraction(self.segment, 44100)) if self.split else None,
            )[0]
        return (expected * std + mean).numpy()

    def arguments(self) -> list:
        args = [self.frames, self.channels, self.sources]
        for i in range(len(self.models)):
            valid = self.segment if self.split else self.training + i * 2
            shape = [self.segment, valid, self.training + i * 2, str(self.split).lower(), 0.4, 1.5]
            supplied = self.draws[i * 2 : i * 2 + 2]
            args.append(
                ",".join(map(str, shape))
                + "/"
                + (",".join(map(str, supplied)) or "-")
                + "/"
                + ",".join(map(str, self.weights[i]))
            )
        return (["cli"] if self.cli else []) + args

    def run(self, examples: Path, folder: Path) -> dict:
        expected = self.reference()
        actual = execute(
            examples / "demucs_offline.exe", folder, self.arguments(), self.original.numpy()
        ).reshape(expected.shape)
        native_events = [
            tuple(map(int, line.split()[1:]))
            for line in (folder / "stderr").read_text().splitlines()
        ]
        expected_events = [(i, int(self.models[i].segment * 44100)) for i, _length in self.events]
        return {
            "channels": self.channels,
            "cli": self.cli,
            "split": self.split,
            "shifts": self.shifted,
            "kind": self.kind,
            "callbacks": len(self.events),
            "order_equal": native_events == expected_events,
            **compare(actual, expected),
        }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--examples", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    identity()
    assert hashlib.sha256(Path(apply_model.__code__.co_filename).read_bytes()).hexdigest() == (
        "316278eeb028c66f99befa4201edca7f598166628aeec8e40a08544f00aa21df"
    )
    args.output.mkdir()
    specs = (
        [
            (b, c, n, t, 2, kind)
            for b, c, n, t in [
                (1, 1, 3, 3),
                (2, 1, 1027, 1027),
                (1, 1, 3073, 4097),
                (2, 1, 4097, 4097),
                (2, 2, 3073, 4097),
                (1, 2, 4097, 4097),
            ]
            for kind in ("mixed", "dc", "tiny")
        ]
        + [(1, c, n, 343980, 4, "mixed") for c, n in [(1, 343980), (2, 343041), (2, 343980)]]
        + [(1, 1, 1, 3, 2, "zero"), (2, 2, 29, 29, 2, "zero")]
    )
    direct_results = []
    for i, spec in enumerate(specs):
        folder = args.output / f"forward-{i}"
        folder.mkdir()
        direct_results.append(direct(args.examples, folder, spec))
    applications = [
        Application(c, cli=cli, split=split, shifted=shifts, kind=kind).run(
            args.examples, args.output / f"apply-{i}"
        )
        for i, (c, cli, split, shifts, kind) in enumerate([
            (1, False, False, False, "dc"),
            (2, True, False, False, "dc"),
            (1, True, True, False, "tiny"),
            (2, True, True, False, "mixed"),
            (1, True, True, True, "mixed"),
            (2, False, True, True, "mixed"),
        ])
    ]
    report = {
        "scope": (
            "explicit serial CPU, parameter-free original HTDemucs forward/apply; "
            "no learned/runtime/cutover"
        ),
        "artifacts": {
            n: hashlib.sha256((args.examples / (n + ".exe")).read_bytes()).hexdigest()
            for n in ("demucs_hybrid", "demucs_host", "demucs_offline", "demucs_spectral")
        },
        "atol": 2e-5,
        "rtol": 2e-5,
        "forward": direct_results,
        "apply": applications,
    }
    report["passed"] = all(r["passed"] for r in direct_results) and all(
        r["passed"] and r["order_equal"] for r in applications
    )
    (args.output / "report.json").write_text(json.dumps(report, indent=2))
    assert report["passed"], report


if __name__ == "__main__":
    main()
