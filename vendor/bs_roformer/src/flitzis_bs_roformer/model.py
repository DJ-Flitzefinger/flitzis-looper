"""Load only the pinned, four-source MUSDB18HQ network and configuration."""

import hashlib
import importlib
import os
import sys
from types import ModuleType
from typing import TYPE_CHECKING, Protocol, cast

import torch
from torch import Tensor, nn

from flitzis_bs_roformer.network import attend
from flitzis_looper.inference_provider import cuda_operation
from flitzis_looper.stem_cancellation import check_cancelled

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

CONFIG_SHA256 = "d8afb980318d0c08b9c2e24a7adc00d4f3150320c127a7e4de861800d1321939"
CONFIG_BYTES = 4566
CHECKPOINT_BYTES = 527385512
CHECKPOINT_SHA256 = "3e9daecd70aaed5b5a0d1f861cc4d77eaa45afb3fc6301b1cf32c1be0f5868fb"
SOURCES = ("drums", "bass", "other", "vocals")
SAMPLE_RATE = 44_100
CHUNK_SIZE = 485_100
BATCH_SIZE = 2
OVERLAP = 2
MAX_TRANSIENT_PCM_BYTES = 1 << 30


class _ArtifactHandle(Protocol):
    def fileno(self) -> int: ...

    def read(self, size: int = -1, /) -> bytes: ...

    def readinto(self, buffer: bytearray, /) -> int: ...

    def readable(self) -> bool: ...


def verify_file(
    path: Path,
    expected_bytes: int,
    expected_sha256: str,
    checkpoint: Callable[[str], None] | None = None,
) -> None:
    """Check an installed artifact's complete size and SHA256 before loading it."""
    with path.open("rb") as handle:
        _verify_handle(handle, path.name, expected_bytes, expected_sha256, checkpoint)


def _verify_handle(
    handle: _ArtifactHandle,
    name: str,
    expected_bytes: int,
    expected_sha256: str,
    checkpoint: Callable[[str], None] | None = None,
) -> None:
    check_cancelled(checkpoint, "before_model_artifact")
    if os.fstat(handle.fileno()).st_size != expected_bytes:
        msg = f"Unexpected artifact size: {name}"
        raise ValueError(msg)
    if checkpoint is None:
        actual = hashlib.file_digest(handle, "sha256").hexdigest()
    else:
        digest = hashlib.sha256()
        while True:
            check_cancelled(checkpoint, "model_artifact_block")
            chunk = handle.read(65536)
            if not chunk:
                break
            digest.update(chunk)
        actual = digest.hexdigest()
    if actual != expected_sha256:
        msg = f"Unexpected artifact identity: {name}"
        raise ValueError(msg)


def _load_weights(path: Path, checkpoint: Callable[[str], None] | None = None) -> dict[str, Tensor]:
    with path.open("rb") as handle:
        _verify_handle(handle, path.name, CHECKPOINT_BYTES, CHECKPOINT_SHA256, checkpoint)
        handle.seek(0)
        check_cancelled(checkpoint, "before_tensor_load")
        weights = torch.load(handle, map_location="cpu", weights_only=True)
        check_cancelled(checkpoint, "after_tensor_load")
    if not isinstance(weights, dict) or not all(
        isinstance(key, str) and isinstance(value, Tensor) for key, value in weights.items()
    ):
        msg = "BS-RoFormer checkpoint must contain a direct tensor state dictionary"
        raise ValueError(msg)
    return cast("dict[str, Tensor]", weights)


def _network_module() -> ModuleType:
    """Resolve upstream's original absolute import without installing `models`."""
    aliases = {
        "models": ModuleType("models"),
        "models.bs_roformer": ModuleType("models.bs_roformer"),
        "models.bs_roformer.attend": attend,
    }
    previous = {name: sys.modules.get(name) for name in aliases}
    try:
        sys.modules.update(aliases)
        return importlib.import_module("flitzis_bs_roformer.network.bs_roformer")
    finally:
        for name, module in previous.items():
            if module is None:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = module


def load_model(
    checkpoint: Path,
    config: Path,
    device: str,
    *,
    cancellation: Callable[[str], None] | None = None,
) -> nn.Module:
    """Initialize the exact release model and strictly load a verified state dictionary.

    The SHA-pinned configuration is deliberately represented as explicit arguments,
    rather than loading YAML's Python tuple tags or its training dependencies.
    """
    check_cancelled(cancellation, "before_model_load")
    verify_file(config, CONFIG_BYTES, CONFIG_SHA256, cancellation)
    if device == "cuda" and not torch.cuda.is_available():
        msg = "CUDA unavailable for BS-RoFormer"
        with cuda_operation(device):
            raise RuntimeError(msg)
    weights = _load_weights(checkpoint, cancellation)
    network = _network_module()
    check_cancelled(cancellation, "before_network_init")
    model = cast(
        "nn.Module",
        network.BSRoformer(
            dim=384,
            depth=8,
            stereo=True,
            num_stems=4,
            time_transformer_depth=1,
            freq_transformer_depth=1,
            linear_transformer_depth=0,
            freqs_per_bands=(2,) * 24 + (4,) * 12 + (12,) * 8 + (24,) * 8 + (48,) * 8 + (128, 129),
            dim_head=64,
            heads=8,
            attn_dropout=0.1,
            ff_dropout=0.1,
            flash_attn=True,
            dim_freqs_in=1025,
            stft_n_fft=2048,
            stft_hop_length=441,
            stft_win_length=2048,
            stft_normalized=False,
            mask_estimator_depth=2,
            multi_stft_resolution_loss_weight=1.0,
            multi_stft_resolutions_window_sizes=(4096, 2048, 1024, 512, 256),
            multi_stft_hop_size=147,
            multi_stft_normalized=False,
            mlp_expansion_factor=2,
            use_torch_checkpoint=False,
            skip_connection=False,
        ),
    )
    check_cancelled(cancellation, "before_state_dict_load")
    model.load_state_dict(weights, strict=True)
    check_cancelled(cancellation, "after_state_dict_load")
    del weights
    with cuda_operation(device):
        result = model.eval().to(device)
    check_cancelled(cancellation, "after_model_load")
    return result
