"""Complete bank journal/Config evidence using the genuine existing Native store."""

import hashlib
import json
from typing import TYPE_CHECKING, Literal, cast

import pytest
from pydantic import ValidationError

from flitzis_looper.bank_snapshot import capture_bank_snapshot
from flitzis_looper.controller import bank_arrangement_journal as bank_journal
from flitzis_looper.controller.bank_arrangement_journal import (
    BankArrangementJournal,
    BankJournalOwner,
    bank_before_project,
    bank_journal_snapshots,
    project_with_journal_bank,
    validate_bank_successor,
    validated_bank_history,
    written_bank_image_matches,
)
from flitzis_looper.controller.persistence import (
    ConfigCommitReceipt,
    PersistenceFenceError,
    ProjectPersistence,
)
from flitzis_looper.copy_snapshot import CONTENT_FIELDS, musical_content_data
from flitzis_looper.models import PadContentIdentity, ProjectState
from flitzis_looper_audio import MaterialMigrationJournalStore
from tests.flitzis_looper.test_copy_snapshot import _full_project

if TYPE_CHECKING:
    from pathlib import Path

_TRANSACTION = "a" * 32
_SUCCESSOR = "b" * 32
type BankCase = Literal["mixed", "full", "clear"]


def _captured(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, case: BankCase = "mixed"
) -> tuple[ProjectPersistence, BankArrangementJournal]:
    monkeypatch.chdir(tmp_path)
    project = ProjectState(key_lock=True, selected_pad=0, selected_bank=0)
    sources = range(36) if case == "full" else (0, 35)
    for sample_id in (*sources, 36, 37, 71, 108):
        project.sample_paths[sample_id] = str(tmp_path / "samples" / f"Take{sample_id}.wav")
        project.pad_content[sample_id] = PadContentIdentity(
            instance_id=f"{sample_id + 1:032x}", material_id=f"{sample_id + 300:032x}"
        )
    project.pad_gain_db[0] = -6.0
    project.pad_eq_low_db[0] = 3.0
    project.manual_bpm[0] = 123.456789
    project.pad_timing_intent[0] = "tap"
    project.pad_loop_start_s[0] = 1.25
    project.pad_loop_end_s[0] = 12.5
    project.pad_loop_bars[0] = 3.5
    project.pad_key_lock[35] = False
    project.pad_gain_db[35] = -8.0
    project.pad_gain_db[36] = -12.0
    project.pad_gain_db[37] = -14.0
    project.pad_gain_db[70] = -19.0  # Complete empty-before control witness.
    persistence = ProjectPersistence(project)
    persistence.config_path = tmp_path / "samples" / "bank.config.json"
    persistence.flush()
    revision, image, digest = persistence.capture_migration(_TRANSACTION)
    snapshot = capture_bank_snapshot(image, None if case == "clear" else 0, 1)
    return persistence, BankArrangementJournal(
        transaction_id=_TRANSACTION,
        config_reference=persistence.config_reference,
        captured_revision=revision,
        intent_revision=revision,
        captured_config_sha256=digest,
        bank=snapshot,
        captured_project_image_json=image.model_dump_json(),
        project_image_json=image.model_dump_json(),
        image_role="before",
        phase="captured",
    )


def _acknowledged(
    owner: BankJournalOwner, journal: BankArrangementJournal
) -> BankArrangementJournal:
    owner.append(journal)
    pending = owner.append(journal.changed(phase="publication_pending"))
    return owner.append(pending.changed(phase="native_ack_observed"))


@pytest.mark.parametrize("case", ["mixed", "full", "clear"])
def test_real_store_records_one_complete_actual_config_receipt_for_each_bank_image(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, case: BankCase
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch, case)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    candidate = project_with_journal_bank(persistence.project, journal)
    receipt = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    committed = owner.append(acknowledged.committed(receipt))
    settled = owner.append(committed.changed(phase="settled"))

    assert receipt.written_text.encode("utf-8") == persistence.config_path.read_bytes()
    assert receipt.sha256 == hashlib.sha256(persistence.config_path.read_bytes()).hexdigest()
    assert receipt.revision == acknowledged.intent_revision == settled.committed_revision
    assert committed.bank == journal.bank
    actual = receipt.projected()
    assert written_bank_image_matches(actual, journal.bank, image_role="after")
    assert project_with_journal_bank(actual, committed).model_dump() == actual.model_dump()
    assert actual.pad_content[:36] == persistence.project.pad_content[:36]
    assert actual.pad_content[108] == persistence.project.pad_content[108]
    if case == "mixed":
        assert actual.pad_content[36] is not None
        assert actual.pad_content[36].copied_from == f"{1:032x}"
        assert actual.pad_content[37] is None
        assert actual.pad_gain_db[37] == 0.0
        assert actual.pad_content[71] is not None
    elif case == "full":
        assert len({row.instance_id for row in actual.pad_content[36:72] if row}) == 36
    else:
        assert all(content is None for content in actual.pad_content[36:72])
    reopened = BankJournalOwner.open(tmp_path / "samples", _TRANSACTION)
    assert reopened.journal == settled
    assert len(reopened.encoded) == 5
    assert validated_bank_history(reopened.encoded, _TRANSACTION)[-1] == settled


def test_fixed_after_projection_preserves_newer_source_outside_and_global_intent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    project = persistence.project
    project.pad_gain_db[0] = -24.0
    project.pad_loop_start_s[35] = 2.0
    project.pad_gain_db[108] = -9.0
    project.volume = 0.25
    project.selected_pad = 108
    project.selected_bank = 3
    persistence.mark_dirty()
    pending = journal.changed(
        phase="native_ack_observed",
        intent_revision=persistence.revision,
        project_image_json=bank_before_project(project, journal).model_dump_json(),
    )
    before = project.model_dump_json()

    candidate = project_with_journal_bank(project, pending)

    assert candidate.pad_gain_db[0] == -24.0
    assert candidate.pad_loop_start_s[35] == 2.0
    assert candidate.pad_gain_db[36] == -6.0
    assert candidate.pad_gain_db[71] == -8.0
    assert candidate.pad_gain_db[108] == -9.0
    assert candidate.volume == 0.25
    assert (candidate.selected_pad, candidate.selected_bank) == (108, 3)
    assert project.model_dump_json() == before
    assert pending.bank == journal.bank


def test_changed_cell_mixture_is_not_accepted_as_a_complete_before_or_after_bank(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    complete = project_with_journal_bank(persistence.project, journal)
    partial = persistence.project.model_copy(deep=True)
    for _, field in CONTENT_FIELDS:
        getattr(partial, field)[36] = getattr(complete, field)[36]
    before = partial.model_dump_json()

    with pytest.raises(ValueError, match="neither the complete original"):
        project_with_journal_bank(partial, journal)

    assert partial.model_dump_json() == before
    assert not written_bank_image_matches(partial, journal.bank, image_role="before")
    assert not written_bank_image_matches(partial, journal.bank, image_role="after")


def test_original_empty_and_after_seals_provide_only_occupied_independent_lease_inputs(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)

    seals = bank_journal_snapshots(journal)

    assert len(seals) == 7  # Two sources, three old targets, two fresh copies.
    assert all(seal.projected().source_path for seal in seals)
    assert seals[0].projected().source_path == seals[2].projected().source_path
    assert seals[0] is not seals[2]
    assert all(seal.projected().source_identity is not None for seal in seals)


def test_before_restore_and_written_normalization_keep_empty_controls_and_lineage(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    after = project_with_journal_bank(persistence.project, journal)
    after.pad_gain_db[0] = -27.0
    restored = bank_before_project(after, journal)
    assert restored.pad_gain_db[0] == -27.0
    assert restored.pad_gain_db[36] == -12.0
    assert restored.pad_gain_db[70] == -19.0
    assert musical_content_data(restored, 71) == musical_content_data(persistence.project, 71)
    receipt = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, after)
    assert receipt.projected().sample_paths[36] == "samples/Take0.wav"
    assert written_bank_image_matches(receipt.projected(), journal.bank, image_role="after")
    assert journal.changed(phase="native_ack_observed").committed(receipt).project_image_json == (
        receipt.written_text
    )


@pytest.mark.parametrize("damage", ["last_before", "last_after", "35", "unknown", "duplicate_key"])
def test_corrupt_final_cell_or_incomplete_nested_seal_is_rejected(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, damage: str
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    data = json.loads(journal.model_dump_json())
    payload = json.loads(data["bank"]["payload_json"])
    if damage == "last_before":
        payload["slots"][35]["before"]["gain_db"] = -31.0
    elif damage == "last_after":
        payload["slots"][35]["after"]["gain_db"] = -32.0
    elif damage == "35":
        payload["slots"].pop()
    elif damage == "unknown":
        payload["slots"][35]["before"]["key_intent"]["unknown"] = 1
    else:
        data["bank"]["payload_json"] = data["bank"]["payload_json"].replace(
            "{", '{"target_bank":1,', 1
        )
        with pytest.raises(ValueError, match="duplicate object keys"):
            validated_bank_history([json.dumps(data)], _TRANSACTION)
        return
    data["bank"]["payload_json"] = json.dumps(payload)
    rejection = {
        "last_before": "Bank participant musical content changed",
        "last_after": "Bank after image changes",
        "35": "at least 36 items",
        "unknown": "unsupported fields|Extra inputs are not permitted",
    }[damage]
    with pytest.raises(ValueError, match=rejection):
        validated_bank_history([json.dumps(data)], _TRANSACTION)


def test_invalid_earlier_history_phase_and_changed_fixed_lineage_cannot_hide_behind_tail(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    pending = journal.changed(phase="publication_pending")
    with pytest.raises(ValueError, match="phase/revision"):
        validated_bank_history(
            [
                journal.model_dump_json(),
                journal.changed(phase="native_ack_observed").model_dump_json(),
            ],
            _TRANSACTION,
        )
    damaged = json.loads(journal.model_dump_json())
    damaged["unknown"] = True
    with pytest.raises(ValueError, match="ordinal"):
        validated_bank_history([json.dumps(damaged), pending.model_dump_json()], _TRANSACTION)
    different = journal.changed(transaction_id=_SUCCESSOR)
    with pytest.raises(ValueError, match="original seals"):
        validated_bank_history(
            [journal.model_dump_json(), different.model_dump_json()], _TRANSACTION
        )


def test_resume_edge_preserves_fixed_bank_and_rejects_foreign_or_terminal_parent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    child = journal.changed(transaction_id=_SUCCESSOR, resume_of=_TRANSACTION)
    validate_bank_successor(journal, child)
    with pytest.raises(ValueError, match="lineage"):
        validate_bank_successor(journal, child.changed(resume_of="c" * 32))
    with pytest.raises(ValueError, match="lineage"):
        validate_bank_successor(journal.changed(phase="cancelled"), child)
    with pytest.raises(ValueError, match="lineage"):
        validate_bank_successor(
            journal, child.changed(config_reference=str(tmp_path / "other.json"))
        )


@pytest.mark.parametrize("complete", [True, False])
def test_uncertain_append_owns_exact_complete_or_partial_native_ordinal(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, complete: bool
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
            message = "injected failure after actual Bank ordinal write"
            raise OSError(message)

    owner = BankJournalOwner(
        tmp_path / "samples", journal, cast("MaterialMigrationJournalStore", FailingStore())
    )
    if complete:
        assert owner.append(journal) == journal
        assert owner.intended is None
        assert owner.encoded == [journal.model_dump_json()]
        assert owner.append(journal) == journal
    else:
        with pytest.raises((ValueError, RuntimeError)):
            owner.append(journal)
        assert owner.journal is None
        assert owner.intended == journal.model_dump_json()
        with pytest.raises(ValueError, match="Previous Bank append"):
            owner.append(journal.changed(error="cannot replace uncertain evidence"))
        path = tmp_path / "samples" / ".material-migrations" / _TRANSACTION / "00.json"
        assert path.read_text(encoding="utf-8") == journal.model_dump_json()[:100]


def test_retry_keeps_capacity_and_bound_failures_leave_existing_store_and_writer_owned(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    for _ in range(3):
        assert owner.append(journal) == journal
    assert len(owner.store.history()) == 1
    for index in range(15):
        owner.append(journal.changed(error=f"bounded retry {index}"))
    before = owner.store.history()
    with pytest.raises(ValueError, match="capacity"):
        owner.append(journal.changed(error="one too many"))
    assert owner.store.history() == before
    with pytest.raises(PersistenceFenceError, match="another material migration"):
        persistence.capture_migration(_SUCCESSOR)


def test_byte_and_inventory_capacity_are_checked_without_a_new_publication_or_store(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    monkeypatch.setattr(bank_journal, "JOURNAL_BYTES", 1)
    with pytest.raises(ValueError, match="record size"):
        journal.changed(error="too large")
    monkeypatch.undo()
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(bank_journal, "JOURNAL_TRANSACTIONS", 0)
    with pytest.raises(ValueError, match="inventory capacity"):
        BankJournalOwner(tmp_path / "samples", journal)
    assert not (tmp_path / "samples" / ".material-migrations" / _TRANSACTION).exists()


def test_receipt_revision_digest_inheritance_and_unchecked_model_are_not_admission_shortcuts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    candidate = project_with_journal_bank(persistence.project, journal)
    receipt = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    committed = journal.changed(phase="native_ack_observed").committed(receipt)
    with pytest.raises(ValidationError):
        committed.changed(committed_config_sha256="0" * 64)
    with pytest.raises(ValidationError):
        committed.changed(committed_revision=receipt.revision + 1)
    changed = receipt.projected()
    changed.key_lock = False
    text = changed.model_dump_json()
    forged = ConfigCommitReceipt(receipt.revision, hashlib.sha256(text.encode()).hexdigest(), text)
    with pytest.raises(ValidationError, match="inheritance"):
        journal.committed(forged)
    unchecked = BankArrangementJournal.model_construct(
        **(committed.model_dump() | {"schema_version": 2})
    )
    with pytest.raises(ValidationError):
        project_with_journal_bank(candidate, unchecked)


def test_row_comparison_reuses_copys_unclaimed_stem_availability_semantics(tmp_path: Path) -> None:
    project = _full_project()
    image = project.model_dump_json()
    journal = BankArrangementJournal(
        transaction_id=_TRANSACTION,
        config_reference=str(tmp_path / "bank.json"),
        captured_revision=project.config_revision,
        intent_revision=project.config_revision,
        captured_config_sha256=None,
        bank=capture_bank_snapshot(project, 0, 1),
        captured_project_image_json=image,
        project_image_json=image,
        image_role="before",
        phase="captured",
    )
    candidate = project_with_journal_bank(project, journal)
    assert candidate.stem_cache[36] is not None
    candidate.stem_cache[36].available = True
    assert written_bank_image_matches(candidate, journal.bank, image_role="after")
    recognized = project_with_journal_bank(candidate, journal)
    assert recognized.stem_cache[36] is not None
    assert recognized.stem_cache[36].available is True
    for seal in bank_journal_snapshots(journal):
        entry = seal.projected().stem_cache
        if entry is not None:
            assert entry.available is False


def test_actual_receipt_cannot_hide_unrecorded_unrelated_same_revision_intent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    candidate = project_with_journal_bank(persistence.project, journal)
    candidate.volume = 0.4
    receipt = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    before = owner.store.history()

    with pytest.raises(ValueError, match="phase/revision"):
        owner.append(acknowledged.committed(receipt))

    assert owner.store.history() == before
    assert owner.journal == acknowledged
    assert owner.intended is None


@pytest.mark.parametrize("state", ["before", "after"])
@pytest.mark.parametrize("conflicting_slot", [0, 108])
def test_fixed_after_id_collision_in_source_or_outside_never_projects_any_target(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, state: str, conflicting_slot: int
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    project = (
        persistence.project.model_copy(deep=True)
        if state == "before"
        else project_with_journal_bank(persistence.project, journal)
    )
    fixed = journal.bank.projected().slots[0].after.source_identity
    assert fixed is not None
    project.pad_content[conflicting_slot] = fixed
    before = project.model_dump_json()

    with pytest.raises(ValueError, match="duplicate content identities"):
        project_with_journal_bank(project, journal)

    assert project.model_dump_json() == before


def test_committed_record_rejects_duplicate_after_identity_outside_the_target_bank(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    candidate = project_with_journal_bank(persistence.project, journal)
    candidate.pad_content[108] = candidate.pad_content[36]
    text = candidate.model_dump_json()
    duplicate = ConfigCommitReceipt(
        persistence.revision, hashlib.sha256(text.encode("utf-8")).hexdigest(), text
    )

    with pytest.raises(ValidationError, match="duplicate content identities"):
        journal.committed(duplicate)


def test_new_valid_source_identity_is_preserved_without_changing_the_fixed_copy_lineage(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    original = persistence.project.pad_content[0]
    assert original is not None
    persistence.project.pad_content[0] = original.model_copy(update={"instance_id": "9" * 32})

    candidate = project_with_journal_bank(persistence.project, journal)

    assert candidate.pad_content[0] is not None
    assert candidate.pad_content[0].instance_id == "9" * 32
    assert candidate.pad_content[36] is not None
    assert candidate.pad_content[36].copied_from == original.instance_id
    assert candidate.pad_content[36] == journal.bank.projected().slots[0].after.source_identity
