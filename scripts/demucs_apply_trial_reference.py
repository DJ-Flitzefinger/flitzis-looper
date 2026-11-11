"""Full original apply_model arithmetic controls; no learned model or weights."""

import argparse
import hashlib
import importlib.metadata
import json
from fractions import Fraction
from itertools import starmap
from pathlib import Path
from typing import TYPE_CHECKING, NamedTuple
from unittest.mock import patch

import torch
from demucs.apply import BagOfModels, apply_model
from demucs_host_trial_reference import ATOL, RTOL, HostCallback, compare, execute, signal

if TYPE_CHECKING:
    import numpy as np


class Callback(HostCallback):
    """Parameter-bearing host callback; no learned module is constructed."""

    def __init__(self, training: int, index: int, events: list) -> None:
        super().__init__(training, 2, 4)
        self.index = index
        self.events = events
        self.register_parameter("placeholder", torch.nn.Parameter(torch.zeros(1)))

    def forward(self, mix: torch.Tensor) -> torch.Tensor:
        """Expose model order and distinct context-sensitive model arithmetic."""
        training = int(self.segment * self.samplerate)
        self.events.append((self.index, mix.shape[-1], training))
        return super().forward(mix) * (1 + self.index * 0.125) + self.index * 0.007


def model(segment: int, training: int, offsets: list, weights: list) -> dict:
    """Declare fixed original HT segment/training geometry and supplied draws."""
    return {"segment": segment, "training": training, "offsets": offsets, "weights": weights}


class Case(NamedTuple):
    """One complete original host invocation."""

    frames: int
    split: bool
    overlap: float
    power: float
    override: bool
    specs: list


def cases() -> list[Case]:
    """Cover odd trims, all shift coordinates, full release/tail and model order."""
    one = [1, 1, 1, 1]
    controls = [
        (17, False, 0.25, 1.0, False, [model(22, 22, [], one)]),
        (17, False, 0.25, 1.0, True, [model(22069, 22073, [0, 22050, 101], one)]),
        (37, True, 0.6, 1.5, True, [model(8193, 8197, [22050, 101, 0], one)]),
        (343980, True, 0.25, 1.0, False, [model(343980, 343980, [0, 22050, 12239], one)]),
        (705601, True, 0.55, 2.5, True, [model(176400, 343980, [22050, 0], one)]),
        (
            401003,
            True,
            0.6,
            1.5,
            False,
            [
                model(343980, 343980, [0, 22050], [0.1, 0, 1.7, -0.25]),
                model(176400, 176400, [12239, 17], [0.2, 3.1, 0.3, 1.25]),
                model(220500, 220500, [22050, 0], [0.7, 0.9, 0.1, 0.125]),
            ],
        ),
        (
            31,
            False,
            0.25,
            1.0,
            True,
            [
                model(22083, 22089, [0, 22050], [0.1, 0.3, 0, 1.1]),
                model(22083, 22091, [1023, 22049], [0.2, 0.7, 2.5, 0.125]),
            ],
        ),
        (
            38,
            True,
            0.25,
            1.0,
            False,
            [model(9, 9, [], [0.1, 0.2, 0.3, 0.4]), model(11, 11, [], one)],
        ),
    ]
    return list(starmap(Case, controls))


def reference(case: Case, pcm: np.ndarray, events: list) -> tuple[np.ndarray, list]:
    """Run original model/shift nesting with lexically supplied original draws."""
    models = [Callback(s["training"], i, events) for i, s in enumerate(case.specs)]
    bag = BagOfModels(models, weights=[s["weights"] for s in case.specs])
    offsets = [o for s in case.specs for o in s["offsets"]]
    draws = iter(offsets)
    consumed = []

    def randint(low: int, high: int) -> int:
        assert (low, high) == (0, 22050)
        offset = next(draws)
        consumed.append(offset)
        return offset

    shifts = len(case.specs[0]["offsets"])
    assert all(len(s["offsets"]) == shifts for s in case.specs)
    segment = float(Fraction(case.specs[0]["segment"], 44100)) if case.override else None
    assert not case.override or all(s["segment"] == case.specs[0]["segment"] for s in case.specs)
    with patch("demucs.apply.random.randint", randint):
        expected = apply_model(
            bag,
            torch.from_numpy(pcm)[None],
            shifts=shifts,
            split=case.split,
            overlap=case.overlap,
            transition_power=case.power,
            segment=segment,
        )[0].numpy()
    assert consumed == offsets
    return expected, consumed


def arguments(case: Case) -> list:
    """Serialize the same per-model geometry and draws for the native bridge."""
    result = [case.frames, 2, 4]
    for spec in case.specs:
        valid = spec["segment"] if case.override else spec["training"]
        geometry = [
            spec["segment"],
            valid,
            spec["training"],
            str(case.split).lower(),
            case.overlap,
            case.power,
        ]
        draws_text = ",".join(map(str, spec["offsets"])) or "-"
        result.append(
            ",".join(map(str, geometry))
            + "/"
            + draws_text
            + "/"
            + ",".join(map(str, spec["weights"]))
        )
    return result


def control(exe: Path, folder: Path, case: Case) -> dict:
    """Compare every physical scalar and exact original model callback order."""
    pcm = signal(case.frames)
    events = []
    expected, consumed = reference(case, pcm, events)
    actual = execute(exe, folder, arguments(case), pcm).reshape(expected.shape)
    native_events = [
        tuple(map(int, line.split()[1:])) for line in (folder / "stderr").read_text().splitlines()
    ]
    assert native_events == events
    return {
        "geometry": case,
        "callbacks": len(events),
        "callback_order_equal": True,
        "draws": consumed,
        **compare(actual, expected),
    }


def main() -> None:
    """Run one frozen executable against the pinned original model-free host."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    assert importlib.metadata.version("demucs") == "4.0.1"
    args.output.mkdir()
    results = [
        control(args.executable, args.output / f"apply-{i}", case) for i, case in enumerate(cases())
    ]
    report = {
        "scope": (
            "supplied-offset shifts, no-split and weighted bags; "
            "no random generator/NN/runtime proof"
        ),
        "demucs": importlib.metadata.version("demucs"),
        "torch": torch.__version__,
        "executable_sha256": hashlib.sha256(args.executable.read_bytes()).hexdigest(),
        "atol": ATOL,
        "rtol": RTOL,
        "cases": results,
        "passed": all(r["passed"] for r in results),
        "independent_reduction_parity": False,
        "statistics_evidence": "unchanged prior highDC FAIL retained; not replayed",
    }
    (args.output / "report.json").write_text(json.dumps(report, indent=2))
    assert report["passed"], report


if __name__ == "__main__":
    main()
