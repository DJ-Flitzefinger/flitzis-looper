"""Historical bank recognition preserves complete images without restored Native ACKs."""

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from flitzis_looper.controller.bank_arrangement_journal import (
    BankJournalOwner,
    bank_before_project,
    project_with_journal_bank,
)
from flitzis_looper.controller.bank_arrangement_recovery import inspect_bank_recovery
from flitzis_looper.copy_snapshot import CONTENT_FIELDS
from tests.flitzis_looper.controller.test_bank_arrangement_journal import (
    _TRANSACTION,
    _acknowledged,
    _captured,
)


@pytest.mark.parametrize(
    "crash_point", ["captured", "queued", "ack", "replaced", "recorded", "settled"]
)
def test_real_store_reopens_each_complete_config_boundary_without_recreating_runtime_authority(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, crash_point: str
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    tail = owner.append(journal)
    if crash_point != "captured":
        tail = owner.append(tail.changed(phase="publication_pending"))
    if crash_point not in {"captured", "queued"}:
        tail = owner.append(tail.changed(phase="native_ack_observed"))
    if crash_point in {"replaced", "recorded", "settled"}:
        candidate = project_with_journal_bank(persistence.project, journal)
        receipt = persistence.commit_migration_receipt(
            _TRANSACTION, persistence.revision, candidate
        )
        if crash_point in {"recorded", "settled"}:
            tail = owner.append(tail.committed(receipt))
        if crash_point == "settled":
            tail = owner.append(tail.changed(phase="settled"))
    actual_bytes = persistence.config_path.read_bytes()
    reopened = BankJournalOwner.open(tmp_path / "samples", _TRANSACTION)
    assert reopened.journal is not None

    decision = inspect_bank_recovery(reopened.journal, actual_bytes, history=reopened.encoded)

    expected = {
        "captured": "original_pending",
        "queued": "original_pending",
        "ack": "original_pending",
        "replaced": "recognized_after",
        "recorded": "recognized_after",
        "settled": "settled",
    }[crash_point]
    assert decision.disposition == expected
    assert persistence.config_path.read_bytes() == actual_bytes
    assert reopened.store.history() == owner.encoded
    if expected == "recognized_after":
        assert decision.receipt is not None
        assert decision.receipt.written_text.encode("utf-8") == actual_bytes
        image = decision.projected()
        assert image is not None
        assert image.pad_content[36] is not None
        assert image.pad_content[37] is None
        assert image.pad_content[71] is not None
        assert image.pad_gain_db[36] == -6.0


def test_fresh_subprocess_reopens_genuine_store_after_config_replace_before_receipt_ordinal(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    _acknowledged(owner, journal)
    candidate = project_with_journal_bank(persistence.project, journal)
    persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    script = """
import json
import sys
from pathlib import Path
from flitzis_looper.controller.bank_arrangement_journal import BankJournalOwner
from flitzis_looper.controller.bank_arrangement_recovery import inspect_bank_recovery
owner = BankJournalOwner.open(Path(sys.argv[1]), sys.argv[3])
decision = inspect_bank_recovery(
    owner.journal, Path(sys.argv[2]).read_bytes(), history=owner.encoded
)
image = decision.projected()
assert decision.disposition == 'recognized_after'
assert decision.receipt is not None
assert image.pad_content[36] is not None and image.pad_content[37] is None
assert image.pad_content[36].copied_from == '00000000000000000000000000000001'
print(json.dumps({'disposition': decision.disposition, 'ordinals': len(owner.encoded)}))
"""
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            script,
            str(tmp_path / "samples"),
            str(persistence.config_path),
            _TRANSACTION,
        ],
        cwd=tmp_path,
        env=os.environ
        | {
            "PYTHONPATH": os.pathsep.join([
                str(Path(__file__).resolve().parents[3]),
                os.environ.get("PYTHONPATH", ""),
            ])
        },
        capture_output=True,
        text=True,
        check=True,
        timeout=30,
    )
    assert json.loads(result.stdout) == {"disposition": "recognized_after", "ordinals": 3}
    assert owner.store.history() == owner.encoded


def test_clear_recognition_retains_all_36_empty_after_witnesses(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch, "clear")
    owner = BankJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    candidate = project_with_journal_bank(persistence.project, journal)
    persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)

    decision = inspect_bank_recovery(
        acknowledged, persistence.config_path.read_bytes(), history=owner.encoded
    )

    assert decision.disposition == "recognized_after"
    restored = decision.projected()
    assert restored is not None
    assert all(content is None for content in restored.pad_content[36:72])
    assert restored.pad_content[0] == persistence.project.pad_content[0]
    assert all(row.after.source_identity is None for row in journal.bank.projected().slots)


def test_newer_source_and_outside_intent_recovers_with_the_original_fixed_after_bank(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    persistence.project.pad_gain_db[0] = -24.0
    persistence.project.pad_gain_db[108] = -7.0
    persistence.project.volume = 0.25
    persistence.mark_dirty()
    current = acknowledged.changed(
        intent_revision=persistence.revision,
        project_image_json=bank_before_project(persistence.project, journal).model_dump_json(),
    )
    owner.append(current)
    candidate = project_with_journal_bank(persistence.project, current)
    persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)

    decision = inspect_bank_recovery(
        current, persistence.config_path.read_bytes(), history=owner.encoded
    )

    assert decision.disposition == "recognized_after"
    restored = decision.projected()
    assert restored is not None
    assert restored.pad_gain_db[0] == -24.0
    assert restored.pad_gain_db[36] == -6.0
    assert restored.pad_gain_db[108] == -7.0
    assert restored.volume == 0.25


def test_prior_actual_receipt_keeps_newer_recorded_intent_without_inventing_new_receipt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    candidate = project_with_journal_bank(persistence.project, journal)
    receipt = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    committed = owner.append(acknowledged.committed(receipt))
    old_bytes = persistence.config_path.read_bytes()
    persistence.project.pad_gain_db[0] = -24.0
    persistence.project.volume = 0.25
    persistence.mark_dirty()
    latest = committed.changed(
        phase="native_ack_observed",
        image_role="before",
        committed_revision=None,
        committed_config_sha256=None,
        intent_revision=persistence.revision,
        project_image_json=bank_before_project(persistence.project, journal).model_dump_json(),
    )
    owner.append(latest)

    decision = inspect_bank_recovery(latest, old_bytes, history=owner.encoded)

    assert decision.disposition == "recognized_after"
    assert decision.receipt is None
    restored = decision.projected()
    assert restored is not None
    assert restored.pad_gain_db[0] == -24.0
    assert restored.pad_gain_db[36] == -6.0
    assert restored.volume == 0.25
    assert restored.config_revision == persistence.revision
    assert persistence.config_path.read_bytes() == old_bytes


def test_partial_bank_and_newer_unrecorded_config_remain_conflicted_and_untouched(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    complete = project_with_journal_bank(persistence.project, journal)
    partial = persistence.project.model_copy(deep=True)
    for _, field in CONTENT_FIELDS:
        getattr(partial, field)[36] = getattr(complete, field)[36]
    partial_bytes = partial.model_dump_json().encode("utf-8")
    first = inspect_bank_recovery(acknowledged, partial_bytes, history=owner.encoded)
    complete.volume = 0.4
    complete.config_revision = acknowledged.intent_revision + 1
    newer_bytes = complete.model_dump_json().encode("utf-8")
    second = inspect_bank_recovery(acknowledged, newer_bytes, history=owner.encoded)

    assert first.disposition == second.disposition == "conflict"
    assert first.projected() is None
    assert second.receipt is None
    assert owner.store.history() == owner.encoded


def test_invalid_earlier_ordinal_or_substituted_scan_tail_never_recovers_valid_config(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    corrupt = json.loads(owner.encoded[0])
    corrupt["unknown"] = True
    history = [json.dumps(corrupt), *owner.encoded[1:]]
    invalid = inspect_bank_recovery(
        acknowledged, persistence.config_path.read_bytes(), history=history
    )
    substituted = inspect_bank_recovery(
        acknowledged.changed(error="not the opened tail"),
        persistence.config_path.read_bytes(),
        history=owner.encoded,
    )
    assert invalid.disposition == substituted.disposition == "conflict"
    assert "history" in str(invalid.error)
    assert owner.store.history() == owner.encoded


def test_missing_config_never_turns_ack_record_into_a_restored_after_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    owner = BankJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    decision = inspect_bank_recovery(acknowledged, None, history=owner.encoded)
    assert decision.disposition == "conflict"
    assert decision.projected() is None
    assert decision.receipt is None
