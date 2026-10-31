"""Generate owned synthetic complete-source references using the original worker.

Run only in the unchanged pinned BeatThis environment. Outputs are reconstructible
developer scratch, never application provenance or musical-quality acceptance.
"""

import argparse
import hashlib
import importlib.util
import json
import sys
from pathlib import Path

import numpy as np
import torch
from beat_this.inference import split_predict_aggregate
from beat_this.model.postprocessor import Postprocessor

CHECKPOINT_SHA256 = "8c328b45f59d8dd3dff219253ff6a8d6482be57d0133a29140e2febbf8eb8331"
CASES = [
    ("short14", 22050, 513),
    ("short65", 22050, 52 * 441 + 17),
    ("rate44100", 44100, 10001),
    ("rate48000", 48000, 48013),
    ("rate96000", 96000, 96017),
    ("track", 48000, 32 * 48000 + 101),
]


def synthetic_signal(rate: int, frames: int) -> np.ndarray:
    """Create exact transient, sustained-content and nonaligned-tail fixture bytes."""
    t = np.arange(frames, dtype=np.float64) / rate
    phase = np.remainder(t, 0.5)
    signal = (
        0.15 * np.sin(2 * np.pi * 220 * t)
        + 0.04 * np.sin(2 * np.pi * 431 * t)
        + 0.6 * np.exp(-phase * 160) * np.sin(2 * np.pi * 70 * t)
    ).astype("<f4")
    signal[0] = 0.9
    signal[-1] = -0.7
    return signal


def check_new_model_outputs(output: Path) -> None:
    """Reject existing or partial reference evidence before any model load/run."""
    for name in [
        "model-reference.json",
        "short14.logits.f32",
        "short65.logits.f32",
        "track.logits.f32",
    ]:
        if (output / name).exists():
            msg = "model reference output already exists"
            raise FileExistsError(msg)


def generate(worker_path: Path, output: Path, checkpoint: Path | None) -> None:
    """Keep all frames and save separate frontend and optional full-model evidence."""
    if checkpoint is not None:
        check_new_model_outputs(output)
    spec = importlib.util.spec_from_file_location("beat_trial_original", worker_path)
    if spec is None or spec.loader is None:
        msg = "original worker unavailable"
        raise ValueError(msg)
    worker = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(worker)
    worker.configure_runtime()
    model = worker.load_local_model(checkpoint, CHECKPOINT_SHA256) if checkpoint else None
    if model is None:
        output.mkdir(parents=False, exist_ok=False)
    records = []
    for name, rate, frames in CASES:
        # Sharp transients, sustained content and non-aligned tail; no private audio.
        signal = synthetic_signal(rate, frames)
        prepared = worker.prepare_signal(signal, rate)
        with torch.inference_mode():
            spect = worker.signal_to_spectrogram(signal, rate)
            record = {
                "name": name,
                "rate": rate,
                "frames": frames,
                "resampled_frames": len(prepared),
                "spectrogram_frames": len(spect),
            }
            if model is not None:
                if name not in {"short14", "short65", "track"}:
                    continue
                logits = split_predict_aggregate(spect, 1500, 6, "keep_first", model)
                beats, downbeats = Postprocessor(type="minimal", fps=50)(
                    logits["beat"], logits["downbeat"]
                )
                raw = b"".join(
                    logits[key].numpy().astype("<f4").tobytes() for key in ["beat", "downbeat"]
                )
                with (output / f"{name}.logits.f32").open("xb") as stream:
                    stream.write(raw)
                duration = frames / rate
                record.update(
                    beat_seconds=beats[(beats >= 0) & (beats < duration)].tolist(),
                    downbeat_seconds=downbeats[(downbeats >= 0) & (downbeats < duration)].tolist(),
                    logits_sha256=hashlib.sha256(raw).hexdigest(),
                )
            else:
                for suffix, values in [
                    ("pcm", signal),
                    ("resampled", prepared),
                    ("spect", spect.numpy()),
                ]:
                    raw = values.astype("<f4").tobytes()
                    (output / f"{name}.{suffix}.f32").write_bytes(raw)
                    record[f"{suffix}_sha256"] = hashlib.sha256(raw).hexdigest()
        records.append(record)
    dest = output / ("model-reference.json" if model is not None else "frontend-reference.json")
    with dest.open("x", encoding="utf-8") as stream:
        json.dump(
            {
                "trial_only": True,
                "worker_sha256": hashlib.sha256(worker_path.read_bytes()).hexdigest(),
                "checkpoint_sha256": CHECKPOINT_SHA256 if model is not None else None,
                "cases": records,
            },
            stream,
            indent=2,
        )
    sys.stdout.write(dest.read_text(encoding="utf-8") + "\n")


def main() -> None:
    """Generate frontend-only references first; model comparisons are explicit."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--checkpoint", type=Path)
    args = parser.parse_args()
    generate(args.worker, args.output, args.checkpoint)


if __name__ == "__main__":
    main()
