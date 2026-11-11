"""Full original Demucs/Julius f32 kernels and conversion outputs, without models.

One frozen native artifact; complete physical outputs, including every tail.
The original Torch operators are unchanged. This is numerical compatibility for
the tested source/device domain, not bitwise or universal dispatch equivalence.
Regular file readers/writers close after synchronous child completion; the outer
owned supervisor separately establishes actual Job/tree END.
"""

import argparse
import hashlib
import importlib.metadata
import json
import logging
import math
import subprocess
from pathlib import Path
from typing import NamedTuple

import julius
import numpy as np
import torch
from demucs import audio as demucs_audio

ATOL = 2e-5
RTOL = 2e-5
logger = logging.getLogger(__name__)

SOURCE_PINS = {
    "demucs/audio.py": "838b072696ba982b4cadbad5e6a00d5a03ef6763692eed136e0384c8cc5df8a8",
    "demucs/separate.py": "3ca74c33421850682ceac3582305f4d46e2c296de96f8db69ddb2cf0568d9f79",
    "julius/resample.py": "94a66e6d7c2527510d3067f31daa5ac98331cad9c0cf2423d8cbb9c5912a26db",
    "julius/core.py": "4f0ce5b63a3a1e7daf5721740f65e15e63ac31508d3e669d5e16d828b96ca9d4",
    "julius/utils.py": "b65ddefe9dc5c291ccebf312db3f25fc29439c29a4178d40a8bd702ba8d7b669",
}


class Case(NamedTuple):
    """Original conversion geometry and signal input."""

    frames: int
    batches: int
    source: int
    target: int
    old: int
    new: int
    zeros: int
    rolloff: float
    full: bool
    length: int | None
    kind: str


def compare(actual: np.ndarray, expected: np.ndarray) -> dict:
    """Compare every finite value at fixed original arithmetic criteria."""
    if actual.shape != expected.shape:
        return {"actual_shape": actual.shape, "expected_shape": expected.shape, "passed": False}
    delta = np.abs(actual.astype(np.float64) - expected.astype(np.float64))
    failed = (
        (~np.isfinite(actual)) | (~np.isfinite(expected)) | (delta > ATOL + RTOL * np.abs(expected))
    )
    return {
        "values": int(actual.size),
        "max_abs": float(delta.max(initial=0)),
        "failed": int(failed.sum()),
        "passed": not bool(failed.any()),
    }


def execute(exe: Path, folder: Path, arguments: list, pcm: np.ndarray) -> tuple[int, np.ndarray]:
    """Use regular files; no pipe reader threads or premature publication."""
    folder.mkdir()
    source, output, errors = (folder / n for n in ("input.f32", "output.f32", "stderr"))
    pcm.astype("<f4").tofile(source)
    with source.open("rb") as reader, output.open("xb") as writer, errors.open("xb") as stderr:
        result = subprocess.run(
            [str(exe), *map(str, arguments)],
            stdin=reader,
            stdout=writer,
            stderr=stderr,
            check=False,
        )
    return result.returncode, np.fromfile(output, dtype="<f4")


def signal(frames: int, batches: int, channels: int, kind: str) -> np.ndarray:
    """Distinct channels/batches; true endpoints and multiple spectral components."""
    time = np.arange(frames, dtype=np.float32)
    pcm = (
        np
        .stack([
            np.sin(time * np.float32(0.071 + lane * 0.012)) * np.float32(0.3)
            + np.cos(time * np.float32(1.17)) * np.float32(0.07)
            + np.float32(0.013 * (lane + 1))
            for lane in range(batches * channels)
        ])
        .reshape(batches, channels, frames)
        .astype(np.float32)
    )
    if kind == "constant":
        pcm.fill(0.25)
    elif kind == "alternating":
        pcm *= np.where(np.arange(frames) % 2, np.float32(-1), np.float32(1))
    else:
        pcm[..., 0] += np.float32(0.4)
        pcm[..., -1] -= np.float32(0.3)
    return pcm


def check_case(exe: Path, folder: Path, case: Case) -> dict:
    """Run the original conversion and compare the complete native output."""
    pcm = signal(case.frames, case.batches, case.source, case.kind)
    tensor = torch.from_numpy(pcm)
    original = julius.ResampleFrac(case.old, case.new, zeros=case.zeros, rolloff=case.rolloff)
    if case.zeros == 24 and case.rolloff == 0.945 and not case.full and case.length is None:
        expected = demucs_audio.convert_audio(tensor, case.old, case.new, case.target)
    else:
        expected = original(
            demucs_audio.convert_audio_channels(tensor, case.target),
            output_length=case.length,
            full=case.full,
        )
    kernel = (
        original.kernel.flatten().numpy() if case.old != case.new else np.empty(0, dtype=np.float32)
    )
    arguments = [*case[:8], str(case.full).lower(), "none" if case.length is None else case.length]
    code, actual = execute(exe, folder, arguments, pcm)
    gcd = math.gcd(case.old, case.new)
    return {
        "geometry": case,
        "reduced": [case.old // gcd, case.new // gcd],
        "native_exit": code,
        "output_frames": expected.shape[-1],
        "kernel": compare(actual[: kernel.size], kernel),
        "pcm": compare(actual[kernel.size :], expected.numpy().flatten()),
    }


def controls(exe: Path, folder: Path) -> list:
    """Related channel/phase/length/tail risks, one proportional control set."""
    # frames,batches,source,target,old,new,zeros,rolloff,full,length,signal
    cases = [
        (17, 2, 2, 2, 44100, 44100, 24, 0.945, True, 999, "mixed"),
        (19, 2, 6, 1, 44100, 44100, 24, 0.945, False, None, "mixed"),
        (7, 2, 1, 4, 44100, 44100, 24, 0.945, False, None, "mixed"),
        (31, 2, 6, 2, 44100, 44100, 24, 0.945, False, None, "mixed"),
        (1, 1, 2, 2, 48000, 44100, 24, 0.945, True, None, "constant"),
        (1, 1, 1, 2, 48000, 16000, 24, 0.945, False, None, "mixed"),
        (343981, 1, 2, 2, 48000, 44100, 24, 0.945, False, None, "mixed"),
        (200180, 1, 2, 2, 44100, 48000, 24, 0.945, False, None, "mixed"),
        (37, 2, 6, 1, 16000, 44100, 24, 0.945, True, None, "mixed"),
        (705601, 1, 1, 2, 44100, 16000, 24, 0.945, False, None, "alternating"),
        (76801, 1, 2, 2, 768000, 44100, 24, 0.945, True, None, "mixed"),
        (4411, 1, 1, 2, 44100, 768000, 24, 0.945, False, None, "mixed"),
        (101, 1, 2, 2, 7, 11, 8, 0.85, False, 0, "mixed"),
        (101, 1, 2, 2, 7, 11, 8, 0.85, False, 159, "constant"),
        (37, 2, 3, 1, 96000, 44100, 24, 0.945, False, 13, "mixed"),
    ]
    return [
        check_case(exe, folder / f"case-{index}", Case(*case)) for index, case in enumerate(cases)
    ]


def original_refuses(pcm: np.ndarray, target: int, length: int | None, *, full: bool) -> bool:
    """Ask the unchanged original to reject the same malformed geometry."""
    try:
        original = julius.ResampleFrac(48000, 44100)
        original(
            demucs_audio.convert_audio_channels(torch.from_numpy(pcm), target),
            output_length=length,
            full=full,
        )
    except ValueError:
        return True
    return False


def refusals(exe: Path, folder: Path) -> list:
    """Original invalid channel/length flags and native malformed/nonfinite domain."""
    pcm = signal(17, 1, 2, "mixed")
    base = [17, 1, 2, 2, 48000, 44100, 24, 0.945, "false", "none"]
    cases = [(3, False, None), (2, False, 999), (2, False, -1), (2, True, 1)]
    results = []
    for index, (target, full, length) in enumerate(cases):
        args = [
            *base[:3],
            target,
            *base[4:8],
            str(full).lower(),
            "none" if length is None else length,
        ]
        code, actual = execute(exe, folder / f"refused-{index}", args, pcm)
        original = original_refuses(pcm, target, length, full=full)
        results.append({
            "kind": [target, length, full],
            "original_refused": original,
            "passed": original and code != 0 and actual.size == 0,
        })
    pcm[0, 1, -1] = np.nan
    code, actual = execute(exe, folder / "nonfinite", base, pcm)
    results.append({"kind": "nonfinite", "passed": code != 0 and actual.size == 0})
    return results


def source_hashes() -> dict:
    """Verify and record the original installed modules that define the oracle."""
    roots = {"demucs": Path(demucs_audio.__file__).parent, "julius": Path(julius.__file__).parent}
    hashes = {}
    for relative, expected in SOURCE_PINS.items():
        package, name = relative.split("/")
        value = hashlib.sha256((roots[package] / name).read_bytes()).hexdigest()
        if value != expected:
            msg = f"original source pin mismatch: {relative} {value}"
            raise ValueError(msg)
        hashes[relative] = value
    return hashes


def main() -> None:
    """Compare actual final artifact against pinned originals, with all results retained."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    if (
        importlib.metadata.version("demucs") != "4.0.1"
        or importlib.metadata.version("julius") != "0.2.7"
    ):
        msg = "unchanged Demucs4.0.1/Julius0.2.7 required"
        raise ValueError(msg)
    torch.set_num_threads(1)
    torch.set_num_interop_threads(1)
    logging.basicConfig(level=logging.INFO, format="%(message)s")
    pins = source_hashes()
    cases = controls(args.executable, args.output)
    errors = refusals(args.executable, args.output)
    report = {
        "scope": "independent nonneural finite f32 Demucs channel conversion/Julius resampling",
        "torch": torch.__version__,
        "demucs": "4.0.1",
        "julius": "0.2.7",
        "sources_sha256": pins,
        "torch_cpu_capability": torch.backends.cpu.get_cpu_capability(),
        "torch_mkldnn_enabled": torch.backends.mkldnn.enabled,
        "executable_sha256": hashlib.sha256(args.executable.read_bytes()).hexdigest(),
        "atol": ATOL,
        "rtol": RTOL,
        "cases": cases,
        "refusals": errors,
        "passed": all(
            r["native_exit"] == 0 and r["kernel"]["passed"] and r["pcm"]["passed"] for r in cases
        )
        and all(r["passed"] for r in errors),
    }
    (args.output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    logger.info("%s", json.dumps(report))
    if not report["passed"]:
        msg = "original Demucs/Julius audio conversion comparison failed"
        raise AssertionError(msg)


if __name__ == "__main__":
    main()
