"""Pure content/configuration identity; no model or source file hashing."""

from dataclasses import replace
from pathlib import Path

import pytest

from flitzis_looper.controller.stem_generation import AudioShape, StemGenerationRequest
from flitzis_looper.controller.stem_identity import request_fingerprint, request_signature


def _request() -> StemGenerationRequest:
    return StemGenerationRequest(
        sample_id=0,
        source_path=Path("source-a.wav"),
        source_version=f"source-a.wav|sha256-v1:{'1' * 64}",
        cache_dir=Path("private-generation"),
        target_shape=AudioShape(48_000, 2, 512),
        model_cache_dir=Path("model-a"),
        original_bytes=2092,
    )


def test_equal_content_and_producer_match_across_all_storage_locators() -> None:
    first = _request()
    second = replace(
        first,
        sample_id=215,
        source_path=Path("source-b.wav"),
        source_version=f"source-b.wav|sha256-v1:{'1' * 64}",
        cache_dir=Path("another-generation"),
        model_cache_dir=Path("another-model-copy"),
    )

    assert request_signature(first) == request_signature(second)
    assert request_fingerprint(first, "2" * 64) == request_fingerprint(second, "2" * 64)
    assert request_fingerprint(first, "2" * 64) is not None


def test_only_active_separator_quality_changes_effective_configuration() -> None:
    demucs = _request()
    quality = replace(demucs, demucs_shifts=4, demucs_overlap=0.25)
    assert request_signature(demucs) != request_signature(quality)
    assert request_fingerprint(demucs, "2" * 64) != request_fingerprint(quality, "2" * 64)

    roformer = replace(demucs, separator="bs-roformer:musdb18hq")
    inactive_quality = replace(roformer, demucs_shifts=4, demucs_overlap=0.25)
    assert request_signature(roformer) == request_signature(inactive_quality)
    assert request_fingerprint(roformer, "2" * 64) == request_fingerprint(
        inactive_quality, "2" * 64
    )


@pytest.mark.parametrize(
    ("source_version", "original_bytes", "producer"),
    [
        ("source-a.wav|10|20", 2092, "2" * 64),
        (f"source-a.wav|sha256-v1:{'1' * 64}\n", 2092, "2" * 64),
        (f"source-a.wav|sha256-v1:{'1' * 64}", None, "2" * 64),
        (f"source-a.wav|sha256-v1:{'1' * 64}", 0, "2" * 64),
        (f"source-a.wav|sha256-v1:{'1' * 64}", 2092, None),
        (f"source-a.wav|sha256-v1:{'1' * 64}", 2092, "unknown"),
    ],
)
def test_incomplete_source_extent_or_producer_cannot_become_current_fingerprint(
    source_version: str, original_bytes: int | None, producer: str | None
) -> None:
    request = replace(_request(), source_version=source_version, original_bytes=original_bytes)
    assert request_fingerprint(request, producer) is None


def test_resolved_source_extent_changes_full_key_without_changing_captured_intent() -> None:
    first = _request()
    unresolved = replace(first, original_bytes=None)
    different_extent = replace(first, original_bytes=2094)

    assert request_signature(first) == request_signature(unresolved)
    assert request_signature(first) == request_signature(different_extent)
    assert request_fingerprint(first, "2" * 64) != request_fingerprint(different_extent, "2" * 64)
    assert request_fingerprint(first) is None


def test_full_shape_device_source_and_producer_changes_cannot_share_full_key() -> None:
    request = _request()
    fingerprint = request_fingerprint(request, "2" * 64)
    changed = (
        replace(request, target_shape=AudioShape(44_100, 2, 512)),
        replace(request, target_shape=AudioShape(48_000, 1, 512)),
        replace(request, target_shape=AudioShape(48_000, 2, 513)),
        replace(request, device_policy="cpu"),
        replace(request, source_version=f"source-a.wav|sha256-v1:{'3' * 64}"),
    )
    for candidate in changed:
        assert request_fingerprint(candidate, "2" * 64) != fingerprint
    assert request_fingerprint(request, "3" * 64) != fingerprint
