"""Sealed complete musical Copy data; current Native acceptance is a separate gate."""

import json
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.accepted_timing import PersistedAcceptedTiming
from flitzis_looper.copy_snapshot import (
    CopySnapshot,
    capture_copy_snapshot,
    project_with_copy_snapshot,
)
from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
from flitzis_looper.material_migration_model import MaterialCreationReceipt, MigrationFileEvidence
from flitzis_looper.models import (
    BeatGrid,
    PadContentIdentity,
    ProjectState,
    SampleAnalysis,
    StemCacheEntry,
    StemFileSet,
)
from flitzis_looper.stem_intent import (
    STEM_COMPONENT_MASK,
    STEM_INSTRUMENTAL_PRESET_MASK,
    PadStemIntent,
    StemMaskDisplayMode,
)
from flitzis_looper.stem_pair_selection import STEM_FINGERPRINT_SCHEMA, StemPairSelection

if TYPE_CHECKING:
    from pathlib import Path


def _historical_timing() -> PersistedAcceptedTiming:
    return PersistedAcceptedTiming(
        schema_version=1,
        encoding="accepted-constant-timing-qm-raw-v1",
        record={
            "fixture": "historical metadata; the native codec must verify the full record",
            "period_bits": "3fe0000000000001",
            "origin": {"seconds_bits": "8000000000000000"},
            "raw": {"beat_frames_bits": ["0000000000000000", "3ff0000000000001"]},
        },
    )


def _full_project() -> ProjectState:
    project = ProjectState(
        config_revision=17,
        selected_pad=215,
        selected_bank=5,
        key_lock=True,
        bpm_lock=True,
        multi_loop=True,
        trigger_quantization_enabled=True,
        trigger_quantization_step="1_16",
        input_mapping_enabled=False,
        stem_separator="bs-roformer:musdb18hq",
        demucs_shifts=4,
        volume=0.6,
        speed=1.25,
        sidebar_left_expanded=False,
    )
    root = "samples/Shared Take"
    birth = MaterialCreationReceipt(
        root_reference=root,
        root_identity=(1, 2, 3),
        binding=MigrationFileEvidence(
            name=".material.json", identity=(1, 4, 5), bytes=10, sha256="0" * 64
        ),
    )
    project.pad_content[0] = PadContentIdentity(
        instance_id="a" * 32, material_id="b" * 32, material_creation=birth
    )
    project.sample_paths[0] = f"{root}/Take.wav"
    project.sample_durations[0] = 31.125
    project.sample_analysis[0] = SampleAnalysis(
        bpm=117.125,
        key="Bb",
        beat_grid=BeatGrid(beats=[0.25, 0.75, 1.25], downbeats=[0.25], bars=[0.25]),
        accepted_timing=_historical_timing(),
    )
    pair = StemPairSelection(
        descriptor_reference=f"{root}/.pcm-cache/stems/v1/.pairs/{'c' * 32}.json",
        stem_set_identity="d" * 64,
        wav_generation=f"{root}/stems/.ready-{'e' * 32}",
        pcm_generation=f"{root}/.pcm-cache/stems/v1/.ready-{'f' * 32}",
    )
    project.stem_cache[0] = StemCacheEntry(
        source_version="captured-historical-source",
        cache_dir=pair.wav_generation,
        stems=StemFileSet(
            vocals=f"{pair.wav_generation}/vocals.wav",
            melody=f"{pair.wav_generation}/melody.wav",
            bass=f"{pair.wav_generation}/bass.wav",
            drums=f"{pair.wav_generation}/drums.wav",
            instrumental=f"{pair.wav_generation}/instrumental.wav",
        ),
        available=True,
        pair=pair,
        fingerprint_schema=STEM_FINGERPRINT_SCHEMA,
        generation_fingerprint="1" * 64,
        producer_identity="2" * 64,
    )
    project.pad_stem_mix_mode[0] = "all_stems"
    project.pad_stem_intent[0] = PadStemIntent(
        enabled_mask=0, last_custom_mask=0, display_mode="custom"
    )
    project.pad_key_lock[0] = None
    project.manual_bpm[0] = 123.456789
    project.pad_timing_intent[0] = "tap"
    project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(version=9, raw_key="Bb"),
        correction="F#m",
        analysis_epoch=11,
        correction_epoch=13,
        base_shift=-5,
        extra_shift=18,
        retrigger=True,
    )
    project.pad_gain_db[0] = -7.5
    project.pad_eq_low_db[0] = 3.0
    project.pad_eq_mid_db[0] = -4.0
    project.pad_eq_high_db[0] = 1.5
    project.pad_loop_start_s[0] = 2.125
    project.pad_loop_end_s[0] = 19.75
    project.pad_loop_auto[0] = True
    project.pad_loop_bars[0] = 3.5
    project.pad_grid_offset_samples[0] = -37
    project.pad_grid_anchor_s[0] = 0.25
    return project


def _target_identity(snapshot: CopySnapshot, identifier: str = "3" * 32) -> PadContentIdentity:
    source = snapshot.projected().source_identity
    return PadContentIdentity(
        instance_id=identifier,
        material_id=source.material_id,
        material_creation=source.material_creation,
        copied_from=source.instance_id,
    )


def _append_historical_frame(analysis: SampleAnalysis) -> None:
    assert analysis.accepted_timing is not None
    raw = analysis.accepted_timing.record["raw"]
    assert isinstance(raw, dict)
    frames = raw["beat_frames_bits"]
    assert isinstance(frames, list)
    frames.append("4000000000000000")


def test_full_non_neutral_snapshot_and_actual_json_reopen_preserve_all_content(
    tmp_path: Path,
) -> None:
    project = _full_project()
    before = project.model_dump_json()
    snapshot = capture_copy_snapshot(project, 0)
    path = tmp_path / "copy.json"
    path.write_text(snapshot.model_dump_json(), encoding="utf-8")

    reopened = CopySnapshot.model_validate_json(path.read_text(encoding="utf-8"))
    captured = reopened.projected()
    candidate = project_with_copy_snapshot(
        project, 215, reopened, target_identity=_target_identity(reopened)
    )

    assert reopened == snapshot
    assert set(json.loads(path.read_text(encoding="utf-8"))) == {"schema_version", "payload_json"}
    assert captured.source_path == project.sample_paths[0]
    assert captured.source_identity == project.pad_content[0]
    assert candidate.sample_paths[215] == project.sample_paths[0]
    assert candidate.sample_durations[215] == 31.125
    assert candidate.sample_analysis[215] == project.sample_analysis[0]
    assert candidate.stem_cache[215] is not None
    assert candidate.stem_cache[215].available is False
    assert captured.stem_cache is not None
    assert candidate.stem_cache[215] == captured.stem_cache
    assert candidate.pad_stem_mix_mode[215] == "all_stems"
    assert candidate.pad_stem_intent[215] == project.pad_stem_intent[0]
    assert candidate.pad_key_lock[215] is None
    assert candidate.resolve_pad_key_lock(215) is True
    assert candidate.manual_bpm[215] == project.manual_bpm[0]
    assert candidate.pad_timing_intent[215] == "tap"
    assert candidate.pad_key_intent[215] == project.pad_key_intent[0]
    assert candidate.pad_key_intent[215].retrigger is True
    assert candidate.pad_gain_db[215] == -7.5
    assert candidate.pad_eq_low_db[215] == 3.0
    assert candidate.pad_eq_mid_db[215] == -4.0
    assert candidate.pad_eq_high_db[215] == 1.5
    assert candidate.pad_loop_start_s[215] == 2.125
    assert candidate.pad_loop_end_s[215] == 19.75
    assert candidate.pad_loop_auto[215] is True
    assert candidate.pad_loop_bars[215] == 3.5
    assert candidate.pad_grid_offset_samples[215] == -37
    assert candidate.pad_grid_anchor_s[215] == 0.25
    assert candidate.pad_content[215] == _target_identity(snapshot)
    assert project.model_dump_json() == before
    assert not (tmp_path / "samples").exists()


@pytest.mark.parametrize(
    ("mask", "remembered", "display_mode"),
    [
        (0, 0, "custom"),
        (5, 5, "custom"),
        (STEM_INSTRUMENTAL_PRESET_MASK, 0, "instrumental"),
        (STEM_COMPONENT_MASK, 5, "all"),
    ],
)
def test_masks_presets_and_remembered_muted_selection_survive_copy(
    mask: int, remembered: int, display_mode: StemMaskDisplayMode
) -> None:
    project = _full_project()
    project.pad_stem_intent[0] = PadStemIntent(
        enabled_mask=mask, last_custom_mask=remembered, display_mode=display_mode
    )
    snapshot = capture_copy_snapshot(project, 0)

    candidate = project_with_copy_snapshot(
        project, 215, snapshot, target_identity=_target_identity(snapshot)
    )

    assert candidate.pad_stem_intent[215] == project.pad_stem_intent[0]
    assert candidate.pad_stem_intent[215] is not project.pad_stem_intent[0]


def test_source_and_projection_mutations_cannot_change_sealed_nested_state() -> None:
    project = _full_project()
    snapshot = capture_copy_snapshot(project, 0)
    sealed = snapshot.payload_json
    source = project.sample_analysis[0]
    assert source is not None
    source.beat_grid.beats.append(7.0)
    _append_historical_frame(source)
    stems = project.stem_cache[0]
    assert stems is not None
    stems.stems.drums = "changed-by-source.wav"
    first = snapshot.projected()
    assert first.analysis is not None
    first.analysis.beat_grid.bars.append(9.0)
    _append_historical_frame(first.analysis)
    assert first.stem_cache is not None
    first.stem_cache.stems.bass = "changed-by-projection.wav"

    second = snapshot.projected()

    assert snapshot.payload_json == sealed
    assert second is not first
    assert second.analysis is not None
    assert second.analysis.beat_grid.beats == [0.25, 0.75, 1.25]
    assert second.analysis.beat_grid.bars == [0.25]
    assert second.analysis.accepted_timing == _historical_timing()
    assert second.stem_cache is not None
    assert second.stem_cache.stems.drums != stems.stems.drums
    assert second.stem_cache.stems.bass != first.stem_cache.stems.bass


def test_two_copies_have_independent_nested_analysis_and_stem_metadata() -> None:
    project = _full_project()
    snapshot = capture_copy_snapshot(project, 0)
    first = project_with_copy_snapshot(
        project, 1, snapshot, target_identity=_target_identity(snapshot)
    )
    copied = project_with_copy_snapshot(
        first, 215, snapshot, target_identity=_target_identity(snapshot, "4" * 32)
    )
    left = copied.sample_analysis[1]
    right = copied.sample_analysis[215]
    source = copied.sample_analysis[0]
    assert left is not None
    assert right is not None
    assert source is not None
    left.beat_grid.beats.append(15.0)
    _append_historical_frame(left)
    left_stems = copied.stem_cache[1]
    right_stems = copied.stem_cache[215]
    assert left_stems is not None
    assert right_stems is not None
    left_stems.stems.vocals = "left-only.wav"

    assert right.beat_grid.beats == source.beat_grid.beats == [0.25, 0.75, 1.25]
    assert right.accepted_timing == source.accepted_timing == _historical_timing()
    assert right_stems.stems.vocals != left_stems.stems.vocals
    assert copied.pad_content[1] != copied.pad_content[215]
    assert snapshot.projected().analysis == source


def test_snapshot_applies_after_origin_removal_without_source_lookup() -> None:
    original = _full_project()
    snapshot = capture_copy_snapshot(original, 0)
    empty_project = ProjectState(key_lock=True, selected_bank=5, selected_pad=215)

    candidate = project_with_copy_snapshot(
        empty_project, 215, snapshot, target_identity=_target_identity(snapshot)
    )

    assert candidate.sample_paths[0] is None
    assert candidate.pad_content[0] is None
    assert candidate.sample_paths[215] == snapshot.projected().source_path
    assert candidate.sample_analysis[215] == original.sample_analysis[0]
    assert candidate.pad_key_intent[215] == original.pad_key_intent[0]
    assert candidate.pad_stem_intent[215] == original.pad_stem_intent[0]


def test_missing_analysis_and_prepared_stems_preserve_requested_musical_intent() -> None:
    project = _full_project()
    project.sample_analysis[0] = None
    project.sample_durations[0] = None
    project.stem_cache[0] = None
    snapshot = capture_copy_snapshot(project, 0)

    candidate = project_with_copy_snapshot(
        project, 215, snapshot, target_identity=_target_identity(snapshot)
    )

    assert candidate.sample_analysis[215] is None
    assert candidate.sample_durations[215] is None
    assert candidate.stem_cache[215] is None
    assert candidate.pad_stem_mix_mode[215] == "all_stems"
    assert candidate.pad_stem_intent[215].enabled_mask == 0
    assert candidate.pad_key_intent[215] == project.pad_key_intent[0]
    assert candidate.manual_bpm[215] == project.manual_bpm[0]
    assert candidate.pad_timing_intent[215] == "tap"


def test_copy_preserves_all_other_slots_global_settings_and_fixed_selection() -> None:
    project = _full_project()
    project.pad_gain_db[36] = -22.0
    snapshot = capture_copy_snapshot(project, 0)
    target_before = project.model_dump(mode="json", exclude_computed_fields=True)
    candidate = project_with_copy_snapshot(
        project, 215, snapshot, target_identity=_target_identity(snapshot)
    )
    after = candidate.model_dump(mode="json", exclude_computed_fields=True)

    for field, old in target_before.items():
        new = after[field]
        if isinstance(old, list):
            assert isinstance(new, list)
            assert new[:215] == old[:215]
        else:
            assert new == old
    assert candidate.selected_pad == 215
    assert candidate.selected_bank == 5
    assert candidate.config_revision == 17
    assert candidate.stem_processing_revision == 0


@pytest.mark.parametrize("field", ["schema_version", "payload_json"])
def test_snapshot_seal_cannot_be_changed_or_deleted(field: str) -> None:
    snapshot = capture_copy_snapshot(_full_project(), 0)

    with pytest.raises(ValidationError, match="frozen"):
        setattr(snapshot, field, "changed")
    with pytest.raises(ValidationError, match="frozen"):
        delattr(snapshot, field)


def test_payload_is_canonicalized_and_contains_only_complete_content_fields() -> None:
    snapshot = capture_copy_snapshot(_full_project(), 0)
    payload = json.loads(snapshot.payload_json)

    reordered = CopySnapshot(payload_json=json.dumps(payload, indent=4, sort_keys=False))

    assert reordered.payload_json == snapshot.payload_json
    assert set(payload) == {
        "source_path",
        "source_identity",
        "duration",
        "analysis",
        "stem_cache",
        "stem_mix_mode",
        "stem_intent",
        "key_lock",
        "manual_bpm",
        "timing_intent",
        "key_intent",
        "gain_db",
        "eq_low_db",
        "eq_mid_db",
        "eq_high_db",
        "loop_start_s",
        "loop_end_s",
        "loop_auto",
        "loop_bars",
        "grid_offset_samples",
        "grid_anchor_s",
    }
    assert payload["stem_cache"]["available"] is False


@pytest.mark.parametrize(
    "field", ["native_guard", "native_ack", "jobs", "voices", "slot_epoch", "pressed", "master_bpm"]
)
def test_payload_rejects_runtime_authority_and_global_fields(field: str) -> None:
    snapshot = capture_copy_snapshot(_full_project(), 0)
    payload = json.loads(snapshot.payload_json) | {field: 1}

    with pytest.raises(ValidationError, match="Extra inputs"):
        CopySnapshot(payload_json=json.dumps(payload))


@pytest.mark.parametrize("revision", [True, False, 1.0, "1", 0, 2, -1, None])
def test_snapshot_schema_rejects_coercion_and_unsupported_versions(revision: object) -> None:
    snapshot = capture_copy_snapshot(_full_project(), 0)

    with pytest.raises(ValidationError, match="schema_version"):
        CopySnapshot.model_validate({
            "schema_version": revision,
            "payload_json": snapshot.payload_json,
        })


@pytest.mark.parametrize(
    ("field", "fault"),
    [
        ("key_lock", 1),
        ("loop_auto", 1),
        ("grid_offset_samples", True),
        ("loop_bars", 1.25),
        ("manual_bpm", 0.0),
        ("gain_db", 100.0),
        ("source_path", None),
        ("source_identity", None),
    ],
)
def test_malformed_musical_payload_is_rejected_without_changing_existing_seal(
    field: str, fault: object
) -> None:
    snapshot = capture_copy_snapshot(_full_project(), 0)
    before = snapshot.payload_json
    payload = json.loads(before) | {field: fault}

    with pytest.raises(ValidationError):
        CopySnapshot(payload_json=json.dumps(payload))

    assert snapshot.payload_json == before


def test_saved_snapshot_cannot_restore_copied_stem_eligibility() -> None:
    snapshot = capture_copy_snapshot(_full_project(), 0)
    payload = json.loads(snapshot.payload_json)
    payload["stem_cache"]["available"] = True

    with pytest.raises(ValidationError, match="availability"):
        CopySnapshot(payload_json=json.dumps(payload))


def test_capture_rejects_unchecked_accepted_envelope_instead_of_silently_discarding_it() -> None:
    project = _full_project()
    analysis = project.sample_analysis[0]
    assert analysis is not None
    analysis.accepted_timing = PersistedAcceptedTiming.model_construct(
        schema_version=1, encoding="accepted-constant-timing-qm-raw-v1", record={}
    )
    before = project.model_dump_json()

    with pytest.raises(ValidationError, match=r"accepted_timing|record"):
        capture_copy_snapshot(project, 0)

    assert project.model_dump_json() == before
    assert analysis.accepted_timing is not None
    assert analysis.accepted_timing.record == {}


def test_capture_rejects_unchecked_stem_intent_and_nonfinite_nested_analysis() -> None:
    project = _full_project()
    project.pad_stem_intent[0] = PadStemIntent.model_construct(
        schema_version=1, enabled_mask=16, last_custom_mask=0, display_mode="custom"
    )

    with pytest.raises(ValidationError, match="enabled_mask"):
        capture_copy_snapshot(project, 0)

    project.pad_stem_intent[0] = PadStemIntent()
    analysis = project.sample_analysis[0]
    assert analysis is not None
    analysis.beat_grid.beats.append(float("nan"))
    with pytest.raises(ValueError, match="JSON compliant"):
        capture_copy_snapshot(project, 0)


def test_capture_revalidates_unchecked_key_intent_without_modifying_the_project() -> None:
    project = _full_project()
    project.pad_key_intent[0] = project.pad_key_intent[0].model_copy(update={"base_shift": 42})
    before = project.model_dump_json()

    with pytest.raises(ValidationError, match="base_shift"):
        capture_copy_snapshot(project, 0)

    assert project.model_dump_json() == before


@pytest.mark.parametrize("factory", ["model_copy", "model_construct"])
def test_unchecked_snapshot_wrappers_are_revalidated_before_projection_and_apply(
    factory: str,
) -> None:
    project = _full_project()
    snapshot = capture_copy_snapshot(project, 0)
    if factory == "model_copy":
        invalid = snapshot.model_copy(update={"schema_version": 2})
    else:
        invalid = CopySnapshot.model_construct(schema_version=2, payload_json=snapshot.payload_json)
    before = project.model_dump_json()

    with pytest.raises(ValidationError, match="schema_version"):
        invalid.projected()
    with pytest.raises(ValidationError, match="schema_version"):
        project_with_copy_snapshot(
            project, 215, invalid, target_identity=_target_identity(snapshot)
        )

    assert project.model_dump_json() == before


@pytest.mark.parametrize(
    "fault", ["reuse_source", "reuse_existing", "material", "birth", "lineage"]
)
def test_invalid_target_identity_is_a_complete_noop(fault: str) -> None:
    project = _full_project()
    snapshot = capture_copy_snapshot(project, 0)
    project.pad_content[1] = PadContentIdentity(instance_id="5" * 32)
    identity = _target_identity(snapshot)
    changes: dict[str, dict[str, object]] = {
        "reuse_source": {"instance_id": "a" * 32, "copied_from": None},
        "reuse_existing": {"instance_id": "5" * 32},
        "material": {"material_id": "6" * 32},
        "birth": {"material_creation": None},
        "lineage": {"copied_from": "7" * 32},
    }
    invalid = identity.model_copy(update=changes[fault])
    before = project.model_dump_json()

    with pytest.raises(ValueError, match="Copy target"):
        project_with_copy_snapshot(project, 215, snapshot, target_identity=invalid)

    assert project.model_dump_json() == before


@pytest.mark.parametrize("sample_id", [True, -1, 216])
def test_invalid_source_and_target_slots_cannot_change_the_project(sample_id: int) -> None:
    project = _full_project()
    snapshot = capture_copy_snapshot(project, 0)
    before = project.model_dump_json()

    with pytest.raises(ValueError, match="sample_id"):
        capture_copy_snapshot(project, sample_id)
    with pytest.raises(ValueError, match="sample_id"):
        project_with_copy_snapshot(
            project, sample_id, snapshot, target_identity=_target_identity(snapshot)
        )

    assert project.model_dump_json() == before


def test_empty_unidentified_and_incomplete_source_tables_are_rejected() -> None:
    project = _full_project()
    with pytest.raises(ValueError, match="loaded content"):
        capture_copy_snapshot(project, 1)
    project.pad_content[0] = None
    with pytest.raises(ValueError, match="loaded content"):
        capture_copy_snapshot(project, 0)
    project.pad_gain_db.pop()
    with pytest.raises(ValueError, match="exactly 216 slots"):
        capture_copy_snapshot(project, 0)
