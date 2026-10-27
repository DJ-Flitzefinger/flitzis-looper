"""Independent bounded receipt retention shares the existing owned output reader."""

import io

import pytest

from flitzis_looper.analysis import process


def read(content: bytes, *, independent: bool) -> bytes:
    reader = process._CommandReader(
        io.BytesIO(content), 32, (b"INPUT ", b"EXECUTION "), independent_receipts=independent
    )
    reader._read()
    return reader.captured()


def test_each_distinct_receipt_survives_bounded_diagnostic_tail() -> None:
    captured = read(b"INPUT source\nEXECUTION cuda:0\n" + b"diagnostic\n" * 100, independent=True)
    assert captured.splitlines().count(b"INPUT source") == 1
    assert captured.splitlines().count(b"EXECUTION cuda:0") == 1
    assert len(captured) < 100


@pytest.mark.parametrize(
    "invalid", [b"EXECUTION cuda:0\nEXECUTION cpu\n", b"EXECUTION " + b"x" * 1001 + b"\n"]
)
def test_one_invalid_prefix_does_not_destroy_the_other_receipt(invalid: bytes) -> None:
    captured = read(b"INPUT source\n" + invalid + b"tail\n" * 100, independent=True)
    assert captured.splitlines().count(b"INPUT source") == 1
    assert captured.splitlines().count(b"EXECUTION invalid receipt") == 2
    assert b"EXECUTION cuda:0" not in captured


def test_legacy_default_still_rejects_multiple_receipts_as_one_contract() -> None:
    captured = read(b"INPUT source\nEXECUTION cuda:0\n", independent=False)
    assert captured.splitlines().count(b"INPUT invalid receipt") == 2
    assert b"INPUT source" not in captured
