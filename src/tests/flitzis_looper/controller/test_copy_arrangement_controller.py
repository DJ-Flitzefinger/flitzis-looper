"""Live occupied Copy boundary failures retain the original complete transaction."""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.copy_arrangement_journal import validated_copy_history
from tests.flitzis_looper.controller.test_pad_arrangement import _harness, _occupy_target

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller.copy_arrangement_journal import CopyArrangementJournal
    from flitzis_looper.controller.pad_arrangement import _Copy
    from flitzis_looper.controller.persistence import ConfigCommitReceipt
    from flitzis_looper.models import ProjectState
    from tests.flitzis_looper.controller.test_pad_arrangement import _Harness


def _assert_settled(harness: _Harness, copy: _Copy) -> None:
    assert not harness.controller.active
    assert harness.persistence._migration_owner is None
    assert copy.journal_custody.readers == []
    assert harness.native.finish_count == 1
    assert harness.native.publish_count == 1
    assert copy.journal is not None
    history = validated_copy_history(copy.journal.store.history(), copy.owner)
    assert history[-1].phase == "settled"
    assert history[-1].target_identity == copy.identity


@pytest.fixture(autouse=True)
def _sample_custody(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    (samples / "source.wav").write_bytes(b"scripted source custody")
    (samples / "target.wav").write_bytes(b"scripted predecessor custody")


@pytest.mark.parametrize("boundary", ["native_ack_observed", "config_committed", "settled"])
def test_claimed_journal_failures_keep_pins_writer_and_same_receipt_until_retry(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, boundary: str
) -> None:
    retired: list[int] = []
    harness = _harness(tmp_path, removed=lambda slot, _old: retired.append(slot))
    predecessor = _occupy_target(harness)
    assert harness.controller.copy_overwrite(0, 215)
    copy = harness.controller._copy
    assert copy is not None
    journal = copy.journal
    assert journal is not None
    original_append = journal.append
    rejected = False

    def append(record: CopyArrangementJournal) -> CopyArrangementJournal:
        nonlocal rejected
        if record.phase == boundary and not rejected:
            rejected = True
            message = "journal boundary temporarily unavailable"
            raise RuntimeError(message)
        return original_append(record)

    monkeypatch.setattr(journal, "append", append)
    original_commit = harness.persistence.commit_migration_receipt
    receipts: list[ConfigCommitReceipt] = []

    def commit(owner: str, revision: int, image: ProjectState) -> ConfigCommitReceipt:
        receipt = original_commit(owner, revision, image)
        receipts.append(receipt)
        return receipt

    monkeypatch.setattr(harness.persistence, "commit_migration_receipt", commit)
    harness.jobs.run()
    harness.controller.poll()
    harness.native.complete_ack = True
    harness.controller.poll()
    assert rejected
    assert harness.controller.active
    assert harness.persistence._migration_owner == copy.owner
    assert copy.journal_custody.acquired
    assert all(not reader.released for reader in copy.journal_custody.readers)
    assert harness.native.cancel_count == 0
    if boundary == "native_ack_observed":
        assert retired == []
        assert harness.project.pad_content[215] == predecessor
        assert receipts == []
    else:
        assert retired == [215]
        assert len(receipts) == 1
        assert harness.persistence.config_path.read_bytes() == receipts[0].written_text.encode()
    if boundary == "settled":
        assert harness.native.finish_count == 1
    harness.controller.poll()
    _assert_settled(harness, copy)
    assert len(receipts) == 1
    assert retired == [215]


def test_publication_journal_failure_cancels_only_proven_unclaimed_copy_without_enqueue(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path)
    predecessor = _occupy_target(harness)
    assert harness.controller.copy_overwrite(0, 215)
    copy = harness.controller._copy
    assert copy is not None
    journal = copy.journal
    assert journal is not None
    append = journal.append

    def reject_publication(record: CopyArrangementJournal) -> CopyArrangementJournal:
        if record.phase == "publication_pending":
            message = "publication evidence unavailable"
            raise RuntimeError(message)
        return append(record)

    monkeypatch.setattr(journal, "append", reject_publication)
    harness.jobs.run()
    harness.controller.poll()
    assert not harness.controller.active
    assert harness.native.publish_count == 0
    assert harness.native.cancel_count == 1
    assert harness.project.pad_content[215] == predecessor
    assert harness.persistence._migration_owner is None
    assert journal.journal is not None
    assert journal.journal.phase == "cancelled"
    assert copy.journal_custody.readers == []


def test_newer_surviving_source_intent_is_journaled_before_second_actual_config_replace(
    tmp_path: Path,
) -> None:
    harness = _harness(tmp_path)
    _occupy_target(harness)
    harness.assets.planned_retirement_failures = 1
    assert harness.controller.copy_overwrite(0, 215)
    copy = harness.controller._copy
    assert copy is not None
    harness.jobs.run()
    harness.controller.poll()
    harness.native.complete_ack = True
    harness.controller.poll()
    assert harness.controller.active
    harness.project.manual_bpm[0] = 97.125
    harness.project.volume = 0.25
    harness.persistence.mark_dirty()
    harness.controller.poll()
    assert not harness.controller.active
    assert harness.project.manual_bpm[0] == 97.125
    assert harness.project.manual_bpm[215] == 123.45
    assert copy.journal is not None
    history = validated_copy_history(copy.journal.store.history(), copy.owner)
    phases = [record.phase for record in history]
    assert phases == [
        "captured",
        "publication_pending",
        "native_ack_observed",
        "config_committed",
        "native_ack_observed",
        "config_committed",
        "settled",
    ]
    assert history[4].intent_revision > history[3].intent_revision
    assert history[4].source == history[0].source
    assert history[-1].target_identity == copy.identity
