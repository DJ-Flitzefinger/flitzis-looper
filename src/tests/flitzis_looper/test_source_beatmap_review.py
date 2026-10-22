"""Explicit compact review intent, without a second map/source authority."""

import json
from typing import cast

import pytest

from flitzis_looper.source_beatmap_review import (
    ReviewWindowMode,
    ReviewWindowRow,
    review_window_operation,
)


def test_nonordinal_raw_and_corrected_existing_references_have_only_explicit_labels() -> None:
    operation = review_window_operation(
        "a" * 64,
        "b" * 64,
        "replace_interior",
        0,
        4,
        (ReviewWindowRow("existing", 1, -0.0), ReviewWindowRow("raw", 42, 0.1)),
    )
    assert operation == {
        "schema_version": 1,
        "operation": "review_raw_window",
        "map_revision": "a" * 64,
        "capture_revision": "b" * 64,
        "mode": "replace_interior",
        "first_anchor_index": 0,
        "last_anchor_index": 4,
        "rows": [
            {"kind": "existing", "anchor_index": 1, "quarter_bits": "8000000000000000"},
            {"kind": "raw", "raw_index": 42, "quarter_bits": "3fb999999999999a"},
        ],
    }
    assert ReviewWindowRow("raw", 42, -0.0).identity() != ReviewWindowRow("raw", 42, 0.0).identity()


@pytest.mark.parametrize("mode", ["extend_start", "extend_end"])
def test_maximum_extension_intent_remains_within_eight_kib(mode: ReviewWindowMode) -> None:
    rows = tuple(ReviewWindowRow("raw", 200 + index * 3, float(index)) for index in range(63))
    operation = review_window_operation("a" * 64, "b" * 64, mode, 63, 63, rows)
    assert len(json.dumps(operation, separators=(",", ":")).encode()) <= 8192
    assert len(cast("list[object]", operation["rows"])) == 63


def test_empty_replacement_explicitly_requests_deletion_only_between_fixed_endpoints() -> None:
    operation = review_window_operation("a" * 64, "b" * 64, "replace_interior", 4, 8, ())
    assert operation["rows"] == []
    assert operation["first_anchor_index"] == 4
    assert operation["last_anchor_index"] == 8


@pytest.mark.parametrize("index", [True, False, -1, 250_000, 1.0, "1"])
def test_invalid_or_boolean_indices_never_form_intent(index: object) -> None:
    with pytest.raises(ValueError, match="indices"):
        review_window_operation(
            "a" * 64,
            "b" * 64,
            "extend_end",
            1,
            1,
            (ReviewWindowRow("raw", cast("int", index), 2.0),),
        )


@pytest.mark.parametrize("quarter", [float("nan"), float("inf"), -float("inf"), 1, True])
def test_nonfinite_or_nonfloat_quarters_never_form_intent(quarter: object) -> None:
    with pytest.raises(ValueError, match="finite quarter"):
        review_window_operation(
            "a" * 64,
            "b" * 64,
            "extend_end",
            1,
            1,
            (ReviewWindowRow("raw", 2, cast("float", quarter)),),
        )


@pytest.mark.parametrize(
    ("mode", "first", "last", "rows"),
    [
        ("extend_end", 1, 2, (ReviewWindowRow("raw", 4, 3.0),)),
        ("extend_end", 1, 1, ()),
        ("extend_start", 0, 0, (ReviewWindowRow("existing", 1, 3.0),)),
        ("replace_interior", 0, 64, ()),
        ("replace_interior", 0, 3, (ReviewWindowRow("existing", 0, 1.0),)),
        ("replace_interior", 0, 3, (ReviewWindowRow("existing", 3, 1.0),)),
        (
            "replace_interior",
            0,
            3,
            (ReviewWindowRow("existing", 1, 1.0), ReviewWindowRow("existing", 1, 2.0)),
        ),
        (
            "extend_end",
            1,
            1,
            (ReviewWindowRow("raw", 4, 3.0), ReviewWindowRow("raw", 5, 2.0)),
        ),
    ],
)
def test_conflicting_window_shapes_and_labels_are_refused(
    mode: ReviewWindowMode, first: int, last: int, rows: tuple[ReviewWindowRow, ...]
) -> None:
    with pytest.raises(ValueError, match=r"Extension|Replacement|Existing|Rows"):
        review_window_operation("a" * 64, "b" * 64, mode, first, last, rows)


@pytest.mark.parametrize("revision", ["", "A" * 64, "g" * 64, "a" * 63, "a" * 65])
def test_exact_revision_is_required(revision: str) -> None:
    with pytest.raises(ValueError, match="revision"):
        review_window_operation(
            "a" * 64, revision, "extend_end", 1, 1, (ReviewWindowRow("raw", 2, 2.0),)
        )
