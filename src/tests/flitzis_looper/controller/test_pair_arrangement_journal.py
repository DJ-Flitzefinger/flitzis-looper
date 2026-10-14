"""Actual bounded journal/Config crash evidence never replays an already applied Swap."""

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING, cast

import pytest
from pydantic import ValidationError

from flitzis_looper.controller.material_migration_recovery import (
    inspect_migration_recovery,
    parse_transaction_journal,
)
from flitzis_looper.controller.pair_arrangement_journal import (
    PairArrangementAfterImage,
    PairArrangementJournal,
    PairJournalOwner,
    validated_pair_history,
)
from flitzis_looper.controller.pair_arrangement_recovery import inspect_pair_recovery
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.material_migration_model import MaterialMigrationJournal, MigrationAssignment
from flitzis_looper.models import PadContentIdentity, ProjectState
from flitzis_looper.pair_snapshot import capture_pair_snapshot, project_with_pair_snapshot
from flitzis_looper_audio import MaterialMigrationJournalStore

if TYPE_CHECKING:
    from flitzis_looper.pair_snapshot import PairOperation

_TRANSACTION = "a" * 32


def _captured(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    operation: PairOperation = "swap",
) -> tuple[ProjectPersistence, PairArrangementJournal]:
    monkeypatch.chdir(tmp_path)
    project = ProjectState()
    project.sample_paths[0] = str(tmp_path / "samples" / "A.wav")
    project.pad_content[0] = PadContentIdentity(instance_id="1" * 32)
    project.pad_gain_db[0] = -6.0
    if operation == "swap":
        project.sample_paths[215] = "samples/B.wav"
        project.pad_content[215] = PadContentIdentity(instance_id="2" * 32, copied_from="1" * 32)
        project.pad_gain_db[215] = -12.0
    project.pad_gain_db[36] = -3.0
    persistence = ProjectPersistence(project)
    persistence.config_path = tmp_path / "samples" / "pair.config.json"
    persistence.flush()
    revision, image, digest = persistence.capture_migration(_TRANSACTION)
    before = capture_pair_snapshot(image, 0, 215, operation=operation)
    return persistence, PairArrangementJournal(
        transaction_id=_TRANSACTION,
        config_reference=persistence.config_reference,
        captured_revision=revision,
        intent_revision=revision,
        captured_config_sha256=digest,
        before=before,
        after=PairArrangementAfterImage.from_before(before),
        project_image_json=image.model_dump_json(),
        phase="captured",
        image_role="before",
    )


def _acknowledged(
    owner: PairJournalOwner, journal: PairArrangementJournal
) -> PairArrangementJournal:
    owner.append(journal)
    pending = owner.append(journal.changed(phase="publication_pending"))
    return owner.append(pending.changed(phase="native_ack_observed"))


@pytest.mark.parametrize("operation", ["move", "swap"])
def test_actual_receipt_is_normalized_written_same_revision_with_complete_after_slots(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    operation: PairOperation,
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch, operation)
    owner = PairJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    candidate = project_with_pair_snapshot(persistence.project, journal.before)
    candidate_text = candidate.model_dump_json()

    receipt = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    committed = owner.append(acknowledged.committed(receipt))

    assert receipt.revision == journal.intent_revision == persistence.revision
    assert receipt.written_text.encode("utf-8") == persistence.config_path.read_bytes()
    assert receipt.sha256 == hashlib.sha256(persistence.config_path.read_bytes()).hexdigest()
    assert receipt.sha256 != hashlib.sha256(candidate_text.encode("utf-8")).hexdigest()
    assert receipt.projected().sample_paths[215] == "samples/A.wav"
    assert committed.image_role == "after"
    assert (
        committed.after.projected().target.source_identity
        == journal.before.projected().source.source_identity
    )
    if operation == "move":
        assert committed.after.projected().source.source_identity is None
        assert receipt.projected().pad_gain_db[0] == ProjectState().pad_gain_db[0]
    assert (
        inspect_pair_recovery(committed, persistence.config_path.read_bytes()).disposition
        == "recognized_after"
    )


@pytest.mark.parametrize("phase", ["captured", "publication_pending", "native_ack_observed"])
def test_exact_original_config_restores_detached_pending_intent_without_saved_permission(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    phase: str,
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    journal = journal.changed(phase=phase)
    decision = inspect_pair_recovery(journal, persistence.config_path.read_bytes())

    assert decision.disposition == "original_pending"
    assert decision.receipt is None
    first = decision.projected()
    second = decision.projected()
    assert first is not None
    assert second is not None
    first.pad_gain_db[36] = -24.0
    assert second.pad_gain_db[36] == -3.0
    assert "native_ack" not in json.loads(journal.model_dump_json())


@pytest.mark.parametrize("crash_point", ["original", "replaced", "recorded", "settled"])
def test_real_native_history_reopens_in_fresh_process_at_each_config_receipt_boundary(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    crash_point: str,
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = PairJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    if crash_point != "original":
        candidate = project_with_pair_snapshot(persistence.project, journal.before)
        receipt = persistence.commit_migration_receipt(
            _TRANSACTION, persistence.revision, candidate
        )
        if crash_point in {"recorded", "settled"}:
            committed = owner.append(acknowledged.committed(receipt))
            if crash_point == "settled":
                owner.append(committed.changed(phase="settled"))
    child_code = """
import json, sys
from pathlib import Path
from flitzis_looper.controller.pair_arrangement_journal import PairJournalOwner
from flitzis_looper.controller.pair_arrangement_recovery import inspect_pair_recovery
owner = PairJournalOwner.open(Path(sys.argv[1]), sys.argv[2])
decision = inspect_pair_recovery(owner.journal, Path(sys.argv[3]).read_bytes())
project = decision.projected()
print(json.dumps({'decision': decision.disposition, 'slots': None if project is None else [
    project.pad_content[i].instance_id if project.pad_content[i] else None for i in [0, 215]],
    'revision': None if decision.receipt is None else decision.receipt.revision}))
"""
    environment = os.environ | {"PYTHONPATH": str(Path(__file__).resolve().parents[3])}
    child = subprocess.run(
        [
            sys.executable,
            "-c",
            child_code,
            str(tmp_path / "samples"),
            _TRANSACTION,
            str(persistence.config_path),
        ],
        cwd=tmp_path,
        env=environment,
        capture_output=True,
        text=True,
        check=True,
        timeout=30,
    )
    actual = json.loads(child.stdout.strip().splitlines()[-1])
    expected = {
        "original": "original_pending",
        "replaced": "recognized_after",
        "recorded": "recognized_after",
        "settled": "settled",
    }[crash_point]
    assert actual["decision"] == expected
    if crash_point == "original":
        assert actual["slots"] == ["1" * 32, "2" * 32]
    elif crash_point != "settled":
        assert actual["slots"] == ["2" * 32, "1" * 32], (
            "recovery must not Swap the after image again"
        )
        assert actual["revision"] == journal.intent_revision


@pytest.mark.parametrize("changed", ["participant", "unrelated", "revision"])
def test_changed_config_at_post_replace_crash_is_byte_identical_and_fenced(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    changed: str,
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    acknowledged = journal.changed(phase="native_ack_observed")
    after = project_with_pair_snapshot(persistence.project, journal.before)
    if changed == "participant":
        after.pad_gain_db[215] = -22.0
    elif changed == "unrelated":
        after.pad_gain_db[36] = -22.0
    else:
        after.config_revision += 1
    data = after.model_dump_json().encode("utf-8")
    persistence.config_path.write_bytes(data)

    assert inspect_pair_recovery(acknowledged, data).disposition == "conflict"
    assert persistence.config_path.read_bytes() == data


def test_newer_unrelated_commit_retry_keeps_pair_seals_and_settles_latest_receipt(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = PairJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    candidate = project_with_pair_snapshot(persistence.project, journal.before)
    first = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    committed = owner.append(acknowledged.committed(first))
    persistence.project.pad_gain_db[36] = -24.0
    persistence.project.volume = 0.25
    persistence.mark_dirty()
    candidate = project_with_pair_snapshot(persistence.project, journal.before)
    latest = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    recommitted = owner.append(committed.committed(latest))
    settled = owner.append(recommitted.changed(phase="settled"))

    assert latest.revision == first.revision + 1
    assert settled.before == journal.before
    assert settled.after == journal.after
    assert latest.projected().pad_gain_db[36] == -24.0
    assert latest.projected().volume == 0.25
    assert PairJournalOwner.open(tmp_path / "samples", _TRANSACTION).journal == settled


def test_legacy_and_pair_coexist_without_pair_entering_legacy_cleanup(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = PairJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    legacy = MaterialMigrationJournal(
        transaction_id="b" * 32,
        config_reference=persistence.config_reference,
        captured_revision=0,
        intent_revision=0,
        config_sha256=journal.captured_config_sha256,
        snapshot_json=journal.project_image_json,
        assignments=(
            MigrationAssignment(sample_id=0, instance_id="1" * 32, old_reference="samples/A.wav"),
        ),
    )
    legacy_store = MaterialMigrationJournalStore(str(tmp_path / "samples"), legacy.transaction_id)
    legacy_store.append(legacy.model_dump_json())

    recovered = inspect_migration_recovery(tmp_path / "samples", persistence.config_path)

    assert recovered.pair_journals == [journal]
    assert recovered.pending == [legacy]
    assert recovered.settled == []
    assert recovered.errors == []


@pytest.mark.parametrize(
    "damage", ["unknown_kind", "future_version", "changed_binding", "changed_seal"]
)
def test_scan_tail_cannot_hide_invalid_earlier_transaction_evidence(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    damage: str,
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    data = json.loads(journal.model_dump_json())
    if damage == "unknown_kind":
        data["kind"] = "future_pair"
    elif damage == "future_version":
        data["schema_version"] = 2
    elif damage == "changed_binding":
        data["config_reference"] = str(tmp_path / "samples" / "other.config.json")
    else:
        data["before"]["payload_json"] = data["before"]["payload_json"].replace(
            '"gain_db":-6.0', '"gain_db":-7.0'
        )
    store = MaterialMigrationJournalStore(str(tmp_path / "samples"), _TRANSACTION)
    store.append(json.dumps(data))
    store.append(journal.model_dump_json())

    recovered = inspect_migration_recovery(tmp_path / "samples", persistence.config_path)

    assert recovered.pair_journals == []
    assert recovered.errors
    assert len(store.history()) == 2


@pytest.mark.parametrize("field", ["captured_revision", "intent_revision", "schema_version"])
def test_bool_revision_or_version_and_serialized_runtime_rights_are_strictly_rejected(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    field: str,
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    data = json.loads(journal.model_dump_json())
    data[field] = True
    with pytest.raises((ValueError, ValidationError)):
        parse_transaction_journal(json.dumps(data))
    data[field] = 1 if field == "schema_version" else 0
    data["native_ack"] = {"lifetime": 7, "action_ticket": "restorable"}
    with pytest.raises(ValidationError):
        parse_transaction_journal(json.dumps(data))


def test_duplicate_retry_error_does_not_spend_ordinal_capacity(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    owner = PairJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    failed = owner.append(journal.changed(error="queue remains full"))
    for _ in range(30):
        assert owner.append(failed) == failed
    assert len(owner.store.history()) == 2


def test_capacity_retains_owner_and_exact_history_without_extra_write(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    owner = PairJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    for index in range(15):
        owner.append(journal.changed(error=f"different bounded error {index}"))
    before = owner.store.history()
    with pytest.raises(ValueError, match="capacity"):
        owner.append(journal.changed(error="one too many"))
    assert owner.store.history() == before
    assert owner.journal is not None


@pytest.mark.parametrize("complete", [True, False])
def test_failed_append_reopens_exact_intended_bytes_or_retains_partial_ordinal(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    complete: bool,
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    native = MaterialMigrationJournalStore(str(tmp_path / "samples"), _TRANSACTION)

    class FailingStore:
        def history(self) -> list[str]:
            return native.history()

        def append(self, text: str) -> str:
            if complete:
                native.append(text)
            else:
                path = tmp_path / "samples" / ".material-migrations" / _TRANSACTION / "00.json"
                path.write_text(text[:100], encoding="utf-8")
            message = "injected failure after actual ordinal write"
            raise OSError(message)

    owner = PairJournalOwner(
        tmp_path / "samples",
        journal,
        cast("MaterialMigrationJournalStore", FailingStore()),
    )
    if complete:
        assert owner.append(journal) == journal
        assert owner.intended is None
        assert owner.encoded == [journal.model_dump_json()]
        assert owner.append(journal) == journal
    else:
        with pytest.raises((RuntimeError, ValueError)):
            owner.append(journal)
        assert owner.journal is None
        assert owner.intended == journal.model_dump_json()
        with pytest.raises(ValueError, match="Previous Pair append"):
            owner.append(journal.changed(error="cannot skip partial evidence"))
        assert (tmp_path / "samples" / ".material-migrations" / _TRANSACTION / "00.json").read_text(
            encoding="utf-8"
        ) == journal.model_dump_json()[:100]


def test_full_history_rejects_settled_record_with_different_commit_receipt(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = PairJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    candidate = project_with_pair_snapshot(persistence.project, journal.before)
    receipt = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    committed = owner.append(acknowledged.committed(receipt))
    candidate.volume = 0.2
    persistence.mark_dirty()
    newer = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    substituted = committed.committed(newer).changed(phase="settled")
    with pytest.raises(ValueError, match="history"):
        validated_pair_history([*owner.encoded, substituted.model_dump_json()], _TRANSACTION)


def test_full_native_inventory_preserves_config_fence_before_pair_directory_creation(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    for index in range(256):
        MaterialMigrationJournalStore(str(tmp_path / "samples"), f"{index + 1:032x}")
    with pytest.raises(ValueError, match="inventory capacity"):
        PairJournalOwner(tmp_path / "samples", journal)
    assert not (tmp_path / "samples" / ".material-migrations" / _TRANSACTION).exists()
    assert persistence._migration_owner == _TRANSACTION
    assert not persistence.flush_if_dirty()


def test_genuinely_missing_config_captured_under_writer_fence_admits_only_original_intent(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    project = ProjectState()
    project.sample_paths[0] = "samples/A.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="1" * 32)
    persistence = ProjectPersistence(project)
    persistence.config_path = samples / "never-written.config.json"
    revision, image, digest = persistence.capture_migration(_TRANSACTION)
    assert digest is None
    assert not persistence.config_path.exists()
    before = capture_pair_snapshot(image, 0, 215, operation="move")
    journal = PairArrangementJournal(
        transaction_id=_TRANSACTION,
        config_reference=persistence.config_reference,
        captured_revision=revision,
        intent_revision=revision,
        captured_config_sha256=digest,
        before=before,
        after=PairArrangementAfterImage.from_before(before),
        project_image_json=image.model_dump_json(),
        image_role="before",
        phase="captured",
    )
    owner = PairJournalOwner(samples, journal)
    owner.append(journal)
    reopened = PairJournalOwner.open(samples, _TRANSACTION)
    assert inspect_pair_recovery(reopened.journal, None).disposition == "original_pending"
    different = ProjectState().model_dump_json().encode("utf-8")
    assert inspect_pair_recovery(reopened.journal, different).disposition == "conflict"


def test_byte_identical_foreign_config_location_never_acquires_pair_intent(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = PairJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    foreign = tmp_path / "samples" / "foreign.config.json"
    original = persistence.config_path.read_bytes()
    foreign.write_bytes(original)

    recovered = inspect_migration_recovery(tmp_path / "samples", foreign)

    assert not recovered.pair_journals
    assert not recovered.pending
    assert not recovered.errors
    assert persistence.config_path.read_bytes() == foreign.read_bytes() == original


@pytest.mark.parametrize("damage", ["gap", "truncated", "substituted_transaction"])
def test_native_incomplete_or_substituted_history_is_visible_and_preserved(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    damage: str,
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    samples = tmp_path / "samples"
    transaction = "b" * 32 if damage == "substituted_transaction" else _TRANSACTION
    store = MaterialMigrationJournalStore(str(samples), transaction)
    directory = samples / ".material-migrations" / transaction
    if damage == "substituted_transaction":
        store.append(journal.model_dump_json())
    else:
        leaf = directory / ("01.json" if damage == "gap" else "00.json")
        leaf.write_text(journal.model_dump_json() if damage == "gap" else "{", encoding="utf-8")
    original = {path.name: path.read_bytes() for path in directory.iterdir()}
    config_bytes = persistence.config_path.read_bytes()

    recovered = inspect_migration_recovery(samples, persistence.config_path)

    assert recovered.errors
    assert not recovered.pair_journals
    assert {path.name: path.read_bytes() for path in directory.iterdir()} == original
    assert persistence.config_path.read_bytes() == config_bytes
