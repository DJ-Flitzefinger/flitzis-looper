"""Monotone admission and producer-resolution races without inference or file I/O."""

from dataclasses import replace
from pathlib import Path

import pytest

from flitzis_looper.controller.stem_generation import AudioShape, StemGenerationRequest
from flitzis_looper.controller.stem_intent import StemIntentRegistry


def _request() -> StemGenerationRequest:
    return StemGenerationRequest(
        sample_id=0,
        source_path=Path("source.wav"),
        source_version=f"source.wav|sha256-v1:{'1' * 64}",
        cache_dir=Path("private-generation"),
        target_shape=AudioShape(48_000, 2, 512),
        model_cache_dir=Path("model"),
    )


def test_equal_admissions_keep_independent_sequences_and_one_material_revision() -> None:
    registry = StemIntentRegistry()
    request = _request()
    first = registry.capture(request, 0)
    second = registry.capture(replace(request, sample_id=215, model_cache_dir=Path("copy")), 0)

    assert first.material_key == "1" * 64
    assert first.material_revision == second.material_revision
    assert first.admission_sequence < second.admission_sequence
    assert registry.bind(first, "2" * 64) == first
    assert registry.bind(second, "2" * 64) == second
    assert registry.is_current(first)
    assert registry.is_current(second)


def test_configuration_and_settings_aba_fence_old_admissions() -> None:
    registry = StemIntentRegistry()
    request = _request()
    first = registry.capture(request, 0)
    changed = registry.capture(replace(request, demucs_shifts=4), 0)
    returned = registry.capture(request, 0)
    assert first.material_revision < changed.material_revision < returned.material_revision
    assert not registry.is_current(first)
    assert registry.bind(first, "2" * 64) is None

    settings_changed = registry.capture(request, 1)
    settings_returned = registry.capture(request, 2)
    assert settings_changed.material_revision < settings_returned.material_revision
    assert not registry.is_current(returned)
    assert registry.bind(returned, "2" * 64) is None
    assert registry.is_current(settings_returned)


def test_inactive_demucs_quality_keeps_roformer_material_intent() -> None:
    registry = StemIntentRegistry()
    request = replace(_request(), separator="bs-roformer:musdb18hq")
    first = registry.capture(request, 0)
    second = registry.capture(replace(request, demucs_shifts=4, demucs_overlap=0.25), 0)

    assert first.material_revision == second.material_revision
    assert registry.is_current(first)
    assert registry.is_current(second)


def test_earlier_late_different_producer_cannot_overwrite_newer_first_resolution() -> None:
    registry = StemIntentRegistry()
    older = registry.capture(_request(), 0)
    equal_earlier = registry.capture(_request(), 0)
    newer = registry.capture(_request(), 0)
    assert registry.bind(newer, "3" * 64) == newer

    assert registry.bind(older, "2" * 64) is None
    assert not registry.is_current(older)
    assert registry.is_current(newer)
    assert registry.bind(older, "3" * 64) is None
    assert registry.bind(equal_earlier, "3" * 64) == equal_earlier
    assert registry.is_current(equal_earlier)


def test_newer_different_producer_advances_once_and_old_aba_never_revives() -> None:
    registry = StemIntentRegistry()
    first = registry.capture(_request(), 0)
    assert registry.bind(first, "2" * 64) == first
    second = registry.capture(_request(), 0)
    bound_second = registry.bind(second, "3" * 64)
    assert bound_second is not None
    assert bound_second.material_revision > first.material_revision
    assert registry.bind(bound_second, "3" * 64) == bound_second
    assert not registry.is_current(first)

    third = registry.capture(_request(), 0)
    bound_third = registry.bind(third, "2" * 64)
    assert bound_third is not None
    assert bound_third.material_revision > bound_second.material_revision
    assert registry.bind(first, "2" * 64) is None
    assert registry.bind(bound_second, "3" * 64) is None
    assert registry.is_current(bound_third)


@pytest.mark.parametrize("third_key", ["3" * 64, "4" * 64])
def test_newer_unbound_admission_can_resolve_after_an_intermediate_producer_replacement(
    third_key: str,
) -> None:
    registry = StemIntentRegistry()
    first = registry.capture(_request(), 0)
    assert registry.bind(first, "2" * 64) == first
    second = registry.capture(_request(), 0)
    third = registry.capture(_request(), 0)
    bound_second = registry.bind(second, "3" * 64)
    assert bound_second is not None
    bound_third = registry.bind(third, third_key)
    assert bound_third is not None
    assert registry.is_current(bound_third)
    if third_key == "3" * 64:
        assert bound_third.material_revision == bound_second.material_revision
        assert registry.is_current(bound_second)
    else:
        assert bound_third.material_revision > bound_second.material_revision
        assert not registry.is_current(bound_second)


def test_global_settings_revision_and_terminal_cleanup_preserve_monotone_fences() -> None:
    registry = StemIntentRegistry()
    first = registry.capture(_request(), 0)
    different_material = replace(_request(), source_version=f"other.wav|sha256-v1:{'4' * 64}")
    registry.capture(different_material, 1)
    assert not registry.is_current(first)
    with pytest.raises(ValueError, match="Settings revision"):
        registry.capture(_request(), 0)
    with pytest.raises(RuntimeError, match="admitted interests"):
        registry.forget_material(first.material_key)
    registry.release(first)
    registry.forget_material(first.material_key)
    assert not registry.is_current(first)
    returned = registry.capture(_request(), 1)
    assert returned.material_revision > first.material_revision


def test_unknown_legacy_source_metadata_cannot_capture_verified_material_intent() -> None:
    registry = StemIntentRegistry()
    with pytest.raises(ValueError, match="complete source digest"):
        registry.capture(replace(_request(), source_version="source.wav|10|20"), 0)


def test_pruned_admissions_and_materials_cannot_revive_when_equal_values_return() -> None:
    registry = StemIntentRegistry()
    first = registry.capture(_request(), 0)
    assert registry.bind(first, "2" * 64) == first
    second = registry.capture(_request(), 0)
    bound_second = registry.bind(second, "3" * 64)
    assert bound_second is not None

    registry.prune([bound_second])
    assert registry.is_current(bound_second)
    assert registry.bind(first, "2" * 64) is None
    registry.prune([])
    assert not registry.is_current(bound_second)
    returned = registry.capture(_request(), 0)
    assert returned.material_revision > bound_second.material_revision
    assert returned.admission_sequence > bound_second.admission_sequence
    assert registry.bind(first, "2" * 64) is None
    assert registry.bind(bound_second, "3" * 64) is None
    assert registry.bind(returned, "2" * 64) == returned
