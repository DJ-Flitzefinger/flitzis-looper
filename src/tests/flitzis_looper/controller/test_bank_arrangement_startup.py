"""Real Bank histories/Config and reader ownership around scripted fresh probes.

The facade proves Controller ordering and retry ownership, without claiming a
Native callback, device, DSP or Human listening result.
"""

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.bank_snapshot import capture_bank_snapshot
from flitzis_looper.controller.bank_arrangement_journal import (
    BankArrangementJournal,
    BankJournalOwner,
    bank_before_project,
    project_with_journal_bank,
    validated_bank_history,
)
from flitzis_looper.controller.bank_arrangement_startup import BankArrangementStartup
from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from tests.flitzis_looper.controller.test_bank_arrangement_controller import _harness
from tests.flitzis_looper.controller.test_bank_arrangement_journal import (
    _TRANSACTION,
    _acknowledged,
    _captured,
)
from tests.flitzis_looper.controller.test_copy_arrangement_startup import (
    _Assets as _CopyAssets,
)
from tests.flitzis_looper.controller.test_copy_arrangement_startup import (
    _Retirement,
)

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.bank_arrangement_startup import OriginalBankRecoveryHandoff
    from flitzis_looper.models import StemCacheEntry
    from flitzis_looper_audio import BankParticipants, RecoveredBankAcceptance


@dataclass(eq=False)
class _Participants:
    source_bank: int | None
    target_bank: int


class _Acceptance:
    def __init__(self, participants: BankParticipants, intents: list[str | None]) -> None:
        self.participants = participants
        self.intents = intents.copy()
        self.status = "pending"
        self.owner = 1001
        self.nonce = 1

    def metadata_json(self) -> str:
        return json.dumps({
            "owner": self.owner,
            "target_bank": 1,
            "status": self.status,
            "nonce": self.nonce,
            "accepted_nonce": self.nonce if self.status == "accepted" else 0,
            "accepted_output_frame": 8192 if self.status == "accepted" else None,
            "released": self.status == "released",
        })


class _Audio:
    """Script probe delivery separately from capture; never synthesize mutation ACKs."""

    def __init__(self) -> None:
        self.participants: list[_Participants] = []
        self.captures: list[_Acceptance] = []
        self.validations: list[_Acceptance] = []
        self.retries: list[_Acceptance] = []
        self.releases: list[_Acceptance] = []
        self.bindings_ready = True
        self.valid = True
        self.release_pressure = 0

    def capture_bank_participants(
        self, source_bank: int | None, target_bank: int
    ) -> BankParticipants:
        if not self.bindings_ready:
            message = "fresh restored Native bank bindings are pending"
            raise RuntimeError(message)
        participants = _Participants(source_bank, target_bank)
        self.participants.append(participants)
        return cast("BankParticipants", participants)

    def capture_recovered_bank(
        self, participants: BankParticipants, intents: list[str | None]
    ) -> RecoveredBankAcceptance:
        captured = cast("_Participants", participants)
        assert captured.source_bank is None
        assert captured.target_bank == 1
        assert captured in self.participants
        assert len(intents) == 36
        acceptance = _Acceptance(participants, intents)
        self.captures.append(acceptance)
        return cast("RecoveredBankAcceptance", acceptance)

    def validate_recovered_bank(self, acceptance: RecoveredBankAcceptance) -> bool:
        captured = cast("_Acceptance", acceptance)
        assert captured in self.captures
        self.validations.append(captured)
        return self.valid and captured.status == "accepted"

    def retry_recovered_bank(self, acceptance: RecoveredBankAcceptance) -> bool:
        captured = cast("_Acceptance", acceptance)
        assert captured in self.captures
        assert captured.status == "waiting_restore"
        self.retries.append(captured)
        captured.nonce += 1
        captured.status = "pending"
        return True

    def release_recovered_bank(self, acceptance: RecoveredBankAcceptance) -> bool:
        captured = cast("_Acceptance", acceptance)
        assert captured in self.captures
        self.releases.append(captured)
        if self.release_pressure:
            self.release_pressure -= 1
            return False
        assert captured.status == "accepted"
        captured.status = "released"
        return True


class _Stems:
    def __init__(self) -> None:
        self.ready = True
        self.calls: list[int] = []

    def recovered_copy_selection_ready(self, sample_id: int, _entry: StemCacheEntry | None) -> bool:
        self.calls.append(sample_id)
        return self.ready


class _Assets(_CopyAssets):
    def reserve(self, capacity: int) -> _Retirement:
        assert capacity == 11 * 36
        return self.retirement


@dataclass(slots=True)
class _App:
    persistence: ProjectPersistence
    project: ProjectState
    session: SessionState
    _audio: _Audio
    _assets: _Assets
    stems: _Stems


def _app(config: Path) -> _App:
    persistence = ProjectPersistence.from_config_path(config)
    persistence.config_path = config
    return _App(
        persistence,
        persistence.project,
        SessionState(),
        _Audio(),
        _Assets(persistence.project),
        _Stems(),
    )


def _startup(app: _App) -> BankArrangementStartup:
    return BankArrangementStartup(cast("AppController", app))


def _prior(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    after: bool = True,
    recorded: bool = True,
    clear: bool = False,
    source_change: bool = False,
) -> tuple[Path, BankArrangementJournal]:
    persistence, journal = _captured(tmp_path, monkeypatch, "clear" if clear else "mixed")
    for path in persistence.project.sample_paths:
        if path is not None:
            Path(path).write_bytes(b"bounded original Bank source fixture")
    owner = BankJournalOwner(tmp_path / "samples", journal)
    current = _acknowledged(owner, journal)
    if source_change:
        persistence.project.pad_gain_db[0] = -24.0
        persistence.project.volume = 0.25
        persistence.mark_dirty()
        current = owner.append(
            current.changed(
                intent_revision=persistence.revision,
                project_image_json=bank_before_project(
                    persistence.project, journal
                ).model_dump_json(),
            )
        )
    if after:
        candidate = project_with_journal_bank(persistence.project, current)
        receipt = persistence.commit_migration_receipt(
            _TRANSACTION, persistence.revision, candidate
        )
        if recorded:
            current = owner.append(current.committed(receipt))
    persistence.release_migration(_TRANSACTION)
    return persistence.config_path, current


def _child(
    config: Path,
    parent: BankArrangementJournal,
    *,
    child_id: str = "b" * 32,
    terminal: bool = False,
) -> BankArrangementJournal:
    initial = parent.changed(
        transaction_id=child_id,
        resume_of=parent.transaction_id,
        phase="captured",
        captured_revision=parent.intent_revision,
        intent_revision=parent.intent_revision,
        committed_revision=None,
        committed_config_sha256=None,
        image_role="before",
    )
    owner = BankJournalOwner(config.parent, initial)
    tail = owner.append(initial)
    if terminal:
        tail = owner.append(tail.changed(phase="publication_pending"))
        tail = owner.append(tail.changed(phase="native_ack_observed"))
        persistence = ProjectPersistence.from_config_path(config)
        persistence.config_path = config
        persistence.capture_migration(child_id)
        candidate = project_with_journal_bank(persistence.project, tail)
        receipt = persistence.commit_migration_receipt(child_id, persistence.revision, candidate)
        tail = owner.append(tail.committed(receipt))
        tail = owner.append(tail.changed(phase="settled"))
        persistence.release_migration(child_id)
    return tail


def _settle_handoff(app: _App, handoff: OriginalBankRecoveryHandoff) -> None:
    owner = handoff.journal
    record = owner.append(handoff.initial.changed(phase="publication_pending"))
    record = owner.append(record.changed(phase="native_ack_observed"))
    candidate = project_with_journal_bank(app.project, record)
    app.persistence.recover_migration_intent(handoff.initial.transaction_id, candidate)
    receipt = app.persistence.commit_migration_receipt(
        handoff.initial.transaction_id, app.persistence.revision, candidate
    )
    owner.append(record.committed(receipt))
    owner.append(record.committed(receipt).changed(phase="settled"))


@pytest.mark.parametrize("clear", [False, True])
def test_after_requires_fresh_target_only_all36_probe_before_receipt_or_reader_release(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, clear: bool
) -> None:
    config, prior = _prior(tmp_path, monkeypatch, recorded=False, clear=clear)
    app = _app(config)
    original = config.read_bytes()
    startup = _startup(app)
    assert startup.fenced_sample_ids == frozenset(range(36, 72))
    assert startup.observer_fenced_sample_ids == frozenset()
    startup.poll()
    assert startup.active
    assert startup.status == "restore_probe"
    assert config.read_bytes() == original
    assert app.persistence._migration_owner == _TRANSACTION
    assert all(not reader.released for reader in app._assets.readers)
    acceptance = app._audio.captures[0]
    assert [(item.source_bank, item.target_bank) for item in app._audio.participants] == [(None, 1)]
    assert len(acceptance.intents) == 36
    if clear:
        assert acceptance.intents == [None] * 36
    else:
        first = json.loads(cast("str", acceptance.intents[0]))
        assert first["snapshot"] == prior.bank.projected().slots[0].after.model_dump(mode="json")
        assert first["resolved_key_lock"] is True
        assert acceptance.intents[1] is None
    assert startup.observer_fenced_sample_ids == frozenset(range(36, 72))
    startup.poll()
    assert len(app._audio.captures) == 1
    assert app._audio.releases == []
    acceptance.status = "accepted"
    startup.poll()
    assert not startup.active
    assert app._audio.releases == [acceptance]
    assert app.persistence._migration_owner is None
    assert all(reader.released for reader in app._assets.readers)
    settled = BankJournalOwner.open(config.parent, _TRANSACTION).journal
    assert settled is not None
    assert settled.phase == "settled"


def test_waiting_restore_unfences_observers_then_retries_same_owner_without_recapture(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, _ = _prior(tmp_path, monkeypatch)
    app = _app(config)
    startup = _startup(app)
    startup.poll()
    acceptance = app._audio.captures[0]
    acceptance.status = "waiting_restore"
    app.stems.ready = False
    startup.poll()
    assert startup.fenced_sample_ids == frozenset(range(36, 72))
    assert startup.observer_fenced_sample_ids == frozenset()
    assert app._audio.retries == []
    app.stems.ready = True
    startup.poll()
    assert app._audio.retries == [acceptance]
    assert app._audio.captures == [acceptance]
    assert acceptance.nonce == 2
    assert startup.active
    assert app._audio.releases == []
    acceptance.status = "accepted"
    startup.poll()
    assert not startup.active
    assert app._audio.captures == [acceptance]


def test_real_selection_readiness_precedes_original_target_only_native_capture(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, _ = _prior(tmp_path, monkeypatch)
    app = _app(config)
    startup = _startup(app)
    app.stems.ready = False
    startup.poll()
    assert app._audio.captures == []
    assert app._audio.participants == []
    assert set(app.stems.calls) == set(range(36, 72))
    assert startup.observer_fenced_sample_ids == frozenset()
    app.stems.ready = True
    startup.poll()
    assert len(app._audio.captures) == 1


def test_original_handoff_keeps_fixed_bank_and_transfers_existing_writer_after_durable_child(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, parent = _prior(tmp_path, monkeypatch, after=False)
    app = _app(config)
    original = config.read_bytes()
    startup = _startup(app)
    assert startup.fenced_sample_ids == frozenset(range(72))
    app.session.loading_sample_ids.add(0)
    assert startup.poll() is None
    app.session.loading_sample_ids.clear()
    app._audio.bindings_ready = False
    assert startup.poll() is None
    assert app.persistence._migration_owner == _TRANSACTION
    app._audio.bindings_ready = True
    handoff = startup.poll()
    assert handoff is not None
    assert handoff.initial.resume_of == parent.transaction_id
    assert handoff.initial.bank == parent.bank
    assert handoff.initial.captured_config_sha256 == parent.captured_config_sha256
    assert handoff.initial.captured_project_image_json == parent.captured_project_image_json
    assert handoff.transferred
    assert handoff.delivered
    assert handoff.participants is cast("BankParticipants", app._audio.participants[-1])
    assert handoff.custody is startup._custody
    assert handoff.retirement is startup._retirement
    assert app.persistence._migration_owner == handoff.initial.transaction_id
    assert BankJournalOwner.open(config.parent, handoff.initial.transaction_id).journal == (
        handoff.initial
    )
    assert config.read_bytes() == original
    assert startup.observer_fenced_sample_ids == frozenset(range(36, 72))
    assert startup.poll() is None
    _settle_handoff(app, handoff)
    startup.complete_original(handoff)
    assert handoff.completed
    assert not startup.active
    assert not (config.parent / ".material-migrations" / _TRANSACTION).exists()
    assert app.persistence._migration_owner == handoff.initial.transaction_id
    assert app._assets.retirement.closed == 0
    assert all(not reader.released for reader in app._assets.readers)


def test_failed_writer_transfer_reuses_durable_child_and_original_participants(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, after=False)
    app = _app(config)
    startup = _startup(app)
    transfer = app.persistence.transfer_migration
    failed = False

    def transfer_once(previous: str, owner: str) -> tuple[int, ProjectState, str | None]:
        nonlocal failed
        durable = BankJournalOwner.open(config.parent, owner)
        assert durable.journal is not None
        assert durable.journal.phase == "captured"
        if not failed:
            failed = True
            message = "writer transfer temporarily unavailable"
            raise OSError(message)
        return transfer(previous, owner)

    monkeypatch.setattr(app.persistence, "transfer_migration", transfer_once)
    assert startup.poll() is None
    handoff = startup._handoff
    assert handoff is not None
    assert not handoff.transferred
    assert not handoff.delivered
    assert app.persistence._migration_owner == _TRANSACTION
    assert len(app._audio.participants) == 1
    assert startup.poll() is handoff
    assert handoff.transferred
    assert handoff.delivered
    assert len(app._audio.participants) == 1


@pytest.mark.parametrize("changed", ["source", "empty_before", "inheritance"])
def test_original_restored_witnesses_reject_aba_empty_controls_and_global_inheritance(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, changed: str
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, after=False)
    app = _app(config)
    startup = _startup(app)
    if changed == "source":
        app.project.pad_content[0] = PadContentIdentity(instance_id="f" * 32)
    elif changed == "empty_before":
        app.project.pad_gain_db[70] = -3.0
    else:
        app.project.key_lock = False
    app.persistence.mark_dirty()
    before = config.read_bytes()
    startup.poll()
    assert startup.active
    assert startup.error
    assert app._audio.captures == []
    assert app._audio.participants == []
    assert app.persistence._migration_owner == _TRANSACTION
    assert config.read_bytes() == before
    assert all(not reader.released for reader in app._assets.readers)


def test_missing_original_custody_keeps_partial_readers_before_ordinary_restore(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, after=False)
    app = _app(config)
    path = Path(cast("str", app.project.sample_paths[0]))
    original = path.read_bytes()
    path.unlink()
    startup = _startup(app)
    assert startup.active
    assert startup.error
    assert not startup._custody.acquired
    assert app._assets.readers
    readers = app._assets.readers.copy()
    assert all(not reader.released for reader in readers)
    path.write_bytes(original)
    handoff = startup.poll()
    assert handoff is not None
    assert startup._custody.acquired
    assert all(reader in app._assets.readers for reader in readers)


@pytest.mark.parametrize("damage", ["early_kind", "later_partial", "source_seal"])
def test_all_ordinals_are_strict_before_any_native_reader_or_config_action(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, damage: str
) -> None:
    config, _ = _prior(tmp_path, monkeypatch)
    directory = config.parent / ".material-migrations" / _TRANSACTION
    if damage == "later_partial":
        (directory / "04.json").write_text("{", encoding="utf-8")
    else:
        leaf = directory / "00.json"
        data = json.loads(leaf.read_text(encoding="utf-8"))
        if damage == "early_kind":
            data["kind"] = "future_bank"
        else:
            bank = json.loads(data["bank"]["payload_json"])
            bank["slots"][0]["source"]["gain_db"] = -13.0
            data["bank"]["payload_json"] = json.dumps(bank)
        leaf.write_text(json.dumps(data), encoding="utf-8")
    history = {leaf.name: leaf.read_bytes() for leaf in directory.glob("*.json")}
    app = _app(config)
    original = config.read_bytes()
    startup = _startup(app)
    startup.poll()
    assert startup.active
    assert startup.error
    assert startup.fenced_sample_ids == frozenset(range(216))
    assert app._audio.captures == []
    assert app._audio.participants == []
    assert app._assets.readers == []
    assert config.read_bytes() == original
    assert {leaf.name: leaf.read_bytes() for leaf in directory.glob("*.json")} == history


@pytest.mark.parametrize("damage", ["missing_parent", "fork", "cycle", "two_roots"])
def test_ambiguous_lineage_preserves_every_history_and_acquires_no_reader(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, damage: str
) -> None:
    config, parent = _prior(tmp_path, monkeypatch, after=False)
    child = _child(config, parent)
    directory = config.parent / ".material-migrations"
    if damage == "missing_parent":
        (directory / child.transaction_id / "00.json").write_text(
            child.changed(resume_of="f" * 32).model_dump_json(), encoding="utf-8"
        )
    elif damage == "fork":
        _child(config, parent, child_id="c" * 32)
    elif damage == "two_roots":
        extra = _child(config, parent, child_id="c" * 32)
        (directory / extra.transaction_id / "00.json").write_text(
            extra.changed(resume_of=None).model_dump_json(), encoding="utf-8"
        )
    else:
        for leaf in (directory / parent.transaction_id).glob("*.json"):
            raw = json.loads(leaf.read_text(encoding="utf-8"))
            raw["resume_of"] = child.transaction_id
            leaf.write_text(json.dumps(raw), encoding="utf-8")
    evidence = {str(leaf): leaf.read_bytes() for leaf in directory.glob("*/*.json")}
    app = _app(config)
    startup = _startup(app)
    startup.poll()
    assert startup.active
    assert startup.error
    assert app._assets.readers == []
    assert app._audio.participants == []
    assert {str(leaf): leaf.read_bytes() for leaf in directory.glob("*/*.json")} == evidence


def test_terminal_descendant_verifies_fresh_targets_and_preserves_newer_dirty_source_intent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, parent = _prior(tmp_path, monkeypatch, after=False)
    child = _child(config, parent, terminal=True)
    app = _app(config)
    app.project.pad_gain_db[0] = -28.0
    app.project.volume = 0.35
    app.persistence.mark_dirty()
    original = config.read_bytes()
    startup = _startup(app)
    startup.poll()
    assert startup.active
    assert len(app._audio.captures) == 1
    app._audio.captures[0].status = "accepted"
    startup.poll()
    assert not startup.active
    assert app.project.pad_gain_db[0] == -28.0
    assert app.project.volume == 0.35
    assert config.read_bytes() == original
    assert app.persistence._dirty
    assert app.persistence._migration_owner is None
    assert not (config.parent / ".material-migrations" / parent.transaction_id).exists()
    assert BankJournalOwner.open(config.parent, child.transaction_id).journal == child
    assert app.persistence.flush_if_dirty()
    assert ProjectState.model_validate_json(config.read_bytes()).pad_gain_db[0] == -28.0


def test_compaction_retry_retains_cursor_after_one_exact_ancestor_was_removed(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, parent = _prior(tmp_path, monkeypatch, after=False)
    middle = _child(config, parent)
    terminal = _child(config, middle, child_id="c" * 32, terminal=True)
    app = _app(config)
    startup = _startup(app)
    startup.poll()
    acceptance = app._audio.captures[0]
    acceptance.status = "accepted"
    ancestor = startup.histories[1]
    real_store = ancestor.store
    failed = False

    class _Store:
        def history(self) -> list[str]:
            return real_store.history()

        def compact(self, expected: list[str]) -> None:
            nonlocal failed
            if not failed:
                failed = True
                message = "second ancestor exact compaction pressure"
                raise OSError(message)
            real_store.compact(expected)

    monkeypatch.setattr(ancestor, "store", _Store())
    startup.poll()
    assert startup.active
    assert startup._compacted == {parent.transaction_id}
    assert not (config.parent / ".material-migrations" / parent.transaction_id).exists()
    assert (config.parent / ".material-migrations" / middle.transaction_id).exists()
    assert app.persistence._migration_owner == terminal.transaction_id
    assert all(not reader.released for reader in app._assets.readers)
    startup.poll()
    assert not startup.active
    assert app._audio.captures == app._audio.releases == [acceptance]
    assert not (config.parent / ".material-migrations" / middle.transaction_id).exists()
    assert BankJournalOwner.open(config.parent, terminal.transaction_id).journal == terminal


def test_competing_newer_recorded_and_unsaved_outside_intent_is_preserved_and_fenced(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, committed = _prior(tmp_path, monkeypatch)
    owner = BankJournalOwner.open(config.parent, _TRANSACTION)
    recorded = ProjectState.model_validate_json(config.read_bytes())
    recorded.pad_gain_db[0] = -24.0
    recorded.config_revision += 1
    owner.append(
        committed.changed(
            phase="native_ack_observed",
            image_role="before",
            project_image_json=bank_before_project(recorded, committed).model_dump_json(),
            intent_revision=recorded.config_revision,
            committed_revision=None,
            committed_config_sha256=None,
        )
    )
    app = _app(config)
    app.project.pad_gain_db[0] = -30.0
    app.persistence.mark_dirty()
    before_project, before_config = app.project.model_dump_json(), config.read_bytes()
    startup = _startup(app)
    startup.poll()
    assert startup.active
    assert startup.error is not None
    assert "Competing unsaved" in startup.error
    assert app.project.model_dump_json() == before_project
    assert config.read_bytes() == before_config
    assert app._audio.participants == []
    assert app.persistence._migration_owner == _TRANSACTION
    assert all(not reader.released for reader in app._assets.readers)


def test_standalone_terminal_history_needs_no_new_native_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, _ = _prior(tmp_path, monkeypatch)
    owner = BankJournalOwner.open(config.parent, _TRANSACTION)
    assert owner.journal is not None
    owner.append(owner.journal.changed(phase="settled"))
    app = _app(config)
    startup = _startup(app)
    assert not startup.active
    assert app.persistence._migration_owner is None
    assert app._audio.participants == []
    assert app._assets.readers == []


@pytest.mark.parametrize("failure", ["stale", "config_return", "terminal_append", "release"])
def test_retry_retains_same_probe_writer_and_readers_through_failed_settlement(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, recorded=False)
    app = _app(config)
    startup = _startup(app)
    startup.poll()
    acceptance = app._audio.captures[0]
    acceptance.status = "accepted"
    if failure == "stale":
        app._audio.valid = False
    elif failure == "release":
        app._audio.release_pressure = 1
    elif failure == "config_return":
        write = app.persistence._atomic_write_text
        failed = False

        def write_then_signal(text: str) -> None:
            nonlocal failed
            write(text)
            if not failed:
                failed = True
                message = "lost actual Config replacement return"
                raise OSError(message)

        monkeypatch.setattr(app.persistence, "_atomic_write_text", write_then_signal)
    else:
        assert startup._after is not None
        owner = startup._after.owner
        append = owner.append
        failed = False

        def append_once(record: BankArrangementJournal) -> BankArrangementJournal:
            nonlocal failed
            if record.phase == "settled" and not failed:
                failed = True
                message = "terminal append pressure"
                raise OSError(message)
            return append(record)

        monkeypatch.setattr(owner, "append", append_once)
    startup.poll()
    assert startup.active
    assert app.persistence._migration_owner == _TRANSACTION
    assert all(not reader.released for reader in app._assets.readers)
    app._audio.valid = True
    startup.poll()
    assert not startup.active
    assert app._audio.captures == [acceptance]
    settled = BankJournalOwner.open(config.parent, _TRANSACTION).journal
    assert settled is not None
    assert settled.phase == "settled"


def test_current_target_and_inheritance_change_never_releases_fresh_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    config, _ = _prior(tmp_path, monkeypatch)
    app = _app(config)
    startup = _startup(app)
    startup.poll()
    app._audio.captures[0].status = "accepted"
    app.project.pad_gain_db[70] = -3.0
    app.persistence.mark_dirty()
    startup.poll()
    assert startup.active
    assert startup.error
    assert app._audio.releases == []
    assert app.persistence._migration_owner == _TRANSACTION
    with pytest.raises(PersistenceFenceError):
        startup.ensure_global_key_lock_edit_allowed()


def _persist_interrupted_bank(
    persistence: ProjectPersistence, tmp_path: Path
) -> BankArrangementJournal:
    revision, captured, digest = persistence.capture_migration(_TRANSACTION)
    parent = BankArrangementJournal(
        transaction_id=_TRANSACTION,
        config_reference=persistence.config_reference,
        captured_revision=revision,
        intent_revision=revision,
        captured_config_sha256=digest,
        bank=capture_bank_snapshot(captured, 0, 1),
        captured_project_image_json=captured.model_dump_json(),
        project_image_json=captured.model_dump_json(),
        image_role="before",
        phase="captured",
    )
    BankJournalOwner(tmp_path / "samples", parent).append(parent)
    persistence.release_migration(_TRANSACTION)
    return parent


def _assert_terminal_bank_receipt(config: Path, transaction: str) -> None:
    latest = BankJournalOwner.open(config.parent, transaction)
    assert validated_bank_history(latest.encoded, transaction)[-1].phase == "settled"
    assert latest.journal is not None
    assert hashlib.sha256(config.read_bytes()).hexdigest() == (
        latest.journal.committed_config_sha256
    )


def test_live_controller_adopts_startup_child_and_compacts_ancestor_before_writer_release(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    app = harness.app
    parent = _persist_interrupted_bank(app.persistence, tmp_path)
    harness.controller.bind_startup()
    startup = harness.controller._startup
    assert startup is not None
    assert startup.active
    assert app.persistence._migration_owner == _TRANSACTION
    harness.controller.poll()
    bank = harness.current()
    handoff = bank.recovery_handoff
    assert handoff is not None
    assert handoff.transferred
    assert bank.initial.bank == parent.bank
    assert bank.custody is startup._custody
    assert bank.retirement is startup._retirement
    assert bank.request.participants is handoff.participants
    ancestor = startup.histories[0]
    compact = ancestor.store.compact
    failed = False
    events: list[tuple[str, str | None]] = []

    def compact_once(expected: list[str]) -> None:
        nonlocal failed
        events.append(("compact", app.persistence._migration_owner))
        assert expected == ancestor.encoded
        if not failed:
            failed = True
            message = "exact ancestor compaction temporarily unavailable"
            raise OSError(message)
        compact(expected)

    # The immutable Native pyclass is wrapped at the retained owner boundary.
    class _Store:
        def history(self) -> list[str]:
            return ancestor.encoded.copy()

        def compact(self, expected: list[str]) -> None:
            compact_once(expected)

    monkeypatch.setattr(ancestor, "store", _Store())
    harness.run_worker()
    harness.controller.poll()
    assert harness.controller.active
    assert harness.current() is bank
    assert handoff.journal.journal is not None
    assert handoff.journal.journal.phase == "settled"
    assert app.persistence._migration_owner == handoff.initial.transaction_id
    assert not handoff.completed
    assert bank.custody.readers
    assert len(harness.native.bank_captures) == len(harness.native.bank_finishes) == 1
    harness.controller.poll()
    assert not harness.controller.active
    assert handoff.completed
    assert all(owner == handoff.initial.transaction_id for _, owner in events)
    assert app.persistence._migration_owner is None
    assert not (tmp_path / "samples" / ".material-migrations" / _TRANSACTION).exists()
    _assert_terminal_bank_receipt(app.persistence.config_path, handoff.initial.transaction_id)
