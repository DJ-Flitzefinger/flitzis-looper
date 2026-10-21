"""Actual compact Config writes and malformed-metadata recovery; no Native proof claim."""

import json
import os
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.models import ProjectState
from flitzis_looper.source_beatmap_intent import (
    PadSourceBeatmapIntent,
    SourceBeatmapReference,
    next_source_beatmap_intent,
)

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path


def reference(generation: str = "a") -> SourceBeatmapReference:
    return SourceBeatmapReference(
        schema_version=1,
        kind="source-beatmap-v1",
        reference=f"samples/.pcm-cache/source-beatmaps/v1/.ready-{generation * 32}/map.json",
        bytes=2048,
        sha256=generation * 64,
        map_revision="b" * 64,
        decoder_identity="c" * 64,
    )


def test_commit_writes_detached_selection_before_live_model_exchange(tmp_path: Path) -> None:
    project = ProjectState(volume=0.71)
    persistence = ProjectPersistence(project)
    persistence.config_path = tmp_path / "samples" / "owned.config.json"
    persistence.flush()
    selected = next_source_beatmap_intent(None, reference())
    persistence.commit_source_beatmap_selection(0, None, selected, lambda: True)
    assert project.pad_source_beatmap_intent[0] is None
    reopened = ProjectPersistence.from_config_path(persistence.config_path)
    assert reopened.project.pad_source_beatmap_intent[0] == selected
    assert reopened.project.volume == 0.71
    assert reopened.project.sample_analysis == project.sample_analysis
    assert reopened.load_error is None


@pytest.mark.parametrize("failure", ["replace", "stale", "revision", "migration"])
def test_precommit_faults_preserve_original_bytes_and_selection(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    failure: str,
) -> None:
    previous = next_source_beatmap_intent(None, reference())
    project = ProjectState()
    project.pad_source_beatmap_intent[0] = previous
    persistence = ProjectPersistence(project)
    persistence.config_path = tmp_path / "owned.config.json"
    persistence.flush()
    before = persistence.config_path.read_bytes()
    original_write = persistence._atomic_write_text

    def fail_replace(*_args: object) -> None:
        message = "injected pre-replacement failure"
        raise OSError(message)

    def mutate_before_replace(
        text: str,
        *,
        before_replace: Callable[[], None] | None = None,
    ) -> None:
        persistence.mark_dirty()
        original_write(text, before_replace=before_replace)

    if failure == "replace":
        monkeypatch.setattr(os, "replace", fail_replace)
    elif failure == "revision":
        monkeypatch.setattr(persistence, "_atomic_write_text", mutate_before_replace)
    elif failure == "migration":
        persistence.capture_migration("d" * 32)
    with pytest.raises((OSError, PersistenceFenceError)):
        persistence.commit_source_beatmap_selection(
            0,
            previous,
            next_source_beatmap_intent(previous, None),
            lambda: failure != "stale",
        )
    assert persistence.config_path.read_bytes() == before
    assert project.pad_source_beatmap_intent[0] is previous
    assert not tuple(tmp_path.glob(".*.tmp"))


def test_malformed_map_rows_preserve_other_settings_and_fence_original_bytes(
    tmp_path: Path,
) -> None:
    good = next_source_beatmap_intent(None, reference()).model_dump(mode="json")
    data = ProjectState(volume=0.63).model_dump(mode="json")
    data["pad_source_beatmap_intent"][1] = good
    data["pad_source_beatmap_intent"][0] = good | {"revision": True}
    config = tmp_path / "owned.config.json"
    original = json.dumps(data, indent=3).encode()
    config.write_bytes(original)
    persistence = ProjectPersistence.from_config_path(config)
    persistence.config_path = config
    assert persistence.project.volume == 0.63
    assert persistence.project.pad_source_beatmap_intent[0] is None
    expected = PadSourceBeatmapIntent.model_validate(good)
    assert persistence.project.pad_source_beatmap_intent[1] == expected
    assert "source beatmap metadata" in persistence.load_error
    persistence.mark_dirty()
    with pytest.raises(PersistenceFenceError):
        persistence.flush()
    assert config.read_bytes() == original


def test_legacy_absent_table_restores_neutral_without_rewriting_input(tmp_path: Path) -> None:
    data = ProjectState(volume=0.42).model_dump(mode="json")
    data.pop("pad_source_beatmap_intent")
    config = tmp_path / "owned.config.json"
    original = json.dumps(data).encode()
    config.write_bytes(original)
    persistence = ProjectPersistence.from_config_path(config)
    assert persistence.load_error is None
    assert persistence.project.pad_source_beatmap_intent == [None] * 216
    assert persistence.project.volume == 0.42
    assert config.read_bytes() == original
