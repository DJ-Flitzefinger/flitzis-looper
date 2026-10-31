"""Export a real BeatThis graph for the opt-in Rust inference experiment.

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


def prepare_dynamic_export(wrapper, tensor, *, dynamic: bool) -> list[str]:
    """Disable only original rotary caching and prove identical original logits."""
    if not dynamic:
        return []
    original = wrapper(tensor)
    cache_modules = []
    for module in wrapper.modules():
        if (
            type(module).__module__ == "rotary_embedding_torch.rotary_embedding_torch"
            and type(module).__name__ == "RotaryEmbedding"
        ):
            if module.learned_freq or module.use_xpos or module.freqs_for != "lang":
                msg = "unexpected rotary configuration"
                raise ValueError(msg)
            module.cache_if_possible = False
            cache_modules.append(type(module).__name__)
    if len(cache_modules) != 1:
        msg = "expected one shared original rotary module"
        raise ValueError(msg)
    uncached = wrapper(tensor)
    for cached, fresh in zip(original, uncached, strict=True):
        if not torch.equal(cached, fresh):
            msg = "disabling the export-only rotary cache changed reference logits"
            raise ValueError(msg)
    return cache_modules


def inspect_graph(path: Path) -> tuple[bytes, dict]:
    """Check the complete bounded inline graph and detach its operator metadata."""
    with path.open("rb") as stream:
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
    return graph_bytes, {
        "operator_domains": domains,
        "operators": sorted({node.op_type for node in graph.graph.node}),
        "initializers": len(graph.graph.initializer),
        "nodes": len(graph.graph.node),
    }


def export(
    worker_path: Path, checkpoint: Path, spectrogram: Path, output: Path, *, dynamic: bool = False
) -> None:
    """Verify with the original loader, export inline weights and save raw reference."""
    if output.exists():
        msg = "trial output already exists"
        raise FileExistsError(msg)
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
    tensor = torch.from_numpy(read_spectrogram(spectrogram))
    output.mkdir(parents=False, exist_ok=False)
    graph_name, axes = {
        False: ("beat-this-1500.onnx", None),
        True: (
            "beat-this-dynamic.onnx",
            {"spectrogram": {1: "time"}, "beat": {1: "time"}, "downbeat": {1: "time"}},
        ),
    }[dynamic]
    with torch.inference_mode():
        cache_modules = prepare_dynamic_export(wrapper, tensor, dynamic=dynamic)
        # Export first, without a warm-up that mutates shared rotary caches.
        torch.onnx.export(
            wrapper,
            (tensor,),
            output / graph_name,
            dynamo=False,
            opset_version=18,
            input_names=["spectrogram"],
            output_names=["beat", "downbeat"],
            external_data=False,
            dynamic_axes=axes,
        )
        reference = wrapper(tensor)
    graph_bytes, graph_info = inspect_graph(output / graph_name)
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
        "dynamic_time": dynamic,
        "export_only_disabled_caches": cache_modules,
        "bins": BINS,
        "graph_sha256": hashlib.sha256(graph_bytes).hexdigest(),
        "graph_bytes": len(graph_bytes),
        "spectrogram_sha256": hashlib.sha256(tensor.numpy().astype("<f4").tobytes()).hexdigest(),
        "reference_sha256": hashlib.sha256(raw).hexdigest(),
        **graph_info,
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
    parser.add_argument("--dynamic", action="store_true")
    args = parser.parse_args()
    export(args.worker, args.checkpoint, args.spectrogram, args.output, dynamic=args.dynamic)


if __name__ == "__main__":
    main()
