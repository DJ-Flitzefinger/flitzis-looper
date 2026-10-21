"""Pure complete bank images; runtime claim, MIDI authority and restart remain separate."""

import json

import pytest
from pydantic import ValidationError

from flitzis_looper.bank_snapshot import (
    BankSnapshot,
    BankSnapshotData,
    capture_bank_snapshot,
    project_with_bank_snapshot,
)
from flitzis_looper.constants import NUM_PADS
from flitzis_looper.copy_snapshot import (
    CONTENT_FIELDS,
    CopySnapshot,
    capture_copy_snapshot,
    copy_after_snapshot,
    musical_content_data,
    musical_snapshot_projection,
    project_with_copy_snapshot,
)
from flitzis_looper.models import PadContentIdentity, ProjectState
from flitzis_looper.pair_snapshot import PairSnapshot, capture_pair_snapshot
from flitzis_looper.stem_intent import PadStemIntent
from tests.flitzis_looper.test_copy_snapshot import (
    _full_project,
    _historical_timing,
    _target_identity,
)


def _place(project: ProjectState, target: int) -> ProjectState:
    snapshot = capture_copy_snapshot(project, 0)
    source = snapshot.projected().source_identity
    return project_with_copy_snapshot(
        project,
        target,
        snapshot,
        target_identity=PadContentIdentity(
            instance_id=f"{target + 1:032x}",
            material_id=source.material_id,
            material_creation=source.material_creation,
            copied_from=source.instance_id,
        ),
    )


def _mixed_project() -> ProjectState:
    project = _full_project()
    for target in (35, 36, 37, 71, 108):
        project = _place(project, target)
    project.pad_gain_db[1] = -17.0  # A real empty source witness can retain edited controls.
    project.pad_gain_db[35] = -12.0
    project.pad_eq_high_db[35] = -8.0
    project.pad_key_lock[35] = False
    project.pad_loop_start_s[35] = 4.0
    project.pad_loop_end_s[35] = 23.0
    project.pad_stem_intent[35] = PadStemIntent(
        enabled_mask=14, last_custom_mask=5, display_mode="instrumental"
    )
    project.selected_pad = 0
    project.selected_bank = 0
    return project


def test_mixed_bank_copy_preserves_complete_music_and_removes_empty_source_targets() -> None:
    project = _mixed_project()
    before = project.model_dump_json()
    snapshot = capture_bank_snapshot(project, 0, 1)

    candidate = project_with_bank_snapshot(project, snapshot)

    assert len(snapshot.projected().slots) == NUM_PADS
    assert candidate.pad_gain_db[36] == -7.5
    assert candidate.pad_loop_start_s[36] == 2.125
    assert candidate.pad_loop_end_s[36] == 19.75
    assert candidate.pad_grid_offset_samples[36] == -37
    assert candidate.pad_key_intent[36] == project.pad_key_intent[0]
    assert candidate.pad_key_lock[36] is None
    assert candidate.pad_gain_db[71] == -12.0
    assert candidate.pad_eq_high_db[71] == -8.0
    assert candidate.pad_stem_intent[71].enabled_mask == 14
    assert candidate.pad_key_lock[71] is False
    assert musical_content_data(candidate, 37) == musical_content_data(ProjectState(), 37)
    assert candidate.sample_paths[37] is None
    assert candidate.pad_content[37] is None
    for source_id, target_id in ((0, 36), (35, 71)):
        source, target = project.pad_content[source_id], candidate.pad_content[target_id]
        assert source is not None
        assert target is not None
        assert target.instance_id != source.instance_id
        assert target.copied_from == source.instance_id
        assert target.material_id == source.material_id
        assert target.material_creation == source.material_creation
        assert candidate.sample_paths[target_id] == project.sample_paths[source_id]
        copied_stems = candidate.stem_cache[target_id]
        assert copied_stems is not None
        assert copied_stems.available is False
    assert candidate.selected_pad == 0
    assert candidate.selected_bank == 0
    assert project.model_dump_json() == before


def test_all_36_copies_are_fresh_independent_instances_sharing_original_material() -> None:
    project = _full_project()
    for target in range(1, NUM_PADS):
        project = _place(project, target)
    snapshot = capture_bank_snapshot(project, 0, 1)

    candidate = project_with_bank_snapshot(project, snapshot)

    source_ids = {content.instance_id for content in project.pad_content[:36] if content}
    copied_ids = {content.instance_id for content in candidate.pad_content[36:72] if content}
    assert len(source_ids) == len(copied_ids) == NUM_PADS
    assert source_ids.isdisjoint(copied_ids)
    assert candidate.pad_content[:36] == project.pad_content[:36]
    for offset in range(NUM_PADS):
        source, copied = project.pad_content[offset], candidate.pad_content[36 + offset]
        assert source is not None
        assert copied is not None
        assert copied.copied_from == source.instance_id
        assert copied.material_id == source.material_id
        assert copied.material_creation == source.material_creation
        assert candidate.sample_paths[36 + offset] == project.sample_paths[offset]
    assert candidate.pad_key_intent[36] is not candidate.pad_key_intent[37]
    assert candidate.pad_stem_intent[36] is not candidate.pad_stem_intent[37]


def test_full_bank_after_rows_equal_existing_individual_copy_projection_of_complete_music() -> None:
    project = _full_project()
    for target in range(1, NUM_PADS):
        project = _place(project, target)
        project.pad_gain_db[target] = -6.0 - target / 10
        project.pad_key_lock[target] = False if target % 2 else None
    before = project.model_dump_json()

    snapshot = capture_bank_snapshot(project, 0, 1)
    captured = snapshot.projected()
    aggregate = project_with_bank_snapshot(project, snapshot)

    for row in captured.slots:
        assert row.source_id is not None
        identity = row.after.source_identity
        assert identity is not None
        source = capture_copy_snapshot(project, row.source_id)
        independent = project_with_copy_snapshot(
            project, row.target_id, source, target_identity=identity
        )
        expected = musical_content_data(independent, row.target_id)
        assert musical_snapshot_projection(row.after) == expected
        assert musical_content_data(aggregate, row.target_id) == expected
        assert identity.copied_from == source.projected().source_identity.instance_id
        assert row.after.analysis is not None
        assert row.after.analysis.accepted_timing == _historical_timing()
        assert row.after.stem_cache is not None
        assert row.after.stem_cache.available is False
    assert project.model_dump_json() == before


@pytest.mark.parametrize("damage", ["same_instance", "material", "material_birth", "copied_from"])
def test_common_copy_after_preserves_fresh_material_birth_and_lineage_guards(damage: str) -> None:
    source = capture_copy_snapshot(_full_project(), 0)
    original = source.projected().source_identity
    identity = _target_identity(source)
    if damage == "same_instance":
        updates = {"instance_id": original.instance_id, "copied_from": None}
    elif damage == "material":
        updates = {"material_id": "9" * 32}
    elif damage == "material_birth":
        updates = {"material_creation": None}
    else:
        updates = {"copied_from": "9" * 32}

    with pytest.raises(ValueError, match="Copy successor must be fresh"):
        copy_after_snapshot(source, identity.model_copy(update=updates))


def test_common_copy_after_revalidates_unchecked_schema_and_nested_stem_metadata() -> None:
    source = capture_copy_snapshot(_full_project(), 0)
    identity = _target_identity(source)
    unsupported = CopySnapshot.model_construct(schema_version=2, payload_json=source.payload_json)
    with pytest.raises(ValidationError):
        copy_after_snapshot(unsupported, identity)
    payload = json.loads(source.payload_json)
    payload["stem_cache"]["stems"]["future_component"] = "unrecognized.wav"
    unchecked = CopySnapshot.model_construct(schema_version=1, payload_json=json.dumps(payload))
    with pytest.raises(ValidationError, match="StemFileSet contains unsupported fields"):
        copy_after_snapshot(unchecked, identity)
    with pytest.raises(ValidationError):
        copy_after_snapshot(source, identity.model_copy(update={"material_id": "invalid-material"}))


def test_clear_has_no_source_dependency_and_resets_all_36_targets_to_canonical_empty() -> None:
    project = _mixed_project()
    snapshot = capture_bank_snapshot(project, None, 1)
    project.pad_gain_db[0] = -20.0
    project.pad_gain_db[108] = -11.0
    project.volume = 0.3
    project.selected_pad = 71
    before = project.model_dump_json()

    candidate = project_with_bank_snapshot(project, snapshot)

    assert all(row.source_id is None and row.source is None for row in snapshot.projected().slots)
    for target in range(36, 72):
        assert musical_content_data(candidate, target) == musical_content_data(
            ProjectState(), target
        )
    assert candidate.pad_gain_db[0] == -20.0
    assert candidate.pad_gain_db[108] == -11.0
    assert candidate.volume == 0.3
    assert candidate.selected_pad == 71
    assert candidate.pad_key_intent[36] is not candidate.pad_key_intent[37]
    assert project.model_dump_json() == before


def test_latest_outside_globals_selection_and_input_setting_survive_projection() -> None:
    project = _mixed_project()
    snapshot = capture_bank_snapshot(project, 0, 1)
    project.pad_gain_db[108] = -22.0
    project.volume = 0.4
    project.speed = 0.9
    project.input_mapping_enabled = True
    project.demucs_shifts = 2
    project.selected_pad = 108
    project.selected_bank = 3
    project.config_revision = 42
    before = project.model_dump(mode="json", exclude_computed_fields=True)

    candidate = project_with_bank_snapshot(project, snapshot)
    after = candidate.model_dump(mode="json", exclude_computed_fields=True)

    content_fields = {field for _, field in CONTENT_FIELDS}
    for field, value in before.items():
        if field in content_fields:
            assert after[field][:36] == value[:36]
            assert after[field][72:] == value[72:]
        else:
            assert after[field] == value
    assert candidate.input_mapping_enabled is True
    # Physical MIDI bindings are not part of ProjectState's musical field table.
    # Their genuine ownership is verified by the separate Native/controller composition.
    payload = json.loads(snapshot.payload_json)
    for row in payload["slots"]:
        assert set(row["before"]) == {
            name
            for name, _ in CONTENT_FIELDS
            if name not in {"source_beatmap_intent", "source_sync_intent"}
        }
        assert not any("midi" in name or "binding" in name for name in row["after"])
    assert project.model_dump(mode="json", exclude_computed_fields=True) == before


@pytest.mark.parametrize("participant", [35, 71])
def test_stale_36th_participant_rejects_whole_projection_without_earlier_target_mutation(
    participant: int,
) -> None:
    project = _mixed_project()
    snapshot = capture_bank_snapshot(project, 0, 1)
    project.pad_gain_db[participant] = -29.0
    before = project.model_dump_json()

    with pytest.raises(ValueError, match="participant musical content changed"):
        project_with_bank_snapshot(project, snapshot)

    assert project.model_dump_json() == before
    assert project.pad_gain_db[36] == -7.5
    assert project.pad_content[37] is not None


def test_last_empty_source_witness_cannot_be_filled_before_projection() -> None:
    project = _full_project()
    snapshot = capture_bank_snapshot(project, 0, 1)
    project = _place(project, 35)
    before = project.model_dump_json()

    with pytest.raises(ValueError, match="participant musical content changed"):
        project_with_bank_snapshot(project, snapshot)

    assert project.model_dump_json() == before
    assert all(content is None for content in project.pad_content[36:72])


def test_inherited_key_lock_is_fenced_but_explicit_override_preserves_latest_global() -> None:
    project = _mixed_project()
    inherited = capture_bank_snapshot(project, 0, 1)
    project.key_lock = False
    before = project.model_dump_json()
    with pytest.raises(ValueError, match="inheritance changed"):
        project_with_bank_snapshot(project, inherited)
    assert project.model_dump_json() == before

    project.pad_key_lock[0] = True
    explicit = capture_bank_snapshot(project, 0, 1)
    project.key_lock = True
    candidate = project_with_bank_snapshot(project, explicit)
    assert candidate.key_lock is True
    assert candidate.pad_key_lock[36] is True
    assert candidate.pad_key_lock[71] is False


def test_stem_availability_never_becomes_a_copied_current_acceptance_claim() -> None:
    project = _mixed_project()
    snapshot = capture_bank_snapshot(project, 0, 1)
    assert project.stem_cache[0] is not None
    project.stem_cache[0].available = False

    candidate = project_with_bank_snapshot(project, snapshot)

    assert candidate.stem_cache[36] is not None
    assert candidate.stem_cache[36].available is False
    assert candidate.stem_cache[0] is not project.stem_cache[0]
    source_stems = candidate.stem_cache[0]
    assert source_stems is not None
    assert source_stems.available is False


def test_each_seal_projection_and_candidate_detach_nested_musical_children() -> None:
    project = _mixed_project()
    snapshot = capture_bank_snapshot(project, 0, 1)
    sealed = snapshot.payload_json
    first = snapshot.projected()
    assert first.slots[0].source is not None
    assert first.slots[0].source.analysis is not None
    first.slots[0].source.analysis.beat_grid.beats.append(91.0)
    assert first.slots[0].after.analysis is not None
    first.slots[0].after.analysis.beat_grid.bars.append(92.0)
    candidate = project_with_bank_snapshot(project, snapshot)
    assert candidate.sample_analysis[36] is not None
    candidate.sample_analysis[36].beat_grid.beats.append(93.0)
    assert candidate.stem_cache[36] is not None
    candidate.stem_cache[36].stems.drums = "changed independent projection.wav"

    fresh = snapshot.projected()

    assert snapshot.payload_json == sealed
    assert fresh.slots[0].after.analysis is not None
    assert fresh.slots[0].after.analysis.beat_grid.beats == [0.25, 0.75, 1.25]
    assert fresh.slots[0].after.analysis.accepted_timing == _historical_timing()
    assert candidate.sample_analysis[0] == project.sample_analysis[0]
    assert candidate.pad_key_intent[36] is not candidate.pad_key_intent[0]
    assert candidate.pad_stem_intent[36] is not candidate.pad_stem_intent[0]
    assert project.stem_cache[0] is not None
    assert project.stem_cache[0].stems.drums != "changed independent projection.wav"


@pytest.mark.parametrize("size", [35, 37])
def test_missing_or_extra_bank_rows_are_rejected(size: int) -> None:
    payload = json.loads(capture_bank_snapshot(_mixed_project(), 0, 1).payload_json)
    payload["slots"] = (
        payload["slots"][:size] if size < 36 else [*payload["slots"], payload["slots"][-1]]
    )
    with pytest.raises(ValidationError):
        BankSnapshot(payload_json=json.dumps(payload))


@pytest.mark.parametrize("change", ["reordered", "duplicate", "foreign_target", "foreign_source"])
def test_reordered_duplicate_and_foreign_slot_ids_cannot_form_a_bank(change: str) -> None:
    payload = json.loads(capture_bank_snapshot(_mixed_project(), 0, 1).payload_json)
    if change == "reordered":
        payload["slots"][0], payload["slots"][35] = payload["slots"][35], payload["slots"][0]
    elif change == "duplicate":
        payload["slots"][35]["target_id"] = 36
    elif change == "foreign_target":
        payload["slots"][35]["target_id"] = 108
    else:
        payload["slots"][35]["source_id"] = 108
    with pytest.raises(ValidationError):
        BankSnapshot(payload_json=json.dumps(payload))


@pytest.mark.parametrize(("source", "target"), [(0, 0), (True, 1), (0, False), (-1, 1), (0, 6)])
def test_same_bank_and_noncanonical_bank_indexes_are_rejected(source: int, target: int) -> None:
    with pytest.raises(ValueError, match=r"Bank Copy requires|Bank index must"):
        capture_bank_snapshot(_mixed_project(), source, target)


@pytest.mark.parametrize("image", ["source", "before", "after"])
def test_nested_unknown_musical_schema_is_not_silently_discarded(image: str) -> None:
    payload = json.loads(capture_bank_snapshot(_mixed_project(), 0, 1).payload_json)
    payload["slots"][0][image]["analysis"]["unknown_bank_field"] = 1
    with pytest.raises(ValidationError):
        BankSnapshot(payload_json=json.dumps(payload))


def test_noncanonical_empty_after_image_and_current_availability_claim_are_rejected() -> None:
    payload = json.loads(capture_bank_snapshot(_mixed_project(), 0, 1).payload_json)
    payload["slots"][1]["after"]["gain_db"] = -1.0
    with pytest.raises(ValidationError):
        BankSnapshot(payload_json=json.dumps(payload))
    payload = json.loads(capture_bank_snapshot(_mixed_project(), 0, 1).payload_json)
    payload["slots"][0]["after"]["stem_cache"]["available"] = True
    with pytest.raises(ValidationError):
        BankSnapshot(payload_json=json.dumps(payload))


def test_duplicate_original_and_after_identities_are_rejected() -> None:
    project = _mixed_project()
    payload = json.loads(capture_bank_snapshot(project, 0, 1).payload_json)
    payload["slots"][35]["before"]["source_identity"]["instance_id"] = payload["slots"][0][
        "before"
    ]["source_identity"]["instance_id"]
    with pytest.raises(ValidationError):
        BankSnapshot(payload_json=json.dumps(payload))
    payload = json.loads(capture_bank_snapshot(project, 0, 1).payload_json)
    payload["slots"][35]["after"]["source_identity"]["instance_id"] = payload["slots"][0]["after"][
        "source_identity"
    ]["instance_id"]
    with pytest.raises(ValidationError):
        BankSnapshot(payload_json=json.dumps(payload))


def test_target_identity_must_still_be_fresh_against_latest_outside_content() -> None:
    project = _mixed_project()
    snapshot = capture_bank_snapshot(project, 0, 1)
    fresh = snapshot.projected().slots[0].after.source_identity
    assert fresh is not None
    project.pad_content[108] = fresh
    before = project.model_dump_json()
    with pytest.raises(ValueError, match="no longer fresh"):
        project_with_bank_snapshot(project, snapshot)
    assert project.model_dump_json() == before


def test_unchecked_wrappers_and_future_schema_versions_are_revalidated() -> None:
    project = _mixed_project()
    snapshot = capture_bank_snapshot(project, 0, 1)
    invalid = BankSnapshot.model_construct(schema_version=2, payload_json=snapshot.payload_json)
    with pytest.raises(ValidationError):
        project_with_bank_snapshot(project, invalid)
    payload = json.loads(snapshot.payload_json)
    payload["slots"].pop()
    unchecked = BankSnapshot.model_construct(schema_version=1, payload_json=json.dumps(payload))
    with pytest.raises(ValidationError):
        unchecked.projected()
    data = snapshot.projected()
    invalid_data = BankSnapshotData.model_construct(
        source_bank=data.source_bank, target_bank=data.target_bank, slots=data.slots[:-1]
    )
    with pytest.raises(ValidationError):
        BankSnapshot.from_data(invalid_data)


def test_json_spelling_is_canonical_and_occupied_after_cannot_change_music_or_lineage() -> None:
    snapshot = capture_bank_snapshot(_mixed_project(), 0, 1)
    payload = json.loads(snapshot.payload_json)
    assert BankSnapshot(payload_json=json.dumps(payload, indent=4)) == snapshot
    payload["slots"][0]["after"]["loop_start_s"] = 3.0
    with pytest.raises(ValidationError):
        BankSnapshot(payload_json=json.dumps(payload))
    payload = json.loads(snapshot.payload_json)
    payload["slots"][0]["after"]["source_identity"]["copied_from"] = "9" * 32
    with pytest.raises(ValidationError):
        BankSnapshot(payload_json=json.dumps(payload))


def test_reopened_musical_seal_needs_no_original_slot_or_artifact_lookup() -> None:
    project = _mixed_project()
    snapshot = capture_bank_snapshot(project, 0, 1)
    encoded = snapshot.model_dump_json()
    project.pad_content[0] = None
    project.sample_paths[0] = None

    reopened = BankSnapshot.model_validate_json(encoded, strict=True).projected()

    identity = reopened.slots[0].after.source_identity
    assert identity is not None
    assert identity.copied_from == "a" * 32
    assert identity.material_id == "b" * 32
    assert reopened.slots[0].after.source_path == "samples/Shared Take/Take.wav"
    # Reopening immutable music is not permission to apply against changed participants.
    with pytest.raises(ValueError, match="participant musical content changed"):
        project_with_bank_snapshot(project, snapshot)


@pytest.mark.parametrize(
    "family", ["copy", "pair_source", "pair_target", "bank_source", "bank_before", "bank_after"]
)
def test_shared_seals_reject_unknown_nested_stem_schema(family: str) -> None:
    project = _mixed_project()
    seal_type: type[CopySnapshot | PairSnapshot | BankSnapshot]
    if family == "copy":
        seal_type = CopySnapshot
        payload = json.loads(capture_copy_snapshot(project, 0).payload_json)
        image = payload
    elif family.startswith("pair_"):
        seal_type = PairSnapshot
        payload = json.loads(capture_pair_snapshot(project, 0, 36, operation="swap").payload_json)
        image = payload[family.removeprefix("pair_")]
    else:
        seal_type = BankSnapshot
        payload = json.loads(capture_bank_snapshot(project, 0, 1).payload_json)
        image = payload["slots"][0][family.removeprefix("bank_")]
    image["stem_cache"]["stems"]["future_component"] = "unrecognized.wav"
    with pytest.raises(ValidationError, match="StemFileSet contains unsupported fields"):
        seal_type(payload_json=json.dumps(payload))
