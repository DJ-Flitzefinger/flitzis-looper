"""Durable piano domains and action identities; no Native/audio acceptance claims."""

import pytest
from pydantic import ValidationError

from flitzis_looper.controller.key_metadata_persistence import recover_project_key_intent
from flitzis_looper.input_mapping.actions import (
    LooperAction,
    parse_pitch_action,
    pitch_base_action,
    pitch_extra_action,
    pitch_note_action,
    pitch_original_action,
    pitch_reset_base_action,
    pitch_step_action,
)
from flitzis_looper.key_intent import MusicalKey, PadKeyIntent, SourceKeyVersion

ROOT_NAMES = ("C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B")


@pytest.mark.parametrize("mode", ["absolute", "twos_complement", "offset_binary"])
@pytest.mark.parametrize("step", [-12, -1, 1, 12])
@pytest.mark.parametrize("piano", [False, True])
def test_encoder_actions_roundtrip_explicit_encoding_and_variant(
    mode: str, step: int, *, piano: bool
) -> None:
    raw = f"pitch.{'note' if piano else 'extra'}.encoder:{step}:{mode}:set_retrigger"
    action = parse_pitch_action(LooperAction.from_key(raw).key)
    assert action is not None
    assert action.target == step
    assert action.kind == ("note_step" if piano else "extra_step")
    assert action.cc_mode == mode
    assert action.retrigger


@pytest.mark.parametrize("root", range(12))
def test_each_source_root_retains_all_three_chromatic_octaves(root: int) -> None:
    source = SourceKeyVersion(version=7, raw_key=ROOT_NAMES[root])
    original_index = 12 + root
    chosen = [PadKeyIntent(source=source, base_shift=index - original_index) for index in range(36)]

    assert len({intent.base_shift for intent in chosen}) == 36
    assert chosen[original_index].base_shift == 0
    assert chosen[original_index].base_key == source.recognized
    for index, intent in enumerate(chosen):
        assert intent.base_key == MusicalKey(root=index % 12, mode="major")
        assert intent.source == source
        assert PadKeyIntent.model_validate_json(intent.model_dump_json()) == intent
    assert [chosen[index].base_shift for index in (0, 12, 24)] == [
        -12 - root,
        -root,
        12 - root,
    ]


@pytest.mark.parametrize("base", range(-5, 7))
def test_all_legacy_base_and_37_offset_combinations_keep_their_exact_values(base: int) -> None:
    for extra in range(-18, 19):
        intent = PadKeyIntent(base_shift=base, extra_shift=extra)
        assert (intent.base_shift, intent.extra_shift) == (base, extra)
        assert -23 <= intent.base_shift + intent.extra_shift <= 24
    assert PadKeyIntent(base_shift=-5, extra_shift=-18).base_shift == -5
    assert PadKeyIntent(base_shift=6, extra_shift=18).extra_shift == 18


def test_octave_bearing_pairs_refuse_every_unsupported_sum_without_clamping() -> None:
    for base in range(-23, 24):
        for extra in range(-18, 19):
            if -23 <= base + extra <= 24:
                intent = PadKeyIntent(base_shift=base, extra_shift=extra)
                assert (intent.base_shift, intent.extra_shift) == (base, extra)
            else:
                with pytest.raises(ValidationError, match="combined pitch"):
                    PadKeyIntent(base_shift=base, extra_shift=extra)
    original = PadKeyIntent(base_shift=23)
    with pytest.raises(ValidationError, match="combined pitch"):
        original.changed(extra_shift=18)
    assert (original.base_shift, original.extra_shift) == (23, 0)


@pytest.mark.parametrize("field", ["base_shift", "extra_shift"])
@pytest.mark.parametrize("invalid", [True, 1.0, "1"])
def test_numeric_intent_never_coerces_noninteger_values(field: str, invalid: object) -> None:
    with pytest.raises(ValidationError):
        PadKeyIntent.model_validate({field: invalid})


@pytest.mark.parametrize("legacy_retrigger", [False, True])
def test_legacy_shared_preference_migrates_once_then_roundtrips_independently(
    *, legacy_retrigger: bool
) -> None:
    restored = PadKeyIntent.model_validate({"schema_version": 1, "retrigger": legacy_retrigger})
    assert (restored.retrigger, restored.extra_retrigger) == (
        legacy_retrigger,
        legacy_retrigger,
    )
    piano_changed = restored.changed(retrigger=not legacy_retrigger)
    assert piano_changed.extra_retrigger is legacy_retrigger
    offset_changed = piano_changed.changed(extra_retrigger=not legacy_retrigger)
    assert offset_changed.retrigger is not legacy_retrigger
    assert PadKeyIntent.model_validate_json(piano_changed.model_dump_json()) == piano_changed
    assert PadKeyIntent.model_validate_json(offset_changed.model_dump_json()) == offset_changed


def test_original_and_offset_reset_preserve_distinct_metadata_and_octave_contracts() -> None:
    intent = PadKeyIntent(
        source=SourceKeyVersion(version=6, raw_key="C"),
        correction="D",
        correction_epoch=9,
        analysis_epoch=8,
        base_shift=14,
        extra_shift=1,
        retrigger=True,
        extra_retrigger=False,
    )
    offset_reset = intent.changed(extra_shift=0)
    assert (offset_reset.base_shift, offset_reset.extra_shift, offset_reset.correction) == (
        14,
        0,
        "D",
    )
    assert offset_reset.correction_epoch == 9
    original = intent.original()
    assert (original.base_shift, original.extra_shift, original.correction) == (0, 0, None)
    assert original.correction_epoch == 10
    assert original.source == intent.source
    assert original.analysis_epoch == intent.analysis_epoch
    assert (original.retrigger, original.extra_retrigger) == (True, False)
    assert original.original() == original


@pytest.mark.parametrize(("base", "extra"), [(23, -18), (-23, 18)])
def test_recovery_retains_a_valid_compensating_pair_and_independent_preferences(
    base: int, extra: int
) -> None:
    raw = {
        "schema_version": 999,
        "base_shift": base,
        "extra_shift": extra,
        "correction": "unknown historical label",
        "correction_epoch": 12,
        "retrigger": True,
        "extra_retrigger": False,
    }
    recovered = recover_project_key_intent({"pad_key_intent": [raw]})["pad_key_intent"]
    assert isinstance(recovered, list)
    intent = recovered[0]
    assert isinstance(intent, PadKeyIntent)
    assert (intent.base_shift, intent.extra_shift) == (base, extra)
    assert intent.correction == "unknown historical label"
    assert intent.correction_epoch == 12
    assert (intent.retrigger, intent.extra_retrigger) == (True, False)


def test_invalid_joint_recovery_retains_only_supported_fields_and_legacy_preference() -> None:
    recovered = recover_project_key_intent({
        "pad_key_intent": [
            {
                "base_shift": 23,
                "extra_shift": 18,
                "correction": "Fm",
                "retrigger": True,
            }
        ]
    })["pad_key_intent"]
    assert isinstance(recovered, list)
    intent = recovered[0]
    assert isinstance(intent, PadKeyIntent)
    assert (intent.base_shift, intent.extra_shift, intent.correction) == (23, 0, "Fm")
    assert (intent.retrigger, intent.extra_retrigger) == (True, True)


@pytest.mark.parametrize("retrigger", [False, True])
def test_new_and_legacy_learned_action_variants_keep_their_serialized_meanings(
    *, retrigger: bool
) -> None:
    actions = [
        (pitch_note_action(-12, retrigger=retrigger), "note", -12),
        (pitch_note_action(0, retrigger=retrigger), "note", 0),
        (pitch_note_action(12, retrigger=retrigger), "note", 12),
        (pitch_original_action(retrigger=retrigger), "original", None),
        (pitch_extra_action(0, retrigger=retrigger), "extra", 0),
        (pitch_reset_base_action(retrigger=retrigger), "reset_base", None),
        (
            pitch_base_action(MusicalKey(root=6, mode="minor"), retrigger=retrigger),
            "base",
            MusicalKey(root=6, mode="minor"),
        ),
    ]
    for step in (-12, -1, 1, 12):
        actions.extend([
            (pitch_step_action(step, piano=True, retrigger=retrigger), "note_step", step),
            (pitch_step_action(step, retrigger=retrigger), "extra_step", step),
        ])
    assert len({action.key for action, _, _ in actions}) == len(actions)
    for action, kind, target in actions:
        restored = LooperAction.model_validate_json(action.model_dump_json())
        parsed = parse_pitch_action(restored.key)
        assert restored.source == "direct"
        assert parsed is not None
        assert (parsed.kind, parsed.target, parsed.retrigger) == (kind, target, retrigger)


@pytest.mark.parametrize(
    "key",
    [
        "pitch.note:24:set",
        "pitch.note:-24:set",
        "pitch.note:+1:set",
        "pitch.note:01:set",
        "pitch.note:-0:set",
        "pitch.note.step:0:set",
        "pitch.note.step:2:set",
        "pitch.extra.step:+12:set",
        "pitch.extra.step:12:set:extra",
        "pitch.original:0:set",
        "pitch.original:toggle",
    ],
)
def test_malformed_octave_and_step_action_keys_have_no_pitch_semantics(key: str) -> None:
    assert parse_pitch_action(key) is None
