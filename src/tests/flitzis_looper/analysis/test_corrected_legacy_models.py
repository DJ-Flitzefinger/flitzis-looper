"""Independent complete-array and fail-closed corrected-QM result regressions."""

import base64
import copy
import json
import math
import struct
from itertools import pairwise
from typing import cast

import pytest

from flitzis_looper.analysis.corrected_legacy_models import decode_corrected_legacy


def packed(values: tuple[float, ...] | tuple[int, ...], code: str) -> str:
    """Build canonical fixture bytes independently of the production packer."""
    return base64.b64encode(struct.pack(f"<{len(values)}{code}", *values)).decode("ascii")


def legacy_fixture(
    frames: tuple[float, ...] = (0.0, 43.00000000000001, 86.25, 129.75, 173.0),
    indices: tuple[int, ...] = (0, 2, 4),
    duration_seconds: int = 10,
) -> dict[str, object]:
    """Construct a full synthetic native result, without a producer approval claim."""
    seconds = tuple(value * (512 / 44100) for value in frames)
    downbeats = tuple(seconds[index] for index in indices)
    total = 0.0
    for first, last in pairwise(frames):
        total += last - first
    bpm = 0.0 if len(frames) < 2 else 60 / (total / (len(frames) - 1) * (512 / 44100))
    return {
        "schema_version": 1,
        "backend": "corrected-qm-native-v1",
        "diagnostic_only": True,
        "identity": {
            "pad_id": 0,
            "request_id": 2,
            "source_id": "loaded-0-1",
            "source_generation": 1,
        },
        "loaded": {
            "sample_rate_hz": 96000,
            "frame_count": duration_seconds * 96000,
            "origin_seconds": 0.0,
            "mono_sha256": "a" * 64,
            "mono_rule": "arithmetic-channel-mean-f64-v1",
        },
        "analyzer": {
            "sample_rate_hz": 44100,
            "frame_count": duration_seconds * 44100,
            "sha256": "b" * 64,
            "odf_hop_samples": 512,
            "transform_revision": "rubato-fft-1.0-44100-delay-trim-tail-flush-v1",
        },
        "configuration": {
            "step_secs": 0.01161,
            "max_bin_hz": 50.0,
            "input_tempo": 120.0,
            "alpha": 0.9,
            "tightness": 4.0,
            "viterbi_sigma": 8.0,
            "window_length": 512,
            "hop_size": 128,
        },
        "raw": {
            "encoding": "float64-le/base64+uint64-le/base64",
            "beat_frames": packed(frames, "d"),
            "downbeat_raw_indices": packed(indices, "Q"),
            "beat_seconds": packed(seconds, "d"),
            "downbeat_seconds": packed(downbeats, "d"),
        },
        "compatibility": {
            "encoding": "float32-le/base64",
            "bpm": packed((bpm,), "f"),
            "beats": packed(seconds, "f"),
            "downbeats": packed(downbeats, "f"),
            "bars": packed(downbeats, "f"),
        },
    }


def object_fields(value: object) -> dict[str, object]:
    """Assert nested fixture/report shape before using its dynamic metadata fields."""
    assert isinstance(value, dict)
    assert all(isinstance(key, str) for key in value)
    return cast("dict[str, object]", value)


def encoded(value: dict[str, object]) -> bytes:
    """Serialize fixture metadata without allowing nonfinite JSON numbers."""
    return json.dumps(value, allow_nan=False, separators=(",", ":")).encode()


def test_complete_binary64_frames_and_seconds_survive_separate_binary32_projection() -> None:
    fixture = legacy_fixture()
    original = encoded(fixture)
    result = decode_corrected_legacy(original)
    assert result.beat_frames == (0.0, 43.00000000000001, 86.25, 129.75, 173.0)
    assert result.downbeat_raw_indices == (0, 2, 4)
    expected = tuple(frame * (512 / 44100) for frame in result.beat_frames)
    assert struct.pack("<5d", *result.beat_seconds) == struct.pack("<5d", *expected)
    assert result.beat_seconds[1] != result.compatibility_beats[1]
    assert result.downbeat_seconds == tuple(expected[i] for i in (0, 2, 4))
    assert result.compatibility_bars == result.compatibility_downbeats
    predictions = result.timing_predictions()
    assert predictions.beat_seconds == result.beat_seconds
    assert predictions.downbeat_seconds == result.downbeat_seconds
    assert predictions.beat_logits == predictions.downbeat_logits == ()
    assert encoded(fixture) == original


@pytest.mark.parametrize(("frames", "indices"), [((), ()), ((0.0,), (0,))])
def test_empty_or_single_result_preserves_zero_compatibility_bpm(
    frames: tuple[float, ...], indices: tuple[int, ...]
) -> None:
    result = decode_corrected_legacy(encoded(legacy_fixture(frames, indices)))
    assert result.beat_frames == frames
    assert result.compatibility_bpm == 0.0
    assert len(result.beat_seconds) == len(frames)


@pytest.mark.parametrize(
    ("section", "name", "value"),
    [
        (None, "schema_version", True),
        (None, "diagnostic_only", 1),
        (None, "diagnostic_only", False),
        ("loaded", "sample_rate_hz", 96000.0),
        ("loaded", "frame_count", True),
        ("loaded", "origin_seconds", False),
        ("analyzer", "sample_rate_hz", 44100.0),
        ("analyzer", "odf_hop_samples", 512.0),
        ("configuration", "window_length", 512.0),
        ("configuration", "hop_size", 128.0),
        ("identity", "request_id", True),
    ],
)
def test_scalar_metadata_rejects_boolean_and_numeric_coercion(
    section: str | None, name: str, value: object
) -> None:
    fixture = legacy_fixture()
    target = fixture if section is None else object_fields(fixture[section])
    target[name] = value
    with pytest.raises(ValueError, match=r"legacy|validation error"):
        decode_corrected_legacy(encoded(fixture))


@pytest.mark.parametrize(
    ("name", "integer"),
    [("max_bin_hz", 50), ("input_tempo", 120), ("tightness", 4), ("viterbi_sigma", 8)],
)
def test_configuration_integer_tokens_cannot_impersonate_native_binary64_fields(
    name: str, integer: int
) -> None:
    fixture = legacy_fixture()
    configuration = object_fields(fixture["configuration"])
    assert configuration[name] == float(integer)
    decode_corrected_legacy(encoded(fixture))
    configuration[name] = integer
    with pytest.raises(ValueError, match="legacy"):
        decode_corrected_legacy(encoded(fixture))


@pytest.mark.parametrize(
    "name", ["step_secs", "max_bin_hz", "input_tempo", "alpha", "tightness", "viterbi_sigma"]
)
def test_all_native_binary64_configuration_fields_reject_boolean_tokens(name: str) -> None:
    fixture = legacy_fixture()
    assert type(object_fields(fixture["configuration"])[name]) is float
    decode_corrected_legacy(encoded(fixture))
    object_fields(fixture["configuration"])[name] = True
    with pytest.raises(ValueError, match="legacy"):
        decode_corrected_legacy(encoded(fixture))


@pytest.mark.parametrize(
    "section", [None, "loaded", "analyzer", "configuration", "raw", "compatibility"]
)
def test_extra_fields_cannot_turn_a_result_into_an_unchecked_receipt(section: str | None) -> None:
    fixture = legacy_fixture()
    (fixture if section is None else object_fields(fixture[section]))["producer_approved"] = True
    with pytest.raises(ValueError, match="extra_forbidden"):
        decode_corrected_legacy(encoded(fixture))


def test_duplicate_field_is_rejected_before_envelope_validation() -> None:
    raw = encoded(legacy_fixture()).replace(
        b'"schema_version":1,', b'"schema_version":1,"schema_version":1,', 1
    )
    with pytest.raises(ValueError, match="duplicate"):
        decode_corrected_legacy(raw)


@pytest.mark.parametrize(
    ("section", "name"),
    [
        ("raw", "beat_frames"),
        ("raw", "beat_seconds"),
        ("raw", "downbeat_seconds"),
        ("raw", "downbeat_raw_indices"),
        ("compatibility", "bpm"),
        ("compatibility", "bars"),
    ],
)
@pytest.mark.parametrize("bad", ["!", "AQ==", "AAAAAAAAAAA=="])
def test_packed_arrays_reject_malformed_partial_and_noncanonical_bytes(
    section: str, name: str, bad: str
) -> None:
    fixture = legacy_fixture()
    object_fields(fixture[section])[name] = bad
    with pytest.raises(ValueError, match=r"base64|packed|legacy|padding"):
        decode_corrected_legacy(encoded(fixture))


@pytest.mark.parametrize(
    ("section", "name", "code"),
    [
        ("raw", "beat_frames", "d"),
        ("raw", "beat_seconds", "d"),
        ("raw", "downbeat_seconds", "d"),
        ("compatibility", "bpm", "f"),
        ("compatibility", "beats", "f"),
        ("compatibility", "downbeats", "f"),
        ("compatibility", "bars", "f"),
    ],
)
@pytest.mark.parametrize("bad", [math.inf, -math.inf, math.nan])
def test_no_nonfinite_packed_value_can_escape_json_validation(
    section: str, name: str, code: str, bad: float
) -> None:
    fixture = legacy_fixture()
    object_fields(fixture[section])[name] = packed((bad,), code)
    with pytest.raises(ValueError, match="legacy"):
        decode_corrected_legacy(encoded(fixture))


@pytest.mark.parametrize(
    ("section", "name", "value"),
    [
        ("analyzer", "frame_count", 441001),
        ("analyzer", "odf_hop_samples", 511),
        ("loaded", "origin_seconds", 0.01),
        ("configuration", "step_secs", 0.0116),
        ("raw", "encoding", "float32-le/base64"),
    ],
)
def test_dimensions_and_timebase_cannot_be_relabelled(
    section: str, name: str, value: object
) -> None:
    fixture = legacy_fixture()
    object_fields(fixture[section])[name] = value
    with pytest.raises(ValueError, match=r"legacy|validation error"):
        decode_corrected_legacy(encoded(fixture))


@pytest.mark.parametrize("indices", [(0, 2, 5), (0, 2, 2), (4, 2, 0), (2**64 - 1,)])
def test_all_downbeat_raw_associations_are_checked(indices: tuple[int, ...]) -> None:
    fixture = legacy_fixture()
    object_fields(fixture["raw"])["downbeat_raw_indices"] = packed(indices, "Q")
    with pytest.raises(ValueError, match="legacy"):
        decode_corrected_legacy(encoded(fixture))


@pytest.mark.parametrize(
    ("field", "values", "code"),
    [
        ("beats", (0.0,), "f"),
        ("downbeats", (0.0,), "f"),
        ("bars", (0.0,), "f"),
        ("bpm", (120.0, 120.0), "f"),
        ("bpm", (-1.0,), "f"),
        ("bpm", (120.0,), "f"),
    ],
)
def test_projection_lengths_values_and_bpm_are_bound_to_same_capture(
    field: str, values: tuple[float, ...], code: str
) -> None:
    fixture = legacy_fixture()
    object_fields(fixture["compatibility"])[field] = packed(values, code)
    with pytest.raises(ValueError, match="legacy"):
        decode_corrected_legacy(encoded(fixture))


def test_late_raw_seconds_corruption_rejects_without_a_prefix_shortcut() -> None:
    frames = tuple(float(i * 43) for i in range(1800))
    fixture = legacy_fixture(frames, tuple(range(0, 1800, 4)), 1000)
    full = decode_corrected_legacy(encoded(fixture))
    assert len(full.beat_seconds) == 1800
    assert len(full.downbeat_seconds) == 450
    altered = list(full.beat_seconds)
    altered[-1] = math.nextafter(altered[-1], math.inf)
    object_fields(fixture["raw"])["beat_seconds"] = packed(tuple(altered), "d")
    with pytest.raises(ValueError, match="integer-hop"):
        decode_corrected_legacy(encoded(fixture))


def test_no_oversize_publication_or_lossy_numeric_array_substitute() -> None:
    fixture = legacy_fixture()
    before = copy.deepcopy(fixture)
    object_fields(fixture["raw"])["beat_seconds"] = [0.0, 0.5]
    with pytest.raises(ValueError, match="string_type"):
        decode_corrected_legacy(encoded(fixture))
    with pytest.raises(ValueError, match="publication limit"):
        decode_corrected_legacy(encoded(before) + b" " * (1024 * 1024))
