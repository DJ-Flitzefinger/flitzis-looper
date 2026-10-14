"""Owning startup recovery uses fresh acceptance and never replays recognized after layouts."""

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller import pair_arrangement_startup
from flitzis_looper.controller.pair_arrangement_journal import (
    PairArrangementAfterImage,
    PairArrangementJournal,
    PairJournalOwner,
)
from flitzis_looper.controller.pair_arrangement_startup import PairArrangementStartup
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper.pair_snapshot import capture_pair_snapshot, project_with_pair_snapshot
from flitzis_looper_audio import MaterialMigrationJournalStore
from tests.flitzis_looper.controller.test_pair_arrangement import _Assets
from tests.flitzis_looper.controller.test_pair_arrangement_journal import _TRANSACTION, _captured

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.pair_snapshot import PairOperation


class _Acceptance:
    def __init__(self, engine: object) -> None:
        self.engine = engine
        self.fresh_source = object()
        self.released = False


class _Audio:
    def __init__(self, project: ProjectState) -> None:
        self.project = project
        self.ready = False
        self.valid = True
        self.captures: list[tuple[int, int, _Acceptance]] = []
        self.validations: list[_Acceptance] = []
        self.releases: list[_Acceptance] = []
        self.release_pressure = 0
        self.release_error = False

    def capture_recovered_pad_pair(self, source: int, target: int) -> _Acceptance:
        if not self.ready:
            message = "current restored source/timing guards not ready"
            raise RuntimeError(message)
        acceptance = _Acceptance(self)
        self.captures.append((source, target, acceptance))
        return acceptance

    def validate_recovered_pad_pair(self, acceptance: _Acceptance) -> bool:
        assert acceptance.engine is self
        assert acceptance is self.captures[-1][2]
        self.validations.append(acceptance)
        return self.valid and not acceptance.released

    def release_recovered_pad_pair(self, acceptance: _Acceptance) -> bool:
        assert acceptance.engine is self
        assert acceptance is self.captures[-1][2]
        self.releases.append(acceptance)
        if self.release_pressure:
            self.release_pressure -= 1
            if self.release_error:
                message = "fresh acceptance release temporarily unavailable"
                raise RuntimeError(message)
            return False
        acceptance.released = True
        return True

    def _capture_pad_pair(self, *args: object) -> None:
        pytest.fail("recognized after must not capture another Move/Swap")

    def _publish_pad_pair(self, *args: object) -> None:
        pytest.fail("recognized after must not publish another Move/Swap")


class _StartupAssets(_Assets):
    def __init__(self, project: ProjectState) -> None:
        super().__init__()
        self.project = project
        self.synced: list[list[str | None]] = []

    def sync_assignments(self) -> None:
        self.synced.append(list(self.project.sample_paths))


def _prior(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    operation: PairOperation = "swap",
    after: bool = False,
    recorded: bool = True,
    unsaved: bool = False,
) -> tuple[Path, PairArrangementJournal]:
    persistence, journal = _captured(tmp_path, monkeypatch, operation)
    owner = PairJournalOwner(tmp_path / "samples", journal)
    owner.append(journal)
    pending = owner.append(journal.changed(phase="publication_pending"))
    acknowledged = owner.append(pending.changed(phase="native_ack_observed"))
    if unsaved:
        persistence.project.volume = 0.25
        persistence.project.pad_gain_db[36] = -24.0
        persistence.mark_dirty()
        acknowledged = owner.append(
            acknowledged.changed(
                intent_revision=persistence.revision,
                project_image_json=persistence.project.model_dump_json(),
            )
        )
    if after:
        candidate = project_with_pair_snapshot(persistence.project, journal.before)
        receipt = persistence.commit_migration_receipt(
            _TRANSACTION, persistence.revision, candidate
        )
        if recorded:
            acknowledged = owner.append(acknowledged.committed(receipt))
    persistence.release_migration(_TRANSACTION)
    return persistence.config_path, acknowledged


def _app(config: Path) -> SimpleNamespace:
    persistence = ProjectPersistence.from_config_path(config)
    persistence.config_path = config
    project = persistence.project
    return SimpleNamespace(
        persistence=persistence,
        project=project,
        session=SessionState(),
        _audio=_Audio(project),
        _assets=_StartupAssets(project),
    )


def _startup(app: SimpleNamespace) -> PairArrangementStartup:
    return PairArrangementStartup(cast("AppController", app))


@pytest.mark.parametrize("operation", ["move", "swap"])
@pytest.mark.parametrize("recorded", [False, True])
def test_actual_after_startup_waits_for_fresh_restore_then_settles_without_another_layout(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    operation: PairOperation,
    *,
    recorded: bool,
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, operation=operation, after=True, recorded=recorded)
    app = _app(config)
    before = config.read_bytes()
    startup = _startup(app)
    assert startup.active
    assert startup.fenced_sample_ids == frozenset((0, 215))
    assert startup.observer_fenced_sample_ids == frozenset()
    assert app.persistence._migration_owner == _TRANSACTION
    app.session.loading_sample_ids.add(215)
    assert startup.poll() is None
    assert app._audio.captures == []
    app.session.loading_sample_ids.clear()
    assert startup.poll() is None
    assert startup.active
    assert app._audio.captures == []
    assert config.read_bytes() == before
    app._audio.ready = True
    assert startup.poll() is None

    assert not startup.active
    assert startup.status == "completed"
    assert len(app._audio.captures) == 1
    acceptance = app._audio.captures[0][2]
    assert app._audio.releases == [acceptance]
    assert acceptance.released
    reopened = PairJournalOwner.open(tmp_path / "samples", _TRANSACTION)
    assert reopened.journal.phase == "settled"
    assert (
        reopened.journal.committed_config_sha256 == hashlib.sha256(config.read_bytes()).hexdigest()
    )
    assert reopened.journal.committed_revision == app.persistence.revision
    if operation == "move":
        assert app.project.pad_content[0] is None
    else:
        assert app.project.pad_content[0].instance_id == "2" * 32
    assert app.project.pad_content[215].instance_id == "1" * 32
    assert app.persistence._migration_owner is None
    assert app._assets.retirement.closed == 1


@pytest.mark.parametrize("release_error", [False, True])
def test_release_pressure_keeps_same_fresh_acceptance_and_latest_unrelated_config(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    release_error: bool,
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, after=True)
    app = _app(config)
    app._audio.ready = True
    app._audio.release_pressure = 1
    app._audio.release_error = release_error
    startup = _startup(app)
    startup.poll()
    assert startup.active
    assert app._assets.retirement.closed == 0
    acceptance = app._audio.captures[0][2]
    assert startup.observer_fenced_sample_ids == startup.fenced_sample_ids
    app.project.pad_gain_db[36] = -23.0
    app.project.volume = 0.2
    app.persistence.mark_dirty()
    startup.poll()

    assert not startup.active
    assert len(app._audio.captures) == 1
    assert app._audio.releases == [acceptance, acceptance]
    current = ProjectState.model_validate_json(config.read_bytes())
    assert current.pad_gain_db[36] == -23.0
    assert current.volume == 0.2
    assert current.config_revision == app.persistence.revision == 1
    assert current.pad_content[0].instance_id == "2" * 32


def test_stale_actual_acceptance_keeps_original_owner_fence_and_config_untouched(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, after=True)
    app = _app(config)
    app._audio.ready = True
    app._audio.valid = False
    startup = _startup(app)
    before = config.read_bytes()
    startup.poll()
    acceptance = app._audio.captures[0][2]
    startup.poll()

    assert startup.active
    assert startup.status == "recovery_pending"
    assert len(app._audio.captures) == 1
    assert app._audio.releases == []
    assert startup._after.acceptance is acceptance
    assert not acceptance.released
    assert app.persistence._migration_owner == _TRANSACTION
    assert app._assets.retirement.closed == 0
    assert config.read_bytes() == before


def test_after_config_replace_signal_retains_fresh_acceptance_and_retries_same_revision(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, after=True, recorded=False)
    app = _app(config)
    app._audio.ready = True
    startup = _startup(app)
    write = app.persistence._atomic_write_text
    failed = False

    def replace_then_signal(text: str) -> None:
        nonlocal failed
        write(text)
        if not failed:
            failed = True
            message = "lost startup Config commit return"
            raise OSError(message)

    monkeypatch.setattr(app.persistence, "_atomic_write_text", replace_then_signal)
    startup.poll()
    assert startup.active
    assert startup._after.receipt is None
    acceptance = startup._after.acceptance
    assert app._audio.releases == []
    assert app._assets.retirement.closed == 0
    startup.poll()
    assert not startup.active
    assert len(app._audio.captures) == 1
    assert app._audio.releases == [acceptance]
    assert PairJournalOwner.open(tmp_path / "samples", _TRANSACTION).journal.committed_revision == 0


def test_original_unsaved_intent_restores_before_loader_then_hands_one_fresh_successor(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    config, historical = _prior(tmp_path, monkeypatch, unsaved=True)
    app = _app(config)
    assert app.project.volume != 0.25
    original_config = config.read_bytes()
    startup = _startup(app)
    assert app.project.volume == 0.25
    assert app.project.pad_gain_db[36] == -24.0
    assert app.persistence.revision == historical.intent_revision == 1
    assert config.read_bytes() == original_config
    assert app._assets.synced
    assert startup.observer_fenced_sample_ids == frozenset()
    app.session.loading_sample_ids.add(0)
    assert startup.poll() is None
    app.session.loading_sample_ids.clear()
    handoff = startup.poll()

    assert handoff is not None
    assert handoff.writer_transferred
    assert handoff.delivered
    assert handoff.initial.phase == "captured"
    assert handoff.initial.resume_of == _TRANSACTION
    assert handoff.owner != _TRANSACTION
    assert handoff.snapshot.projected().source.source_identity.instance_id == "1" * 32
    assert app.persistence._migration_owner == handoff.owner
    assert app._audio.captures == []
    assert startup.poll() is None
    assert (tmp_path / "samples" / ".material-migrations" / _TRANSACTION).exists()
    pending = handoff.journal.append(handoff.initial.changed(phase="publication_pending"))
    acknowledged = handoff.journal.append(pending.changed(phase="native_ack_observed"))
    candidate = project_with_pair_snapshot(app.project, handoff.snapshot)
    receipt = app.persistence.commit_migration_receipt(
        handoff.owner, app.persistence.revision, candidate
    )
    committed = handoff.journal.append(acknowledged.committed(receipt))
    handoff.journal.append(committed.changed(phase="settled"))
    startup.complete_original(handoff)

    assert not startup.active
    assert handoff.completed
    assert not (tmp_path / "samples" / ".material-migrations" / _TRANSACTION).exists()
    assert (tmp_path / "samples" / ".material-migrations" / handoff.owner).exists()
    assert app.persistence._migration_owner == handoff.owner, (
        "coordinator alone releases transferred writer"
    )
    assert app._assets.retirement.closed == 0, "coordinator alone releases transferred retirement"


def test_original_successor_creation_capacity_keeps_predecessor_and_restored_intent_owned(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, unsaved=True)
    app = _app(config)
    native_owner = PairJournalOwner

    class AtCapacity:
        @staticmethod
        def open(samples: Path, transaction: str) -> PairJournalOwner:
            return native_owner.open(samples, transaction)

        def __new__(cls, *args: object, **kwargs: object) -> object:
            message = "journal inventory capacity exhausted"
            raise ValueError(message)

    monkeypatch.setattr(pair_arrangement_startup, "PairJournalOwner", AtCapacity)
    startup = _startup(app)
    before = config.read_bytes()
    assert startup.poll() is None
    assert startup.active
    assert startup.error == "journal inventory capacity exhausted"
    assert app.project.volume == 0.25
    assert app.persistence._migration_owner == _TRANSACTION
    assert app._assets.retirement.closed == 0
    assert config.read_bytes() == before
    assert (
        PairJournalOwner.open(tmp_path / "samples", _TRANSACTION).journal.phase
        == "native_ack_observed"
    )


@pytest.mark.parametrize(
    "damage",
    [
        "newer_config",
        "unknown_history",
        "partial_history",
        "writer_collision",
        "unsaved_current",
    ],
)
def test_newer_unknown_or_foreign_writer_state_is_preserved_without_saved_authority(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    damage: str,
) -> None:
    config, _ = _prior(tmp_path, monkeypatch)
    if damage == "newer_config":
        data = json.loads(config.read_text(encoding="utf-8"))
        data["volume"], data["config_revision"] = 0.2, 10
        config.write_text(json.dumps(data), encoding="utf-8")
    elif damage == "unknown_history":
        store = MaterialMigrationJournalStore.open(str(tmp_path / "samples"), _TRANSACTION)
        store.append('{"kind":"future_pair","schema_version":19}')
    elif damage == "partial_history":
        leaf = tmp_path / "samples" / ".material-migrations" / _TRANSACTION / "03.json"
        leaf.write_text("{", encoding="utf-8")
    app = _app(config)
    if damage == "writer_collision":
        app.persistence.capture_migration("b" * 32)
    elif damage == "unsaved_current":
        app.project.volume = 0.15
        app.persistence.mark_dirty()
    before_config = config.read_bytes()
    before_project = app.project.model_dump_json()
    startup = _startup(app)
    startup.poll()

    assert startup.active
    assert startup.error
    assert config.read_bytes() == before_config
    assert app.project.model_dump_json() == before_project
    assert app._audio.captures == []
    assert app._audio.releases == []
    assert app._assets.retirement.closed == 0
    expected_owner = "b" * 32 if damage == "writer_collision" else app.persistence._migration_owner
    assert app.persistence._migration_owner == expected_owner
    assert not app.persistence.flush_if_dirty()


def test_foreign_equal_byte_config_does_not_acquire_selected_pair_owner(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    config, _ = _prior(tmp_path, monkeypatch)
    foreign = config.with_name("foreign.config.json")
    before = config.read_bytes()
    foreign.write_bytes(before)
    app = _app(foreign)
    startup = _startup(app)
    assert not startup.active
    assert app.persistence._migration_owner is None
    assert startup.poll() is None
    assert app._audio.captures == []
    assert config.read_bytes() == foreign.read_bytes() == before


def test_durable_fresh_preclaim_cancellation_preserves_original_config_and_recovered_intent(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, unsaved=True)
    app = _app(config)
    startup = _startup(app)
    original = config.read_bytes()
    handoff = startup.poll()
    assert handoff is not None
    # The live coordinator alone may create this phase after actual fresh preclaim cancel.
    handoff.journal.append(handoff.initial.changed(phase="cancelled"))
    startup.complete_original(handoff)
    assert not startup.active
    assert handoff.completed
    assert config.read_bytes() == original
    assert app.project.volume == 0.25
    assert app.persistence._migration_owner == handoff.owner
    assert app._assets.retirement.closed == 0
    assert not (tmp_path / "samples" / ".material-migrations" / _TRANSACTION).exists()
    assert PairJournalOwner.open(tmp_path / "samples", handoff.owner).journal.phase == "cancelled"


def test_compaction_failure_keeps_exact_parent_successor_and_transferred_writer_owned(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    config, _ = _prior(tmp_path, monkeypatch)
    app = _app(config)
    startup = _startup(app)
    handoff = startup.poll()
    assert handoff is not None
    pending = handoff.journal.append(handoff.initial.changed(phase="publication_pending"))
    acknowledged = handoff.journal.append(pending.changed(phase="native_ack_observed"))
    candidate = project_with_pair_snapshot(app.project, handoff.snapshot)
    receipt = app.persistence.commit_migration_receipt(
        handoff.owner, app.persistence.revision, candidate
    )
    committed = handoff.journal.append(acknowledged.committed(receipt))
    handoff.journal.append(committed.changed(phase="settled"))
    handoff.predecessor.store.append('{"unknown":"later evidence"}')
    parent_before = handoff.predecessor.store.history()
    child_before = handoff.journal.store.history()
    config_before = config.read_bytes()

    with pytest.raises(ValueError, match="history changed"):
        startup.complete_original(handoff)

    assert startup.active
    assert not handoff.completed
    assert handoff.predecessor.store.history() == parent_before
    assert handoff.journal.store.history() == child_before
    assert config.read_bytes() == config_before
    assert app.persistence._migration_owner == handoff.owner
    assert app._assets.retirement.closed == 0


def _durable_successor(
    config: Path,
    parent: PairArrangementJournal,
    phase: str,
    *,
    child_id: str = "b" * 32,
    unsaved: bool = False,
) -> PairArrangementJournal:
    app = _app(config)
    if unsaved:
        app.project.volume = 0.35
        app.project.pad_gain_db[36] = -22.0
        app.persistence.mark_dirty()
    revision, project, digest = app.persistence.capture_migration(child_id)
    original = parent.before.projected()
    snapshot = capture_pair_snapshot(
        project, original.source_id, original.target_id, operation=original.operation
    )
    record = PairArrangementJournal(
        transaction_id=child_id,
        resume_of=parent.transaction_id,
        config_reference=app.persistence.config_reference,
        captured_revision=revision,
        intent_revision=revision,
        captured_config_sha256=digest,
        before=snapshot,
        after=PairArrangementAfterImage.from_before(snapshot),
        project_image_json=project.model_dump_json(),
        image_role="before",
        phase="captured",
    )
    owner = PairJournalOwner(config.parent, record)
    owner.append(record)
    if phase == "cancelled":
        record = owner.append(record.changed(phase="cancelled"))
    elif phase != "captured":
        record = owner.append(record.changed(phase="publication_pending"))
        if phase != "publication_pending":
            record = owner.append(record.changed(phase="native_ack_observed"))
            if phase in {"config_committed", "settled"}:
                candidate = project_with_pair_snapshot(app.project, snapshot)
                receipt = app.persistence.commit_migration_receipt(child_id, revision, candidate)
                record = owner.append(record.committed(receipt))
                if phase == "settled":
                    record = owner.append(record.changed(phase="settled"))
    app.persistence.release_migration(child_id)
    return record


@pytest.mark.parametrize(
    "phase",
    [
        "captured",
        "publication_pending",
        "native_ack_observed",
        "config_committed",
        "settled",
        "cancelled",
    ],
)
def test_fresh_process_reopens_interrupted_handoff_and_selects_latest_complete_lineage_tip(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    phase: str,
) -> None:
    config, parent = _prior(tmp_path, monkeypatch)
    child = _durable_successor(config, parent, phase)
    child_history = PairJournalOwner.open(config.parent, child.transaction_id).encoded
    original_config = config.read_bytes()
    script = """
import json, sys
from pathlib import Path
from tests.flitzis_looper.controller.test_pair_arrangement_startup import _app, _startup
app = _app(Path(sys.argv[1]))
startup = _startup(app)
app._audio.ready = True
handoff = startup.poll()
print(json.dumps({
    'active': startup.active, 'status': startup.status, 'error': startup.error,
    'captures': len(app._audio.captures),
    'handoff_parent': None if handoff is None else handoff.initial.resume_of,
    'ancestors': [] if handoff is None else [
        owner.initial.transaction_id for owner in handoff.ancestors
    ],
    'slots': [
        app.project.pad_content[i].instance_id if app.project.pad_content[i] else None
        for i in [0, 215]
    ],
}))
"""
    child_process = subprocess.run(
        [sys.executable, "-c", script, str(config)],
        cwd=tmp_path,
        env=os.environ | {"PYTHONPATH": str(Path(__file__).resolve().parents[3])},
        capture_output=True,
        text=True,
        check=True,
        timeout=30,
    )
    actual = json.loads(child_process.stdout.strip().splitlines()[-1])
    assert actual["error"] is None
    if phase in {"captured", "publication_pending", "native_ack_observed"}:
        assert actual["handoff_parent"] == child.transaction_id
        assert actual["ancestors"] == [_TRANSACTION, child.transaction_id]
        assert actual["captures"] == 0
        assert actual["slots"] == ["1" * 32, "2" * 32]
        assert PairJournalOwner.open(config.parent, child.transaction_id).encoded == child_history
    else:
        assert not actual["active"]
        assert actual["status"] == "completed"
        assert actual["handoff_parent"] is None
        assert actual["captures"] == (0 if phase == "cancelled" else 1)
        expected_slots = ["1" * 32, "2" * 32] if phase == "cancelled" else ["2" * 32, "1" * 32]
        assert actual["slots"] == expected_slots
        assert not (config.parent / ".material-migrations" / _TRANSACTION).exists()
        if phase in {"settled", "cancelled"}:
            assert (
                PairJournalOwner.open(config.parent, child.transaction_id).encoded == child_history
            )
    assert config.read_bytes() == original_config


def test_cancelled_terminal_tip_restores_latest_unsaved_before_intent_without_pair_replay(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    config, parent = _prior(tmp_path, monkeypatch)
    child = _durable_successor(config, parent, "cancelled", unsaved=True)
    app = _app(config)
    original = config.read_bytes()
    startup = _startup(app)
    assert app.project.volume == 0.35
    assert app.project.pad_gain_db[36] == -22.0
    assert startup.poll() is None
    assert not startup.active
    assert app._audio.captures == []
    assert app._audio.releases == []
    assert config.read_bytes() == original
    assert PairJournalOwner.open(config.parent, child.transaction_id).journal.phase == "cancelled"


def test_original_handoff_carries_and_compacts_complete_ancestor_chain_after_real_receipt(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    config, parent = _prior(tmp_path, monkeypatch)
    child = _durable_successor(config, parent, "native_ack_observed")
    app = _app(config)
    startup = _startup(app)
    handoff = startup.poll()
    assert handoff is not None
    assert handoff.initial.resume_of == child.transaction_id
    assert [owner.initial.transaction_id for owner in handoff.ancestors] == [
        _TRANSACTION,
        child.transaction_id,
    ]
    pending = handoff.journal.append(handoff.initial.changed(phase="publication_pending"))
    acknowledged = handoff.journal.append(pending.changed(phase="native_ack_observed"))
    candidate = project_with_pair_snapshot(app.project, handoff.snapshot)
    receipt = app.persistence.commit_migration_receipt(
        handoff.owner, app.persistence.revision, candidate
    )
    committed = handoff.journal.append(acknowledged.committed(receipt))
    handoff.journal.append(committed.changed(phase="settled"))
    startup.complete_original(handoff)
    assert not startup.active
    assert not (config.parent / ".material-migrations" / _TRANSACTION).exists()
    assert not (config.parent / ".material-migrations" / child.transaction_id).exists()
    assert PairJournalOwner.open(config.parent, handoff.owner).journal.phase == "settled"
    assert app.persistence._migration_owner == handoff.owner


@pytest.mark.parametrize("changed", ["before_startup", "during_restore"])
def test_terminal_settled_tip_preserves_newer_intent_without_rewriting_terminal_history(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    changed: str,
) -> None:
    config, parent = _prior(tmp_path, monkeypatch)
    child = _durable_successor(config, parent, "settled")
    history = PairJournalOwner.open(config.parent, child.transaction_id).encoded
    app = _app(config)
    if changed == "before_startup":
        app.project.volume = 0.25
        app.persistence.mark_dirty()
    startup = _startup(app)
    if changed == "during_restore":
        app.project.volume = 0.25
        app.persistence.mark_dirty()
    app._audio.ready = True
    original = config.read_bytes()
    startup.poll()
    assert startup.active
    assert startup.error
    assert app.project.volume == 0.25
    assert config.read_bytes() == original
    assert PairJournalOwner.open(config.parent, child.transaction_id).encoded == history
    assert (config.parent / ".material-migrations" / _TRANSACTION).exists()
    assert app.persistence._migration_owner is not None
    assert app._audio.captures == []


@pytest.mark.parametrize("damage", ["two_roots", "fork", "cycle", "changed_seal", "changed_digest"])
def test_unknown_or_ambiguous_lineage_is_fenced_without_touching_config_or_history(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    damage: str,
) -> None:
    config, parent = _prior(tmp_path, monkeypatch)
    child = _durable_successor(config, parent, "captured")
    if damage in {"two_roots", "fork"}:
        additional = _durable_successor(config, parent, "captured", child_id="c" * 32)
        if damage == "two_roots":
            record = additional.changed(resume_of=None)
            leaf = config.parent / ".material-migrations" / additional.transaction_id / "00.json"
            leaf.write_text(record.model_dump_json(), encoding="utf-8")
    elif damage == "cycle":
        directory = config.parent / ".material-migrations" / _TRANSACTION
        for leaf in sorted(directory.glob("*.json")):
            raw = json.loads(leaf.read_text(encoding="utf-8"))
            raw["resume_of"] = child.transaction_id
            leaf.write_text(json.dumps(raw), encoding="utf-8")
    else:
        if damage == "changed_digest":
            record = child.changed(captured_config_sha256="0" * 64)
        else:
            project = ProjectState.model_validate_json(child.project_image_json)
            project.pad_gain_db[0] = -7.0
            before = capture_pair_snapshot(project, 0, 215, operation="swap")
            record = child.changed(
                before=before,
                after=PairArrangementAfterImage.from_before(before),
                project_image_json=project.model_dump_json(),
            )
        leaf = config.parent / ".material-migrations" / child.transaction_id / "00.json"
        leaf.write_text(record.model_dump_json(), encoding="utf-8")
    directory = config.parent / ".material-migrations"
    before = {
        str(leaf.relative_to(directory)): leaf.read_bytes() for leaf in directory.glob("*/*.json")
    }
    app = _app(config)
    original_config = config.read_bytes()
    startup = _startup(app)
    startup.poll()
    assert startup.active
    assert startup.status == "unresolved"
    assert app.persistence._migration_owner is not None
    assert config.read_bytes() == original_config
    assert app._audio.captures == []
    assert {
        str(leaf.relative_to(directory)): leaf.read_bytes() for leaf in directory.glob("*/*.json")
    } == before
