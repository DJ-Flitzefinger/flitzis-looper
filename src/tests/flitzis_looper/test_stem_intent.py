"""Durable stem selection and pure preset transitions without native authority."""

import json
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.stem_intent import (
    STEM_COMPONENT_MASK,
    STEM_INSTRUMENTAL_PRESET_MASK,
    STEM_MASK_BASS,
    STEM_MASK_DRUMS,
    STEM_MASK_MELODY,
    STEM_MASK_VOCALS,
    PadStemIntent,
    StemMaskDisplayMode,
    transition_stem_intent,
)

if TYPE_CHECKING:
    from pathlib import Path


def test_missing_legacy_selection_defaults_to_all_components_and_display_all() -> None:
    intent = PadStemIntent.model_validate({})

    assert intent.model_dump() == {
        "schema_version": 1,
        "enabled_mask": STEM_COMPONENT_MASK,
        "last_custom_mask": STEM_COMPONENT_MASK,
        "display_mode": "all",
    }


@pytest.mark.parametrize(
    ("mask", "remembered", "display_mode"),
    [
        (0, 0, "custom"),
        (STEM_MASK_VOCALS | STEM_MASK_BASS, STEM_MASK_VOCALS | STEM_MASK_BASS, "custom"),
        (STEM_INSTRUMENTAL_PRESET_MASK, STEM_MASK_DRUMS, "instrumental"),
        (STEM_COMPONENT_MASK, STEM_MASK_MELODY, "all"),
    ],
)
def test_actual_json_reopen_preserves_selection_without_artifact_io(
    tmp_path: Path, mask: int, remembered: int, display_mode: StemMaskDisplayMode
) -> None:
    original = PadStemIntent(
        enabled_mask=mask, last_custom_mask=remembered, display_mode=display_mode
    )
    path = tmp_path / "selection.json"
    path.write_text(original.model_dump_json(), encoding="utf-8")

    reopened = PadStemIntent.model_validate_json(path.read_text(encoding="utf-8"))

    assert reopened == original
    assert reopened is not original
    assert reopened.model_dump_json() == original.model_dump_json()
    assert set(json.loads(path.read_text(encoding="utf-8"))) == {
        "schema_version",
        "enabled_mask",
        "last_custom_mask",
        "display_mode",
    }
    assert not (tmp_path / "samples").exists()


def test_copied_intents_and_input_payloads_have_independent_immutable_state() -> None:
    payload: dict[str, object] = {
        "enabled_mask": STEM_MASK_VOCALS,
        "last_custom_mask": STEM_MASK_VOCALS,
        "display_mode": "custom",
    }
    original = PadStemIntent.model_validate(payload)
    copied = PadStemIntent.model_validate(original)
    payload["enabled_mask"] = STEM_MASK_DRUMS

    changed = transition_stem_intent(copied, STEM_MASK_DRUMS)

    assert original == copied
    assert copied is not original
    assert original.enabled_mask == STEM_MASK_VOCALS
    assert original.last_custom_mask == STEM_MASK_VOCALS
    assert changed.enabled_mask == STEM_MASK_DRUMS
    assert changed.last_custom_mask == STEM_MASK_DRUMS
    assert changed is not copied
    assert not (
        set(PadStemIntent.model_fields)
        - {"schema_version", "enabled_mask", "last_custom_mask", "display_mode"}
    )


@pytest.mark.parametrize(
    "field", ["schema_version", "enabled_mask", "last_custom_mask", "display_mode"]
)
def test_saved_selection_fields_cannot_be_changed_or_deleted(field: str) -> None:
    intent = PadStemIntent()

    with pytest.raises(ValidationError, match="frozen"):
        setattr(intent, field, 0)
    with pytest.raises(ValidationError, match="frozen"):
        delattr(intent, field)


@pytest.mark.parametrize("mask", range(STEM_COMPONENT_MASK + 1))
def test_custom_selection_accepts_each_component_combination_and_remembers_it(mask: int) -> None:
    original = PadStemIntent(last_custom_mask=STEM_MASK_DRUMS)

    selected = transition_stem_intent(original, mask)

    assert selected.enabled_mask == mask
    assert selected.last_custom_mask == mask
    assert selected.display_mode == "custom"
    assert original.display_mode == "all"
    assert original.last_custom_mask == STEM_MASK_DRUMS


def test_presets_retain_custom_selection_until_it_is_restored() -> None:
    custom_mask = STEM_MASK_VOCALS | STEM_MASK_BASS
    custom = transition_stem_intent(PadStemIntent(), custom_mask)

    instrumental = transition_stem_intent(custom, STEM_INSTRUMENTAL_PRESET_MASK, "instrumental")
    all_components = transition_stem_intent(instrumental, STEM_COMPONENT_MASK, "all")
    restored = transition_stem_intent(all_components, all_components.last_custom_mask, "custom")

    assert instrumental.enabled_mask == STEM_INSTRUMENTAL_PRESET_MASK
    assert instrumental.display_mode == "instrumental"
    assert instrumental.last_custom_mask == custom_mask
    assert all_components.enabled_mask == STEM_COMPONENT_MASK
    assert all_components.display_mode == "all"
    assert all_components.last_custom_mask == custom_mask
    assert restored == custom


@pytest.mark.parametrize(
    ("mask", "display_mode"),
    [(STEM_INSTRUMENTAL_PRESET_MASK, "instrumental"), (STEM_COMPONENT_MASK, "all")],
)
def test_leaving_custom_remembers_active_mask_instead_of_stale_remembered_mask(
    mask: int, display_mode: StemMaskDisplayMode
) -> None:
    current = PadStemIntent(
        enabled_mask=STEM_MASK_VOCALS | STEM_MASK_MELODY,
        last_custom_mask=STEM_MASK_DRUMS,
        display_mode="custom",
    )

    selected = transition_stem_intent(current, mask, display_mode)

    assert selected.last_custom_mask == current.enabled_mask
    assert current.last_custom_mask == STEM_MASK_DRUMS


def test_repeated_custom_selection_refreshes_remembered_mask() -> None:
    current = PadStemIntent(
        enabled_mask=STEM_MASK_VOCALS,
        last_custom_mask=STEM_MASK_DRUMS,
        display_mode="custom",
    )

    selected = transition_stem_intent(current, current.enabled_mask, "custom")

    assert selected.enabled_mask == current.enabled_mask
    assert selected.display_mode == current.display_mode
    assert selected.last_custom_mask == current.enabled_mask
    assert current.last_custom_mask == STEM_MASK_DRUMS


def test_fully_muted_custom_selection_survives_preset_toggles_and_json_reopen() -> None:
    muted = transition_stem_intent(PadStemIntent(), 0)
    preset = transition_stem_intent(muted, STEM_INSTRUMENTAL_PRESET_MASK, "instrumental")
    reopened = PadStemIntent.model_validate_json(preset.model_dump_json())

    restored = transition_stem_intent(reopened, reopened.last_custom_mask)

    assert restored == muted
    assert restored.enabled_mask == restored.last_custom_mask == 0
    assert restored.display_mode == "custom"


@pytest.mark.parametrize("field", ["enabled_mask", "last_custom_mask"])
@pytest.mark.parametrize("fault", [True, False, 1.0, "1", None, -1, 16, 1 << 4, 255])
def test_masks_reject_boolean_coercion_and_values_outside_component_bits(
    field: str, fault: object
) -> None:
    values: dict[str, object] = {
        "enabled_mask": STEM_MASK_VOCALS,
        "last_custom_mask": STEM_MASK_VOCALS,
        "display_mode": "custom",
        field: fault,
    }

    with pytest.raises(ValidationError, match=field):
        PadStemIntent.model_validate(values)
    with pytest.raises(ValidationError, match=field):
        PadStemIntent.model_validate_json(json.dumps(values))


@pytest.mark.parametrize("revision", [True, False, 1.0, "1", None, 0, 2, -1])
def test_schema_revision_rejects_boolean_coercion_and_unsupported_versions(
    revision: object,
) -> None:
    values = PadStemIntent().model_dump() | {"schema_version": revision}

    with pytest.raises(ValidationError, match="schema_version"):
        PadStemIntent.model_validate(values)
    with pytest.raises(ValidationError, match="schema_version"):
        PadStemIntent.model_validate_json(json.dumps(values))


@pytest.mark.parametrize("display_mode", ["preset", "full_mix", "all_stems", True, None])
def test_display_mode_only_accepts_the_three_selection_modes(display_mode: object) -> None:
    with pytest.raises(ValidationError, match="display_mode"):
        PadStemIntent.model_validate({"display_mode": display_mode})


@pytest.mark.parametrize(
    ("mask", "display_mode"),
    [
        (0, "all"),
        (STEM_MASK_VOCALS, "all"),
        (STEM_COMPONENT_MASK, "instrumental"),
        (STEM_MASK_DRUMS, "instrumental"),
    ],
)
def test_presets_reject_inconsistent_component_selection(
    mask: int, display_mode: StemMaskDisplayMode
) -> None:
    values = {"enabled_mask": mask, "display_mode": display_mode}

    with pytest.raises(ValidationError, match="stem display mode"):
        PadStemIntent.model_validate(values)
    with pytest.raises(ValidationError, match="stem display mode"):
        PadStemIntent.model_validate_json(json.dumps(values))


@pytest.mark.parametrize(
    "field", ["native_ack", "request_id", "sample_id", "available", "mute_mask", "unknown"]
)
def test_runtime_authority_and_duplicate_mute_fields_cannot_be_persisted(field: str) -> None:
    with pytest.raises(ValidationError, match="Extra inputs"):
        PadStemIntent.model_validate(PadStemIntent().model_dump() | {field: 1})


@pytest.mark.parametrize(
    ("mask", "display_mode"),
    [(True, "custom"), (16, "custom"), (0, "all"), (STEM_COMPONENT_MASK, "instrumental")],
)
def test_failed_transition_preserves_all_original_selection_fields(
    mask: int, display_mode: StemMaskDisplayMode
) -> None:
    original = transition_stem_intent(PadStemIntent(), STEM_MASK_VOCALS | STEM_MASK_BASS)
    before = original.model_dump_json()

    with pytest.raises(ValidationError):
        transition_stem_intent(original, mask, display_mode)

    assert original.model_dump_json() == before


@pytest.mark.parametrize(
    "update",
    [
        {"enabled_mask": True},
        {"last_custom_mask": 16},
        {"display_mode": "preset"},
        {"schema_version": 2},
        {"enabled_mask": 0},
    ],
)
def test_unvalidated_model_copy_is_rechecked_at_schema_and_transition_boundaries(
    update: dict[str, object],
) -> None:
    original = PadStemIntent()
    invalid = original.model_copy(update=update)

    with pytest.raises(ValidationError):
        PadStemIntent.model_validate(invalid)
    with pytest.raises(ValidationError):
        transition_stem_intent(invalid, STEM_MASK_VOCALS, "custom")
    assert original == PadStemIntent()


def test_model_construct_cannot_bypass_schema_or_transition_revalidation() -> None:
    invalid = PadStemIntent.model_construct(
        schema_version=1, enabled_mask=True, last_custom_mask=0, display_mode="custom"
    )

    with pytest.raises(ValidationError, match="enabled_mask"):
        PadStemIntent.model_validate(invalid)
    with pytest.raises(ValidationError, match="enabled_mask"):
        transition_stem_intent(invalid, STEM_COMPONENT_MASK, "all")
