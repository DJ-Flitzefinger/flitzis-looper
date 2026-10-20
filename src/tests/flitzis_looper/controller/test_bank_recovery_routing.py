"""Shared strict routing and Legacy admission, with real journal/Config evidence.

Controller audio and reconciler seams below prove custody policy only. They do
not establish Native source ACK, physical cleanup, realtime or hearing acceptance.
"""

import json
from concurrent.futures import Future
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import material_migration as migration_module
from flitzis_looper.controller.bank_arrangement_journal import (
    BankJournalOwner,
    project_with_journal_bank,
)
from flitzis_looper.controller.material_migration import MaterialMigrationController
from flitzis_looper.controller.material_migration_recovery import (
    MigrationRecovery,
    inspect_migration_recovery,
    parse_transaction_journal,
)
from flitzis_looper.material_migration_model import MaterialMigrationJournal, MigrationAssignment
from flitzis_looper.models import PadContentIdentity, SessionState
from flitzis_looper_audio import MaterialMigrationJournalStore
from tests.flitzis_looper.controller.test_bank_arrangement_journal import (
    _acknowledged,
    _captured,
)
from tests.flitzis_looper.controller.test_copy_arrangement_journal import (
    _captured as _copy_captured,
)
from tests.flitzis_looper.controller.test_material_creation_history import _policy_birth
from tests.flitzis_looper.controller.test_pair_arrangement_journal import (
    _captured as _pair_captured,
)

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize("terminal", [None, "cancelled", "settled"])
def test_bank_routes_only_to_bank_inventory_including_terminal_evidence(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, terminal: str | None
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    tail = owner.append(journal)
    if terminal == "cancelled":
        tail = owner.append(tail.changed(phase="cancelled"))
    elif terminal == "settled":
        tail = _acknowledged(owner, journal)
        candidate = project_with_journal_bank(persistence.project, journal)
        receipt = persistence.commit_migration_receipt(
            journal.transaction_id, persistence.revision, candidate
        )
        tail = owner.append(tail.committed(receipt))
        tail = owner.append(tail.changed(phase="settled"))
    history = owner.store.history()
    config_bytes = persistence.config_path.read_bytes()

    recovered = inspect_migration_recovery(tmp_path / "samples", persistence.config_path)

    assert parse_transaction_journal(tail.model_dump_json()) == tail
    assert recovered.bank_journals == [tail]
    assert recovered.pending == []
    assert recovered.settled == []
    assert recovered.pair_journals == []
    assert recovered.copy_journals == []
    assert recovered.intent is None
    assert recovered.errors == []
    assert recovered.unfinished_arrangement_kinds == (() if terminal else ("pad_bank",))
    assert owner.store.history() == history
    assert persistence.config_path.read_bytes() == config_bytes


@pytest.mark.parametrize("damage", ["kind", "version", "bool_version", "unknown_field"])
def test_bank_route_rejects_unknown_or_non_strict_schema(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, damage: str
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    data = json.loads(journal.model_dump_json())
    if damage == "kind":
        data["kind"] = "future_bank"
    elif damage == "version":
        data["schema_version"] = 2
    elif damage == "bool_version":
        data["schema_version"] = True
    else:
        data["native_ack"] = {"saved_permission": True}

    expected = "Extra inputs" if damage == "unknown_field" else "Unknown migration transaction"
    with pytest.raises(ValueError, match=expected):
        parse_transaction_journal(json.dumps(data))


def test_bank_for_equal_byte_foreign_config_keeps_its_distinct_owner_and_history(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    foreign = tmp_path / "samples" / "other.config.json"
    foreign.write_bytes(persistence.config_path.read_bytes())
    before = owner.store.history()

    recovered = inspect_migration_recovery(tmp_path / "samples", foreign)

    assert recovered.bank_journals == []

    assert recovered.pending == []

    assert recovered.settled == []
    assert recovered.unfinished_arrangement_kinds == ()
    assert recovered.errors == []
    assert persistence._migration_owner == journal.transaction_id
    assert owner.store.history() == before


@pytest.mark.parametrize(
    "damage", ["unknown_kind", "future_version", "config", "missing_field", "phase_edge"]
)
def test_valid_bank_scan_tail_cannot_hide_an_invalid_earlier_ordinal(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, damage: str
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    data = json.loads(journal.model_dump_json())
    if damage == "unknown_kind":
        data["kind"] = "future_bank"
    elif damage == "future_version":
        data["schema_version"] = 2
    elif damage == "config":
        data["config_reference"] = str(tmp_path / "samples" / "other.config.json")
    elif damage == "missing_field":
        del data["error"]
    else:
        data["phase"] = "publication_pending"
    store = MaterialMigrationJournalStore(str(tmp_path / "samples"), journal.transaction_id)
    store.append(json.dumps(data))
    store.append(journal.model_dump_json())
    history = store.history()

    recovered = inspect_migration_recovery(tmp_path / "samples", persistence.config_path)

    assert recovered.errors
    assert recovered.bank_journals == []
    assert recovered.pending == []
    assert recovered.settled == []
    assert store.history() == history


@pytest.mark.parametrize("companion", ["legacy", "pair", "copy", "all"])
def test_mixed_unfinished_kinds_keep_all_histories_visible_without_legacy_cleanup(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, companion: str
) -> None:
    persistence, bank = _captured(tmp_path, monkeypatch)
    samples = tmp_path / "samples"
    BankJournalOwner(samples, bank).append(bank)
    expected = {"pad_bank"}
    if companion in {"pair", "all"}:
        (tmp_path / "pair-input").mkdir()
        _, pair = _pair_captured(tmp_path / "pair-input", monkeypatch)
        pair = pair.changed(transaction_id="c" * 32, config_reference=persistence.config_reference)
        store = MaterialMigrationJournalStore(str(samples), pair.transaction_id)
        store.append(pair.model_dump_json())
        expected.add("pad_pair")
    if companion in {"copy", "all"}:
        (tmp_path / "copy-input").mkdir()
        _, copy = _copy_captured(tmp_path / "copy-input", monkeypatch)
        copy = copy.changed(transaction_id="d" * 32, config_reference=persistence.config_reference)
        store = MaterialMigrationJournalStore(str(samples), copy.transaction_id)
        store.append(copy.model_dump_json())
        expected.add("pad_copy_overwrite")
    if companion in {"legacy", "all"}:
        legacy = MaterialMigrationJournal(
            transaction_id="e" * 32,
            config_reference=persistence.config_reference,
            captured_revision=bank.captured_revision,
            intent_revision=bank.intent_revision,
            config_sha256=bank.captured_config_sha256,
            snapshot_json=bank.project_image_json,
            assignments=(
                MigrationAssignment(
                    sample_id=0, instance_id=f"{1:032x}", old_reference="samples/Take0.wav"
                ),
            ),
        )
        store = MaterialMigrationJournalStore(str(samples), legacy.transaction_id)
        store.append(legacy.model_dump_json())
    before = MaterialMigrationJournalStore.scan(str(samples))

    recovered = inspect_migration_recovery(samples, persistence.config_path)

    assert set(recovered.unfinished_arrangement_kinds) == expected
    assert recovered.bank_journals == [bank]
    assert len(recovered.pair_journals) == int(companion in {"pair", "all"})
    assert len(recovered.copy_journals) == int(companion in {"copy", "all"})
    assert len(recovered.pending) == int(companion in {"legacy", "all"})
    assert recovered.settled == []
    assert recovered.errors == [
        "Incompatible unfinished Material/Pair/Copy/Bank owners retain every history"
    ]
    assert MaterialMigrationJournalStore.scan(str(samples)) == before


def test_material_defers_before_bank_pins_without_capturing_writer_or_scanning_each_poll(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    constructor = Mock()
    monkeypatch.setattr(migration_module, "MigrationArtifactReconciler", constructor)
    scan = Mock(wraps=inspect_migration_recovery)
    monkeypatch.setattr(migration_module, "inspect_migration_recovery", scan)
    audio = Mock()
    audio.migration_artifact_ledger_supported.return_value = True
    service = MaterialMigrationController(persistence, SessionState(), audio, Mock(), Mock())

    service.bind_arrangement_recovery_guard(lambda: persistence._migration_owner is None)
    service.schedule_after_restore()
    for _ in range(3):
        service.poll()
    with pytest.raises(RuntimeError, match="arrangement recovery"):
        service.begin(0)
    with pytest.raises(ValueError, match="Arrangement recovery"):
        service.retry_recovery()

    assert service.status == "unresolved"
    assert persistence._migration_owner == journal.transaction_id
    assert service._recovery_owner is None
    assert service._startup_scheduled
    assert service._startup_candidates
    scan.assert_called_once()
    constructor.assert_not_called()
    audio.hold_material_migration.assert_not_called()
    assert owner.store.history() == [journal.model_dump_json()]


def test_material_birth_restore_observer_remains_reachable_while_bank_cleanup_is_deferred(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    BankJournalOwner(tmp_path / "samples", journal).append(journal)
    worker = Mock(available=True)
    restored: Future[None] = Future()
    restored.set_result(None)
    worker.restore_material_creation.return_value = restored
    constructor = Mock(return_value=worker)
    monkeypatch.setattr(migration_module, "MigrationArtifactReconciler", constructor)
    audio = Mock()
    audio.migration_artifact_ledger_supported.return_value = True
    loader = Mock()
    service = MaterialMigrationController(persistence, SessionState(), audio, Mock(), loader)
    constructor.assert_not_called()
    _, alias = _policy_birth(tmp_path / "samples")
    content = PadContentIdentity(
        instance_id="f" * 32,
        material_id=alias.material_id,
        material_creation=alias.material_creation,
    )
    persistence.project.sample_paths[108] = alias.new_reference
    persistence.project.pad_content[108] = content
    loader.bind_material_creation_restore.call_args.args[0](108, content)

    service.poll()
    service.poll()

    constructor.assert_called_once()
    worker.restore_material_creation.assert_called_once_with(
        108, content.instance_id, content.material_creation, None
    )
    worker.retire.assert_not_called()
    worker.capture.assert_not_called()
    assert persistence._migration_owner == journal.transaction_id
    assert service._creation_restore_future is None
    assert service.status == "unresolved"


def test_only_explicit_strict_refresh_after_bank_settlement_reopens_legacy_cleanup(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    worker = Mock()
    constructor = Mock(return_value=worker)
    monkeypatch.setattr(migration_module, "MigrationArtifactReconciler", constructor)
    scan = Mock(wraps=inspect_migration_recovery)
    monkeypatch.setattr(migration_module, "inspect_migration_recovery", scan)
    audio = Mock()
    audio.migration_artifact_ledger_supported.return_value = True
    service = MaterialMigrationController(persistence, SessionState(), audio, Mock(), Mock())

    assert not service.refresh_arrangement_recovery()
    constructor.assert_not_called()
    owner.append(journal.changed(phase="cancelled"))
    persistence.release_migration(journal.transaction_id)
    service.poll()
    assert service.status == "unresolved"
    constructor.assert_not_called()
    assert scan.call_count == 2

    assert service.refresh_arrangement_recovery()
    assert service.status == "idle"
    assert persistence._migration_owner is None
    service._poll_cleanup()
    constructor.assert_called_once()
    assert scan.call_count == 3
    worker.retire.assert_not_called()


def test_refresh_cannot_clear_cached_deferral_when_terminal_tail_hides_broken_bank_history(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    constructor = Mock()
    monkeypatch.setattr(migration_module, "MigrationArtifactReconciler", constructor)
    audio = Mock()
    audio.migration_artifact_ledger_supported.return_value = True
    service = MaterialMigrationController(persistence, SessionState(), audio, Mock(), Mock())
    # All individual records validate; the backwards phase edge does not.
    owner.store.append(journal.changed(phase="publication_pending").model_dump_json())
    owner.store.append(journal.model_dump_json())
    owner.store.append(journal.changed(phase="cancelled").model_dump_json())
    persistence.release_migration(journal.transaction_id)
    history = owner.store.history()

    assert not service.refresh_arrangement_recovery()
    service.poll()

    assert service.status == "unresolved"
    assert service.error
    assert service._recovery_errors
    assert service._recovery_owner is None
    assert persistence._migration_owner is None
    constructor.assert_not_called()
    assert owner.store.history() == history


@pytest.mark.parametrize("kind", ["pad_pair", "pad_copy_overwrite", "pad_bank"])
def test_retry_never_releases_legacy_writer_when_an_arrangement_owner_is_discovered(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, kind: str
) -> None:
    persistence, bank = _captured(tmp_path, monkeypatch)
    persistence.release_migration(bank.transaction_id)
    error_scan = MigrationRecovery(errors=["incomplete selected history"])
    monkeypatch.setattr(
        migration_module, "inspect_migration_recovery", Mock(return_value=error_scan)
    )
    audio = Mock()
    audio.migration_artifact_ledger_supported.return_value = False
    service = MaterialMigrationController(persistence, SessionState(), audio, Mock(), Mock())
    owned = service._recovery_owner
    assert owned is not None
    recovery = MigrationRecovery()
    if kind == "pad_bank":
        recovery.bank_journals.append(bank)
    elif kind == "pad_pair":
        (tmp_path / "pair-input").mkdir()
        _, pair = _pair_captured(tmp_path / "pair-input", monkeypatch)
        recovery.pair_journals.append(pair)
    else:
        (tmp_path / "copy-input").mkdir()
        _, copy = _copy_captured(tmp_path / "copy-input", monkeypatch)
        recovery.copy_journals.append(copy)
    monkeypatch.setattr(migration_module, "inspect_migration_recovery", Mock(return_value=recovery))

    with pytest.raises(ValueError, match="Arrangement recovery"):
        service.retry_recovery()
    service.poll()

    assert service._recovery_owner == persistence._migration_owner == owned
    assert service.status == "unresolved"
    audio.hold_material_migration.return_value.release.assert_not_called()
