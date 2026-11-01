"""Export and compare the exact pinned BS release in an admitted CPU-only child.

The fixed binary is a trial asset, never productive model provenance. This tool
uses the original strict same-handle configuration/checkpoint loader, all release
weights and original neural modules. No ONNX/BeatThis tooling closure is mixed in.
Run only via the existing offline owned runner with fresh complete admission.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
from typing import TYPE_CHECKING, cast

import torch
from flitzis_bs_roformer.model import CHECKPOINT_SHA256, CONFIG_SHA256, load_model
from torch import Tensor, nn

from scripts.bs_learned_weights import schema

if TYPE_CHECKING:
    from collections.abc import Callable


MASK_ATOL = 3e-4
MASK_RTOL = 3e-4
PCM_ATOL = 3e-5
PCM_RTOL = 3e-4


def write_tensor(path: Path, tensor: Tensor) -> None:
    """Write exclusive finite CPU float32 bytes without an inferred success receipt."""
    value = tensor.detach().to(device="cpu", dtype=torch.float32).contiguous()
    if not bool(torch.isfinite(value).all()):
        msg = "Nonfinite original learned BS fixture"
        raise ValueError(msg)
    with path.open("xb") as handle:
        handle.write(value.numpy().astype("<f4", copy=False).tobytes())


def export_weights(model: nn.Module, output: Path) -> None:
    """Write every effective tensor after strict original loading, plus its digest."""
    state = model.state_dict()
    expected = dict(schema())
    if set(state) != set(expected):
        msg = f"Unexpected complete BS state schema: {set(state) ^ set(expected)}"
        raise ValueError(msg)
    for name, shape in expected.items():
        value = state[name]
        if value.dtype != torch.float32 or tuple(value.shape) != shape:
            msg = f"Unexpected BS tensor shape/dtype: {name}"
            raise ValueError(msg)
        if not bool(torch.isfinite(value).all()):
            msg = f"Nonfinite BS weight: {name}"
            raise ValueError(msg)
    # state_dict resolves original shared rotary module aliases AFTER strict load,
    # including last-key load semantics. Exporting the raw checkpoint would not.
    weights = output / "weights.bsnn"
    with weights.open("xb") as handle:
        handle.write(b"FLBSNN01")
        for name, _shape in schema():
            handle.write(state[name].contiguous().numpy().astype("<f4", copy=False).tobytes())
    with weights.open("rb") as handle:
        digest = hashlib.file_digest(handle, "sha256").hexdigest()
    metadata = {
        "scope": "original learned BS CPU arithmetic; trial assets only",
        "checkpoint_sha256": CHECKPOINT_SHA256,
        "config_sha256": CONFIG_SHA256,
        "weights_sha256": digest,
        "weights_bytes": weights.stat().st_size,
        "tensors": len(expected),
        "torch": torch.__version__,
        "mask_atol": MASK_ATOL,
        "mask_rtol": MASK_RTOL,
        "pcm_atol": PCM_ATOL,
        "pcm_rtol": PCM_RTOL,
    }
    with (output / "weights.json").open("x", encoding="utf-8") as handle:
        json.dump(metadata, handle, indent=2)


def reference_case(model: nn.Module, frames: int, output: Path) -> None:
    """Capture one actual original learned forward, retaining every complex mask."""
    time = torch.arange(frames, dtype=torch.float64)
    pcm = torch.stack((
        0.21 * torch.sin(time * 0.071) + 0.13 * torch.cos(time * 0.019),
        0.17 * torch.cos(time * 0.113) + 0.08 * (time.remainder(2) * 2 - 1),
    )).float()
    pcm[:, 0] = torch.tensor((0.9, -0.8))
    pcm[:, -1] = torch.tensor((-0.7, 0.6))
    write_tensor(output / f"{frames}.pcm.f32", pcm)
    window = torch.hann_window(2048)
    spectral = torch.stft(pcm, 2048, 441, 2048, window, return_complex=True)
    features = torch.view_as_real(spectral).permute(2, 1, 0, 3).contiguous()
    write_tensor(output / f"{frames}.features.f32", features)
    captured: list[Tensor] = []

    def capture(_module: object, _input: object, value: Tensor) -> None:
        captured.append(value)

    heads = model.get_submodule("mask_estimators")
    if not isinstance(heads, nn.ModuleList) or len(heads) != 4:
        msg = "Incomplete original four-head mask module"
        raise ValueError(msg)
    hooks = [head.register_forward_hook(capture) for head in heads]
    try:
        prediction = model(pcm.unsqueeze(0))
    finally:
        for hook in hooks:
            hook.remove()
    masks = torch.stack(captured, dim=1).reshape(1, 4, -1, 1025, 2, 2)
    write_tensor(output / f"{frames}.masks.f32", masks)
    write_tensor(output / f"{frames}.output.f32", prediction)


def export(checkpoint: Path, config: Path, output: Path) -> None:
    """Export all effective weights and distinct learned chunk arithmetic cases."""
    if os.environ.get("CUDA_VISIBLE_DEVICES") != "-1":
        msg = "The admitted CPU oracle requires CUDA_VISIBLE_DEVICES=-1 before import"
        raise ValueError(msg)
    output.mkdir(parents=False, exist_ok=False)
    torch.set_num_threads(1)
    torch.set_num_interop_threads(1)
    model = load_model(checkpoint, config, "cpu")
    is_initialized = cast("Callable[[], bool]", torch.cuda.is_initialized)
    if is_initialized():
        msg = "The CPU oracle initialized CUDA"
        raise RuntimeError(msg)
    export_weights(model, output)
    # These are complete spectral/model chunks, not track padding substitutes.
    # The release-length case tests original1101 time positions, all62 frequency
    # positions and all learned weights; no padded/truncated attention dimension.
    with torch.inference_mode():
        for frames in (1025, 4099, 485100):
            reference_case(model, frames, output)
    if is_initialized():
        msg = "The CPU oracle initialized CUDA during inference"
        raise RuntimeError(msg)
    with (output / "reference-complete.json").open("x", encoding="utf-8") as handle:
        json.dump({"cases": [1025, 4099, 485100], "cuda_initialized": False}, handle)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("config", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    export(args.checkpoint, args.config, args.output)
