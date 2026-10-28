"""Pure complete pair projections; Native pair ACK and asset custody remain separate."""

import builtins
import json
from typing import TYPE_CHECKING, cast

import pytest
from pydantic import ValidationError

from flitzis_looper.copy_snapshot import (
    CONTENT_FIELDS,
    CopySnapshotData,
    capture_copy_snapshot,
    musical_content_data,
    project_with_copy_snapshot,
)
from flitzis_looper.models import PadContentIdentity, ProjectState
from flitzis_looper.pair_snapshot import (
    PairOperation,
    PairSlotData,
    PairSnapshot,
    capture_pair_snapshot,
    project_with_pair_snapshot,
)
from flitzis_looper.stem_intent import PadStemIntent
from tests.flitzis_looper.test_copy_snapshot import _full_project, _historical_timing

if TYPE_CHECKING:
    from pathlib import Path


def _two_contents() -> ProjectState:
    project = _full_project()
    captured = capture_copy_snapshot(project, 0)
    source = captured.projected().source_identity
    project = project_with_copy_snapshot(
        project,
        215,
        captured,
        target_identity=PadContentIdentity(
            instance_id="3" * 32,
            material_id=source.material_id,
            material_creation=source.material_creation,
            copied_from=source.instance_id,
        ),
    )
    project.pad_gain_db[215] = -3.25
    project.pad_eq_high_db[215] = -6.0
    project.pad_loop_start_s[215] = 4.5
    project.pad_loop_end_s[215] = 25.0
    project.pad_loop_auto[215] = False
    project.pad_loop_bars[215] = 5.0
    project.pad_grid_offset_samples[215] = 77
    project.pad_key_lock[215] = False
    project.pad_stem_mix_mode[215] = "full_mix"
    project.pad_stem_intent[215] = PadStemIntent(
        enabled_mask=14, last_custom_mask=5, display_mode="instrumental"
    )
    analysis = project.sample_analysis[215]
    assert analysis is not None
    analysis.beat_grid.bars.append(8.0)
    return project


def test_pair_slots_derive_the_exact_existing_complete_copy_schema() -> None:
    assert set(PairSlotData.model_fields) == set(CopySnapshotData.model_fields)
    assert set(PairSlotData.model_fields) == {name for name, _ in CONTENT_FIELDS}
    assert len(CONTENT_FIELDS) == 21


def test_move_preserves_complete_content_identity_and_resets_independent_empty_defaults() -> None:
    project = _full_project()
    project.pad_gain_db[215] = -19.0
    before = project.model_dump_json()
    snapshot = capture_pair_snapshot(project, 0, 215, operation="move")
    source = musical_content_data(project, 0)
    identity = project.pad_content[0]

    candidate = project_with_pair_snapshot(project, snapshot)

    assert musical_content_data(candidate, 215) == source
    assert candidate.pad_content[215] == identity
    assert candidate.pad_content[215] is not identity
    assert musical_content_data(candidate, 0) == musical_content_data(ProjectState(), 0)
    assert candidate.pad_stem_intent[0] is not candidate.pad_stem_intent[1]
    assert candidate.pad_key_intent[0] is not candidate.pad_key_intent[1]
    assert candidate.stem_cache[215] is not None
    assert candidate.stem_cache[215].available is False
    assert project.model_dump_json() == before


def test_swap_exchanges_complete_images_with_unchanged_birth_and_copy_lineage() -> None:
    project = _two_contents()
    before = project.model_dump_json()
    source = musical_content_data(project, 0)
    target = musical_content_data(project, 215)
    snapshot = capture_pair_snapshot(project, 0, 215, operation="swap")

    candidate = project_with_pair_snapshot(project, snapshot)

    assert musical_content_data(candidate, 0) == target
    assert musical_content_data(candidate, 215) == source
    assert candidate.pad_content[0] == project.pad_content[215]
    assert candidate.pad_content[215] == project.pad_content[0]
    assert candidate.pad_content[0] is not None
    assert candidate.pad_content[0].copied_from == "a" * 32
    assert candidate.pad_content[0].material_creation is not None
    assert project.pad_content[215] is not None
    assert candidate.pad_content[0].material_creation == project.pad_content[215].material_creation
    assert project.model_dump_json() == before


@pytest.mark.parametrize(
    ("operation", "selected", "expected"),
    [
        ("move", 0, 215),
        ("move", 215, 215),
        ("move", 36, 36),
        ("swap", 0, 215),
        ("swap", 215, 0),
        ("swap", 36, 36),
    ],
)
def test_pair_projection_follows_selected_survivor_and_preserves_empty_slot_selection(
    operation: PairOperation, selected: int, expected: int
) -> None:
    project = _full_project() if operation == "move" else _two_contents()
    project.selected_pad = selected
    before = project.model_dump_json()
    snapshot = capture_pair_snapshot(project, 0, 215, operation=operation)
    candidate = project_with_pair_snapshot(project, snapshot)
    assert candidate.selected_pad == expected
    assert candidate.selected_bank == project.selected_bank
    assert project.model_dump_json() == before


@pytest.mark.parametrize("operation", ["move", "swap"])
def test_projection_preserves_latest_unrelated_slots_globals_and_selection(
    operation: PairOperation,
) -> None:
    project = _full_project() if operation == "move" else _two_contents()
    snapshot = capture_pair_snapshot(project, 0, 215, operation=operation)
    project.pad_gain_db[36] = -22.0
    project.pad_stem_intent[36] = PadStemIntent(enabled_mask=5, display_mode="custom")
    project.volume = 0.4
    project.speed = 0.9
    project.key_lock = False
    project.stem_separator = "demucs:htdemucs"
    project.demucs_shifts = 2
    project.selected_pad = 36
    project.selected_bank = 1
    project.config_revision = 42
    before = project.model_dump(mode="json", exclude_computed_fields=True)

    candidate = project_with_pair_snapshot(project, snapshot)
    after = candidate.model_dump(mode="json", exclude_computed_fields=True)

    for field, value in before.items():
        current = after[field]
        if isinstance(value, list):
            assert isinstance(current, list)
            assert current[1:215] == value[1:215]
        else:
            assert current == value
    assert candidate.selected_pad == 36
    assert candidate.selected_bank == 1
    assert project.model_dump(mode="json", exclude_computed_fields=True) == before


def test_pair_seal_and_each_projection_detach_nested_analysis_stems_and_intents() -> None:
    project = _two_contents()
    snapshot = capture_pair_snapshot(project, 0, 215, operation="swap")
    sealed = snapshot.payload_json
    first = snapshot.projected()
    assert first.source.analysis is not None
    first.source.analysis.beat_grid.beats.append(91.0)
    assert first.source.analysis.accepted_timing is not None
    first.source.analysis.accepted_timing.record["changed"] = ["historical-only"]
    assert first.target.stem_cache is not None
    first.target.stem_cache.stems.drums = "changed projection.wav"
    candidate = project_with_pair_snapshot(project, snapshot)
    analysis = candidate.sample_analysis[0]
    assert analysis is not None
    analysis.beat_grid.beats.append(92.0)

    fresh = snapshot.projected()

    assert snapshot.payload_json == sealed
    assert fresh.source.analysis is not None
    assert fresh.source.analysis.beat_grid.beats == [0.25, 0.75, 1.25]
    assert fresh.source.analysis.accepted_timing == _historical_timing()
    assert fresh.target.analysis is not None
    assert 92.0 not in fresh.target.analysis.beat_grid.beats
    assert fresh.target.stem_cache is not None
    assert fresh.target.stem_cache.stems.drums != "changed projection.wav"
    assert project.sample_analysis[215] == fresh.target.analysis
    assert candidate.pad_stem_intent[0] is not project.pad_stem_intent[215]


@pytest.mark.parametrize("participant", [0, 215])
@pytest.mark.parametrize("changed", ["identity", "path", "gain", "mask", "analysis", "stem_set"])
def test_changed_participant_identity_or_musical_seal_rejects_without_mutation(
    participant: int, changed: str
) -> None:
    project = _two_contents()
    snapshot = capture_pair_snapshot(project, 0, 215, operation="swap")
    if changed == "identity":
        identity = project.pad_content[participant]
        assert identity is not None
        project.pad_content[participant] = identity.model_copy(update={"instance_id": "9" * 32})
    elif changed == "path":
        project.sample_paths[participant] = "samples/changed.wav"
    elif changed == "gain":
        project.pad_gain_db[participant] = -1.0
    elif changed == "mask":
        project.pad_stem_intent[participant] = PadStemIntent(enabled_mask=6, display_mode="custom")
    elif changed == "analysis":
        analysis = project.sample_analysis[participant]
        assert analysis is not None
        analysis.beat_grid.beats.append(7.0)
    else:
        entry = project.stem_cache[participant]
        assert entry is not None
        entry.generation_fingerprint = "8" * 64
    before = project.model_dump_json()

    with pytest.raises(ValueError, match="changed"):
        project_with_pair_snapshot(project, snapshot)

    assert project.model_dump_json() == before


def test_changed_empty_target_musical_settings_and_occupancy_are_fenced() -> None:
    project = _full_project()
    snapshot = capture_pair_snapshot(project, 0, 215, operation="move")
    project.pad_gain_db[215] = -8.0
    with pytest.raises(ValueError, match="changed"):
        project_with_pair_snapshot(project, snapshot)
    project.pad_gain_db[215] = 0.0
    project.sample_paths[215] = "samples/new assignment.wav"
    project.pad_content[215] = PadContentIdentity(instance_id="7" * 32)
    before = project.model_dump_json()

    with pytest.raises(ValueError, match="empty"):
        project_with_pair_snapshot(project, snapshot)

    assert project.model_dump_json() == before


def test_move_cannot_overwrite_an_occupied_target() -> None:
    project = _two_contents()
    before = project.model_dump_json()

    with pytest.raises(ValueError, match="Move target must be empty"):
        capture_pair_snapshot(project, 0, 215, operation="move")

    assert project.model_dump_json() == before


@pytest.mark.parametrize("participant", [0, 215])
def test_swap_rejects_a_participant_that_became_empty(participant: int) -> None:
    project = _two_contents()
    snapshot = capture_pair_snapshot(project, 0, 215, operation="swap")
    project.sample_paths[participant] = None
    project.pad_content[participant] = None
    before = project.model_dump_json()

    with pytest.raises(ValueError, match=r"Pair source|Swap target"):
        project_with_pair_snapshot(project, snapshot)

    assert project.model_dump_json() == before


@pytest.mark.parametrize("occupied", [False, True])
def test_runtime_stem_availability_neither_changes_the_musical_seal_nor_transfers_permission(
    *, occupied: bool
) -> None:
    project = _two_contents() if occupied else _full_project()
    operation: PairOperation = "swap" if occupied else "move"
    snapshot = capture_pair_snapshot(project, 0, 215, operation=operation)
    entry = project.stem_cache[0]
    assert entry is not None
    entry.available = not entry.available

    candidate = project_with_pair_snapshot(project, snapshot)

    assert candidate.stem_cache[215] is not None
    assert candidate.stem_cache[215].available is False
    if occupied:
        assert candidate.stem_cache[0] is not None
        assert candidate.stem_cache[0].available is False


@pytest.mark.parametrize("identifier", [True, False, -1, 216, 1.0, "0"])
@pytest.mark.parametrize("participant", ["source", "target"])
def test_strict_invalid_participant_slots_cannot_capture_or_change_a_project(
    identifier: object, participant: str
) -> None:
    project = _full_project()
    before = project.model_dump_json()
    source = cast("int", identifier) if participant == "source" else 0
    target = cast("int", identifier) if participant == "target" else 215

    with pytest.raises(ValueError, match="sample_id"):
        capture_pair_snapshot(project, source, target, operation="move")

    assert project.model_dump_json() == before


@pytest.mark.parametrize(
    ("source", "target", "operation"),
    [(0, 0, "swap"), (215, 0, "move"), (0, 215, "swap"), (0, 0, "move")],
)
def test_self_empty_source_and_wrong_target_occupancy_reject(
    source: int, target: int, operation: PairOperation
) -> None:
    project = _full_project()
    before = project.model_dump_json()

    with pytest.raises(ValueError, match=r"Pair|Move|Swap"):
        capture_pair_snapshot(project, source, target, operation=operation)

    assert project.model_dump_json() == before


@pytest.mark.parametrize("when", ["capture", "projection"])
def test_duplicate_content_uuid_anywhere_in_latest_project_rejects(when: str) -> None:
    project = _full_project()
    snapshot = capture_pair_snapshot(project, 0, 215, operation="move")
    project.pad_content[36] = project.pad_content[0]
    before = project.model_dump_json()

    if when == "capture":
        with pytest.raises(ValueError, match="duplicate"):
            capture_pair_snapshot(project, 0, 215, operation="move")
    else:
        with pytest.raises(ValueError, match="duplicate"):
            project_with_pair_snapshot(project, snapshot)

    assert project.model_dump_json() == before


@pytest.mark.parametrize("field", ["native_ack", "voices", "holds", "jobs", "runtime_lifetime"])
def test_pair_payload_rejects_runtime_tokens_and_unrequested_authority(field: str) -> None:
    snapshot = capture_pair_snapshot(_full_project(), 0, 215, operation="move")
    payload = json.loads(snapshot.payload_json)
    payload["source"][field] = 1

    with pytest.raises(ValidationError, match="Extra inputs"):
        PairSnapshot(payload_json=json.dumps(payload))


@pytest.mark.parametrize("revision", [True, "1", 1.0, 0, 2])
def test_unchecked_pair_wrapper_is_revalidated_before_projection_or_apply(revision: object) -> None:
    project = _full_project()
    snapshot = capture_pair_snapshot(project, 0, 215, operation="move")
    unchecked = snapshot.model_copy(update={"schema_version": revision})
    before = project.model_dump_json()

    with pytest.raises(ValidationError, match="schema_version"):
        unchecked.projected()
    with pytest.raises(ValidationError, match="schema_version"):
        project_with_pair_snapshot(project, unchecked)

    assert project.model_dump_json() == before


def test_pair_rejects_coerced_slots_in_payload_and_unclaimed_availability() -> None:
    snapshot = capture_pair_snapshot(_full_project(), 0, 215, operation="move")
    payload = json.loads(snapshot.payload_json)
    payload["source_id"] = False
    with pytest.raises(ValidationError, match="source_id"):
        PairSnapshot(payload_json=json.dumps(payload))
    payload["source_id"] = 0
    payload["source"]["stem_cache"]["available"] = True
    with pytest.raises(ValidationError, match="availability"):
        PairSnapshot(payload_json=json.dumps(payload))


def test_capture_rejects_inconsistent_occupancy_and_incomplete_musical_tables() -> None:
    project = _full_project()
    project.pad_content[215] = PadContentIdentity(instance_id="3" * 32)
    with pytest.raises(ValidationError, match="occupancy"):
        capture_pair_snapshot(project, 0, 215, operation="move")
    project.pad_content[215] = None
    project.pad_gain_db.pop()
    with pytest.raises(ValueError, match="exactly 216"):
        capture_pair_snapshot(project, 0, 215, operation="move")


def test_pair_roundtrip_is_canonical_and_functions_perform_no_file_io(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    project = _two_contents()

    def reject_io(*_args: object, **_kwargs: object) -> None:
        pytest.fail("pure pair snapshot attempted file I/O")

    monkeypatch.setattr(builtins, "open", reject_io)
    monkeypatch.setattr(type(tmp_path), "open", reject_io)
    snapshot = capture_pair_snapshot(project, 0, 215, operation="swap")
    reopened = PairSnapshot.model_validate_json(snapshot.model_dump_json(), strict=True)
    candidate = project_with_pair_snapshot(project, reopened)

    assert reopened == snapshot
    assert candidate.pad_content[0] == project.pad_content[215]
    assert not (tmp_path / "samples").exists()
    assert set(json.loads(snapshot.model_dump_json())) == {"schema_version", "payload_json"}


@pytest.mark.parametrize("field", ["schema_version", "payload_json"])
def test_pair_seal_cannot_be_assigned_or_deleted(field: str) -> None:
    snapshot = capture_pair_snapshot(_full_project(), 0, 215, operation="move")

    with pytest.raises(ValidationError, match="frozen"):
        setattr(snapshot, field, "changed")
    with pytest.raises(ValidationError, match="frozen"):
        delattr(snapshot, field)
