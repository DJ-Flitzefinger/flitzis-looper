"""Real migration config writes preserve current intent and serialize ordinary autosave."""

import hashlib
import json
import os
from typing import TYPE_CHECKING, cast

import pytest
from pydantic import ValidationError

from flitzis_looper.controller import persistence as persistence_module
from flitzis_looper.controller.persistence import (
    PROJECT_CONFIG_PATH,
    PersistenceFenceError,
    ProjectPersistence,
)
from flitzis_looper.controller.timing_persistence import verified_project_timing
from flitzis_looper.key_intent import MAX_KEY_EPOCH, PadKeyIntent, SourceKeyVersion
from flitzis_looper.material_migration_model import MaterialMigrationAlias
from flitzis_looper.models import BeatGrid, PadContentIdentity, ProjectState, SampleAnalysis

if TYPE_CHECKING:
    from pathlib import Path


_TRANSACTION = "1" * 32
_OTHER_TRANSACTION = "2" * 32
_MATERIAL = "a" * 32
_OLD_REFERENCE = "samples/old.wav"
_NEW_REFERENCE = f"samples/materials/M{_MATERIAL}/original/old.wav"


def _project() -> ProjectState:
    project = ProjectState(
        selected_pad=215,
        selected_bank=5,
        volume=0.6,
        stem_separator="bs-roformer:musdb18hq",
        demucs_shifts=4,
    )
    for sample_id, instance in [(0, "b" * 32), (215, "c" * 32)]:
        project.sample_paths[sample_id] = _OLD_REFERENCE
        project.pad_content[sample_id] = PadContentIdentity(instance_id=instance)
        project.pad_gain_db[sample_id] = -3.0
        project.pad_loop_start_s[sample_id] = 2.0
        project.pad_loop_end_s[sample_id] = 5.0
        project.manual_bpm[sample_id] = 123.0
        project.pad_timing_intent[sample_id] = "manual"
        project.sample_analysis[sample_id] = SampleAnalysis(
            bpm=119.0,
            key="Em" if sample_id == 0 else "Bb",
            beat_grid=BeatGrid(beats=[0.0, 0.5], downbeats=[0.0], bars=[0.0]),
        )
    project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(version=MAX_KEY_EPOCH, raw_key="Em"),
        correction="Cm",
        analysis_epoch=MAX_KEY_EPOCH,
        correction_epoch=MAX_KEY_EPOCH,
        base_shift=-4,
        extra_shift=12,
        retrigger=True,
    )
    project.pad_key_intent[215] = PadKeyIntent(
        source=SourceKeyVersion(version=37, raw_key="Bb"),
        correction="legacy arbitrary key ♭",
        analysis_epoch=7,
        correction_epoch=11,
        base_shift=6,
        extra_shift=-18,
        retrigger=False,
    )
    return project


def _alias() -> MaterialMigrationAlias:
    return MaterialMigrationAlias(
        transaction_id=_TRANSACTION,
        material_id=_MATERIAL,
        old_reference=_OLD_REFERENCE,
        new_reference=_NEW_REFERENCE,
        original_sha256="d" * 64,
        original_bytes=8192,
        decoder_identity="e" * 64,
        playback_identity="f" * 64,
        cache_path=f"samples/materials/M{_MATERIAL}/.pcm/.ready-{'3' * 32}",
        old_source_version="verified-old-lineage",
        new_source_version="verified-new-lineage",
    )


def _candidate(current: ProjectState) -> ProjectState:
    candidate = current.model_copy(deep=True)
    for sample_id in [0, 215]:
        if candidate.sample_paths[sample_id] != _OLD_REFERENCE:
            continue
        content = candidate.pad_content[sample_id]
        assert content is not None
        candidate.sample_paths[sample_id] = _NEW_REFERENCE
        candidate.pad_content[sample_id] = PadContentIdentity(
            instance_id=content.instance_id, material_id=_MATERIAL
        )
    candidate.material_migrations[_TRANSACTION] = _alias()
    return candidate


def _saved_persistence(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> ProjectPersistence:
    monkeypatch.chdir(tmp_path)
    persistence = ProjectPersistence(_project())
    persistence.mark_dirty()
    persistence.flush(now=0.0)
    return persistence


def test_capture_is_deep_and_reports_actual_existing_config_digest(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence = _saved_persistence(tmp_path, monkeypatch)
    original_bytes = PROJECT_CONFIG_PATH.read_bytes()
    original = persistence.project.model_copy(deep=True)

    revision, captured, digest = persistence.capture_migration(_TRANSACTION)

    assert revision == persistence.revision == original.config_revision
    assert digest == hashlib.sha256(original_bytes).hexdigest()
    assert captured == original
    assert captured is not persistence.project
    assert captured.sample_paths is not persistence.project.sample_paths
    assert captured.pad_key_intent[0] is not persistence.project.pad_key_intent[0]
    assert captured.pad_key_intent[0].source is not persistence.project.pad_key_intent[0].source
    assert captured.pad_content[215] is not persistence.project.pad_content[215]
    persistence.project.pad_gain_db[0] = -9.0
    persistence.project.pad_key_intent[215] = persistence.project.pad_key_intent[215].changed(
        extra_shift=-7
    )
    persistence.project.sample_paths[0] = "samples/replacement.wav"
    persistence.mark_dirty()
    assert captured == original
    assert PROJECT_CONFIG_PATH.read_bytes() == original_bytes
    persistence.release_migration(_TRANSACTION)


def test_capture_fences_writes_before_deep_copy_and_handles_missing_config(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    persistence = ProjectPersistence(_project())
    persistence.mark_dirty()
    model_copy = ProjectState.model_copy

    def capture_under_fence(project: ProjectState, *, deep: bool = False) -> ProjectState:
        if project is persistence.project:
            with pytest.raises(PersistenceFenceError):
                persistence.flush(now=0.0)
            assert not persistence.maybe_flush(now=100.0)
        return model_copy(project, deep=deep)

    monkeypatch.setattr(ProjectState, "model_copy", capture_under_fence)
    revision, captured, digest = persistence.capture_migration(_TRANSACTION)
    assert revision == 1
    assert captured == persistence.project
    assert digest is None
    assert not PROJECT_CONFIG_PATH.exists()
    persistence.release_migration(_TRANSACTION)


def test_only_matching_transaction_can_commit_or_release_and_autosave_stays_fenced(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence = _saved_persistence(tmp_path, monkeypatch)
    original_bytes = PROJECT_CONFIG_PATH.read_bytes()
    persistence.project.volume = 0.8
    persistence.mark_dirty()
    revision, captured, _ = persistence.capture_migration(_TRANSACTION)
    with pytest.raises(PersistenceFenceError):
        persistence.capture_migration(_OTHER_TRANSACTION)
    with pytest.raises(PersistenceFenceError):
        persistence.release_migration(_OTHER_TRANSACTION)
    with pytest.raises(PersistenceFenceError):
        persistence.commit_migration(_OTHER_TRANSACTION, revision, _candidate(captured))
    assert not persistence.maybe_flush(now=100.0)
    assert not persistence.flush_if_dirty(now=100.0)
    with pytest.raises(PersistenceFenceError):
        persistence.flush(now=100.0)
    assert PROJECT_CONFIG_PATH.read_bytes() == original_bytes
    persistence.release_migration(_TRANSACTION)
    assert persistence.flush_if_dirty(now=100.0)
    assert ProjectPersistence.from_config_path().project.volume == pytest.approx(0.8)


def test_real_migration_commit_reopens_complete_endpoint_intent_without_live_adoption(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence = _saved_persistence(tmp_path, monkeypatch)
    persistence.mark_dirty()
    revision, captured, _ = persistence.capture_migration(_TRANSACTION)
    candidate = _candidate(captured)
    live_before = persistence.project.model_dump(exclude={"config_revision"})
    candidate_before = candidate.model_dump()

    saved_revision, digest = persistence.commit_migration(_TRANSACTION, revision, candidate)

    assert saved_revision == revision
    assert digest == hashlib.sha256(PROJECT_CONFIG_PATH.read_bytes()).hexdigest()
    assert persistence.project.model_dump(exclude={"config_revision"}) == live_before
    assert candidate.model_dump() == candidate_before
    assert persistence.project.config_revision == revision
    reopened = ProjectPersistence.from_config_path().project
    assert reopened.config_revision == revision
    assert reopened.pad_key_intent == captured.pad_key_intent
    assert reopened.pad_key_intent[0].correction_epoch == MAX_KEY_EPOCH
    assert reopened.pad_key_intent[0].analysis_epoch == MAX_KEY_EPOCH
    assert reopened.pad_key_intent[0].source == captured.pad_key_intent[0].source
    assert list(reopened.manual_key) == list(captured.manual_key)
    assert reopened.sample_paths[0] == reopened.sample_paths[215] == _NEW_REFERENCE
    for sample_id in [0, 215]:
        old = captured.pad_content[sample_id]
        new = reopened.pad_content[sample_id]
        assert old is not None
        assert new is not None
        assert new.instance_id == old.instance_id
        assert new.material_id == _MATERIAL
        assert new is not old
    assert reopened.pad_content[0] != reopened.pad_content[215]
    assert reopened.material_migrations[_TRANSACTION] == _alias()
    assert reopened.sample_analysis == captured.sample_analysis
    assert reopened.manual_bpm == captured.manual_bpm
    assert reopened.pad_gain_db == captured.pad_gain_db
    assert reopened.pad_loop_start_s == captured.pad_loop_start_s
    assert reopened.pad_loop_end_s == captured.pad_loop_end_s
    assert reopened.selected_pad == 215
    assert reopened.selected_bank == 5
    assert reopened.stem_separator == captured.stem_separator
    raw = json.loads(PROJECT_CONFIG_PATH.read_text(encoding="utf-8"))
    assert raw["pad_key_intent"][0] == captured.pad_key_intent[0].model_dump(mode="json")
    assert raw["pad_key_intent"][215] == captured.pad_key_intent[215].model_dump(mode="json")
    assert "ack" not in raw["material_migrations"][_TRANSACTION]
    assert not persistence.flush_if_dirty(now=100.0)
    with pytest.raises(PersistenceFenceError):
        persistence.flush(now=100.0)
    persistence.release_migration(_TRANSACTION)
    assert not persistence.flush_if_dirty(now=100.0)


def test_stale_revision_rejects_before_write_then_current_candidate_keeps_newer_intent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence = _saved_persistence(tmp_path, monkeypatch)
    original_bytes = PROJECT_CONFIG_PATH.read_bytes()
    captured_revision, captured, _ = persistence.capture_migration(_TRANSACTION)
    persistence.project.sample_paths[0] = "samples/replacement.wav"
    persistence.project.pad_content[0] = PadContentIdentity(instance_id="4" * 32)
    persistence.project.pad_key_intent[0] = PadKeyIntent(
        source=SourceKeyVersion(version=1, raw_key="Dm"), extra_shift=2
    )
    persistence.project.pad_gain_db[215] = -9.0
    persistence.project.pad_key_intent[215] = persistence.project.pad_key_intent[215].changed(
        extra_shift=-7, retrigger=True
    )
    persistence.project.volume = 0.8
    persistence.mark_dirty()
    with pytest.raises(PersistenceFenceError, match="changed"):
        persistence.commit_migration(_TRANSACTION, captured_revision, _candidate(captured))
    assert PROJECT_CONFIG_PATH.read_bytes() == original_bytes
    current = _candidate(persistence.project)
    saved_revision, _ = persistence.commit_migration(_TRANSACTION, persistence.revision, current)
    reopened = ProjectPersistence.from_config_path().project
    assert saved_revision > captured_revision
    assert reopened.sample_paths[0] == "samples/replacement.wav"
    assert reopened.pad_content[0] == persistence.project.pad_content[0]
    assert reopened.pad_key_intent == persistence.project.pad_key_intent
    assert reopened.sample_paths[215] == _NEW_REFERENCE
    assert reopened.pad_gain_db[215] == -9.0
    assert reopened.volume == pytest.approx(0.8)
    persistence.release_migration(_TRANSACTION)


def test_change_during_actual_timing_verification_aborts_before_atomic_write(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence = _saved_persistence(tmp_path, monkeypatch)
    original_bytes = PROJECT_CONFIG_PATH.read_bytes()
    revision, captured, _ = persistence.capture_migration(_TRANSACTION)
    verify = verified_project_timing

    def verify_then_edit(project: ProjectState, audio: object) -> ProjectState:
        assert audio is None
        verified = verify(project, None)
        persistence.project.pad_key_intent[215] = persistence.project.pad_key_intent[215].changed(
            extra_shift=-7
        )
        persistence.mark_dirty()
        return verified

    monkeypatch.setattr(persistence_module, "verified_project_timing", verify_then_edit)
    with pytest.raises(PersistenceFenceError, match="during timing verification"):
        persistence.commit_migration(_TRANSACTION, revision, _candidate(captured))
    assert PROJECT_CONFIG_PATH.read_bytes() == original_bytes
    assert persistence.project.pad_key_intent[215].extra_shift == -7
    assert not persistence.flush_if_dirty(now=100.0)
    with pytest.raises(PersistenceFenceError):
        persistence.flush(now=100.0)
    monkeypatch.setattr(persistence_module, "verified_project_timing", verify)
    persistence.release_migration(_TRANSACTION)
    assert persistence.flush_if_dirty(now=100.0)
    assert ProjectPersistence.from_config_path().project.pad_key_intent[215].extra_shift == -7


def test_newer_change_during_real_atomic_write_remains_dirty_after_older_snapshot_save(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence = _saved_persistence(tmp_path, monkeypatch)
    revision, captured, _ = persistence.capture_migration(_TRANSACTION)
    write = persistence._atomic_write_text

    def write_then_edit(content: str) -> None:
        write(content)
        persistence.project.volume = 0.8
        persistence.project.pad_key_intent[215] = persistence.project.pad_key_intent[215].changed(
            extra_shift=-7
        )
        persistence.mark_dirty()

    monkeypatch.setattr(persistence, "_atomic_write_text", write_then_edit)
    saved_revision, digest = persistence.commit_migration(
        _TRANSACTION, revision, _candidate(captured)
    )
    assert saved_revision == revision
    assert persistence.revision == revision + 1
    assert digest == hashlib.sha256(PROJECT_CONFIG_PATH.read_bytes()).hexdigest()
    older = ProjectPersistence.from_config_path().project
    assert older.volume == captured.volume
    assert older.pad_key_intent[215] == captured.pad_key_intent[215]
    assert persistence.project.pad_key_intent[215].extra_shift == -7
    monkeypatch.setattr(persistence, "_atomic_write_text", write)
    persistence.release_migration(_TRANSACTION)
    assert persistence.flush_if_dirty(now=100.0), "a newer edit must still require its own write"
    newer = ProjectPersistence.from_config_path().project
    assert newer.config_revision == revision + 1
    assert newer.volume == pytest.approx(0.8)
    assert newer.pad_key_intent[215].extra_shift == -7


def test_actual_atomic_replace_failure_preserves_config_live_model_and_writer_fence(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence = _saved_persistence(tmp_path, monkeypatch)
    original_bytes = PROJECT_CONFIG_PATH.read_bytes()
    persistence.project.pad_gain_db[215] = -9.0
    persistence.mark_dirty()
    revision, captured, _ = persistence.capture_migration(_TRANSACTION)
    before = persistence.project.model_dump()

    def reject_replace(source: object, destination: object) -> None:
        assert source != destination
        assert destination == PROJECT_CONFIG_PATH
        message = "injected migration replace failure"
        raise OSError(message)

    with monkeypatch.context() as failure:
        failure.setattr(os, "replace", reject_replace)
        with pytest.raises(OSError, match="migration replace failure"):
            persistence.commit_migration(_TRANSACTION, revision, _candidate(captured))
    assert PROJECT_CONFIG_PATH.read_bytes() == original_bytes
    assert persistence.project.model_dump() == before
    assert not list(PROJECT_CONFIG_PATH.parent.glob(f".{PROJECT_CONFIG_PATH.name}.*.tmp"))
    assert not persistence.flush_if_dirty(now=100.0)
    with pytest.raises(PersistenceFenceError):
        persistence.flush(now=100.0)
    persistence.release_migration(_TRANSACTION)
    assert persistence.flush_if_dirty(now=100.0)
    assert ProjectPersistence.from_config_path().project.pad_gain_db[215] == -9.0


def test_commit_revalidates_unsafe_candidate_before_any_config_write(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence = _saved_persistence(tmp_path, monkeypatch)
    original_bytes = PROJECT_CONFIG_PATH.read_bytes()
    revision, captured, _ = persistence.capture_migration(_TRANSACTION)
    candidate = _candidate(captured)
    candidate.material_migrations[_TRANSACTION] = _alias().model_copy(
        update={"transaction_id": "invalid"}
    )
    with pytest.raises(ValidationError):
        persistence.commit_migration(_TRANSACTION, revision, candidate)
    assert PROJECT_CONFIG_PATH.read_bytes() == original_bytes
    assert persistence.project.material_migrations == {}
    with pytest.raises(PersistenceFenceError):
        persistence.flush(now=100.0)
    persistence.release_migration(_TRANSACTION)


@pytest.mark.parametrize("bad", [None, False, True, "", "A" * 32, "a" * 31, 1])
def test_invalid_transaction_ids_cannot_create_bypass_or_release_a_writer_fence(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, bad: object
) -> None:
    persistence = _saved_persistence(tmp_path, monkeypatch)
    original_bytes = PROJECT_CONFIG_PATH.read_bytes()
    before = persistence.project.model_dump()
    invalid_id = cast("str", bad)
    with pytest.raises(ValueError, match="transaction ID"):
        persistence.capture_migration(invalid_id)
    revision, captured, _ = persistence.capture_migration(_TRANSACTION)
    with pytest.raises(ValueError, match="transaction ID"):
        persistence.release_migration(invalid_id)
    with pytest.raises(ValueError, match="transaction ID"):
        persistence.commit_migration(invalid_id, revision, _candidate(captured))
    assert PROJECT_CONFIG_PATH.read_bytes() == original_bytes
    assert persistence.project.model_dump() == before
    with pytest.raises(PersistenceFenceError):
        persistence.flush(now=100.0)
    persistence.release_migration(_TRANSACTION)
    persistence.mark_dirty()
    assert persistence.flush_if_dirty(now=100.0), "invalid capture must not leak a fence"


@pytest.mark.parametrize(
    ("bad", "current_revision"),
    [(False, 0), (True, 1), (-1, 1), ("1", 1), (None, 1), (1.0, 1)],
)
def test_invalid_expected_revision_fails_before_atomic_write_even_when_equal_to_integer(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    bad: object,
    current_revision: int,
) -> None:
    monkeypatch.chdir(tmp_path)
    project = _project()
    project.config_revision = current_revision
    persistence = ProjectPersistence(project)
    persistence.flush(now=0.0)
    original_bytes = PROJECT_CONFIG_PATH.read_bytes()
    before = project.model_dump()
    revision, captured, _ = persistence.capture_migration(_TRANSACTION)
    assert revision == current_revision

    def unexpected_write(content: str) -> None:
        pytest.fail(f"an invalid revision reached the atomic writer ({len(content)} bytes)")

    monkeypatch.setattr(persistence, "_atomic_write_text", unexpected_write)
    with pytest.raises(ValueError, match="strict integer"):
        persistence.commit_migration(_TRANSACTION, cast("int", bad), _candidate(captured))
    assert PROJECT_CONFIG_PATH.read_bytes() == original_bytes
    assert project.model_dump() == before
    assert persistence.revision == current_revision
    with pytest.raises(PersistenceFenceError):
        persistence.flush(now=100.0)
    persistence.release_migration(_TRANSACTION)


@pytest.mark.parametrize(
    "bad",
    [
        {"schema_version": True},
        {"transaction_id": "A" * 32},
        {"material_id": "a" * 31},
        {"old_reference": 1},
        {"new_reference": ""},
        {"original_sha256": "d" * 63},
        {"original_bytes": True},
        {"original_bytes": 0},
        {"decoder_identity": "g" * 64},
        {"playback_identity": None},
        {"cache_path": False},
        {"old_source_version": ""},
        {"new_source_version": 9},
        {"ack": True},
        {"permit": "forged-live-authority"},
    ],
)
def test_alias_schema_rejects_invalid_evidence_and_persisted_live_tokens(
    bad: dict[str, object],
) -> None:
    invalid = _alias().model_dump() | bad
    with pytest.raises(ValidationError):
        MaterialMigrationAlias.model_validate(invalid)
    with pytest.raises(ValidationError):
        ProjectState.model_validate({"material_migrations": {_TRANSACTION: invalid}})


@pytest.mark.parametrize("malformed", ["revision_bool", "alias_value", "alias_key", "both"])
def test_new_migration_metadata_corruption_preserves_other_current_intent(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    malformed: str,
) -> None:
    monkeypatch.chdir(tmp_path)
    project = ProjectState()
    project.sample_paths[0] = "samples/performer.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="1" * 32)
    project.pad_key_intent[0] = PadKeyIntent(
        correction=None, base_shift=4, extra_shift=17, retrigger=True
    )
    project.volume = 0.42
    project.selected_pad = 215
    data = project.model_dump(mode="json")
    if malformed in {"revision_bool", "both"}:
        data["config_revision"] = True
    if malformed in {"alias_value", "both"}:
        data["material_migrations"] = {_TRANSACTION: {"ack": "historic fake permission"}}
    if malformed == "alias_key":
        data["material_migrations"] = {"wrong": _alias().model_dump(mode="json")}
    PROJECT_CONFIG_PATH.parent.mkdir()
    PROJECT_CONFIG_PATH.write_text(json.dumps(data), encoding="utf-8")
    loaded = ProjectPersistence.from_config_path(PROJECT_CONFIG_PATH)
    assert loaded.project.sample_paths == project.sample_paths
    assert loaded.project.pad_content == project.pad_content
    assert loaded.project.pad_key_intent == project.pad_key_intent
    assert loaded.project.volume == 0.42
    assert loaded.project.selected_pad == 215
    assert not loaded.project.material_migrations
    assert type(loaded.project.config_revision) is int
    assert loaded.load_error is not None
