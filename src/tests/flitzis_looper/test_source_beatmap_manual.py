"""Manual beat intent preserves explicit sample, revision and binary64 coordinates."""

import json
import struct
from typing import cast

import pytest

from flitzis_looper.source_beatmap_manual import ManualBeatMode, insert_manual_beat_operation


def _operation(**changes: object) -> dict[str, object]:
    values: dict[str, object] = {
        "map_revision": "a" * 64,
        "capture_revision": "b" * 64,
        "mode": "interior",
        "first_anchor_index": 2,
        "last_anchor_index": 3,
        "sample_index": 24_001,
        "sample_rate_hz": 48_000,
        "quarter": -0.0,
    }
    values.update(changes)
    return insert_manual_beat_operation(**values)  # type: ignore[arg-type]


def test_explicit_intent_has_exact_bits_and_no_inferred_source_or_raw_position() -> None:
    assert _operation() == {
        "schema_version": 1,
        "operation": "insert_manual_beat",
        "map_revision": "a" * 64,
        "capture_revision": "b" * 64,
        "mode": "interior",
        "first_anchor_index": 2,
        "last_anchor_index": 3,
        "sample_index": 24_001,
        "sample_rate_hz": 48_000,
        "quarter_bits": "8000000000000000",
    }
    assert _operation(quarter=0.0)["quarter_bits"] == "0000000000000000"
    assert _operation(quarter=1.0 / 3.0)["quarter_bits"] == struct.pack(">d", 1.0 / 3.0).hex()


@pytest.mark.parametrize(
    ("mode", "first", "last"),
    [("interior", 249_998, 249_999), ("extend_start", 0, 0), ("extend_end", 249_999, 249_999)],
)
def test_each_mode_is_small_and_only_native_resolves_actual_geometry(
    mode: ManualBeatMode, first: int, last: int
) -> None:
    operation = _operation(
        mode=mode,
        first_anchor_index=first,
        last_anchor_index=last,
        sample_index=(1 << 64) - 1,
        sample_rate_hz=(1 << 32) - 1,
        quarter=-1.7976931348623157e308,
    )
    assert len(json.dumps(operation, separators=(",", ":")).encode("utf-8")) < 8192
    assert operation["mode"] == mode


@pytest.mark.parametrize("field", ["map_revision", "capture_revision"])
@pytest.mark.parametrize("invalid", [None, True, "", "a" * 63, "a" * 65, "A" * 64, "g" * 64])
def test_both_exact_current_revisions_are_required(field: str, invalid: object) -> None:
    with pytest.raises(ValueError, match="revisions"):
        _operation(**{field: invalid})


@pytest.mark.parametrize("field", ["first_anchor_index", "last_anchor_index"])
@pytest.mark.parametrize("invalid", [True, False, -1, 250_000, 1.0, "1"])
def test_anchor_indices_reject_coercions_and_range_errors(field: str, invalid: object) -> None:
    with pytest.raises(ValueError, match="indices"):
        _operation(**{field: invalid})


@pytest.mark.parametrize("invalid", [True, False, -1, 1 << 64, 1.0, "1"])
def test_physical_sample_is_an_exact_nonnegative_u64(invalid: object) -> None:
    with pytest.raises(ValueError, match="Source sample"):
        _operation(sample_index=invalid)


@pytest.mark.parametrize("invalid", [True, False, -1, 0, 1 << 32, 48_000.0, "48000"])
def test_actual_rate_is_an_exact_positive_u32(invalid: object) -> None:
    with pytest.raises(ValueError, match="sample rate"):
        _operation(sample_rate_hz=invalid)


@pytest.mark.parametrize(
    "invalid", [True, False, 1, "1", float("nan"), float("inf"), -float("inf")]
)
def test_quarter_requires_finite_binary64_without_integer_or_bool_coercion(invalid: object) -> None:
    with pytest.raises(ValueError, match="finite quarter"):
        _operation(quarter=invalid)


@pytest.mark.parametrize(
    ("mode", "first", "last"),
    [("interior", 1, 1), ("interior", 1, 3), ("extend_start", 1, 1), ("extend_end", 1, 2)],
)
def test_fixed_endpoint_shapes_cannot_replace_or_span_several_intervals(
    mode: ManualBeatMode, first: int, last: int
) -> None:
    with pytest.raises(ValueError, match="endpoints"):
        _operation(mode=mode, first_anchor_index=first, last_anchor_index=last)


@pytest.mark.parametrize("invalid", [None, True, "replace_interior", "", ["interior"]])
def test_unknown_modes_do_not_form_intent(invalid: object) -> None:
    with pytest.raises(ValueError, match="Choose"):
        _operation(mode=cast("ManualBeatMode", invalid))
