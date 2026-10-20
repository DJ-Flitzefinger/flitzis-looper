"""Persisted indexed pad tables cannot admit missing or displaced pad rows."""

import json
from typing import TYPE_CHECKING

import pytest
from pydantic import ValidationError

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import ProjectState

if TYPE_CHECKING:
    from pathlib import Path


PAD_TABLES = ("sample_paths", "sample_durations", "sample_analysis", "manual_bpm")


@pytest.mark.parametrize("table", PAD_TABLES)
@pytest.mark.parametrize("length", [0, NUM_SAMPLES - 1, NUM_SAMPLES + 1])
def test_explicit_incorrect_pad_table_length_is_rejected_before_indexed_access(
    table: str, length: int
) -> None:
    with pytest.raises(ValidationError):
        ProjectState.model_validate_json(json.dumps({table: [None] * length}))


@pytest.mark.parametrize("table", PAD_TABLES)
def test_exact_or_omitted_table_keeps_valid_defaults_and_last_pad(table: str) -> None:
    for data in ({}, {table: [None] * NUM_SAMPLES}):
        state = ProjectState.model_validate_json(json.dumps(data))
        assert len(getattr(state, table)) == NUM_SAMPLES
        assert getattr(state, table)[NUM_SAMPLES - 1] is None


@pytest.mark.parametrize("table", PAD_TABLES)
def test_invalid_table_uses_existing_config_policy_without_rewriting_input(
    tmp_path: Path, table: str
) -> None:
    config = tmp_path / "synthetic-invalid-config.json"
    original = json.dumps({table: []}).encode("utf-8")
    config.write_bytes(original)
    persistence = ProjectPersistence.from_config_path(config)
    assert len(getattr(persistence.project, table)) == NUM_SAMPLES
    assert config.read_bytes() == original
