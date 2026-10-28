"""Historical settled evidence must not create fresh recovery or block normal Close.

Fixtures use real Config/journal files, Native journal storage, strict histories
and App startup owners. Mocked audio does not prove device or Native ACK behavior.
"""

import hashlib
import json
from typing import TYPE_CHECKING, Literal

import pytest

from flitzis_looper.controller import recording
from flitzis_looper.controller.app import AppController
from flitzis_looper.controller.bank_arrangement_journal import validated_bank_history
from flitzis_looper.controller.copy_arrangement_journal import validated_copy_history
from flitzis_looper.controller.material_migration_recovery import inspect_migration_recovery
from flitzis_looper.controller.pair_arrangement_journal import (
    validated_pair_history,
    validated_project_image,
)
from flitzis_looper.models import ProjectState
from flitzis_looper_audio import MaterialMigrationJournalStore
from tests.flitzis_looper.controller.test_bank_arrangement_startup import _prior as _bank_prior
from tests.flitzis_looper.controller.test_copy_arrangement_startup import _prior as _copy_prior
from tests.flitzis_looper.controller.test_pair_arrangement_startup import _prior as _pair_prior

if TYPE_CHECKING:
    from pathlib import Path
    from unittest.mock import Mock

type JournalKind = Literal["pair", "copy", "bank"]


def _encoded(data: object) -> str:
    return json.dumps(data, ensure_ascii=False, separators=(",", ":"), allow_nan=False)


def _legacy_image(text: str, budget: int) -> str:
    data = json.loads(text)
    data["resident_pcm_budget_mib"] = budget
    data.pop("resident_pcm_budget_automatic")
    return _encoded(data)


def _historical_settled_case(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    kind: JournalKind,
    budget: int,
) -> tuple[Path, list[str]]:
    if kind == "pair":
        config, current = _pair_prior(tmp_path, monkeypatch, after=True)
    elif kind == "copy":
        config, current = _copy_prior(tmp_path, monkeypatch, after=True)
    else:
        config, current = _bank_prior(tmp_path, monkeypatch, after=True)
    store = MaterialMigrationJournalStore.open(str(config.parent), current.transaction_id)
    terminal = json.loads(current.model_dump_json())
    terminal["phase"] = "settled"
    store.append(_encoded(terminal))
    records = [json.loads(text) for text in store.history()]
    del store
    historical: list[str] = []
    captured_digest = None
    for index, record in enumerate(records):
        for field in ("project_image_json", "captured_project_image_json"):
            if field in record:
                record[field] = _legacy_image(record[field], budget)
        if captured_digest is None:
            captured_digest = hashlib.sha256(record["project_image_json"].encode()).hexdigest()
        record["captured_config_sha256"] = captured_digest
        if record["committed_config_sha256"] is not None:
            record["committed_config_sha256"] = hashlib.sha256(
                record["project_image_json"].encode()
            ).hexdigest()
        text = _encoded(record)
        # Construct historical fixture bytes before opening the App. Production
        # compatibility must never rewrite those bytes or their stored digests.
        leaf = config.parent / ".material-migrations" / current.transaction_id / f"{index:02}.json"
        leaf.write_text(text, encoding="utf-8")
        historical.append(text)
    assert current.committed_revision is not None
    # A later saved project may have unloaded all participants. Standalone
    # settled history has no authority to resurrect them or reserve its writer.
    project = ProjectState(
        config_revision=current.committed_revision + 1, resident_pcm_budget_mib=budget
    )
    config.write_text(project.model_dump_json(), encoding="utf-8")
    return config, historical


def _settle_empty_recording_discovery(app: AppController) -> None:
    owner = app.recording._owner
    if owner is None:
        return
    assert isinstance(owner, recording._RecoveryOwner)
    owner.worker._thread.join(timeout=1.0)
    assert not owner.worker._thread.is_alive()
    app.recording.poll()
    assert app.recording.status().retired


@pytest.mark.parametrize("kind", ["pair", "copy", "bank"])
@pytest.mark.parametrize("budget", [512, 9925])
def test_settled_legacy_histories_accept_known_missing_admission_field_without_rewriting(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    kind: JournalKind,
    budget: int,
) -> None:
    config, historical = _historical_settled_case(tmp_path, monkeypatch, kind, budget)
    transaction = json.loads(historical[0])["transaction_id"]
    store = MaterialMigrationJournalStore.open(str(config.parent), transaction)
    validators = {
        "pair": validated_pair_history,
        "copy": validated_copy_history,
        "bank": validated_bank_history,
    }

    records = validators[kind](store.history(), transaction)

    assert records[-1].phase == "settled"
    for original, record in zip(historical, records, strict=True):
        raw = json.loads(original)
        assert record.project_image_json == raw["project_image_json"]
        assert record.committed_config_sha256 == raw["committed_config_sha256"]
        image = validated_project_image(record.project_image_json)
        assert image.resident_pcm_budget_mib == budget
        assert image.resident_pcm_budget_automatic is (budget == 512)
    assert store.history() == historical


@pytest.mark.parametrize("kind", ["pair", "copy", "bank"])
@pytest.mark.parametrize("budget", [512, 9925])
def test_settled_legacy_history_startup_closes_without_spurious_writer_recovery(
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    kind: JournalKind,
    budget: int,
) -> None:
    config, historical = _historical_settled_case(tmp_path, monkeypatch, kind, budget)
    directory = config.parent / ".material-migrations"
    before = {config: config.read_bytes()}
    before.update({leaf: leaf.read_bytes() for leaf in directory.glob("*/*.json")})

    app = AppController(project_config_path=config)
    _settle_empty_recording_discovery(app)
    ready = app.prepare_shutdown()

    assert {path: path.read_bytes() for path in before} == before
    assert ready, app.pending_shutdown_error
    assert app.persistence._migration_owner is None
    assert not app.arrangement.active
    assert not app.pairs.active
    assert not app.banks.active
    assert app.material_migration._recovery_owner is None
    recovery = inspect_migration_recovery(config.parent, config)
    assert not recovery.errors
    assert not recovery.pending
    assert not recovery.unfinished_arrangement_kinds
    assert len(recovery.pair_journals + recovery.copy_journals + recovery.bank_journals) == 1
    assert app.project.resident_pcm_budget_mib == budget
    assert app.project.resident_pcm_budget_automatic is (budget == 512)
    assert app.pending_shutdown_error is None
    audio_engine_mock.shut_down.assert_not_called()
    transaction = json.loads(historical[0])["transaction_id"]
    store = MaterialMigrationJournalStore.open(str(config.parent), transaction)
    assert store.history() == historical

    app.shut_down()

    assert app._shutdown_complete
    audio_engine_mock.shut_down.assert_called_once()
    assert {path: path.read_bytes() for path in before if path != config} == {
        path: content for path, content in before.items() if path != config
    }
    assert store.history() == historical


@pytest.mark.parametrize("budget", [512, 9925])
@pytest.mark.parametrize("automatic", [False, True])
def test_project_image_preserves_explicit_admission_mode_for_current_project_images(
    budget: int, *, automatic: bool
) -> None:
    project = ProjectState(resident_pcm_budget_mib=budget, resident_pcm_budget_automatic=automatic)
    data = project.model_dump(mode="json", exclude_computed_fields=True)

    restored = validated_project_image(_encoded(data))

    assert restored.resident_pcm_budget_mib == budget
    assert restored.resident_pcm_budget_automatic is automatic


@pytest.mark.parametrize("automatic", [0, 1, "false", "true", None])
def test_project_image_rejects_non_boolean_explicit_admission_mode(automatic: object) -> None:
    data = ProjectState().model_dump(mode="json", exclude_computed_fields=True)
    data["resident_pcm_budget_automatic"] = automatic

    with pytest.raises(ValueError, match="valid boolean"):
        validated_project_image(_encoded(data))


@pytest.mark.parametrize("corruption", ["missing", "unknown", "nested"])
def test_legacy_admission_image_does_not_allow_unrelated_missing_or_unknown_metadata(
    corruption: str,
) -> None:
    data = json.loads(_legacy_image(ProjectState().model_dump_json(), 9925))
    if corruption == "missing":
        data.pop("volume")
    elif corruption == "unknown":
        data["unrecognized_project_field"] = True
    else:
        data["pad_key_intent"][0]["unrecognized_key_field"] = True

    error = (
        "Extra inputs are not permitted"
        if corruption == "nested"
        else "incomplete or contains unknown fields"
    )
    with pytest.raises(ValueError, match=error):
        validated_project_image(_encoded(data))


@pytest.mark.parametrize("nested", [False, True])
def test_legacy_admission_image_rejects_duplicate_json_keys(*, nested: bool) -> None:
    text = _legacy_image(ProjectState().model_dump_json(), 9925)
    if nested:
        text = text.replace('"base_shift":0', '"base_shift":0,"base_shift":0', 1)
    else:
        text = text[:-1] + ',"resident_pcm_budget_mib":9925}'

    with pytest.raises(ValueError, match="duplicate object keys"):
        validated_project_image(text)
