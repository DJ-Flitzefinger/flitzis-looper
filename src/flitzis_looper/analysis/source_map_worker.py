"""One borrowed Native source-map request through the installed Beat This worker.

The existing Rust cold job owns admission, complete-source custody and the staged
PCM file. This bridge creates no job, polling thread, installation or PCM owner.
"""

import json
import re
from dataclasses import asdict, replace
from pathlib import Path
from threading import Event, current_thread, main_thread
from time import monotonic
from typing import TYPE_CHECKING, Protocol

from pydantic import TypeAdapter

from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatComponentResult,
    BeatModelIdentity,
    BeatWorkerRequest,
    MonoPcmInput,
    WorkerLimits,
    validate_component_result,
)
from flitzis_looper.analysis.publication import _pack
from flitzis_looper.analysis.setup import load_worker_configuration
from flitzis_looper.analysis.worker import BeatWorkerAdapter

if TYPE_CHECKING:
    from flitzis_looper.analysis.contracts import ComponentStatus
    from flitzis_looper.analysis.worker import WorkerConfiguration

MAX_SOURCE_MAP_WORKER_BYTES = 16 * 1024 * 1024
_IDENTITY = TypeAdapter(AnalysisIdentity)
_POLL_SECONDS = 0.02
_ARRAY_NAMES = ("beat_seconds", "downbeat_seconds", "beat_logits", "downbeat_logits")


class NativeCancellationProbe(Protocol):
    """Read the original Native request; never cancel or release it from Python."""

    def is_cancelled(self) -> bool: ...


class _NativeCancellationEvent(Event):
    """Adapt the existing Event polling API without another cancellation owner."""

    def __init__(self, probe: NativeCancellationProbe) -> None:
        super().__init__()
        self._probe = probe

    def is_set(self) -> bool:
        return self._probe.is_cancelled()

    def wait(self, timeout: float | None = None) -> bool:
        deadline = None if timeout is None else monotonic() + max(0.0, timeout)
        while not self.is_set():
            remaining = None if deadline is None else deadline - monotonic()
            if remaining is not None and remaining <= 0.0:
                return False
            super().wait(_POLL_SECONDS if remaining is None else min(_POLL_SECONDS, remaining))
        return True


def _envelope(
    identity: AnalysisIdentity,
    input_metadata: dict[str, object],
    model: BeatModelIdentity | None,
    status: ComponentStatus,
    reason: str,
    predictions: dict[str, str] | None = None,
) -> str:
    envelope = {
        "schema_version": 1,
        "encoding": "source-map-beat-this-worker-v1",
        "identity": asdict(identity),
        "input": input_metadata,
        "model": None if model is None else asdict(model),
        "status": status,
        "reason": reason[:1024],
        "resources_released": True,
        "predictions": predictions,
    }
    encoded = json.dumps(envelope, allow_nan=False, separators=(",", ":"))
    if len(encoded.encode("utf-8")) > MAX_SOURCE_MAP_WORKER_BYTES:
        envelope.update(
            status="failed", reason="source_map_worker_response_limit", predictions=None
        )
        encoded = json.dumps(envelope, allow_nan=False, separators=(",", ":"))
    return encoded


def _validated_input(
    pcm_path: str,
    sample_rate_hz: int,
    frame_count: int,
    mono_sha256: str,
    identity_json: str,
    installation_root: str,
) -> tuple[AnalysisIdentity, MonoPcmInput, Path, dict[str, object]]:
    """Check the borrowed Native identity and actual staged-input geometry."""
    if len(identity_json.encode("utf-8")) > 32 * 1024:
        message = "source_map_worker_identity_limit"
        raise ValueError(message)
    identity = _IDENTITY.validate_json(identity_json, strict=True)
    if identity.request_id == 0 or identity.source_generation == 0:
        message = "source_map_worker_identity_unavailable"
        raise ValueError(message)
    if re.fullmatch(r"[0-9a-f]{64}", mono_sha256) is None:
        message = "source_map_worker_mono_digest_invalid"
        raise ValueError(message)
    pcm, installed = Path(pcm_path), Path(installation_root)
    if not pcm.is_absolute() or not installed.is_absolute():
        message = "source_map_worker_paths_must_be_absolute"
        raise ValueError(message)
    if (
        type(sample_rate_hz) is not int
        or not 0 < sample_rate_hz <= 768000
        or type(frame_count) is not int
        or frame_count <= 0
    ):
        message = "source_map_worker_input_geometry_invalid"
        raise ValueError(message)
    input_metadata: dict[str, object] = {
        "sample_rate_hz": sample_rate_hz,
        "frame_count": frame_count,
        "source_zero_frame": 0,
        "mono_sha256": mono_sha256,
        "dtype": "float32-le",
    }
    return identity, MonoPcmInput(pcm, sample_rate_hz, frame_count), installed, input_metadata


def _run_retired(
    request: BeatWorkerRequest,
    configuration: WorkerConfiguration,
    cancel: _NativeCancellationEvent,
) -> BeatComponentResult:
    """Validate one existing adapter outcome while retaining its real readers."""
    limits = WorkerLimits()
    adapter = BeatWorkerAdapter(configuration, limits)
    try:
        result = adapter.run(request, cancel)
        validate_component_result(result, request, limits)
    except (OSError, RuntimeError, TypeError, ValueError) as error:
        result = BeatComponentResult(request.identity, request.model, "failed", str(error)[:1024])
    finally:
        # The adapter's existing retiring process owns descendants, pipe readers
        # and temporary files. A finite reap failure is not permission to return.
        while not adapter.retired.wait(_POLL_SECONDS):
            pass
    result = replace(result, resources_released=True)
    if cancel.is_set():
        result = replace(result, status="cancelled", reason="cancelled", predictions=None)
    return result


def run_source_map_beat_this(
    pcm_path: str,
    sample_rate_hz: int,
    frame_count: int,
    mono_sha256: str,
    identity_json: str,
    installation_root: str,
    cancellation: NativeCancellationProbe,
) -> str:
    """Block on the existing cold worker until process/readers actually retire.

    Input metadata is supplied and verified by the owning Native staging pass.
    It describes that actual file, not the worker's internal resampled frontend.
    The separate 16-MiB bridge limit does not change legacy publication limits.
    """
    if current_thread() is main_thread():
        message = "source-map beat analysis must run on the existing background worker"
        raise RuntimeError(message)
    identity, pcm, installed, input_metadata = _validated_input(
        pcm_path, sample_rate_hz, frame_count, mono_sha256, identity_json, installation_root
    )
    cancel = _NativeCancellationEvent(cancellation)
    if cancel.is_set():
        return _envelope(identity, input_metadata, None, "cancelled", "cancelled")
    try:
        # Existing configuration verification is synchronous and local. Original
        # Native admission remains held until it returns; no setup is called.
        configuration = load_worker_configuration(installed, pcm.path.parent)
    except (OSError, ValueError) as error:
        status: ComponentStatus = "cancelled" if cancel.is_set() else "unavailable"
        reason = (
            "cancelled" if status == "cancelled" else f"worker_installation_unavailable: {error}"
        )
        return _envelope(identity, input_metadata, None, status, reason)
    model = configuration.model
    if cancel.is_set():
        return _envelope(identity, input_metadata, model, "cancelled", "cancelled")
    result = _run_retired(BeatWorkerRequest(identity, pcm, model), configuration, cancel)
    packed: dict[str, str] | None = None
    if result.status == "ready" and result.predictions is not None:
        packed = {
            "encoding": "float64-le/base64",
            **{name: _pack(getattr(result.predictions, name)) for name in _ARRAY_NAMES},
        }
    return _envelope(identity, input_metadata, model, result.status, result.reason, packed)
