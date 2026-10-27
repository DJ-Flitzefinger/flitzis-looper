"""Global timing intent migration, independent content and safe corrupt recovery."""

import hashlib
import json
from itertools import product
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.bank_snapshot import capture_bank_snapshot, project_with_bank_snapshot
from flitzis_looper.controller.pair_arrangement_journal import validated_project_image
from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.copy_snapshot import capture_copy_snapshot, project_with_copy_snapshot
from flitzis_looper.models import ProjectState
from flitzis_looper.pair_snapshot import capture_pair_snapshot, project_with_pair_snapshot
from flitzis_looper.source_sync_intent import PadSourceSyncIntent
from flitzis_looper.timing_policy import (
    MAX_TIMING_POLICY_REVISION,
    GlobalTimingPolicy,
    neutral_global_timing_policy,
    next_global_timing_policy,
    source_sync_preference,
    source_sync_requested,
    variable_source_timing,
)
from tests.flitzis_looper.test_copy_snapshot import _full_project, _target_identity

if TYPE_CHECKING:
    from pathlib import Path


def _policy(**changes: object) -> dict[str, object]:
    return {"schema_version": 1, "sync": None, "realtime_bpm": None, "revision": 0} | changes


@pytest.mark.parametrize(
    "data",
    [
        {},
        {"schema_version": 1, "sync": None, "revision": 0},
        {"sync": None, "realtime_bpm": None, "revision": 0},
        _policy(schema_version=True),
        _policy(schema_version=1.0),
        _policy(schema_version=2),
        _policy(sync=1),
        _policy(realtime_bpm="false"),
        _policy(revision=True),
        _policy(revision=-1),
        _policy(revision=1.0),
        _policy(revision=1 << 64),
        _policy(effective=True),
    ],
)
def test_global_metadata_is_complete_strict_and_never_restores_audio_ack(
    data: dict[str, object],
) -> None:
    with pytest.raises(ValidationError):
        GlobalTimingPolicy.model_validate(data)


def test_policy_changes_are_independent_checked_immutable_and_revalidated() -> None:
    neutral = neutral_global_timing_policy()
    sync = next_global_timing_policy(neutral, control="sync", enabled=True)
    fixed = next_global_timing_policy(sync, control="realtime_bpm", enabled=False)
    assert neutral.is_neutral
    assert not sync.is_neutral
    assert fixed == GlobalTimingPolicy.model_validate(
        _policy(sync=True, realtime_bpm=False, revision=2)
    )
    assert next_global_timing_policy(fixed, control="sync", enabled=True) == fixed
    with pytest.raises(ValidationError):
        fixed.sync = False
    with pytest.raises(TypeError):
        next_global_timing_policy(fixed, control="sync", enabled=1)  # type: ignore[arg-type]
    with pytest.raises(ValueError, match="capacity"):
        next_global_timing_policy(
            GlobalTimingPolicy.model_validate(_policy(revision=MAX_TIMING_POLICY_REVISION)),
            control="sync",
            enabled=False,
        )
    with pytest.raises(ValidationError):
        GlobalTimingPolicy.model_validate(
            GlobalTimingPolicy.model_construct(**_policy(sync="false"))
        )


@pytest.mark.parametrize(
    ("global_sync", "realtime", "requested", "revision"),
    list(product([None, False, True], [None, False, True], [False, True], [0, 7])),
)
def test_policy_matrix_preserves_explicit_pad_preference_and_legacy_behavior(
    *, global_sync: bool | None, realtime: bool | None, requested: bool, revision: int
) -> None:
    policy = GlobalTimingPolicy.model_validate(_policy(sync=global_sync, realtime_bpm=realtime))
    pad = PadSourceSyncIntent(requested=requested, revision=revision)
    expected_preference = global_sync is True if not requested and revision == 0 else requested
    assert source_sync_preference(policy, pad) is expected_preference
    assert source_sync_requested(policy, pad) is (expected_preference and global_sync is not False)
    assert variable_source_timing(policy) is True
    assert pad == PadSourceSyncIntent(requested=requested, revision=revision)


def test_historical_project_omission_preserves_old_pad_behavior_and_raw_seal() -> None:
    project = _full_project()
    project.multi_loop = True
    project.bpm_lock = True
    project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=4)
    data = project.model_dump(mode="json")
    data.pop("global_timing_policy")
    text = json.dumps(data, indent=3)
    digest = hashlib.sha256(text.encode()).hexdigest()
    restored = validated_project_image(text)
    assert restored.global_timing_policy.is_neutral
    assert restored.resolve_pad_source_sync(0)
    assert not restored.resolve_pad_source_sync(1)
    assert restored.pad_source_sync_intent == project.pad_source_sync_intent
    assert hashlib.sha256(text.encode()).hexdigest() == digest
    assert "global_timing_policy" not in json.loads(capture_copy_snapshot(project, 0).payload_json)


def test_copy_move_bank_and_clear_keep_current_global_choices_outside_content() -> None:
    project = _full_project()
    source_policy = GlobalTimingPolicy.model_validate(
        _policy(sync=True, realtime_bpm=True, revision=2)
    )
    project.global_timing_policy = source_policy
    project.pad_source_sync_intent[0] = PadSourceSyncIntent(requested=True, revision=5)
    copy = capture_copy_snapshot(project, 0)
    pair = capture_pair_snapshot(project, 0, 215, operation="move")
    bank = capture_bank_snapshot(project, 0, 1)
    current = next_global_timing_policy(source_policy, control="sync", enabled=False)
    project.global_timing_policy = current
    copied = project_with_copy_snapshot(project, 215, copy, target_identity=_target_identity(copy))
    moved = project_with_pair_snapshot(project, pair)
    banked = project_with_bank_snapshot(project, bank)
    cleared = project_with_bank_snapshot(banked, capture_bank_snapshot(banked, None, 1))
    for candidate in (copied, moved, banked, cleared):
        assert candidate.global_timing_policy == current
        assert not candidate.resolve_pad_source_sync(0)
    assert copied.pad_source_sync_intent[215] == project.pad_source_sync_intent[0]
    assert moved.pad_source_sync_intent[0] == PadSourceSyncIntent(requested=False, revision=0)
    assert moved.pad_source_sync_intent[215] == project.pad_source_sync_intent[0]
    assert cleared.pad_source_sync_intent[36].revision == 0


@pytest.mark.parametrize("invalid", [None, {}, _policy(schema_version=True), _policy(sync=1)])
def test_corrupt_global_policy_is_safe_off_and_retains_original_with_save_fence(
    tmp_path: Path, invalid: object
) -> None:
    project = ProjectState(volume=0.61, speed=0.93)
    project.pad_source_sync_intent[1] = PadSourceSyncIntent(requested=True, revision=8)
    data = project.model_dump(mode="json")
    data["global_timing_policy"] = invalid
    original = json.dumps(data, indent=3).encode()
    config = tmp_path / "owned.config.json"
    config.write_bytes(original)
    persistence = ProjectPersistence.from_config_path(config)
    persistence.config_path = config
    assert persistence.project.volume == 0.61
    assert persistence.project.speed == 0.93
    assert persistence.project.pad_source_sync_intent == project.pad_source_sync_intent
    assert persistence.project.global_timing_policy == GlobalTimingPolicy.model_validate(
        _policy(sync=False, realtime_bpm=False)
    )
    assert not persistence.project.resolve_pad_source_sync(1)
    assert persistence.load_error is not None
    persistence.mark_dirty()
    with pytest.raises(PersistenceFenceError):
        persistence.flush()
    assert config.read_bytes() == original


def test_invalid_pad_row_cannot_inherit_global_on_during_fenced_recovery(tmp_path: Path) -> None:
    project = ProjectState(volume=0.61, multi_loop=True, bpm_lock=True)
    project.global_timing_policy = GlobalTimingPolicy.model_validate(_policy(sync=True, revision=9))
    project.pad_source_sync_intent[1] = PadSourceSyncIntent(requested=True, revision=8)
    project.pad_source_sync_intent[2] = PadSourceSyncIntent(requested=False, revision=3)
    data = project.model_dump(mode="json")
    data["pad_source_sync_intent"][0] = {"requested": True, "revision": True}
    original = json.dumps(data).encode()
    config = tmp_path / "owned.config.json"
    config.write_bytes(original)
    persistence = ProjectPersistence.from_config_path(config)
    persistence.config_path = config
    recovered = persistence.project
    assert recovered.global_timing_policy == project.global_timing_policy
    assert recovered.pad_source_sync_intent[0] == PadSourceSyncIntent(requested=False, revision=1)
    assert not recovered.resolve_pad_source_sync(0)
    assert recovered.resolve_pad_source_sync(1)
    assert not recovered.resolve_pad_source_sync(2)
    assert recovered.resolve_pad_source_sync(3)
    assert persistence.load_error is not None
    persistence.mark_dirty()
    with pytest.raises(PersistenceFenceError):
        persistence.flush()
    assert config.read_bytes() == original
