"""Pure key metadata, absolute selection and neutral durable intent contracts."""

from typing import TYPE_CHECKING, cast

import pytest
from pydantic import ValidationError

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.key_intent import (
    MAX_KEY_EPOCH,
    KeyCorrectionView,
    MusicalKey,
    PadKeyIntent,
    SourceKeyVersion,
    recognize_key,
)
from flitzis_looper.models import BeatGrid, ProjectState, SampleAnalysis

if TYPE_CHECKING:
    from typing import Literal

    from pydantic import BaseModel

ROOT_NAMES = ("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")
ABSOLUTE_DELTAS = (0, 1, 2, 3, 4, 5, 6, -5, -4, -3, -2, -1)


@pytest.mark.parametrize("mode", ["major", "minor"])
@pytest.mark.parametrize("source_root", range(12))
@pytest.mark.parametrize("target_root", range(12))
def test_every_absolute_key_selects_once_without_accumulation(
    mode: Literal["major", "minor"], source_root: int, target_root: int
) -> None:
    suffix = "m" if mode == "minor" else ""
    original = PadKeyIntent(
        source=SourceKeyVersion(version=9, raw_key="unknown detected text"),
        correction=ROOT_NAMES[source_root] + suffix,
        analysis_epoch=7,
        correction_epoch=4,
        base_shift=-5,
        extra_shift=12,
        retrigger=True,
    )
    target = MusicalKey(root=target_root, mode=mode)

    selected = original.with_base_key(target)

    assert selected.base_shift == ABSOLUTE_DELTAS[(target_root - source_root) % 12]
    assert selected.base_key == target
    assert selected.extra_shift == 12
    assert selected.result_key == target
    assert selected.changed(base_shift=original.base_shift) == original
    assert selected.with_base_key(target) == selected
    assert selected.with_base_key(target).with_base_key(target) == selected
    assert original.base_shift == -5
    if (target_root - source_root) % 12 == 6:
        assert selected.base_shift == 6


@pytest.mark.parametrize(("extra", "label", "total"), [(0, "Cm", -4), (2, "Dm", -2), (12, "Cm", 8)])
def test_e_minor_to_c_minor_keeps_octave_as_numeric_intent(
    extra: int, label: str, total: int
) -> None:
    source = PadKeyIntent(source=SourceKeyVersion(raw_key="Em"))
    base = source.with_base_key(MusicalKey(root=0, mode="minor"))
    result = base.changed(extra_shift=extra)

    assert result.source_label == "Em"
    assert result.base_shift == -4
    assert result.base_key is not None
    assert result.base_key.label == "Cm"
    assert result.result_key is not None
    assert result.result_key.label == label
    assert result.base_shift + result.extra_shift == total
    assert result.extra_shift == extra
    assert result.changed(extra_shift=extra) == result
    assert base.extra_shift == 0


@pytest.mark.parametrize("mode", ["major", "minor"])
@pytest.mark.parametrize("root", range(12))
def test_producer_names_recognize_exact_pitch_class_and_mode(
    mode: Literal["major", "minor"], root: int
) -> None:
    label = ROOT_NAMES[root] + ("m" if mode == "minor" else "")
    key = recognize_key(label)
    assert key == MusicalKey(root=root, mode=mode)
    assert key is not None
    assert key.label == label
    assert key.shifted(12) == key
    assert key.shifted(-12) == key


@pytest.mark.parametrize(("raw", "root"), [("Ab", 8), ("Eb", 3), ("Bb", 10)])
@pytest.mark.parametrize("suffix", ["", "m"])
def test_flat_aliases_preserve_raw_metadata_and_derive_canonical_labels(
    raw: str, root: int, suffix: str
) -> None:
    intent = PadKeyIntent(source=SourceKeyVersion(version=3, raw_key=raw + suffix))
    key = intent.source_key
    assert intent.source_label == raw + suffix
    assert key is not None
    assert key.root == root
    assert key.mode == ("minor" if suffix else "major")
    assert key.label == ROOT_NAMES[root] + suffix


@pytest.mark.parametrize("raw", [None, "", "H", "Db", "C minor", "8A", "C ", "Am7", "unknown ♭"])
def test_unknown_key_never_guesses_but_relative_intent_remains_usable(raw: str | None) -> None:
    intent = PadKeyIntent(correction=raw, base_shift=6, extra_shift=18)
    assert recognize_key(raw) is None
    assert intent.source_label == raw
    assert intent.source_key is None
    assert intent.base_key is None
    assert intent.result_key is None
    assert intent.changed(extra_shift=-18).extra_shift == -18
    with pytest.raises(ValueError, match="recognized source"):
        intent.with_base_key(MusicalKey(root=0, mode="major"))
    assert intent.base_shift == 6
    assert intent.extra_shift == 18


@pytest.mark.parametrize(("source", "target_mode"), [("Cm", "major"), ("C", "minor")])
def test_absolute_selection_rejects_mode_conversion_without_mutating_intent(
    source: str, target_mode: Literal["major", "minor"]
) -> None:
    intent = PadKeyIntent(correction=source, base_shift=2, extra_shift=-7, retrigger=True)
    before = intent.model_dump()
    with pytest.raises(ValueError, match="same major/minor mode"):
        intent.with_base_key(MusicalKey(root=5, mode=target_mode))
    assert intent.model_dump() == before


def test_correction_and_three_resets_keep_independent_numeric_choices() -> None:
    original = PadKeyIntent(
        source=SourceKeyVersion(version=5, raw_key="Em"),
        correction="Cm",
        analysis_epoch=8,
        correction_epoch=2,
        base_shift=-4,
        extra_shift=12,
        retrigger=True,
    )
    changed_mode = original.corrected("F#")
    assert changed_mode.base_shift == -4
    assert changed_mode.extra_shift == 12
    assert changed_mode.retrigger
    assert changed_mode.source == original.source
    assert changed_mode.analysis_epoch == 8
    assert changed_mode.correction_epoch == 3
    assert changed_mode.corrected("F#") is changed_mode
    assert changed_mode.base_key == MusicalKey(root=2, mode="major")

    no_correction = original.corrected(None)
    assert no_correction.source_label == "Em"
    assert no_correction.base_shift == -4
    assert no_correction.extra_shift == 12
    assert no_correction.retrigger
    assert no_correction.correction_epoch == 3
    assert no_correction.corrected(None) is no_correction

    no_base = original.changed(base_shift=0)
    assert no_base.changed(base_shift=-4) == original
    no_extra = original.changed(extra_shift=0)
    assert no_extra.changed(extra_shift=12) == original
    assert original.correction == "Cm"
    assert original.correction_epoch == 2


@pytest.mark.parametrize("extra", range(-18, 19))
def test_all_extra_choices_are_absolute_and_preserve_source_and_base(extra: int) -> None:
    original = PadKeyIntent(correction="unknown", base_shift=-5, extra_shift=18)
    changed = original.changed(extra_shift=extra)
    assert changed.extra_shift == extra
    assert changed.changed(extra_shift=extra) == changed
    assert changed.changed(extra_shift=18) == original


@pytest.mark.parametrize(
    "updates",
    [
        {"base_shift": -6},
        {"base_shift": 7},
        {"base_shift": True},
        {"base_shift": 1.0},
        {"base_shift": "1"},
        {"extra_shift": -19},
        {"extra_shift": 19},
        {"extra_shift": False},
        {"extra_shift": 2.0},
        {"extra_shift": "2"},
        {"retrigger": 0},
        {"retrigger": 1},
        {"retrigger": "true"},
        {"schema_version": True},
        {"schema_version": 0},
        {"schema_version": 2},
        {"correction": 12},
        {"correction": False},
        {"unknown": "field"},
    ],
)
def test_changed_rejects_coercion_and_out_of_range_updates_atomically(
    updates: dict[str, object],
) -> None:
    original = PadKeyIntent(correction="Em", base_shift=6, extra_shift=-18, retrigger=True)
    before = original.model_dump()
    with pytest.raises(ValidationError):
        original.changed(**updates)
    assert original.model_dump() == before


@pytest.mark.parametrize("field", ["analysis_epoch", "correction_epoch"])
@pytest.mark.parametrize("value", [-1, MAX_KEY_EPOCH + 1, True, 1.0, "1"])
def test_epochs_are_strict_bounded_integers(field: str, value: object) -> None:
    with pytest.raises(ValidationError):
        PadKeyIntent.model_validate({field: value})


@pytest.mark.parametrize("version", [-1, MAX_KEY_EPOCH + 1, False, 1.0, "1"])
def test_source_version_is_strict_and_cannot_wrap(version: object) -> None:
    with pytest.raises(ValidationError):
        SourceKeyVersion.model_validate({"version": version, "raw_key": "Em"})


@pytest.mark.parametrize("raw", [None, False, 12])
def test_source_display_metadata_does_not_coerce_non_strings(raw: object) -> None:
    with pytest.raises(ValidationError):
        SourceKeyVersion.model_validate({"raw_key": raw})


@pytest.mark.parametrize("root", [-1, 12, True, 1.0, "1"])
def test_recognized_root_rejects_invalid_values(root: object) -> None:
    with pytest.raises(ValidationError):
        MusicalKey.model_validate({"root": root, "mode": "minor"})


@pytest.mark.parametrize("mode", ["dorian", "", None, True])
def test_recognized_key_rejects_unsupported_modes(mode: object) -> None:
    with pytest.raises(ValidationError):
        MusicalKey.model_validate({"root": 4, "mode": mode})


@pytest.mark.parametrize("semitones", [True, False, 1.0, "1", None])
def test_label_shift_rejects_coercion(semitones: object) -> None:
    key = MusicalKey(root=4, mode="minor")
    with pytest.raises(TypeError, match="integer"):
        key.shifted(cast("int", semitones))
    assert key.label == "Em"


def test_changed_revalidates_existing_unchecked_model_copy_fields() -> None:
    original = PadKeyIntent(base_shift=6)
    unchecked = original.model_copy(update={"base_shift": True})
    with pytest.raises(ValidationError):
        unchecked.changed(extra_shift=2)
    assert original.base_shift == 6


def test_epoch_exhaustion_rejects_new_correction_but_allows_idempotence() -> None:
    original = PadKeyIntent(
        correction="Em", correction_epoch=MAX_KEY_EPOCH, analysis_epoch=MAX_KEY_EPOCH
    )
    assert original.corrected("Em") is original
    for correction in (None, "Cm"):
        with pytest.raises(ValueError, match="capacity exhausted"):
            original.corrected(correction)
    assert original.correction == "Em"
    assert original.correction_epoch == MAX_KEY_EPOCH


@pytest.mark.parametrize(
    ("value", "field", "update"),
    [
        (MusicalKey(root=4, mode="minor"), "root", 5),
        (SourceKeyVersion(version=2, raw_key="Em"), "raw_key", "Cm"),
        (PadKeyIntent(correction="Em"), "correction", "Cm"),
    ],
)
def test_durable_metadata_objects_are_frozen(value: BaseModel, field: str, update: object) -> None:
    before = value.model_dump()
    with pytest.raises(ValidationError, match="frozen"):
        setattr(value, field, update)
    assert value.model_dump() == before


def test_copy_and_roundtrip_keep_complete_intent_independent() -> None:
    original = PadKeyIntent(
        source=SourceKeyVersion(version=MAX_KEY_EPOCH, raw_key="Em"),
        correction="Cm",
        analysis_epoch=MAX_KEY_EPOCH,
        correction_epoch=8,
        base_shift=-5,
        extra_shift=-18,
        retrigger=True,
    )
    copied = original.model_copy(deep=True)
    restored = PadKeyIntent.model_validate_json(original.model_dump_json())
    assert copied == restored == original
    assert copied is not original
    assert copied.source is not original.source
    edited = copied.corrected(None).changed(base_shift=6, extra_shift=18, retrigger=False)
    assert edited.base_shift + edited.extra_shift == 24
    assert original.base_shift + original.extra_shift == -23
    assert original.correction == restored.correction == "Cm"


def test_fixed_correction_facade_updates_both_endpoint_slots_without_aliasing() -> None:
    project = ProjectState()
    project.pad_key_intent[0] = PadKeyIntent(base_shift=-4, extra_shift=12, retrigger=True)
    original_last = project.pad_key_intent[215]
    view = KeyCorrectionView(project.pad_key_intent)
    view[0] = "Cm"
    view[-1] = "unknown legacy text"
    assert len(view) == NUM_SAMPLES
    assert view[:2] == ["Cm", None]
    assert view[-1] == "unknown legacy text"
    assert project.manual_key[0] == "Cm"
    assert project.pad_key_intent[0].base_shift == -4
    assert project.pad_key_intent[0].extra_shift == 12
    assert project.pad_key_intent[0].retrigger
    assert original_last == PadKeyIntent()
    assert project.pad_key_intent[214] == PadKeyIntent()
    view[0] = None
    assert project.pad_key_intent[215].correction == "unknown legacy text"


def test_fixed_facade_rejects_resize_and_invalid_slice_without_partial_update() -> None:
    project = ProjectState()
    view = project.manual_key
    before = list(project.pad_key_intent)
    with pytest.raises(ValueError, match="fixed pad count"):
        view.insert(0, "Cm")
    with pytest.raises(ValueError, match="fixed pad count"):
        del view[0]
    with pytest.raises(ValueError, match="fixed pad count"):
        view[:2] = ["Cm"]
    with pytest.raises(ValidationError):
        view[:2] = ["Cm", cast("str", 1)]
    assert project.pad_key_intent == before
    assert all(
        current is previous
        for current, previous in zip(project.pad_key_intent, before, strict=True)
    )


def test_facade_epoch_exhaustion_rolls_back_the_entire_multi_slot_update() -> None:
    project = ProjectState()
    project.pad_key_intent[1] = PadKeyIntent(correction_epoch=MAX_KEY_EPOCH)
    before = list(project.pad_key_intent)
    with pytest.raises(ValueError, match="capacity exhausted"):
        project.manual_key[:2] = ["Cm", "Em"]
    assert project.pad_key_intent == before
    assert project.manual_key[:2] == [None, None]
    assert project.pad_key_intent[0] is before[0]
    assert project.pad_key_intent[1] is before[1]


def test_legacy_migration_preserves_arbitrary_text_and_neutral_audio_intent() -> None:
    legacy = ProjectState().model_dump(mode="json", exclude={"pad_key_intent"})
    corrections: list[str | None] = [None] * NUM_SAMPLES
    corrections[0] = "unknown legacy ♭ text"
    corrections[215] = "Bbm"
    legacy["manual_key"] = corrections
    analyses: list[object] = [None] * NUM_SAMPLES
    analysis = SampleAnalysis(
        bpm=123.0, key="Em", beat_grid=BeatGrid(beats=[], downbeats=[], bars=[])
    )
    legacy["sample_analysis"] = analyses
    analyses[0] = analysis.model_dump(mode="json")

    project = ProjectState.model_validate(legacy)

    assert project.manual_key[0] == corrections[0]
    assert project.pad_key_intent[0].source == SourceKeyVersion(version=0, raw_key="Em")
    assert project.pad_key_intent[0].source_key is None
    assert project.manual_key[215] == "Bbm"
    assert project.pad_key_intent[215].source_key == MusicalKey(root=10, mode="minor")
    assert all(intent.base_shift == intent.extra_shift == 0 for intent in project.pad_key_intent)
    assert all(not intent.retrigger for intent in project.pad_key_intent)
    assert all(
        intent.analysis_epoch == intent.correction_epoch == 0 for intent in project.pad_key_intent
    )
    reopened = ProjectState.model_validate_json(project.model_dump_json())
    assert reopened.pad_key_intent == project.pad_key_intent


def test_explicit_none_in_new_schema_never_resurrects_legacy_correction() -> None:
    project = ProjectState()
    previous = PadKeyIntent(
        source=SourceKeyVersion(version=4, raw_key="Em"),
        correction="Cm",
        correction_epoch=6,
        base_shift=-4,
        extra_shift=12,
        retrigger=True,
    )
    project.pad_key_intent[0] = previous.corrected(None)
    saved = project.model_dump(mode="json")
    saved["manual_key"] = ["Cm"] * NUM_SAMPLES
    restored = ProjectState.model_validate(saved)
    assert restored.pad_key_intent[0] == project.pad_key_intent[0]
    assert restored.manual_key[0] is None
    for _ in range(3):
        restored = ProjectState.model_validate_json(restored.model_dump_json())
        assert restored.pad_key_intent[0] == project.pad_key_intent[0]
        assert restored.manual_key[0] is None
