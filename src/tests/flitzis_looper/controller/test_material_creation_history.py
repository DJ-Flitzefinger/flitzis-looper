"""Durable history and visible retry policy with explicit terminal-status doubles.

The native journal, saved config and file bytes are real. Numeric birth identities
and queue statuses here are controller-policy inputs; they grant no native cleanup
authority. Genuine birth, fresh ACK and physical readers are covered in Rust children.
"""

import hashlib
import json
from dataclasses import dataclass
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.material_migration_cleanup import (
    CleanupResult,
    MigrationArtifactReconciler,
    MigrationCleanupQueue,
)
from flitzis_looper.controller.material_migration_history import merge_artifacts, rollback_artifacts
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.material_migration_model import (
    MaterialCreationReceipt,
    MaterialMigrationAlias,
    MaterialMigrationJournal,
    MigrationArtifactEvidence,
    MigrationArtifactRecord,
    MigrationAssignment,
    MigrationFileEvidence,
)
from flitzis_looper.models import ProjectState
from flitzis_looper_audio import MaterialMigrationJournalStore
from tests.conftest import write_mono_pcm16_wav

if TYPE_CHECKING:
    from pathlib import Path


def _policy_birth(samples: Path) -> tuple[MigrationArtifactRecord, MaterialMigrationAlias]:
    old = samples / "old.wav"
    write_mono_pcm16_wav(old, 48_000)
    material = samples / "Readable owned root.wav"
    material.mkdir()
    original = material / old.name
    original.write_bytes(old.read_bytes())
    digest = hashlib.sha256(original.read_bytes()).hexdigest()
    binding = material / ".material.json"
    binding.write_text(
        json.dumps({
            "schema_version": 1,
            "sha256": digest,
            "bytes": original.stat().st_size,
            "original_leaf": original.name,
        }),
        encoding="utf-8",
    )
    creation = MaterialCreationReceipt(
        root_reference="samples/Readable owned root.wav",
        root_identity=(1, 2, 3),
        binding=MigrationFileEvidence(
            name=binding.name,
            identity=(4, 5, 6),
            bytes=binding.stat().st_size,
            sha256=hashlib.sha256(binding.read_bytes()).hexdigest(),
        ),
    )
    reference = f"{creation.root_reference}/{original.name}"
    evidence = MigrationArtifactEvidence(
        kind="original",
        reference=reference,
        identity=(7, 8, 9),
        samples_identity=(10, 11, 12),
        files=(
            MigrationFileEvidence(
                name=original.name, identity=(7, 8, 9), bytes=original.stat().st_size, sha256=digest
            ),
        ),
        material_creation=creation,
    )
    alias = MaterialMigrationAlias(
        transaction_id="a" * 32,
        material_id=digest[:32],
        old_reference="samples/old.wav",
        new_reference=reference,
        original_sha256=digest,
        original_bytes=original.stat().st_size,
        decoder_identity="b" * 64,
        playback_identity="c" * 64,
        cache_path=f"{creation.root_reference}/.pcm-cache/v1/.ready-{'d' * 32}",
        old_source_version=f"samples/old.wav|sha256-v1:{digest}",
        new_source_version=f"{reference}|sha256-v1:{digest}",
        material_creation=creation,
    )
    return MigrationArtifactRecord(role="target", created=True, evidence=evidence), alias


def _journal(
    writer: ProjectPersistence, artifact: MigrationArtifactRecord, alias: MaterialMigrationAlias
) -> MaterialMigrationJournal:
    return MaterialMigrationJournal(
        transaction_id=alias.transaction_id,
        phase="failed",
        config_reference=writer.config_reference,
        captured_revision=writer.revision,
        snapshot_json=writer.project.model_dump_json(),
        assignments=(
            MigrationAssignment(
                sample_id=0, instance_id="1" * 32, old_reference=alias.old_reference
            ),
        ),
        alias=alias,
        artifacts=(artifact,),
        error="controller-only terminal-status fixture",
    )


def test_terminal_transfer_keeps_birth_origin_until_root_terminal_before_parent_compaction(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    artifact, alias = _policy_birth(samples)
    terminal = artifact.model_copy(update={"original_retired": True})
    current = ProjectState()
    writer = ProjectPersistence(current)
    writer.config_path = tmp_path / "project.config.json"
    writer.flush()
    parent = _journal(writer, terminal, alias)
    parent_store = MaterialMigrationJournalStore(str(samples), parent.transaction_id)
    parent_store.append(parent.model_dump_json())
    child_alias = MaterialMigrationAlias.model_validate(
        alias.model_dump()
        | {
            "transaction_id": "e" * 32,
            "resume_of": parent.transaction_id,
        }
    )
    current.material_migrations[child_alias.transaction_id] = child_alias
    writer.mark_dirty()
    writer.flush()
    child = parent.changed(
        transaction_id=child_alias.transaction_id,
        phase="config_committed",
        error=None,
        alias=child_alias,
        resume_of=parent.transaction_id,
        artifacts=(),
        committed_revision=writer.revision,
        committed_config_sha256=hashlib.sha256(writer.config_path.read_bytes()).hexdigest(),
    )
    child_store = MaterialMigrationJournalStore(str(samples), child.transaction_id)
    child_store.append(child.model_dump_json())
    assert artifact.evidence.material_creation is not None
    root = tmp_path / artifact.evidence.material_creation.root_reference
    reconciler = MigrationArtifactReconciler(samples, writer.config_path)
    try:
        transferred = reconciler.transfer_history(parent, child, current).result(timeout=10)
        inherited = transferred.artifacts[0]
        assert inherited.role == "rollback"
        assert not inherited.created
        assert inherited.original_retired
        assert inherited.evidence == artifact.evidence
        assert rollback_artifacts(parent) == (inherited,)
        assert parent_store.history() == [parent.model_dump_json()]
        assert (
            MaterialMigrationJournal.model_validate_json(child_store.history()[-1]) == transferred
        )
        assert root.is_dir()
        assert (root / ".material.json").is_file()
        assert merge_artifacts(
            (inherited,), (inherited.model_copy(update={"original_retired": False}),)
        ) == (inherited,)
        with pytest.raises(ValueError, match="lineage changed"):
            merge_artifacts((inherited,), (inherited.model_copy(update={"created": True}),))
        del parent_store  # release this test's real journal-directory reader before compaction
        # Remove only these owned test fixtures; this simulates observed root terminal
        # for metadata compaction and is not a native artifact-deletion acceptance test.
        for leaf in root.iterdir():
            leaf.unlink()
        root.rmdir()
        again = reconciler.transfer_history(parent, transferred, current).result(timeout=10)
        assert again == transferred
        assert not (samples / ".material-migrations" / parent.transaction_id).exists()
        assert (
            MaterialMigrationJournal.model_validate_json(child_store.history()[-1]) == transferred
        )
        assert ProjectState.model_validate_json(writer.config_path.read_bytes()) == current
    finally:
        reconciler.shut_down()


@dataclass(slots=True)
class _TerminalRetryCase:
    current: ProjectState
    writer: ProjectPersistence
    journal: MaterialMigrationJournal
    store: MaterialMigrationJournalStore
    lease: Mock
    pending: CleanupResult
    queue: MigrationCleanupQueue
    reconciler: MigrationArtifactReconciler
    config_bytes: bytes
    binding: Path
    binding_bytes: bytes

    def assert_saved_bytes_unchanged(self) -> None:
        assert self.binding.read_bytes() == self.binding_bytes
        assert self.writer.config_path.read_bytes() == self.config_bytes


def _terminal_retry_case(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> _TerminalRetryCase:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    artifact, alias = _policy_birth(samples)
    writer = ProjectPersistence(ProjectState())
    writer.config_path = tmp_path / "project.config.json"
    writer.flush()
    journal = _journal(writer, artifact, alias)
    store = MaterialMigrationJournalStore(str(samples), journal.transaction_id)
    store.append(journal.model_dump_json())
    lease = Mock()
    lease.original_retirement_status.return_value = "complete"
    lease.retirement_status.return_value = "pending"
    lease.retirement_error.return_value = None
    pending = CleanupResult(
        queued=(artifact.evidence.reference,),
        pending=(artifact.evidence.reference,),
        leases=(lease,),
        journal=journal,
        original_leases=((artifact.evidence.reference, lease),),
    )
    queue = MigrationCleanupQueue(samples)
    queue._pending = pending
    queue._journal = journal
    reconciler = MigrationArtifactReconciler(samples, writer.config_path)
    config_bytes = writer.config_path.read_bytes()
    creation = artifact.evidence.material_creation
    assert creation is not None
    binding = tmp_path / creation.root_reference / ".material.json"
    return _TerminalRetryCase(
        current=writer.project,
        writer=writer,
        journal=journal,
        store=store,
        lease=lease,
        pending=pending,
        queue=queue,
        reconciler=reconciler,
        config_bytes=config_bytes,
        binding=binding,
        binding_bytes=binding.read_bytes(),
    )


def _block_terminal_append(case: _TerminalRetryCase, monkeypatch: pytest.MonkeyPatch) -> Mock:
    """Keep Native journal inspection real while denying only the terminal append."""
    blocked = Mock(wraps=case.store)
    blocked.append.side_effect = OSError("durable terminal append denied")
    monkeypatch.setattr(
        "flitzis_looper.controller.material_migration_cleanup.MaterialMigrationJournalStore",
        Mock(open=Mock(return_value=blocked)),
    )
    return blocked


@pytest.mark.parametrize("failure", ["append", "current_record"])
def test_original_terminal_journal_failure_keeps_pending_root_lease_and_retries_visibly(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    case = _terminal_retry_case(tmp_path, monkeypatch)
    try:
        blocked = None
        changed = None
        if failure == "append":
            blocked = _block_terminal_append(case, monkeypatch)
        else:
            changed = case.journal.changed(error="actual newer durable journal wins")
            case.store.append(changed.model_dump_json())
        before = case.store.history()
        case.queue.poll(case.reconciler, case.current, admit=False)
        assert case.queue.error is not None
        assert ("append denied" if failure == "append" else "journal changed") in case.queue.error
        assert case.queue._pending is case.pending
        assert case.pending.leases == (case.lease,)
        assert case.pending.original_leases == (
            (case.journal.artifacts[0].evidence.reference, case.lease),
        )
        assert case.pending.journal == case.journal
        assert case.pending.journal is not None
        assert not case.pending.journal.cleanup_complete
        assert case.store.history() == before
        case.lease.release.assert_not_called()
        case.assert_saved_bytes_unchanged()
        if failure == "append":
            assert blocked is not None
            blocked.append.side_effect = None
            monkeypatch.setattr(
                "flitzis_looper.controller.material_migration_cleanup.MaterialMigrationJournalStore",
                MaterialMigrationJournalStore,
            )
        else:
            # Bind the queue to the actual latest record before retrying; no stale
            # metadata is allowed to overwrite the external current-record change.
            assert changed is not None
            case.pending.journal = changed
        case.queue.poll(case.reconciler, case.current, admit=False)
        latest = MaterialMigrationJournal.model_validate_json(case.store.history()[-1])
        assert latest.artifacts[0].original_retired
        assert not latest.cleanup_complete
        assert case.queue._pending is case.pending
        assert case.pending.leases == (case.lease,)
        assert case.pending.original_leases == ()
        assert case.lease.retirement_status.return_value == "pending"
        case.lease.retirement_status.return_value = "complete"
        case.queue.poll(case.reconciler, case.current, admit=False)
        complete = MaterialMigrationJournal.model_validate_json(case.store.history()[-1])
        assert complete.cleanup_complete
        assert complete.artifacts[0].original_retired
        assert case.queue._pending is None
        case.assert_saved_bytes_unchanged()
    finally:
        case.reconciler.shut_down()
