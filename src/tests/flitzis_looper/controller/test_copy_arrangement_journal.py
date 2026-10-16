"""Real occupied Copy journal/Config evidence preserves A, B and fixed C across exit."""

import gc
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import cast

import pytest
from pydantic import ValidationError

from flitzis_looper.controller.copy_arrangement_journal import (
    CopyArrangementJournal,
    CopyJournalOwner,
    copy_after_snapshot,
    project_with_journal_copy,
    validated_copy_history,
    written_copy_image_matches,
)
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.copy_snapshot import capture_copy_snapshot
from flitzis_looper.models import PadContentIdentity, ProjectState
from flitzis_looper_audio import MaterialMigrationJournalStore

_TRANSACTION = "a" * 32


def _captured(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    source_key_lock: bool | None = None,
) -> tuple[ProjectPersistence, CopyArrangementJournal]:
    monkeypatch.chdir(tmp_path)
    project = ProjectState(key_lock=True)
    project.sample_paths[0] = str(tmp_path / "samples" / "A.wav")
    project.pad_content[0] = PadContentIdentity(instance_id="1" * 32, material_id="4" * 32)
    project.pad_key_lock[0] = source_key_lock
    project.pad_gain_db[0] = -6.0
    project.pad_eq_low_db[0] = 3.0
    project.manual_bpm[0] = 123.456789
    project.pad_timing_intent[0] = "tap"
    project.pad_loop_start_s[0] = 1.25
    project.pad_loop_end_s[0] = 12.5
    project.pad_loop_bars[0] = 3.5
    project.sample_paths[215] = "samples/B.wav"
    project.pad_content[215] = PadContentIdentity(instance_id="2" * 32, material_id="5" * 32)
    project.pad_gain_db[215] = -12.0
    project.pad_gain_db[36] = -3.0
    persistence = ProjectPersistence(project)
    persistence.config_path = tmp_path / "samples" / "copy.config.json"
    persistence.flush()
    revision, image, digest = persistence.capture_migration(_TRANSACTION)
    source = capture_copy_snapshot(image, 0)
    identity = PadContentIdentity(
        instance_id="3" * 32,
        material_id=source.projected().source_identity.material_id,
        copied_from=source.projected().source_identity.instance_id,
    )
    return persistence, CopyArrangementJournal(
        transaction_id=_TRANSACTION,
        config_reference=persistence.config_reference,
        captured_revision=revision,
        intent_revision=revision,
        captured_config_sha256=digest,
        source_id=0,
        target_id=215,
        source=source,
        predecessor=capture_copy_snapshot(image, 215),
        after=copy_after_snapshot(source, identity),
        resolved_key_lock=image.resolve_pad_key_lock(0),
        captured_project_image_json=image.model_dump_json(),
        project_image_json=image.model_dump_json(),
        image_role="before",
        phase="captured",
    )


def _acknowledged(
    owner: CopyJournalOwner, journal: CopyArrangementJournal
) -> CopyArrangementJournal:
    owner.append(journal)
    pending = owner.append(journal.changed(phase="publication_pending"))
    return owner.append(pending.changed(phase="native_ack_observed"))


def test_actual_receipt_keeps_surviving_a_and_fixed_c_with_distinct_original_b_evidence(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = CopyJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    candidate = project_with_journal_copy(persistence.project, journal)
    candidate_text = candidate.model_dump_json()
    receipt = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    committed = owner.append(acknowledged.committed(receipt))

    assert set(json.loads(committed.model_dump_json())) == {
        "kind",
        "schema_version",
        "transaction_id",
        "resume_of",
        "config_reference",
        "captured_revision",
        "intent_revision",
        "captured_config_sha256",
        "source_id",
        "target_id",
        "source",
        "predecessor",
        "after",
        "resolved_key_lock",
        "captured_project_image_json",
        "project_image_json",
        "image_role",
        "phase",
        "committed_revision",
        "committed_config_sha256",
        "error",
    }
    assert receipt.written_text.encode("utf-8") == persistence.config_path.read_bytes()
    assert receipt.sha256 == hashlib.sha256(persistence.config_path.read_bytes()).hexdigest()
    assert receipt.sha256 != hashlib.sha256(candidate_text.encode("utf-8")).hexdigest()
    actual = receipt.projected()
    source_identity, target_identity = actual.pad_content[0], actual.pad_content[215]
    assert source_identity is not None
    assert target_identity is not None
    assert [source_identity.instance_id, target_identity.instance_id] == ["1" * 32, "3" * 32]
    assert actual.sample_paths[215] == "samples/A.wav"
    assert actual.pad_gain_db[0] == actual.pad_gain_db[215] == -6.0
    assert actual.manual_bpm[215] == 123.456789
    assert actual.resolve_pad_key_lock(215) is True
    assert committed.predecessor.projected().source_identity.instance_id == "2" * 32
    assert committed.target_identity.copied_from == "1" * 32
    assert committed.source == journal.source
    assert committed.after == journal.after
    assert written_copy_image_matches(actual, journal.after, journal.target_id)
    assert project_with_journal_copy(actual, committed).model_dump() == actual.model_dump()


@pytest.mark.parametrize("phase", ["captured", "publication_pending", "native_ack_observed"])
def test_pending_current_project_cannot_change_fixed_c_global_key_lock_inheritance(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, phase: str
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    current = persistence.project.model_copy(deep=True)
    current.key_lock = False
    with pytest.raises(ValidationError, match="resolved Key Lock inheritance"):
        journal.changed(
            transaction_id="b" * 32,
            resume_of=_TRANSACTION,
            phase=phase,
            project_image_json=current.model_dump_json(),
        )


def test_actual_receipt_cannot_change_fixed_c_global_key_lock_inheritance(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = CopyJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    candidate = project_with_journal_copy(persistence.project, journal)
    candidate.key_lock = False
    receipt = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    assert receipt.written_text.encode("utf-8") == persistence.config_path.read_bytes()
    assert receipt.projected().resolve_pad_key_lock(journal.target_id) is False
    with pytest.raises(ValidationError, match="resolved Key Lock inheritance"):
        owner.append(acknowledged.committed(receipt))
    assert owner.journal == acknowledged
    assert written_copy_image_matches(receipt.projected(), journal.after, journal.target_id)


def test_explicit_fixed_c_key_lock_override_remains_independent_of_newer_global_intent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch, source_key_lock=True)
    persistence.project.key_lock = False
    pending = journal.changed(
        phase="publication_pending",
        project_image_json=persistence.project.model_dump_json(),
    )
    acknowledged = pending.changed(phase="native_ack_observed")
    candidate = project_with_journal_copy(persistence.project, acknowledged)
    receipt = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    committed = acknowledged.committed(receipt)
    assert committed.resolved_key_lock is True
    assert receipt.projected().key_lock is False
    assert receipt.projected().resolve_pad_key_lock(journal.target_id) is True


def test_cancelled_copy_preserves_changed_global_key_lock_without_claiming_c(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    persistence.project.key_lock = False
    cancelled = journal.changed(
        phase="cancelled",
        project_image_json=persistence.project.model_dump_json(),
    )
    assert json.loads(cancelled.project_image_json)["key_lock"] is False
    assert cancelled.resolved_key_lock is True
    assert cancelled.predecessor == journal.predecessor


@pytest.mark.parametrize("source_change", ["edited", "deleted"])
def test_postclaim_before_image_and_actual_receipt_preserve_newer_surviving_source_intent(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    source_change: str,
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = CopyJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    if source_change == "edited":
        persistence.project.pad_gain_db[0] = -24.0
    else:
        persistence.project.pad_content[0] = None
        persistence.project.sample_paths[0] = None
    persistence.project.pad_gain_db[36] = -22.0
    persistence.project.volume = 0.25
    persistence.mark_dirty()
    acknowledged = owner.append(
        acknowledged.changed(
            intent_revision=persistence.revision,
            project_image_json=persistence.project.model_dump_json(),
        )
    )
    candidate = project_with_journal_copy(persistence.project, acknowledged)
    receipt = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    committed = owner.append(acknowledged.committed(receipt))
    settled = owner.append(committed.changed(phase="settled"))

    actual = receipt.projected()
    assert actual.pad_gain_db[215] == -6.0
    assert actual.pad_content[215] == journal.target_identity
    if source_change == "edited":
        assert actual.pad_gain_db[0] == -24.0
        assert actual.pad_content[0] is not None
        assert actual.pad_content[0].instance_id == "1" * 32
    else:
        assert actual.pad_content[0] is None
        assert actual.sample_paths[0] is None
    assert actual.pad_gain_db[36] == -22.0
    assert actual.volume == 0.25
    assert settled.captured_project_image_json == journal.captured_project_image_json
    assert CopyJournalOwner.open(tmp_path / "samples", _TRANSACTION).journal == settled


@pytest.mark.parametrize(
    "crash_point", ["captured", "queued", "ack", "replaced", "recorded", "settled"]
)
def test_real_native_store_reopens_in_fresh_process_at_every_config_receipt_boundary(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    crash_point: str,
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = CopyJournalOwner(tmp_path / "samples", journal)
    current = owner.append(journal)
    if crash_point != "captured":
        current = owner.append(current.changed(phase="publication_pending"))
    if crash_point not in {"captured", "queued"}:
        current = owner.append(current.changed(phase="native_ack_observed"))
    if crash_point in {"replaced", "recorded", "settled"}:
        candidate = project_with_journal_copy(persistence.project, current)
        receipt = persistence.commit_migration_receipt(
            _TRANSACTION, persistence.revision, candidate
        )
        if crash_point in {"recorded", "settled"}:
            current = owner.append(current.committed(receipt))
        if crash_point == "settled":
            owner.append(current.changed(phase="settled"))
    child_code = """
import json, sys
from pathlib import Path
from flitzis_looper.controller.copy_arrangement_journal import (
    CopyJournalOwner, validated_copy_history)
from flitzis_looper.controller.copy_arrangement_recovery import inspect_copy_recovery
owner = CopyJournalOwner.open(Path(sys.argv[1]), sys.argv[2])
decision = inspect_copy_recovery(owner.journal, Path(sys.argv[3]).read_bytes(),
    history=validated_copy_history(owner.encoded, sys.argv[2]))
project = decision.projected()
print(json.dumps({'decision': decision.disposition, 'slots': None if project is None else [
    project.pad_content[i].instance_id if project.pad_content[i] else None for i in [0, 215]],
    'c': owner.journal.target_identity.instance_id,
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
        "captured": "original_pending",
        "queued": "original_pending",
        "ack": "original_pending",
        "replaced": "recognized_after",
        "recorded": "recognized_after",
        "settled": "settled",
    }[crash_point]
    assert actual["decision"] == expected
    assert actual["c"] == "3" * 32
    if expected == "original_pending":
        assert actual["slots"] == ["1" * 32, "2" * 32]
    elif expected == "recognized_after":
        assert actual["slots"] == ["1" * 32, "3" * 32], "recognized C must not be recopied"
        assert actual["revision"] == journal.intent_revision


@pytest.mark.parametrize(
    "field",
    [
        "captured_revision",
        "intent_revision",
        "schema_version",
        "source_id",
        "target_id",
        "committed_revision",
        "resolved_key_lock",
    ],
)
def test_bool_numbers_and_nonboolean_resolved_key_lock_are_rejected(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, field: str
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    data = json.loads(journal.model_dump_json())
    data[field] = 1 if field == "resolved_key_lock" else True
    with pytest.raises(ValidationError):
        CopyArrangementJournal.model_validate_json(json.dumps(data), strict=True)


def _damaged_snapshot_payload(payload: str, damage: str) -> str:
    if damage == "snapshot_duplicate":
        return payload.replace('"gain_db":-6.0', '"gain_db":-6.0,"gain_db":-7.0')
    snapshot = json.loads(payload)
    if damage == "snapshot_nan":
        snapshot["gain_db"] = float("nan")
    elif damage == "nested_runtime_right":
        snapshot["key_intent"]["native_action_ticket"] = "historical rights"
    else:
        snapshot["key_intent"].pop("retrigger")
    return json.dumps(snapshot)


def _damaged_journal_fields(text: str, damage: str) -> str:
    data = json.loads(text)
    if damage == "unknown_kind":
        data["kind"] = "pad_pair"
    elif damage == "unknown_field":
        data["native_ack"] = {"lifetime": 7}
    elif damage == "future_version":
        data["schema_version"] = 2
    elif damage == "incomplete_ordinal":
        data.pop("resume_of")
    elif damage == "incomplete_snapshot_wrapper":
        data["source"].pop("schema_version")
    else:
        project = json.loads(data["captured_project_image_json"])
        if damage == "missing_project_field":
            project.pop("pad_gain_db")
        else:
            project["unknown_runtime_right"] = True
        data["captured_project_image_json"] = json.dumps(project)
    return json.dumps(data)


@pytest.mark.parametrize(
    "damage",
    [
        "outer_duplicate",
        "snapshot_duplicate",
        "snapshot_nan",
        "unknown_kind",
        "unknown_field",
        "nested_runtime_right",
        "future_version",
        "missing_project_field",
        "unknown_project_field",
        "incomplete_snapshot_metadata",
        "incomplete_ordinal",
        "incomplete_snapshot_wrapper",
    ],
)
def test_unambiguous_complete_history_rejects_malformed_evidence(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, damage: str
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    data = json.loads(journal.model_dump_json())
    if damage == "outer_duplicate":
        text = '{"phase":"captured",' + journal.model_dump_json()[1:]
    elif damage in {
        "snapshot_duplicate",
        "snapshot_nan",
        "nested_runtime_right",
        "incomplete_snapshot_metadata",
    }:
        data["source"]["payload_json"] = _damaged_snapshot_payload(
            data["source"]["payload_json"], damage
        )
        text = json.dumps(data)
    else:
        text = _damaged_journal_fields(journal.model_dump_json(), damage)
    with pytest.raises((ValueError, TypeError, ValidationError)):
        validated_copy_history([text], _TRANSACTION)


@pytest.mark.parametrize(
    "damage", ["changed_c", "old_b_identity", "changed_material", "changed_lineage"]
)
def test_fixed_c_cannot_change_musical_content_or_reuse_old_target_lineage(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, damage: str
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    data = json.loads(journal.model_dump_json())
    after = json.loads(data["after"]["payload_json"])
    if damage == "changed_c":
        after["gain_db"] = -24.0
    elif damage == "old_b_identity":
        after["source_identity"]["instance_id"] = "2" * 32
    elif damage == "changed_material":
        after["source_identity"]["material_id"] = "5" * 32
    else:
        after["source_identity"]["copied_from"] = "2" * 32
    data["after"]["payload_json"] = json.dumps(after)
    with pytest.raises(ValidationError):
        CopyArrangementJournal.model_validate_json(json.dumps(data), strict=True)


def test_invalid_earlier_ordinal_cannot_hide_behind_valid_scan_tail(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    store = MaterialMigrationJournalStore(str(tmp_path / "samples"), _TRANSACTION)
    changed = json.loads(journal.model_dump_json())
    changed["config_reference"] = str(tmp_path / "samples" / "foreign.config.json")
    store.append(json.dumps(changed))
    store.append(journal.model_dump_json())
    original = store.history()
    with pytest.raises(ValueError, match="history"):
        CopyJournalOwner.open(tmp_path / "samples", _TRANSACTION)
    assert store.history() == original


@pytest.mark.parametrize("complete", [True, False])
def test_uncertain_append_adopts_only_exact_completed_bytes_and_retains_partial_ordinal(
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
            message = "injected failure after actual ordinal write"
            raise OSError(message)

    owner = CopyJournalOwner(
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
        with pytest.raises(ValueError, match="Previous Copy append"):
            owner.append(journal.changed(error="cannot replace uncertain evidence"))
        path = tmp_path / "samples" / ".material-migrations" / _TRANSACTION / "00.json"
        assert path.read_text(encoding="utf-8") == journal.model_dump_json()[:100]


def test_repeat_retry_does_not_spend_capacity_and_full_history_retains_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    owner = CopyJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    failed = owner.append(journal.changed(error="queue remains full"))
    for _ in range(30):
        assert owner.append(failed) == failed
    assert len(owner.store.history()) == 2
    for index in range(14):
        owner.append(journal.changed(error=f"different bounded error {index}"))
    before = owner.store.history()
    with pytest.raises(ValueError, match="capacity"):
        owner.append(journal.changed(error="one too many"))
    assert owner.store.history() == before
    assert owner.journal is not None


def test_newer_receipt_must_be_recorded_before_settled_and_keep_original_seals(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = CopyJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    candidate = project_with_journal_copy(persistence.project, journal)
    receipt = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    committed = owner.append(acknowledged.committed(receipt))
    before_retry = list(owner.encoded)
    persistence.project.pad_gain_db[36] = -22.0
    persistence.mark_dirty()
    pending = owner.append(
        committed.changed(
            phase="native_ack_observed",
            image_role="before",
            intent_revision=persistence.revision,
            project_image_json=persistence.project.model_dump_json(),
            committed_revision=None,
            committed_config_sha256=None,
        )
    )
    newer = persistence.commit_migration_receipt(
        _TRANSACTION, persistence.revision, project_with_journal_copy(persistence.project, journal)
    )
    with pytest.raises(ValueError, match="history"):
        validated_copy_history(
            [*before_retry, committed.committed(newer).model_dump_json()], _TRANSACTION
        )
    with pytest.raises(ValueError, match="history"):
        validated_copy_history(
            [*owner.encoded, committed.committed(newer).changed(phase="settled").model_dump_json()],
            _TRANSACTION,
        )
    recommitted = owner.append(pending.committed(newer))
    settled = owner.append(recommitted.changed(phase="settled"))
    assert settled.source == journal.source
    assert settled.predecessor == journal.predecessor
    assert settled.after == journal.after
    assert settled.committed_revision == receipt.revision + 1


@pytest.mark.parametrize("newer", [False, True])
def test_replacement_retry_records_new_pre_c_image_only_at_strictly_newer_revision(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, newer: bool
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = CopyJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    receipt = persistence.commit_migration_receipt(
        _TRANSACTION, persistence.revision, project_with_journal_copy(persistence.project, journal)
    )
    committed = owner.append(acknowledged.committed(receipt))
    if newer:
        persistence.project.pad_gain_db[0] = -24.0
        persistence.project.volume = 0.25
        persistence.mark_dirty()
    retry = committed.changed(
        phase="native_ack_observed",
        image_role="before",
        intent_revision=persistence.revision,
        project_image_json=persistence.project.model_dump_json(),
        committed_revision=None,
        committed_config_sha256=None,
    )
    if not newer:
        with pytest.raises(ValueError, match="history"):
            owner.append(retry)
        assert owner.journal == committed
        return
    owner.append(retry)
    newer_receipt = persistence.commit_migration_receipt(
        _TRANSACTION, persistence.revision, project_with_journal_copy(persistence.project, retry)
    )
    owner.append(retry.committed(newer_receipt))
    assert newer_receipt.projected().pad_gain_db[0] == -24.0
    assert newer_receipt.projected().pad_gain_db[215] == -6.0
    assert newer_receipt.projected().volume == 0.25
    assert owner.journal is not None
    assert owner.journal.after == journal.after


@pytest.mark.parametrize("replaced", [False, True])
def test_fresh_process_recognizes_prior_receipt_during_newer_pre_c_replacement_retry(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, replaced: bool
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = CopyJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    first = persistence.commit_migration_receipt(
        _TRANSACTION, persistence.revision, project_with_journal_copy(persistence.project, journal)
    )
    committed = owner.append(acknowledged.committed(first))
    persistence.project.pad_gain_db[0] = -24.0
    persistence.project.pad_gain_db[36] = -22.0
    persistence.mark_dirty()
    latest = owner.append(
        committed.changed(
            phase="native_ack_observed",
            image_role="before",
            intent_revision=persistence.revision,
            project_image_json=persistence.project.model_dump_json(),
            committed_revision=None,
            committed_config_sha256=None,
        )
    )
    if replaced:
        persistence.commit_migration_receipt(
            _TRANSACTION,
            persistence.revision,
            project_with_journal_copy(persistence.project, latest),
        )
    child_code = """
import json, sys
from pathlib import Path
from flitzis_looper.controller.copy_arrangement_journal import (
    CopyJournalOwner, validated_copy_history)
from flitzis_looper.controller.copy_arrangement_recovery import inspect_copy_recovery
owner = CopyJournalOwner.open(Path(sys.argv[1]), sys.argv[2])
decision = inspect_copy_recovery(owner.journal, Path(sys.argv[3]).read_bytes(),
    history=validated_copy_history(owner.encoded, sys.argv[2]))
project = decision.projected()
print(json.dumps({'decision': decision.disposition,
    'source_gain': project.pad_gain_db[0], 'target_gain': project.pad_gain_db[215],
    'unrelated_gain': project.pad_gain_db[36], 'c': project.pad_content[215].instance_id,
    'revision': project.config_revision,
    'receipt_revision': None if decision.receipt is None else decision.receipt.revision}))
"""
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
        env=os.environ | {"PYTHONPATH": str(Path(__file__).resolve().parents[3])},
        capture_output=True,
        text=True,
        check=True,
        timeout=30,
    )
    actual = json.loads(child.stdout.strip().splitlines()[-1])
    assert actual == {
        "decision": "recognized_after",
        "source_gain": -24.0,
        "target_gain": -6.0,
        "unrelated_gain": -22.0,
        "c": "3" * 32,
        "revision": latest.intent_revision,
        "receipt_revision": latest.intent_revision if replaced else None,
    }


def test_successor_retains_original_seals_with_restored_save_normalization_and_newer_intent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    restored = persistence.project.model_copy(deep=True)
    restored.sample_paths = ProjectPersistence._normalize_sample_paths_for_save(
        restored.sample_paths
    )
    restored.volume = 0.25
    restored.config_revision = 7
    child = journal.changed(
        transaction_id="b" * 32,
        resume_of=_TRANSACTION,
        captured_revision=7,
        intent_revision=7,
        project_image_json=restored.model_dump_json(),
    )
    assert child.captured_project_image_json == journal.captured_project_image_json
    assert child.source == journal.source
    assert child.predecessor == journal.predecessor
    assert child.after == journal.after
    assert project_with_journal_copy(restored, child).volume == 0.25
    with pytest.raises(ValidationError, match="complete original project image"):
        journal.changed(project_image_json=restored.model_dump_json(), intent_revision=7)


@pytest.mark.parametrize("phase", ["publication_pending", "cancelled"])
def test_pending_or_cancelled_record_preserves_newer_a_without_changing_fixed_copy(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, phase: str
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = CopyJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    persistence.project.sample_paths[0] = None
    persistence.project.pad_content[0] = None
    persistence.project.volume = 0.25
    persistence.mark_dirty()
    latest = journal.changed(
        phase=phase,
        intent_revision=persistence.revision,
        project_image_json=persistence.project.model_dump_json(),
    )
    owner.append(latest)
    assert latest.source == journal.source
    assert latest.after == journal.after
    assert latest.predecessor == journal.predecessor
    assert json.loads(latest.project_image_json)["pad_content"][0] is None


def test_full_native_inventory_preserves_config_fence_before_copy_directory_creation(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    for index in range(256):
        MaterialMigrationJournalStore(str(tmp_path / "samples"), f"{index + 1:032x}")
    with pytest.raises(ValueError, match="inventory capacity"):
        CopyJournalOwner(tmp_path / "samples", journal)
    assert not (tmp_path / "samples" / ".material-migrations" / _TRANSACTION).exists()
    assert persistence._migration_owner == _TRANSACTION
    assert not persistence.flush_if_dirty()


def test_actual_native_directory_custody_preserves_replaced_unknown_journal_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, journal = _captured(tmp_path, monkeypatch)
    owner = CopyJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    directory = tmp_path / "samples" / ".material-migrations" / _TRANSACTION
    retained = directory.with_name("retained-transaction")
    if os.name == "nt":
        with pytest.raises(OSError, match=r"\[WinError (?:5|32)\]"):
            directory.rename(retained)
        assert owner.store.history() == [journal.model_dump_json()]
    del owner
    gc.collect()
    directory.rename(retained)
    directory.mkdir()
    unknown = directory / "foreign-owner.txt"
    unknown.write_text("preserve replacement owner", encoding="utf-8")
    with pytest.raises((ValueError, RuntimeError)):
        CopyJournalOwner.open(tmp_path / "samples", _TRANSACTION)
    assert (retained / "00.json").read_text(encoding="utf-8") == journal.model_dump_json()
    assert unknown.read_text(encoding="utf-8") == "preserve replacement owner"


def test_compaction_requires_actual_complete_successor_and_transfers_closed_writer(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    parent = CopyJournalOwner(tmp_path / "samples", journal)
    _acknowledged(parent, journal)
    successor_record = journal.changed(transaction_id="b" * 32, resume_of=_TRANSACTION)
    successor = CopyJournalOwner(tmp_path / "samples", successor_record)
    acknowledged = _acknowledged(successor, successor_record)
    candidate = project_with_journal_copy(persistence.project, successor_record)
    receipt = persistence.commit_migration_receipt(_TRANSACTION, persistence.revision, candidate)
    successor.append(acknowledged.committed(receipt))
    parent.compact_into(successor, persistence)

    assert persistence._migration_owner == successor_record.transaction_id
    assert not (tmp_path / "samples" / ".material-migrations" / _TRANSACTION).exists()
    reopened = CopyJournalOwner.open(tmp_path / "samples", successor_record.transaction_id)
    assert reopened.journal == successor.journal


@pytest.mark.parametrize("phase", ["config_committed", "cancelled"])
def test_phase_history_cannot_skip_claim_or_cancel_after_observed_native_ack(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, phase: str
) -> None:
    persistence, journal = _captured(tmp_path, monkeypatch)
    owner = CopyJournalOwner(tmp_path / "samples", journal)
    acknowledged = _acknowledged(owner, journal)
    if phase == "cancelled":
        invalid = acknowledged.changed(phase="cancelled")
        history = [*owner.encoded, invalid.model_dump_json()]
    else:
        receipt = persistence.commit_migration_receipt(
            _TRANSACTION,
            persistence.revision,
            project_with_journal_copy(persistence.project, journal),
        )
        history = [journal.model_dump_json(), journal.committed(receipt).model_dump_json()]
    with pytest.raises(ValueError, match="history"):
        validated_copy_history(history, _TRANSACTION)
