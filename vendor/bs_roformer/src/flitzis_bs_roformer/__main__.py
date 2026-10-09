"""Run the pinned offline separator in a disposable background subprocess."""

import argparse
import json
import subprocess
import sys
from pathlib import Path
from tempfile import TemporaryDirectory

import torch

from flitzis_bs_roformer.audio import decode_source
from flitzis_bs_roformer.inference import ChunkPlan, separate_pcm
from flitzis_bs_roformer.model import load_model


def main() -> None:
    """Validate explicit artifact paths and write the release's four sources."""
    parser = argparse.ArgumentParser(description="BS-RoFormer MUSDB18HQ offline separator")
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--device", choices=("cpu", "cuda"), required=True)
    args = parser.parse_args()
    plan = ChunkPlan()
    plan.validate()
    args.output.mkdir(parents=True, exist_ok=True)
    model = load_model(args.checkpoint, args.config, args.device)
    _emit({"stage": "model_ready", "device": args.device, "torch": torch.__version__})
    with TemporaryDirectory(prefix=".decode-", dir=args.output) as temporary:
        decoded = Path(temporary) / "source.f32"
        frames = decode_source(args.source, decoded)
        separate_pcm(model, decoded, args.output, frames, args.device, plan)
    _emit({
        "stage": "complete",
        "device": args.device,
        "frames": frames,
        "sample_rate": 44_100,
        "channels": 2,
        "chunk_size": plan.chunk_size,
        "batch_size": plan.batch_size,
        "overlap": plan.overlap,
        "amp": args.device == "cuda",
        "normalize": False,
        "transient_pcm_bytes_bound": plan.transient_pcm_bytes,
    })


def _emit(status: dict[str, str | int | bool]) -> None:
    sys.stdout.write(json.dumps(status) + "\n")
    sys.stdout.flush()


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        sys.stderr.write(str(error) + "\n")
        sys.stderr.flush()
        sys.exit(1)
