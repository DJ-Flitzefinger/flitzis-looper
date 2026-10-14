"""Python consumes Native location proof without rewriting historical journal evidence."""

import hashlib
import json
import os
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import material_migration_recovery as recovery
from flitzis_looper.material_migration_model import MaterialMigrationJournal, MigrationAssignment
from flitzis_looper.models import ProjectState


def _reference(path: Path) -> str:
    return os.path.normcase(str(path.resolve()))


def _native_locations(monkeypatch: pytest.MonkeyPatch, locations: dict[str, str]) -> Mock:
    def resolve(_samples: str, reference: str) -> str:
        return locations.get(reference, reference)

    resolver = Mock(side_effect=resolve)
    monkeypatch.setattr(
        recovery.flitzis_looper_audio, "resolve_migration_config_reference", resolver, raising=False
    )
    return resolver


def test_native_location_projection_filters_record_without_rewriting_original_binding(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    samples = tmp_path / "samples"
    current = samples / "selected.config.json"
    historical = _reference(tmp_path / "old-checkout/samples/selected.config.json")
    resolver = _native_locations(monkeypatch, {historical: _reference(current)})
    journal_json = json.dumps({"config_reference": historical})
    encoded = json.dumps({"transaction_id": "1" * 32, "record": journal_json, "error": None})

    record = recovery._record_for_config(encoded, samples, _reference(current))

    assert record is not None
    assert record["record"] == journal_json
    resolver.assert_called_once_with(str(samples), historical)


def test_equal_byte_other_config_stays_a_distinct_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    samples = tmp_path / "samples"
    samples.mkdir()
    selected = samples / "selected.config.json"
    other = samples / "other.config.json"
    for path in (selected, other):
        path.write_bytes(b"identical bytes")
    _native_locations(monkeypatch, {})
    encoded = json.dumps({"record": json.dumps({"config_reference": _reference(other)})})

    assert recovery._record_for_config(encoded, samples, _reference(selected)) is None
    assert selected.read_bytes() == other.read_bytes()


def test_unverified_native_projection_keeps_recovery_visible_without_reading_intent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    samples = tmp_path / "samples"
    samples.mkdir()
    resolver = Mock(side_effect=RuntimeError("projection custody changed"))
    monkeypatch.setattr(
        recovery.flitzis_looper_audio, "resolve_migration_config_reference", resolver, raising=False
    )
    scan = Mock()
    monkeypatch.setattr(recovery, "MaterialMigrationJournalStore", Mock(scan=scan))

    result = recovery.inspect_migration_recovery(samples, samples / "selected.config.json")

    assert result.errors == ["projection custody changed"]
    assert result.intent is None
    assert not result.pending
    scan.assert_not_called()


def test_missing_native_location_api_fails_closed_without_import_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.delattr(
        recovery.flitzis_looper_audio, "resolve_migration_config_reference", raising=False
    )

    with pytest.raises(RuntimeError, match="Native config-location resolution is unavailable"):
        recovery.resolve_config_reference(
            tmp_path / "samples", str(tmp_path / "selected.config.json")
        )


def test_newer_current_config_wins_over_relocated_pending_snapshot(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    samples = tmp_path / "samples"
    samples.mkdir()
    config = samples / "selected.config.json"
    captured = ProjectState(volume=0.1)
    current = ProjectState(volume=0.8)
    current.config_revision = 9
    config.write_text(current.model_dump_json(), encoding="utf-8")
    historical = _reference(tmp_path / "old-checkout/samples/selected.config.json")
    _native_locations(monkeypatch, {historical: _reference(config)})
    journal = MaterialMigrationJournal(
        transaction_id="1" * 32,
        config_reference=historical,
        captured_revision=0,
        intent_revision=7,
        config_sha256=hashlib.sha256(captured.model_dump_json().encode()).hexdigest(),
        snapshot_json=captured.model_dump_json(),
        assignments=(
            MigrationAssignment(sample_id=0, instance_id="2" * 32, old_reference="samples/old.wav"),
        ),
    )
    encoded = journal.model_dump_json()
    store = recovery.MaterialMigrationJournalStore(str(samples), journal.transaction_id)
    store.append(encoded)
    history = store.history()
    before = config.read_bytes()

    result = recovery.inspect_migration_recovery(samples, config)

    assert not result.errors
    assert result.pending == [journal]
    assert result.intent is None
    assert result.settled == []
    assert result.pair_journals == []
    assert config.read_bytes() == before
    assert result.pending[0].config_reference == historical
    assert (
        recovery.MaterialMigrationJournalStore.open(str(samples), journal.transaction_id).history()
        == history
        == [encoded]
    )


def test_alias_location_comparison_delegates_both_addresses_to_native(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    samples = tmp_path / "samples"
    current = _reference(samples / "selected.config.json")
    historical = _reference(tmp_path / "old/samples/selected.config.json")
    resolver = _native_locations(monkeypatch, {historical: current})

    assert recovery.config_reference_matches(samples, current, historical)

    assert resolver.call_args_list[0].args == (str(samples), current)
    assert resolver.call_args_list[1].args == (str(samples), historical)
