"""Versioned metadata-only boundary for the optional beat worker."""

import math
from dataclasses import dataclass
from itertools import pairwise
from pathlib import Path
from typing import Annotated, Literal

from pydantic import ConfigDict, Field, FiniteFloat, TypeAdapter

type IdentityNumber = Annotated[int, Field(strict=True, ge=0, le=2**64 - 1)]
type IdentityText = Annotated[str, Field(strict=True, min_length=1, max_length=4096)]
type PositiveNumber = Annotated[int, Field(strict=True, gt=0)]
type ProtocolOne = Annotated[int, Field(strict=True, ge=1, le=1)]
type ComponentStatus = Literal["ready", "unavailable", "failed", "cancelled"]


class _StrictDto:
    __slots__ = ()
    __pydantic_config__ = ConfigDict(extra="forbid")


@dataclass(frozen=True, slots=True)
class AnalysisIdentity(_StrictDto):
    """Identify a loaded source and the existing analysis request that owns it."""

    pad_id: IdentityNumber
    request_id: IdentityNumber
    source_id: IdentityText
    source_generation: IdentityNumber


@dataclass(frozen=True, slots=True)
class MonoPcmInput(_StrictDto):
    """Borrow full-track mono PCM; its owner retains the file until readers retire."""

    path: Path
    sample_rate_hz: Annotated[int, Field(strict=True, gt=0, le=768000)]
    frame_count: PositiveNumber
    origin_seconds: Annotated[FiniteFloat, Field(ge=0.0, le=0.0)] = 0.0
    dtype: Literal["float32-le"] = "float32-le"
    channels: ProtocolOne = 1


@dataclass(frozen=True, slots=True)
class BeatModelIdentity(_StrictDto):
    """Explicit model/configuration provenance, empty digest means unconfigured."""

    sha256: Annotated[str, Field(strict=True, max_length=64)] = ""
    frontend_id: IdentityText = "unconfigured"
    environment_id: IdentityText = "unconfigured"
    package_version: Literal["1.1.0"] = "1.1.0"
    checkpoint: Literal["final0"] = "final0"
    postprocessor: Literal["minimal"] = "minimal"
    device: Literal["cpu"] = "cpu"
    precision: Literal["float32"] = "float32"


@dataclass(frozen=True, slots=True)
class BeatWorkerRequest(_StrictDto):
    """One versioned borrowed PCM request, never an encoded audio JSON payload."""

    identity: AnalysisIdentity
    pcm: MonoPcmInput
    model: BeatModelIdentity
    schema_version: ProtocolOne = 1


@dataclass(frozen=True, slots=True)
class BeatPredictions(_StrictDto):
    """Raw source-relative seconds and uncalibrated 50-Hz model logits."""

    beat_seconds: tuple[FiniteFloat, ...]
    downbeat_seconds: tuple[FiniteFloat, ...]
    beat_logits: tuple[FiniteFloat, ...]
    downbeat_logits: tuple[FiniteFloat, ...]


@dataclass(frozen=True, slots=True)
class BeatWorkerResponse(_StrictDto):
    """Success response echoed by an explicitly configured local worker."""

    identity: AnalysisIdentity
    model: BeatModelIdentity
    predictions: BeatPredictions
    schema_version: ProtocolOne


@dataclass(frozen=True, slots=True)
class BeatComponentResult(_StrictDto):
    """Independent attempt outcome; a retiring process is not a terminal branch."""

    identity: AnalysisIdentity
    model: BeatModelIdentity
    status: ComponentStatus
    reason: str
    predictions: BeatPredictions | None = None
    resources_released: bool = True


@dataclass(frozen=True, slots=True)
class WorkerLimits:
    """Provisional B1a engineering bounds; real inference acceptance is B1b/B2."""

    max_pcm_bytes: int = 512 * 1024 * 1024
    max_checkpoint_bytes: int = 256 * 1024 * 1024
    max_request_bytes: int = 32 * 1024
    max_response_bytes: int = 8 * 1024 * 1024
    max_prediction_count: int = 250000
    timeout_seconds: float = 120.0
    reap_timeout_seconds: float = 5.0

    def __post_init__(self) -> None:
        """Reject limits that could remove bounds or make timeout checks ineffective."""
        values = (
            self.max_pcm_bytes,
            self.max_checkpoint_bytes,
            self.max_request_bytes,
            self.max_response_bytes,
            self.max_prediction_count,
            self.timeout_seconds,
            self.reap_timeout_seconds,
        )
        if any(not math.isfinite(value) or value <= 0 for value in values):
            msg = "worker limits must be finite and positive"
            raise ValueError(msg)


_REQUEST_ADAPTER = TypeAdapter(BeatWorkerRequest)
_RESPONSE_ADAPTER = TypeAdapter(BeatWorkerResponse)
_COMPONENT_ADAPTER = TypeAdapter(BeatComponentResult)


def encode_request(request: BeatWorkerRequest, limits: WorkerLimits) -> bytes:
    """Validate the DTO, source shape and whole-file byte extent before process startup."""
    encoded = _REQUEST_ADAPTER.dump_json(request)
    _REQUEST_ADAPTER.validate_json(encoded, strict=True)
    if len(encoded) > limits.max_request_bytes:
        msg = "request_limit"
        raise ValueError(msg)
    if request.pcm.frame_count * 4 > limits.max_pcm_bytes:
        msg = "pcm_limit"
        raise ValueError(msg)
    if not Path(request.pcm.path).is_absolute():
        msg = "pcm_path_must_be_absolute"
        raise ValueError(msg)
    if request.pcm.path.stat().st_size != request.pcm.frame_count * 4:
        msg = "pcm_size_mismatch"
        raise ValueError(msg)
    return encoded


def decode_response(
    encoded: bytes, request: BeatWorkerRequest, limits: WorkerLimits
) -> BeatPredictions:
    """Reject mismatched identity, nonfinite/unsorted positions and excessive output."""
    if len(encoded) > limits.max_response_bytes:
        msg = "response_limit"
        raise ValueError(msg)
    response = _RESPONSE_ADAPTER.validate_json(encoded, strict=True)
    if response.identity != request.identity or response.model != request.model:
        msg = "response_identity_mismatch"
        raise ValueError(msg)
    predictions = response.predictions
    duration = request.pcm.frame_count / request.pcm.sample_rate_hz
    for times in (predictions.beat_seconds, predictions.downbeat_seconds):
        if len(times) > limits.max_prediction_count:
            msg = "prediction_limit"
            raise ValueError(msg)
        if any(value < 0.0 or value >= duration for value in times):
            msg = "prediction_outside_source"
            raise ValueError(msg)
        if any(before >= after for before, after in pairwise(times)):
            msg = "predictions_not_strictly_increasing"
            raise ValueError(msg)
    if len(predictions.beat_logits) != len(predictions.downbeat_logits):
        msg = "logit_count_mismatch"
        raise ValueError(msg)
    if len(predictions.beat_logits) > limits.max_prediction_count:
        msg = "logit_limit"
        raise ValueError(msg)
    return predictions


def validate_component_result(
    result: BeatComponentResult, request: BeatWorkerRequest, limits: WorkerLimits
) -> None:
    """Validate every adapter outcome before the job owner merges independent results.

    Injected adapters obey the same identity and prediction rules as the subprocess
    boundary. Resource retirement remains a separate event owned by that adapter;
    callers must wait for it even when validation rejects this result.
    """
    if not isinstance(result, BeatComponentResult):
        msg = "invalid_beat_component_type"
        raise TypeError(msg)
    encoded = _COMPONENT_ADAPTER.dump_json(result)
    if len(encoded) > limits.max_response_bytes:
        msg = "beat_component_limit"
        raise ValueError(msg)
    validated = _COMPONENT_ADAPTER.validate_json(encoded, strict=True)
    if validated.identity != request.identity or validated.model != request.model:
        msg = "beat_component_identity_mismatch"
        raise ValueError(msg)
    if validated.status != "ready":
        if validated.predictions is not None:
            msg = "unsuccessful_beat_component_has_predictions"
            raise ValueError(msg)
        return
    if validated.predictions is None:
        msg = "ready_beat_component_has_no_predictions"
        raise ValueError(msg)
    response = BeatWorkerResponse(
        validated.identity, validated.model, validated.predictions, schema_version=1
    )
    decode_response(_RESPONSE_ADAPTER.dump_json(response), request, limits)
