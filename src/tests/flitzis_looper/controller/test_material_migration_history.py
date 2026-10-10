"""Actual bounded config/journal/receipt reconciliation; no source or timing ACK claims."""

import hashlib
import json
import os
import time
from pathlib import Path

import pytest

from flitzis_looper.controller.material_migration_cleanup import (
    CleanupResult,
    MigrationArtifactReconciler,
    MigrationCleanupQueue,
)
from flitzis_looper.controller.material_migration_history import (
    check_alias_capacity,
    successor_aliases,
)
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.key_intent import MAX_KEY_EPOCH, PadKeyIntent, SourceKeyVersion
from flitzis_looper.material_migration_model import (
    MaterialMigrationAlias,
    MaterialMigrationJournal,
    MigrationArtifactEvidence,
    MigrationArtifactRecord,
    MigrationAssignment,
)
from flitzis_looper.models import STEM_KINDS, PadContentIdentity, ProjectState, StemCacheEntry
from flitzis_looper_audio import MaterialMigrationJournalStore, MigrationArtifactLease
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import write_test_stem_marker


def _alias(index: int, original: Path) -> MaterialMigrationAlias:
    digest = hashlib.sha256(original.read_bytes()).hexdigest()
    material = f"{index + 1:032x}"
    old = f"samples/{original.name}"
    new = f"samples/materials/M{material}/original/{original.name}"
    target = Path(new)
    target.parent.mkdir(parents=True)
    target.write_bytes(original.read_bytes())
    return MaterialMigrationAlias(
        transaction_id=f"{index + 1000:032x}",
        material_id=material,
        old_reference=old,
        new_reference=new,
        original_sha256=digest,
        original_bytes=original.stat().st_size,
        decoder_identity="b" * 64,
        playback_identity="c" * 64,
        cache_path=f"samples/materials/M{material}/.pcm-cache/v1/.ready-{'d' * 32}",
        old_source_version=f"{old}|sha256-v1:{digest}",
        new_source_version=f"{new}|sha256-v1:{digest}",
    )


def _original_record(samples: Path, reference: str) -> MigrationArtifactRecord:
    lease = MigrationArtifactLease.capture(str(samples), reference)
    try:
        evidence = MigrationArtifactEvidence.model_validate_json(lease.receipt_json())
    finally:
        lease.release()
    return MigrationArtifactRecord(role="rollback", created=False, evidence=evidence)


def _settled(reconciler: MigrationArtifactReconciler, result: CleanupResult) -> None:
    deadline = time.monotonic() + 10
    while not reconciler.settled(result):
        assert time.monotonic() < deadline
        time.sleep(0.002)
    assert not result.errors, result.errors


def _capacity_project(samples: Path) -> ProjectState:
    current = ProjectState()
    for index in range(216):
        original = samples / f"original-{index}.wav"
        write_mono_pcm16_wav(original, 48_000)
        alias = _alias(index, original)
        current.sample_paths[index] = alias.old_reference
        current.pad_content[index] = PadContentIdentity(instance_id=f"{index + 5000:032x}")
        current.pad_key_intent[index] = PadKeyIntent(
            source=SourceKeyVersion(version=MAX_KEY_EPOCH, raw_key=f"retained-{index}"),
            correction=f"correction-{index}",
            analysis_epoch=MAX_KEY_EPOCH,
            correction_epoch=MAX_KEY_EPOCH,
            extra_shift=index % 37 - 18,
            base_shift=index % 12 - 5,
            retrigger=bool(index % 2),
        )
        current.material_migrations[alias.transaction_id] = alias
    return current


def _commit_successor(
    index: int,
    parent: MaterialMigrationJournal,
    current: ProjectState,
    persistence: ProjectPersistence,
) -> MaterialMigrationJournal:
    check_alias_capacity(current, parent)
    assert parent.alias is not None
    content = current.pad_content[index]
    assert content is not None
    child_id = f"{index + 10000:032x}"
    alias = MaterialMigrationAlias.model_validate(
        parent.alias.model_dump()
        | {
            "transaction_id": child_id,
            "resume_of": parent.transaction_id,
        }
    )
    revision, captured, previous_sha = persistence.capture_migration(child_id)
    candidate = current.model_copy(deep=True)
    candidate.sample_paths[index] = alias.new_reference
    candidate.pad_content[index] = PadContentIdentity(
        instance_id=content.instance_id,
        material_id=alias.material_id,
    )
    candidate.material_migrations = successor_aliases(candidate, alias, parent)
    committed, config_sha = persistence.commit_migration(child_id, revision, candidate)
    current.sample_paths = candidate.sample_paths
    current.pad_content = candidate.pad_content
    current.material_migrations = candidate.material_migrations
    persistence.release_migration(child_id)
    return MaterialMigrationJournal(
        transaction_id=child_id,
        phase="config_committed",
        resume_of=parent.transaction_id,
        config_reference=persistence.config_reference,
        captured_revision=revision,
        intent_revision=committed,
        config_sha256=previous_sha,
        committed_revision=committed,
        committed_config_sha256=config_sha,
        snapshot_json=captured.model_dump_json(),
        assignments=parent.assignments,
        alias=alias,
        artifacts=parent.artifacts,
    )


def test_all_216_aliases_and_journals_transfer_without_raising_limits_or_losing_intent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    current = _capacity_project(samples)
    parents: list[MaterialMigrationJournal] = []
    expected_keys = tuple(current.pad_key_intent)
    expected_ids = tuple(item.instance_id for item in current.pad_content if item is not None)
    persistence = ProjectPersistence(current)
    persistence.config_path = samples / "capacity.config.json"
    persistence.flush()
    snapshot = current.model_dump_json()
    digest = hashlib.sha256(persistence.config_path.read_bytes()).hexdigest()
    for index, alias in enumerate(current.material_migrations.values()):
        parent = MaterialMigrationJournal(
            transaction_id=alias.transaction_id,
            phase="unresolved",
            config_reference=persistence.config_reference,
            captured_revision=persistence.revision,
            intent_revision=persistence.revision,
            config_sha256=digest,
            snapshot_json=snapshot,
            alias=alias,
            assignments=(
                MigrationAssignment(
                    sample_id=index,
                    instance_id=expected_ids[index],
                    old_reference=alias.old_reference,
                ),
            ),
            artifacts=(_original_record(samples, alias.old_reference),),
            error="interrupted; historical metadata supplies no runtime ACK",
        )
        MaterialMigrationJournalStore(str(samples), parent.transaction_id).append(
            parent.model_dump_json()
        )
        parents.append(parent)
    assert len(MaterialMigrationJournalStore.scan(str(samples))) == 216
    reconciler = MigrationArtifactReconciler(samples, persistence.config_path)
    children: list[MaterialMigrationJournal] = []
    try:
        for index, parent in enumerate(parents):
            child = _commit_successor(index, parent, current, persistence)
            MaterialMigrationJournalStore(str(samples), child.transaction_id).append(
                child.model_dump_json()
            )
            assert len(tuple((samples / ".material-migrations").iterdir())) == 217
            updated = reconciler.transfer_history(parent, child, current).result(timeout=10)
            children.append(updated)
            assert len(current.material_migrations) == 216
            assert len(tuple((samples / ".material-migrations").iterdir())) == 216
            assert not (samples / ".material-migrations" / parent.transaction_id).exists()
            assert updated.artifacts == parent.artifacts
        saved = ProjectState.model_validate_json(persistence.config_path.read_bytes())
        assert tuple(saved.pad_key_intent) == expected_keys
        assert tuple(item.instance_id for item in saved.pad_content if item) == expected_ids
        assert len(saved.material_migrations) == 216
        assert len(MaterialMigrationJournalStore.scan(str(samples))) == 216
        for child in children:
            assert child.alias is not None
            result = reconciler.retire(child, current).result(timeout=10)
            _settled(reconciler, result)
            assert not result.protected
            assert result.queued == (child.alias.old_reference,)
        assert not any(samples.glob("original-*.wav"))
        assert all(
            Path(alias.new_reference).is_file() for alias in saved.material_migrations.values()
        )
    finally:
        reconciler.shut_down()


@pytest.mark.parametrize("damage", ["unknown", "gap", "edited"])
def test_open_appender_and_compaction_reject_new_unknown_or_changed_history_before_write(
    tmp_path: Path, damage: str
) -> None:
    samples = tmp_path / "samples"
    samples.mkdir()
    transaction = "a" * 32
    store = MaterialMigrationJournalStore(str(samples), transaction)
    store.append('{"record":0}')
    store.append('{"record":1}')
    expected = store.history()
    directory = samples / ".material-migrations" / transaction
    if damage == "unknown":
        (directory / "unknown.json").write_text("external data", encoding="utf-8")
    elif damage == "gap":
        (directory / "00.json").unlink()
    else:
        (directory / "00.json").write_text('{"record":9}', encoding="utf-8")
    before = {path.name: path.read_bytes() for path in directory.iterdir()}
    if damage != "edited":
        with pytest.raises((RuntimeError, ValueError)):
            store.append('{"record":2}')
        assert {path.name: path.read_bytes() for path in directory.iterdir()} == before
    with pytest.raises((RuntimeError, ValueError)):
        store.compact(expected)
    assert {path.name: path.read_bytes() for path in directory.iterdir()} == before


def _stem_entry(alias: MaterialMigrationAlias, reference: str, *, old: bool) -> StemCacheEntry:
    cache = Path(reference)
    cache.mkdir(parents=True)
    for kind in STEM_KINDS:
        write_mono_pcm16_wav(cache / f"{kind}.wav", 48_000)
    version = alias.old_source_version if old else alias.new_source_version
    write_test_stem_marker(cache, version)
    return StemCacheEntry(
        source_version=version,
        cache_dir=reference,
        stems=expected_stem_files(reference),
        available=True,
    )


def _published_p2a_history(
    samples: Path,
    alias: MaterialMigrationAlias,
    old_cache: str,
    new_cache: str,
) -> tuple[ProjectState, ProjectPersistence, MaterialMigrationJournal]:
    before = ProjectState()
    before.sample_paths[0] = alias.old_reference
    before.pad_content[0] = PadContentIdentity(instance_id="f" * 32)
    before.stem_cache[0] = _stem_entry(alias, old_cache, old=True)
    before.pad_stem_mix_mode[0] = "all_stems"
    current = before.model_copy(deep=True)
    current.sample_paths[0] = alias.new_reference
    current.pad_content[0] = PadContentIdentity(instance_id="f" * 32, material_id=alias.material_id)
    current.stem_cache[0] = _stem_entry(alias, new_cache, old=False)
    current.material_migrations[alias.transaction_id] = alias
    writer = ProjectPersistence(current)
    writer.config_path = samples / "published.config.json"
    writer.flush()
    current = ProjectState.model_validate_json(writer.config_path.read_bytes())
    writer.project = current
    captured = MaterialMigrationJournal(
        transaction_id=alias.transaction_id,
        config_reference=writer.config_reference,
        captured_revision=0,
        intent_revision=0,
        snapshot_json=before.model_dump_json(),
        assignments=(
            MigrationAssignment(
                sample_id=0, instance_id="f" * 32, old_reference=alias.old_reference
            ),
        ),
    )
    committed = captured.changed(
        phase="config_committed",
        alias=alias,
        committed_revision=current.config_revision,
        committed_config_sha256=hashlib.sha256(writer.config_path.read_bytes()).hexdigest(),
        snapshot_json=current.model_dump_json(),
    )
    store = MaterialMigrationJournalStore(str(samples), alias.transaction_id)
    for record in (captured, committed):
        # Exactly the published P2a representation: no P2b artifact/lineage fields.
        raw = record.model_dump()
        for field in ("artifacts", "resume_of", "cleanup_complete"):
            raw.pop(field)
        if raw["alias"] is not None:
            raw["alias"].pop("resume_of")
        store.append(json.dumps(raw))
    assert not MaterialMigrationJournal.model_validate_json(store.history()[-1]).artifacts
    return current, writer, committed


@pytest.mark.parametrize("retain_current", [True, False])
def test_published_p2a_committed_history_without_ledger_reopens_fresh_original_and_full_wav_proofs(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, retain_current: bool
) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    old = samples / "old.wav"
    write_mono_pcm16_wav(old, 48_000)
    alias = _alias(0, old)
    old_cache = "samples/stems/#1"
    new_cache = f"samples/materials/M{alias.material_id}/stems/.ready-{'e' * 32}"
    current, writer, committed = _published_p2a_history(samples, alias, old_cache, new_cache)
    immutable_new = {path.name: path.read_bytes() for path in Path(new_cache).iterdir()}
    original_new = Path(alias.new_reference).read_bytes()
    if not retain_current:
        current.sample_paths[0] = None
        current.stem_cache[0] = None
        writer.mark_dirty()
        writer.flush()
        current = ProjectState.model_validate_json(writer.config_path.read_bytes())
    reconciler = MigrationArtifactReconciler(samples, writer.config_path)
    try:
        result = reconciler.retire(committed, current).result(timeout=10)
        _settled(reconciler, result)
        assert result.journal is not None
        assert {item.evidence.kind for item in result.journal.artifacts} == {
            "original",
            "stem_directory",
        }
        assert all(item.evidence.samples_identity for item in result.journal.artifacts)
        assert {item.evidence.reference for item in result.journal.artifacts} == {
            alias.old_reference,
            old_cache,
        }
        assert not old.exists()
        assert not Path(old_cache).exists()
        assert not (samples / "stems").exists()
        assert Path(alias.new_reference).read_bytes() == original_new
        assert {path.name: path.read_bytes() for path in Path(new_cache).iterdir()} == immutable_new
        saved = ProjectState.model_validate_json(writer.config_path.read_bytes())
        assert saved == current
        assert saved.pad_stem_mix_mode[0] == "all_stems"
        assert (saved.stem_cache[0] is not None) == retain_current
    finally:
        reconciler.shut_down()


@pytest.mark.parametrize("mutation", ["access", "write", "replace"])
def test_config_inventory_binds_open_handle_and_current_path_but_allows_access_updates(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    mutation: str,
) -> None:
    config = tmp_path / "guard.config.json"
    config.write_text(ProjectState().model_dump_json(), encoding="utf-8")
    initial = config.stat()
    original_fstat = os.fstat
    original_stat = Path.stat
    finished = {"count": 0, "ready": False}

    def finish_read(fd: int) -> os.stat_result:
        metadata = original_fstat(fd)
        finished["count"] += 1
        finished["ready"] = finished["count"] == 2
        return metadata

    def path_stat(path: Path, *, follow_symlinks: bool = True) -> os.stat_result:
        if path == config and finished["ready"]:
            finished["ready"] = False
            if mutation == "access":
                os.utime(config, ns=(initial.st_atime_ns - 10**12, initial.st_mtime_ns))
            elif mutation == "write":
                config.write_text(ProjectState(volume=0.5).model_dump_json(), encoding="utf-8")
                os.utime(config, ns=(initial.st_atime_ns, initial.st_mtime_ns + 10**9))
            else:
                replacement = tmp_path / "replacement.json"
                replacement.write_bytes(config.read_bytes())
                os.replace(replacement, config)
        return original_stat(path, follow_symlinks=follow_symlinks)

    monkeypatch.setattr(os, "fstat", finish_read)
    monkeypatch.setattr(Path, "stat", path_stat)
    if mutation == "access":
        assert MigrationArtifactReconciler._read_config(config) == ProjectState()
    else:
        with pytest.raises(ValueError, match="changed during reference inventory"):
            MigrationArtifactReconciler._read_config(config)


def test_published_old_wav_marker_must_match_saved_lineage_before_cleanup(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    original = samples / "old.wav"
    write_mono_pcm16_wav(original, 48_000)
    alias = _alias(0, original)
    old_cache = "samples/stems/#1"
    new_cache = f"samples/materials/M{alias.material_id}/stems/.ready-{'e' * 32}"
    current, writer, committed = _published_p2a_history(samples, alias, old_cache, new_cache)
    write_test_stem_marker(Path(old_cache), alias.new_source_version)
    before = {path.name: path.read_bytes() for path in Path(old_cache).iterdir()}
    journal_before = (
        samples / ".material-migrations" / alias.transaction_id / "01.json"
    ).read_bytes()
    reconciler = MigrationArtifactReconciler(samples, writer.config_path)
    try:
        with pytest.raises(ValueError, match="old stem marker changed"):
            reconciler.retire(committed, current).result(timeout=10)
        assert original.is_file()
        assert {path.name: path.read_bytes() for path in Path(old_cache).iterdir()} == before
        assert (
            samples / ".material-migrations" / alias.transaction_id / "01.json"
        ).read_bytes() == journal_before
        assert ProjectState.model_validate_json(writer.config_path.read_bytes()) == current
    finally:
        reconciler.shut_down()


def _poll_cleanup(
    queue: MigrationCleanupQueue, reconciler: MigrationArtifactReconciler, current: ProjectState
) -> None:
    deadline = time.monotonic() + 10
    while queue.busy or queue._queue:
        assert time.monotonic() < deadline
        queue.poll(reconciler, current, admit=True)
        time.sleep(0.002)


def _protect_upgraded_cleanup(
    queue: MigrationCleanupQueue,
    reconciler: MigrationArtifactReconciler,
    current: ProjectState,
    committed: MaterialMigrationJournal,
    monkeypatch: pytest.MonkeyPatch,
    failure_mode: str,
) -> None:
    with monkeypatch.context() as patch:
        held = reconciler._guard.lock_inventory() if failure_mode == "gate" else None
        if failure_mode == "worker":

            def fail_references(*_: object) -> tuple[set[Path], set[str], list[str]]:
                message = "actual inventory worker failure after durable upgrade"
                raise RuntimeError(message)

            patch.setattr(reconciler, "_references", fail_references)
        try:
            queue.add(committed)
            _poll_cleanup(queue, reconciler, current)
        finally:
            if held is not None:
                held.release()
    assert queue.error is not None
    if failure_mode == "protected":
        assert queue.error == "cleanup protected by saved references"
    elif failure_mode == "worker":
        assert "actual inventory worker failure" in queue.error


@pytest.mark.parametrize("failure_mode", ["protected", "gate", "worker"])
def test_published_p2a_upgrade_retains_durable_ledger_when_protected_then_retries_actual_cleanup(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    failure_mode: str,
) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    write_mono_pcm16_wav(samples / "old.wav", 48_000)
    alias = _alias(0, samples / "old.wav")
    old_cache = "samples/stems/#1"
    new_cache = f"samples/materials/M{alias.material_id}/stems/.ready-{'e' * 32}"
    current, writer, committed = _published_p2a_history(samples, alias, old_cache, new_cache)
    other = ProjectState(volume=0.5)
    other.sample_paths[215] = alias.old_reference
    entry = current.stem_cache[0]
    assert entry is not None
    other.stem_cache[215] = entry.model_copy(
        update={
            "source_version": alias.old_source_version,
            "cache_dir": old_cache,
            "stems": expected_stem_files(old_cache),
            "available": False,
        }
    )
    other_writer = ProjectPersistence(other)
    other_writer.config_path = samples / "other.config.json"
    other_writer.flush()
    queue = MigrationCleanupQueue(samples)
    reconciler = MigrationArtifactReconciler(samples, writer.config_path)
    try:
        _protect_upgraded_cleanup(queue, reconciler, current, committed, monkeypatch, failure_mode)
        assert len(queue._deferred) == 1
        upgraded = queue._deferred[0]
        assert upgraded.artifacts
        assert not upgraded.cleanup_complete
        store = MaterialMigrationJournalStore.open(str(samples), committed.transaction_id)
        assert MaterialMigrationJournal.model_validate_json(store.history()[-1]) == upgraded
        assert (samples / "old.wav").is_file()
        assert Path(old_cache).is_dir()
        other.sample_paths[215] = None
        other.stem_cache[215] = None
        other_writer.mark_dirty()
        other_writer.flush()
        queue.retry()
        _poll_cleanup(queue, reconciler, current)
        assert queue.error is None
        assert not queue._deferred
        latest = MaterialMigrationJournal.model_validate_json(store.history()[-1])
        assert latest.cleanup_complete
        assert latest.artifacts == upgraded.artifacts
        assert not (samples / "old.wav").exists()
        assert not Path(old_cache).exists()
        assert not (samples / "stems").exists()
        assert Path(alias.new_reference).is_file()
        assert Path(new_cache).is_dir()
        assert ProjectState.model_validate_json(writer.config_path.read_bytes()) == current
        assert ProjectState.model_validate_json(other_writer.config_path.read_bytes()).volume == 0.5
    finally:
        reconciler.shut_down()


@pytest.mark.parametrize("spelling", ["backslash", "absolute"])
def test_published_legacy_stem_reference_matches_native_receipt_after_typed_resolution(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    spelling: str,
) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    original = samples / "old.wav"
    write_mono_pcm16_wav(original, 48_000)
    alias = _alias(0, original)
    old_cache = "samples\\stems\\#1" if spelling == "backslash" else str(samples / "stems/#1")
    new_cache = f"samples/materials/M{alias.material_id}/stems/.ready-{'e' * 32}"
    current, writer, committed = _published_p2a_history(samples, alias, old_cache, new_cache)
    reconciler = MigrationArtifactReconciler(samples, writer.config_path)
    try:
        result = reconciler.retire(committed, current).result(timeout=10)
        _settled(reconciler, result)
        assert not result.protected
        assert result.journal is not None
        assert {item.evidence.reference for item in result.journal.artifacts} == {
            alias.old_reference,
            "samples/stems/#1",
        }
        assert not Path(old_cache).exists()
        assert Path(new_cache).is_dir()
        assert ProjectState.model_validate_json(writer.config_path.read_bytes()) == current
    finally:
        reconciler.shut_down()
