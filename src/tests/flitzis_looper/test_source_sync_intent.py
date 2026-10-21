"""Strict request metadata, exact legacy seals and independent content projections."""

import json
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.bank_snapshot import capture_bank_snapshot, project_with_bank_snapshot
from flitzis_looper.controller.pair_arrangement_journal import validated_project_image
from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.copy_snapshot import capture_copy_snapshot, project_with_copy_snapshot
from flitzis_looper.models import ProjectState
from flitzis_looper.pair_snapshot import capture_pair_snapshot, project_with_pair_snapshot
from flitzis_looper.source_sync_intent import (
    MAX_SOURCE_SYNC_REVISION,
    PadSourceSyncIntent,
    neutral_source_sync_intent,
    next_source_sync_intent,
)
from tests.flitzis_looper.test_copy_snapshot import _full_project, _target_identity

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize(
    "data",
    [
        {},
        {"requested": False},
        {"revision": 0},
        {"requested": 1, "revision": 0},
        {"requested": True, "revision": True},
        {"requested": True, "revision": -1},
        {"requested": True, "revision": 1.0},
        {"requested": True, "revision": 1 << 64},
        {"requested": True, "revision": 1, "effective": True},
    ],
)
def test_explicit_sync_metadata_is_complete_strict_and_bounded(data: dict[str, object]) -> None:
    with pytest.raises(ValidationError):
        PadSourceSyncIntent.model_validate(data)


def test_request_revision_is_independent_checked_and_immutable() -> None:
    neutral = neutral_source_sync_intent()
    on = next_source_sync_intent(neutral, requested=True)
    off = next_source_sync_intent(on, requested=False)
    assert on == PadSourceSyncIntent(requested=True, revision=1)
    assert off == PadSourceSyncIntent(requested=False, revision=2)
    assert neutral == neutral_source_sync_intent()
    with pytest.raises(ValueError, match="capacity"):
        next_source_sync_intent(
            PadSourceSyncIntent(requested=True, revision=MAX_SOURCE_SYNC_REVISION), requested=False
        )
    with pytest.raises(ValidationError):
        on.requested = False
    rows = ProjectState().pad_source_sync_intent
    assert len(rows) == 216
    assert rows[0] is not rows[1]


def test_neutral_seal_omits_only_neutral_and_copied_request_remains_detached() -> None:
    project = _full_project()
    legacy = capture_copy_snapshot(project, 0)
    assert "source_sync_intent" not in json.loads(legacy.payload_json)
    requested = PadSourceSyncIntent(requested=True, revision=7)
    project.pad_source_sync_intent[0] = requested
    snapshot = capture_copy_snapshot(project, 0)
    assert json.loads(snapshot.payload_json)["source_sync_intent"] == {
        "requested": True,
        "revision": 7,
    }
    project.pad_source_sync_intent[0] = next_source_sync_intent(requested, requested=False)
    candidate = project_with_copy_snapshot(
        project, 215, snapshot, target_identity=_target_identity(snapshot)
    )
    assert candidate.pad_source_sync_intent[215] == requested
    assert candidate.pad_source_sync_intent[215] is not requested
    assert candidate.pad_source_sync_intent[0].revision == 8
    assert json.loads(capture_copy_snapshot(candidate, 0).payload_json)["source_sync_intent"] == {
        "requested": False,
        "revision": 8,
    }
    assert capture_copy_snapshot(_full_project(), 0).payload_json == legacy.payload_json


def test_move_bank_copy_and_clear_follow_content_without_shared_mutable_intent() -> None:
    project = _full_project()
    requested = PadSourceSyncIntent(requested=True, revision=5)
    project.pad_source_sync_intent[0] = requested
    moved = project_with_pair_snapshot(
        project, capture_pair_snapshot(project, 0, 215, operation="move")
    )
    assert moved.pad_source_sync_intent[0] == neutral_source_sync_intent()
    assert moved.pad_source_sync_intent[215] == requested
    copied = project_with_bank_snapshot(project, capture_bank_snapshot(project, 0, 1))
    assert copied.pad_source_sync_intent[36] == requested
    assert copied.pad_source_sync_intent[36] is not requested
    cleared = project_with_bank_snapshot(copied, capture_bank_snapshot(copied, None, 1))
    assert cleared.pad_source_sync_intent[36:72] == [neutral_source_sync_intent()] * 36
    assert cleared.pad_source_sync_intent[0] == requested


def test_legacy_project_table_omission_preserves_text_and_rejects_unknown_keys() -> None:
    data = _full_project().model_dump(mode="json")
    data.pop("pad_source_sync_intent")
    text = json.dumps(data, indent=3)
    assert (
        validated_project_image(text).pad_source_sync_intent == [neutral_source_sync_intent()] * 216
    )
    assert json.dumps(data, indent=3) == text
    with pytest.raises(ValueError, match="unknown"):
        validated_project_image(json.dumps(data | {"source_sync_effective": True}))


def test_malformed_sync_row_preserves_other_rows_and_unsafe_save_fence(tmp_path: Path) -> None:
    project = ProjectState(volume=0.61)
    project.pad_source_sync_intent[1] = PadSourceSyncIntent(requested=True, revision=8)
    data = project.model_dump(mode="json")
    data["pad_source_sync_intent"][0] = {"requested": True, "revision": True}
    config = tmp_path / "owned.config.json"
    original = json.dumps(data, indent=3).encode()
    config.write_bytes(original)
    persistence = ProjectPersistence.from_config_path(config)
    persistence.config_path = config
    assert persistence.project.volume == 0.61
    assert persistence.project.pad_source_sync_intent[0] == neutral_source_sync_intent()
    assert persistence.project.pad_source_sync_intent[1] == project.pad_source_sync_intent[1]
    assert persistence.load_error is not None
    persistence.mark_dirty()
    with pytest.raises(PersistenceFenceError):
        persistence.flush()
    assert config.read_bytes() == original
