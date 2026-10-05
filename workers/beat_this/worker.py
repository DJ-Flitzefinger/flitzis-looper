"""Local-only, one-request Beat This 1.1.0 CPU worker (Python 3.12 runtime)."""

import argparse
import hashlib
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
MODEL_CONFIGURATION = {
    "package_version": "1.1.0",
    "checkpoint": "final0",
    "postprocessor": "minimal",
    "device": "cpu",
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


def environment_manifest(project_dir: Path) -> dict[str, object]:
    """Verify the runtime dependency closure against its immutable installation lock."""
    lock_bytes = (project_dir / "uv.lock").read_bytes()
    lock = tomllib.loads(lock_bytes.decode("utf-8"))
    packages = {package["name"]: package for package in lock["package"]}
    root = packages["flitzis-beat-this-worker"]
    pending = [dependency["name"] for dependency in root["dependencies"]]
    installed = {}
    while pending:
        name = pending.pop()
        if name in installed:
            continue
        package = packages[name]
        installed[name] = version(name)
        if installed[name] != package["version"]:
            msg = f"environment_version_mismatch:{name}"
            raise ValueError(msg)
        pending.extend(dependency["name"] for dependency in package.get("dependencies", []))
    if (
        platform.python_version() != "3.12.13"
        or sys.platform != "win32"
        or platform.machine() != "AMD64"
        or torch.version.cuda is not None
    ):
        msg = "environment_platform_mismatch"
        raise ValueError(msg)
    digest = hashlib.sha256(lock_bytes).hexdigest()
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
    if set(request) != {"schema_version", "identity", "model", "pcm"}:
        msg = "request_fields_mismatch"
        raise ValueError(msg)
    if type(request["schema_version"]) is not int or request["schema_version"] != 1:
        msg = "request_schema_mismatch"
        raise ValueError(msg)
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


def infer(request: dict, checkpoint_path: Path) -> dict:
    """Run pinned full-track frontend/chunk aggregation and minimal postprocessing."""
    signal = read_pcm(request["pcm"])
    model = load_local_model(checkpoint_path, request["model"]["sha256"])
    with torch.inference_mode():
        spectrogram = signal_to_spectrogram(signal, request["pcm"]["sample_rate_hz"])
        logits = split_predict_aggregate(
            spect=spectrogram,
            chunk_size=1500,
            border_size=6,
            overlap_mode="keep_first",
            model=model,
        )
        beats, downbeats = Postprocessor(type="minimal", fps=50)(logits["beat"], logits["downbeat"])
    duration = request["pcm"]["frame_count"] / request["pcm"]["sample_rate_hz"]
    # Centered STFT can include a final frame at the exclusive source boundary.
    # Keep all logits as evidence; only physically out-of-source detections are excluded.
    return {
        "schema_version": 1,
        "identity": request["identity"],
        "model": request["model"],
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
