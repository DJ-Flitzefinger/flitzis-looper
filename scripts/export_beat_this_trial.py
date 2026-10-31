"""Export one real fixed BeatThis chunk for the opt-in Rust inference experiment.

Run in the existing BeatThis 3.12 environment with ONNX tooling on a separate
PYTHONPATH. This does not install into that environment or change App dispatch.
All inputs/outputs are developer-owned scratch; no result is a product receipt.
"""

import argparse
import hashlib
import importlib.util
import json
import os
import sys
from importlib.metadata import version
from pathlib import Path

import numpy as np
import onnx
import torch

CHECKPOINT_SHA256 = "8c328b45f59d8dd3dff219253ff6a8d6482be57d0133a29140e2febbf8eb8331"
CHECKPOINT_BYTES = 81_058_141
FRAMES = 1500
BINS = 128
MAX_GRAPH_BYTES = 128 * 1024 * 1024


class LogitPair(torch.nn.Module):
    """Retain the original unchanged network and expose both ordered outputs."""

    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, spectrogram):
        output = self.model(spectrogram)
        return output["beat"], output["downbeat"]


def read_spectrogram(path: Path) -> np.ndarray:
    """Read exact finite float32 LE frames without accepting a partial chunk."""
    with path.open("rb") as stream:
        raw = stream.read(FRAMES * BINS * 4 + 1)
    if len(raw) != FRAMES * BINS * 4:
        msg = "trial requires exactly 1500 by 128 float32 samples"
        raise ValueError(msg)
    array = np.frombuffer(raw, dtype="<f4").copy().reshape(1, FRAMES, BINS)
    if not np.isfinite(array).all():
        msg = "nonfinite trial spectrogram"
        raise ValueError(msg)
    return array


def export(worker_path: Path, checkpoint: Path, spectrogram: Path, output: Path) -> None:
    """Verify with the original loader, export inline weights and save raw reference."""
    if torch.__version__.split("+")[0] != "2.8.0" or version("beat-this") != "1.1.0":
        msg = "trial requires the unchanged BeatThis 1.1.0 / Torch 2.8.0 closure"
        raise ValueError(msg)
    if checkpoint.stat().st_size != CHECKPOINT_BYTES:
        msg = "unexpected final0 checkpoint size"
        raise ValueError(msg)
    source = worker_path.read_bytes()
    lock = worker_path.with_name("uv.lock").read_bytes()
    spec = importlib.util.spec_from_file_location("beat_this_trial_original", worker_path)
    if spec is None or spec.loader is None:
        msg = "cannot load original BeatThis worker"
        raise ValueError(msg)
    worker = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(worker)
    worker.configure_runtime()
    # The original loader hashes and loads the SAME retained checkpoint handle,
    # reconstructs its hyperparameters and loads its complete state_dict strictly.
    wrapper = LogitPair(
        worker.load_local_model(checkpoint.resolve(strict=True), CHECKPOINT_SHA256)
    ).eval()
    array = read_spectrogram(spectrogram)
    tensor = torch.from_numpy(array)
    output.mkdir(parents=False, exist_ok=False)
    with torch.inference_mode():
        # Export first, without a warm-up that mutates shared rotary caches.
        torch.onnx.export(
            wrapper,
            (tensor,),
            output / "beat-this-1500.onnx",
            dynamo=False,
            opset_version=18,
            input_names=["spectrogram"],
            output_names=["beat", "downbeat"],
            external_data=False,
        )
        reference = wrapper(tensor)
    with (output / "beat-this-1500.onnx").open("rb") as stream:
        graph_bytes = stream.read(MAX_GRAPH_BYTES + 1)
    if not 0 < len(graph_bytes) <= MAX_GRAPH_BYTES:
        msg = "exported graph exceeds trial bound"
        raise ValueError(msg)
    graph = onnx.load_model_from_string(graph_bytes)
    onnx.checker.check_model(graph, full_check=True)
    if any(weight.data_location == onnx.TensorProto.EXTERNAL for weight in graph.graph.initializer):
        msg = "external trial weights are unsupported"
        raise ValueError(msg)
    domains = sorted({node.domain for node in graph.graph.node})
    if set(domains) - {"", "ai.onnx"}:
        msg = "trial graph contains nonstandard operators"
        raise ValueError(msg)
    for data in reference:
        if tuple(data.shape) != (1, FRAMES) or not torch.isfinite(data).all():
            msg = "invalid original BeatThis reference logits"
            raise ValueError(msg)
    raw = b"".join(data.cpu().numpy().astype("<f4").tobytes() for data in reference)
    (output / "reference.f32").write_bytes(raw)
    if worker_path.read_bytes() != source or worker_path.with_name("uv.lock").read_bytes() != lock:
        msg = "worker source changed during export"
        raise ValueError(msg)
    receipt = {
        "schema_version": 1,
        "trial_only": True,
        "checkpoint_sha256": CHECKPOINT_SHA256,
        "checkpoint_bytes": CHECKPOINT_BYTES,
        "worker_sha256": hashlib.sha256(source).hexdigest(),
        "lock_sha256": hashlib.sha256(lock).hexdigest(),
        "torch_version": str(torch.__version__),
        "onnx_version": version("onnx"),
        "frames": FRAMES,
        "bins": BINS,
        "graph_sha256": hashlib.sha256(graph_bytes).hexdigest(),
        "graph_bytes": len(graph_bytes),
        "spectrogram_sha256": hashlib.sha256(array.astype("<f4").tobytes()).hexdigest(),
        "reference_sha256": hashlib.sha256(raw).hexdigest(),
        "operator_domains": domains,
        "operators": sorted({node.op_type for node in graph.graph.node}),
        "initializers": len(graph.graph.initializer),
        "nodes": len(graph.graph.node),
        "pid": os.getpid(),
    }
    # No completion receipt exists if any earlier export/check/reference step fails.
    (output / "trial.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    sys.stdout.write(json.dumps(receipt) + "\n")


def main() -> None:
    """Create a new diagnostic directory, leaving failures available for inspection."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path, required=True)
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--spectrogram", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    export(args.worker, args.checkpoint, args.spectrogram, args.output)


if __name__ == "__main__":
    main()
