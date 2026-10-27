"""Local-only Beat This protocol2 CUDA-auto worker, with one fresh CPU fallback."""

import argparse
import gc
import hashlib
import importlib.util
import inspect
import json
import math
import os
import platform
import sys
import tomllib
from importlib.metadata import version
from pathlib import Path

import numpy as np
import soxr
import torch
from beat_this.inference import split_predict_aggregate
from beat_this.model.beat_tracker import BeatThis
from beat_this.model.postprocessor import Postprocessor
from beat_this.preprocessing import LogMelSpect
from beat_this.utils import replace_state_dict_key

FRONTEND_ID = "beat-this-1.1.0-soxr-hq-logmel-v1"
MAX_REQUEST_BYTES = 32 * 1024
MAX_PCM_BYTES = 512 * 1024 * 1024
MAX_CHECKPOINT_BYTES = 256 * 1024 * 1024
MAX_RESPONSE_BYTES = 8 * 1024 * 1024
MAX_PREDICTIONS = 250000
TARGET_RATE = 22050
HOP_LENGTH = 441
REFLECT_PADDING = 512
LOCK_SHA256 = "0a662b8a7b4058899bc8eb0b775d186bc1e5acf6f101ec2186d3f998c84d0f30"
MODEL_CONFIGURATION = {
    "package_version": "1.1.0",
    "checkpoint": "final0",
    "postprocessor": "minimal",
    "device": "auto",
    "precision": "float32",
    "frontend_id": FRONTEND_ID,
}


def _network_audit(event: str, arguments: tuple[object, ...]) -> None:
    if event in {"socket.connect", "socket.connect_ex", "socket.getaddrinfo", "socket.sendto"}:
        msg = "network_disabled_in_beat_worker"
        raise RuntimeError(msg)


def configure_runtime() -> None:
    """Deny worker network operations and bound Torch CPU computation threads."""
    sys.addaudithook(_network_audit)
    torch.set_num_threads(1)
    torch.set_num_interop_threads(1)
    torch.set_default_dtype(torch.float32)
    torch.backends.cuda.matmul.allow_tf32 = False
    torch.backends.cudnn.allow_tf32 = False
    torch.set_float32_matmul_precision("highest")


def environment_manifest(project_dir: Path) -> dict[str, object]:
    """Inspect only the locked Windows worker closure, never parent-App markers."""
    lock_bytes = (project_dir / "uv.lock").read_bytes()
    lock = tomllib.loads(lock_bytes.decode("utf-8"))
    # -I deliberately excludes the script directory from sys.path. Load only this
    # installation's separately hashed helper, never a parent or ambient module.
    spec = importlib.util.spec_from_file_location(
        "worker_profile_markers", project_dir / "profile_markers.py"
    )
    if spec is None or spec.loader is None:
        message = "missing_worker_marker_helper"
        raise ValueError(message)
    markers = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(markers)
    expected = markers.expected_packages(lock, "flitzis-beat-this-cuda-worker")
    installed = {name: version(name) for name in expected}
    if installed != expected:
        message = "environment_version_mismatch"
        raise ValueError(message)
    if (
        platform.python_version() != "3.12.13"
        or sys.platform != "win32"
        or platform.machine() != "AMD64"
        or torch.version.cuda != "12.8"
    ):
        message = "environment_platform_mismatch"
        raise ValueError(message)
    digest = hashlib.sha256(lock_bytes).hexdigest()
    if digest != LOCK_SHA256:
        message = "unknown_cuda_worker_lock"
        raise ValueError(message)
    return {
        "schema_version": 1,
        "frontend_id": FRONTEND_ID,
        "environment_id": f"uv-lock-sha256:{digest}",
        "lock_sha256": digest,
        "worker_sha256": hashlib.sha256((project_dir / "worker.py").read_bytes()).hexdigest(),
        "python_version": platform.python_version(),
        "packages": dict(sorted(installed.items())),
    }


def _bounded_json(path: Path) -> dict:
    with path.open("rb") as stream:
        encoded = stream.read(MAX_REQUEST_BYTES + 1)
    if len(encoded) > MAX_REQUEST_BYTES:
        msg = "request_limit"
        raise ValueError(msg)
    request = json.loads(encoded)
    if not isinstance(request, dict):
        msg = "invalid_request"
        raise TypeError(msg)
    return request


def _positive_integer(value: object, maximum: int) -> bool:
    return type(value) is int and 0 < value <= maximum


def validate_request(request: dict, environment: dict[str, object]) -> None:
    """Validate independent worker-side metadata before loading borrowed PCM."""
    if set(request) != {"schema_version", "identity", "model", "pcm", "execution_policy"}:
        msg = "request_fields_mismatch"
        raise ValueError(msg)
    if type(request["schema_version"]) is not int or request["schema_version"] != 2:
        msg = "request_schema_mismatch"
        raise ValueError(msg)
    if request["execution_policy"] not in {"auto", "cpu", "cuda"}:
        message = "execution_policy_mismatch"
        raise ValueError(message)
    _validate_identity(request["identity"])
    model = request["model"]
    expected = {**MODEL_CONFIGURATION, "environment_id": environment["environment_id"]}
    if not isinstance(model, dict) or set(model) != {*expected, "sha256"}:
        msg = "model_fields_mismatch"
        raise ValueError(msg)
    if any(model[name] != value for name, value in expected.items()):
        msg = "model_configuration_mismatch"
        raise ValueError(msg)
    digest = model["sha256"]
    if (
        not isinstance(digest, str)
        or len(digest) != 64
        or any(character not in "0123456789abcdef" for character in digest)
    ):
        msg = "invalid_checkpoint_digest"
        raise ValueError(msg)
    _validate_pcm(request["pcm"])


def _validate_identity(identity: dict) -> None:
    fields = {"pad_id", "request_id", "source_id", "source_generation"}
    if not isinstance(identity, dict) or set(identity) != fields:
        msg = "identity_fields_mismatch"
        raise ValueError(msg)
    for name in fields - {"source_id"}:
        value = identity[name]
        if type(value) is not int or not 0 <= value <= 2**64 - 1:
            msg = "invalid_identity_number"
            raise ValueError(msg)
    if not isinstance(identity["source_id"], str) or not 1 <= len(identity["source_id"]) <= 4096:
        msg = "invalid_source_identity"
        raise ValueError(msg)


def _validate_pcm(pcm: dict) -> None:
    fields = {"path", "sample_rate_hz", "frame_count", "origin_seconds", "dtype", "channels"}
    if not isinstance(pcm, dict) or set(pcm) != fields:
        msg = "pcm_fields_mismatch"
        raise ValueError(msg)
    if not isinstance(pcm["path"], str) or not Path(pcm["path"]).is_absolute():
        msg = "pcm_path_must_be_absolute"
        raise ValueError(msg)
    if not _positive_integer(pcm["sample_rate_hz"], 768000) or not _positive_integer(
        pcm["frame_count"], MAX_PCM_BYTES // 4
    ):
        msg = "pcm_shape_limit"
        raise ValueError(msg)
    if (
        pcm["dtype"] != "float32-le"
        or type(pcm["channels"]) is not int
        or pcm["channels"] != 1
        or type(pcm["origin_seconds"]) not in {float, int}
        or pcm["origin_seconds"] != 0
    ):
        msg = "pcm_format_mismatch"
        raise ValueError(msg)
    # Admission happens before rate conversion or STFT allocation. Never truncate input.
    max_frames = math.ceil(pcm["frame_count"] * TARGET_RATE / pcm["sample_rate_hz"])
    if max_frames // HOP_LENGTH + 1 > MAX_PREDICTIONS:
        msg = "prediction_limit"
        raise ValueError(msg)


def read_pcm(pcm: dict) -> np.ndarray:
    """Read every shared-mono frame without another decode or silence trimming."""
    with Path(pcm["path"]).open("rb") as stream:
        if os.fstat(stream.fileno()).st_size != pcm["frame_count"] * 4:
            msg = "pcm_size_mismatch"
            raise ValueError(msg)
        signal = np.fromfile(stream, dtype="<f4", count=pcm["frame_count"])
        if len(signal) != pcm["frame_count"] or stream.read(1):
            msg = "pcm_changed_during_read"
            raise ValueError(msg)
    if not np.isfinite(signal).all():
        msg = "nonfinite_pcm"
        raise ValueError(msg)
    return signal


def prepare_signal(signal: np.ndarray, sample_rate: int) -> np.ndarray:
    """Match upstream full-buffer soxr HQ resampling, including its rounded tail length."""
    if sample_rate != TARGET_RATE:
        signal = soxr.resample(signal, in_rate=sample_rate, out_rate=TARGET_RATE, quality="HQ")
    if len(signal) <= REFLECT_PADDING:
        msg = "audio_too_short_for_reference_reflect_padding"
        raise ValueError(msg)
    return signal


def signal_to_spectrogram(signal: np.ndarray, sample_rate: int) -> torch.Tensor:
    """Preserve upstream centered Hann STFT/Slaney log-mel frame-zero convention."""
    prepared = prepare_signal(signal, sample_rate)
    return LogMelSpect(device="cpu")(torch.tensor(prepared, dtype=torch.float32, device="cpu"))


def load_local_model(checkpoint_path: Path, expected_digest: str) -> BeatThis:
    """Hash and load the same local file handle; never call upstream URL/shortname loaders."""
    if not checkpoint_path.is_absolute():
        msg = "checkpoint_path_must_be_absolute"
        raise ValueError(msg)
    with checkpoint_path.open("rb") as stream:
        size = os.fstat(stream.fileno()).st_size
        if not 0 < size <= MAX_CHECKPOINT_BYTES:
            msg = "checkpoint_size_limit"
            raise ValueError(msg)
        digest = hashlib.sha256()
        read_bytes = 0
        while chunk := stream.read(1024 * 1024):
            read_bytes += len(chunk)
            if read_bytes > MAX_CHECKPOINT_BYTES:
                msg = "checkpoint_size_limit"
                raise ValueError(msg)
            digest.update(chunk)
        if read_bytes != size or digest.hexdigest() != expected_digest:
            msg = "checkpoint_hash_mismatch"
            raise ValueError(msg)
        stream.seek(0)
        checkpoint = torch.load(stream, map_location="cpu", weights_only=True)
    parameters = inspect.signature(BeatThis).parameters
    hyperparameters = {
        name: value for name, value in checkpoint["hyper_parameters"].items() if name in parameters
    }
    model = BeatThis(**hyperparameters)
    model.load_state_dict(replace_state_dict_key(checkpoint["state_dict"], "model.", ""))
    return model.to(device="cpu", dtype=torch.float32).eval()


def _selected_device(policy: str) -> tuple[str, str | None]:
    if policy == "cpu":
        return "cpu", None
    try:
        available = torch.cuda.is_available()
    except (RuntimeError, OSError):
        available, reason = False, "cuda_probe_error"
    else:
        reason = None if available else "cuda_unavailable"
    if available:
        return "cuda:0", None
    if policy == "cuda":
        raise RuntimeError(reason)
    return "cpu", reason


def _placed(model: BeatThis, spectrogram: torch.Tensor, device: str) -> bool:
    target = torch.device(device)
    parameters = list(model.parameters())
    return (
        bool(parameters)
        and all(p.device == target and p.dtype == torch.float32 for p in parameters)
        and all(
            b.device == target and (not b.is_floating_point() or b.dtype == torch.float32)
            for b in model.buffers()
        )
        and spectrogram.device == target
        and spectrogram.dtype == torch.float32
    )


def _execution(request: dict, device: str, fallback: str | None) -> dict:
    index = torch.device(device).index
    return {
        "schema_version": 1,
        "requested_policy": request["execution_policy"],
        "device": device,
        "device_name": torch.cuda.get_device_name(index) if index is not None else None,
        "precision": "float32",
        "torch_version": str(torch.__version__),
        "cuda_build_version": torch.version.cuda,
        "fallback_reason": fallback,
    }


def _require_placement(model: BeatThis, placed_input: torch.Tensor, device: str) -> None:
    if not _placed(model, placed_input, device):
        message = "inference_device_mismatch"
        raise RuntimeError(message)


def _emit_execution(request: dict, execution: dict) -> None:
    # Diagnostic placement progress on stderr. The final validated stdout
    # response is the completion receipt; placement alone never confirms it.
    event = {"event": "execution", "identity": request["identity"], "execution": execution}
    sys.stderr.write(json.dumps(event, allow_nan=False, separators=(",", ":")) + "\n")
    sys.stderr.flush()


def _attempt(
    request: dict,
    checkpoint_path: Path,
    cpu_spectrogram: torch.Tensor,
    device: str,
    fallback: str | None,
) -> tuple[dict | None, dict | str]:
    # Hash/protocol/checkpoint errors are outside CUDA fallback catches.
    model = load_local_model(checkpoint_path, request["model"]["sha256"])
    placed_input = None
    logits = None
    try:
        try:
            model = model.to(device=device, dtype=torch.float32).eval()
            placed_input = cpu_spectrogram.to(device=device, dtype=torch.float32)
            _require_placement(model, placed_input, device)
            execution = _execution(request, device, fallback)
        except (RuntimeError, OSError):
            if device != "cpu":
                return None, "cuda_placement_error"
            raise
        _emit_execution(request, execution)
        try:
            with torch.inference_mode():
                logits = split_predict_aggregate(
                    spect=placed_input,
                    chunk_size=1500,
                    border_size=6,
                    overlap_mode="keep_first",
                    model=model,
                )
                # CPU postprocessing retains its existing numerical implementation.
                output = {
                    name: logits[name].detach().to(device="cpu", dtype=torch.float32)
                    for name in ("beat", "downbeat")
                }
        except (RuntimeError, OSError):
            if device != "cpu":
                return None, "cuda_inference_error"
            raise
        return output, execution
    finally:
        # Returning a code rather than a CUDA exception avoids retaining its
        # traceback/model tensors while a fresh CPU attempt runs.
        del logits, placed_input, model


def _release_failed_cuda() -> None:
    gc.collect()
    try:
        torch.cuda.empty_cache()
    except (RuntimeError, OSError):
        # No failed model/input/logit object survives _attempt. The process
        # owner still retires the complete CUDA context at its actual terminal.
        return


def infer(request: dict, checkpoint_path: Path) -> dict:
    """Use one complete CPU frontend and at most one fresh CPU fallback model."""
    signal = read_pcm(request["pcm"])
    spectrogram = signal_to_spectrogram(signal, request["pcm"]["sample_rate_hz"])
    del signal
    device, fallback = _selected_device(request["execution_policy"])
    logits, execution = _attempt(request, checkpoint_path, spectrogram, device, fallback)
    if logits is None:
        if request["execution_policy"] != "auto":
            raise RuntimeError(execution)
        _release_failed_cuda()
        # Reload/reverify the checkpoint; never move a failed CUDA model to CPU.
        logits, execution = _attempt(request, checkpoint_path, spectrogram, "cpu", str(execution))
    if logits is None or not isinstance(execution, dict):
        message = "invalid_inference_attempt_result"
        raise RuntimeError(message)
    # Frontend, postprocessing, I/O, cancellation and protocol errors never retry.
    beats, downbeats = Postprocessor(type="minimal", fps=50)(logits["beat"], logits["downbeat"])
    duration = request["pcm"]["frame_count"] / request["pcm"]["sample_rate_hz"]
    return {
        "schema_version": 2,
        "identity": request["identity"],
        "model": request["model"],
        "execution": execution,
        "predictions": {
            "beat_seconds": beats[(beats >= 0) & (beats < duration)].tolist(),
            "downbeat_seconds": downbeats[(downbeats >= 0) & (downbeats < duration)].tolist(),
            "beat_logits": logits["beat"].float().tolist(),
            "downbeat_logits": logits["downbeat"].float().tolist(),
        },
    }


def main() -> None:
    """Verify setup provenance or execute exactly one bounded file-based request."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--request", type=Path)
    parser.add_argument("--checkpoint", type=Path)
    parser.add_argument("--environment-manifest", type=Path)
    options = parser.parse_args()
    configure_runtime()
    environment = environment_manifest(Path(__file__).resolve().parent)
    if options.environment_manifest:
        if options.request or options.checkpoint:
            parser.error("environment verification cannot also execute inference")
        options.environment_manifest.write_text(json.dumps(environment, indent=2), encoding="utf-8")
        return
    if options.request is None or options.checkpoint is None:
        parser.error("--request and --checkpoint are required for inference")
    request = _bounded_json(options.request)
    validate_request(request, environment)
    encoded = json.dumps(infer(request, options.checkpoint), allow_nan=False, separators=(",", ":"))
    output = encoded.encode("utf-8")
    if len(output) > MAX_RESPONSE_BYTES:
        msg = "response_limit"
        raise ValueError(msg)
    sys.stdout.buffer.write(output)


if __name__ == "__main__":
    main()
