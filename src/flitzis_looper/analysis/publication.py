"""Lossless, bounded diagnostic envelopes independent of temporary PCM ownership."""

import base64
import json
import math
import struct
from dataclasses import asdict, dataclass, replace
from typing import Annotated

from pydantic import ConfigDict, Field, TypeAdapter

from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatComponentResult,
    BeatWorkerRequest,
    WorkerLimits,
    validate_component_result,
)

MAX_ENVELOPE_BYTES = 1024 * 1024
_ENCODING = "float64-le/base64"
_ARRAY_NAMES = ("beat_seconds", "downbeat_seconds", "beat_logits", "downbeat_logits")
_DEFAULT_LIMITS = WorkerLimits()
_MAX_COUNT = _DEFAULT_LIMITS.max_prediction_count


@dataclass(frozen=True, slots=True)
class PublishedAnalysisResult:
    """Decoded diagnostic result; reading it neither adopts analysis nor runs inference."""

    __pydantic_config__ = ConfigDict(extra="forbid")

    identity: AnalysisIdentity
    beat: BeatComponentResult
    key: dict[str, object]
    schema_version: Annotated[int, Field(strict=True, ge=1, le=2)]


_RESULT_ADAPTER = TypeAdapter(PublishedAnalysisResult)


def _pack(values: tuple[float, ...]) -> str:
    if len(values) > _MAX_COUNT:
        msg = "beat prediction limit exceeded"
        raise ValueError(msg)
    if any(not math.isfinite(value) for value in values):
        msg = "nonfinite beat prediction"
        raise ValueError(msg)
    return base64.b64encode(struct.pack(f"<{len(values)}d", *values)).decode("ascii")


def _unpack(encoded: object) -> tuple[float, ...]:
    # Check text extent before allocating decoded bytes; Base64 never compresses data.
    if not isinstance(encoded, str) or len(encoded) > 4 * ((_MAX_COUNT * 8 + 2) // 3):
        msg = "invalid packed prediction extent"
        raise ValueError(msg)
    raw = base64.b64decode(encoded, validate=True)
    if len(raw) % 8 or len(raw) // 8 > _MAX_COUNT:
        msg = "invalid packed prediction byte count"
        raise ValueError(msg)
    # Python's strict decoder alone accepts nonzero padding bits and surplus padding.
    if base64.b64encode(raw).decode("ascii") != encoded:
        msg = "noncanonical packed prediction"
        raise ValueError(msg)
    return tuple(value for (value,) in struct.iter_unpack("<d", raw))


def encode_result(beat: BeatComponentResult, key: dict[str, object]) -> str:
    """Encode validated retired outcomes, explicitly failing beats if full output is oversize.

    Every ready result uses inline uncompressed binary64 arrays, preserving even
    values that are not exactly representable in float32. No external files survive
    retirement. The caller validates component identity/source extent before packing.
    """
    if (beat.status == "ready") != (beat.predictions is not None):
        msg = "beat component status/predictions mismatch"
        raise ValueError(msg)
    component = asdict(replace(beat, predictions=None))
    version = 1
    if beat.status == "ready" and beat.predictions is not None:
        version = 2
        component["predictions"] = {
            "encoding": _ENCODING,
            **{name: _pack(getattr(beat.predictions, name)) for name in _ARRAY_NAMES},
        }
    envelope = {
        "schema_version": version,
        "identity": asdict(beat.identity),
        "beat": component,
        "key": key,
    }
    encoded = json.dumps(envelope, allow_nan=False, separators=(",", ":"))
    if len(encoded.encode("utf-8")) <= MAX_ENVELOPE_BYTES:
        return encoded
    failed = replace(
        beat, status="failed", reason="Beat result exceeds publication size limit", predictions=None
    )
    envelope.update(schema_version=1, beat=asdict(failed))
    encoded = json.dumps(envelope, allow_nan=False, separators=(",", ":"))
    if len(encoded.encode("utf-8")) > MAX_ENVELOPE_BYTES:
        msg = "offline result limit exceeded"
        raise ValueError(msg)
    return encoded


def _expand_predictions(envelope: dict[str, object]) -> None:
    beat = envelope.get("beat")
    if not isinstance(beat, dict):
        msg = "missing beat component"
        raise TypeError(msg)
    predictions = beat.get("predictions")
    if beat.get("status") != "ready":
        if predictions is not None:
            msg = "unsuccessful_beat_component_has_predictions"
            raise ValueError(msg)
        return
    if not isinstance(predictions, dict) or set(predictions) != {"encoding", *_ARRAY_NAMES}:
        msg = "invalid packed prediction fields"
        raise ValueError(msg)
    if predictions["encoding"] != _ENCODING:
        msg = "unknown packed prediction encoding"
        raise ValueError(msg)
    beat["predictions"] = {name: _unpack(predictions[name]) for name in _ARRAY_NAMES}


def decode_result(
    encoded: str, request: BeatWorkerRequest, limits: WorkerLimits = _DEFAULT_LIMITS
) -> PublishedAnalysisResult:
    """Read/validate either envelope version without PCM/model files or inference.

    The retained request supplies source extent and expected identity/provenance.
    Restoring project analysis remains separately gated; this is a diagnostic reader.
    """
    if len(encoded.encode("utf-8")) > MAX_ENVELOPE_BYTES:
        msg = "offline result limit exceeded"
        raise ValueError(msg)
    envelope: object = json.loads(encoded)
    if not isinstance(envelope, dict):
        msg = "offline result must be an object"
        raise TypeError(msg)
    if envelope.get("schema_version") == 2:
        _expand_predictions(envelope)
    result = _RESULT_ADAPTER.validate_json(json.dumps(envelope, allow_nan=False), strict=True)
    if result.identity != request.identity:
        msg = "offline result identity mismatch"
        raise ValueError(msg)
    validate_component_result(result.beat, request, limits)
    if not result.beat.resources_released:
        msg = "offline beat resources are still retiring"
        raise ValueError(msg)
    if (
        result.key.get("status") not in {"ready", "unavailable", "failed", "cancelled"}
        or not isinstance(result.key.get("key"), str)
        or len(json.dumps(result.key).encode("utf-8")) > 16_384
    ):
        msg = "invalid independent key component"
        raise ValueError(msg)
    return result
