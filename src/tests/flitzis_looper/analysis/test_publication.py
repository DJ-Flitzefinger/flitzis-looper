import base64
import json
import math
import struct
from dataclasses import asdict, replace
from random import Random
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatComponentResult,
    BeatModelIdentity,
    BeatPredictions,
    BeatWorkerRequest,
    MonoPcmInput,
    WorkerLimits,
    encode_request,
)
from flitzis_looper.analysis.publication import MAX_ENVELOPE_BYTES, decode_result, encode_result

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.analysis.contracts import ComponentStatus


_ARRAY_NAMES = ("beat_seconds", "downbeat_seconds", "beat_logits", "downbeat_logits")
_KEY: dict[str, object] = {
    "status": "ready",
    "key": "G#m",
    "detail": "",
    "provenance": "KeyNet/native",
}


@pytest.fixture
def worker_request(tmp_path: Path) -> BeatWorkerRequest:
    pcm = tmp_path / "mono.f32le"
    pcm.write_bytes(bytes(128 * 4))
    return BeatWorkerRequest(
        identity=AnalysisIdentity(3, 42, "source-fixture", 9),
        pcm=MonoPcmInput(pcm, 48_000, 128),
        model=BeatModelIdentity(
            sha256="a" * 64,
            frontend_id="fixture-reference-frontend",
            environment_id="fixture-locked-cpu-environment",
        ),
    )


def _ready(worker_request: BeatWorkerRequest) -> BeatComponentResult:
    return BeatComponentResult(
        identity=worker_request.identity,
        model=worker_request.model,
        status="ready",
        reason="",
        predictions=BeatPredictions(
            beat_seconds=(-0.0, 1 / 48_000, 127 / 48_000),
            downbeat_seconds=(0.0,),
            beat_logits=(-0.0, math.ulp(0.0), -math.ulp(0.0), 0.12345678901234568),
            downbeat_logits=(1.0000000000000002, -1.0000000000000002, 2.5, -3.75),
        ),
    )


def _binary64(values: tuple[float, ...]) -> bytes:
    return struct.pack(f"<{len(values)}d", *values)


def _packed(values: tuple[float, ...]) -> str:
    return base64.b64encode(_binary64(values)).decode("ascii")


def _envelope(beat: BeatComponentResult) -> dict[str, object]:
    parsed: object = json.loads(encode_result(beat, _KEY))
    assert isinstance(parsed, dict)
    return parsed


def _object_field(envelope: dict[str, object], name: str) -> dict[str, object]:
    value = envelope[name]
    assert isinstance(value, dict)
    return value


def _predictions(envelope: dict[str, object]) -> dict[str, object]:
    return _object_field(_object_field(envelope, "beat"), "predictions")


def test_ready_v2_preserves_every_binary64_bit_and_provenance(
    worker_request: BeatWorkerRequest,
) -> None:
    random = Random(20261005)
    raw_bits = [0, 1 << 63, 1, (1 << 63) | 1, 0x7FEFFFFFFFFFFFFF]
    while len(raw_bits) < 256:
        candidate = random.getrandbits(64)
        if candidate & 0x7FF0000000000000 != 0x7FF0000000000000:
            raw_bits.append(candidate)
    values = struct.unpack("<256d", struct.pack("<256Q", *raw_bits))
    beat = _ready(worker_request)
    assert beat.predictions is not None
    beat = replace(
        beat,
        predictions=replace(beat.predictions, beat_logits=values, downbeat_logits=values[::-1]),
    )

    encoded = encode_result(beat, _KEY)
    wire = json.loads(encoded)
    assert wire["schema_version"] == 2
    assert wire["beat"]["predictions"]["encoding"] == "float64-le/base64"
    decoded = decode_result(encoded, worker_request)

    assert decoded.schema_version == 2
    assert decoded.identity == worker_request.identity
    assert decoded.beat.model == worker_request.model
    assert decoded.beat.status == "ready"
    assert decoded.beat.resources_released
    assert decoded.key == _KEY
    assert decoded.beat.predictions is not None
    assert beat.predictions is not None
    for name in _ARRAY_NAMES:
        expected = getattr(beat.predictions, name)
        assert _binary64(getattr(decoded.beat.predictions, name)) == _binary64(expected)
        assert wire["beat"]["predictions"][name] == _packed(expected)


def test_v2_reads_after_pcm_retirement_without_optional_installation(
    worker_request: BeatWorkerRequest, tmp_path: Path
) -> None:
    # worker_request validation needs the full PCM while publication reading needs only metadata.
    encode_request(worker_request, WorkerLimits())
    encoded = encode_result(_ready(worker_request), _KEY)
    worker_request.pcm.path.unlink()
    assert list(tmp_path.iterdir()) == []

    decoded = decode_result(encoded, worker_request)

    assert decoded.beat == _ready(worker_request)
    assert decoded.beat.model.sha256 == "a" * 64
    assert decoded.key == _KEY
    assert list(tmp_path.iterdir()) == []


def test_legacy_v1_numeric_predictions_remain_readable(worker_request: BeatWorkerRequest) -> None:
    beat = _ready(worker_request)
    legacy = {
        "schema_version": 1,
        "identity": asdict(worker_request.identity),
        "beat": asdict(beat),
        "key": _KEY,
    }

    decoded = decode_result(json.dumps(legacy, allow_nan=False), worker_request)

    assert decoded.schema_version == 1
    assert decoded.beat == beat
    assert decoded.key == _KEY
    assert decoded.beat.predictions is not None
    assert beat.predictions is not None
    for name in _ARRAY_NAMES:
        assert _binary64(getattr(decoded.beat.predictions, name)) == _binary64(
            getattr(beat.predictions, name)
        )


@pytest.mark.parametrize("status", ["unavailable", "failed", "cancelled"])
def test_unsuccessful_beat_v1_retains_independent_key(
    worker_request: BeatWorkerRequest, status: ComponentStatus
) -> None:
    beat = replace(
        _ready(worker_request), status=status, reason="fixture outcome", predictions=None
    )

    decoded = decode_result(encode_result(beat, _KEY), worker_request)

    assert decoded.schema_version == 1
    assert decoded.beat == beat
    assert decoded.key == _KEY


@pytest.mark.parametrize(
    "encoded",
    [
        "?AAAAAAA8D8=",  # Invalid alphabet.
        "AAAAAAAA8D8",  # Missing required padding.
        "AAAAAAAA8D8=\n",  # Whitespace is not canonical inline Base64.
        "AAAAAAAA8D9=",  # Same bytes as 1.0 with nonzero padding bits.
        "AAAAAAAA8D8==",  # Surplus padding is accepted by some decoders.
        "AAAAAAAA",  # Six bytes cannot form one binary64.
        123,
    ],
)
def test_malformed_or_noncanonical_binary64_is_rejected(
    worker_request: BeatWorkerRequest, encoded: object
) -> None:
    envelope = _envelope(_ready(worker_request))
    _predictions(envelope)["beat_logits"] = encoded

    with pytest.raises(ValueError, match=r"(?i)base64|padding|packed prediction"):
        decode_result(json.dumps(envelope), worker_request)


@pytest.mark.parametrize("value", [math.inf, -math.inf, math.nan])
def test_nonfinite_packed_values_are_rejected(
    worker_request: BeatWorkerRequest, value: float
) -> None:
    envelope = _envelope(_ready(worker_request))
    _predictions(envelope)["beat_logits"] = _packed((value,) * 4)

    with pytest.raises(ValueError, match=r"finite|compliant"):
        decode_result(json.dumps(envelope), worker_request)


@pytest.mark.parametrize(
    ("name", "values"),
    [
        ("beat_seconds", (-0.001,)),
        ("beat_seconds", (128 / 48_000,)),
        ("downbeat_seconds", (1.0,)),
        ("beat_seconds", (0.001, 0.0)),
        ("beat_seconds", (0.001, 0.001)),
        ("downbeat_logits", (0.5,)),
    ],
)
def test_invalid_source_positions_and_mismatched_logits_are_rejected(
    worker_request: BeatWorkerRequest, name: str, values: tuple[float, ...]
) -> None:
    envelope = _envelope(_ready(worker_request))
    _predictions(envelope)[name] = _packed(values)

    with pytest.raises(
        ValueError,
        match=r"prediction_outside_source|predictions_not_strictly_increasing|logit_count",
    ):
        decode_result(json.dumps(envelope), worker_request)


@pytest.mark.parametrize("field", ["pad_id", "request_id", "source_id", "source_generation"])
@pytest.mark.parametrize("component", ["identity", "beat"])
def test_stale_outer_or_component_identity_is_rejected(
    worker_request: BeatWorkerRequest, field: str, component: str
) -> None:
    envelope = _envelope(_ready(worker_request))
    identity = envelope[component]
    assert isinstance(identity, dict)
    if component == "beat":
        identity = identity["identity"]
        assert isinstance(identity, dict)
    identity[field] = "other-source" if field == "source_id" else 999

    with pytest.raises(ValueError, match=r"identity mismatch|identity_mismatch"):
        decode_result(json.dumps(envelope), worker_request)


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("sha256", "b" * 64),
        ("frontend_id", "other-frontend"),
        ("environment_id", "other-environment"),
        ("checkpoint", "small0"),
        ("precision", "float64"),
    ],
)
def test_altered_or_invalid_model_provenance_is_rejected(
    worker_request: BeatWorkerRequest, field: str, value: str
) -> None:
    envelope = _envelope(_ready(worker_request))
    _object_field(_object_field(envelope, "beat"), "model")[field] = value

    with pytest.raises(ValueError, match=r"identity_mismatch|validation error"):
        decode_result(json.dumps(envelope), worker_request)


@pytest.mark.parametrize("encoding", ["float32-le/base64", "float64-be/base64", "zlib/base64", ""])
def test_unknown_prediction_encoding_is_rejected(
    worker_request: BeatWorkerRequest, encoding: str
) -> None:
    envelope = _envelope(_ready(worker_request))
    _predictions(envelope)["encoding"] = encoding

    with pytest.raises(ValueError, match="unknown packed prediction encoding"):
        decode_result(json.dumps(envelope), worker_request)


@pytest.mark.parametrize("component", ["outer", "beat", "model", "identity", "predictions"])
def test_unexpected_schema_fields_are_rejected(
    worker_request: BeatWorkerRequest, component: str
) -> None:
    envelope = _envelope(_ready(worker_request))
    targets = {
        "outer": envelope,
        "beat": _object_field(envelope, "beat"),
        "model": _object_field(_object_field(envelope, "beat"), "model"),
        "identity": _object_field(envelope, "identity"),
        "predictions": _predictions(envelope),
    }
    targets[component]["unexpected"] = True

    with pytest.raises(ValueError, match=r"Unexpected keyword|packed prediction fields"):
        decode_result(json.dumps(envelope), worker_request)


@pytest.mark.parametrize("version", [0, 3, True, "2", 2.0, None])
def test_unknown_or_noninteger_schema_version_is_rejected(
    worker_request: BeatWorkerRequest, version: object
) -> None:
    envelope = _envelope(_ready(worker_request))
    envelope["schema_version"] = version

    with pytest.raises(ValueError, match="schema_version"):
        decode_result(json.dumps(envelope), worker_request)


@pytest.mark.parametrize("invalid_part", ["missing", "retiring", "failed_with_predictions"])
def test_prediction_presence_and_resource_retirement_are_validated(
    worker_request: BeatWorkerRequest, invalid_part: str
) -> None:
    envelope = _envelope(_ready(worker_request))
    beat = envelope["beat"]
    assert isinstance(beat, dict)
    if invalid_part == "missing":
        beat["predictions"] = None
    elif invalid_part == "retiring":
        beat["resources_released"] = False
    else:
        beat["status"] = "failed"

    with pytest.raises(ValueError, match=r"prediction|resources are still retiring"):
        decode_result(json.dumps(envelope), worker_request)


@pytest.mark.parametrize(
    "limits", [WorkerLimits(max_prediction_count=3), WorkerLimits(max_response_bytes=128)]
)
def test_versioned_reader_preserves_original_worker_limits(
    worker_request: BeatWorkerRequest, limits: WorkerLimits
) -> None:
    with pytest.raises(ValueError, match=r"logit_limit|beat_component_limit"):
        decode_result(encode_result(_ready(worker_request), _KEY), worker_request, limits)


def test_packer_preserves_the_250000_value_bound(worker_request: BeatWorkerRequest) -> None:
    beat = _ready(worker_request)
    assert beat.predictions is not None
    beat = replace(beat, predictions=replace(beat.predictions, beat_logits=(0.0,) * 250_001))

    with pytest.raises(ValueError, match="prediction limit"):
        encode_result(beat, _KEY)


@pytest.mark.parametrize("invalid_status", ["ready_without_predictions", "failed_with_predictions"])
def test_packer_rejects_status_prediction_mismatch(
    worker_request: BeatWorkerRequest, invalid_status: str
) -> None:
    beat = _ready(worker_request)
    if invalid_status == "ready_without_predictions":
        beat = replace(beat, predictions=None)
    else:
        beat = replace(beat, status="failed")

    with pytest.raises(ValueError, match="prediction"):
        encode_result(beat, _KEY)


def test_still_oversize_binary64_fails_beat_without_discarding_key(
    worker_request: BeatWorkerRequest,
) -> None:
    beat = _ready(worker_request)
    assert beat.predictions is not None
    beat = replace(
        beat,
        predictions=replace(
            beat.predictions,
            beat_logits=(0.12345678901234568,) * 50_000,
            downbeat_logits=(-0.9876543210987654,) * 50_000,
        ),
    )

    encoded = encode_result(beat, _KEY)
    decoded = decode_result(encoded, worker_request)

    assert len(encoded.encode("utf-8")) <= MAX_ENVELOPE_BYTES
    assert decoded.beat.status == "failed"
    assert "publication size limit" in decoded.beat.reason
    assert decoded.beat.predictions is None
    assert decoded.beat.identity == worker_request.identity
    assert decoded.beat.model == worker_request.model
    assert decoded.key == _KEY


def test_final_envelope_cap_is_enforced_before_reading(worker_request: BeatWorkerRequest) -> None:
    encoded = encode_result(_ready(worker_request), _KEY)

    with pytest.raises(ValueError, match="offline result limit"):
        decode_result(encoded + " " * MAX_ENVELOPE_BYTES, worker_request)


@pytest.mark.parametrize(
    "key", [{"status": "unexpected", "key": "C"}, {"status": "ready", "key": 1}]
)
def test_invalid_independent_key_component_is_rejected(
    worker_request: BeatWorkerRequest, key: dict[str, object]
) -> None:
    with pytest.raises(ValueError, match="invalid independent key component"):
        decode_result(encode_result(_ready(worker_request), key), worker_request)
