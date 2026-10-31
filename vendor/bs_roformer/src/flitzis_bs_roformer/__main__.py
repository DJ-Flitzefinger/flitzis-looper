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
from flitzis_looper.bs_input_provenance import (
    begin_input_attempt,
    finish_input_attempt,
    input_completion_line,
)
from flitzis_looper.inference_provider import (
    CUDA_FAILURE_EXIT,
    CudaProviderError,
    emit_cuda_failure,
)
from flitzis_looper.offline_runtime import configure_child
from flitzis_looper.stem_cancellation import (
    CANCEL_EXIT_CODE,
    StemCancellation,
    StemInferenceCancelledError,
    emit_cancellation_ack,
)
from flitzis_looper.stem_execution import completed_execution_line


def main() -> None:
    """Validate explicit artifact paths and write the release's four sources."""
    threads = configure_child(
        torch.set_num_threads,
        torch.set_num_interop_threads,
        torch.get_num_threads,
        torch.get_num_interop_threads,
    )
    parser = argparse.ArgumentParser(description="BS-RoFormer MUSDB18HQ offline separator")
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--device", choices=("cpu", "cuda"), required=True)
    args = parser.parse_args()
    cancellation = StemCancellation.from_environment()
    cancellation.checkpoint("before_model_stage")
    plan = ChunkPlan()
    plan.validate()
    args.output.mkdir(parents=True, exist_ok=True)
    if cancellation.marker is None:
        model = load_model(args.checkpoint, args.config, args.device)
    else:
        model = load_model(
            args.checkpoint, args.config, args.device, cancellation=cancellation.checkpoint
        )
    _emit({"stage": "model_ready", "device": args.device, "torch": torch.__version__})
    begin_input_attempt(args.output)
    try:
        with TemporaryDirectory(prefix=".decode-", dir=args.output) as temporary:
            decoded = Path(temporary) / "source.f32"
            frames = decode_source(args.source, decoded, checkpoint=cancellation.checkpoint)
            separate_pcm(
                model,
                decoded,
                args.output,
                frames,
                args.device,
                plan,
                checkpoint=cancellation.checkpoint,
            )
    finally:
        finish_input_attempt()
    cancellation.checkpoint("before_completion_receipt")
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
    sys.stdout.write(
        completed_execution_line(
            "bs-roformer",
            "BS-RoFormer MUSDB18HQ",
            args.device,
            threads=threads,
            cuda_peak_bytes=torch.cuda.max_memory_reserved() if args.device == "cuda" else None,
        )
        + "\n"
    )
    sys.stdout.write(input_completion_line() + "\n")
    sys.stdout.flush()


def _emit(status: dict[str, str | int | bool]) -> None:
    sys.stdout.write(json.dumps(status) + "\n")
    sys.stdout.flush()


if __name__ == "__main__":
    try:
        main()
    except StemInferenceCancelledError as error:
        emit_cancellation_ack(error)
        sys.exit(CANCEL_EXIT_CODE)
    except CudaProviderError as error:
        emit_cuda_failure(error)
        sys.exit(CUDA_FAILURE_EXIT)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        sys.stderr.write(str(error) + "\n")
        sys.stderr.flush()
        sys.exit(1)
