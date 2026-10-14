"""Malformed new selection metadata never discards unrelated performer settings."""

import json
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import ProjectState
from flitzis_looper.stem_intent import PadStemIntent

if TYPE_CHECKING:
    from pathlib import Path


def _configured_project() -> ProjectState:
    project = ProjectState()
    project.volume = 0.37
    project.pad_gain_db[36] = -7.0
    project.pad_stem_mix_mode[36] = "all_stems"
    project.pad_stem_intent[36] = PadStemIntent(
        enabled_mask=0, last_custom_mask=5, display_mode="custom"
    )
    project.pad_stem_intent[215] = PadStemIntent(
        enabled_mask=14, last_custom_mask=3, display_mode="instrumental"
    )
    return project


def test_missing_legacy_table_preserves_other_intent_and_defaults_selection(tmp_path: Path) -> None:
    project = _configured_project()
    raw = project.model_dump()
    raw.pop("pad_stem_intent")
    config = tmp_path / "legacy.json"
    config.write_text(json.dumps(raw), encoding="utf-8")

    restored = ProjectPersistence.from_config_path(config)
    assert restored.load_error is None
    assert restored.project.volume == project.volume
    assert restored.project.pad_gain_db[36] == -7.0
    assert restored.project.pad_stem_mix_mode[36] == "all_stems"
    assert all(value == PadStemIntent() for value in restored.project.pad_stem_intent)


@pytest.mark.parametrize(
    "malformed",
    [
        {"enabled_mask": True},
        {"enabled_mask": 16},
        {"schema_version": 2},
        {"enabled_mask": 0, "display_mode": "all"},
        {"source_generation": 44},
    ],
)
def test_one_rejected_selection_preserves_valid_slots_settings_and_original_config(
    tmp_path: Path, malformed: dict[str, object]
) -> None:
    project = _configured_project()
    raw = project.model_dump()
    raw["pad_stem_intent"][0] = malformed
    config = tmp_path / "invalid-selection.json"
    original = json.dumps(raw)
    config.write_text(original, encoding="utf-8")

    restored = ProjectPersistence.from_config_path(config)
    assert restored.load_error == "Unsupported stem selection metadata retained on disk"
    assert restored.project.volume == project.volume
    assert restored.project.pad_gain_db == project.pad_gain_db
    assert restored.project.pad_stem_mix_mode == project.pad_stem_mix_mode
    assert restored.project.pad_stem_intent[0] == PadStemIntent()
    assert restored.project.pad_stem_intent[36] == project.pad_stem_intent[36]
    assert restored.project.pad_stem_intent[215] == project.pad_stem_intent[215]
    assert len(restored.project.pad_stem_intent) == NUM_SAMPLES
    assert config.read_text(encoding="utf-8") == original


@pytest.mark.parametrize("length", [1, NUM_SAMPLES + 1])
def test_invalid_table_length_is_visible_and_recovery_is_bounded(
    tmp_path: Path, length: int
) -> None:
    project = _configured_project()
    raw = project.model_dump()
    raw["pad_stem_intent"] = [
        {"enabled_mask": 0, "last_custom_mask": 3, "display_mode": "custom"} for _ in range(length)
    ]
    config = tmp_path / "invalid-length.json"
    config.write_text(json.dumps(raw), encoding="utf-8")

    restored = ProjectPersistence.from_config_path(config)
    assert restored.load_error == "Unsupported stem selection metadata retained on disk"
    assert restored.project.volume == project.volume
    assert len(restored.project.pad_stem_intent) == NUM_SAMPLES
    assert restored.project.pad_stem_intent[0].enabled_mask == 0
    assert restored.project.pad_stem_intent[min(length, NUM_SAMPLES) - 1].enabled_mask == 0
    if length < NUM_SAMPLES:
        assert restored.project.pad_stem_intent[length] == PadStemIntent()


def test_non_table_selection_defaults_only_selection_and_reports_failure(tmp_path: Path) -> None:
    project = _configured_project()
    raw = project.model_dump()
    raw["pad_stem_intent"] = {"36": {"enabled_mask": 0}}
    config = tmp_path / "wrong-shape.json"
    config.write_text(json.dumps(raw), encoding="utf-8")

    restored = ProjectPersistence.from_config_path(config)
    assert restored.load_error == "Unsupported stem selection metadata retained on disk"
    assert restored.project.pad_gain_db == project.pad_gain_db
    assert restored.project.pad_stem_mix_mode == project.pad_stem_mix_mode
    assert all(value == PadStemIntent() for value in restored.project.pad_stem_intent)
